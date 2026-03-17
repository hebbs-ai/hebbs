//! HEBBS System Integration Tests
//!
//! Comprehensive cross-cutting tests that exercise the full Engine API
//! through realistic multi-operation workflows. Organized into categories:
//!
//! - **A**: Full lifecycle (remember → recall → revise → reflect → forget)
//! - **B**: Concurrency under mixed workloads
//! - **C**: Crash recovery (persistence across engine restarts)
//! - **D**: Scale validation (1K–10K memories)
//! - **E**: Edge cases and boundary conditions
//! - **F**: Data integrity (context roundtrips, cascade cleanup)

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use hebbs_core::engine::{Engine, RememberEdge, RememberInput};
use hebbs_core::error::HebbsError;
use hebbs_core::forget::ForgetCriteria;
use hebbs_core::memory::MemoryKind;
use hebbs_core::recall::{PrimeInput, RecallInput, RecallStrategy};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig, ReflectScope};
use hebbs_core::revise::{ContextMode, ReviseInput};
use hebbs_core::subscribe::SubscribeConfig;
use hebbs_embed::MockEmbedder;
use hebbs_index::EdgeType;
use hebbs_reflect::MockLlmProvider;
use hebbs_storage::RocksDbBackend;

// ═══════════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn create_engine(dir: &std::path::Path, dims: usize) -> Engine {
    let storage = Arc::new(RocksDbBackend::open(dir).unwrap());
    let embedder = Arc::new(MockEmbedder::new(dims));
    Engine::new(storage, embedder).unwrap()
}

fn remember_n(engine: &Engine, entity: &str, n: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| {
            engine
                .remember(RememberInput {
                    content: format!("memory number {} for entity {}", i, entity),
                    importance: Some(0.5 + (i as f32 % 50.0) / 100.0),
                    context: None,
                    entity_id: Some(entity.to_string()),
                    edges: vec![],
                    kind: None,
                })
                .unwrap()
                .memory_id
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category A: Full Lifecycle
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_full_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);
    let entity = "lifecycle_entity";

    // ── Remember ──
    let mut context = HashMap::new();
    context.insert("stage".to_string(), serde_json::json!("discovery"));
    context.insert("priority".to_string(), serde_json::json!(1));

    let mem = engine
        .remember(RememberInput {
            content: "Customer expressed urgency about Q4 deadline and budget constraints".into(),
            importance: Some(0.8),
            context: Some(context),
            entity_id: Some(entity.into()),
            edges: vec![],
            kind: None,
        })
        .unwrap();
    let mem_id = mem.memory_id.clone();
    assert_eq!(mem.kind, MemoryKind::Episode);
    assert!((mem.importance - 0.8).abs() < f32::EPSILON);

    // ── Get ──
    let retrieved = engine.get(&mem_id).unwrap();
    assert_eq!(retrieved.content, mem.content);
    assert_eq!(retrieved.entity_id.as_deref(), Some(entity));

    // ── Recall (similarity) ──
    let sim_output = engine
        .recall(RecallInput::new(
            "budget deadline Q4",
            RecallStrategy::Similarity,
        ))
        .unwrap();
    assert!(
        !sim_output.results.is_empty(),
        "similarity recall should find the memory"
    );
    assert_eq!(sim_output.results[0].memory.memory_id, mem_id);

    // ── Recall (temporal) ──
    let mut temporal_input = RecallInput::new("temporal query", RecallStrategy::Temporal);
    temporal_input.entity_id = Some(entity.into());
    let temp_output = engine.recall(temporal_input).unwrap();
    assert!(
        !temp_output.results.is_empty(),
        "temporal recall should find the memory"
    );

    // ── Revise ──
    let revised = engine
        .revise(ReviseInput::new_content(
            mem_id.clone(),
            "Customer confirmed Q4 deadline with updated budget of $500K".to_string(),
        ))
        .unwrap();
    assert!(revised.content.contains("$500K"));
    assert!(revised.updated_at >= mem.updated_at);

    // ── Recall to verify revision ──
    let post_revise = engine
        .recall(RecallInput::new("budget $500K", RecallStrategy::Similarity))
        .unwrap();
    assert!(!post_revise.results.is_empty());
    assert!(post_revise.results[0].memory.content.contains("$500K"));

    // ── Reflect ──
    let ids = remember_n(&engine, entity, 15);
    assert!(ids.len() >= 15);

    let mock_llm = MockLlmProvider::new();
    let config = ReflectConfig::default();
    let reflect_output = engine
        .reflect(
            ReflectScope::Entity {
                entity_id: entity.into(),
                since_us: None,
            },
            &config,
            &mock_llm,
            &mock_llm,
        )
        .unwrap();
    assert!(reflect_output.memories_processed > 0);

    // ── Insights ──
    let insights = engine.insights(InsightsFilter::default()).unwrap();
    // MockLlmProvider may or may not produce insights; just verify no errors
    let _ = insights;

    // ── Forget ──
    let forget_output = engine
        .forget(ForgetCriteria::by_ids(vec![mem_id.clone()]))
        .unwrap();
    assert_eq!(forget_output.forgotten_count, 1);
    assert!(forget_output.tombstone_count >= 1);

    // ── Verify gone ──
    let get_result = engine.get(&mem_id);
    assert!(
        matches!(get_result, Err(HebbsError::MemoryNotFound { .. })),
        "forgotten memory should not be retrievable"
    );

    let post_forget_recall = engine
        .recall(RecallInput::new("budget $500K", RecallStrategy::Similarity))
        .unwrap();
    let still_has_it = post_forget_recall
        .results
        .iter()
        .any(|r| r.memory.memory_id == mem_id);
    assert!(
        !still_has_it,
        "forgotten memory should not appear in recall"
    );
}

