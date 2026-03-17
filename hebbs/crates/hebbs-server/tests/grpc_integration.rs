use std::sync::Arc;
use std::time::Instant;

use hebbs_core::auth::KeyCache;
use hebbs_core::engine::Engine;
use hebbs_core::rate_limit::RateLimitConfig;
use hebbs_embed::MockEmbedder;
use hebbs_index::HnswParams;
use hebbs_proto::generated::{self as pb};
use hebbs_server::grpc::health_service::HealthServiceImpl;
use hebbs_server::grpc::memory_service::MemoryServiceImpl;
use hebbs_server::grpc::reflect_service::ReflectServiceImpl;
use hebbs_server::metrics::HebbsMetrics;
use hebbs_server::middleware::AuthState;
use hebbs_storage::InMemoryBackend;
use tonic::Request;

use pb::health_service_server::HealthService;
use pb::memory_service_server::MemoryService;
use pb::reflect_service_server::ReflectService;

fn test_auth_state() -> Arc<AuthState> {
    Arc::new(AuthState {
        key_cache: Arc::new(KeyCache::new()),
        rate_limiter: Arc::new(hebbs_core::rate_limit::RateLimiter::new(RateLimitConfig {
            enabled: false,
            ..Default::default()
        })),
        auth_enabled: false,
    })
}

fn test_engine() -> Arc<Engine> {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    Arc::new(Engine::new_with_params(backend, embedder, params, 42).unwrap())
}

fn test_memory_service() -> MemoryServiceImpl {
    MemoryServiceImpl {
        engine: test_engine(),
        metrics: Arc::new(HebbsMetrics::new()),
        auth_state: test_auth_state(),
    }
}

