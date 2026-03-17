use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use hebbs_core::decay::compute_decay_score;
use hebbs_core::engine::{Engine, RememberEdge, RememberInput};
use hebbs_core::forget::{ForgetCriteria, Tombstone};
use hebbs_core::memory::Memory;
use hebbs_core::recall::{PrimeInput, RecallInput, RecallStrategy, ScoringWeights};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig, ReflectScope};
use hebbs_core::revise::ReviseInput;
use hebbs_core::subscribe::SubscribeConfig;
use hebbs_embed::MockEmbedder;
use hebbs_index::{EdgeType, HnswParams};
use hebbs_reflect::MockLlmProvider;
use hebbs_storage::InMemoryBackend;

fn create_engine() -> Engine {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = hebbs_index::HnswParams::with_m(384, 4);
    Engine::new_with_params(backend, embedder, params, 42).unwrap()
}

fn create_engine_with_memories(n: usize) -> Engine {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 16);
    let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

    for i in 0..n {
        engine
            .remember(RememberInput {
                content: format!(
                    "benchmark memory content number {} with some padding text",
                    i
                ),
                importance: Some(0.3 + (i as f32 % 7.0) / 10.0),
                context: None,
                entity_id: Some(format!("entity_{}", i % 20)),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }
    engine
}

fn bench_remember_single(c: &mut Criterion) {
    let engine = create_engine();

    c.bench_function("remember_single_200B", |b| {
        let content = "x".repeat(200);
        b.iter(|| {
            engine
                .remember(black_box(RememberInput {
                    content: content.clone(),
                    importance: Some(0.7),
                    context: None,
                    entity_id: Some("bench_entity".to_string()),
                    edges: vec![],
            kind: None,
        }))
                .unwrap();
        });
    });
}

fn bench_remember_with_context(c: &mut Criterion) {
    let engine = create_engine();

    c.bench_function("remember_with_context", |b| {
        let mut ctx = HashMap::new();
        ctx.insert("stage".to_string(), serde_json::json!("discovery"));
        ctx.insert(
            "tags".to_string(),
            serde_json::json!(["urgent", "follow-up"]),
        );
        ctx.insert("score".to_string(), serde_json::json!(85));

        b.iter(|| {
            engine
                .remember(black_box(RememberInput {
                    content: "Customer expressed urgency about Q4 deadline and needs pricing"
                        .to_string(),
                    importance: Some(0.8),
                    context: Some(ctx.clone()),
                    entity_id: Some("customer_bench".to_string()),
                    edges: vec![],
            kind: None,
        }))
                .unwrap();
        });
    });
}

fn bench_get_single(c: &mut Criterion) {
    let engine = create_engine();
    let memory = engine
        .remember(RememberInput {
            content: "x".repeat(200),
            importance: Some(0.7),
            context: None,
            entity_id: None,
            edges: vec![],
            kind: None,
        })
        .unwrap();
    let id = memory.memory_id.clone();

    c.bench_function("get_single", |b| {
        b.iter(|| {
            engine.get(black_box(&id)).unwrap();
        });
    });
}

fn bench_get_miss(c: &mut Criterion) {
    let engine = create_engine();
    // Populate with some data so bloom filter is exercised
    for i in 0..1000 {
        engine
            .remember(RememberInput {
                content: format!("memory {}", i),
                importance: None,
                context: None,
                entity_id: None,
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let nonexistent_id = [0xFFu8; 16];
    c.bench_function("get_miss", |b| {
        b.iter(|| {
            let _ = engine.get(black_box(&nonexistent_id));
        });
    });
}

fn bench_remember_batch(c: &mut Criterion) {
    c.bench_function("remember_batch_10000", |b| {
        b.iter(|| {
            let engine = create_engine();
            for i in 0..10_000 {
                engine
                    .remember(RememberInput {
                        content: format!("batch memory number {}", i),
                        importance: Some(0.5),
                        context: None,
                        entity_id: Some("batch_entity".to_string()),
                        edges: vec![],
                        kind: None,
                    })
                    .unwrap();
            }
        });
    });
}

fn bench_serialization(c: &mut Criterion) {
    let mut ctx = HashMap::new();
    ctx.insert("stage".to_string(), serde_json::json!("discovery"));
    let context_bytes = Memory::serialize_context(&ctx).unwrap();

    let memory = Memory {
        memory_id: vec![42u8; 16],
        content: "Customer expressed urgency about Q4 deadline".to_string(),
        importance: 0.8,
        context_bytes,
        entity_id: Some("customer_123".to_string()),
        embedding: None,
        created_at: 1_700_000_000_000_000,
        updated_at: 1_700_000_000_000_000,
        last_accessed_at: 1_700_000_000_000_000,
        access_count: 0,
        decay_score: 0.8,
        kind: hebbs_core::memory::MemoryKind::Episode,
        device_id: None,
        logical_clock: 0,
        associative_embedding: None,
    };

    let bytes = memory.to_bytes();

    c.bench_function("serialize_memory", |b| {
        b.iter(|| {
            black_box(&memory).to_bytes();
        });
    });

    c.bench_function("deserialize_memory", |b| {
        b.iter(|| {
            Memory::from_bytes(black_box(&bytes)).unwrap();
        });
    });
}

fn bench_delete(c: &mut Criterion) {
    let engine = create_engine();

    c.bench_function("delete_single", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for _ in 0..iters {
                let mem = engine
                    .remember(RememberInput {
                        content: "to be deleted".to_string(),
                        importance: None,
                        context: None,
                        entity_id: None,
                        edges: vec![],
            kind: None,
        })
                    .unwrap();
                let start = std::time::Instant::now();
                engine.delete(&mem.memory_id).unwrap();
                total += start.elapsed();
            }
            total
        });
    });
}

// ═══════════════════════════════════════════════════════════════════
//  Phase 4: Recall Engine Benchmarks
// ═══════════════════════════════════════════════════════════════════

fn bench_recall_similarity(c: &mut Criterion) {
    let mut group = c.benchmark_group("recall_similarity");
    group.sample_size(20);

    for &size in &[1_000, 10_000] {
        let engine = create_engine_with_memories(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                engine
                    .recall(black_box(RecallInput::new(
                        "benchmark memory content",
                        RecallStrategy::Similarity,
                    )))
                    .unwrap();
            });
        });
    }
    group.finish();
}

