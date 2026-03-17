use std::collections::HashMap;

use hebbs_core::engine::{RememberEdge, RememberInput};
use hebbs_core::forget::ForgetCriteria;
use hebbs_core::memory::{Memory, MemoryKind};
use hebbs_core::recall::{
    PrimeInput, PrimeOutput, RecallInput, RecallOutput, RecallResult, RecallStrategy,
    StrategyDetail,
};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig, ReflectScope};
use hebbs_core::revise::{ContextMode, ReviseInput};
use hebbs_index::EdgeType;
use ulid::Ulid;

// ═══════════════════════════════════════════════════════════════════════
//  Memory -> JSON
// ═══════════════════════════════════════════════════════════════════════

pub fn memory_to_json(m: &Memory) -> String {
    let ulid = if m.memory_id.len() == 16 {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&m.memory_id);
        Ulid::from_bytes(bytes).to_string()
    } else {
        hex::encode(&m.memory_id)
    };

    let context = m.context().unwrap_or_default();
    let kind_str = match m.kind {
        MemoryKind::Episode => "episode",
        MemoryKind::Insight => "insight",
        MemoryKind::Revision => "revision",
        MemoryKind::Document => "document",
        MemoryKind::Proposition => "proposition",
    };

    let mut obj = serde_json::json!({
        "memory_id": ulid,
        "content": m.content,
        "importance": m.importance,
        "context": context,
        "created_at": m.created_at,
        "updated_at": m.updated_at,
        "last_accessed_at": m.last_accessed_at,
        "access_count": m.access_count,
        "decay_score": m.decay_score,
        "kind": kind_str,
        "logical_clock": m.logical_clock,
    });

    if let Some(ref eid) = m.entity_id {
        obj["entity_id"] = serde_json::json!(eid);
    }
    if let Some(ref did) = m.device_id {
        obj["device_id"] = serde_json::json!(did);
    }
    if m.embedding.is_some() {
        obj["has_embedding"] = serde_json::json!(true);
    }

    obj.to_string()
}

pub fn memories_to_json(memories: &[Memory]) -> String {
    let arr: Vec<serde_json::Value> = memories
        .iter()
        .map(|m| serde_json::from_str(&memory_to_json(m)).unwrap_or_default())
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string())
}

// ═══════════════════════════════════════════════════════════════════════
//  JSON -> RememberInput
// ═══════════════════════════════════════════════════════════════════════

