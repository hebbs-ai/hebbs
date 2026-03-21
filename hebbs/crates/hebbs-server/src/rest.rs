use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{delete, get, post, put},
    Json, Router,
};
use hebbs_core::engine::{Engine, RememberInput};
use hebbs_core::forget::ForgetCriteria;
use hebbs_core::memory::MemoryKind;
use hebbs_core::recall::{
    PrimeInput, RecallInput, RecallStrategy, ScoringWeights, StrategyDetail, DEFAULT_MAX_AGE_US,
    DEFAULT_REINFORCEMENT_CAP,
};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig};
use hebbs_core::revise::{ContextMode, ReviseInput};
use hebbs_core::subscribe::SubscribeConfig;
use hebbs_index::EdgeType;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::convert;
use crate::metrics::HebbsMetrics;
use crate::middleware::TenantExtractor;

pub(crate) struct SseSubscriptionEntry {
    handle: hebbs_core::subscribe::SubscriptionHandle,
}

type SubscriptionMap = Arc<Mutex<HashMap<u64, SseSubscriptionEntry>>>;

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub metrics: Arc<HebbsMetrics>,
    pub start_time: std::time::Instant,
    pub version: String,
    pub data_dir: PathBuf,
    pub(crate) sse_subscriptions: SubscriptionMap,
    pub reflect_config: Option<ReflectConfig>,
}

impl AppState {
    pub fn new(
        engine: Arc<Engine>,
        metrics: Arc<HebbsMetrics>,
        start_time: std::time::Instant,
        version: String,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            engine,
            metrics,
            start_time,
            version,
            data_dir,
            sse_subscriptions: Arc::new(Mutex::new(HashMap::new())),
            reflect_config: None,
        }
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/v1/memories",
            get(list_memories_handler).post(remember_handler),
        )
        .route("/v1/memories/batch", post(batch_get_handler))
        .route("/v1/memories/:id", get(get_handler))
        .route("/v1/memories/:id/edges", get(edges_handler))
        .route("/v1/recall", post(recall_handler))
        .route("/v1/prime", post(prime_handler))
        .route("/v1/revise/:id", put(revise_handler))
        .route("/v1/forget", post(forget_handler))
        .route("/v1/subscribe", post(sse_subscribe_handler))
        .route("/v1/subscribe/:id/feed", post(sse_feed_handler))
        .route("/v1/subscribe/:id", delete(sse_close_handler))
        .route("/v1/insights", get(insights_handler))
        .route("/v1/reflect/prepare", post(reflect_prepare_handler))
        .route("/v1/reflect/commit", post(reflect_commit_handler))
        .route(
            "/v1/contradictions/prepare",
            post(contradiction_prepare_handler),
        )
        .route(
            "/v1/contradictions/commit",
            post(contradiction_commit_handler),
        )
        .route("/v1/graph", post(graph_handler))
        .route("/v1/entities", get(entities_handler))
        .route("/v1/health/live", get(liveness_handler))
        .route("/v1/health/ready", get(readiness_handler))
        .route("/v1/metrics", get(metrics_handler))
        .with_state(state)
}

// ═══════════════════════════════════════════════════════════════════════
//  JSON Request/Response Types
// ═══════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct RememberBody {
    content: String,
    importance: Option<f32>,
    context: Option<serde_json::Map<String, serde_json::Value>>,
    entity_id: Option<String>,
}

#[derive(Deserialize)]
struct ScoringWeightsBody {
    w_relevance: Option<f32>,
    w_recency: Option<f32>,
    w_importance: Option<f32>,
    w_reinforcement: Option<f32>,
    max_age_us: Option<u64>,
    reinforcement_cap: Option<u64>,
}

