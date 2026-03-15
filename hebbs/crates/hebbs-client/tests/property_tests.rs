use proptest::prelude::*;
use std::collections::HashMap;
use ulid::Ulid;

use hebbs_client::*;

// ═══════════════════════════════════════════════════════════════════════
//  Property tests for type constructors and builders
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #[test]
    fn remember_options_preserves_content(content in "\\PC{1,200}") {
        let opts = RememberOptions::new(content.clone());
        prop_assert_eq!(opts.content, content);
        prop_assert_eq!(opts.importance, None);
        prop_assert_eq!(opts.entity_id, None);
        prop_assert!(opts.edges.is_empty());
    }

    #[test]
    fn remember_options_importance_bounded(importance in 0.0f32..=1.0f32) {
        let opts = RememberOptions::new("test").importance(importance);
        prop_assert_eq!(opts.importance, Some(importance));
    }

    #[test]
    fn recall_options_top_k(k in 1u32..1000u32) {
        let opts = RecallOptions::new("cue").top_k(k);
        prop_assert_eq!(opts.top_k, Some(k));
    }

    #[test]
    fn forget_criteria_by_id_roundtrip(raw_bytes in prop::array::uniform16(any::<u8>())) {
        let id = Ulid::from_bytes(raw_bytes);
        let criteria = ForgetCriteria::by_id(id);
        prop_assert_eq!(criteria.memory_ids.len(), 1);
        prop_assert_eq!(criteria.memory_ids[0], id);
    }

    #[test]
    fn forget_criteria_multi_ids(count in 1usize..50usize) {
        let ids: Vec<Ulid> = (0..count).map(|_| Ulid::new()).collect();
        let criteria = ForgetCriteria::by_ids(ids.clone());
        prop_assert_eq!(criteria.memory_ids.len(), count);
    }

    #[test]
    fn revise_options_preserves_id(raw_bytes in prop::array::uniform16(any::<u8>())) {
        let id = Ulid::from_bytes(raw_bytes);
        let opts = ReviseOptions::new(id);
        prop_assert_eq!(opts.memory_id, id);
        prop_assert_eq!(opts.content, None);
        prop_assert_eq!(opts.importance, None);
    }

    #[test]
    fn revise_options_content_update(
        raw_bytes in prop::array::uniform16(any::<u8>()),
        content in "\\PC{1,200}"
    ) {
        let id = Ulid::from_bytes(raw_bytes);
        let opts = ReviseOptions::new(id).content(content.clone());
        prop_assert_eq!(opts.content, Some(content));
    }

    #[test]
    fn subscribe_options_confidence_range(threshold in 0.0f32..=1.0f32) {
        let opts = SubscribeOptions::new().confidence_threshold(threshold);
        prop_assert!((opts.confidence_threshold - threshold).abs() < f32::EPSILON);
    }

    #[test]
    fn prime_options_max_memories(n in 1u32..200u32) {
        let opts = PrimeOptions::new("entity").max_memories(n);
        prop_assert_eq!(opts.max_memories, Some(n));
    }

    #[test]
    fn insights_filter_max_results(n in 1u32..1000u32) {
        let filter = InsightsFilter::new().max_results(n);
        prop_assert_eq!(filter.max_results, Some(n));
    }

    #[test]
    fn scoring_weights_non_negative(
        w_r in 0.0f32..=1.0f32,
        w_rec in 0.0f32..=1.0f32,
        w_i in 0.0f32..=1.0f32,
        w_reinf in 0.0f32..=1.0f32,
    ) {
        let w = ScoringWeights {
            w_relevance: w_r,
            w_recency: w_rec,
            w_importance: w_i,
            w_reinforcement: w_reinf,
            ..ScoringWeights::default()
        };
        prop_assert!(w.w_relevance >= 0.0);
        prop_assert!(w.w_recency >= 0.0);
        prop_assert!(w.w_importance >= 0.0);
        prop_assert!(w.w_reinforcement >= 0.0);
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Property tests for retry policy
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #[test]
    fn retry_delay_always_positive(attempt in 0u32..10u32) {
        let policy = hebbs_client::RetryPolicy::default();
        let delay = policy.delay_for_attempt(attempt);
        prop_assert!(delay.as_nanos() > 0);
    }

    #[test]
    fn retry_delay_bounded_by_max(attempt in 0u32..20u32) {
        use std::time::Duration;
        let policy = hebbs_client::RetryPolicy {
            max_backoff: Duration::from_secs(1),
            jitter: 0.0,
            ..Default::default()
        };
        let delay = policy.delay_for_attempt(attempt);
        prop_assert!(delay <= Duration::from_millis(1100));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Property tests for error taxonomy
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #[test]
    fn client_error_display_not_empty(msg in "\\PC{1,100}") {
        let err = ClientError::NotFound { message: msg.clone() };
        let display = err.to_string();
        prop_assert!(!display.is_empty());
        prop_assert!(display.contains(&msg));
    }

    #[test]
    fn connection_error_contains_endpoint(
        endpoint in "[a-z]{1,20}:[0-9]{1,5}",
        reason in "\\PC{1,50}"
    ) {
        let err = ClientError::ConnectionFailed {
            endpoint: endpoint.clone(),
            reason: reason.clone(),
        };
        let display = err.to_string();
        prop_assert!(display.contains(&endpoint));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Property tests for edge types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_edge_types_display() {
    let types = vec![
        EdgeType::CausedBy,
        EdgeType::RelatedTo,
        EdgeType::FollowedBy,
        EdgeType::RevisedFrom,
        EdgeType::InsightFrom,
        EdgeType::Contradicts,
    ];
    for t in types {
        let s = t.to_string();
        assert!(!s.is_empty());
    }
}

#[test]
fn all_memory_kinds_display() {
    let kinds = vec![
        MemoryKind::Episode,
        MemoryKind::Insight,
        MemoryKind::Revision,
    ];
    for k in kinds {
        let s = k.to_string();
        assert!(!s.is_empty());
    }
}

#[test]
fn all_recall_strategies_display() {
    let strategies = vec![
        RecallStrategy::Similarity,
        RecallStrategy::Temporal,
        RecallStrategy::Causal,
        RecallStrategy::Analogical,
    ];
    for s in strategies {
        let display = s.to_string();
        assert!(!display.is_empty());
    }
}

#[test]
fn serving_status_display() {
    assert_eq!(ServingStatus::Serving.to_string(), "SERVING");
    assert_eq!(ServingStatus::NotServing.to_string(), "NOT_SERVING");
    assert_eq!(ServingStatus::Unknown.to_string(), "UNKNOWN");
}

#[test]
fn strategy_config_temporal_has_entity() {
    let sc = StrategyConfig::temporal("my_entity");
    assert_eq!(sc.strategy, RecallStrategy::Temporal);
    assert_eq!(sc.entity_id.unwrap(), "my_entity");
}

#[test]
fn strategy_config_causal_has_seed() {
    let id = Ulid::new();
    let sc = StrategyConfig::causal(id);
    assert_eq!(sc.strategy, RecallStrategy::Causal);
    assert_eq!(sc.seed_memory_id.unwrap(), id);
}

#[test]
fn recall_options_multi_strategy() {
    let opts = RecallOptions::new("query").strategies(vec![
        StrategyConfig::similarity(),
        StrategyConfig::temporal("ent"),
    ]);
    assert_eq!(opts.strategies.len(), 2);
}

#[test]
fn context_mode_default_is_merge() {
    let opts = ReviseOptions::new(Ulid::new());
    assert_eq!(opts.context_mode, ContextMode::Merge);
}

#[test]
fn revise_context_replace() {
    let mut ctx = HashMap::new();
    ctx.insert("key".to_string(), serde_json::json!("value"));
    let opts = ReviseOptions::new(Ulid::new()).context_replace(ctx);
    assert_eq!(opts.context_mode, ContextMode::Replace);
    assert!(opts.context.is_some());
}

#[test]
fn revise_context_merge() {
    let mut ctx = HashMap::new();
    ctx.insert("key".to_string(), serde_json::json!("value"));
    let opts = ReviseOptions::new(Ulid::new()).context_merge(ctx);
    assert_eq!(opts.context_mode, ContextMode::Merge);
    assert!(opts.context.is_some());
}