pub fn json_to_remember_input(json: &str) -> Result<RememberInput, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {}", e))?;

    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .ok_or("content is required")?
        .to_string();

    let importance = v
        .get("importance")
        .and_then(|i| i.as_f64())
        .map(|f| f as f32);

    let context = v
        .get("context")
        .and_then(|c| serde_json::from_value::<HashMap<String, serde_json::Value>>(c.clone()).ok());

    let entity_id = v
        .get("entity_id")
        .and_then(|e| e.as_str())
        .map(|s| s.to_string());

    let edges = parse_edges(&v)?;

    Ok(RememberInput {
        content,
        importance,
        context,
        entity_id,
        edges,
        kind: None,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  JSON -> RecallInput
// ═══════════════════════════════════════════════════════════════════════

pub fn json_to_recall_input(cue: &str, opts_json: &str) -> Result<RecallInput, String> {
    let opts: serde_json::Value = if opts_json.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(opts_json).map_err(|e| format!("invalid options JSON: {}", e))?
    };

    let strategy_str = opts
        .get("strategy")
        .and_then(|s| s.as_str())
        .unwrap_or("similarity");

    let strategy = parse_strategy(strategy_str)?;

    let top_k = opts
        .get("top_k")
        .and_then(|k| k.as_u64())
        .map(|k| k as usize);
    let entity_id = opts
        .get("entity_id")
        .and_then(|e| e.as_str())
        .map(|s| s.to_string());
    let max_depth = opts
        .get("max_depth")
        .and_then(|d| d.as_u64())
        .map(|d| d as usize);
    let ef_search = opts
        .get("ef_search")
        .and_then(|e| e.as_u64())
        .map(|e| e as usize);

    let time_range = opts.get("time_range").and_then(|tr| {
        let start = tr.get("start_us")?.as_u64()?;
        let end = tr.get("end_us")?.as_u64()?;
        Some((start, end))
    });

    let cue_context = opts
        .get("cue_context")
        .and_then(|c| serde_json::from_value::<HashMap<String, serde_json::Value>>(c.clone()).ok());

    Ok(RecallInput {
        cue: cue.to_string(),
        strategies: vec![strategy],
        top_k,
        entity_id,
        time_range,
        edge_types: None,
        max_depth,
        ef_search,
        scoring_weights: None,
        cue_context,
        causal_direction: None,
        analogy_a_id: None,
        analogy_b_id: None,
        seed_memory_id: None,
        analogical_alpha: None,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  RecallOutput -> JSON
// ═══════════════════════════════════════════════════════════════════════

pub fn recall_output_to_json(output: &RecallOutput) -> String {
    let results: Vec<serde_json::Value> =
        output.results.iter().map(recall_result_to_json).collect();

    let errors: Vec<serde_json::Value> = output
        .strategy_errors
        .iter()
        .map(|e| {
            serde_json::json!({
                "strategy": format!("{:?}", e.strategy),
                "message": e.message,
            })
        })
        .collect();

    serde_json::json!({
        "results": results,
        "strategy_errors": errors,
    })
    .to_string()
}

fn recall_result_to_json(r: &RecallResult) -> serde_json::Value {
    let memory: serde_json::Value =
        serde_json::from_str(&memory_to_json(&r.memory)).unwrap_or_default();

    let details: Vec<serde_json::Value> = r
        .strategy_details
        .iter()
        .map(strategy_detail_to_json)
        .collect();

    serde_json::json!({
        "memory": memory,
        "score": r.score,
        "strategy_details": details,
    })
}

fn strategy_detail_to_json(d: &StrategyDetail) -> serde_json::Value {
    match d {
        StrategyDetail::Similarity {
            distance,
            relevance,
        } => serde_json::json!({
            "strategy": "similarity",
            "distance": distance,
            "relevance": relevance,
        }),
        StrategyDetail::Temporal {
            timestamp,
            rank,
            relevance,
        } => serde_json::json!({
            "strategy": "temporal",
            "timestamp": timestamp,
            "rank": rank,
            "relevance": relevance,
        }),
        StrategyDetail::Causal {
            depth,
            edge_type,
            seed_id,
            relevance,
        } => {
            let seed_ulid = Ulid::from_bytes(*seed_id).to_string();
            serde_json::json!({
                "strategy": "causal",
                "depth": depth,
                "edge_type": format!("{:?}", edge_type),
                "seed_id": seed_ulid,
                "relevance": relevance,
            })
        }
        StrategyDetail::Analogical {
            embedding_similarity,
            structural_similarity,
            relevance,
            used_vector_analogy,
        } => serde_json::json!({
            "strategy": "analogical",
            "embedding_similarity": embedding_similarity,
            "structural_similarity": structural_similarity,
            "relevance": relevance,
            "used_vector_analogy": used_vector_analogy,
        }),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  JSON -> ReviseInput
// ═══════════════════════════════════════════════════════════════════════

pub fn json_to_revise_input(json: &str) -> Result<ReviseInput, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {}", e))?;

    let memory_id_str = v
        .get("memory_id")
        .and_then(|m| m.as_str())
        .ok_or("memory_id is required")?;

    let ulid = Ulid::from_string(memory_id_str).map_err(|e| format!("invalid ULID: {}", e))?;

    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());

    let importance = v
        .get("importance")
        .and_then(|i| i.as_f64())
        .map(|f| f as f32);

    let context = v
        .get("context")
        .and_then(|c| serde_json::from_value::<HashMap<String, serde_json::Value>>(c.clone()).ok());

    let context_mode = match v.get("context_mode").and_then(|m| m.as_str()) {
        Some("replace") => ContextMode::Replace,
        _ => ContextMode::Merge,
    };

    let entity_id = v
        .get("entity_id")
        .and_then(|e| e.as_str())
        .map(|s| Some(s.to_string()));

    let edges = parse_edges(&v)?;

    Ok(ReviseInput {
        memory_id: ulid.to_bytes().to_vec(),
        content,
        importance,
        context,
        context_mode,
        entity_id,
        edges,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  JSON -> ForgetCriteria
// ═══════════════════════════════════════════════════════════════════════

pub fn json_to_forget_criteria(json: &str) -> Result<ForgetCriteria, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {}", e))?;

    let memory_ids = v
        .get("memory_ids")
        .and_then(|ids| ids.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|id| {
                    id.as_str()
                        .and_then(|s| Ulid::from_string(s).ok())
                        .map(|u| u.to_bytes().to_vec())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let entity_id = v
        .get("entity_id")
        .and_then(|e| e.as_str())
        .map(|s| s.to_string());

    let staleness_threshold_us = v.get("staleness_threshold_us").and_then(|s| s.as_u64());

    let access_count_floor = v.get("access_count_floor").and_then(|a| a.as_u64());

    let memory_kind = v
        .get("memory_kind")
        .and_then(|k| k.as_str())
        .and_then(parse_memory_kind);

    let decay_score_floor = v
        .get("decay_score_floor")
        .and_then(|d| d.as_f64())
        .map(|f| f as f32);

    Ok(ForgetCriteria {
        memory_ids,
        entity_id,
        staleness_threshold_us,
        access_count_floor,
        memory_kind,
        decay_score_floor,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  JSON -> PrimeInput
// ═══════════════════════════════════════════════════════════════════════

pub fn json_to_prime_input(json: &str) -> Result<PrimeInput, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {}", e))?;

    let entity_id = v
        .get("entity_id")
        .and_then(|e| e.as_str())
        .ok_or("entity_id is required")?
        .to_string();

    let context = v
        .get("context")
        .and_then(|c| serde_json::from_value::<HashMap<String, serde_json::Value>>(c.clone()).ok());

    let max_memories = v
        .get("max_memories")
        .and_then(|m| m.as_u64())
        .map(|n| n as usize);

    let recency_window_us = v.get("recency_window_us").and_then(|r| r.as_u64());

    let similarity_cue = v
        .get("similarity_cue")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());

    Ok(PrimeInput {
        entity_id,
        context,
        max_memories,
        recency_window_us,
        similarity_cue,
        scoring_weights: None,
    })
}

pub fn prime_output_to_json(output: &PrimeOutput) -> String {
    let results: Vec<serde_json::Value> =
        output.results.iter().map(recall_result_to_json).collect();

    serde_json::json!({
        "results": results,
        "temporal_count": output.temporal_count,
        "similarity_count": output.similarity_count,
    })
    .to_string()
}

// ═══════════════════════════════════════════════════════════════════════
//  JSON -> Reflect params
// ═══════════════════════════════════════════════════════════════════════

pub fn json_to_reflect_params(json: &str) -> Result<(ReflectConfig, ReflectScope), String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {}", e))?;

    let scope = if let Some(entity_id) = v.get("entity_id").and_then(|e| e.as_str()) {
        let since_us = v.get("since_us").and_then(|s| s.as_u64());
        ReflectScope::Entity {
            entity_id: entity_id.to_string(),
            since_us,
        }
    } else {
        let since_us = v.get("since_us").and_then(|s| s.as_u64());
        ReflectScope::Global { since_us }
    };

    let config = ReflectConfig::default();
    Ok((config, scope))
}

// ═══════════════════════════════════════════════════════════════════════
//  JSON -> InsightsFilter
// ═══════════════════════════════════════════════════════════════════════

pub fn json_to_insights_filter(json: &str) -> Result<InsightsFilter, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {}", e))?;

    Ok(InsightsFilter {
        entity_id: v
            .get("entity_id")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string()),
        min_confidence: v
            .get("min_confidence")
            .and_then(|c| c.as_f64())
            .map(|f| f as f32),
        max_results: v
            .get("max_results")
            .and_then(|m| m.as_u64())
            .map(|n| n as usize),
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════

fn parse_strategy(s: &str) -> Result<RecallStrategy, String> {
    match s.to_lowercase().as_str() {
        "similarity" => Ok(RecallStrategy::Similarity),
        "temporal" => Ok(RecallStrategy::Temporal),
        "causal" => Ok(RecallStrategy::Causal),
        "analogical" => Ok(RecallStrategy::Analogical),
        _ => Err(format!("unknown strategy: {}", s)),
    }
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

fn parse_edge_type(s: &str) -> Result<EdgeType, String> {
    match s.to_lowercase().as_str() {
        "caused_by" => Ok(EdgeType::CausedBy),
        "related_to" => Ok(EdgeType::RelatedTo),
        "followed_by" => Ok(EdgeType::FollowedBy),
        "revised_from" => Ok(EdgeType::RevisedFrom),
        "insight_from" => Ok(EdgeType::InsightFrom),
        "contradicts" => Ok(EdgeType::Contradicts),
        "has_entity" => Ok(EdgeType::HasEntity),
        "entity_relation" => Ok(EdgeType::EntityRelation),
        "proposition_of" => Ok(EdgeType::PropositionOf),
        _ => Err(format!("unknown edge type: {}", s)),
    }
}

fn parse_edges(v: &serde_json::Value) -> Result<Vec<RememberEdge>, String> {
    let edges = match v.get("edges").and_then(|e| e.as_array()) {
        Some(arr) => arr
            .iter()
            .map(|edge| {
                let target_str = edge
                    .get("target_id")
                    .and_then(|t| t.as_str())
                    .ok_or("edge.target_id is required")?;
                let target = Ulid::from_string(target_str)
                    .map_err(|e| format!("invalid target ULID: {}", e))?;
                let edge_type_str = edge
                    .get("edge_type")
                    .and_then(|t| t.as_str())
                    .ok_or("edge.edge_type is required")?;
                let edge_type = parse_edge_type(edge_type_str)?;
                let confidence = edge
                    .get("confidence")
                    .and_then(|c| c.as_f64())
                    .map(|f| f as f32);
                Ok(RememberEdge {
                    target_id: target.to_bytes(),
                    edge_type,
                    confidence,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        None => Vec::new(),
    };
    Ok(edges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_input_from_json() {
        let json = r#"{"content": "test memory", "importance": 0.8}"#;
        let input = json_to_remember_input(json).unwrap();
        assert_eq!(input.content, "test memory");
        assert_eq!(input.importance, Some(0.8));
    }

    #[test]
    fn remember_input_missing_content() {
        let json = r#"{"importance": 0.5}"#;
        assert!(json_to_remember_input(json).is_err());
    }

    #[test]
    fn recall_input_default_strategy() {
        let input = json_to_recall_input("test cue", "").unwrap();
        assert_eq!(input.cue, "test cue");
        assert_eq!(input.strategies, vec![RecallStrategy::Similarity]);
    }

    #[test]
    fn recall_input_temporal_strategy() {
        let opts = r#"{"strategy": "temporal", "entity_id": "acme"}"#;
        let input = json_to_recall_input("cue", opts).unwrap();
        assert_eq!(input.strategies, vec![RecallStrategy::Temporal]);
        assert_eq!(input.entity_id, Some("acme".to_string()));
    }

    #[test]
    fn forget_criteria_by_entity() {
        let json = r#"{"entity_id": "customer_123"}"#;
        let criteria = json_to_forget_criteria(json).unwrap();
        assert_eq!(criteria.entity_id, Some("customer_123".to_string()));
    }

    #[test]
    fn revise_input_with_content() {
        let id = Ulid::new();
        let json = format!(r#"{{"memory_id": "{}", "content": "updated"}}"#, id);
        let input = json_to_revise_input(&json).unwrap();
        assert_eq!(input.content, Some("updated".to_string()));
    }

    #[test]
    fn prime_input_from_json() {
        let json = r#"{"entity_id": "acme", "max_memories": 50}"#;
        let input = json_to_prime_input(json).unwrap();
        assert_eq!(input.entity_id, "acme");
        assert_eq!(input.max_memories, Some(50));
    }

    #[test]
    fn reflect_params_entity_scope() {
        let json = r#"{"entity_id": "acme"}"#;
        let (_, scope) = json_to_reflect_params(json).unwrap();
        match scope {
            ReflectScope::Entity { entity_id, .. } => assert_eq!(entity_id, "acme"),
            _ => panic!("expected entity scope"),
        }
    }

    #[test]
    fn reflect_params_global_scope() {
        let json = r#"{}"#;
        let (_, scope) = json_to_reflect_params(json).unwrap();
        assert!(matches!(scope, ReflectScope::Global { .. }));
    }

    #[test]
    fn insights_filter_from_json() {
        let json = r#"{"entity_id": "acme", "min_confidence": 0.5, "max_results": 10}"#;
        let filter = json_to_insights_filter(json).unwrap();
        assert_eq!(filter.entity_id, Some("acme".to_string()));
        assert_eq!(filter.min_confidence, Some(0.5));
        assert_eq!(filter.max_results, Some(10));
    }

    #[test]
    fn strategy_parsing() {
        assert_eq!(
            parse_strategy("similarity").unwrap(),
            RecallStrategy::Similarity
        );
        assert_eq!(
            parse_strategy("TEMPORAL").unwrap(),
            RecallStrategy::Temporal
        );
        assert_eq!(parse_strategy("Causal").unwrap(), RecallStrategy::Causal);
        assert!(parse_strategy("unknown").is_err());
    }

    #[test]
    fn edge_type_parsing() {
        assert_eq!(parse_edge_type("caused_by").unwrap(), EdgeType::CausedBy);
        assert_eq!(parse_edge_type("related_to").unwrap(), EdgeType::RelatedTo);
        assert!(parse_edge_type("unknown").is_err());
    }

    #[test]
    fn memory_kind_parsing() {
        assert_eq!(parse_memory_kind("episode"), Some(MemoryKind::Episode));
        assert_eq!(parse_memory_kind("insight"), Some(MemoryKind::Insight));
        assert_eq!(parse_memory_kind("revision"), Some(MemoryKind::Revision));
        assert_eq!(parse_memory_kind("unknown"), None);
    }
}
