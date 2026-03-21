use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hebbs_core::engine::{Engine, RememberEdge, RememberInput, SCHEMA_VERSION};
use hebbs_core::forget::{encode_tombstone_key, tombstone_prefix, ForgetCriteria, Tombstone};
use hebbs_core::keys;
use hebbs_core::memory::{Memory, MemoryKind};
use hebbs_core::recall::{PrimeInput, RecallInput, RecallStrategy};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig, ReflectScope};
use hebbs_core::revise::ReviseInput;
use hebbs_core::subscribe::{SubscribeConfig, SubscribePush};
use hebbs_embed::MockEmbedder;
use hebbs_index::{EdgeType, GraphIndex, HnswParams};
use hebbs_reflect::MockLlmProvider;
use hebbs_storage::{ColumnFamilyName, InMemoryBackend, RocksDbBackend, StorageBackend};

fn rocksdb_engine(dir: &std::path::Path) -> Engine {
    let backend = Arc::new(RocksDbBackend::open(dir).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    Engine::new(backend, embedder).unwrap()
}

/// 1,000 memories survive a process restart (re-open of RocksDB).
#[test]
fn memories_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut ids: Vec<Vec<u8>> = Vec::new();

    {
        let engine = rocksdb_engine(dir.path());
        for i in 0..1_000 {
            let mem = engine
                .remember(RememberInput {
                    content: format!("persistent memory number {}", i),
                    importance: Some(0.5 + (i as f32 % 50.0) / 100.0),
                    context: None,
                    entity_id: Some(format!("entity_{}", i % 10)),
                    edges: vec![],
                    kind: None,
                })
                .unwrap();
            ids.push(mem.memory_id);
        }
        // engine + RocksDB dropped here
    }

    {
        let engine = rocksdb_engine(dir.path());
        for (i, id) in ids.iter().enumerate() {
            let mem = engine.get(id).unwrap();
            assert_eq!(
                mem.content,
                format!("persistent memory number {}", i),
                "memory {} corrupted after restart",
                i
            );
        }
        assert_eq!(engine.count().unwrap(), 1_000);
    }
}

/// 10 threads × 1,000 memories = 10,000 total with zero corruption.
#[test]
fn concurrent_writes_no_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let engine = Arc::new(Engine::new(backend, embedder).unwrap());

    let num_threads = 10;
    let writes_per_thread = 1_000;

    let handles: Vec<_> = (0..num_threads)
        .map(|thread_id| {
            let engine = engine.clone();
            thread::spawn(move || {
                let mut ids = Vec::with_capacity(writes_per_thread);
                for i in 0..writes_per_thread {
                    let mem = engine
                        .remember(RememberInput {
                            content: format!("thread_{}_memory_{}", thread_id, i),
                            importance: Some(0.5),
                            context: None,
                            entity_id: Some(format!("thread_{}", thread_id)),
                            edges: vec![],
                            kind: None,
                        })
                        .unwrap();
                    ids.push(mem.memory_id);
                }
                ids
            })
        })
        .collect();

    let all_ids: Vec<Vec<u8>> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();

    assert_eq!(all_ids.len(), num_threads * writes_per_thread);

    // Verify all memories are readable and uncorrupted
    for id in &all_ids {
        let mem = engine.get(id).unwrap();
        assert!(mem.content.starts_with("thread_"));
    }

    // Verify count matches
    assert_eq!(engine.count().unwrap(), num_threads * writes_per_thread);

    // Verify no duplicate IDs
    let unique: std::collections::HashSet<&Vec<u8>> = all_ids.iter().collect();
    assert_eq!(
        unique.len(),
        all_ids.len(),
        "ULID collision detected under concurrency"
    );
}

/// Schema version is persisted and validated on re-open.
#[test]
fn schema_version_persists() {
    let dir = tempfile::tempdir().unwrap();

    {
        let _engine = rocksdb_engine(dir.path());
    }

    // Verify directly in storage
    let backend = RocksDbBackend::open(dir.path()).unwrap();
    let key = keys::encode_meta_key("schema_version");
    let val = backend.get(ColumnFamilyName::Meta, &key).unwrap().unwrap();
    let version = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
    assert_eq!(version, SCHEMA_VERSION);
}

/// Delete followed by get returns MemoryNotFound.
#[test]
fn delete_then_get_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine(dir.path());

    let mem = engine
        .remember(RememberInput {
            content: "will be deleted".to_string(),
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        })
        .unwrap();

    engine.delete(&mem.memory_id).unwrap();

    let result = engine.get(&mem.memory_id);
    assert!(result.is_err());
}

