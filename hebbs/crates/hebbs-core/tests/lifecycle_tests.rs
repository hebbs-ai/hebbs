//! Tests for Engine lifecycle: Weak references in workers, vault eviction,
//! RocksDB lock release, and concurrent access during shutdown.
//!
//! These tests verify that the fix for the RocksDB lock lifecycle bug works:
//! background workers (decay, reflect) hold Weak<dyn StorageBackend> instead
//! of Arc, so they cannot prevent the Engine from being dropped and the
//! RocksDB LOCK file from being released.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hebbs_core::decay::DecayConfig;
use hebbs_core::engine::{Engine, RememberInput};
use hebbs_core::recall::{RecallInput, RecallStrategy};
use hebbs_embed::MockEmbedder;
use hebbs_index::HnswParams;
use hebbs_storage::{InMemoryBackend, RocksDbBackend};

fn remember_input(content: &str) -> RememberInput {
    RememberInput {
        content: content.to_string(),
        importance: Some(0.5),
        context: None,
        entity_id: None,
        edges: vec![],
        kind: None,
    }
}

fn decay_config_fast() -> DecayConfig {
    DecayConfig {
        sweep_interval_us: 1_000_000, // 1 second (minimum)
        enabled: true,
        ..DecayConfig::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test A: shutdown() releases worker Arc references
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn shutdown_releases_worker_arcs() {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let engine = Arc::new(
        Engine::new_with_params(backend.clone(), embedder, HnswParams::with_m(384, 4), 42).unwrap(),
    );

    // Before starting workers: only engine holds storage via Arc<Engine>
    let initial_engine_count = Arc::strong_count(&engine);
    assert_eq!(initial_engine_count, 1, "only one Arc<Engine> should exist");

    // Start decay worker (worker holds Weak<Storage>, not Arc)
    engine.start_decay(decay_config_fast());

    // The Engine Arc count should still be 1 because the worker holds Weak, not Arc
    assert_eq!(
        Arc::strong_count(&engine),
        1,
        "decay worker should not hold Arc<Engine>"
    );

    // Give worker time to start
    thread::sleep(Duration::from_millis(100));

    // Shutdown stops workers and joins threads
    engine.shutdown();

    // After shutdown, Engine Arc count is still 1
    assert_eq!(
        Arc::strong_count(&engine),
        1,
        "after shutdown, no background workers should hold references"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test B: Workers exit naturally when Engine is dropped (no explicit shutdown)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workers_exit_on_engine_drop_without_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("db");

    // Open RocksDB, create engine, start workers
    {
        let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        engine.start_decay(decay_config_fast());
        engine.remember(remember_input("test memory")).unwrap();

        // Drop engine WITHOUT calling shutdown()
        // Workers hold Weak<Storage>, so they cannot prevent Storage from dropping.
        // Engine::drop() calls stop_decay() + stop_reflect() as defense-in-depth.
    }

    // If Weak refs work correctly, RocksDB LOCK is released.
    // Reopening should succeed.
    let backend2 = RocksDbBackend::open(&db_path);
    assert!(
        backend2.is_ok(),
        "RocksDB should reopen after Engine drop without explicit shutdown: {:?}",
        backend2.err()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test C: RocksDB reopens after engine drop, data persists
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rocksdb_reopens_after_engine_drop_data_persists() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("db");

    // Phase 1: create engine, store data, start workers, drop
    {
        let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        engine.start_decay(decay_config_fast());

        for i in 0..10 {
            engine
                .remember(remember_input(&format!("memory {}", i)))
                .unwrap();
        }

        assert_eq!(engine.count().unwrap(), 10);

        // Explicit shutdown for clean exit
        engine.shutdown();
    }

    // Phase 2: reopen, verify all data persists
    {
        let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        assert_eq!(
            engine.count().unwrap(),
            10,
            "all 10 memories should persist across restart"
        );

        // Recall should work on reopened engine
        let results = engine
            .recall(RecallInput {
                cue: "memory".to_string(),
                strategies: vec![RecallStrategy::Similarity],
                top_k: Some(5),
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

        assert!(
            !results.results.is_empty(),
            "recall should return results after reopen"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test D: Full eviction lifecycle (open → workers → evict → reopen → recall)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn vault_evict_and_reopen_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("db");

    // Phase 1: open with workers, store data
    {
        let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Arc::new(Engine::new(backend, embedder).unwrap());

        engine.start_decay(decay_config_fast());

        engine
            .remember(remember_input("eviction test fact"))
            .unwrap();

        // Simulate eviction: shutdown then drop (matches VaultManager behavior)
        engine.shutdown();
        // engine Arc dropped here
    }

    // Phase 2: reopen with fresh workers (matches get_or_open behavior)
    {
        let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Arc::new(Engine::new(backend, embedder).unwrap());

        engine.start_decay(decay_config_fast());

        // Verify data survives eviction + reopen
        assert_eq!(engine.count().unwrap(), 1);

        let results = engine
            .recall(RecallInput {
                cue: "eviction test".to_string(),
                strategies: vec![RecallStrategy::Similarity],
                top_k: Some(5),
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

        assert!(
            !results.results.is_empty(),
            "recall should find data after eviction + reopen"
        );

        engine.shutdown();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test E: shutdown() is idempotent (safe to call multiple times)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn shutdown_idempotent() {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let engine = Engine::new(backend, embedder).unwrap();

    engine.start_decay(decay_config_fast());

    // First shutdown
    engine.shutdown();

    // Second shutdown (should not panic or deadlock)
    engine.shutdown();

    // Third for good measure
    engine.shutdown();
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test F: Concurrent recall during shutdown (no deadlock)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_recall_during_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("db");

    let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let engine = Arc::new(Engine::new(backend, embedder).unwrap());

    engine.start_decay(decay_config_fast());

    // Pre-populate with data
    for i in 0..50 {
        engine
            .remember(remember_input(&format!("concurrent test {}", i)))
            .unwrap();
    }

    // Spawn recall threads
    let engine_clone = engine.clone();
    let recall_handle = thread::spawn(move || {
        let mut successes = 0;
        let mut errors = 0;
        for _ in 0..20 {
            match engine_clone.recall(RecallInput {
                cue: "concurrent test".to_string(),
                strategies: vec![RecallStrategy::Similarity],
                top_k: Some(5),
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
            }) {
                Ok(_) => successes += 1,
                Err(_) => errors += 1,
            }
        }
        (successes, errors)
    });

    // Give recalls a head start
    thread::sleep(Duration::from_millis(10));

    // Shutdown from main thread while recalls are in progress
    engine.shutdown();

    // Recalls must complete (no deadlock). Some may error, that's fine.
    let (successes, errors) = recall_handle
        .join()
        .expect("recall thread should not panic");
    assert!(
        successes + errors == 20,
        "all recall attempts should complete (got {} successes, {} errors)",
        successes,
        errors
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test G: Multiple open-close cycles on same RocksDB path
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_open_close_cycles() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("db");

    for cycle in 0..5 {
        let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        engine.start_decay(decay_config_fast());

        engine
            .remember(remember_input(&format!("cycle {} memory", cycle)))
            .unwrap();

        assert_eq!(
            engine.count().unwrap(),
            (cycle + 1) as usize,
            "cycle {}: should have {} memories",
            cycle,
            cycle + 1
        );

        engine.shutdown();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test H: Drop without shutdown on multiple engines in sequence
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sequential_engine_drops_without_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("db");

    for cycle in 0..3 {
        let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let engine = Engine::new(backend, embedder).unwrap();

        engine.start_decay(decay_config_fast());

        engine
            .remember(remember_input(&format!("no-shutdown cycle {}", cycle)))
            .unwrap();

        // Drop without shutdown. Weak refs ensure lock is released.
        // Engine::drop() calls stop_decay() as defense-in-depth.
    }

    // Final open to verify all data persists
    let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let engine = Engine::new(backend, embedder).unwrap();
    assert_eq!(
        engine.count().unwrap(),
        3,
        "all 3 memories should persist across drop-without-shutdown cycles"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test I: Concurrent remember + recall + shutdown
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_remember_recall_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("db");

    let backend = Arc::new(RocksDbBackend::open(&db_path).unwrap());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let engine = Arc::new(Engine::new(backend, embedder).unwrap());

    engine.start_decay(decay_config_fast());

    // Writer thread
    let engine_w = engine.clone();
    let writer = thread::spawn(move || {
        for i in 0..30 {
            let _ = engine_w.remember(remember_input(&format!("concurrent write {}", i)));
            thread::sleep(Duration::from_millis(1));
        }
    });

    // Reader thread
    let engine_r = engine.clone();
    let reader = thread::spawn(move || {
        for _ in 0..30 {
            let _ = engine_r.recall(RecallInput {
                cue: "concurrent write".to_string(),
                strategies: vec![RecallStrategy::Similarity],
                top_k: Some(3),
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
            });
            thread::sleep(Duration::from_millis(1));
        }
    });

    // Let them run briefly then shutdown
    thread::sleep(Duration::from_millis(15));
    engine.shutdown();

    // Both threads must complete without panic
    writer.join().expect("writer should not panic");
    reader.join().expect("reader should not panic");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Test J: Engine with decay running, storage Arc refcount stays correct
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn storage_arc_refcount_with_decay_worker() {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());

    // Engine + IndexManager + other internals hold Arc<Storage> clones
    let engine = Engine::new(backend.clone(), embedder).unwrap();
    let before_decay = Arc::strong_count(&backend);

    // Start decay worker (uses Weak, should NOT increase strong count)
    engine.start_decay(decay_config_fast());
    thread::sleep(Duration::from_millis(100));

    assert_eq!(
        Arc::strong_count(&backend),
        before_decay,
        "decay worker should not increase storage refcount (uses Weak)"
    );

    engine.shutdown();

    assert_eq!(
        Arc::strong_count(&backend),
        before_decay,
        "after shutdown: refcount unchanged"
    );

    drop(engine);

    assert_eq!(
        Arc::strong_count(&backend),
        1,
        "after engine drop: only the test's backend ref remains"
    );
}
