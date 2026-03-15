use std::collections::HashMap;

use hebbs_core::engine::{RememberEdge, RememberInput};
use hebbs_core::forget::ForgetCriteria;
use hebbs_core::memory::{Memory, MemoryKind};
use hebbs_core::recall::{PrimeInput, RecallInput, RecallStrategy, ScoringWeights, StrategyDetail};
use hebbs_core::reflect::{ClusterMemorySummary, InsightsFilter, PreparedCluster, ReflectScope};
use hebbs_core::revise::{ContextMode, ReviseInput};
use hebbs_index::EdgeType;

use hebbs_proto::generated as pb;

// ═══════════════════════════════════════════════════════════════════════
//  Memory <-> Proto
// ═══════════════════════════════════════════════════════════════════════

pub fn memory_to_proto(m: &Memory) -> pb::Memory {
    memory_to_proto_with_lineage(m, &[])
}

/// Convert a Memory to proto, attaching source_memory_ids for Insight-kind memories.
pub fn memory_to_proto_with_lineage(m: &Memory, source_ids: &[[u8; 16]]) -> pb::Memory {
    let context = context_bytes_to_struct(&m.context_bytes);

    pb::Memory {
        memory_id: m.memory_id.clone(),
        content: m.content.clone(),
        importance: m.importance,
        context,
        entity_id: m.entity_id.clone(),
        embedding: m.embedding.clone().unwrap_or_default(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        last_accessed_at: m.last_accessed_at,
        access_count: m.access_count,
        decay_score: m.decay_score,
        kind: memory_kind_to_proto(m.kind) as i32,
        device_id: m.device_id.clone(),
        logical_clock: m.logical_clock,
        source_memory_ids: source_ids.iter().map(|id| id.to_vec()).collect(),
    }
}

/// Resolve InsightFrom edges for a batch of memories.
/// Returns a map from memory_id → Vec<source_id> for Insight-kind memories only.
/// O(n) edge lookups where n = number of insights in the batch.
pub fn resolve_lineage_batch(
    engine: &hebbs_core::engine::Engine,
    tenant: &hebbs_core::tenant::TenantContext,
    memories: &[Memory],
) -> HashMap<[u8; 16], Vec<[u8; 16]>> {
    resolve_lineage_batch_refs(engine, tenant, &memories.iter().collect::<Vec<_>>())
}

/// Same as [`resolve_lineage_batch`] but accepts a slice of references.
pub fn resolve_lineage_batch_refs(
    engine: &hebbs_core::engine::Engine,
    tenant: &hebbs_core::tenant::TenantContext,
    memories: &[&Memory],
) -> HashMap<[u8; 16], Vec<[u8; 16]>> {
    let mut lineage = HashMap::new();
    for m in memories {
        if m.kind != MemoryKind::Insight {
            continue;
        }
        if m.memory_id.len() != 16 {
            continue;
        }
        let mut id = [0u8; 16];
        id.copy_from_slice(&m.memory_id);
        let source_ids = engine
            .outgoing_edges_for_tenant(tenant, &id)
            .unwrap_or_default()
            .into_iter()
            .filter(|(et, _, _)| *et == EdgeType::InsightFrom)
            .map(|(_, target, _)| target)
            .collect::<Vec<_>>();
        if !source_ids.is_empty() {
            lineage.insert(id, source_ids);
        }
    }
    lineage
}

/// Look up lineage source IDs from a pre-built lineage map for a given memory.
pub fn get_lineage_for_memory(
    lineage: &HashMap<[u8; 16], Vec<[u8; 16]>>,
    memory_id: &[u8],
) -> Vec<[u8; 16]> {
    if memory_id.len() != 16 {
        return Vec::new();
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(memory_id);
    lineage.get(&id).cloned().unwrap_or_default()
}

fn memory_kind_to_proto(k: MemoryKind) -> pb::MemoryKind {
    match k {
        MemoryKind::Episode => pb::MemoryKind::Episode,
        MemoryKind::Insight => pb::MemoryKind::Insight,
        MemoryKind::Revision => pb::MemoryKind::Revision,
    }
}

fn proto_to_memory_kind(k: i32) -> Option<MemoryKind> {
    match pb::MemoryKind::try_from(k) {
        Ok(pb::MemoryKind::Episode) => Some(MemoryKind::Episode),
        Ok(pb::MemoryKind::Insight) => Some(MemoryKind::Insight),
        Ok(pb::MemoryKind::Revision) => Some(MemoryKind::Revision),
        _ => None,
    }
}

fn context_bytes_to_struct(bytes: &[u8]) -> Option<prost_types::Struct> {
    if bytes.is_empty() {
        return None;
    }
    let map: HashMap<String, serde_json::Value> = match serde_json::from_slice(bytes) {
        Ok(m) => m,
        Err(_) => return None,
    };
    Some(json_map_to_struct(&map))
}

#[allow(dead_code)]
fn struct_to_context_bytes(s: &Option<prost_types::Struct>) -> Vec<u8> {
    match s {
        Some(st) => {
            let map = struct_to_json_map(st);
            serde_json::to_vec(&map).unwrap_or_default()
        }
        None => Vec::new(),
    }
}

pub fn struct_to_json_map(s: &prost_types::Struct) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::new();
    for (k, v) in &s.fields {
        map.insert(k.clone(), prost_value_to_json(v));
    }
    map
}

fn prost_value_to_json(v: &prost_types::Value) -> serde_json::Value {
    use prost_types::value::Kind;
    match &v.kind {
        Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::NumberValue(n)) => serde_json::json!(*n),
        Some(Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(Kind::StructValue(s)) => {
            serde_json::Value::Object(struct_to_json_map(s).into_iter().collect())
        }
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.iter().map(prost_value_to_json).collect())
        }
        None => serde_json::Value::Null,
    }
}