/// Entity listing works correctly across RocksDB (not just in-memory).
#[test]
fn list_by_entity_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine(dir.path());

    for i in 0..20 {
        engine
            .remember(RememberInput {
                content: format!("memory {}", i),
                importance: None,
                context: None,
                entity_id: Some(if i % 2 == 0 {
                    "even".to_string()
                } else {
                    "odd".to_string()
                }),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let even = engine.list_by_entity("even", 100).unwrap();
    assert_eq!(even.len(), 10);

    let odd = engine.list_by_entity("odd", 100).unwrap();
    assert_eq!(odd.len(), 10);

    let none = engine.list_by_entity("nonexistent", 100).unwrap();
    assert_eq!(none.len(), 0);
}

/// Write batch atomicity: multiple operations in a single batch all succeed.
#[test]
fn write_batch_atomicity_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let backend = RocksDbBackend::open(dir.path()).unwrap();

    let ops = vec![
        hebbs_storage::BatchOperation::Put {
            cf: ColumnFamilyName::Default,
            key: b"key_1".to_vec(),
            value: b"val_1".to_vec(),
        },
        hebbs_storage::BatchOperation::Put {
            cf: ColumnFamilyName::Default,
            key: b"key_2".to_vec(),
            value: b"val_2".to_vec(),
        },
        hebbs_storage::BatchOperation::Put {
            cf: ColumnFamilyName::Meta,
            key: b"batch_marker".to_vec(),
            value: b"done".to_vec(),
        },
    ];

    backend.write_batch(&ops).unwrap();

    assert_eq!(
        backend.get(ColumnFamilyName::Default, b"key_1").unwrap(),
        Some(b"val_1".to_vec())
    );
    assert_eq!(
        backend.get(ColumnFamilyName::Default, b"key_2").unwrap(),
        Some(b"val_2".to_vec())
    );
    assert_eq!(
        backend
            .get(ColumnFamilyName::Meta, b"batch_marker")
            .unwrap(),
        Some(b"done".to_vec())
    );
}

/// Concurrent reads and writes do not interfere.
#[test]
fn concurrent_read_write_no_interference() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let engine = Arc::new(Engine::new(backend, embedder).unwrap());

    // Pre-populate with 100 memories
    let mut pre_ids = Vec::new();
    for i in 0..100 {
        let mem = engine
            .remember(RememberInput {
                content: format!("pre-existing memory {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: None,
                edges: vec![],
                kind: None,
            })
            .unwrap();
        pre_ids.push(mem.memory_id);
    }

    let writer_engine = engine.clone();
    let reader_engine = engine.clone();
    let reader_ids = pre_ids.clone();

    // Writer thread: add 500 more memories
    let writer = thread::spawn(move || {
        for i in 0..500 {
            writer_engine
                .remember(RememberInput {
                    content: format!("concurrent write {}", i),
                    importance: Some(0.6),
                    context: None,
                    entity_id: None,
                    edges: vec![],
                    kind: None,
                })
                .unwrap();
        }
    });

    // Reader thread: continuously read pre-existing memories
    let reader = thread::spawn(move || {
        for _ in 0..10 {
            for id in &reader_ids {
                let mem = reader_engine.get(id).unwrap();
                assert!(mem.content.starts_with("pre-existing memory"));
            }
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();

    assert_eq!(engine.count().unwrap(), 600);
}

/// Context data survives serialization through RocksDB.
#[test]
fn context_survives_rocksdb_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine(dir.path());

    let mut ctx = std::collections::HashMap::new();
    ctx.insert(
        "tags".to_string(),
        serde_json::json!(["urgent", "follow-up", "pricing"]),
    );
    ctx.insert("conversion_score".to_string(), serde_json::json!(0.85));
    ctx.insert(
        "nested".to_string(),
        serde_json::json!({"key": "value", "num": 42}),
    );

    let mem = engine
        .remember(RememberInput {
            content: "context test".to_string(),
            importance: Some(0.9),
            context: Some(ctx.clone()),
            entity_id: Some("ctx_test".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let retrieved = engine.get(&mem.memory_id).unwrap();
    let retrieved_ctx = retrieved.context().unwrap();
    assert_eq!(retrieved_ctx, ctx);
}

/// Verify all 5 column families exist after open.
#[test]
fn all_column_families_exist() {
    let dir = tempfile::tempdir().unwrap();
    let backend = RocksDbBackend::open(dir.path()).unwrap();

    // Verify we can write to every CF
    for cf in ColumnFamilyName::all() {
        backend.put(*cf, b"test_key", b"test_val").unwrap();
        let val = backend.get(*cf, b"test_key").unwrap();
        assert_eq!(val, Some(b"test_val".to_vec()), "CF {} failed", cf);
        backend.delete(*cf, b"test_key").unwrap();
    }
}

/// Compaction completes without error.
#[test]
fn compaction_runs_without_error() {
    let dir = tempfile::tempdir().unwrap();
    let backend = RocksDbBackend::open(dir.path()).unwrap();

    // Write some data first
    for i in 0..100u32 {
        backend
            .put(ColumnFamilyName::Default, &i.to_be_bytes(), b"value")
            .unwrap();
    }

    // Compact should not error
    for cf in ColumnFamilyName::all() {
        backend.compact(*cf).unwrap();
    }
}

/// Verify memory IDs (ULIDs) are time-sortable:
/// memories created later have byte-greater IDs.
#[test]
fn ulid_time_sorting() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine(dir.path());

    let mut ids: Vec<Vec<u8>> = Vec::new();
    for i in 0..100 {
        let mem = engine
            .remember(RememberInput {
                content: format!("memory {}", i),
                importance: None,
                context: None,
                entity_id: None,
                edges: vec![],
                kind: None,
            })
            .unwrap();
        ids.push(mem.memory_id);
    }

    // IDs should be monotonically non-decreasing
    for window in ids.windows(2) {
        assert!(
            window[0] <= window[1],
            "ULID ordering violated: {:?} > {:?}",
            window[0],
            window[1]
        );
    }
}

/// Phase 2: remember() stores a 384-dim normalized embedding via RocksDB.
#[test]
fn remember_stores_embedding_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine(dir.path());

    let mem = engine
        .remember(RememberInput {
            content: "Customer asked about enterprise pricing".to_string(),
            importance: Some(0.8),
            context: None,
            entity_id: Some("cust_42".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    assert!(mem.embedding.is_some());
    let emb = mem.embedding.as_ref().unwrap();
    assert_eq!(emb.len(), 384);

    // Verify L2-normalized
    let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 1e-5,
        "embedding norm {} is not 1.0",
        norm
    );

    // Verify embedding survives persistence
    let retrieved = engine.get(&mem.memory_id).unwrap();
    assert_eq!(mem.embedding, retrieved.embedding);
}

/// Phase 2: different content produces different embeddings.
#[test]
fn different_content_different_embeddings() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine(dir.path());

    let m1 = engine
        .remember(RememberInput {
            content: "The weather is sunny".to_string(),
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let m2 = engine
        .remember(RememberInput {
            content: "Quarterly revenue exceeded expectations".to_string(),
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        })
        .unwrap();

    assert_ne!(m1.embedding, m2.embedding);
}

/// Phase 2: embedding survives RocksDB restart.
#[test]
fn embedding_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let original_embedding;
    let memory_id;

    {
        let engine = rocksdb_engine(dir.path());
        let mem = engine
            .remember(RememberInput {
                content: "important memory".to_string(),
                importance: Some(0.9),
                context: None,
                entity_id: None,
                edges: vec![],
                kind: None,
            })
            .unwrap();
        original_embedding = mem.embedding.clone();
        memory_id = mem.memory_id;
    }

    {
        let engine = rocksdb_engine(dir.path());
        let retrieved = engine.get(&memory_id).unwrap();
        assert_eq!(
            original_embedding, retrieved.embedding,
            "embedding must survive process restart"
        );
    }
}

/// Phase 2: concurrent remember calls all produce embeddings.
#[test]
fn concurrent_remember_all_have_embeddings() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let engine = Arc::new(Engine::new(backend, embedder).unwrap());

    let num_threads = 8;
    let writes_per_thread = 100;

    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let engine = engine.clone();
            thread::spawn(move || {
                let mut ids = Vec::with_capacity(writes_per_thread);
                for i in 0..writes_per_thread {
                    let mem = engine
                        .remember(RememberInput {
                            content: format!("thread {} memory {}", t, i),
                            importance: Some(0.5),
                            context: None,
                            entity_id: None,
                            edges: vec![],
                            kind: None,
                        })
                        .unwrap();
                    assert!(
                        mem.embedding.is_some(),
                        "embedding missing for thread {} memory {}",
                        t,
                        i
                    );
                    assert_eq!(mem.embedding.as_ref().unwrap().len(), 384);
                    ids.push(mem.memory_id);
                }
                ids
            })
        })
        .collect();

    let all_ids: Vec<Vec<u8>> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    assert_eq!(all_ids.len(), num_threads * writes_per_thread);

    // Verify all stored embeddings are intact
    for id in &all_ids {
        let mem = engine.get(id).unwrap();
        assert!(mem.embedding.is_some());
        let emb = mem.embedding.as_ref().unwrap();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "embedding norm {} is not 1.0",
            norm
        );
    }
}

/// Storage backends behave identically for the core operations.
/// This is the canonical test from the Phase 1 risk register:
/// "In-memory storage mock diverges from RocksDB behavior"
#[test]
fn backend_behavioral_parity() {
    let dir = tempfile::tempdir().unwrap();
    let rocksdb = RocksDbBackend::open(dir.path()).unwrap();
    let inmem = InMemoryBackend::new();

    let backends: Vec<(&str, &dyn StorageBackend)> =
        vec![("rocksdb", &rocksdb), ("inmemory", &inmem)];

    for (name, backend) in &backends {
        // put + get
        backend
            .put(ColumnFamilyName::Default, b"k1", b"v1")
            .unwrap();
        assert_eq!(
            backend.get(ColumnFamilyName::Default, b"k1").unwrap(),
            Some(b"v1".to_vec()),
            "{}: put+get failed",
            name
        );

        // get miss
        assert_eq!(
            backend.get(ColumnFamilyName::Default, b"missing").unwrap(),
            None,
            "{}: get miss failed",
            name
        );

        // delete
        backend.delete(ColumnFamilyName::Default, b"k1").unwrap();
        assert_eq!(
            backend.get(ColumnFamilyName::Default, b"k1").unwrap(),
            None,
            "{}: delete failed",
            name
        );

        // delete non-existent (should succeed silently)
        backend
            .delete(ColumnFamilyName::Default, b"never_existed")
            .unwrap();

        // prefix iterator
        backend
            .put(ColumnFamilyName::Default, b"pfx_a", b"1")
            .unwrap();
        backend
            .put(ColumnFamilyName::Default, b"pfx_b", b"2")
            .unwrap();
        backend
            .put(ColumnFamilyName::Default, b"other", b"3")
            .unwrap();
        let results = backend
            .prefix_iterator(ColumnFamilyName::Default, b"pfx_")
            .unwrap();
        assert_eq!(results.len(), 2, "{}: prefix iterator count wrong", name);
        assert_eq!(results[0].0, b"pfx_a", "{}: prefix sort wrong", name);
        assert_eq!(results[1].0, b"pfx_b", "{}: prefix sort wrong", name);

        // range iterator
        for i in 0u8..10 {
            backend.put(ColumnFamilyName::Temporal, &[i], &[i]).unwrap();
        }
        let range = backend
            .range_iterator(ColumnFamilyName::Temporal, &[2], &[5])
            .unwrap();
        assert_eq!(range.len(), 3, "{}: range iterator count wrong", name);
        assert_eq!(range[0].0, vec![2u8], "{}: range start wrong", name);
        assert_eq!(range[2].0, vec![4u8], "{}: range end wrong", name);

        // write batch
        let ops = vec![
            hebbs_storage::BatchOperation::Put {
                cf: ColumnFamilyName::Meta,
                key: b"batch_a".to_vec(),
                value: b"ba".to_vec(),
            },
            hebbs_storage::BatchOperation::Put {
                cf: ColumnFamilyName::Meta,
                key: b"batch_b".to_vec(),
                value: b"bb".to_vec(),
            },
        ];
        backend.write_batch(&ops).unwrap();
        assert_eq!(
            backend.get(ColumnFamilyName::Meta, b"batch_a").unwrap(),
            Some(b"ba".to_vec()),
            "{}: write batch failed",
            name
        );
        assert_eq!(
            backend.get(ColumnFamilyName::Meta, b"batch_b").unwrap(),
            Some(b"bb".to_vec()),
            "{}: write batch failed",
            name
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Phase 4: Recall Engine Integration Tests
// ═══════════════════════════════════════════════════════════════════

fn rocksdb_engine_with_larger_hnsw(dir: &std::path::Path) -> Engine {
    let backend = Arc::new(RocksDbBackend::open(dir).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 16);
    Engine::new_with_params(backend, embedder, params, 42).unwrap()
}

/// Full recall lifecycle: remember 1K+ memories, run each strategy, verify correctness.
#[test]
fn recall_full_lifecycle_1k_memories() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    // Insert 1,000 memories across 10 entities
    let mut all_ids: Vec<Vec<u8>> = Vec::new();
    for i in 0..1_000 {
        let mem = engine
            .remember(RememberInput {
                content: format!("lifecycle test memory content number {}", i),
                importance: Some(0.3 + (i as f32 % 7.0) / 10.0),
                context: None,
                entity_id: Some(format!("entity_{}", i % 10)),
                edges: vec![],
                kind: None,
            })
            .unwrap();
        all_ids.push(mem.memory_id);
    }

    // Similarity recall
    let sim_output = engine
        .recall(RecallInput::new(
            "lifecycle test memory content",
            RecallStrategy::Similarity,
        ))
        .unwrap();
    assert!(
        !sim_output.results.is_empty(),
        "similarity recall returned no results"
    );
    assert!(sim_output.strategy_errors.is_empty());

    // Temporal recall
    let mut temp_input = RecallInput::new("events", RecallStrategy::Temporal);
    temp_input.entity_id = Some("entity_0".to_string());
    temp_input.top_k = Some(100);
    let temp_output = engine.recall(temp_input).unwrap();
    assert_eq!(
        temp_output.results.len(),
        100,
        "entity_0 should have exactly 100 memories"
    );

    // Causal recall (text seed mode)
    let causal_output = engine
        .recall(RecallInput::new(
            "lifecycle test memory",
            RecallStrategy::Causal,
        ))
        .unwrap();
    assert!(causal_output.strategy_errors.is_empty());

    // Analogical recall
    let ana_output = engine
        .recall(RecallInput::new(
            "lifecycle test memory",
            RecallStrategy::Analogical,
        ))
        .unwrap();
    assert!(
        !ana_output.results.is_empty(),
        "analogical recall returned no results"
    );
}

/// Multi-strategy consistency: run same recall as single and multi,
/// verify multi-strategy contains all single-strategy results.
#[test]
fn recall_multi_strategy_consistency_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    for i in 0..200 {
        engine
            .remember(RememberInput {
                content: format!("consistency test memory {}", i),
                importance: Some(0.6),
                context: None,
                entity_id: Some("cons_entity".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    // Single-strategy results
    let mut sim_input = RecallInput::new("consistency test", RecallStrategy::Similarity);
    sim_input.top_k = Some(10);
    let sim_ids: HashSet<Vec<u8>> = engine
        .recall(sim_input)
        .unwrap()
        .results
        .iter()
        .map(|r| r.memory.memory_id.clone())
        .collect();

    let mut temp_input = RecallInput::new("events", RecallStrategy::Temporal);
    temp_input.entity_id = Some("cons_entity".to_string());
    temp_input.top_k = Some(10);
    let temp_ids: HashSet<Vec<u8>> = engine
        .recall(temp_input)
        .unwrap()
        .results
        .iter()
        .map(|r| r.memory.memory_id.clone())
        .collect();

    // Multi-strategy with generous top_k
    let mut multi_input = RecallInput::multi(
        "consistency test",
        vec![RecallStrategy::Similarity, RecallStrategy::Temporal],
    );
    multi_input.entity_id = Some("cons_entity".to_string());
    multi_input.top_k = Some(200);
    let multi_ids: HashSet<Vec<u8>> = engine
        .recall(multi_input)
        .unwrap()
        .results
        .iter()
        .map(|r| r.memory.memory_id.clone())
        .collect();

    for id in &sim_ids {
        assert!(
            multi_ids.contains(id),
            "multi-strategy missing similarity result"
        );
    }
    for id in &temp_ids {
        assert!(
            multi_ids.contains(id),
            "multi-strategy missing temporal result"
        );
    }
}

/// Reinforcement persists across engine restart.
#[test]
fn reinforcement_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let memory_id;

    {
        let engine = rocksdb_engine_with_larger_hnsw(dir.path());
        let mem = engine
            .remember(RememberInput {
                content: "reinforcement persist test".to_string(),
                importance: Some(0.8),
                context: None,
                entity_id: Some("reinf_entity".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
        memory_id = mem.memory_id.clone();

        // Recall to reinforce
        let output = engine
            .recall(RecallInput::new(
                "reinforcement persist test",
                RecallStrategy::Similarity,
            ))
            .unwrap();
        assert!(!output.results.is_empty());
    }

    {
        let engine = rocksdb_engine_with_larger_hnsw(dir.path());
        let retrieved = engine.get(&memory_id).unwrap();
        assert!(
            retrieved.access_count >= 1,
            "access_count must survive engine restart, got {}",
            retrieved.access_count
        );
    }
}

/// Concurrent recall: 10 threads running recall simultaneously.
#[test]
fn concurrent_recall_no_panics() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 16);
    let engine = Arc::new(Engine::new_with_params(backend, embedder, params, 42).unwrap());

    // Pre-populate
    for i in 0..200 {
        engine
            .remember(RememberInput {
                content: format!("concurrent recall memory {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("conc_entity".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let num_threads = 10;
    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let engine = engine.clone();
            thread::spawn(move || {
                for i in 0..20 {
                    let output = engine
                        .recall(RecallInput::new(
                            format!("concurrent recall memory {}", (t * 20 + i) % 200),
                            RecallStrategy::Similarity,
                        ))
                        .unwrap();
                    assert!(!output.results.is_empty());
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("recall thread panicked");
    }
}

/// Prime round-trip: remember 50 memories for entity, prime returns results.
#[test]
fn prime_round_trip_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    for i in 0..50 {
        engine
            .remember(RememberInput {
                content: format!("prime test interaction {}", i),
                importance: Some(0.6),
                context: None,
                entity_id: Some("prime_entity".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let output = engine.prime(PrimeInput::new("prime_entity")).unwrap();
    assert!(
        !output.results.is_empty(),
        "prime() should return results for entity with 50 memories"
    );
    assert!(
        output.temporal_count > 0,
        "prime() should include temporal results"
    );

    // Verify no duplicates
    let ids: Vec<&Vec<u8>> = output.results.iter().map(|r| &r.memory.memory_id).collect();
    let unique: HashSet<&Vec<u8>> = ids.iter().cloned().collect();
    assert_eq!(ids.len(), unique.len(), "prime() produced duplicates");
}

/// Edge case: recall on empty database returns empty for all strategies.
#[test]
fn recall_empty_database_all_strategies() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    let strategies = [RecallStrategy::Similarity, RecallStrategy::Analogical];

    for strategy in &strategies {
        let output = engine
            .recall(RecallInput::new("nothing", strategy.clone()))
            .unwrap();
        assert!(
            output.results.is_empty(),
            "empty database should return no results for {:?}",
            strategy
        );
    }

    // Temporal: returns error (no entity_id)
    let mut temp_input = RecallInput::new("nothing", RecallStrategy::Temporal);
    temp_input.entity_id = Some("nobody".to_string());
    let temp_output = engine.recall(temp_input).unwrap();
    assert!(temp_output.results.is_empty());

    // Causal with text seed: returns empty (no memories to find seed in)
    let causal_output = engine
        .recall(RecallInput::new("nothing", RecallStrategy::Causal))
        .unwrap();
    assert!(causal_output.results.is_empty());
}

/// Causal recall from a memory with no edges returns only the seed (or empty).
#[test]
fn causal_recall_no_edges_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    let mem = engine
        .remember(RememberInput {
            content: "isolated memory with no edges".to_string(),
            importance: Some(0.5),
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let cue_hex = hex::encode(&mem.memory_id);
    let output = engine
        .recall(RecallInput::new(cue_hex, RecallStrategy::Causal))
        .unwrap();
    assert!(output.strategy_errors.is_empty());
}

/// Recall with graph edges: causal traversal finds connected memories.
/// Graph traversal follows forward edges, so we traverse from the source
/// node that has outgoing edges to find its targets.
#[test]
fn causal_recall_with_edges_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    let root = engine
        .remember(RememberInput {
            content: "root event: budget freeze announced".to_string(),
            importance: Some(0.9),
            context: None,
            entity_id: Some("acme".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();
    let mut root_id = [0u8; 16];
    root_id.copy_from_slice(&root.memory_id);

    let effect1 = engine
        .remember(RememberInput {
            content: "effect: deal postponed to Q2".to_string(),
            importance: Some(0.8),
            context: None,
            entity_id: Some("acme".to_string()),
            edges: vec![RememberEdge {
                target_id: root_id,
                edge_type: EdgeType::CausedBy,
                confidence: Some(0.95),
            }],
            kind: None,
        })
        .unwrap();
    let mut effect1_id = [0u8; 16];
    effect1_id.copy_from_slice(&effect1.memory_id);

    let _effect2 = engine
        .remember(RememberInput {
            content: "effect: pricing revision needed".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("acme".to_string()),
            edges: vec![RememberEdge {
                target_id: root_id,
                edge_type: EdgeType::CausedBy,
                confidence: Some(0.85),
            }],
            kind: None,
        })
        .unwrap();

    // Causal recall from effect1 via ID — it has an outgoing CausedBy edge to root
    let effect1_hex = hex::encode(&effect1.memory_id);
    let output = engine
        .recall(RecallInput::new(effect1_hex, RecallStrategy::Causal))
        .unwrap();
    assert!(output.strategy_errors.is_empty());
    assert!(
        !output.results.is_empty(),
        "causal recall should find connected memories via forward edges"
    );
    // The traversal should find root_id connected via CausedBy
    let found_ids: Vec<[u8; 16]> = output
        .results
        .iter()
        .map(|r| {
            let mut id = [0u8; 16];
            id.copy_from_slice(&r.memory.memory_id);
            id
        })
        .collect();
    assert!(
        found_ids.contains(&root_id),
        "causal recall from effect should find the root cause"
    );
}

/// Recall at scale: 10K memories, similarity recall returns semantically reasonable results.
#[test]
fn recall_at_10k_scale() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    for i in 0..10_000 {
        engine
            .remember(RememberInput {
                content: format!("scale test memory content number {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some(format!("entity_{}", i % 100)),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    // Similarity recall
    let output = engine
        .recall(RecallInput::new(
            "scale test memory content number 5000",
            RecallStrategy::Similarity,
        ))
        .unwrap();
    assert!(!output.results.is_empty());
    assert!(output.strategy_errors.is_empty());

    // Verify results are ranked by composite score descending
    for window in output.results.windows(2) {
        assert!(
            window[0].score >= window[1].score,
            "results not sorted by score: {} < {}",
            window[0].score,
            window[1].score
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Phase 5: Revise, Forget, Decay Integration Tests
// ═══════════════════════════════════════════════════════════════════

/// Phase 5: remember a memory, revise its content, verify persistence and stable ID.
#[test]
fn revise_roundtrip_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine(dir.path());

    let mem = engine
        .remember(RememberInput {
            content: "original content before revision".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("revise_test".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let original_id = mem.memory_id.clone();

    let revised = engine
        .revise(ReviseInput::new_content(
            original_id.clone(),
            "revised content after update",
        ))
        .unwrap();

    assert_eq!(
        revised.memory_id, original_id,
        "memory_id must not change on revise"
    );
    assert_eq!(revised.content, "revised content after update");

    let retrieved = engine.get(&original_id).unwrap();
    assert_eq!(retrieved.content, "revised content after update");
    assert_eq!(retrieved.memory_id, original_id);
}

/// Phase 5: revision persists across engine restart.
#[test]
fn revise_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let memory_id;

    {
        let engine = rocksdb_engine(dir.path());
        let mem = engine
            .remember(RememberInput {
                content: "pre-restart content".to_string(),
                importance: Some(0.6),
                context: None,
                entity_id: Some("restart_revise".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
        memory_id = mem.memory_id.clone();

        engine
            .revise(ReviseInput::new_content(
                memory_id.clone(),
                "post-revision content",
            ))
            .unwrap();
    }

    {
        let engine = rocksdb_engine(dir.path());
        let retrieved = engine.get(&memory_id).unwrap();
        assert_eq!(
            retrieved.content, "post-revision content",
            "revised content must survive engine restart"
        );
    }
}

/// Phase 5: revising content triggers re-embedding; recall finds the revised memory.
#[test]
fn revise_re_embeds_on_content_change_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    let mem = engine
        .remember(RememberInput {
            content: "the weather is sunny and warm today".to_string(),
            importance: Some(0.8),
            context: None,
            entity_id: Some("re_embed_test".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let original_embedding = mem.embedding.clone().unwrap();

    let revised = engine
        .revise(ReviseInput::new_content(
            mem.memory_id.clone(),
            "quarterly revenue exceeded all expectations significantly",
        ))
        .unwrap();

    let revised_embedding = revised.embedding.clone().unwrap();
    assert_ne!(
        original_embedding, revised_embedding,
        "embedding must change when content changes"
    );

    let output = engine
        .recall(RecallInput::new(
            "quarterly revenue exceeded expectations",
            RecallStrategy::Similarity,
        ))
        .unwrap();
    assert!(
        !output.results.is_empty(),
        "recall should find the revised memory"
    );

    let found_ids: HashSet<Vec<u8>> = output
        .results
        .iter()
        .map(|r| r.memory.memory_id.clone())
        .collect();
    assert!(
        found_ids.contains(&mem.memory_id),
        "revised memory should appear in similarity recall for new content"
    );
}

/// Phase 5: revise creates a predecessor snapshot accessible via RevisedFrom graph edge.
#[test]
fn revise_creates_predecessor_snapshot_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let raw_backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let engine = Engine::new(
        Arc::clone(&raw_backend) as Arc<dyn StorageBackend>,
        Arc::new(MockEmbedder::default_dims()),
    )
    .unwrap();

    let mem = engine
        .remember(RememberInput {
            content: "snapshot test original content".to_string(),
            importance: Some(0.9),
            context: None,
            entity_id: Some("snapshot_entity".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let original_content = mem.content.clone();

    engine
        .revise(ReviseInput::new_content(
            mem.memory_id.clone(),
            "snapshot test revised content",
        ))
        .unwrap();

    // Find the snapshot via RevisedFrom forward edge in the Graph CF.
    // Forward key: [0xF0][source_id 16B][edge_type 1B][target_id 16B] = 34 bytes
    let mut mem_id_arr = [0u8; 16];
    mem_id_arr.copy_from_slice(&mem.memory_id);
    let graph_prefix = GraphIndex::encode_forward_prefix(&mem_id_arr);
    let graph_entries = raw_backend
        .prefix_iterator(ColumnFamilyName::Graph, &graph_prefix)
        .unwrap();

    let mut snapshot_id: Option<Vec<u8>> = None;
    for (key, _) in &graph_entries {
        if let Ok((_src, edge_type, target)) = GraphIndex::decode_forward_key(key) {
            if edge_type == EdgeType::RevisedFrom {
                snapshot_id = Some(target.to_vec());
                break;
            }
        }
    }

    let snapshot_id = snapshot_id.expect("RevisedFrom edge must exist after revise");

    // Retrieve the snapshot from the default CF and verify content
    let snapshot_key = keys::encode_memory_key(&snapshot_id);
    let snapshot_bytes = raw_backend
        .get(ColumnFamilyName::Default, &snapshot_key)
        .unwrap()
        .expect("snapshot memory must exist in default CF");
    let snapshot = Memory::from_bytes(&snapshot_bytes).unwrap();
    assert_eq!(
        snapshot.content, original_content,
        "snapshot must preserve the original content"
    );
}

/// Phase 5: forget specific memories by ID, verify count and NotFound.
#[test]
fn forget_by_id_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine(dir.path());

    let mut ids = Vec::new();
    for i in 0..5 {
        let mem = engine
            .remember(RememberInput {
                content: format!("forget-by-id memory {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: None,
                edges: vec![],
                kind: None,
            })
            .unwrap();
        ids.push(mem.memory_id);
    }

    let to_forget = vec![ids[1].clone(), ids[3].clone()];
    let output = engine
        .forget(ForgetCriteria::by_ids(to_forget.clone()))
        .unwrap();
    assert_eq!(output.forgotten_count, 2);

    assert_eq!(engine.count().unwrap(), 3);

    for fid in &to_forget {
        assert!(
            engine.get(fid).is_err(),
            "forgotten memory should return NotFound"
        );
    }

    for &idx in &[0, 2, 4] {
        assert!(
            engine.get(&ids[idx]).is_ok(),
            "surviving memory {} must be readable",
            idx
        );
    }
}

/// Phase 5: forget all memories for an entity, verify only the other entity survives.
#[test]
fn forget_by_entity_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine(dir.path());

    for i in 0..10 {
        engine
            .remember(RememberInput {
                content: format!("alpha memory {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("alpha".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let mut beta_ids = Vec::new();
    for i in 0..10 {
        let mem = engine
            .remember(RememberInput {
                content: format!("beta memory {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("beta".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
        beta_ids.push(mem.memory_id);
    }

    let output = engine.forget(ForgetCriteria::by_entity("alpha")).unwrap();
    assert_eq!(output.forgotten_count, 10);

    assert_eq!(engine.count().unwrap(), 10);

    for id in &beta_ids {
        let mem = engine.get(id).unwrap();
        assert_eq!(mem.entity_id.as_deref(), Some("beta"));
    }
}

/// Regression: forget-by-entity must work after compaction flushes the Temporal CF
/// to SST files. Previously, a noop prefix extractor on the Temporal CF caused
/// range_iterator to return 0 results after compaction because each key was its
/// own prefix and the bloom filter rejected seek targets that weren't exact keys.
#[test]
fn forget_by_entity_after_compaction_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let raw_backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let engine = Engine::new(
        Arc::clone(&raw_backend) as Arc<dyn StorageBackend>,
        Arc::new(MockEmbedder::default_dims()),
    )
    .unwrap();

    for i in 0..6 {
        engine
            .remember(RememberInput {
                content: format!("user pref memory {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("user_prefs".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let extra = engine
        .remember(RememberInput {
            content: "meeting note to delete first".to_string(),
            importance: Some(0.5),
            context: None,
            entity_id: Some("meetings".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    // Delete one memory by ID, which triggers compaction on all CFs
    // (including Temporal), flushing memtable data to SST files.
    let output = engine
        .forget(ForgetCriteria::by_ids(vec![extra.memory_id]))
        .unwrap();
    assert_eq!(output.forgotten_count, 1);

    // Force an additional compaction to guarantee SST flush.
    raw_backend.compact(ColumnFamilyName::Temporal).unwrap();

    // This is the operation that previously returned forgotten_count: 0.
    let output = engine
        .forget(ForgetCriteria::by_entity("user_prefs"))
        .unwrap();
    assert_eq!(
        output.forgotten_count, 6,
        "forget-by-entity must find all 6 memories even after Temporal CF compaction"
    );

    assert_eq!(engine.count().unwrap(), 0);
}

/// Phase 5: forget creates deserializable tombstones in the meta CF.
#[test]
fn forget_creates_tombstones_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let raw_backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let engine = Engine::new(
        Arc::clone(&raw_backend) as Arc<dyn StorageBackend>,
        Arc::new(MockEmbedder::default_dims()),
    )
    .unwrap();

    let mem = engine
        .remember(RememberInput {
            content: "tombstone test memory".to_string(),
            importance: Some(0.5),
            context: None,
            entity_id: Some("tombstone_entity".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    engine
        .forget(ForgetCriteria::by_ids(vec![mem.memory_id.clone()]))
        .unwrap();

    let prefix = tombstone_prefix();
    let entries = raw_backend
        .prefix_iterator(ColumnFamilyName::Meta, &prefix)
        .unwrap();

    assert!(
        !entries.is_empty(),
        "at least one tombstone must exist after forget"
    );

    let mut found_tombstone = false;
    for (_key, value) in &entries {
        if let Ok(ts) = Tombstone::from_bytes(value) {
            if ts.memory_id == mem.memory_id {
                found_tombstone = true;
                assert_eq!(ts.entity_id.as_deref(), Some("tombstone_entity"));
                assert!(ts.forget_timestamp_us > 0);
            }
        }
    }
    assert!(
        found_tombstone,
        "tombstone for the forgotten memory must be present"
    );
}

/// Phase 5: forgetting a revised memory also cascade-deletes its predecessor snapshot.
#[test]
fn forget_cascade_deletes_snapshots_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let raw_backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let engine = Engine::new(
        Arc::clone(&raw_backend) as Arc<dyn StorageBackend>,
        Arc::new(MockEmbedder::default_dims()),
    )
    .unwrap();

    let mem = engine
        .remember(RememberInput {
            content: "cascade test original".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("cascade_entity".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    engine
        .revise(ReviseInput::new_content(
            mem.memory_id.clone(),
            "cascade test revised",
        ))
        .unwrap();

    // Find the snapshot ID via forward graph edge before forget
    let mut mem_id_arr = [0u8; 16];
    mem_id_arr.copy_from_slice(&mem.memory_id);
    let graph_prefix = GraphIndex::encode_forward_prefix(&mem_id_arr);
    let graph_entries = raw_backend
        .prefix_iterator(ColumnFamilyName::Graph, &graph_prefix)
        .unwrap();
    let mut snapshot_id: Option<Vec<u8>> = None;
    for (key, _) in &graph_entries {
        if let Ok((_src, edge_type, target)) = GraphIndex::decode_forward_key(key) {
            if edge_type == EdgeType::RevisedFrom {
                snapshot_id = Some(target.to_vec());
                break;
            }
        }
    }
    let snapshot_id = snapshot_id.expect("snapshot must exist before forget");

    // Forget the memory -- should cascade-delete the snapshot
    let output = engine
        .forget(ForgetCriteria::by_ids(vec![mem.memory_id.clone()]))
        .unwrap();
    assert_eq!(output.forgotten_count, 1);
    assert!(
        output.cascade_count >= 1,
        "cascade should delete the snapshot"
    );

    // Verify the primary memory is gone
    assert!(engine.get(&mem.memory_id).is_err());

    // Verify the snapshot is also gone
    let snapshot_key = keys::encode_memory_key(&snapshot_id);
    let snapshot_val = raw_backend
        .get(ColumnFamilyName::Default, &snapshot_key)
        .unwrap();
    assert!(
        snapshot_val.is_none(),
        "snapshot must be cascade-deleted when the parent memory is forgotten"
    );
}

/// Phase 5: full lifecycle -- remember, revise, forget, verify counts and recall.
#[test]
fn revise_then_forget_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    let mut ids = Vec::new();
    for i in 0..5 {
        let mem = engine
            .remember(RememberInput {
                content: format!("lifecycle memory number {}", i),
                importance: Some(0.6),
                context: None,
                entity_id: Some("lifecycle_entity".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
        ids.push(mem.memory_id);
    }

    // Revise memories 0 and 1
    engine
        .revise(ReviseInput::new_content(
            ids[0].clone(),
            "lifecycle memory zero revised",
        ))
        .unwrap();
    engine
        .revise(ReviseInput::new_content(
            ids[1].clone(),
            "lifecycle memory one revised",
        ))
        .unwrap();

    // Forget memory 0 (revised) and memory 2 (unrevised)
    engine
        .forget(ForgetCriteria::by_ids(vec![ids[0].clone(), ids[2].clone()]))
        .unwrap();

    // 5 original + 2 snapshots from revisions = 7 total.
    // Forget ids[0] cascade-deletes its snapshot → -2.
    // Forget ids[2] (unrevised, no snapshot) → -1.
    // Remaining: ids[1] (revised), ids[3], ids[4], + snapshot of ids[1] = 4.
    assert_eq!(engine.count().unwrap(), 4);

    // Remaining memories: ids[1] (revised), ids[3], ids[4]
    let surviving = engine.get(&ids[1]).unwrap();
    assert_eq!(surviving.content, "lifecycle memory one revised");
    assert!(engine.get(&ids[3]).is_ok());
    assert!(engine.get(&ids[4]).is_ok());

    // Recall should find the surviving memories
    let output = engine
        .recall(RecallInput::new(
            "lifecycle memory",
            RecallStrategy::Similarity,
        ))
        .unwrap();
    assert!(!output.results.is_empty());

    let result_ids: HashSet<Vec<u8>> = output
        .results
        .iter()
        .map(|r| r.memory.memory_id.clone())
        .collect();

    assert!(
        !result_ids.contains(&ids[0]),
        "forgotten memory should not appear in recall"
    );
    assert!(
        !result_ids.contains(&ids[2]),
        "forgotten memory should not appear in recall"
    );
}

/// Phase 5: forgotten memories must not appear in similarity recall results.
#[test]
fn forget_similarity_search_cleanup_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let engine = rocksdb_engine_with_larger_hnsw(dir.path());

    let mut ids = Vec::new();
    for i in 0..20 {
        let mem = engine
            .remember(RememberInput {
                content: format!("search cleanup memory content number {}", i),
                importance: Some(0.5),
                context: None,
                entity_id: Some("cleanup_entity".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
        ids.push(mem.memory_id);
    }

    let forget_ids: Vec<Vec<u8>> = ids[0..5].to_vec();
    let forgotten_set: HashSet<Vec<u8>> = forget_ids.iter().cloned().collect();

    engine.forget(ForgetCriteria::by_ids(forget_ids)).unwrap();

    assert_eq!(engine.count().unwrap(), 15);

    let mut recall_input =
        RecallInput::new("search cleanup memory content", RecallStrategy::Similarity);
    recall_input.top_k = Some(20);

    let output = engine.recall(recall_input).unwrap();
    for result in &output.results {
        assert!(
            !forgotten_set.contains(&result.memory.memory_id),
            "forgotten memory {:?} must not appear in recall results",
            result.memory.memory_id
        );
    }
}

/// Phase 5: concurrent revisions produce no corruption.
#[test]
fn concurrent_revise_no_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let engine = Arc::new(Engine::new(backend, embedder).unwrap());

    let num_threads = 5;
    let writes_per_thread = 20;

    // Each thread remembers its own memories
    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let engine = engine.clone();
            thread::spawn(move || {
                let mut ids = Vec::with_capacity(writes_per_thread);
                for i in 0..writes_per_thread {
                    let mem = engine
                        .remember(RememberInput {
                            content: format!("conc_rev_t{}_m{}_original", t, i),
                            importance: Some(0.5),
                            context: None,
                            entity_id: Some(format!("conc_thread_{}", t)),
                            edges: vec![],
                            kind: None,
                        })
                        .unwrap();
                    ids.push(mem.memory_id);
                }

                for (i, id) in ids.iter().enumerate() {
                    engine
                        .revise(ReviseInput::new_content(
                            id.clone(),
                            format!("conc_rev_t{}_m{}_revised", t, i),
                        ))
                        .unwrap();
                }

                ids
            })
        })
        .collect();

    let all_ids: Vec<Vec<u8>> = handles
        .into_iter()
        .flat_map(|h| h.join().expect("revise thread panicked"))
        .collect();

    assert_eq!(all_ids.len(), num_threads * writes_per_thread);

    for id in &all_ids {
        let mem = engine.get(id).unwrap();
        assert!(
            mem.content.contains("_revised"),
            "memory content should reflect revision: {}",
            mem.content
        );
    }
}

/// Phase 5: tombstone GC collects old tombstones but keeps recent ones.
#[test]
fn tombstone_gc_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    let raw_backend = Arc::new(RocksDbBackend::open(dir.path()).unwrap());
    let engine = Engine::new(
        Arc::clone(&raw_backend) as Arc<dyn StorageBackend>,
        Arc::new(MockEmbedder::default_dims()),
    )
    .unwrap();

    let mem = engine
        .remember(RememberInput {
            content: "gc test memory".to_string(),
            importance: Some(0.5),
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        })
        .unwrap();

    engine
        .forget(ForgetCriteria::by_ids(vec![mem.memory_id.clone()]))
        .unwrap();

    // Insert an artificially old tombstone (timestamp = 0) that should be GC'd
    let old_tombstone = Tombstone {
        memory_id: vec![0u8; 16],
        entity_id: None,
        forget_timestamp_us: 0,
        criteria_description: "synthetic old tombstone".to_string(),
        cascade_count: 0,
        content_hash: vec![],
    };
    let old_ts_key = encode_tombstone_key(0, &[0u8; 16]);
    raw_backend
        .put(
            ColumnFamilyName::Meta,
            &old_ts_key,
            &old_tombstone.to_bytes(),
        )
        .unwrap();

    // Verify both tombstones exist
    let prefix = tombstone_prefix();
    let before = raw_backend
        .prefix_iterator(ColumnFamilyName::Meta, &prefix)
        .unwrap();
    assert!(
        before.len() >= 2,
        "should have at least 2 tombstones before GC, got {}",
        before.len()
    );

    // GC should collect the old tombstone (timestamp 0 is far past the TTL)
    let collected = engine.gc_tombstones().unwrap();
    assert!(
        collected >= 1,
        "gc_tombstones should collect at least the old tombstone, collected {}",
        collected
    );

    // The fresh tombstone from the forget should still be there
    let after = raw_backend
        .prefix_iterator(ColumnFamilyName::Meta, &prefix)
        .unwrap();
    assert!(!after.is_empty(), "recent tombstone should survive GC");
    assert!(
        after.len() < before.len(),
        "GC should have reduced tombstone count: before={}, after={}",
        before.len(),
        after.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Phase 6: Subscribe integration tests
// ═══════════════════════════════════════════════════════════════════════════

fn test_engine() -> Engine {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    Engine::new_with_params(backend, embedder, params, 42).unwrap()
}

fn subscribe_config_for(entity: &str) -> SubscribeConfig {
    SubscribeConfig {
        entity_id: Some(entity.to_string()),
        confidence_threshold: 0.0,
        chunk_min_tokens: 3,
        chunk_max_wait_us: 50_000,
        coarse_threshold: 0.0,
        ..Default::default()
    }
}

#[test]
fn subscribe_basic_lifecycle() {
    let engine = test_engine();

    let contents = [
        "budget pricing negotiation strategy",
        "client budget constraints analysis",
        "pricing model quarterly review",
        "negotiation tactics enterprise deals",
        "budget allocation fiscal planning",
    ];
    for content in &contents {
        engine
            .remember(RememberInput {
                content: content.to_string(),
                importance: Some(0.7),
                context: None,
                entity_id: Some("acme".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let config = subscribe_config_for("acme");
    let mut handle = engine.subscribe(config).unwrap();

    handle.feed("budget pricing negotiation strategy").unwrap();

    let push: SubscribePush = handle
        .recv_timeout(Duration::from_secs(2))
        .expect("should receive at least one push");
    assert!(push.confidence >= 0.0);
    assert!(push.push_timestamp_us > 0);

    handle.close();
}

#[test]
fn subscribe_entity_isolation() {
    let engine = test_engine();

    engine
        .remember(RememberInput {
            content: "alpha project milestone tracking".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("alpha".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    engine
        .remember(RememberInput {
            content: "beta deployment infrastructure setup".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("beta".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let config = subscribe_config_for("alpha");
    let mut handle = engine.subscribe(config).unwrap();

    handle.feed("beta deployment infrastructure setup").unwrap();
    let push = handle.recv_timeout(Duration::from_millis(500));
    assert!(
        push.is_none(),
        "should not receive push for beta content on alpha subscription"
    );

    handle.feed("alpha project milestone tracking").unwrap();
    let push = handle.recv_timeout(Duration::from_secs(2));
    assert!(push.is_some(), "should receive push for alpha content");

    handle.close();
}

#[test]
fn subscribe_dedup_prevents_repeated_push() {
    let engine = test_engine();

    engine
        .remember(RememberInput {
            content: "quarterly revenue forecast analysis".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("acme".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let config = subscribe_config_for("acme");
    let mut handle = engine.subscribe(config).unwrap();

    handle.feed("quarterly revenue forecast analysis").unwrap();
    let push1 = handle.recv_timeout(Duration::from_secs(2));
    assert!(push1.is_some(), "first feed should produce a push");

    handle.feed("quarterly revenue forecast analysis").unwrap();
    let push2 = handle.recv_timeout(Duration::from_millis(500));
    assert!(
        push2.is_none(),
        "second feed of same text should be deduped"
    );

    handle.reset_dedup();
    thread::sleep(Duration::from_millis(200));

    handle.feed("quarterly revenue forecast analysis").unwrap();
    let push3 = handle.recv_timeout(Duration::from_secs(2));
    assert!(push3.is_some(), "should receive push after reset_dedup");

    handle.close();
}

#[test]
fn subscribe_close_is_clean() {
    let engine = test_engine();

    engine
        .remember(RememberInput {
            content: "contract negotiation terms review".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("acme".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let config = subscribe_config_for("acme");
    let mut handle = engine.subscribe(config).unwrap();

    handle.feed("contract negotiation terms review").unwrap();
    handle.close();

    let result = handle.feed("more text here now");
    assert!(result.is_err(), "feed after close should return error");
}

#[test]
fn subscribe_pause_resume() {
    let engine = test_engine();

    engine
        .remember(RememberInput {
            content: "pipeline conversion rate optimization".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("acme".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let config = subscribe_config_for("acme");
    let mut handle = engine.subscribe(config).unwrap();

    handle.pause();
    thread::sleep(Duration::from_millis(200));

    handle
        .feed("pipeline conversion rate optimization")
        .unwrap();
    let push = handle.recv_timeout(Duration::from_millis(500));
    assert!(push.is_none(), "should not receive push while paused");

    handle.resume();
    let push = handle.recv_timeout(Duration::from_secs(2));
    assert!(push.is_some(), "should receive push after resume");

    handle.close();
}

#[test]
fn subscribe_multiple_concurrent() {
    let engine = test_engine();

    let entities = ["ent_one", "ent_two", "ent_three"];
    let contents = [
        "hardware procurement logistics management",
        "software development lifecycle planning",
        "customer retention strategy analysis",
    ];

    for (entity, content) in entities.iter().zip(contents.iter()) {
        engine
            .remember(RememberInput {
                content: content.to_string(),
                importance: Some(0.7),
                context: None,
                entity_id: Some(entity.to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let mut handles: Vec<_> = entities
        .iter()
        .map(|entity| engine.subscribe(subscribe_config_for(entity)).unwrap())
        .collect();

    for (handle, content) in handles.iter().zip(contents.iter()) {
        handle.feed(*content).unwrap();
    }

    for (i, handle) in handles.iter().enumerate() {
        let push = handle.recv_timeout(Duration::from_secs(2));
        assert!(push.is_some(), "subscription {} should receive a push", i);
        let push = push.unwrap();
        assert_eq!(
            push.memory.entity_id.as_deref(),
            Some(entities[i]),
            "push should be for entity {}",
            entities[i],
        );
    }

    for handle in &mut handles {
        handle.close();
    }
}

#[test]
fn subscribe_new_write_during_subscription() {
    let engine = test_engine();

    engine
        .remember(RememberInput {
            content: "strategic account planning review".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("acme".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let mut config = subscribe_config_for("acme");
    config.bloom_refresh_write_count = 1;
    let mut handle = engine.subscribe(config).unwrap();

    handle.feed("strategic account planning review").unwrap();
    let push = handle.recv_timeout(Duration::from_secs(2));
    assert!(push.is_some(), "should get initial push");

    engine
        .remember(RememberInput {
            content: "strategic account planning review".to_string(),
            importance: Some(0.9),
            context: None,
            entity_id: Some("acme".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let push = handle.recv_timeout(Duration::from_secs(2));
    assert!(
        push.is_some(),
        "should receive push for newly written memory"
    );

    handle.close();
}

#[test]
fn subscribe_stats_track_chunks() {
    let engine = test_engine();

    engine
        .remember(RememberInput {
            content: "market segmentation targeting positioning".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("acme".to_string()),
            edges: vec![],
            kind: None,
        })
        .unwrap();

    let config = subscribe_config_for("acme");
    let mut handle = engine.subscribe(config).unwrap();

    for _ in 0..3 {
        handle
            .feed("market segmentation targeting positioning")
            .unwrap();
        thread::sleep(Duration::from_millis(200));
    }

    thread::sleep(Duration::from_millis(500));

    let stats = handle.stats();
    assert!(
        stats.chunks_processed > 0,
        "chunks_processed should be > 0, got {}",
        stats.chunks_processed,
    );

    handle.close();
}

#[test]
fn subscribe_backpressure_drops_oldest() {
    let engine = test_engine();

    for i in 0..20 {
        engine
            .remember(RememberInput {
                content: format!("backpressure test memory content item {}", i),
                importance: Some(0.7),
                context: None,
                entity_id: Some("acme".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let config = SubscribeConfig {
        entity_id: Some("acme".to_string()),
        confidence_threshold: 0.0,
        chunk_min_tokens: 3,
        chunk_max_wait_us: 50_000,
        coarse_threshold: 0.0,
        output_queue_depth: 10,
        hnsw_top_k: 1,
        ..Default::default()
    };
    let mut handle = engine.subscribe(config).unwrap();

    for i in 0..20 {
        handle
            .feed(format!("backpressure test memory content item {}", i))
            .unwrap();
        thread::sleep(Duration::from_millis(200));
    }

    thread::sleep(Duration::from_secs(1));

    let stats = handle.stats();
    assert!(
        stats.pushes_dropped > 0,
        "should have dropped pushes with small output queue, pushes_sent={}, pushes_dropped={}",
        stats.pushes_sent,
        stats.pushes_dropped,
    );

    handle.close();
}

// ─── Phase 7: Reflection Pipeline Integration Tests ──────────────

fn in_memory_engine() -> Engine {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    Engine::new_with_params(backend, embedder, params, 42).unwrap()
}

fn populate_memories(engine: &Engine, entity: &str, count: usize) -> Vec<Vec<u8>> {
    let mut ids = Vec::new();
    for i in 0..count {
        let mem = engine
            .remember(RememberInput {
                content: format!(
                    "Customer {} mentioned topic {} during call {} with specific detail {}",
                    entity,
                    i % 5,
                    i,
                    i * 7
                ),
                importance: Some(0.5 + (i % 10) as f32 * 0.05),
                context: None,
                entity_id: Some(entity.into()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
        ids.push(mem.memory_id.clone());
    }
    ids
}

/// reflect() produces insights from a batch of episode memories.
#[test]
fn reflect_produces_insights() {
    let engine = in_memory_engine();
    populate_memories(&engine, "customer_001", 30);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();
    let scope = ReflectScope::Global { since_us: None };

    let output = engine.reflect(scope, &config, &mock, &mock).unwrap();

    assert!(
        output.insights_created > 0,
        "expected insights, got 0. clusters_found={}",
        output.clusters_found
    );
    assert!(output.clusters_found > 0);
    assert_eq!(output.memories_processed, 30);
}

/// insights() returns stored Insight memories after reflect().
#[test]
fn insights_query_returns_results() {
    let engine = in_memory_engine();
    populate_memories(&engine, "customer_002", 30);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();
    let scope = ReflectScope::Global { since_us: None };

    engine.reflect(scope, &config, &mock, &mock).unwrap();

    let insights = engine
        .insights(InsightsFilter {
            entity_id: None,
            min_confidence: None,
            max_results: Some(100),
        })
        .unwrap();

    assert!(
        !insights.is_empty(),
        "expected insights from insights() query"
    );
    for insight in &insights {
        assert_eq!(insight.kind, MemoryKind::Insight);
        assert!(!insight.content.is_empty());
        assert!(insight.importance > 0.0);
    }
}

/// Insights have InsightFrom graph edges linking to source memories.
#[test]
fn insights_have_lineage_edges() {
    let engine = in_memory_engine();
    populate_memories(&engine, "customer_003", 30);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();

    engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    let insights = engine.insights(InsightsFilter::default()).unwrap();

    assert!(!insights.is_empty());

    let mut total_edges = 0;
    for insight in &insights {
        let mut id = [0u8; 16];
        id.copy_from_slice(&insight.memory_id);
        let outgoing = engine.outgoing_edges(&id).unwrap_or_default();
        let insight_from_edges: Vec<_> = outgoing
            .iter()
            .filter(|(et, _, _)| *et == EdgeType::InsightFrom)
            .collect();
        total_edges += insight_from_edges.len();
    }

    assert!(
        total_edges > 0,
        "insights must have InsightFrom edges to source memories"
    );
}

/// Entity-scoped reflect only considers memories from that entity.
#[test]
fn reflect_entity_scoped() {
    let engine = in_memory_engine();
    populate_memories(&engine, "entity_alpha", 20);
    populate_memories(&engine, "entity_beta", 20);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();

    let output = engine
        .reflect(
            ReflectScope::Entity {
                entity_id: "entity_alpha".into(),
                since_us: None,
            },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    assert_eq!(output.memories_processed, 20);
}

/// Incremental reflect: cursor prevents re-processing.
#[test]
fn reflect_incremental_cursor() {
    let engine = in_memory_engine();
    populate_memories(&engine, "inc_entity", 20);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();
    let scope = ReflectScope::Global { since_us: None };

    let first = engine
        .reflect(scope.clone(), &config, &mock, &mock)
        .unwrap();
    assert!(first.insights_created > 0);

    let second = engine.reflect(scope, &config, &mock, &mock).unwrap();

    assert!(
        second.memories_processed <= first.memories_processed,
        "second run should not re-process all memories: first={}, second={}",
        first.memories_processed,
        second.memories_processed,
    );
}

/// reflect() with too few memories returns gracefully with zero insights.
#[test]
fn reflect_insufficient_memories() {
    let engine = in_memory_engine();
    populate_memories(&engine, "sparse", 3);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig {
        min_memories_for_reflect: 10,
        ..Default::default()
    };

    let output = engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    assert_eq!(output.insights_created, 0);
}

/// Insights are discoverable via similarity recall.
#[test]
fn insights_are_recallable() {
    let engine = in_memory_engine();
    populate_memories(&engine, "recall_test", 30);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();

    engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    let insights = engine.insights(InsightsFilter::default()).unwrap();
    assert!(!insights.is_empty());

    let recall_output = engine
        .recall(RecallInput {
            cue: "consolidated insight".into(),
            strategies: vec![RecallStrategy::Similarity],
            top_k: Some(50),
            entity_id: None,
            time_range: None,
            edge_types: None,
            max_depth: None,
            ef_search: None,
            scoring_weights: None,
            cue_context: None,
            causal_direction: None,
            analogy_a_id: None,
            analogy_b_id: None,
            seed_memory_id: None,
            analogical_alpha: None,
        })
        .unwrap();

    let recalled_kinds: Vec<_> = recall_output
        .results
        .iter()
        .map(|r| r.memory.kind)
        .collect();
    assert!(
        recalled_kinds.contains(&MemoryKind::Insight),
        "similarity recall should find insights among results"
    );
}

/// revise() on a source memory triggers stale insight marking.
#[test]
fn revise_triggers_insight_invalidation() {
    let engine = in_memory_engine();
    let mem_ids = populate_memories(&engine, "stale_test", 30);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();

    engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    // Revise a source memory -- should not panic even if the source
    // has InsightFrom incoming edges.
    engine
        .revise(ReviseInput {
            memory_id: mem_ids[0].clone(),
            content: Some("completely different content after revision".into()),
            importance: None,
            context: None,
            context_mode: hebbs_core::revise::ContextMode::Merge,
            entity_id: None,
            edges: vec![],
        })
        .unwrap();
}

/// start_reflect/stop_reflect lifecycle runs without panics or hangs.
#[test]
fn reflect_worker_lifecycle() {
    let engine = in_memory_engine();
    populate_memories(&engine, "worker_test", 20);

    let config = ReflectConfig {
        trigger_check_interval_us: 100_000,
        enabled: true,
        ..Default::default()
    };
    engine.start_reflect(config);

    thread::sleep(Duration::from_millis(50));

    engine.pause_reflect();
    engine.resume_reflect();

    thread::sleep(Duration::from_millis(50));

    engine.stop_reflect();
}

/// Concurrent reflect() calls don't panic or corrupt state.
#[test]
fn reflect_concurrent_safety() {
    let engine = Arc::new(in_memory_engine());
    populate_memories(&engine, "concurrent", 30);

    let mock = Arc::new(MockLlmProvider::new());
    let config = Arc::new(ReflectConfig::default());

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let eng = engine.clone();
            let m = mock.clone();
            let c = config.clone();
            thread::spawn(move || {
                eng.reflect(
                    ReflectScope::Global { since_us: None },
                    &c,
                    m.as_ref(),
                    m.as_ref(),
                )
                .unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let insights = engine.insights(InsightsFilter::default()).unwrap();
    assert!(!insights.is_empty());
}

/// forget() on a source memory triggers stale marking for derived insights.
#[test]
fn forget_triggers_insight_invalidation() {
    let engine = in_memory_engine();
    let mem_ids = populate_memories(&engine, "forget_stale", 30);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();

    engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    let insights_before = engine.insights(InsightsFilter::default()).unwrap();
    assert!(!insights_before.is_empty());

    // Forget a source memory -- should not panic.
    engine
        .forget(ForgetCriteria::by_ids(vec![mem_ids[0].clone()]))
        .unwrap();
}

/// insights() with min_confidence filter works.
#[test]
fn insights_filter_by_confidence() {
    let engine = in_memory_engine();
    populate_memories(&engine, "conf_filter", 30);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();

    engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    let high_conf = engine
        .insights(InsightsFilter {
            min_confidence: Some(0.99),
            ..Default::default()
        })
        .unwrap();

    let all = engine.insights(InsightsFilter::default()).unwrap();

    assert!(
        high_conf.len() <= all.len(),
        "high confidence filter should return fewer or equal results"
    );
}

/// insights() with max_results pagination works.
#[test]
fn insights_filter_max_results() {
    let engine = in_memory_engine();
    populate_memories(&engine, "paginate", 50);

    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();

    engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    let limited = engine
        .insights(InsightsFilter {
            max_results: Some(1),
            ..Default::default()
        })
        .unwrap();

    assert!(limited.len() <= 1);
}