fn bench_recall_temporal(c: &mut Criterion) {
    let mut group = c.benchmark_group("recall_temporal");
    group.sample_size(20);

    for &size in &[1_000, 10_000] {
        let engine = create_engine_with_memories(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            let mut input = RecallInput::new("events", RecallStrategy::Temporal);
            input.entity_id = Some("entity_0".to_string());
            input.top_k = Some(10);
            b.iter(|| {
                engine
                    .recall(black_box(RecallInput {
                        cue: "events".to_string(),
                        strategies: vec![RecallStrategy::Temporal],
                        top_k: Some(10),
                        entity_id: Some("entity_0".to_string()),
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
                    }))
                    .unwrap();
            });
        });
    }
    group.finish();
}

fn bench_recall_causal(c: &mut Criterion) {
    let mut group = c.benchmark_group("recall_causal");
    group.sample_size(20);

    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 16);
    let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

    // Build a chain of 100 memories with edges
    let mut prev_id = None;
    for i in 0..100 {
        let mut edges = Vec::new();
        if let Some(pid) = prev_id {
            edges.push(RememberEdge {
                target_id: pid,
                edge_type: EdgeType::FollowedBy,
                confidence: Some(0.9),
            });
        }
        let mem = engine
            .remember(RememberInput {
                content: format!("causal chain memory {}", i),
                importance: Some(0.6),
                context: None,
                entity_id: Some("causal_entity".to_string()),
                edges,
            })
            .unwrap();
        let mut id = [0u8; 16];
        id.copy_from_slice(&mem.memory_id);
        prev_id = Some(id);
    }

    let seed_hex = hex::encode(prev_id.unwrap());

    group.bench_function("100_edges_depth_5", |b| {
        b.iter(|| {
            let mut input = RecallInput::new(seed_hex.clone(), RecallStrategy::Causal);
            input.max_depth = Some(5);
            engine.recall(black_box(input)).unwrap();
        });
    });
    group.finish();
}

fn bench_recall_analogical(c: &mut Criterion) {
    let mut group = c.benchmark_group("recall_analogical");
    group.sample_size(20);

    let engine = create_engine_with_memories(1_000);

    let mut cue_ctx = HashMap::new();
    cue_ctx.insert("stage".to_string(), serde_json::json!("discovery"));
    cue_ctx.insert("outcome".to_string(), serde_json::json!("positive"));

    group.bench_function("1000_memories", |b| {
        b.iter(|| {
            let mut input =
                RecallInput::new("benchmark memory content", RecallStrategy::Analogical);
            input.cue_context = Some(cue_ctx.clone());
            engine.recall(black_box(input)).unwrap();
        });
    });
    group.finish();
}