fn test_memory_service_with_engine(engine: Arc<Engine>) -> MemoryServiceImpl {
    MemoryServiceImpl {
        engine,
        metrics: Arc::new(HebbsMetrics::new()),
        auth_state: test_auth_state(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Remember
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_remember_basic() {
    let svc = test_memory_service();
    let req = Request::new(pb::RememberRequest {
        content: "The sun is hot.".to_string(),
        importance: Some(0.8),
        context: None,
        entity_id: Some("user-1".to_string()),
        edges: vec![],
        tenant_id: None,
        kind: None,
    });

    let resp = svc.remember(req).await.unwrap();
    let memory = resp.into_inner().memory.unwrap();

    assert_eq!(memory.content, "The sun is hot.");
    assert!((memory.importance - 0.8).abs() < 0.01);
    assert_eq!(memory.entity_id.as_deref(), Some("user-1"));
    assert_eq!(memory.memory_id.len(), 16);
    assert!(memory.created_at > 0);
    assert_eq!(memory.kind, pb::MemoryKind::Episode as i32);
}

#[tokio::test]
async fn grpc_remember_empty_content_fails() {
    let svc = test_memory_service();
    let req = Request::new(pb::RememberRequest {
        content: String::new(),
        importance: None,
        context: None,
        entity_id: None,
        edges: vec![],
        tenant_id: None,
        kind: None,
    });

    let err = svc.remember(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("content must not be empty"));
}

#[tokio::test]
async fn grpc_remember_default_importance() {
    let svc = test_memory_service();
    let req = Request::new(pb::RememberRequest {
        content: "default importance test".to_string(),
        importance: None,
        context: None,
        entity_id: None,
        edges: vec![],
        tenant_id: None,
        kind: None,
    });

    let resp = svc.remember(req).await.unwrap();
    let memory = resp.into_inner().memory.unwrap();
    assert!((memory.importance - 0.5).abs() < 0.01);
}

#[tokio::test]
async fn grpc_remember_with_context() {
    let svc = test_memory_service();

    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        "topic".to_string(),
        prost_types::Value {
            kind: Some(prost_types::value::Kind::StringValue("science".to_string())),
        },
    );
    fields.insert(
        "priority".to_string(),
        prost_types::Value {
            kind: Some(prost_types::value::Kind::NumberValue(5.0)),
        },
    );

    let req = Request::new(pb::RememberRequest {
        content: "Water boils at 100C.".to_string(),
        importance: None,
            kind: None,
        context: Some(prost_types::Struct { fields }),
        entity_id: None,
        edges: vec![],
        tenant_id: None,
    });

    let resp = svc.remember(req).await.unwrap();
    let memory = resp.into_inner().memory.unwrap();
    let ctx = memory.context.unwrap();
    assert!(ctx.fields.contains_key("topic"));
    assert!(ctx.fields.contains_key("priority"));
}

#[tokio::test]
async fn grpc_remember_invalid_importance() {
    let svc = test_memory_service();
    let req = Request::new(pb::RememberRequest {
        content: "bad importance".to_string(),
        importance: Some(2.0),
        context: None,
        entity_id: None,
        edges: vec![],
        tenant_id: None,
        kind: None,
    });

    let err = svc.remember(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

// ═══════════════════════════════════════════════════════════════════════
//  Get
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_get_after_remember() {
    let engine = test_engine();
    let svc = test_memory_service_with_engine(engine);

    let remember_resp = svc
        .remember(Request::new(pb::RememberRequest {
            content: "remembered for get".to_string(),
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            tenant_id: None,
            kind: None,
        }))
        .await
        .unwrap();
    let memory_id = remember_resp.into_inner().memory.unwrap().memory_id;

    let get_resp = svc
        .get(Request::new(pb::GetRequest {
            memory_id: memory_id.clone(),
            tenant_id: None,
        }))
        .await
        .unwrap();
    let got = get_resp.into_inner().memory.unwrap();

    assert_eq!(got.memory_id, memory_id);
    assert_eq!(got.content, "remembered for get");
}

#[tokio::test]
async fn grpc_get_not_found() {
    let svc = test_memory_service();
    let req = Request::new(pb::GetRequest {
        memory_id: vec![0u8; 16],
        tenant_id: None,
    });

    let err = svc.get(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn grpc_get_invalid_id_length() {
    let svc = test_memory_service();
    let req = Request::new(pb::GetRequest {
        memory_id: vec![0u8; 8],
        tenant_id: None,
    });

    let err = svc.get(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_recall_similarity() {
    let engine = test_engine();
    let svc = test_memory_service_with_engine(engine);

    for i in 0..5 {
        svc.remember(Request::new(pb::RememberRequest {
                kind: None,
            content: format!("fact number {} about the world", i),
            importance: None,
            context: None,
            entity_id: Some("user-1".to_string()),
            edges: vec![],
            tenant_id: None,
        }))
        .await
        .unwrap();
    }

    let recall_resp = svc
        .recall(Request::new(pb::RecallRequest {
            cue: "fact about the world".to_string(),
            strategies: vec![pb::RecallStrategyConfig {
                strategy_type: pb::RecallStrategyType::Similarity as i32,
                top_k: Some(3),
                ..Default::default()
            }],
            top_k: Some(3),
            scoring_weights: None,
            cue_context: None,
            tenant_id: None,
        }))
        .await
        .unwrap();

    let results = recall_resp.into_inner().results;
    assert!(!results.is_empty());
    assert!(results.len() <= 3);
    assert!(results[0].score > 0.0);
}

#[tokio::test]
async fn grpc_recall_temporal() {
    let engine = test_engine();
    let svc = test_memory_service_with_engine(engine);

    for i in 0..3 {
        svc.remember(Request::new(pb::RememberRequest {
                kind: None,
            content: format!("temporal event {}", i),
            importance: None,
            context: None,
            entity_id: Some("entity-a".to_string()),
            edges: vec![],
            tenant_id: None,
        }))
        .await
        .unwrap();
    }

    let recall_resp = svc
        .recall(Request::new(pb::RecallRequest {
            cue: "temporal event".to_string(),
            strategies: vec![pb::RecallStrategyConfig {
                strategy_type: pb::RecallStrategyType::Temporal as i32,
                entity_id: Some("entity-a".to_string()),
                ..Default::default()
            }],
            top_k: Some(5),
            scoring_weights: None,
            cue_context: None,
            tenant_id: None,
        }))
        .await
        .unwrap();

    let results = recall_resp.into_inner().results;
    assert!(!results.is_empty());
}

#[tokio::test]
async fn grpc_recall_empty_cue_fails() {
    let svc = test_memory_service();
    let req = Request::new(pb::RecallRequest {
        cue: String::new(),
        strategies: vec![pb::RecallStrategyConfig {
            strategy_type: pb::RecallStrategyType::Similarity as i32,
            ..Default::default()
        }],
        top_k: None,
        scoring_weights: None,
        cue_context: None,
        tenant_id: None,
    });

    let err = svc.recall(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn grpc_recall_no_strategies_fails() {
    let svc = test_memory_service();
    let req = Request::new(pb::RecallRequest {
        cue: "test".to_string(),
        strategies: vec![],
        top_k: None,
        scoring_weights: None,
        cue_context: None,
        tenant_id: None,
    });

    let err = svc.recall(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

// ═══════════════════════════════════════════════════════════════════════
//  Prime
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_prime_basic() {
    let engine = test_engine();
    let svc = test_memory_service_with_engine(engine);

    for i in 0..5 {
        svc.remember(Request::new(pb::RememberRequest {
                kind: None,
            content: format!("prime memory {} about cooking", i),
            importance: None,
            context: None,
            entity_id: Some("chef".to_string()),
            edges: vec![],
            tenant_id: None,
        }))
        .await
        .unwrap();
    }

    let resp = svc
        .prime(Request::new(pb::PrimeRequest {
            entity_id: "chef".to_string(),
            context: None,
            max_memories: Some(10),
            recency_window_us: None,
            similarity_cue: Some("cooking recipes".to_string()),
            scoring_weights: None,
            tenant_id: None,
        }))
        .await
        .unwrap();

    let output = resp.into_inner();
    assert!(!output.results.is_empty());
}

#[tokio::test]
async fn grpc_prime_empty_entity_fails() {
    let svc = test_memory_service();
    let req = Request::new(pb::PrimeRequest {
        entity_id: String::new(),
        context: None,
        max_memories: None,
        recency_window_us: None,
        similarity_cue: None,
        scoring_weights: None,
        tenant_id: None,
    });

    let err = svc.prime(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

// ═══════════════════════════════════════════════════════════════════════
//  Revise
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_revise_content() {
    let engine = test_engine();
    let svc = test_memory_service_with_engine(engine);

    let memory_id = svc
        .remember(Request::new(pb::RememberRequest {
            content: "original content".to_string(),
            importance: Some(0.6),
            context: None,
            entity_id: None,
            edges: vec![],
            tenant_id: None,
            kind: None,
        }))
        .await
        .unwrap()
        .into_inner()
        .memory
        .unwrap()
        .memory_id;

    let resp = svc
        .revise(Request::new(pb::ReviseRequest {
            memory_id: memory_id.clone(),
            content: Some("revised content".to_string()),
            importance: None,
            context: None,
            context_mode: pb::ContextMode::Merge as i32,
            entity_id: None,
            edges: vec![],
            tenant_id: None,
        }))
        .await
        .unwrap();

    let revised = resp.into_inner().memory.unwrap();
    assert_eq!(revised.content, "revised content");
    assert_eq!(revised.memory_id, memory_id);
    assert!(revised.updated_at >= revised.created_at);
    assert_eq!(revised.kind, pb::MemoryKind::Revision as i32);
}

#[tokio::test]
async fn grpc_revise_importance() {
    let engine = test_engine();
    let svc = test_memory_service_with_engine(engine);

    let memory_id = svc
        .remember(Request::new(pb::RememberRequest {
            content: "importance test".to_string(),
            importance: Some(0.3),
            context: None,
            entity_id: None,
            edges: vec![],
            tenant_id: None,
            kind: None,
        }))
        .await
        .unwrap()
        .into_inner()
        .memory
        .unwrap()
        .memory_id;

    let resp = svc
        .revise(Request::new(pb::ReviseRequest {
            memory_id,
            content: None,
            importance: Some(0.9),
            context: None,
            context_mode: pb::ContextMode::Merge as i32,
            entity_id: None,
            edges: vec![],
            tenant_id: None,
        }))
        .await
        .unwrap();

    let revised = resp.into_inner().memory.unwrap();
    assert!((revised.importance - 0.9).abs() < 0.01);
}

#[tokio::test]
async fn grpc_revise_not_found() {
    let svc = test_memory_service();
    let req = Request::new(pb::ReviseRequest {
        memory_id: vec![0u8; 16],
        content: Some("update".to_string()),
        importance: None,
        context: None,
        context_mode: pb::ContextMode::Merge as i32,
        entity_id: None,
        edges: vec![],
        tenant_id: None,
    });

    let err = svc.revise(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

// ═══════════════════════════════════════════════════════════════════════
//  Forget
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_forget_by_id() {
    let engine = test_engine();
    let svc = test_memory_service_with_engine(engine.clone());

    let memory_id = svc
        .remember(Request::new(pb::RememberRequest {
            content: "to be forgotten".to_string(),
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
            tenant_id: None,
            kind: None,
        }))
        .await
        .unwrap()
        .into_inner()
        .memory
        .unwrap()
        .memory_id;

    let forget_resp = svc
        .forget(Request::new(pb::ForgetRequest {
            memory_ids: vec![memory_id.clone()],
            entity_id: None,
            staleness_threshold_us: None,
            access_count_floor: None,
            memory_kind: None,
            decay_score_floor: None,
            tenant_id: None,
        }))
        .await
        .unwrap();

    let output = forget_resp.into_inner();
    assert_eq!(output.forgotten_count, 1);
    assert!(output.tombstone_count >= 1);

    let err = svc
        .get(Request::new(pb::GetRequest {
            memory_id,
            tenant_id: None,
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn grpc_forget_empty_criteria_fails() {
    let svc = test_memory_service();
    let req = Request::new(pb::ForgetRequest {
        memory_ids: vec![],
        entity_id: None,
        staleness_threshold_us: None,
        access_count_floor: None,
        memory_kind: None,
        decay_score_floor: None,
        tenant_id: None,
    });

    let err = svc.forget(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

// ═══════════════════════════════════════════════════════════════════════
//  Health
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_health_check() {
    let engine = test_engine();
    let svc = HealthServiceImpl {
        engine: engine.clone(),
        start_time: Instant::now(),
        version: "0.1.0-test".to_string(),
        data_dir: std::env::temp_dir(),
    };

    let resp = svc
        .check(Request::new(pb::HealthCheckRequest {}))
        .await
        .unwrap();
    let health = resp.into_inner();

    assert_eq!(
        health.status,
        pb::health_check_response::ServingStatus::Serving as i32
    );
    assert_eq!(health.version, "0.1.0-test");
    assert_eq!(health.memory_count, 0);
}

#[tokio::test]
async fn grpc_health_reflects_memory_count() {
    let engine = test_engine();
    let metrics = Arc::new(HebbsMetrics::new());
    let mem_svc = MemoryServiceImpl {
        engine: engine.clone(),
        metrics: metrics.clone(),
        auth_state: test_auth_state(),
    };

    for i in 0..3 {
        mem_svc
            .remember(Request::new(pb::RememberRequest {
                    kind: None,
                content: format!("health count test {}", i),
                importance: None,
                context: None,
                entity_id: None,
                edges: vec![],
                tenant_id: None,
            }))
            .await
            .unwrap();
    }

    let health_svc = HealthServiceImpl {
        engine,
        start_time: Instant::now(),
        version: "test".to_string(),
        data_dir: std::env::temp_dir(),
    };

    let resp = health_svc
        .check(Request::new(pb::HealthCheckRequest {}))
        .await
        .unwrap();
    assert_eq!(resp.into_inner().memory_count, 3);
}

// ═══════════════════════════════════════════════════════════════════════
//  Reflect / Insights
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_insights_empty() {
    let engine = test_engine();
    let metrics = Arc::new(HebbsMetrics::new());
    let config = hebbs_core::reflect::ReflectConfig::default();
    let proposal =
        Arc::from(hebbs_reflect::create_provider(&config.proposal_provider_config).unwrap());
    let validation =
        Arc::from(hebbs_reflect::create_provider(&config.validation_provider_config).unwrap());
    let svc = ReflectServiceImpl {
        engine,
        metrics,
        reflect_config: config,
        proposal_provider: proposal,
        validation_provider: validation,
        auth_state: test_auth_state(),
    };

    let resp = svc
        .get_insights(Request::new(pb::GetInsightsRequest {
            entity_id: None,
            min_confidence: None,
            max_results: Some(10),
            tenant_id: None,
        }))
        .await
        .unwrap();

    assert!(resp.into_inner().insights.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
//  Multi-operation workflows
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_remember_recall_revise_forget_lifecycle() {
    let engine = test_engine();
    let svc = test_memory_service_with_engine(engine);

    // Remember
    let mem = svc
        .remember(Request::new(pb::RememberRequest {
            content: "lifecycle test memory".to_string(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("lifecycle".to_string()),
            edges: vec![],
            tenant_id: None,
            kind: None,
        }))
        .await
        .unwrap()
        .into_inner()
        .memory
        .unwrap();

    // Recall
    let recall_resp = svc
        .recall(Request::new(pb::RecallRequest {
            cue: "lifecycle test".to_string(),
            strategies: vec![pb::RecallStrategyConfig {
                strategy_type: pb::RecallStrategyType::Similarity as i32,
                top_k: Some(5),
                ..Default::default()
            }],
            top_k: Some(5),
            scoring_weights: None,
            cue_context: None,
            tenant_id: None,
        }))
        .await
        .unwrap();
    let recall_results = recall_resp.into_inner().results;
    assert!(!recall_results.is_empty());

    // Revise
    let revised = svc
        .revise(Request::new(pb::ReviseRequest {
            memory_id: mem.memory_id.clone(),
            content: Some("revised lifecycle memory".to_string()),
            importance: Some(0.9),
            context: None,
            context_mode: pb::ContextMode::Merge as i32,
            entity_id: None,
            edges: vec![],
            tenant_id: None,
        }))
        .await
        .unwrap()
        .into_inner()
        .memory
        .unwrap();
    assert_eq!(revised.content, "revised lifecycle memory");

    // Forget
    let forget_resp = svc
        .forget(Request::new(pb::ForgetRequest {
            memory_ids: vec![mem.memory_id.clone()],
            entity_id: None,
            staleness_threshold_us: None,
            access_count_floor: None,
            memory_kind: None,
            decay_score_floor: None,
            tenant_id: None,
        }))
        .await
        .unwrap();
    assert_eq!(forget_resp.into_inner().forgotten_count, 1);

    // Verify forgotten
    let err = svc
        .get(Request::new(pb::GetRequest {
            memory_id: mem.memory_id,
            tenant_id: None,
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn grpc_bulk_remember_and_recall() {
    let engine = test_engine();
    let svc = test_memory_service_with_engine(engine);

    for i in 0..20 {
        svc.remember(Request::new(pb::RememberRequest {
                kind: None,
            content: format!("bulk memory item {} about various topics", i),
            importance: Some(0.5 + (i as f32 * 0.02)),
            context: None,
            entity_id: Some("bulk-entity".to_string()),
            edges: vec![],
            tenant_id: None,
        }))
        .await
        .unwrap();
    }

    let resp = svc
        .recall(Request::new(pb::RecallRequest {
            cue: "bulk memory topics".to_string(),
            strategies: vec![pb::RecallStrategyConfig {
                strategy_type: pb::RecallStrategyType::Similarity as i32,
                top_k: Some(10),
                ..Default::default()
            }],
            top_k: Some(10),
            scoring_weights: None,
            cue_context: None,
            tenant_id: None,
        }))
        .await
        .unwrap();

    let results = resp.into_inner().results;
    assert!(!results.is_empty());
    assert!(results.len() <= 10);

    for i in 1..results.len() {
        assert!(results[i - 1].score >= results[i].score);
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Metrics observation
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grpc_metrics_increment_on_operations() {
    let engine = test_engine();
    let metrics = Arc::new(HebbsMetrics::new());
    let svc = MemoryServiceImpl {
        engine: engine.clone(),
        metrics: metrics.clone(),
        auth_state: test_auth_state(),
    };

    svc.remember(Request::new(pb::RememberRequest {
        content: "metrics test".to_string(),
        importance: None,
        context: None,
        entity_id: None,
        edges: vec![],
        tenant_id: None,
        kind: None,
    }))
    .await
    .unwrap();

    let rendered = metrics.render();
    assert!(rendered.contains("hebbs_operation_duration_seconds"));
    assert!(rendered.contains("remember"));
}
