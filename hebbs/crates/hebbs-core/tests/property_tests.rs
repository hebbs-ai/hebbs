use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use proptest::prelude::*;

use hebbs_core::decay::compute_decay_score;
use hebbs_core::engine::{Engine, RememberInput};
use hebbs_core::forget::{ForgetCriteria, Tombstone};
use hebbs_core::keys;
use hebbs_core::memory::{Memory, MemoryKind};
use hebbs_core::recall::{RecallInput, RecallStrategy, ScoringWeights};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig, ReflectScope};
use hebbs_core::revise::ReviseInput;
use hebbs_core::subscribe::SubscribeConfig;
use hebbs_embed::MockEmbedder;
use hebbs_index::{EdgeType, HnswParams};
use hebbs_reflect::MockLlmProvider;
use hebbs_storage::InMemoryBackend;

/// Strategy to generate arbitrary valid Memory structs.
/// Split into two groups because proptest tuples max at 12 elements.
fn arb_memory() -> impl Strategy<Value = Memory> {
    let core_fields = (
        prop::collection::vec(any::<u8>(), 16..=16), // memory_id
        "[a-zA-Z0-9 ]{1,200}",                       // content
        0.0f32..=1.0f32,                             // importance
        prop::option::of("[a-zA-Z0-9_]{1,50}"),      // entity_id
        0u64..=u64::MAX / 2,                         // created_at
        0u64..=u64::MAX / 2,                         // updated_at
        0u64..=u64::MAX / 2,                         // last_accessed_at
        0u64..10_000u64,                             // access_count
        0.0f32..=1.0f32,                             // decay_score
    );

    let extra_fields = (
        prop_oneof![
            Just(MemoryKind::Episode),
            Just(MemoryKind::Insight),
            Just(MemoryKind::Revision),
        ],
        prop::option::of("[a-zA-Z0-9_]{1,30}"), // device_id
        0u64..10_000u64,                        // logical_clock
    );

    (core_fields, extra_fields).prop_map(
        |(
            (
                memory_id,
                content,
                importance,
                entity_id,
                created_at,
                updated_at,
                last_accessed_at,
                access_count,
                decay_score,
            ),
            (kind, device_id, logical_clock),
        )| {
            Memory {
                memory_id,
                content,
                importance,
                context_bytes: Vec::new(),
                entity_id,
                embedding: None,
                created_at,
                updated_at,
                last_accessed_at,
                access_count,
                decay_score,
                kind,
                device_id,
                logical_clock,
                associative_embedding: None,
            }
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// For any valid Memory, serialize then deserialize == original.
    #[test]
    fn serialization_roundtrip(mem in arb_memory()) {
        let bytes = mem.to_bytes();
        let restored = Memory::from_bytes(&bytes).expect("deserialization must succeed");
        prop_assert_eq!(&mem.memory_id, &restored.memory_id);
        prop_assert_eq!(&mem.content, &restored.content);
        prop_assert_eq!(mem.importance, restored.importance);
        prop_assert_eq!(&mem.context_bytes, &restored.context_bytes);
        prop_assert_eq!(&mem.entity_id, &restored.entity_id);
        prop_assert_eq!(&mem.embedding, &restored.embedding);
        prop_assert_eq!(mem.created_at, restored.created_at);
        prop_assert_eq!(mem.updated_at, restored.updated_at);
        prop_assert_eq!(mem.last_accessed_at, restored.last_accessed_at);
        prop_assert_eq!(mem.access_count, restored.access_count);
        prop_assert_eq!(mem.decay_score, restored.decay_score);
        prop_assert_eq!(mem.kind, restored.kind);
        prop_assert_eq!(&mem.device_id, &restored.device_id);
        prop_assert_eq!(mem.logical_clock, restored.logical_clock);
    }

    /// Memory with embedding survives round-trip.
    #[test]
    fn serialization_roundtrip_with_embedding(
        mem in arb_memory(),
        embedding in prop::collection::vec(-1.0f32..1.0f32, 384..=384),
    ) {
        let mut mem = mem;
        mem.embedding = Some(embedding);
        let bytes = mem.to_bytes();
        let restored = Memory::from_bytes(&bytes).unwrap();
        prop_assert_eq!(&mem.embedding, &restored.embedding);
    }

    /// Context JSON bytes survive serialization round-trip.
    #[test]
    fn context_roundtrip(
        key in "[a-zA-Z_]{1,20}",
        val in "[a-zA-Z0-9 ]{1,50}"
    ) {
        let mut ctx = HashMap::new();
        ctx.insert(key.clone(), serde_json::json!(val.clone()));
        let ctx_bytes = Memory::serialize_context(&ctx).unwrap();

        let mem = Memory {
            memory_id: vec![0u8; 16],
            content: "test".to_string(),
            importance: 0.5,
            context_bytes: ctx_bytes,
            entity_id: None,
            embedding: None,
            created_at: 0,
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 0,
            decay_score: 0.5,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };

        let bytes = mem.to_bytes();
        let restored = Memory::from_bytes(&bytes).unwrap();
        let restored_ctx = restored.context().unwrap();
        prop_assert_eq!(&restored_ctx[&key], &serde_json::json!(val));
    }

    /// ULID byte representations sort in the same order as timestamps.
    /// Timestamps are constrained to 48-bit range (ULID spec).
    #[test]
    fn ulid_byte_ordering(
        ts_a in 0u64..(1u64 << 48),
        ts_b in 0u64..(1u64 << 48),
    ) {
        let ulid_a = ulid::Ulid::from_parts(ts_a, 0);
        let ulid_b = ulid::Ulid::from_parts(ts_b, 0);
        let bytes_a = ulid_a.to_bytes();
        let bytes_b = ulid_b.to_bytes();

        if ts_a < ts_b {
            prop_assert!(bytes_a < bytes_b, "ULID byte ordering violated");
        } else if ts_a > ts_b {
            prop_assert!(bytes_a > bytes_b);
        } else {
            prop_assert_eq!(bytes_a, bytes_b);
        }
    }

    /// Temporal key encoding: same entity with different timestamps sorts chronologically.
    #[test]
    fn temporal_key_chronological_sort(
        entity in "[a-zA-Z_]{1,20}",
        ts_a in 0u64..u64::MAX / 2,
        ts_b in 0u64..u64::MAX / 2,
    ) {
        let key_a = keys::encode_temporal_key(&entity, ts_a);
        let key_b = keys::encode_temporal_key(&entity, ts_b);

        if ts_a < ts_b {
            prop_assert!(key_a < key_b);
        } else if ts_a > ts_b {
            prop_assert!(key_a > key_b);
        } else {
            prop_assert_eq!(key_a, key_b);
        }
    }

    /// Temporal keys for different entities never share prefixes.
    #[test]
    fn entity_prefix_isolation(
        entity_a in "[a-zA-Z]{1,10}",
        entity_b in "[a-zA-Z]{1,10}",
        ts in 0u64..1_000_000u64,
    ) {
        prop_assume!(entity_a != entity_b);
        let key_a = keys::encode_temporal_key(&entity_a, ts);
        let key_b = keys::encode_temporal_key(&entity_b, ts);
        let prefix_a = keys::encode_temporal_prefix(&entity_a);
        let prefix_b = keys::encode_temporal_prefix(&entity_b);

        prop_assert!(key_a.starts_with(&prefix_a));
        prop_assert!(!key_a.starts_with(&prefix_b));
        prop_assert!(key_b.starts_with(&prefix_b));
        prop_assert!(!key_b.starts_with(&prefix_a));
    }

    /// u64 big-endian encoding preserves numeric ordering.
    #[test]
    fn u64_be_ordering(a in any::<u64>(), b in any::<u64>()) {
        let bytes_a = keys::encode_u64_be(a);
        let bytes_b = keys::encode_u64_be(b);

        match a.cmp(&b) {
            std::cmp::Ordering::Less => prop_assert!(bytes_a.as_slice() < bytes_b.as_slice()),
            std::cmp::Ordering::Greater => prop_assert!(bytes_a.as_slice() > bytes_b.as_slice()),
            std::cmp::Ordering::Equal => prop_assert_eq!(bytes_a, bytes_b),
        }
    }

    /// u64 big-endian round-trip.
    #[test]
    fn u64_be_roundtrip(val in any::<u64>()) {
        let encoded = keys::encode_u64_be(val);
        let decoded = keys::decode_u64_be(&encoded);
        prop_assert_eq!(val, decoded);
    }
}

// Engine-level properties: remember + get round-trip, embedding invariants.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn engine_remember_get_roundtrip(
        content in "[a-zA-Z0-9 ]{1,200}",
        importance in 0.0f32..=1.0f32,
        entity_id in prop::option::of("[a-zA-Z0-9_]{1,20}"),
    ) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        let original = engine.remember(RememberInput {
            content: content.clone(),
            importance: Some(importance),
            context: None,
            entity_id: entity_id.clone(),
            edges: vec![],
            kind: None,
        }).unwrap();

        let retrieved = engine.get(&original.memory_id).unwrap();
        prop_assert_eq!(&original.content, &retrieved.content);
        prop_assert_eq!(original.importance, retrieved.importance);
        prop_assert_eq!(&original.entity_id, &retrieved.entity_id);
        prop_assert_eq!(&original.memory_id, &retrieved.memory_id);
    }

    /// Phase 2 contract: every remembered memory has a non-None embedding.
    #[test]
    fn remember_always_produces_embedding(
        content in "[a-zA-Z0-9 ]{1,200}",
    ) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        let memory = engine.remember(RememberInput {
            content,
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        }).unwrap();

        prop_assert!(memory.embedding.is_some(), "embedding must be Some after Phase 2");
        let emb = memory.embedding.as_ref().unwrap();
        prop_assert_eq!(emb.len(), 384, "embedding must have 384 dimensions");
    }

    /// Phase 2 invariant: embedding is L2-normalized (unit length).
    #[test]
    fn embedding_is_normalized(
        content in "[a-zA-Z0-9 ]{1,200}",
    ) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        let memory = engine.remember(RememberInput {
            content,
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        }).unwrap();

        let emb = memory.embedding.as_ref().unwrap();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        prop_assert!((norm - 1.0).abs() < 1e-5, "embedding norm {} is not 1.0", norm);
    }

    /// Phase 2 invariant: same content produces identical embeddings.
    #[test]
    fn same_content_same_embedding(
        content in "[a-zA-Z0-9 ]{1,100}",
    ) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        let m1 = engine.remember(RememberInput {
            content: content.clone(),
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        }).unwrap();

        let m2 = engine.remember(RememberInput {
            content,
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        }).unwrap();

        prop_assert_eq!(&m1.embedding, &m2.embedding,
            "identical content must produce identical embeddings");
    }

    /// Phase 2 invariant: embedding survives storage round-trip.
    #[test]
    fn embedding_survives_roundtrip(
        content in "[a-zA-Z0-9 ]{1,200}",
    ) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        let original = engine.remember(RememberInput {
            content,
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        }).unwrap();

        let retrieved = engine.get(&original.memory_id).unwrap();
        prop_assert_eq!(&original.embedding, &retrieved.embedding,
            "embedding must survive storage round-trip");
    }
}