fn bench_recall_multi_strategy(c: &mut Criterion) {
    let mut group = c.benchmark_group("recall_multi_strategy");
    group.sample_size(20);

    for &size in &[1_000, 10_000] {
        let engine = create_engine_with_memories(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let mut input = RecallInput::multi(
                    "benchmark memory content",
                    vec![RecallStrategy::Similarity, RecallStrategy::Temporal],
                );
                input.entity_id = Some("entity_0".to_string());
                engine.recall(black_box(input)).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_prime(c: &mut Criterion) {
    let mut group = c.benchmark_group("prime");
    group.sample_size(20);

    for &size in &[1_000, 10_000] {
        let engine = create_engine_with_memories(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                engine
                    .prime(black_box(PrimeInput::new("entity_0")))
                    .unwrap();
            });
        });
    }
    group.finish();
}

fn bench_composite_scoring(c: &mut Criterion) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;

    let weights = ScoringWeights::default();
    let memories: Vec<Memory> = (0..100)
        .map(|i| Memory {
            memory_id: vec![i as u8; 16],
            content: format!("memory {}", i),
            importance: 0.5 + (i as f32 % 5.0) / 10.0,
            context_bytes: Vec::new(),
            entity_id: None,
            embedding: None,
            created_at: now - (i as u64 * 1_000_000),
            updated_at: 0,
            last_accessed_at: now - (i as u64 * 500_000),
            access_count: i as u64,
            decay_score: 0.5,
            kind: hebbs_core::memory::MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        })
        .collect();

    c.bench_function("composite_scoring_100_memories", |b| {
        b.iter(|| {
            for mem in &memories {
                black_box(hebbs_core::engine::compute_composite_score(
                    0.8, mem, &weights, now,
                ));
            }
        });
    });
}

fn bench_reinforcement_overhead(c: &mut Criterion) {
    let engine = create_engine_with_memories(1_000);

    c.bench_function("recall_with_reinforcement_1k", |b| {
        b.iter(|| {
            engine
                .recall(black_box(RecallInput::new(
                    "benchmark memory content",
                    RecallStrategy::Similarity,
                )))
                .unwrap();
        });
    });
}

// ═══════════════════════════════════════════════════════════════════
//  Phase 5: Revise, Forget, Decay Benchmarks
// ═══════════════════════════════════════════════════════════════════

fn bench_revise_content(c: &mut Criterion) {
    let engine = create_engine_with_memories(1_000);

    c.bench_function("revise_content", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for i in 0..iters {
                let mem = engine
                    .remember(RememberInput {
                        content: format!("to be revised {}", i),
                        importance: Some(0.7),
                        context: None,
                        entity_id: Some("revise_bench".to_string()),
                        edges: vec![],
                        kind: None,
                    })
                    .unwrap();
                let start = std::time::Instant::now();
                engine
                    .revise(black_box(ReviseInput::new_content(
                        mem.memory_id,
                        format!("revised content {}", i),
                    )))
                    .unwrap();
                total += start.elapsed();
            }
            total
        });
    });
}

fn bench_forget_by_id(c: &mut Criterion) {
    let engine = create_engine_with_memories(1_000);

    c.bench_function("forget_by_id", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for i in 0..iters {
                let mem = engine
                    .remember(RememberInput {
                        content: format!("to be forgotten {}", i),
                        importance: Some(0.5),
                        context: None,
                        entity_id: Some("forget_bench".to_string()),
                        edges: vec![],
                        kind: None,
                    })
                    .unwrap();
                let start = std::time::Instant::now();
                engine
                    .forget(black_box(ForgetCriteria::by_ids(vec![mem.memory_id])))
                    .unwrap();
                total += start.elapsed();
            }
            total
        });
    });
}

fn bench_forget_by_entity(c: &mut Criterion) {
    c.bench_function("forget_by_entity_100", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for iter_idx in 0..iters {
                let engine = create_engine();
                let entity = format!("forget_entity_{}", iter_idx);
                for i in 0..100 {
                    engine
                        .remember(RememberInput {
                            content: format!("entity memory {} batch {}", i, iter_idx),
                            importance: Some(0.5),
                            context: None,
                            entity_id: Some(entity.clone()),
                            edges: vec![],
                            kind: None,
                        })
                        .unwrap();
                }
                let start = std::time::Instant::now();
                engine
                    .forget(black_box(ForgetCriteria::by_entity(&entity)))
                    .unwrap();
                total += start.elapsed();
            }
            total
        });
    });
}