#[derive(Deserialize)]
struct RecallBody {
    cue: String,
    strategies: Vec<String>,
    top_k: Option<usize>,
    entity_id: Option<String>,
    time_range: Option<TimeRangeBody>,
    max_depth: Option<usize>,
    scoring_weights: Option<ScoringWeightsBody>,
    ef_search: Option<usize>,
    edge_types: Option<Vec<String>>,
    seed_memory_id: Option<String>,
    analogical_alpha: Option<f32>,
    cue_context: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct TimeRangeBody {
    start_us: u64,
    end_us: u64,
}

#[derive(Deserialize)]
struct PrimeBody {
    entity_id: String,
    context: Option<serde_json::Map<String, serde_json::Value>>,
    max_memories: Option<usize>,
    recency_window_us: Option<u64>,
    similarity_cue: Option<String>,
    scoring_weights: Option<ScoringWeightsBody>,
}

#[derive(Deserialize)]
struct ReviseBody {
    content: Option<String>,
    importance: Option<f32>,
    context: Option<serde_json::Map<String, serde_json::Value>>,
    context_mode: Option<String>,
    entity_id: Option<String>,
}

#[derive(Deserialize)]
struct ForgetBody {
    memory_ids: Option<Vec<String>>,
    entity_id: Option<String>,
    staleness_threshold_us: Option<u64>,
    access_count_floor: Option<u64>,
    memory_kind: Option<String>,
    decay_score_floor: Option<f32>,
}

#[derive(Deserialize)]
struct InsightsQuery {
    entity_id: Option<String>,
    min_confidence: Option<f32>,
    max_results: Option<usize>,
}

#[derive(Deserialize)]
struct ListMemoriesQuery {
    entity_id: Option<String>,
    kind: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct BatchGetBody {
    memory_ids: Vec<String>,
}

#[derive(Deserialize)]
struct GraphBody {
    center_id: String,
    depth: Option<usize>,
    edge_types: Option<Vec<String>>,
    max_nodes: Option<usize>,
}

#[derive(Serialize)]
struct EdgeJson {
    source_id: String,
    target_id: String,
    edge_type: String,
    confidence: f32,
    timestamp_us: u64,
}

#[derive(Serialize)]
struct EdgesResponseJson {
    outgoing: Vec<EdgeJson>,
    incoming: Vec<EdgeJson>,
}

#[derive(Serialize)]
struct GraphResponseJson {
    nodes: Vec<MemoryJson>,
    edges: Vec<EdgeJson>,
    truncated: bool,
}

#[derive(Serialize)]
struct ListMemoriesResponseJson {
    memories: Vec<MemoryJson>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
struct MemoryJson {
    memory_id: String,
    content: String,
    importance: f32,
    context: serde_json::Value,
    entity_id: Option<String>,
    created_at: u64,
    updated_at: u64,
    last_accessed_at: u64,
    access_count: u64,
    decay_score: f32,
    kind: String,
    logical_clock: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_memory_ids: Vec<String>,
}

#[derive(Serialize)]
struct ErrorJson {
    error_code: String,
    message: String,
}

#[derive(Serialize)]
struct ForgetResponseJson {
    forgotten_count: usize,
    cascade_count: usize,
    truncated: bool,
    tombstone_count: usize,
}

#[derive(Serialize)]
struct StrategyDetailJson {
    strategy: String,
    relevance: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    distance: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rank: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embedding_similarity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    structural_similarity: Option<f32>,
}

fn strategy_detail_to_json(d: &StrategyDetail) -> StrategyDetailJson {
    match d {
        StrategyDetail::Similarity {
            distance,
            relevance,
        } => StrategyDetailJson {
            strategy: "similarity".to_string(),
            relevance: *relevance,
            distance: Some(*distance),
            timestamp: None,
            rank: None,
            depth: None,
            embedding_similarity: None,
            structural_similarity: None,
        },
        StrategyDetail::Temporal {
            timestamp,
            rank,
            relevance,
        } => StrategyDetailJson {
            strategy: "temporal".to_string(),
            relevance: *relevance,
            distance: None,
            timestamp: Some(*timestamp),
            rank: Some(*rank),
            depth: None,
            embedding_similarity: None,
            structural_similarity: None,
        },
        StrategyDetail::Causal {
            depth, relevance, ..
        } => StrategyDetailJson {
            strategy: "causal".to_string(),
            relevance: *relevance,
            distance: None,
            timestamp: None,
            rank: None,
            depth: Some(*depth),
            embedding_similarity: None,
            structural_similarity: None,
        },
        StrategyDetail::Analogical {
            embedding_similarity,
            structural_similarity,
            relevance,
            ..
        } => StrategyDetailJson {
            strategy: "analogical".to_string(),
            relevance: *relevance,
            distance: None,
            timestamp: None,
            rank: None,
            depth: None,
            embedding_similarity: Some(*embedding_similarity),
            structural_similarity: Some(*structural_similarity),
        },
    }
}

#[derive(Serialize)]
struct RecallResultJson {
    memory: MemoryJson,
    score: f32,
    relevance: f32,
    strategy_details: Vec<StrategyDetailJson>,
}

#[derive(Serialize)]
#[allow(dead_code)]
struct HealthJson {
    status: String,
    version: String,
    memory_count: usize,
    uptime_seconds: u64,
}

fn memory_to_json(m: &hebbs_core::memory::Memory) -> MemoryJson {
    memory_to_json_with_lineage(m, &[])
}

fn memory_to_json_with_lineage(
    m: &hebbs_core::memory::Memory,
    source_ids: &[[u8; 16]],
) -> MemoryJson {
    let ctx: serde_json::Value = if m.context_bytes.is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(&m.context_bytes)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
    };

