use std::collections::HashMap;

use ulid::Ulid;

use hebbs_proto::generated as pb;

use crate::types::*;

// ═══════════════════════════════════════════════════════════════════════
//  Memory conversions
// ═══════════════════════════════════════════════════════════════════════

pub fn proto_memory_to_domain(m: &pb::Memory) -> Result<Memory, String> {
    let memory_id = bytes_to_ulid(&m.memory_id)?;
    let context = match &m.context {
        Some(s) => struct_to_json_map(s),
        None => HashMap::new(),
    };
    let embedding = if m.embedding.is_empty() {
        None
    } else {
        Some(m.embedding.clone())
    };

    Ok(Memory {
        memory_id,
        content: m.content.clone(),
        importance: m.importance,
        context,
        entity_id: m.entity_id.clone(),
        embedding,
        created_at: m.created_at,
        updated_at: m.updated_at,
        last_accessed_at: m.last_accessed_at,
        access_count: m.access_count,
        decay_score: m.decay_score,
        kind: proto_to_memory_kind(m.kind),
        device_id: m.device_id.clone(),
        logical_clock: m.logical_clock,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  Remember
// ═══════════════════════════════════════════════════════════════════════

pub fn remember_options_to_proto(
    opts: &RememberOptions,
    tenant_id: Option<&str>,
) -> pb::RememberRequest {
    pb::RememberRequest {
        content: opts.content.clone(),
        importance: opts.importance,
        context: opts.context.as_ref().map(json_map_to_struct),
        entity_id: opts.entity_id.clone(),
        edges: opts.edges.iter().map(edge_to_proto).collect(),
        tenant_id: tenant_id.map(str::to_string),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall
// ═══════════════════════════════════════════════════════════════════════

pub fn recall_options_to_proto(opts: &RecallOptions, tenant_id: Option<&str>) -> pb::RecallRequest {
    let strategies = opts
        .strategies
        .iter()
        .map(strategy_config_to_proto)
        .collect();

    pb::RecallRequest {
        cue: opts.cue.clone(),
        strategies,
        top_k: opts.top_k,
        scoring_weights: opts.scoring_weights.as_ref().map(scoring_weights_to_proto),
        cue_context: opts.cue_context.as_ref().map(json_map_to_struct),
        tenant_id: tenant_id.map(str::to_string),
    }
}

fn strategy_config_to_proto(sc: &StrategyConfig) -> pb::RecallStrategyConfig {
    pb::RecallStrategyConfig {
        strategy_type: strategy_to_proto_i32(sc.strategy),
        top_k: sc.top_k,
        ef_search: sc.ef_search,
        entity_id: sc.entity_id.clone(),
        time_range: sc.time_range.map(|(s, e)| pb::TimeRange {
            start_us: s,
            end_us: e,
        }),
        seed_memory_id: sc.seed_memory_id.map(|u| u.to_bytes().to_vec()),
        edge_types: sc
            .edge_types
            .iter()
            .map(|et| edge_type_to_proto_i32(*et))
            .collect(),
        max_depth: sc.max_depth,
        analogical_alpha: sc.analogical_alpha,
    }
}

pub fn proto_recall_result_to_domain(r: &pb::RecallResult) -> Result<RecallResult, String> {
    let memory = match &r.memory {
        Some(m) => proto_memory_to_domain(m)?,
        None => return Err("recall result missing memory".to_string()),
    };

    let strategy_details = r
        .strategy_details
        .iter()
        .filter_map(|d| proto_strategy_detail_to_domain(d).ok())
        .collect();

    Ok(RecallResult {
        memory,
        score: r.score,
        strategy_details,
    })
}

fn proto_strategy_detail_to_domain(
    d: &pb::StrategyDetailMessage,
) -> Result<StrategyDetail, String> {
    let st = pb::RecallStrategyType::try_from(d.strategy_type)
        .map_err(|_| format!("invalid strategy type: {}", d.strategy_type))?;

    match st {
        pb::RecallStrategyType::Similarity => Ok(StrategyDetail::Similarity {
            distance: d.distance.unwrap_or(0.0),
            relevance: d.relevance,
        }),
        pb::RecallStrategyType::Temporal => Ok(StrategyDetail::Temporal {
            timestamp: d.timestamp.unwrap_or(0),
            rank: d.rank.unwrap_or(0),
            relevance: d.relevance,
        }),
        pb::RecallStrategyType::Causal => {
            let seed_id = match &d.seed_id {
                Some(b) => bytes_to_ulid(b)?,
                None => Ulid::nil(),
            };
            let edge_type = d
                .causal_edge_type
                .map(proto_to_edge_type)
                .unwrap_or(EdgeType::CausedBy);
            Ok(StrategyDetail::Causal {
                depth: d.depth.unwrap_or(0),
                edge_type,
                seed_id,
                relevance: d.relevance,
            })
        }
        pb::RecallStrategyType::Analogical => Ok(StrategyDetail::Analogical {
            embedding_similarity: d.embedding_similarity.unwrap_or(0.0),
            structural_similarity: d.structural_similarity.unwrap_or(0.0),
            relevance: d.relevance,
            used_vector_analogy: false,
        }),
        _ => Err(format!("unspecified strategy type: {}", d.strategy_type)),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Prime
// ═══════════════════════════════════════════════════════════════════════

pub fn prime_options_to_proto(opts: &PrimeOptions, tenant_id: Option<&str>) -> pb::PrimeRequest {
    pb::PrimeRequest {
        entity_id: opts.entity_id.clone(),
        context: opts.context.as_ref().map(json_map_to_struct),
        max_memories: opts.max_memories,
        recency_window_us: opts.recency_window_us,
        similarity_cue: opts.similarity_cue.clone(),
        scoring_weights: opts.scoring_weights.as_ref().map(scoring_weights_to_proto),
        tenant_id: tenant_id.map(str::to_string),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Revise
// ═══════════════════════════════════════════════════════════════════════

pub fn revise_options_to_proto(opts: &ReviseOptions, tenant_id: Option<&str>) -> pb::ReviseRequest {
    let context_mode = match opts.context_mode {
        ContextMode::Merge => pb::ContextMode::Merge as i32,
        ContextMode::Replace => pb::ContextMode::Replace as i32,
    };

    pb::ReviseRequest {
        memory_id: opts.memory_id.to_bytes().to_vec(),
        content: opts.content.clone(),
        importance: opts.importance,
        context: opts.context.as_ref().map(json_map_to_struct),
        context_mode,
        entity_id: opts.entity_id.clone(),
        edges: opts.edges.iter().map(edge_to_proto).collect(),
        tenant_id: tenant_id.map(str::to_string),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Forget
// ═══════════════════════════════════════════════════════════════════════

pub fn forget_criteria_to_proto(c: &ForgetCriteria, tenant_id: Option<&str>) -> pb::ForgetRequest {
    pb::ForgetRequest {
        memory_ids: c.memory_ids.iter().map(|u| u.to_bytes().to_vec()).collect(),
        entity_id: c.entity_id.clone(),
        staleness_threshold_us: c.staleness_threshold_us,
        access_count_floor: c.access_count_floor,
        memory_kind: c.memory_kind.map(memory_kind_to_proto_i32),
        decay_score_floor: c.decay_score_floor,
        tenant_id: tenant_id.map(str::to_string),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Subscribe
// ═══════════════════════════════════════════════════════════════════════

pub fn subscribe_options_to_proto(
    opts: &SubscribeOptions,
    tenant_id: Option<&str>,
) -> pb::SubscribeRequest {
    pb::SubscribeRequest {
        entity_id: opts.entity_id.clone(),
        kind_filter: opts
            .kind_filter
            .iter()
            .map(|k| memory_kind_to_proto_i32(*k))
            .collect(),
        confidence_threshold: opts.confidence_threshold,
        time_scope_us: opts.time_scope_us,
        output_buffer_size: opts.output_buffer_size,
        coarse_threshold: opts.coarse_threshold,
        tenant_id: tenant_id.map(str::to_string),
    }
}

pub fn proto_push_to_domain(p: &pb::SubscribePushMessage) -> Result<SubscribePush, String> {
    let memory = match &p.memory {
        Some(m) => proto_memory_to_domain(m)?,
        None => return Err("push missing memory".to_string()),
    };
    Ok(SubscribePush {
        subscription_id: p.subscription_id,
        memory,
        confidence: p.confidence,
        push_timestamp_us: p.push_timestamp_us,
        sequence_number: p.sequence_number,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  Reflect / Insights
// ═══════════════════════════════════════════════════════════════════════

pub fn reflect_scope_to_proto(
    scope: &crate::types::ReflectScope,
    tenant_id: Option<&str>,
) -> pb::ReflectRequest {
    let scope_msg = match scope {
        crate::types::ReflectScope::Entity {
            entity_id,
            since_us,
        } => pb::ReflectScope {
            scope: Some(pb::reflect_scope::Scope::Entity(pb::EntityScope {
                entity_id: entity_id.clone(),
                since_us: *since_us,
            })),
        },
        crate::types::ReflectScope::Global { since_us } => pb::ReflectScope {
            scope: Some(pb::reflect_scope::Scope::Global(pb::GlobalScope {
                since_us: *since_us,
            })),
        },
    };

    pb::ReflectRequest {
        scope: Some(scope_msg),
        tenant_id: tenant_id.map(str::to_string),
    }
}

pub fn insights_filter_to_proto(
    f: &InsightsFilter,
    tenant_id: Option<&str>,
) -> pb::GetInsightsRequest {
    pb::GetInsightsRequest {
        entity_id: f.entity_id.clone(),
        min_confidence: f.min_confidence,
        max_results: f.max_results,
        tenant_id: tenant_id.map(str::to_string),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Primitive helpers
// ═══════════════════════════════════════════════════════════════════════

pub fn bytes_to_ulid(bytes: &[u8]) -> Result<Ulid, String> {
    if bytes.len() != 16 {
        return Err(format!("memory_id must be 16 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(bytes);
    Ok(Ulid::from_bytes(arr))
}

fn edge_to_proto(e: &RememberEdge) -> pb::Edge {
    pb::Edge {
        target_id: e.target_id.to_bytes().to_vec(),
        edge_type: edge_type_to_proto_i32(e.edge_type),
        confidence: e.confidence,
    }
}

fn edge_type_to_proto_i32(et: EdgeType) -> i32 {
    match et {
        EdgeType::CausedBy => pb::EdgeType::CausedBy as i32,
        EdgeType::RelatedTo => pb::EdgeType::RelatedTo as i32,
        EdgeType::FollowedBy => pb::EdgeType::FollowedBy as i32,
        EdgeType::RevisedFrom => pb::EdgeType::RevisedFrom as i32,
        EdgeType::InsightFrom => pb::EdgeType::InsightFrom as i32,
        EdgeType::Contradicts => pb::EdgeType::Contradicts as i32,
    }
}

fn proto_to_edge_type(v: i32) -> EdgeType {
    match pb::EdgeType::try_from(v) {
        Ok(pb::EdgeType::CausedBy) => EdgeType::CausedBy,
        Ok(pb::EdgeType::RelatedTo) => EdgeType::RelatedTo,
        Ok(pb::EdgeType::FollowedBy) => EdgeType::FollowedBy,
        Ok(pb::EdgeType::RevisedFrom) => EdgeType::RevisedFrom,
        Ok(pb::EdgeType::InsightFrom) => EdgeType::InsightFrom,
        Ok(pb::EdgeType::Contradicts) => EdgeType::Contradicts,
        _ => EdgeType::CausedBy,
    }
}

fn proto_to_memory_kind(v: i32) -> MemoryKind {
    match pb::MemoryKind::try_from(v) {
        Ok(pb::MemoryKind::Insight) => MemoryKind::Insight,
        Ok(pb::MemoryKind::Revision) => MemoryKind::Revision,
        _ => MemoryKind::Episode,
    }
}

fn memory_kind_to_proto_i32(k: MemoryKind) -> i32 {
    match k {
        MemoryKind::Episode => pb::MemoryKind::Episode as i32,
        MemoryKind::Insight => pb::MemoryKind::Insight as i32,
        MemoryKind::Revision => pb::MemoryKind::Revision as i32,
    }
}

fn strategy_to_proto_i32(s: RecallStrategy) -> i32 {
    match s {
        RecallStrategy::Similarity => pb::RecallStrategyType::Similarity as i32,
        RecallStrategy::Temporal => pb::RecallStrategyType::Temporal as i32,
        RecallStrategy::Causal => pb::RecallStrategyType::Causal as i32,
        RecallStrategy::Analogical => pb::RecallStrategyType::Analogical as i32,
    }
}

fn scoring_weights_to_proto(w: &ScoringWeights) -> pb::ScoringWeights {
    pb::ScoringWeights {
        w_relevance: w.w_relevance,
        w_recency: w.w_recency,
        w_importance: w.w_importance,
        w_reinforcement: w.w_reinforcement,
        max_age_us: w.max_age_us,
        reinforcement_cap: w.reinforcement_cap,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Struct <-> JSON map
// ═══════════════════════════════════════════════════════════════════════

fn struct_to_json_map(s: &prost_types::Struct) -> HashMap<String, serde_json::Value> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_roundtrip() {
        let id = Ulid::new();
        let bytes = id.to_bytes().to_vec();
        let back = bytes_to_ulid(&bytes).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn ulid_invalid_length() {
        assert!(bytes_to_ulid(&[0u8; 8]).is_err());
        assert!(bytes_to_ulid(&[]).is_err());
    }

    #[test]
    fn json_struct_roundtrip() {
        let mut map = HashMap::new();
        map.insert("key".to_string(), serde_json::json!("value"));
        map.insert("num".to_string(), serde_json::json!(42.0));
        map.insert("flag".to_string(), serde_json::json!(true));
        map.insert("null".to_string(), serde_json::Value::Null);
        map.insert("arr".to_string(), serde_json::json!([1.0, "two", false]));

        let s = json_map_to_struct(&map);
        let back = struct_to_json_map(&s);

        assert_eq!(back["key"], serde_json::json!("value"));
        assert_eq!(back["num"], serde_json::json!(42.0));
        assert_eq!(back["flag"], serde_json::json!(true));
        assert_eq!(back["null"], serde_json::Value::Null);
    }

    #[test]
    fn edge_type_roundtrip() {
        for et in [
            EdgeType::CausedBy,
            EdgeType::RelatedTo,
            EdgeType::FollowedBy,
            EdgeType::RevisedFrom,
            EdgeType::InsightFrom,
            EdgeType::Contradicts,
        ] {
            let proto = edge_type_to_proto_i32(et);
            let back = proto_to_edge_type(proto);
            assert_eq!(et, back);
        }
    }

    #[test]
    fn memory_kind_roundtrip() {
        for k in [
            MemoryKind::Episode,
            MemoryKind::Insight,
            MemoryKind::Revision,
        ] {
            let proto = memory_kind_to_proto_i32(k);
            let back = proto_to_memory_kind(proto);
            assert_eq!(k, back);
        }
    }

    #[test]
    fn strategy_roundtrip() {
        for s in [
            RecallStrategy::Similarity,
            RecallStrategy::Temporal,
            RecallStrategy::Causal,
            RecallStrategy::Analogical,
        ] {
            let _ = strategy_to_proto_i32(s);
        }
    }

    #[test]
    fn remember_options_conversion() {
        let opts = RememberOptions::new("test content").importance(0.9);
        let proto = remember_options_to_proto(&opts, None);
        assert_eq!(proto.content, "test content");
        assert_eq!(proto.importance, Some(0.9));
    }

    #[test]
    fn forget_criteria_conversion() {
        let id = Ulid::new();
        let c = ForgetCriteria::by_id(id);
        let proto = forget_criteria_to_proto(&c, None);
        assert_eq!(proto.memory_ids.len(), 1);
        assert_eq!(proto.memory_ids[0], id.to_bytes().to_vec());
    }

    #[test]
    fn subscribe_options_conversion() {
        let opts = SubscribeOptions::new()
            .entity_id("test")
            .confidence_threshold(0.7);
        let proto = subscribe_options_to_proto(&opts, None);
        assert_eq!(proto.entity_id, Some("test".to_string()));
        assert!((proto.confidence_threshold - 0.7).abs() < f32::EPSILON);
    }
}