fn bench_decay_score_computation(c: &mut Criterion) {
    let now = 1_700_000_000_000_000u64;
    let half_life = 30 * 24 * 3600 * 1_000_000u64;

    c.bench_function("compute_decay_score_1000", |b| {
        b.iter(|| {
            for i in 0u64..1_000 {
                black_box(compute_decay_score(
                    0.7,
                    now - (i * 1_000_000),
                    i % 100,
                    now,
                    half_life,
                    100,
                ));
            }
        });
    });
}

fn bench_tombstone_serialization(c: &mut Criterion) {
    let tombstone = Tombstone {
        memory_id: vec![42u8; 16],
        entity_id: Some("customer_123".to_string()),
        forget_timestamp_us: 1_700_000_000_000_000,
        criteria_description: "by_id: explicit deletion".to_string(),
        cascade_count: 3,
        content_hash: vec![0xAB; 32],
    };
    let bytes = tombstone.to_bytes();

    c.bench_function("tombstone_serialize", |b| {
        b.iter(|| black_box(&tombstone).to_bytes());
    });

    c.bench_function("tombstone_deserialize", |b| {
        b.iter(|| Tombstone::from_bytes(black_box(&bytes)).unwrap());
    });
}

fn bench_revise_at_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("revise_at_scale");
    group.sample_size(10);

    for &size in &[1_000, 10_000] {
        let engine = create_engine_with_memories(size);

        let mut targets: Vec<Vec<u8>> = Vec::new();
        for i in 0..50 {
            let mem = engine
                .remember(RememberInput {
                    content: format!("target for revision {}", i),
                    importance: Some(0.7),
                    context: None,
                    entity_id: Some("revise_scale".to_string()),
                    edges: vec![],
                    kind: None,
                })
                .unwrap();
            targets.push(mem.memory_id);
        }

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            let mut idx = 0usize;
            b.iter(|| {
                let target = &targets[idx % targets.len()];
                engine
                    .revise(black_box(ReviseInput::new_content(
                        target.clone(),
                        format!("revised at scale iter {}", idx),
                    )))
                    .unwrap();
                idx += 1;
            });
        });
    }
    group.finish();
}

// ═══════════════════════════════════════════════════════════════════
//  Phase 6: Subscribe Pipeline Benchmarks
// ═══════════════════════════════════════════════════════════════════

fn subscribe_config_fast() -> SubscribeConfig {
    SubscribeConfig {
        confidence_threshold: 0.0,
        coarse_threshold: 0.0,
        chunk_min_tokens: 3,
        chunk_max_wait_us: 50_000,
        ..SubscribeConfig::default()
    }
}

fn bench_subscribe_pipeline_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("subscribe_pipeline_match");
    group.sample_size(20);

    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

    for i in 0..1_000 {
        engine
            .remember(RememberInput {
                content: format!(
                    "enterprise sales conversation about budget and quarterly revenue target {}",
                    i
                ),
                importance: Some(0.6),
                context: None,
                entity_id: Some("bench".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let mut config = subscribe_config_fast();
    config.entity_id = Some("bench".to_string());

    group.bench_function("1000_memories_match", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let handle = engine.subscribe(config.clone()).unwrap();
                handle
                    .feed("enterprise sales budget quarterly revenue")
                    .unwrap();
                handle.flush();

                let start = std::time::Instant::now();
                let _push = handle.recv_timeout(Duration::from_secs(2));
                total += start.elapsed();
                drop(handle);
            }
            total
        });
    });
    group.finish();
}

fn bench_subscribe_pipeline_bloom_reject(c: &mut Criterion) {
    let mut group = c.benchmark_group("subscribe_bloom_reject");
    group.sample_size(20);

    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

    for i in 0..1_000 {
        engine
            .remember(RememberInput {
                content: format!(
                    "enterprise sales budget quarterly revenue pipeline forecast {}",
                    i
                ),
                importance: Some(0.6),
                context: None,
                entity_id: Some("bloom_bench".to_string()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    let mut config = subscribe_config_fast();
    config.entity_id = Some("bloom_bench".to_string());

    group.bench_function("1000_memories_irrelevant", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let handle = engine.subscribe(config.clone()).unwrap();

                let start = std::time::Instant::now();
                handle
                    .feed("quantum physics photon electron wavelength particle")
                    .unwrap();
                handle.flush();
                // Irrelevant text should be rejected quickly by bloom.
                // Short timeout: we expect no push.
                let _maybe = handle.recv_timeout(Duration::from_millis(50));
                total += start.elapsed();

                let stats = handle.stats();
                black_box(&stats);
                drop(handle);
            }
            total
        });
    });
    group.finish();
}