    MemoryJson {
        memory_id: hex::encode(&m.memory_id),
        content: m.content.clone(),
        importance: m.importance,
        context: ctx,
        entity_id: m.entity_id.clone(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        last_accessed_at: m.last_accessed_at,
        access_count: m.access_count,
        decay_score: m.decay_score,
        kind: match m.kind {
            MemoryKind::Episode => "episode".to_string(),
            MemoryKind::Insight => "insight".to_string(),
            MemoryKind::Revision => "revision".to_string(),
            MemoryKind::Document => "document".to_string(),
            MemoryKind::Proposition => "proposition".to_string(),
        },
        logical_clock: m.logical_clock,
        source_memory_ids: source_ids.iter().map(hex::encode).collect(),
    }
}

fn json_error(status: StatusCode, code: &str, msg: &str) -> (StatusCode, Json<ErrorJson>) {
    (
        status,
        Json(ErrorJson {
            error_code: code.to_string(),
            message: msg.to_string(),
        }),
    )
}

fn map_hebbs_error(e: hebbs_core::error::HebbsError) -> (StatusCode, Json<ErrorJson>) {
    let (status, msg) = convert::hebbs_error_to_http_status(&e);
    let code = match &e {
        hebbs_core::error::HebbsError::MemoryNotFound { .. } => "not_found",
        hebbs_core::error::HebbsError::InvalidInput { .. } => "invalid_input",
        hebbs_core::error::HebbsError::Storage(_) => "storage_error",
        hebbs_core::error::HebbsError::Reflect(_) => "reflect_error",
        _ => "internal_error",
    };
    json_error(status, code, &msg)
}

fn scoring_weights_from_body(sw: ScoringWeightsBody) -> ScoringWeights {
    ScoringWeights {
        w_relevance: sw.w_relevance.unwrap_or(0.5),
        w_recency: sw.w_recency.unwrap_or(0.2),
        w_importance: sw.w_importance.unwrap_or(0.2),
        w_reinforcement: sw.w_reinforcement.unwrap_or(0.1),
        max_age_us: sw.max_age_us.unwrap_or(DEFAULT_MAX_AGE_US),
        reinforcement_cap: sw.reinforcement_cap.unwrap_or(DEFAULT_REINFORCEMENT_CAP),
    }
}

fn recall_result_to_rest_json_with_lineage(
    r: &hebbs_core::recall::RecallResult,
    lineage: &std::collections::HashMap<[u8; 16], Vec<[u8; 16]>>,
) -> RecallResultJson {
    let primary_relevance = r
        .strategy_details
        .first()
        .map(|d| d.relevance())
        .unwrap_or(0.0);
    let sources = convert::get_lineage_for_memory(lineage, &r.memory.memory_id);
    RecallResultJson {
        memory: memory_to_json_with_lineage(&r.memory, &sources),
        score: r.score,
        relevance: primary_relevance,
        strategy_details: r
            .strategy_details
            .iter()
            .map(strategy_detail_to_json)
            .collect(),
    }
}

fn parse_strategy(s: &str) -> Option<RecallStrategy> {
    match s.to_lowercase().as_str() {
        "similarity" => Some(RecallStrategy::Similarity),
        "temporal" => Some(RecallStrategy::Temporal),
        "causal" => Some(RecallStrategy::Causal),
        "analogical" => Some(RecallStrategy::Analogical),
        _ => None,
    }
}

fn parse_edge_type(s: &str) -> Option<EdgeType> {
    match s.to_lowercase().as_str() {
        "caused_by" => Some(EdgeType::CausedBy),
        "followed_by" => Some(EdgeType::FollowedBy),
        "related_to" => Some(EdgeType::RelatedTo),
        "revised_from" => Some(EdgeType::RevisedFrom),
        "insight_from" => Some(EdgeType::InsightFrom),
        "contradicts" => Some(EdgeType::Contradicts),
        "has_entity" => Some(EdgeType::HasEntity),
        "entity_relation" => Some(EdgeType::EntityRelation),
        "proposition_of" => Some(EdgeType::PropositionOf),
        _ => None,
    }
}

fn edge_type_to_str(et: EdgeType) -> &'static str {
    match et {
        EdgeType::CausedBy => "caused_by",
        EdgeType::RelatedTo => "related_to",
        EdgeType::FollowedBy => "followed_by",
        EdgeType::RevisedFrom => "revised_from",
        EdgeType::InsightFrom => "insight_from",
        EdgeType::Contradicts => "contradicts",
        EdgeType::HasEntity => "has_entity",
        EdgeType::EntityRelation => "entity_relation",
        EdgeType::PropositionOf => "proposition_of",
    }
}

fn parse_hex_id_16(s: &str) -> Option<[u8; 16]> {
    let bytes = hex::decode(s).ok()?;
    if bytes.len() == 16 {
        let mut arr = [0u8; 16];
        arr.copy_from_slice(&bytes);
        Some(arr)
    } else {
        None
    }
}

fn json_value_to_hashmap(val: serde_json::Value) -> Option<HashMap<String, serde_json::Value>> {
    match val {
        serde_json::Value::Object(map) => Some(map.into_iter().collect()),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Handlers
// ═══════════════════════════════════════════════════════════════════════

async fn remember_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Json(body): Json<RememberBody>,
) -> impl IntoResponse {
    let start = std::time::Instant::now();

    let context = body
        .context
        .map(|m| m.into_iter().collect::<std::collections::HashMap<_, _>>());

    let input = RememberInput {
        content: body.content,
        importance: body.importance,
        context,
        entity_id: body.entity_id,
        edges: Vec::new(),
        kind: None,
    };

    let engine = state.engine.clone();
    let result =
        tokio::task::spawn_blocking(move || engine.remember_for_tenant(&tenant, input)).await;

    match result {
        Ok(Ok(memory)) => {
            let elapsed = start.elapsed().as_secs_f64();
            state.metrics.observe_operation("remember", "ok", elapsed);
            state.metrics.memory_count.inc();
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(memory_to_json(&memory)).unwrap()),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            let elapsed = start.elapsed().as_secs_f64();
            state
                .metrics
                .observe_operation("remember", "error", elapsed);
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn get_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let memory_id = match hex::decode(&id) {
        Ok(v) if v.len() == 16 => v,
        _ => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "memory_id must be a 32-character hex string (16 bytes)",
            )
            .into_response();
        }
    };

    let engine = state.engine.clone();
    let result =
        tokio::task::spawn_blocking(move || engine.get_for_tenant(&tenant, &memory_id)).await;

    match result {
        Ok(Ok(memory)) => (
            StatusCode::OK,
            Json(serde_json::to_value(memory_to_json(&memory)).unwrap()),
        )
            .into_response(),
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn recall_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Json(body): Json<RecallBody>,
) -> impl IntoResponse {
    let start = std::time::Instant::now();

    let strategies: Vec<RecallStrategy> = match body
        .strategies
        .iter()
        .map(|s| parse_strategy(s).ok_or_else(|| format!("unknown strategy: {}", s)))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(s) => s,
        Err(e) => {
            return json_error(StatusCode::BAD_REQUEST, "invalid_input", &e).into_response();
        }
    };

    let edge_types = body.edge_types.map(|types| {
        types
            .iter()
            .filter_map(|s| parse_edge_type(s))
            .collect::<Vec<EdgeType>>()
    });

    let seed_memory_id = body.seed_memory_id.and_then(|s| parse_hex_id_16(&s));

    let cue_context = body.cue_context.and_then(json_value_to_hashmap);

    let input = RecallInput {
        cue: body.cue,
        strategies,
        top_k: body.top_k,
        entity_id: body.entity_id,
        time_range: body.time_range.map(|tr| (tr.start_us, tr.end_us)),
        edge_types,
        max_depth: body.max_depth,
        ef_search: body.ef_search,
        scoring_weights: body.scoring_weights.map(scoring_weights_from_body),
        cue_context,
        causal_direction: None,
        analogy_a_id: None,
        analogy_b_id: None,
        seed_memory_id,
        analogical_alpha: body.analogical_alpha,
    };

    let engine = state.engine.clone();
    let tenant_clone = tenant.clone();
    let result =
        tokio::task::spawn_blocking(move || engine.recall_for_tenant(&tenant, input)).await;

    match result {
        Ok(Ok(output)) => {
            let elapsed = start.elapsed().as_secs_f64();
            state.metrics.observe_operation("recall", "ok", elapsed);

            let memories_ref: Vec<_> = output.results.iter().map(|r| &r.memory).collect();
            let lineage =
                convert::resolve_lineage_batch_refs(&state.engine, &tenant_clone, &memories_ref);
            let results: Vec<RecallResultJson> = output
                .results
                .iter()
                .map(|r| recall_result_to_rest_json_with_lineage(r, &lineage))
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::to_value(&results).unwrap()),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            let elapsed = start.elapsed().as_secs_f64();
            state.metrics.observe_operation("recall", "error", elapsed);
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn prime_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Json(body): Json<PrimeBody>,
) -> impl IntoResponse {
    let start = std::time::Instant::now();

    let context = body
        .context
        .map(|m| m.into_iter().collect::<std::collections::HashMap<_, _>>());

    let input = PrimeInput {
        entity_id: body.entity_id,
        context,
        max_memories: body.max_memories,
        recency_window_us: body.recency_window_us,
        similarity_cue: body.similarity_cue,
        scoring_weights: body.scoring_weights.map(scoring_weights_from_body),
    };

    let engine = state.engine.clone();
    let tenant_clone = tenant.clone();
    let result = tokio::task::spawn_blocking(move || engine.prime_for_tenant(&tenant, input)).await;

    match result {
        Ok(Ok(output)) => {
            let elapsed = start.elapsed().as_secs_f64();
            state.metrics.observe_operation("prime", "ok", elapsed);

            let memories_ref: Vec<_> = output.results.iter().map(|r| &r.memory).collect();
            let lineage =
                convert::resolve_lineage_batch_refs(&state.engine, &tenant_clone, &memories_ref);
            let results: Vec<RecallResultJson> = output
                .results
                .iter()
                .map(|r| recall_result_to_rest_json_with_lineage(r, &lineage))
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::to_value(&results).unwrap()),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn revise_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Path(id): Path<String>,
    Json(body): Json<ReviseBody>,
) -> impl IntoResponse {
    let start = std::time::Instant::now();

    let memory_id = match hex::decode(&id) {
        Ok(v) if v.len() == 16 => v,
        _ => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "memory_id must be a 32-character hex string (16 bytes)",
            )
            .into_response();
        }
    };

    let context = body
        .context
        .map(|m| m.into_iter().collect::<std::collections::HashMap<_, _>>());

    let context_mode = match body.context_mode.as_deref() {
        Some("replace") => ContextMode::Replace,
        _ => ContextMode::Merge,
    };

    let input = ReviseInput {
        memory_id,
        content: body.content,
        importance: body.importance,
        context,
        context_mode,
        entity_id: body.entity_id.map(Some),
        edges: Vec::new(),
    };

    let engine = state.engine.clone();
    let result =
        tokio::task::spawn_blocking(move || engine.revise_for_tenant(&tenant, input)).await;

    match result {
        Ok(Ok(memory)) => {
            let elapsed = start.elapsed().as_secs_f64();
            state.metrics.observe_operation("revise", "ok", elapsed);
            (
                StatusCode::OK,
                Json(serde_json::to_value(memory_to_json(&memory)).unwrap()),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            let elapsed = start.elapsed().as_secs_f64();
            state.metrics.observe_operation("revise", "error", elapsed);
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn forget_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Json(body): Json<ForgetBody>,
) -> impl IntoResponse {
    let start = std::time::Instant::now();

    let memory_ids: Vec<Vec<u8>> = body
        .memory_ids
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| hex::decode(&s).ok())
        .collect();

    let memory_kind = body.memory_kind.and_then(|k| match k.as_str() {
        "episode" => Some(MemoryKind::Episode),
        "insight" => Some(MemoryKind::Insight),
        "revision" => Some(MemoryKind::Revision),
        _ => None,
    });

    let criteria = ForgetCriteria {
        memory_ids,
        entity_id: body.entity_id,
        staleness_threshold_us: body.staleness_threshold_us,
        access_count_floor: body.access_count_floor,
        memory_kind,
        decay_score_floor: body.decay_score_floor,
    };

    let engine = state.engine.clone();
    let result =
        tokio::task::spawn_blocking(move || engine.forget_for_tenant(&tenant, criteria)).await;

    match result {
        Ok(Ok(output)) => {
            let elapsed = start.elapsed().as_secs_f64();
            state.metrics.observe_operation("forget", "ok", elapsed);
            (
                StatusCode::OK,
                Json(ForgetResponseJson {
                    forgotten_count: output.forgotten_count,
                    cascade_count: output.cascade_count,
                    truncated: output.truncated,
                    tombstone_count: output.tombstone_count,
                }),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn insights_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    axum::extract::Query(query): axum::extract::Query<InsightsQuery>,
) -> impl IntoResponse {
    let filter = InsightsFilter {
        entity_id: query.entity_id,
        min_confidence: query.min_confidence,
        max_results: query.max_results,
    };

    let engine = state.engine.clone();
    let tenant_clone = tenant.clone();
    let result =
        tokio::task::spawn_blocking(move || engine.insights_for_tenant(&tenant, filter)).await;

    match result {
        Ok(Ok(insights)) => {
            let lineage = convert::resolve_lineage_batch(&state.engine, &tenant_clone, &insights);
            let json: Vec<MemoryJson> = insights
                .iter()
                .map(|m| {
                    let mut id = [0u8; 16];
                    if m.memory_id.len() == 16 {
                        id.copy_from_slice(&m.memory_id);
                    }
                    let sources = lineage.get(&id).map(|v| v.as_slice()).unwrap_or(&[]);
                    memory_to_json_with_lineage(m, sources)
                })
                .collect();
            (StatusCode::OK, Json(serde_json::to_value(&json).unwrap())).into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Reflect Prepare / Commit (agent-driven two-step)
// ═══════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct ReflectPrepareBody {
    entity_id: Option<String>,
    since_us: Option<u64>,
}

#[derive(Serialize)]
struct ClusterMemoryJson {
    memory_id: String,
    content: String,
    importance: f32,
    entity_id: Option<String>,
    created_at: u64,
}

#[derive(Serialize)]
struct ClusterPromptJson {
    cluster_id: u32,
    member_count: u32,
    proposal_system_prompt: String,
    proposal_user_prompt: String,
    memory_ids: Vec<String>,
    validation_context: serde_json::Value,
    memories: Vec<ClusterMemoryJson>,
}

#[derive(Serialize)]
struct ReflectPrepareJson {
    session_id: String,
    memories_processed: usize,
    clusters: Vec<ClusterPromptJson>,
    existing_insight_count: usize,
}

#[derive(Deserialize)]
struct InsightInputBody {
    content: String,
    confidence: f32,
    source_memory_ids: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    cluster_id: Option<u32>,
}

#[derive(Deserialize)]
struct ReflectCommitBody {
    session_id: String,
    insights: Vec<InsightInputBody>,
}

#[derive(Serialize)]
struct ReflectCommitJson {
    insights_created: usize,
}

async fn reflect_prepare_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Json(body): Json<ReflectPrepareBody>,
) -> impl IntoResponse {
    let config = match &state.reflect_config {
        Some(c) => c.clone(),
        None => ReflectConfig::default().validated(),
    };

    let scope = if let Some(eid) = body.entity_id {
        hebbs_core::reflect::ReflectScope::Entity {
            entity_id: eid,
            since_us: body.since_us,
        }
    } else {
        hebbs_core::reflect::ReflectScope::Global {
            since_us: body.since_us,
        }
    };

    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        engine.reflect_prepare_for_tenant(&tenant, scope, &config)
    })
    .await;

    match result {
        Ok(Ok(output)) => {
            let clusters: Vec<ClusterPromptJson> = output
                .clusters
                .iter()
                .map(|c| ClusterPromptJson {
                    cluster_id: c.cluster_id,
                    member_count: c.member_count,
                    proposal_system_prompt: c.proposal_system_prompt.clone(),
                    proposal_user_prompt: c.proposal_user_prompt.clone(),
                    memory_ids: c.memory_ids.iter().map(hex::encode).collect(),
                    validation_context: serde_json::from_str(&c.validation_context)
                        .unwrap_or(serde_json::Value::Null),
                    memories: c
                        .memories
                        .iter()
                        .map(|m| ClusterMemoryJson {
                            memory_id: hex::encode(m.memory_id),
                            content: m.content.clone(),
                            importance: m.importance,
                            entity_id: m.entity_id.clone(),
                            created_at: m.created_at,
                        })
                        .collect(),
                })
                .collect();

            let resp = ReflectPrepareJson {
                session_id: output.session_id,
                memories_processed: output.memories_processed,
                clusters,
                existing_insight_count: output.existing_insight_count,
            };
            (StatusCode::OK, Json(serde_json::to_value(&resp).unwrap())).into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn reflect_commit_handler(
    State(state): State<AppState>,
    TenantExtractor(_tenant): TenantExtractor,
    Json(body): Json<ReflectCommitBody>,
) -> impl IntoResponse {
    let insights: Result<Vec<hebbs_reflect::ProducedInsight>, String> = body
        .insights
        .iter()
        .map(|i| {
            let source_ids: Result<Vec<[u8; 16]>, String> = i
                .source_memory_ids
                .iter()
                .map(|hex_id| {
                    let bytes = hex::decode(hex_id)
                        .map_err(|e| format!("invalid hex memory ID '{hex_id}': {e}"))?;
                    if bytes.len() != 16 {
                        return Err(format!(
                            "memory ID '{hex_id}' has {} bytes, expected 16",
                            bytes.len()
                        ));
                    }
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(&bytes);
                    Ok(arr)
                })
                .collect();

            Ok(hebbs_reflect::ProducedInsight {
                content: i.content.clone(),
                confidence: i.confidence,
                source_memory_ids: source_ids?,
                tags: i.tags.clone(),
                cluster_id: i.cluster_id.unwrap_or(0) as usize,
            })
        })
        .collect();

    let insights = match insights {
        Ok(v) => v,
        Err(msg) => {
            return json_error(StatusCode::BAD_REQUEST, "invalid_input", &msg).into_response()
        }
    };

    let engine = state.engine.clone();
    let session_id = body.session_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        engine.reflect_commit_for_tenant(
            &hebbs_core::tenant::TenantContext::default(),
            &session_id,
            insights,
        )
    })
    .await;

    match result {
        Ok(Ok(output)) => {
            let resp = ReflectCommitJson {
                insights_created: output.insights_created,
            };
            (StatusCode::OK, Json(serde_json::to_value(&resp).unwrap())).into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Contradiction Prepare / Commit (agent-driven two-step)
// ═══════════════════════════════════════════════════════════════════════

#[derive(Serialize)]
struct PendingContradictionJson {
    pending_id: String,
    memory_id_a: String,
    memory_id_b: String,
    content_a_snippet: String,
    content_b_snippet: String,
    classifier_score: f32,
    classifier_method: String,
    similarity: f32,
    created_at: u64,
}

#[derive(Deserialize)]
struct ContradictionVerdictBody {
    pending_id: String,
    verdict: String,
    confidence: f32,
    reasoning: Option<String>,
}

#[derive(Deserialize)]
struct ContradictionCommitBody {
    verdicts: Vec<ContradictionVerdictBody>,
}

#[derive(Serialize)]
struct ContradictionCommitJson {
    contradictions_confirmed: usize,
    revisions_created: usize,
    dismissed: usize,
}

async fn contradiction_prepare_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
) -> impl IntoResponse {
    let engine = state.engine.clone();
    let result =
        tokio::task::spawn_blocking(move || engine.contradiction_prepare_for_tenant(&tenant)).await;

    match result {
        Ok(Ok(pending)) => {
            let candidates: Vec<PendingContradictionJson> = pending
                .iter()
                .map(|p| PendingContradictionJson {
                    pending_id: hex::encode(p.id),
                    memory_id_a: hex::encode(p.memory_id_a),
                    memory_id_b: hex::encode(p.memory_id_b),
                    content_a_snippet: p.content_a_snippet.clone(),
                    content_b_snippet: p.content_b_snippet.clone(),
                    classifier_score: p.classifier_score,
                    classifier_method: match p.classifier_method {
                        hebbs_core::contradict::ClassifierMethod::Heuristic => {
                            "heuristic".to_string()
                        }
                        hebbs_core::contradict::ClassifierMethod::Llm => "llm".to_string(),
                    },
                    similarity: p.similarity,
                    created_at: p.created_at,
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::to_value(&candidates).unwrap()),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn contradiction_commit_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Json(body): Json<ContradictionCommitBody>,
) -> impl IntoResponse {
    use hebbs_core::contradict::ContradictionVerdict;

    let verdicts: Vec<ContradictionVerdict> = body
        .verdicts
        .iter()
        .map(|v| ContradictionVerdict {
            pending_id: v.pending_id.clone(),
            verdict: v.verdict.clone(),
            confidence: v.confidence,
            reasoning: v.reasoning.clone(),
        })
        .collect();

    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        engine.contradiction_commit_for_tenant(&tenant, &verdicts)
    })
    .await;

    match result {
        Ok(Ok(output)) => {
            let resp = ContradictionCommitJson {
                contradictions_confirmed: output.contradictions_confirmed,
                revisions_created: output.revisions_created,
                dismissed: output.dismissed,
            };
            (StatusCode::OK, Json(serde_json::to_value(&resp).unwrap())).into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  SSE Subscribe Types & Handlers
// ═══════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct SubscribeBody {
    entity_id: Option<String>,
    kind_filter: Option<Vec<String>>,
    confidence_threshold: Option<f32>,
    time_scope_us: Option<u64>,
}

#[derive(Deserialize)]
struct SseFeedBody {
    text: String,
}

fn parse_memory_kind(s: &str) -> Option<MemoryKind> {
    match s.to_lowercase().as_str() {
        "episode" => Some(MemoryKind::Episode),
        "insight" => Some(MemoryKind::Insight),
        "revision" => Some(MemoryKind::Revision),
        "document" => Some(MemoryKind::Document),
        "proposition" => Some(MemoryKind::Proposition),
        _ => None,
    }
}

async fn sse_subscribe_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Json(body): Json<SubscribeBody>,
) -> impl IntoResponse {
    let explicit_kinds: Vec<MemoryKind> = body
        .kind_filter
        .unwrap_or_default()
        .iter()
        .filter_map(|s| parse_memory_kind(s))
        .collect();

    let defaults = SubscribeConfig::default();

    let config = SubscribeConfig {
        entity_id: body.entity_id,
        memory_kinds: if explicit_kinds.is_empty() {
            defaults.memory_kinds
        } else {
            explicit_kinds
        },
        confidence_threshold: body.confidence_threshold.unwrap_or(0.5),
        time_scope_us: body.time_scope_us,
        ..defaults
    };

    let engine = state.engine.clone();
    let handle =
        match tokio::task::spawn_blocking(move || engine.subscribe_for_tenant(&tenant, config))
            .await
        {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                let (status, json) = map_hebbs_error(e);
                return (status, json).into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorJson {
                        error_code: "internal_error".to_string(),
                        message: format!("task join error: {}", e),
                    }),
                )
                    .into_response();
            }
        };

    let subscription_id = handle.id();
    state
        .sse_subscriptions
        .lock()
        .insert(subscription_id, SseSubscriptionEntry { handle });

    state.metrics.active_subscriptions.inc();

    let subscriptions = state.sse_subscriptions.clone();
    let metrics = state.metrics.clone();

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, std::convert::Infallible>>(128);

    tokio::spawn(async move {
        let mut sequence: u64 = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(5)).await;

            let push_opt = {
                let subs = subscriptions.lock();
                match subs.get(&subscription_id) {
                    Some(entry) => entry.handle.try_recv(),
                    None => break,
                }
            };

            if let Some(push) = push_opt {
                sequence += 1;
                let msg = serde_json::json!({
                    "subscription_id": subscription_id,
                    "memory": memory_to_json(&push.memory),
                    "confidence": push.confidence,
                    "push_timestamp_us": push.push_timestamp_us,
                    "sequence_number": sequence,
                });
                let event = Event::default().data(msg.to_string());
                if tx.send(Ok(event)).await.is_err() {
                    break;
                }
            }

            if tx.is_closed() {
                break;
            }
        }

        let mut subs = subscriptions.lock();
        if let Some(mut entry) = subs.remove(&subscription_id) {
            entry.handle.close();
        }
        metrics.active_subscriptions.dec();
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn sse_feed_handler(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(body): Json<SseFeedBody>,
) -> impl IntoResponse {
    let subs = state.sse_subscriptions.lock();
    let entry = match subs.get(&id) {
        Some(e) => e,
        None => {
            return json_error(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("subscription {} not found", id),
            )
            .into_response();
        }
    };

    let _ = entry.handle.feed(&body.text);
    (StatusCode::OK, Json(serde_json::json!({}))).into_response()
}

async fn sse_close_handler(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let mut subs = state.sse_subscriptions.lock();
    if let Some(mut entry) = subs.remove(&id) {
        entry.handle.close();
        state.metrics.active_subscriptions.dec();
    }

    (StatusCode::OK, Json(serde_json::json!({}))).into_response()
}

// ═══════════════════════════════════════════════════════════════════════
//  Graph & Exploration Handlers
// ═══════════════════════════════════════════════════════════════════════

async fn list_memories_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    axum::extract::Query(query): axum::extract::Query<ListMemoriesQuery>,
) -> impl IntoResponse {
    let kind = query.kind.and_then(|k| parse_memory_kind(&k));
    let limit = query.limit.unwrap_or(100).min(1000);
    let cursor = query.cursor.and_then(|s| parse_hex_id_16(&s));

    let entity_id = query.entity_id.clone();
    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        engine.list_memories_for_tenant(&tenant, entity_id.as_deref(), kind, cursor.as_ref(), limit)
    })
    .await;

    match result {
        Ok(Ok(memories)) => {
            let next_cursor = if memories.len() >= limit {
                memories.last().map(|m| hex::encode(&m.memory_id))
            } else {
                None
            };
            let json_memories: Vec<MemoryJson> = memories.iter().map(memory_to_json).collect();
            let resp = ListMemoriesResponseJson {
                memories: json_memories,
                next_cursor,
            };
            (StatusCode::OK, Json(serde_json::to_value(&resp).unwrap())).into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn edges_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let memory_id = match parse_hex_id_16(&id) {
        Some(v) => v,
        None => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "memory_id must be a 32-character hex string (16 bytes)",
            )
            .into_response();
        }
    };

    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let outgoing = engine.outgoing_edges_for_tenant(&tenant, &memory_id)?;
        let incoming = engine.incoming_edges_for_tenant(&tenant, &memory_id)?;
        Ok::<_, hebbs_core::error::HebbsError>((outgoing, incoming))
    })
    .await;

    match result {
        Ok(Ok((outgoing, incoming))) => {
            let out_json: Vec<EdgeJson> = outgoing
                .iter()
                .map(|(et, target, meta)| EdgeJson {
                    source_id: id.clone(),
                    target_id: hex::encode(target),
                    edge_type: edge_type_to_str(*et).to_string(),
                    confidence: meta.confidence,
                    timestamp_us: meta.timestamp_us,
                })
                .collect();
            let in_json: Vec<EdgeJson> = incoming
                .iter()
                .map(|(et, source, meta)| EdgeJson {
                    source_id: hex::encode(source),
                    target_id: id.clone(),
                    edge_type: edge_type_to_str(*et).to_string(),
                    confidence: meta.confidence,
                    timestamp_us: meta.timestamp_us,
                })
                .collect();
            let resp = EdgesResponseJson {
                outgoing: out_json,
                incoming: in_json,
            };
            (StatusCode::OK, Json(serde_json::to_value(&resp).unwrap())).into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn graph_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Json(body): Json<GraphBody>,
) -> impl IntoResponse {
    let center_id = match parse_hex_id_16(&body.center_id) {
        Some(v) => v,
        None => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "center_id must be a 32-character hex string (16 bytes)",
            )
            .into_response();
        }
    };

    let max_depth = body.depth.unwrap_or(2).min(5);
    let max_nodes = body.max_nodes.unwrap_or(100).min(500);

    let edge_types: Vec<EdgeType> = body
        .edge_types
        .map(|types| types.iter().filter_map(|s| parse_edge_type(s)).collect())
        .unwrap_or_else(|| {
            vec![
                EdgeType::CausedBy,
                EdgeType::RelatedTo,
                EdgeType::FollowedBy,
                EdgeType::RevisedFrom,
                EdgeType::InsightFrom,
                EdgeType::Contradicts,
                EdgeType::HasEntity,
                EdgeType::EntityRelation,
                EdgeType::PropositionOf,
            ]
        });

    let engine = state.engine.clone();
    let tenant_clone = tenant.clone();
    let result = tokio::task::spawn_blocking(move || {
        let (traversal, truncated) = engine.traverse_graph_for_tenant(
            &tenant,
            &center_id,
            &edge_types,
            max_depth,
            max_nodes,
        )?;

        // Collect all unique node IDs (center + traversal results)
        let mut node_ids: Vec<[u8; 16]> = Vec::with_capacity(traversal.len() + 1);
        node_ids.push(center_id);
        let mut seen = std::collections::HashSet::new();
        seen.insert(center_id);
        for entry in &traversal {
            if seen.insert(entry.memory_id) {
                node_ids.push(entry.memory_id);
            }
        }

        let memories = engine.batch_get_for_tenant(&tenant_clone, &node_ids)?;

        // Collect edges for all nodes in the subgraph
        let mut edges = Vec::new();
        for nid in &node_ids {
            let outgoing = engine.outgoing_edges_for_tenant(&tenant_clone, nid)?;
            for (et, target, meta) in outgoing {
                if seen.contains(&target) {
                    edges.push((*nid, et, target, meta));
                }
            }
        }

        Ok::<_, hebbs_core::error::HebbsError>((memories, edges, truncated))
    })
    .await;

    match result {
        Ok(Ok((memories, edges, truncated))) => {
            let nodes: Vec<MemoryJson> = memories.iter().map(memory_to_json).collect();
            let edge_json: Vec<EdgeJson> = edges
                .iter()
                .map(|(source, et, target, meta)| EdgeJson {
                    source_id: hex::encode(source),
                    target_id: hex::encode(target),
                    edge_type: edge_type_to_str(*et).to_string(),
                    confidence: meta.confidence,
                    timestamp_us: meta.timestamp_us,
                })
                .collect();
            let resp = GraphResponseJson {
                nodes,
                edges: edge_json,
                truncated,
            };
            (StatusCode::OK, Json(serde_json::to_value(&resp).unwrap())).into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn entities_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
) -> impl IntoResponse {
    let engine = state.engine.clone();
    let result =
        tokio::task::spawn_blocking(move || engine.list_entities_for_tenant(&tenant)).await;

    match result {
        Ok(Ok(entities)) => (
            StatusCode::OK,
            Json(serde_json::to_value(&entities).unwrap()),
        )
            .into_response(),
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn batch_get_handler(
    State(state): State<AppState>,
    TenantExtractor(tenant): TenantExtractor,
    Json(body): Json<BatchGetBody>,
) -> impl IntoResponse {
    if body.memory_ids.len() > 500 {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_input",
            "batch size must not exceed 500",
        )
        .into_response();
    }

    let ids: Vec<[u8; 16]> = body
        .memory_ids
        .iter()
        .filter_map(|s| parse_hex_id_16(s))
        .collect();

    let engine = state.engine.clone();
    let result =
        tokio::task::spawn_blocking(move || engine.batch_get_for_tenant(&tenant, &ids)).await;

    match result {
        Ok(Ok(memories)) => {
            let json_memories: Vec<MemoryJson> = memories.iter().map(memory_to_json).collect();
            (
                StatusCode::OK,
                Json(serde_json::to_value(&json_memories).unwrap()),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            let (status, json) = map_hebbs_error(e);
            (status, json).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error_code: "internal_error".to_string(),
                message: format!("task join error: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn liveness_handler() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"status": "alive"})))
}

async fn readiness_handler(State(state): State<AppState>) -> impl IntoResponse {
    // Check data directory still exists on disk.
    if !state.data_dir.exists() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "not_ready",
                "reason": "data directory missing"
            })),
        )
            .into_response();
    }

    let engine = state.engine.clone();
    let count_result = tokio::task::spawn_blocking(move || engine.count()).await;

    match count_result {
        Ok(Ok(_)) => (StatusCode::OK, Json(serde_json::json!({"status": "ready"}))).into_response(),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "not_ready"})),
        )
            .into_response(),
    }
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let body = state.metrics.render();
    (
        StatusCode::OK,
        [(
            http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}