// Phase 4: Recall engine property tests.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// For any set of memories, recall deduplicates: no two results share the same memory_id.
    #[test]
    fn recall_deduplication(n in 5usize..30) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 16);
        let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

        for i in 0..n {
            engine.remember(RememberInput {
                content: format!("property test memory {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("prop_entity".to_string()),
                edges: vec![],
                kind: None,
            }).unwrap();
        }

        let mut input = RecallInput::multi(
            "property test memory",
            vec![RecallStrategy::Similarity, RecallStrategy::Temporal],
        );
        input.entity_id = Some("prop_entity".to_string());
        input.top_k = Some(n);
        let output = engine.recall(input).unwrap();

        let ids: Vec<&Vec<u8>> = output.results.iter().map(|r| &r.memory.memory_id).collect();
        let unique: HashSet<&Vec<u8>> = ids.iter().cloned().collect();
        prop_assert_eq!(ids.len(), unique.len(), "deduplication violated: found duplicate memory_ids");
    }

    /// Composite score is always non-negative and bounded.
    #[test]
    fn composite_score_bounded(n in 3usize..15) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 16);
        let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

        for i in 0..n {
            engine.remember(RememberInput {
                content: format!("bounded score memory {}", i),
                importance: Some((i as f32 / n as f32).clamp(0.0, 1.0)),
                context: None,
                entity_id: Some("bound_entity".to_string()),
                edges: vec![],
                kind: None,
            }).unwrap();
        }

        let mut input = RecallInput::new("bounded score memory", RecallStrategy::Similarity);
        input.top_k = Some(n);
        let output = engine.recall(input).unwrap();

        let weights = ScoringWeights::default();
        let max_score = weights.w_relevance + weights.w_recency + weights.w_importance + weights.w_reinforcement;

        for result in &output.results {
            prop_assert!(result.score >= 0.0,
                "score {} is negative", result.score);
            prop_assert!(result.score <= max_score + 0.01,
                "score {} exceeds max {}", result.score, max_score);
        }
    }

    /// Reinforcement: access_count after n recalls is >= n.
    #[test]
    fn reinforcement_monotonic(recalls in 1usize..6) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 16);
        let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

        let mem = engine.remember(RememberInput {
            content: "reinforcement monotonic test".to_string(),
            importance: Some(0.8),
            context: None,
            entity_id: Some("reinforce".to_string()),
            edges: vec![],
            kind: None,
        }).unwrap();

        for _ in 0..recalls {
            engine.recall(RecallInput::new(
                "reinforcement monotonic test", RecallStrategy::Similarity,
            )).unwrap();
        }

        let retrieved = engine.get(&mem.memory_id).unwrap();
        prop_assert!(retrieved.access_count >= recalls as u64,
            "access_count {} should be >= {} after {} recalls",
            retrieved.access_count, recalls, recalls);
    }

    /// Multi-strategy results are a subset of the union of individual results.
    #[test]
    fn multi_strategy_subset_of_union(n in 10usize..25) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 16);
        let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

        for i in 0..n {
            engine.remember(RememberInput {
                content: format!("union test content {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("union_entity".to_string()),
                edges: vec![],
                kind: None,
            }).unwrap();
        }

        let k = n.min(10);

        // Individual strategies
        let mut sim_input = RecallInput::new("union test content", RecallStrategy::Similarity);
        sim_input.top_k = Some(k);
        let sim_ids: HashSet<Vec<u8>> = engine.recall(sim_input).unwrap()
            .results.iter().map(|r| r.memory.memory_id.clone()).collect();

        let mut temp_input = RecallInput::new("union test", RecallStrategy::Temporal);
        temp_input.entity_id = Some("union_entity".to_string());
        temp_input.top_k = Some(k);
        let temp_ids: HashSet<Vec<u8>> = engine.recall(temp_input).unwrap()
            .results.iter().map(|r| r.memory.memory_id.clone()).collect();

        let union: HashSet<Vec<u8>> = sim_ids.union(&temp_ids).cloned().collect();

        // Multi-strategy
        let mut multi_input = RecallInput::multi(
            "union test content",
            vec![RecallStrategy::Similarity, RecallStrategy::Temporal],
        );
        multi_input.entity_id = Some("union_entity".to_string());
        multi_input.top_k = Some(n);
        let multi_ids: HashSet<Vec<u8>> = engine.recall(multi_input).unwrap()
            .results.iter().map(|r| r.memory.memory_id.clone()).collect();

        // Multi-strategy should be a superset of union (dedup only removes dups, not unique results)
        // Actually multi should contain all from both, given generous top_k
        for id in &union {
            prop_assert!(multi_ids.contains(id),
                "multi-strategy result missing a memory found by individual strategy");
        }
    }
}