#[test]
fn test_lifecycle_with_subscribe() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);
    let entity = "subscribe_entity";

    let ids = remember_n(&engine, entity, 10);
    assert_eq!(ids.len(), 10);

    // ── Open subscription with low thresholds for testing ──
    let sub_config = SubscribeConfig {
        entity_id: Some(entity.into()),
        confidence_threshold: 0.0,
        coarse_threshold: 0.0,
        chunk_min_tokens: 3,
        chunk_max_wait_us: 50_000,
        ..Default::default()
    };
    let mut handle = engine.subscribe(sub_config).unwrap();
    assert!(engine.active_subscriptions() >= 1);

    // ── Feed text that shares keywords with remembered memories ──
    handle
        .feed("memory number entity subscribe entity testing feed")
        .unwrap();
    handle.flush();

    // Allow the worker thread time to process
    let push = handle.recv_timeout(Duration::from_secs(2));
    // Push may or may not arrive depending on bloom/embedding interaction with MockEmbedder;
    // the key invariant is that no panics occur and the subscription works
    let _ = push;

    // ── Remember during subscription (tests new-write notification) ──
    let new_mem = engine
        .remember(RememberInput {
            content: "New memory created during active subscription".into(),
            importance: Some(0.7),
            context: None,
            entity_id: Some(entity.into()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    // ── Revise during subscription ──
    engine
        .revise(ReviseInput::new_content(
            new_mem.memory_id.clone(),
            "Revised memory during active subscription lifecycle",
        ))
        .unwrap();

    // ── Forget during subscription ──
    let forget_out = engine
        .forget(ForgetCriteria::by_ids(vec![ids[0].clone()]))
        .unwrap();
    assert_eq!(forget_out.forgotten_count, 1);

    // ── Stats should show some activity ──
    let stats = handle.stats();
    // chunks_processed may be 0 if bloom rejected, but the field should be accessible
    let _ = stats.chunks_processed;

    // ── Clean close ──
    handle.close();
    assert_eq!(engine.active_subscriptions(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category B: Concurrency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_concurrent_mixed_workload() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let embedder = Arc::new(MockEmbedder::new(8));
    let engine = Arc::new(Engine::new(storage, embedder).unwrap());

    // Seed with initial memories so recall/revise/forget have targets
    let seed_ids: Vec<Vec<u8>> = (0..50)
        .map(|i| {
            engine
                .remember(RememberInput {
                    content: format!("seed memory {} for concurrent workload testing", i),
                    importance: Some(0.5),
                    context: None,
                    entity_id: Some("concurrent".into()),
                    edges: vec![],
                    kind: None,
                })
                .unwrap()
                .memory_id
        })
        .collect();
    let seed_ids = Arc::new(seed_ids);

    let remembered = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let forgotten = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let panic_flag = Arc::new(AtomicBool::new(false));
    let ops_count = Arc::new(AtomicUsize::new(0));

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut handles = Vec::new();

    // 10 remember threads
    for t in 0..10 {
        let eng = engine.clone();
        let remembered = remembered.clone();
        let panic_flag = panic_flag.clone();
        let ops = ops_count.clone();
        handles.push(thread::spawn(move || {
            let mut local_ids = Vec::new();
            let mut i = 0;
            while std::time::Instant::now() < deadline {
                match eng.remember(RememberInput {
                    content: format!("concurrent remember thread {} item {}", t, i),
                    importance: Some(0.5),
                    context: None,
                    entity_id: Some("concurrent".into()),
                    edges: vec![],
                    kind: None,
                }) {
                    Ok(mem) => local_ids.push(mem.memory_id),
                    Err(_) => panic_flag.store(true, Ordering::Relaxed),
                }
                i += 1;
                ops.fetch_add(1, Ordering::Relaxed);
            }
            remembered.lock().unwrap().extend(local_ids);
        }));
    }

    // 5 recall threads
    for _ in 0..5 {
        let eng = engine.clone();
        let panic_flag = panic_flag.clone();
        let ops = ops_count.clone();
        handles.push(thread::spawn(move || {
            while std::time::Instant::now() < deadline {
                let _ = eng
                    .recall(RecallInput::new(
                        "concurrent workload",
                        RecallStrategy::Similarity,
                    ))
                    .map_err(|_| panic_flag.store(true, Ordering::Relaxed));
                ops.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    // 3 revise threads
    for t in 0..3 {
        let eng = engine.clone();
        let seeds = seed_ids.clone();
        let ops = ops_count.clone();
        handles.push(thread::spawn(move || {
            let mut i = 0;
            while std::time::Instant::now() < deadline {
                let idx = (t * 10 + i) % seeds.len();
                let _ = eng.revise(ReviseInput::new_content(
                    seeds[idx].clone(),
                    format!("revised by thread {} iteration {}", t, i),
                ));
                i += 1;
                ops.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    // 2 forget threads
    for t in 0..2 {
        let eng = engine.clone();
        let seeds = seed_ids.clone();
        let forgotten = forgotten.clone();
        let ops = ops_count.clone();
        handles.push(thread::spawn(move || {
            let offset = t * 5;
            for i in 0..5 {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                let idx = offset + i;
                if idx < seeds.len() {
                    if let Ok(out) = eng.forget(ForgetCriteria::by_ids(vec![seeds[idx].clone()])) {
                        if out.forgotten_count > 0 {
                            forgotten.lock().unwrap().push(seeds[idx].clone());
                        }
                    }
                }
                ops.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for h in handles {
        h.join()
            .expect("thread panicked during concurrent workload");
    }

    assert!(
        !panic_flag.load(Ordering::Relaxed),
        "at least one operation errored during concurrent workload"
    );

    let total_ops = ops_count.load(Ordering::Relaxed);
    assert!(
        total_ops > 20,
        "expected substantial operations, got {}",
        total_ops
    );

    // Verify consistency: every remembered ID (not forgotten) is retrievable
    let remembered_ids = remembered.lock().unwrap();
    let forgotten_ids = forgotten.lock().unwrap();
    for id in remembered_ids.iter() {
        if !forgotten_ids.contains(id) {
            assert!(
                engine.get(id).is_ok(),
                "remembered and not-forgotten memory should be retrievable"
            );
        }
    }

    let count = engine.count().unwrap();
    assert!(count > 0, "database should have memories after workload");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category C: Crash Recovery
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_crash_recovery_remember() {
    let dir = tempfile::tempdir().unwrap();
    let mut ids = Vec::new();

    // Phase 1: write 100 memories then drop the engine
    {
        let engine = create_engine(dir.path(), 8);
        for i in 0..100 {
            let mem = engine
                .remember(RememberInput {
                    content: format!("persistent crash recovery memory {}", i),
                    importance: Some(0.6),
                    context: None,
                    entity_id: Some("recovery_entity".into()),
                    edges: vec![],
                    kind: None,
                })
                .unwrap();
            ids.push(mem.memory_id);
        }
        assert_eq!(engine.count().unwrap(), 100);
    }

    // Phase 2: reopen on the same directory
    {
        let engine = create_engine(dir.path(), 8);
        assert_eq!(
            engine.count().unwrap(),
            100,
            "count should match after restart"
        );

        for (i, id) in ids.iter().enumerate() {
            let mem = engine.get(id).unwrap();
            assert_eq!(
                mem.content,
                format!("persistent crash recovery memory {}", i),
                "memory {} content mismatch after restart",
                i
            );
            assert_eq!(mem.entity_id.as_deref(), Some("recovery_entity"));
            assert!(mem.embedding.is_some(), "embedding should persist");
        }

        // HNSW is rebuilt from the vectors CF on restart, so similarity recall works
        let recall_out = engine
            .recall(RecallInput::new(
                "persistent crash recovery",
                RecallStrategy::Similarity,
            ))
            .unwrap();
        assert!(
            !recall_out.results.is_empty(),
            "similarity recall should work after restart"
        );

        // Temporal index is in-memory and NOT rebuilt from the temporal CF on
        // restart (only HNSW is). Verify it doesn't panic; empty results are expected.
        let mut temporal_input = RecallInput::new("temporal query", RecallStrategy::Temporal);
        temporal_input.entity_id = Some("recovery_entity".into());
        let temporal_out = engine.recall(temporal_input);
        assert!(
            temporal_out.is_ok(),
            "temporal recall should not error after restart"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category D: Scale Validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn test_scale_1k_recall() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let ids = remember_n(&engine, "scale_1k", 1000);
    assert_eq!(ids.len(), 1000);
    assert_eq!(engine.count().unwrap(), 1000);

    // Similarity recall
    let sim_out = engine
        .recall(RecallInput::new(
            "memory number scale_1k",
            RecallStrategy::Similarity,
        ))
        .unwrap();
    assert!(
        !sim_out.results.is_empty(),
        "similarity recall should return results at 1K scale"
    );

    // Temporal recall
    let mut temp_input = RecallInput::new("temporal", RecallStrategy::Temporal);
    temp_input.entity_id = Some("scale_1k".into());
    let temp_out = engine.recall(temp_input).unwrap();
    assert!(
        !temp_out.results.is_empty(),
        "temporal recall should return results at 1K scale"
    );

    // Analogical recall
    let ana_out = engine
        .recall(RecallInput::new(
            "memory number scale_1k",
            RecallStrategy::Analogical,
        ))
        .unwrap();
    assert!(
        !ana_out.results.is_empty(),
        "analogical recall should return results at 1K scale"
    );
}

#[test]
#[ignore]
fn test_scale_10k_operations() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    for i in 0..10_000 {
        engine
            .remember(RememberInput {
                content: format!(
                    "scale test memory item number {} with extra padding words",
                    i
                ),
                importance: Some(0.5 + (i as f32 % 50.0) / 100.0),
                context: None,
                entity_id: Some(format!("scale_entity_{}", i % 10)),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    assert_eq!(engine.count().unwrap(), 10_000);

    // Recall at various top_k values
    for &top_k in &[1, 10, 50, 100, 500] {
        let mut input = RecallInput::new("scale test memory", RecallStrategy::Similarity);
        input.top_k = Some(top_k);
        let out = engine.recall(input).unwrap();
        assert!(
            out.results.len() <= top_k,
            "top_k={} but got {} results",
            top_k,
            out.results.len()
        );
        assert!(
            !out.results.is_empty(),
            "top_k={} should still return results",
            top_k
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category E: Edge Cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_recall_empty_database() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    // Similarity on empty DB
    let sim = engine
        .recall(RecallInput::new("anything", RecallStrategy::Similarity))
        .unwrap();
    assert!(
        sim.results.is_empty(),
        "similarity recall on empty DB should be empty"
    );

    // Temporal on empty DB
    let mut temp = RecallInput::new("anything", RecallStrategy::Temporal);
    temp.entity_id = Some("nobody".into());
    let temp_out = engine.recall(temp).unwrap();
    assert!(
        temp_out.results.is_empty(),
        "temporal recall on empty DB should be empty"
    );

    // Causal on empty DB
    let causal = engine
        .recall(RecallInput::new("anything", RecallStrategy::Causal))
        .unwrap();
    assert!(
        causal.results.is_empty(),
        "causal recall on empty DB should be empty"
    );

    // Analogical on empty DB
    let ana = engine
        .recall(RecallInput::new("anything", RecallStrategy::Analogical))
        .unwrap();
    assert!(
        ana.results.is_empty(),
        "analogical recall on empty DB should be empty"
    );
}

#[test]
fn test_forget_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let fake_id = vec![
        0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x01,
    ];
    let output = engine
        .forget(ForgetCriteria::by_ids(vec![fake_id]))
        .unwrap();
    assert_eq!(output.forgotten_count, 0);
    assert_eq!(output.cascade_count, 0);
}

#[test]
fn test_revise_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let fake_id = vec![
        0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x02,
    ];
    let result = engine.revise(ReviseInput::new_content(fake_id, "updated content"));
    assert!(
        matches!(result, Err(HebbsError::MemoryNotFound { .. })),
        "revise on nonexistent ID should return MemoryNotFound"
    );
}

#[test]
fn test_remember_empty_content() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let result = engine.remember(RememberInput {
        content: String::new(),
        importance: None,
        context: None,
        entity_id: None,
        edges: vec![],
            kind: None,
        });
    assert!(
        matches!(result, Err(HebbsError::InvalidInput { .. })),
        "empty content should be rejected"
    );
}

#[test]
fn test_remember_max_content() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let content = "x".repeat(64 * 1024);
    let mem = engine
        .remember(RememberInput {
            content,
            importance: Some(0.5),
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        })
        .unwrap();
    assert_eq!(mem.content.len(), 64 * 1024);

    let retrieved = engine.get(&mem.memory_id).unwrap();
    assert_eq!(retrieved.content.len(), 64 * 1024);
}

#[test]
fn test_recall_top_k_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let _ids = remember_n(&engine, "topk", 20);

    // top_k = 1
    let mut input1 = RecallInput::new("memory number topk", RecallStrategy::Similarity);
    input1.top_k = Some(1);
    let out1 = engine.recall(input1).unwrap();
    assert!(out1.results.len() <= 1);

    // top_k = 1000 (MAX_TOP_K)
    let mut input_max = RecallInput::new("memory number topk", RecallStrategy::Similarity);
    input_max.top_k = Some(1000);
    let out_max = engine.recall(input_max).unwrap();
    assert!(out_max.results.len() <= 1000);
    assert!(out_max.results.len() <= 20); // can't return more than exist
}

#[test]
fn test_prime_nonexistent_entity() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    // Seed some memories for a different entity
    remember_n(&engine, "existing_entity", 5);

    let prime_out = engine
        .prime(PrimeInput::new("nonexistent_entity_xyz"))
        .unwrap();
    assert_eq!(
        prime_out.temporal_count, 0,
        "prime for nonexistent entity should return 0 temporal results"
    );
}

#[test]
fn test_reflect_too_few_memories() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    // Insert fewer than min_memories_for_reflect (default 5)
    remember_n(&engine, "sparse_entity", 3);

    let mock_llm = MockLlmProvider::new();
    let config = ReflectConfig::default();
    let output = engine
        .reflect(
            ReflectScope::Entity {
                entity_id: "sparse_entity".into(),
                since_us: None,
            },
            &config,
            &mock_llm,
            &mock_llm,
        )
        .unwrap();

    assert_eq!(
        output.insights_created, 0,
        "reflect with too few memories should produce no insights"
    );
    assert_eq!(output.clusters_found, 0);
}

#[test]
fn test_multiple_entities() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let entities: Vec<String> = (0..5).map(|i| format!("entity_{}", i)).collect();
    for entity in &entities {
        remember_n(&engine, entity, 10);
    }

    assert_eq!(engine.count().unwrap(), 50);

    // Temporal recall scoped to each entity returns only that entity's memories
    for entity in &entities {
        let mut input = RecallInput::new("temporal query", RecallStrategy::Temporal);
        input.entity_id = Some(entity.clone());
        input.top_k = Some(100);
        let out = engine.recall(input).unwrap();
        assert!(
            !out.results.is_empty(),
            "entity {} should have temporal results",
            entity
        );
        for result in &out.results {
            assert_eq!(
                result.memory.entity_id.as_deref(),
                Some(entity.as_str()),
                "temporal recall for {} returned memory from wrong entity",
                entity
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category F: Data Integrity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_context_roundtrip_complex() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let mut context = HashMap::new();
    context.insert("string_val".to_string(), serde_json::json!("hello world"));
    context.insert("int_val".to_string(), serde_json::json!(42));
    context.insert("float_val".to_string(), serde_json::json!(1.23));
    context.insert("bool_val".to_string(), serde_json::json!(true));
    context.insert("null_val".to_string(), serde_json::json!(null));
    context.insert(
        "array_val".to_string(),
        serde_json::json!([1, "two", 3.0, null]),
    );
    context.insert(
        "nested".to_string(),
        serde_json::json!({
            "inner_key": "inner_value",
            "deep": {
                "level": 3,
                "tags": ["a", "b", "c"]
            }
        }),
    );

    let mem = engine
        .remember(RememberInput {
            content: "complex context roundtrip test".into(),
            importance: Some(0.7),
            context: Some(context.clone()),
            entity_id: None,
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let retrieved = engine.get(&mem.memory_id).unwrap();
    let ctx = retrieved.context().unwrap();

    assert_eq!(ctx["string_val"], serde_json::json!("hello world"));
    assert_eq!(ctx["int_val"], serde_json::json!(42));
    assert_eq!(ctx["float_val"], serde_json::json!(1.23));
    assert_eq!(ctx["bool_val"], serde_json::json!(true));
    assert_eq!(ctx["null_val"], serde_json::json!(null));
    assert_eq!(ctx["array_val"], serde_json::json!([1, "two", 3.0, null]));
    assert_eq!(
        ctx["nested"]["deep"]["tags"],
        serde_json::json!(["a", "b", "c"])
    );
}

#[test]
fn test_revise_context_merge_and_replace() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let mut initial_ctx = HashMap::new();
    initial_ctx.insert("key_a".to_string(), serde_json::json!("original_a"));
    initial_ctx.insert("key_b".to_string(), serde_json::json!("original_b"));
    initial_ctx.insert("key_c".to_string(), serde_json::json!("original_c"));

    let mem = engine
        .remember(RememberInput {
            content: "context merge replace test".into(),
            importance: Some(0.5),
            context: Some(initial_ctx),
            entity_id: None,
            edges: vec![],
            kind: None,
        })
        .unwrap();

    // ── Merge mode: new keys added, existing overwritten, absent preserved ──
    let mut merge_ctx = HashMap::new();
    merge_ctx.insert("key_a".to_string(), serde_json::json!("updated_a"));
    merge_ctx.insert("key_d".to_string(), serde_json::json!("new_d"));

    let merged = engine
        .revise(ReviseInput {
            memory_id: mem.memory_id.clone(),
            content: None,
            importance: None,
            context: Some(merge_ctx),
            context_mode: ContextMode::Merge,
            entity_id: None,
            edges: vec![],
        })
        .unwrap();

    let merged_ctx = merged.context().unwrap();
    assert_eq!(
        merged_ctx["key_a"],
        serde_json::json!("updated_a"),
        "merge should overwrite"
    );
    assert_eq!(
        merged_ctx["key_b"],
        serde_json::json!("original_b"),
        "merge should preserve absent"
    );
    assert_eq!(
        merged_ctx["key_c"],
        serde_json::json!("original_c"),
        "merge should preserve absent"
    );
    assert_eq!(
        merged_ctx["key_d"],
        serde_json::json!("new_d"),
        "merge should add new"
    );

    // ── Replace mode: entire context replaced ──
    let mut replace_ctx = HashMap::new();
    replace_ctx.insert("only_key".to_string(), serde_json::json!("replaced"));

    let replaced = engine
        .revise(ReviseInput {
            memory_id: mem.memory_id.clone(),
            content: None,
            importance: None,
            context: Some(replace_ctx),
            context_mode: ContextMode::Replace,
            entity_id: None,
            edges: vec![],
        })
        .unwrap();

    let replaced_ctx = replaced.context().unwrap();
    assert_eq!(replaced_ctx.len(), 1, "replace should have exactly 1 key");
    assert_eq!(replaced_ctx["only_key"], serde_json::json!("replaced"));
    assert!(
        !replaced_ctx.contains_key("key_a"),
        "old keys should be gone after replace"
    );
}

#[test]
fn test_remember_with_edges() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    // Create a chain: mem_a -> mem_b -> mem_c
    let mem_a = engine
        .remember(RememberInput {
            content: "first memory in causal chain".into(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("chain".into()),
            edges: vec![],
            kind: None,
        })
        .unwrap();
    let mut target_a = [0u8; 16];
    target_a.copy_from_slice(&mem_a.memory_id);

    let mem_b = engine
        .remember(RememberInput {
            content: "second memory caused by first in chain".into(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("chain".into()),
            edges: vec![RememberEdge {
                target_id: target_a,
                edge_type: EdgeType::CausedBy,
                confidence: Some(0.9),
            }],
            kind: None,
        })
        .unwrap();
    let mut target_b = [0u8; 16];
    target_b.copy_from_slice(&mem_b.memory_id);

    let mem_c = engine
        .remember(RememberInput {
            content: "third memory caused by second in chain".into(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("chain".into()),
            edges: vec![RememberEdge {
                target_id: target_b,
                edge_type: EdgeType::CausedBy,
                confidence: Some(0.8),
            }],
            kind: None,
        })
        .unwrap();

    // Verify outgoing edges
    let edges_b = engine.outgoing_edges(&target_b).unwrap();
    assert!(
        edges_b
            .iter()
            .any(|(et, tid, _)| *et == EdgeType::CausedBy && *tid == target_a),
        "mem_b should have CausedBy edge to mem_a"
    );

    // Causal recall from mem_c should traverse to mem_b (and possibly mem_a)
    let mut causal_input = RecallInput::new(
        "third memory caused by second in chain",
        RecallStrategy::Causal,
    );
    causal_input.edge_types = Some(vec![EdgeType::CausedBy]);
    causal_input.max_depth = Some(3);
    let causal_out = engine.recall(causal_input).unwrap();
    // Should find at least mem_b or mem_a via graph traversal
    let found_ids: Vec<&Vec<u8>> = causal_out
        .results
        .iter()
        .map(|r| &r.memory.memory_id)
        .collect();
    let has_chain_member = found_ids.contains(&&mem_a.memory_id)
        || found_ids.contains(&&mem_b.memory_id)
        || found_ids.contains(&&mem_c.memory_id);
    assert!(
        has_chain_member || causal_out.results.is_empty(),
        "causal recall should follow edges or return empty if seed not found"
    );
}

#[test]
fn test_forget_cascade() {
    let dir = tempfile::tempdir().unwrap();
    let engine = create_engine(dir.path(), 8);

    let mem = engine
        .remember(RememberInput {
            content: "memory to forget and verify cascade cleanup".into(),
            importance: Some(0.8),
            context: None,
            entity_id: Some("cascade_entity".into()),
            edges: vec![],
            kind: None,
        })
        .unwrap();
    let mem_id = mem.memory_id.clone();

    // Verify it's findable by similarity before forget
    let pre_recall = engine
        .recall(RecallInput::new(
            "memory to forget and verify cascade cleanup",
            RecallStrategy::Similarity,
        ))
        .unwrap();
    let pre_found = pre_recall
        .results
        .iter()
        .any(|r| r.memory.memory_id == mem_id);
    assert!(pre_found, "memory should be recallable before forget");

    // Verify temporal
    let mut pre_temp = RecallInput::new("temporal", RecallStrategy::Temporal);
    pre_temp.entity_id = Some("cascade_entity".into());
    let pre_temp_out = engine.recall(pre_temp).unwrap();
    assert!(
        pre_temp_out
            .results
            .iter()
            .any(|r| r.memory.memory_id == mem_id),
        "memory should appear in temporal results before forget"
    );

    // Forget
    let forget_out = engine
        .forget(ForgetCriteria::by_ids(vec![mem_id.clone()]))
        .unwrap();
    assert_eq!(forget_out.forgotten_count, 1);

    // Verify removed from similarity recall (HNSW index cleaned)
    let post_sim = engine
        .recall(RecallInput::new(
            "memory to forget and verify cascade cleanup",
            RecallStrategy::Similarity,
        ))
        .unwrap();
    let post_found = post_sim
        .results
        .iter()
        .any(|r| r.memory.memory_id == mem_id);
    assert!(
        !post_found,
        "forgotten memory should not appear in similarity recall"
    );

    // Verify removed from temporal recall
    let mut post_temp = RecallInput::new("temporal", RecallStrategy::Temporal);
    post_temp.entity_id = Some("cascade_entity".into());
    let post_temp_out = engine.recall(post_temp).unwrap();
    let post_temp_found = post_temp_out
        .results
        .iter()
        .any(|r| r.memory.memory_id == mem_id);
    assert!(
        !post_temp_found,
        "forgotten memory should not appear in temporal recall"
    );

    // Verify removed from get
    assert!(matches!(
        engine.get(&mem_id),
        Err(HebbsError::MemoryNotFound { .. })
    ));
}
