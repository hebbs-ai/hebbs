use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use tower::ServiceExt;

use hebbs_core::engine::{Engine, RememberInput};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig, ReflectScope};
use hebbs_embed::MockEmbedder;
use hebbs_index::{EdgeType, HnswParams};
use hebbs_reflect::MockLlmProvider;
use hebbs_server::metrics::HebbsMetrics;
use hebbs_server::rest::{self, AppState};
use hebbs_storage::InMemoryBackend;

fn test_engine() -> Arc<Engine> {
    let backend = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    Arc::new(Engine::new_with_params(backend, embedder, params, 42).unwrap())
}

fn test_app(engine: Arc<Engine>) -> axum::Router {
    let state = AppState::new(
        engine,
        Arc::new(HebbsMetrics::new()),
        Instant::now(),
        "test".to_string(),
        std::env::temp_dir(),
    );
    rest::create_router(state)
}

async fn body_to_json(body: Body) -> serde_json::Value {
    let bytes = http_body_util::BodyExt::collect(body)
        .await
        .unwrap()
        .to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════
//  Health
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_liveness() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .uri("/v1/health/live")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["status"], "alive");
}

#[tokio::test]
async fn rest_readiness() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .uri("/v1/health/ready")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["status"], "ready");
}

#[tokio::test]
async fn rest_metrics_endpoint() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .uri("/v1/metrics")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("hebbs_"));
}

// ═══════════════════════════════════════════════════════════════════════
//  Remember
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_remember_basic() {
    let engine = test_engine();
    let app = test_app(engine);

    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "content": "REST test memory",
                "importance": 0.7,
                "entity_id": "rest-user"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["content"], "REST test memory");
    assert!(json["memory_id"].as_str().unwrap().len() == 32);
    assert_eq!(json["entity_id"], "rest-user");
    assert_eq!(json["kind"], "episode");
}

#[tokio::test]
async fn rest_remember_empty_content() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"content": ""}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rest_remember_with_context() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "content": "context test",
                "context": {
                    "source": "unit-test",
                    "priority": 5
                }
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["context"]["source"], "unit-test");
}

// ═══════════════════════════════════════════════════════════════════════
//  Get
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_get_after_remember() {
    let engine = test_engine();
    let app = test_app(engine.clone());

    // Remember
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"content": "for get test"}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let json = body_to_json(resp.into_body()).await;
    let memory_id = json["memory_id"].as_str().unwrap().to_string();

    // Get
    let app2 = test_app(engine);
    let req = Request::builder()
        .uri(format!("/v1/memories/{}", memory_id))
        .body(Body::empty())
        .unwrap();

    let resp = app2.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let got = body_to_json(resp.into_body()).await;
    assert_eq!(got["memory_id"], memory_id);
    assert_eq!(got["content"], "for get test");
}

#[tokio::test]
async fn rest_get_not_found() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .uri("/v1/memories/00000000000000000000000000000000")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rest_get_invalid_id() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .uri("/v1/memories/not-a-hex-id")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_recall_similarity() {
    let engine = test_engine();

    // Populate
    let app = test_app(engine.clone());
    for i in 0..5 {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({
                    "content": format!("REST recall test item {}", i),
                    "entity_id": "recall-user"
                }))
                .unwrap(),
            ))
            .unwrap();
        app.clone().oneshot(req).await.unwrap();
    }

    // Recall
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/recall")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "cue": "recall test item",
                "strategies": ["similarity"],
                "top_k": 3
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    let results = json.as_array().unwrap();
    assert!(!results.is_empty());
    assert!(results.len() <= 3);
}