// Phase 5: Tombstone serialization round-trip.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn tombstone_roundtrip(
        memory_id in prop::collection::vec(any::<u8>(), 16..=16),
        entity_id in prop::option::of("[a-zA-Z0-9_]{1,50}"),
        timestamp in 0u64..=u64::MAX / 2,
        description in "[a-zA-Z0-9 ]{1,100}",
        cascade in 0u32..100,
        hash in prop::collection::vec(any::<u8>(), 32..=32),
    ) {
        let tombstone = Tombstone {
            memory_id: memory_id.clone(),
            entity_id: entity_id.clone(),
            forget_timestamp_us: timestamp,
            criteria_description: description.clone(),
            cascade_count: cascade,
            content_hash: hash.clone(),
        };
        let bytes = tombstone.to_bytes();
        let restored = Tombstone::from_bytes(&bytes).unwrap();
        prop_assert_eq!(&restored.memory_id, &memory_id);
        prop_assert_eq!(&restored.entity_id, &entity_id);
        prop_assert_eq!(restored.forget_timestamp_us, timestamp);
        prop_assert_eq!(&restored.criteria_description, &description);
        prop_assert_eq!(restored.cascade_count, cascade);
        prop_assert_eq!(&restored.content_hash, &hash);
    }
}

// Phase 5: Decay score properties.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn decay_score_bounded(
        importance in 0.0f32..=1.0f32,
        access_count in 0u64..1_000,
        age_seconds in 0u64..365 * 24 * 3600,
    ) {
        let half_life = 30 * 24 * 3600 * 1_000_000u64;
        let now = 1_000_000_000_000_000u64;
        let last_accessed = now.saturating_sub(age_seconds * 1_000_000);
        let score = compute_decay_score(importance, last_accessed, access_count, now, half_life, 100);
        prop_assert!(score >= 0.0, "decay score must be non-negative, got {}", score);
        prop_assert!(score <= importance * 2.0 + 0.001, "decay score {} exceeds 2*importance {}", score, importance * 2.0);
    }

    #[test]
    fn decay_score_importance_monotonic(
        imp_a in 0.01f32..=0.49f32,
        imp_b in 0.51f32..=1.0f32,
        access_count in 0u64..100,
        age_seconds in 0u64..30 * 24 * 3600,
    ) {
        let half_life = 30 * 24 * 3600 * 1_000_000u64;
        let now = 1_000_000_000_000_000u64;
        let last_accessed = now.saturating_sub(age_seconds * 1_000_000);
        let score_a = compute_decay_score(imp_a, last_accessed, access_count, now, half_life, 100);
        let score_b = compute_decay_score(imp_b, last_accessed, access_count, now, half_life, 100);
        prop_assert!(score_b >= score_a, "higher importance should yield >= score: {} vs {}", score_a, score_b);
    }

    #[test]
    fn decay_score_recency_monotonic(
        importance in 0.1f32..=1.0f32,
        access_count in 1u64..50,
        old_age_days in 30u64..365,
        recent_age_days in 0u64..29,
    ) {
        let half_life = 30 * 24 * 3600 * 1_000_000u64;
        let now = 1_000_000_000_000_000u64;
        let old_accessed = now.saturating_sub(old_age_days * 24 * 3600 * 1_000_000);
        let recent_accessed = now.saturating_sub(recent_age_days * 24 * 3600 * 1_000_000);
        let score_old = compute_decay_score(importance, old_accessed, access_count, now, half_life, 100);
        let score_recent = compute_decay_score(importance, recent_accessed, access_count, now, half_life, 100);
        prop_assert!(score_recent >= score_old, "more recent access should yield >= score: {} vs {}", score_old, score_recent);
    }

    #[test]
    fn decay_score_reinforcement_monotonic(
        importance in 0.1f32..=1.0f32,
        low_access in 0u64..50,
        extra_access in 1u64..50,
        age_seconds in 0u64..30 * 24 * 3600,
    ) {
        let half_life = 30 * 24 * 3600 * 1_000_000u64;
        let now = 1_000_000_000_000_000u64;
        let last_accessed = now.saturating_sub(age_seconds * 1_000_000);
        let high_access = low_access + extra_access;
        let score_low = compute_decay_score(importance, last_accessed, low_access, now, half_life, 100);
        let score_high = compute_decay_score(importance, last_accessed, high_access, now, half_life, 100);
        prop_assert!(score_high >= score_low, "higher access_count should yield >= score: {} vs {}", score_low, score_high);
    }

    #[test]
    fn decay_score_zero_importance_always_zero(
        access_count in 0u64..1_000,
        age_seconds in 0u64..365 * 24 * 3600,
    ) {
        let half_life = 30 * 24 * 3600 * 1_000_000u64;
        let now = 1_000_000_000_000_000u64;
        let last_accessed = now.saturating_sub(age_seconds * 1_000_000);
        let score = compute_decay_score(0.0, last_accessed, access_count, now, half_life, 100);
        prop_assert_eq!(score, 0.0, "zero importance must yield zero score");
    }
}