fn bench_subscribe_notification_fanout(c: &mut Criterion) {
    let mut group = c.benchmark_group("subscribe_notification_fanout");
    group.sample_size(20);

    for &n_subs in &[10usize, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(n_subs),
            &n_subs,
            |b, &n_subs| {
                let backend = Arc::new(InMemoryBackend::new());
                let embedder = Arc::new(MockEmbedder::default_dims());
                let params = HnswParams::with_m(384, 4);
                let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

                for i in 0..100 {
                    engine
                        .remember(RememberInput {
                            content: format!("seed memory for fanout bench {}", i),
                            importance: Some(0.5),
                            context: None,
                            entity_id: Some("fanout".to_string()),
                            edges: vec![],
                            kind: None,
                        })
                        .unwrap();
                }

                let config = subscribe_config_fast();
                let handles: Vec<_> = (0..n_subs)
                    .map(|_| engine.subscribe(config.clone()).unwrap())
                    .collect();

                b.iter(|| {
                    engine
                        .remember(black_box(RememberInput {
                            content: "new memory written during active subscriptions fanout"
                                .to_string(),
                            importance: Some(0.7),
                            context: None,
                            entity_id: Some("fanout".to_string()),
                            edges: vec![],
            kind: None,
        }))
                        .unwrap();
                });

                drop(handles);
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_remember_single,
    bench_remember_with_context,
    bench_get_single,
    bench_get_miss,
    bench_remember_batch,
    bench_serialization,
    bench_delete,
    bench_recall_similarity,
    bench_recall_temporal,
    bench_recall_causal,
    bench_recall_analogical,
    bench_recall_multi_strategy,
    bench_prime,
    bench_composite_scoring,
    bench_reinforcement_overhead,
    bench_revise_content,
    bench_forget_by_id,
    bench_forget_by_entity,
    bench_decay_score_computation,
    bench_tombstone_serialization,
    bench_revise_at_scale,
    bench_subscribe_pipeline_match,
    bench_subscribe_pipeline_bloom_reject,
    bench_subscribe_notification_fanout,
    bench_reflect_pipeline,
    bench_reflect_consolidation,
    bench_insights_query,
);
criterion_main!(benches);

// ─── Phase 7 Benchmarks ──────────────────────────────────────────

fn create_engine_for_reflect(n: usize) -> Engine {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    let engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();

    for i in 0..n {
        engine
            .remember(RememberInput {
                content: format!(
                    "Memory about topic {} in context {} with detail {}",
                    i % 7,
                    i % 3,
                    i
                ),
                importance: Some(0.4 + (i % 6) as f32 * 0.1),
                context: None,
                entity_id: Some("bench_entity".into()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }
    engine
}

fn bench_reflect_pipeline(c: &mut Criterion) {
    let engine = create_engine_for_reflect(100);
    let mock = MockLlmProvider::new();
    let config = ReflectConfig {
        min_memories_for_reflect: 5,
        min_cluster_size: 3,
        ..Default::default()
    };

    c.bench_function("reflect_100_memories_mock_llm", |b| {
        b.iter(|| {
            black_box(
                engine
                    .reflect(
                        ReflectScope::Global { since_us: None },
                        &config,
                        &mock,
                        &mock,
                    )
                    .unwrap(),
            )
        })
    });
}

fn bench_reflect_consolidation(c: &mut Criterion) {
    let engine = create_engine_for_reflect(50);
    let mock = MockLlmProvider::new();
    let config = ReflectConfig {
        min_memories_for_reflect: 5,
        min_cluster_size: 3,
        ..Default::default()
    };

    engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    c.bench_function("reflect_consolidation_50_memories", |b| {
        b.iter(|| {
            black_box(
                engine
                    .reflect(
                        ReflectScope::Global { since_us: None },
                        &config,
                        &mock,
                        &mock,
                    )
                    .unwrap(),
            )
        })
    });
}

fn bench_insights_query(c: &mut Criterion) {
    let engine = create_engine_for_reflect(200);
    let mock = MockLlmProvider::new();
    let config = ReflectConfig {
        min_memories_for_reflect: 5,
        min_cluster_size: 3,
        ..Default::default()
    };

    engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();

    c.bench_function("insights_query_after_200_memories", |b| {
        b.iter(|| {
            black_box(
                engine
                    .insights(InsightsFilter {
                        max_results: Some(50),
                        ..Default::default()
                    })
                    .unwrap(),
            )
        })
    });
}