fn json_map_to_struct(map: &HashMap<String, serde_json::Value>) -> prost_types::Struct {
    prost_types::Struct {
        fields: map
            .iter()
            .map(|(k, v)| (k.clone(), json_to_prost_value(v)))
            .collect(),
    }
}

fn json_to_prost_value(v: &serde_json::Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match v {
        serde_json::Value::Null => Kind::NullValue(0),
        serde_json::Value::Bool(b) => Kind::BoolValue(*b),
        serde_json::Value::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Kind::StringValue(s.clone()),
        serde_json::Value::Array(arr) => Kind::ListValue(prost_types::ListValue {
            values: arr.iter().map(json_to_prost_value).collect(),
        }),
        serde_json::Value::Object(obj) => {
            let map: HashMap<String, serde_json::Value> =
                obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            Kind::StructValue(json_map_to_struct(&map))
        }
    };
    prost_types::Value { kind: Some(kind) }
}

// ═══════════════════════════════════════════════════════════════════════
//  Remember
// ═══════════════════════════════════════════════════════════════════════

pub fn proto_to_remember_input(req: pb::RememberRequest) -> Result<RememberInput, String> {
    let context = req.context.as_ref().map(struct_to_json_map);

    let edges = req
        .edges
        .into_iter()
        .map(proto_to_remember_edge)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(RememberInput {
        content: req.content,
        importance: req.importance,
        context,
        entity_id: req.entity_id,
        edges,
    })
}

fn proto_to_remember_edge(e: pb::Edge) -> Result<RememberEdge, String> {
    if e.target_id.len() != 16 {
        return Err(format!(
            "edge target_id must be 16 bytes, got {}",
            e.target_id.len()
        ));
    }
    let mut target_id = [0u8; 16];
    target_id.copy_from_slice(&e.target_id);

    Ok(RememberEdge {
        target_id,
        edge_type: proto_to_edge_type(e.edge_type)?,
        confidence: e.confidence,
    })
}

fn proto_to_edge_type(v: i32) -> Result<EdgeType, String> {
    match pb::EdgeType::try_from(v) {
        Ok(pb::EdgeType::CausedBy) => Ok(EdgeType::CausedBy),
        Ok(pb::EdgeType::RelatedTo) => Ok(EdgeType::RelatedTo),
        Ok(pb::EdgeType::FollowedBy) => Ok(EdgeType::FollowedBy),
        Ok(pb::EdgeType::RevisedFrom) => Ok(EdgeType::RevisedFrom),
        Ok(pb::EdgeType::InsightFrom) => Ok(EdgeType::InsightFrom),
        Ok(pb::EdgeType::Contradicts) => Ok(EdgeType::Contradicts),
        _ => Err(format!("invalid edge type: {}", v)),
    }
}