// Phase 5: Revise and forget engine properties.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn revise_preserves_id(
        original_content in "[a-zA-Z0-9 ]{10,100}",
        new_content in "[a-zA-Z0-9 ]{10,100}",
    ) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 16);
        let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

        let original = engine.remember(RememberInput {
            content: original_content,
            importance: Some(0.5),
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        }).unwrap();

        let revised = engine.revise(ReviseInput::new_content(
            original.memory_id.clone(),
            new_content,
        )).unwrap();

        prop_assert_eq!(&revised.memory_id, &original.memory_id, "revise must preserve memory_id");
    }

    #[test]
    fn revise_get_roundtrip(
        original_content in "[a-zA-Z0-9 ]{10,100}",
        new_content in "[a-zA-Z0-9 ]{10,100}",
    ) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 16);
        let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

        let original = engine.remember(RememberInput {
            content: original_content,
            importance: Some(0.5),
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        }).unwrap();

        engine.revise(ReviseInput::new_content(
            original.memory_id.clone(),
            new_content.clone(),
        )).unwrap();

        let retrieved = engine.get(&original.memory_id).unwrap();
        prop_assert_eq!(&retrieved.content, &new_content, "get after revise must return new content");
    }

    #[test]
    fn forget_by_id_reduces_count(
        n in 2usize..20,
    ) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 16);
        let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

        let mut ids = Vec::new();
        for i in 0..n {
            let mem = engine.remember(RememberInput {
                content: format!("forget prop test {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: None,
                edges: vec![],
                kind: None,
            }).unwrap();
            ids.push(mem.memory_id);
        }

        let before = engine.count().unwrap();
        prop_assert_eq!(before, n);

        let output = engine.forget(ForgetCriteria::by_ids(vec![ids[0].clone()])).unwrap();
        prop_assert_eq!(output.forgotten_count, 1);

        let after = engine.count().unwrap();
        prop_assert_eq!(after, n - 1);
    }

    #[test]
    fn forget_by_entity_count(
        entity_count in 3usize..15,
        other_count in 1usize..10,
    ) {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 16);
        let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

        for i in 0..entity_count {
            engine.remember(RememberInput {
                content: format!("target entity memory {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("target_entity".to_string()),
                edges: vec![],
                kind: None,
            }).unwrap();
        }

        for i in 0..other_count {
            engine.remember(RememberInput {
                content: format!("other entity memory {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("other_entity".to_string()),
                edges: vec![],
                kind: None,
            }).unwrap();
        }

        let output = engine.forget(ForgetCriteria::by_entity("target_entity")).unwrap();
        prop_assert_eq!(output.forgotten_count, entity_count, "should forget exactly entity_count memories");

        let remaining = engine.count().unwrap();
        prop_assert_eq!(remaining, other_count, "should have other_count remaining");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Phase 6: Subscribe property tests
// ═══════════════════════════════════════════════════════════════════════════

fn prop_engine() -> Engine {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    Engine::new_with_params(backend, embedder, params, 42).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// Every pushed memory must have confidence >= the configured threshold.
    /// MockEmbedder is hash-based: same text → identical embedding → distance 0 → confidence 1.0.
    #[test]
    fn subscribe_pushed_confidence_above_threshold(
        threshold_idx in 0usize..5,
    ) {
        let thresholds = [0.0f32, 0.2, 0.4, 0.6, 0.8];
        let threshold = thresholds[threshold_idx];

        let engine = prop_engine();
        let content = "budget pricing negotiation objection handling";

        for i in 0..20 {
            engine.remember(RememberInput {
                content: format!("{} memory{}", content, i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("prop_entity".to_string()),
                edges: vec![],
                kind: None,
            }).unwrap();
        }

        let config = SubscribeConfig {
            entity_id: Some("prop_entity".to_string()),
            confidence_threshold: threshold,
            coarse_threshold: 0.0,
            chunk_min_tokens: 3,
            chunk_max_wait_us: 50_000,
            ..Default::default()
        }.validated();

        let mut handle = engine.subscribe(config).unwrap();

        for i in 0..20 {
            handle.feed(format!("{} memory{}", content, i)).unwrap();
        }

        std::thread::sleep(Duration::from_secs(2));

        let mut pushes = Vec::new();
        while let Some(push) = handle.try_recv() {
            pushes.push(push);
        }

        for push in &pushes {
            prop_assert!(
                push.confidence >= threshold,
                "confidence {} below threshold {}",
                push.confidence,
                threshold
            );
        }

        handle.close();
    }

    /// Deduplication: feeding the same content multiple times must not produce
    /// duplicate memory IDs in the push stream.
    #[test]
    fn subscribe_no_duplicate_memory_ids(seed in 0u64..1000) {
        let _ = seed;
        let engine = prop_engine();
        let content = "budget pricing negotiation objection handling";

        for i in 0..10 {
            engine.remember(RememberInput {
                content: format!("{} item{}", content, i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("dedup_entity".to_string()),
                edges: vec![],
                kind: None,
            }).unwrap();
        }

        let config = SubscribeConfig {
            entity_id: Some("dedup_entity".to_string()),
            confidence_threshold: 0.0,
            coarse_threshold: 0.0,
            chunk_min_tokens: 3,
            chunk_max_wait_us: 50_000,
            ..Default::default()
        }.validated();

        let mut handle = engine.subscribe(config).unwrap();

        for repeat in 0..5 {
            for i in 0..10 {
                handle.feed(format!("{} item{}", content, i)).unwrap();
            }
            if repeat < 4 {
                std::thread::sleep(Duration::from_millis(200));
            }
        }

        std::thread::sleep(Duration::from_secs(2));

        let mut seen_ids: HashSet<Vec<u8>> = HashSet::new();
        while let Some(push) = handle.try_recv() {
            let id = push.memory.memory_id.clone();
            prop_assert!(
                seen_ids.insert(id.clone()),
                "duplicate memory_id {:?} in push stream",
                id
            );
        }

        handle.close();
    }

    /// The output queue must never exceed the configured depth.
    /// Stats track pushes_dropped which increments when the ring buffer evicts.
    #[test]
    fn subscribe_output_bounded(seed in 0u64..1000) {
        let _ = seed;
        let engine = prop_engine();
        let content = "budget pricing negotiation objection handling";

        for i in 0..5 {
            engine.remember(RememberInput {
                content: format!("{} bounded{}", content, i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("bounded_entity".to_string()),
                edges: vec![],
                kind: None,
            }).unwrap();
        }

        let queue_depth: usize = 10;
        let config = SubscribeConfig {
            entity_id: Some("bounded_entity".to_string()),
            confidence_threshold: 0.0,
            coarse_threshold: 0.0,
            chunk_min_tokens: 3,
            chunk_max_wait_us: 50_000,
            output_queue_depth: queue_depth,
            ..Default::default()
        }.validated();

        let mut handle = engine.subscribe(config).unwrap();

        handle.reset_dedup();
        for round in 0..10 {
            for i in 0..5 {
                handle.feed(format!("{} bounded{}", content, i)).unwrap();
            }
            if round < 9 {
                std::thread::sleep(Duration::from_millis(100));
            }
            handle.reset_dedup();
        }

        std::thread::sleep(Duration::from_secs(2));

        let stats = handle.stats();
        // pushes_sent counts everything pushed into the ring buffer.
        // The ring buffer capacity is queue_depth, so at any snapshot
        // the number of items in the buffer cannot exceed queue_depth.
        // We can't directly read buffer len, but we can verify that
        // sent - dropped (the items that survived in the buffer) <= capacity.
        let surviving = stats.pushes_sent.saturating_sub(stats.pushes_dropped);
        prop_assert!(
            surviving <= queue_depth as u64,
            "surviving pushes {} exceeds queue depth {} (sent={}, dropped={})",
            surviving,
            queue_depth,
            stats.pushes_sent,
            stats.pushes_dropped,
        );

        // Drain to double-check
        let mut drained = 0u64;
        while handle.try_recv().is_some() {
            drained += 1;
        }
        prop_assert!(
            drained <= queue_depth as u64,
            "drained {} exceeds queue depth {}",
            drained,
            queue_depth,
        );

        handle.close();
    }

    /// Bloom filter no-false-negatives (tested indirectly through the full pipeline):
    /// memories whose keywords exactly match the fed text must produce pushes.
    /// MockEmbedder guarantees same text → same embedding → confidence 1.0,
    /// so any pipeline rejection would indicate a bloom false negative.
    #[test]
    fn subscribe_bloom_no_false_negatives(seed in 0u64..1000) {
        let _ = seed;
        let engine = prop_engine();

        let keywords = [
            "algorithm", "database", "encryption", "fibonacci", "gradient",
        ];

        for (i, kw) in keywords.iter().enumerate() {
            engine.remember(RememberInput {
                content: format!("{} context term{}", kw, i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("bloom_entity".to_string()),
                edges: vec![],
                kind: None,
            }).unwrap();
        }

        let config = SubscribeConfig {
            entity_id: Some("bloom_entity".to_string()),
            confidence_threshold: 0.0,
            coarse_threshold: 0.0,
            chunk_min_tokens: 3,
            chunk_max_wait_us: 50_000,
            ..Default::default()
        }.validated();

        let mut handle = engine.subscribe(config).unwrap();

        for (i, kw) in keywords.iter().enumerate() {
            handle.feed(format!("{} context term{}", kw, i)).unwrap();
        }

        std::thread::sleep(Duration::from_secs(2));

        let mut pushed_ids: HashSet<Vec<u8>> = HashSet::new();
        while let Some(push) = handle.try_recv() {
            pushed_ids.insert(push.memory.memory_id.clone());
        }

        prop_assert!(
            pushed_ids.len() == keywords.len(),
            "expected {} pushes (one per keyword memory), got {}; bloom may have false negatives",
            keywords.len(),
            pushed_ids.len(),
        );

        handle.close();
    }
}

// ─── Phase 7: Reflection Pipeline Property Tests ─────────────────

fn reflect_engine() -> Engine {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    Engine::new_with_params(backend, embedder, params, 42).unwrap()
}

fn populate_n(engine: &Engine, n: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| {
            engine
                .remember(RememberInput {
                    content: format!("test memory about topic {} detail {}", i % 7, i),
                    importance: Some(0.3 + (i % 7) as f32 * 0.1),
                    context: None,
                    entity_id: Some("prop_entity".into()),
                    edges: vec![],
                    kind: None,
                })
                .unwrap()
                .memory_id
        })
        .collect()
}

proptest! {
    /// Every insight produced by reflect() has valid importance in [0, 1].
    #[test]
    fn reflect_insights_have_valid_importance(count in 15usize..=60) {
        let engine = reflect_engine();
        populate_n(&engine, count);

        let mock = MockLlmProvider::new();
        let config = ReflectConfig {
            min_memories_for_reflect: 5,
            min_cluster_size: 3,
            ..Default::default()
        };

        let output = engine.reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        ).unwrap();

        if output.insights_created > 0 {
            let insights = engine.insights(InsightsFilter::default()).unwrap();
            for insight in &insights {
                prop_assert!(
                    insight.importance >= 0.0 && insight.importance <= 1.0,
                    "importance out of range: {}", insight.importance
                );
            }
        }
    }

    /// Every insight has kind == Insight.
    #[test]
    fn reflect_insights_have_correct_kind(count in 15usize..=50) {
        let engine = reflect_engine();
        populate_n(&engine, count);

        let mock = MockLlmProvider::new();
        let config = ReflectConfig {
            min_memories_for_reflect: 5,
            min_cluster_size: 3,
            ..Default::default()
        };

        engine.reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        ).unwrap();

        let insights = engine.insights(InsightsFilter::default()).unwrap();
        for insight in &insights {
            prop_assert_eq!(insight.kind, MemoryKind::Insight);
        }
    }

    /// Every insight has non-empty content.
    #[test]
    fn reflect_insights_have_content(count in 15usize..=50) {
        let engine = reflect_engine();
        populate_n(&engine, count);

        let mock = MockLlmProvider::new();
        let config = ReflectConfig {
            min_memories_for_reflect: 5,
            min_cluster_size: 3,
            ..Default::default()
        };

        engine.reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        ).unwrap();

        let insights = engine.insights(InsightsFilter::default()).unwrap();
        for insight in &insights {
            prop_assert!(!insight.content.is_empty(), "insight content must not be empty");
        }
    }

    /// Every insight has InsightFrom outgoing edges.
    #[test]
    fn reflect_insights_have_lineage(count in 15usize..=40) {
        let engine = reflect_engine();
        populate_n(&engine, count);

        let mock = MockLlmProvider::new();
        let config = ReflectConfig {
            min_memories_for_reflect: 5,
            min_cluster_size: 3,
            ..Default::default()
        };

        let output = engine.reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        ).unwrap();

        if output.insights_created > 0 {
            let insights = engine.insights(InsightsFilter::default()).unwrap();
            for insight in &insights {
                let mut id = [0u8; 16];
                id.copy_from_slice(&insight.memory_id);
                let edges = engine.outgoing_edges(&id).unwrap_or_default();
                let insight_edges: Vec<_> = edges
                    .iter()
                    .filter(|(et, _, _)| *et == EdgeType::InsightFrom)
                    .collect();
                prop_assert!(
                    !insight_edges.is_empty(),
                    "insight must have InsightFrom edges"
                );
            }
        }
    }

    /// Total memory count increases after reflect (insights are stored).
    #[test]
    fn reflect_increases_memory_count(count in 20usize..=50) {
        let engine = reflect_engine();
        populate_n(&engine, count);

        let before = engine.count().unwrap();

        let mock = MockLlmProvider::new();
        let config = ReflectConfig {
            min_memories_for_reflect: 5,
            min_cluster_size: 3,
            ..Default::default()
        };

        let output = engine.reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        ).unwrap();

        let after = engine.count().unwrap();
        prop_assert!(
            after >= before + output.insights_created,
            "count should increase by at least insights_created: before={}, after={}, created={}",
            before, after, output.insights_created
        );
    }
}