// ═══════════════════════════════════════════════════════════════════════
//  Prime
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_prime_basic() {
    let engine = test_engine();
    let app = test_app(engine.clone());

    for i in 0..5 {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({
                    "content": format!("prime rest test {}", i),
                    "entity_id": "prime-entity"
                }))
                .unwrap(),
            ))
            .unwrap();
        app.clone().oneshot(req).await.unwrap();
    }

    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/prime")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "entity_id": "prime-entity",
                "max_memories": 10,
                "similarity_cue": "prime rest test"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert!(!json.as_array().unwrap().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
//  Revise
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_revise_content() {
    let engine = test_engine();
    let app = test_app(engine.clone());

    // Create
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"content": "original rest content"}"#))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let json = body_to_json(resp.into_body()).await;
    let memory_id = json["memory_id"].as_str().unwrap().to_string();

    // Revise
    let req = Request::builder()
        .method(http::Method::PUT)
        .uri(format!("/v1/revise/{}", memory_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"content": "revised via REST"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let revised = body_to_json(resp.into_body()).await;
    assert_eq!(revised["content"], "revised via REST");
    assert_eq!(revised["kind"], "revision");
}

// ═══════════════════════════════════════════════════════════════════════
//  Forget
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_forget_by_id() {
    let engine = test_engine();
    let app = test_app(engine.clone());

    // Create
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"content": "to forget via rest"}"#))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let json = body_to_json(resp.into_body()).await;
    let memory_id = json["memory_id"].as_str().unwrap().to_string();

    // Forget
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/forget")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "memory_ids": [memory_id.clone()]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let forget_json = body_to_json(resp.into_body()).await;
    assert_eq!(forget_json["forgotten_count"], 1);

    // Verify gone
    let req = Request::builder()
        .uri(format!("/v1/memories/{}", memory_id))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ═══════════════════════════════════════════════════════════════════════
//  Insights
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_insights_empty() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .uri("/v1/insights")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert!(json.as_array().unwrap().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
//  Full REST lifecycle
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_full_lifecycle() {
    let engine = test_engine();
    let app = test_app(engine.clone());

    // 1. Remember
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "content": "lifecycle via REST",
                "importance": 0.8,
                "entity_id": "lifecycle-user"
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = body_to_json(resp.into_body()).await;
    let memory_id = json["memory_id"].as_str().unwrap().to_string();

    // 2. Get
    let req = Request::builder()
        .uri(format!("/v1/memories/{}", memory_id))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 3. Recall
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/recall")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "cue": "lifecycle",
                "strategies": ["similarity"],
                "top_k": 5
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 4. Revise
    let req = Request::builder()
        .method(http::Method::PUT)
        .uri(format!("/v1/revise/{}", memory_id))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"content": "revised lifecycle REST"}"#))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Forget
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/forget")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "memory_ids": [memory_id.clone()]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 6. Verify forgotten
    let req = Request::builder()
        .uri(format!("/v1/memories/{}", memory_id))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ═══════════════════════════════════════════════════════════════════════
//  Insight Lineage
// ═══════════════════════════════════════════════════════════════════════

/// Verify that `/v1/insights` returns `source_memory_ids` for each insight.
#[tokio::test]
async fn rest_insights_include_source_memory_ids() {
    let engine = test_engine();

    // Populate enough memories to trigger clustering (30 memories, min_cluster_size=2)
    for i in 0..30 {
        engine
            .remember(RememberInput {
                content: format!(
                    "Customer lineage-test mentioned topic {} during call {} with detail {}",
                    i % 5,
                    i,
                    i * 7
                ),
                importance: Some(0.5 + (i % 10) as f32 * 0.05),
                context: None,
                entity_id: Some("lineage-entity".into()),
                edges: vec![],
                kind: None,
            })
            .unwrap();
    }

    // Run reflect to produce insights with InsightFrom edges
    let mock = MockLlmProvider::new();
    let config = ReflectConfig::default();
    let output = engine
        .reflect(
            ReflectScope::Global { since_us: None },
            &config,
            &mock,
            &mock,
        )
        .unwrap();
    assert!(output.insights_created > 0, "reflect must produce insights");

    // Verify edges exist at engine level (sanity check)
    let insights = engine.insights(InsightsFilter::default()).unwrap();
    assert!(!insights.is_empty());
    let mut engine_lineage_count = 0;
    for insight in &insights {
        let mut id = [0u8; 16];
        id.copy_from_slice(&insight.memory_id);
        let edges = engine.outgoing_edges(&id).unwrap_or_default();
        let insight_from: Vec<_> = edges
            .iter()
            .filter(|(et, _, _)| *et == EdgeType::InsightFrom)
            .collect();
        engine_lineage_count += insight_from.len();
    }
    assert!(
        engine_lineage_count > 0,
        "insights must have InsightFrom edges at the engine level"
    );

    // Now test the REST API returns source_memory_ids
    let app = test_app(engine);
    let req = Request::builder()
        .uri("/v1/insights")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    let insights_json = json.as_array().unwrap();
    assert!(!insights_json.is_empty(), "REST insights must not be empty");

    let mut total_source_ids = 0;
    for insight in insights_json {
        assert_eq!(insight["kind"], "insight");
        let source_ids = insight["source_memory_ids"]
            .as_array()
            .expect("source_memory_ids must be a JSON array");
        total_source_ids += source_ids.len();
        for sid in source_ids {
            let hex = sid.as_str().expect("source_memory_id must be a hex string");
            assert_eq!(hex.len(), 32, "source_memory_id must be 32-char hex");
        }
    }

    assert!(
        total_source_ids > 0,
        "REST /v1/insights must include source_memory_ids for lineage (Principle 6)"
    );
}

/// Verify that episode memories returned via REST do not include source_memory_ids.
#[tokio::test]
async fn rest_episodes_omit_source_memory_ids() {
    let engine = test_engine();
    let app = test_app(engine);

    // Remember an episode
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"content": "plain episode memory"}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["kind"], "episode");
    // source_memory_ids should be absent (skip_serializing_if = "Vec::is_empty")
    assert!(
        json.get("source_memory_ids").is_none(),
        "episode memories must not include source_memory_ids field"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Contradiction Prepare / Commit
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_contradiction_prepare_empty() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/contradictions/prepare")
        .header("content-type", "application/json")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn rest_contradiction_commit_empty_verdicts() {
    let app = test_app(test_engine());
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/contradictions/commit")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"verdicts": []}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["contradictions_confirmed"], 0);
    assert_eq!(json["revisions_created"], 0);
    assert_eq!(json["dismissed"], 0);
}