fn edge_type_to_proto(e: EdgeType) -> pb::EdgeType {
    match e {
        EdgeType::CausedBy => pb::EdgeType::CausedBy,
        EdgeType::RelatedTo => pb::EdgeType::RelatedTo,
        EdgeType::FollowedBy => pb::EdgeType::FollowedBy,
        EdgeType::RevisedFrom => pb::EdgeType::RevisedFrom,
        EdgeType::InsightFrom => pb::EdgeType::InsightFrom,
        EdgeType::Contradicts => pb::EdgeType::Contradicts,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall
// ═══════════════════════════════════════════════════════════════════════

pub fn proto_to_recall_input(req: pb::RecallRequest) -> Result<RecallInput, String> {
    let mut strategies = Vec::new();
    let mut entity_id = None;
    let mut time_range = None;
    let mut edge_types_vec = None;
    let mut max_depth = None;
    let mut ef_search = None;
    let mut seed_memory_id = None;
    let mut analogical_alpha = None;

    for sc in &req.strategies {
        let strategy = match pb::RecallStrategyType::try_from(sc.strategy_type) {
            Ok(pb::RecallStrategyType::Similarity) => RecallStrategy::Similarity,
            Ok(pb::RecallStrategyType::Temporal) => RecallStrategy::Temporal,
            Ok(pb::RecallStrategyType::Causal) => RecallStrategy::Causal,
            Ok(pb::RecallStrategyType::Analogical) => RecallStrategy::Analogical,
            _ => {
                return Err(format!(
                    "invalid recall strategy type: {}",
                    sc.strategy_type
                ))
            }
        };
        strategies.push(strategy);

        if let Some(ref eid) = sc.entity_id {
            entity_id = Some(eid.clone());
        }
        if let Some(ref tr) = sc.time_range {
            time_range = Some((tr.start_us, tr.end_us));
        }
        if !sc.edge_types.is_empty() {
            let mut ets = Vec::new();
            for &et in &sc.edge_types {
                ets.push(proto_to_edge_type(et)?);
            }
            edge_types_vec = Some(ets);
        }
        if let Some(d) = sc.max_depth {
            max_depth = Some(d as usize);
        }
        if let Some(e) = sc.ef_search {
            ef_search = Some(e as usize);
        }
        if let Some(ref bytes) = sc.seed_memory_id {
            if bytes.len() == 16 {
                let mut id = [0u8; 16];
                id.copy_from_slice(bytes);
                seed_memory_id = Some(id);
            }
        }
        if let Some(a) = sc.analogical_alpha {
            analogical_alpha = Some(a);
        }
    }

    let scoring_weights = req.scoring_weights.map(|w| proto_to_scoring_weights(&w));

    let cue_context = req.cue_context.as_ref().map(struct_to_json_map);

    Ok(RecallInput {
        cue: req.cue,
        strategies,
        top_k: req.top_k.map(|v| v as usize),
        entity_id,
        time_range,
        edge_types: edge_types_vec,
        max_depth,
        ef_search,
        scoring_weights,
        cue_context,
        causal_direction: None,
        analogy_a_id: None,
        analogy_b_id: None,
        seed_memory_id,
        analogical_alpha,
    })
}

fn proto_to_scoring_weights(w: &pb::ScoringWeights) -> ScoringWeights {
    ScoringWeights {
        w_relevance: w.w_relevance,
        w_recency: w.w_recency,
        w_importance: w.w_importance,
        w_reinforcement: w.w_reinforcement,
        max_age_us: w.max_age_us,
        reinforcement_cap: w.reinforcement_cap,
    }
}

pub fn recall_result_to_proto(r: &hebbs_core::recall::RecallResult) -> pb::RecallResult {
    recall_result_to_proto_with_lineage(r, &HashMap::new())
}

pub fn recall_result_to_proto_with_lineage(
    r: &hebbs_core::recall::RecallResult,
    lineage: &HashMap<[u8; 16], Vec<[u8; 16]>>,
) -> pb::RecallResult {
    let sources = get_lineage_for_memory(lineage, &r.memory.memory_id);
    pb::RecallResult {
        memory: Some(memory_to_proto_with_lineage(&r.memory, &sources)),
        score: r.score,
        strategy_details: r
            .strategy_details
            .iter()
            .map(strategy_detail_to_proto)
            .collect(),
    }
}

fn strategy_detail_to_proto(d: &StrategyDetail) -> pb::StrategyDetailMessage {
    match d {
        StrategyDetail::Similarity {
            distance,
            relevance,
        } => pb::StrategyDetailMessage {
            strategy_type: pb::RecallStrategyType::Similarity as i32,
            relevance: *relevance,
            distance: Some(*distance),
            ..Default::default()
        },
        StrategyDetail::Temporal {
            timestamp,
            rank,
            relevance,
        } => pb::StrategyDetailMessage {
            strategy_type: pb::RecallStrategyType::Temporal as i32,
            relevance: *relevance,
            timestamp: Some(*timestamp),
            rank: Some(*rank as u32),
            ..Default::default()
        },
        StrategyDetail::Causal {
            depth,
            edge_type,
            seed_id,
            relevance,
        } => pb::StrategyDetailMessage {
            strategy_type: pb::RecallStrategyType::Causal as i32,
            relevance: *relevance,
            depth: Some(*depth as u32),
            causal_edge_type: Some(edge_type_to_proto(*edge_type) as i32),
            seed_id: Some(seed_id.to_vec()),
            ..Default::default()
        },
        StrategyDetail::Analogical {
            embedding_similarity,
            structural_similarity,
            relevance,
            ..
        } => pb::StrategyDetailMessage {
            strategy_type: pb::RecallStrategyType::Analogical as i32,
            relevance: *relevance,
            embedding_similarity: Some(*embedding_similarity),
            structural_similarity: Some(*structural_similarity),
            ..Default::default()
        },
    }
}

#[allow(dead_code)]
fn recall_strategy_to_proto(s: &RecallStrategy) -> pb::RecallStrategyType {
    match s {
        RecallStrategy::Similarity => pb::RecallStrategyType::Similarity,
        RecallStrategy::Temporal => pb::RecallStrategyType::Temporal,
        RecallStrategy::Causal => pb::RecallStrategyType::Causal,
        RecallStrategy::Analogical => pb::RecallStrategyType::Analogical,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Prime
// ═══════════════════════════════════════════════════════════════════════

pub fn proto_to_prime_input(req: pb::PrimeRequest) -> Result<PrimeInput, String> {
    let context = req.context.as_ref().map(struct_to_json_map);
    let scoring_weights = req.scoring_weights.map(|w| proto_to_scoring_weights(&w));

    Ok(PrimeInput {
        entity_id: req.entity_id,
        context,
        max_memories: req.max_memories.map(|v| v as usize),
        recency_window_us: req.recency_window_us,
        similarity_cue: req.similarity_cue,
        scoring_weights,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  Revise
// ═══════════════════════════════════════════════════════════════════════

pub fn proto_to_revise_input(req: pb::ReviseRequest) -> Result<ReviseInput, String> {
    let context = req.context.as_ref().map(struct_to_json_map);

    let context_mode = match pb::ContextMode::try_from(req.context_mode) {
        Ok(pb::ContextMode::Replace) => ContextMode::Replace,
        _ => ContextMode::Merge,
    };

    let edges = req
        .edges
        .into_iter()
        .map(proto_to_remember_edge)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ReviseInput {
        memory_id: req.memory_id,
        content: req.content,
        importance: req.importance,
        context,
        context_mode,
        entity_id: req.entity_id.map(Some),
        edges,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  Forget
// ═══════════════════════════════════════════════════════════════════════

pub fn proto_to_forget_criteria(req: pb::ForgetRequest) -> Result<ForgetCriteria, String> {
    Ok(ForgetCriteria {
        memory_ids: req.memory_ids,
        entity_id: req.entity_id,
        staleness_threshold_us: req.staleness_threshold_us,
        access_count_floor: req.access_count_floor,
        memory_kind: req.memory_kind.and_then(proto_to_memory_kind),
        decay_score_floor: req.decay_score_floor,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  Reflect
// ═══════════════════════════════════════════════════════════════════════

pub fn proto_to_reflect_scope(req: &pb::ReflectRequest) -> Result<ReflectScope, String> {
    let scope_msg = req
        .scope
        .as_ref()
        .ok_or_else(|| "scope is required".to_string())?;

    match &scope_msg.scope {
        Some(pb::reflect_scope::Scope::Entity(e)) => Ok(ReflectScope::Entity {
            entity_id: e.entity_id.clone(),
            since_us: e.since_us,
        }),
        Some(pb::reflect_scope::Scope::Global(g)) => Ok(ReflectScope::Global {
            since_us: g.since_us,
        }),
        None => Err("scope variant is required".to_string()),
    }
}

pub fn proto_to_insights_filter(req: &pb::GetInsightsRequest) -> InsightsFilter {
    InsightsFilter {
        entity_id: req.entity_id.clone(),
        min_confidence: req.min_confidence,
        max_results: req.max_results.map(|v| v as usize),
    }
}

pub fn proto_to_reflect_prepare_scope(
    req: &pb::ReflectPrepareRequest,
) -> Result<ReflectScope, String> {
    let scope_msg = req
        .scope
        .as_ref()
        .ok_or_else(|| "scope is required".to_string())?;

    match &scope_msg.scope {
        Some(pb::reflect_scope::Scope::Entity(e)) => Ok(ReflectScope::Entity {
            entity_id: e.entity_id.clone(),
            since_us: e.since_us,
        }),
        Some(pb::reflect_scope::Scope::Global(g)) => Ok(ReflectScope::Global {
            since_us: g.since_us,
        }),
        None => Err("scope variant is required".to_string()),
    }
}

pub fn prepared_cluster_to_proto(c: &PreparedCluster) -> pb::ClusterPrompt {
    pb::ClusterPrompt {
        cluster_id: c.cluster_id,
        member_count: c.member_count,
        proposal_system_prompt: c.proposal_system_prompt.clone(),
        proposal_user_prompt: c.proposal_user_prompt.clone(),
        memory_ids: c.memory_ids.iter().map(hex::encode).collect(),
        validation_context: c.validation_context.clone(),
        memories: c.memories.iter().map(memory_summary_to_proto).collect(),
    }
}

pub fn memory_summary_to_proto(m: &ClusterMemorySummary) -> pb::ClusterMemorySummary {
    pb::ClusterMemorySummary {
        memory_id: hex::encode(m.memory_id),
        content: m.content.clone(),
        importance: m.importance,
        entity_id: m.entity_id.clone(),
        created_at: m.created_at,
    }
}

pub fn proto_to_produced_insight(
    p: &pb::ProducedInsightInput,
) -> Result<hebbs_reflect::ProducedInsight, String> {
    let source_memory_ids: Result<Vec<[u8; 16]>, String> = p
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
        content: p.content.clone(),
        confidence: p.confidence,
        source_memory_ids: source_memory_ids?,
        tags: p.tags.clone(),
        cluster_id: p.cluster_id.unwrap_or(0) as usize,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  Error mapping
// ═══════════════════════════════════════════════════════════════════════

pub fn hebbs_error_to_status(e: hebbs_core::error::HebbsError) -> tonic::Status {
    use hebbs_core::error::HebbsError;
    match &e {
        HebbsError::MemoryNotFound { .. } => tonic::Status::not_found(e.to_string()),
        HebbsError::InvalidInput { .. } => tonic::Status::invalid_argument(e.to_string()),
        HebbsError::Serialization { .. } => tonic::Status::internal(e.to_string()),
        HebbsError::Internal { .. } => tonic::Status::internal(e.to_string()),
        HebbsError::Storage(_) => tonic::Status::unavailable(e.to_string()),
        HebbsError::Embedding(_) => tonic::Status::internal(e.to_string()),
        HebbsError::Index(_) => tonic::Status::internal(e.to_string()),
        HebbsError::Reflect(_) => tonic::Status::unavailable(e.to_string()),
        HebbsError::Unauthorized { .. } => tonic::Status::unauthenticated(e.to_string()),
        HebbsError::Forbidden { .. } => tonic::Status::permission_denied(e.to_string()),
        HebbsError::RateLimited { .. } => tonic::Status::resource_exhausted(e.to_string()),
        HebbsError::TenantNotFound { .. } => tonic::Status::not_found(e.to_string()),
        _ => tonic::Status::internal(e.to_string()),
    }
}

pub fn hebbs_error_to_http_status(e: &hebbs_core::error::HebbsError) -> (http::StatusCode, String) {
    use hebbs_core::error::HebbsError;
    match e {
        HebbsError::MemoryNotFound { .. } => (http::StatusCode::NOT_FOUND, e.to_string()),
        HebbsError::TenantNotFound { .. } => (http::StatusCode::NOT_FOUND, e.to_string()),
        HebbsError::InvalidInput { .. } => (http::StatusCode::BAD_REQUEST, e.to_string()),
        HebbsError::Storage(_) => (http::StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
        HebbsError::Reflect(_) => (http::StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
        HebbsError::Unauthorized { .. } => (http::StatusCode::UNAUTHORIZED, e.to_string()),
        HebbsError::Forbidden { .. } => (http::StatusCode::FORBIDDEN, e.to_string()),
        HebbsError::RateLimited { .. } => (http::StatusCode::TOO_MANY_REQUESTS, e.to_string()),
        _ => (http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_struct_round_trip() {
        let mut map = HashMap::new();
        map.insert("key".to_string(), serde_json::json!("value"));
        map.insert("num".to_string(), serde_json::json!(42.0));
        map.insert("flag".to_string(), serde_json::json!(true));

        let s = json_map_to_struct(&map);
        let back = struct_to_json_map(&s);

        assert_eq!(
            back.get("key").unwrap(),
            &serde_json::Value::String("value".to_string())
        );
        assert_eq!(back.get("num").unwrap(), &serde_json::json!(42.0));
        assert_eq!(back.get("flag").unwrap(), &serde_json::json!(true));
    }

    #[test]
    fn edge_type_round_trip() {
        let types = [
            EdgeType::CausedBy,
            EdgeType::RelatedTo,
            EdgeType::FollowedBy,
            EdgeType::RevisedFrom,
            EdgeType::InsightFrom,
            EdgeType::Contradicts,
        ];
        for et in types {
            let proto = edge_type_to_proto(et) as i32;
            let back = proto_to_edge_type(proto).unwrap();
            assert_eq!(et, back);
        }
    }

    #[test]
    fn memory_kind_round_trip() {
        let kinds = [
            MemoryKind::Episode,
            MemoryKind::Insight,
            MemoryKind::Revision,
        ];
        for k in kinds {
            let proto = memory_kind_to_proto(k) as i32;
            let back = proto_to_memory_kind(proto).unwrap();
            assert_eq!(k, back);
        }
    }

    #[test]
    fn error_mapping_not_found() {
        let err = hebbs_core::error::HebbsError::MemoryNotFound {
            memory_id: "test".to_string(),
        };
        let status = hebbs_error_to_status(err);
        assert_eq!(status.code(), tonic::Code::NotFound);
    }

    #[test]
    fn error_mapping_invalid_input() {
        let err = hebbs_core::error::HebbsError::InvalidInput {
            operation: "test",
            message: "bad".to_string(),
        };
        let status = hebbs_error_to_status(err);
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn memory_to_proto_with_lineage_populates_source_ids() {
        let m = Memory {
            memory_id: vec![1u8; 16],
            content: "test insight".into(),
            importance: 0.8,
            context_bytes: vec![],
            entity_id: Some("ent".into()),
            embedding: None,
            created_at: 100,
            updated_at: 100,
            last_accessed_at: 100,
            access_count: 1,
            decay_score: 1.0,
            kind: MemoryKind::Insight,
            device_id: None,
            logical_clock: 1,
            associative_embedding: None,
        };
        let source_a = [2u8; 16];
        let source_b = [3u8; 16];
        let proto = memory_to_proto_with_lineage(&m, &[source_a, source_b]);
        assert_eq!(proto.source_memory_ids.len(), 2);
        assert_eq!(proto.source_memory_ids[0], source_a.to_vec());
        assert_eq!(proto.source_memory_ids[1], source_b.to_vec());
    }

    #[test]
    fn memory_to_proto_without_lineage_has_empty_sources() {
        let m = Memory {
            memory_id: vec![1u8; 16],
            content: "test episode".into(),
            importance: 0.5,
            context_bytes: vec![],
            entity_id: None,
            embedding: None,
            created_at: 100,
            updated_at: 100,
            last_accessed_at: 100,
            access_count: 1,
            decay_score: 1.0,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 1,
            associative_embedding: None,
        };
        let proto = memory_to_proto(&m);
        assert!(proto.source_memory_ids.is_empty());
    }

    #[test]
    fn get_lineage_for_memory_returns_correct_sources() {
        let id_a = [1u8; 16];
        let source_1 = [10u8; 16];
        let source_2 = [11u8; 16];
        let mut lineage = HashMap::new();
        lineage.insert(id_a, vec![source_1, source_2]);

        let result = get_lineage_for_memory(&lineage, &id_a);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], source_1);

        // Non-existent ID returns empty
        let id_b = [2u8; 16];
        let result = get_lineage_for_memory(&lineage, &id_b);
        assert!(result.is_empty());

        // Invalid-length ID returns empty
        let result = get_lineage_for_memory(&lineage, &[0u8; 8]);
        assert!(result.is_empty());
    }
}
