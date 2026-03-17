use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// The canonical HEBBS memory record as seen by client SDK consumers.
///
/// Mirrors the fields of `hebbs_core::memory::Memory` without depending
/// on the engine crate. Memory IDs are `Ulid` for ergonomics; context
/// is a deserialized `HashMap` rather than raw bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub memory_id: Ulid,
    pub content: String,
    pub importance: f32,
    pub context: HashMap<String, serde_json::Value>,
    pub entity_id: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_accessed_at: u64,
    pub access_count: u64,
    pub decay_score: f32,
    pub kind: MemoryKind,
    pub device_id: Option<String>,
    pub logical_clock: u64,
}

impl PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        self.memory_id == other.memory_id
    }
}

impl Eq for Memory {}

impl std::hash::Hash for Memory {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.memory_id.hash(state);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryKind {
    Episode,
    Insight,
    Revision,
    Document,
    Proposition,
}

impl fmt::Display for MemoryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryKind::Episode => write!(f, "Episode"),
            MemoryKind::Insight => write!(f, "Insight"),
            MemoryKind::Revision => write!(f, "Revision"),
            MemoryKind::Document => write!(f, "Document"),
            MemoryKind::Proposition => write!(f, "Proposition"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    CausedBy,
    RelatedTo,
    FollowedBy,
    RevisedFrom,
    InsightFrom,
    Contradicts,
    HasEntity,
    EntityRelation,
    PropositionOf,
}

impl fmt::Display for EdgeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EdgeType::CausedBy => write!(f, "caused_by"),
            EdgeType::RelatedTo => write!(f, "related_to"),
            EdgeType::FollowedBy => write!(f, "followed_by"),
            EdgeType::RevisedFrom => write!(f, "revised_from"),
            EdgeType::InsightFrom => write!(f, "insight_from"),
            EdgeType::Contradicts => write!(f, "contradicts"),
            EdgeType::HasEntity => write!(f, "has_entity"),
            EdgeType::EntityRelation => write!(f, "entity_relation"),
            EdgeType::PropositionOf => write!(f, "proposition_of"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum RecallStrategy {
    #[default]
    Similarity,
    Temporal,
    Causal,
    Analogical,
}

impl fmt::Display for RecallStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecallStrategy::Similarity => write!(f, "similarity"),
            RecallStrategy::Temporal => write!(f, "temporal"),
            RecallStrategy::Causal => write!(f, "causal"),
            RecallStrategy::Analogical => write!(f, "analogical"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ContextMode {
    #[default]
    Merge,
    Replace,
}

/// An edge to attach to a memory during remember or revise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RememberEdge {
    pub target_id: Ulid,
    pub edge_type: EdgeType,
    pub confidence: Option<f32>,
}

/// Options for `remember()`. Constructed via builder methods on `HebbsClient`.
#[derive(Debug, Clone, Default)]
pub struct RememberOptions {
    pub content: String,
    pub importance: Option<f32>,
    pub context: Option<HashMap<String, serde_json::Value>>,
    pub entity_id: Option<String>,
    pub edges: Vec<RememberEdge>,
}

impl RememberOptions {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            ..Default::default()
        }
    }

    pub fn importance(mut self, v: f32) -> Self {
        self.importance = Some(v);
        self
    }

    pub fn context(mut self, v: HashMap<String, serde_json::Value>) -> Self {
        self.context = Some(v);
        self
    }

    pub fn entity_id(mut self, v: impl Into<String>) -> Self {
        self.entity_id = Some(v.into());
        self
    }

    pub fn edge(mut self, e: RememberEdge) -> Self {
        self.edges.push(e);
        self
    }
}

/// Composite scoring weights for recall result ranking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringWeights {
    pub w_relevance: f32,
    pub w_recency: f32,
    pub w_importance: f32,
    pub w_reinforcement: f32,
    pub max_age_us: u64,
    pub reinforcement_cap: u64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            w_relevance: 0.4,
            w_recency: 0.2,
            w_importance: 0.2,
            w_reinforcement: 0.2,
            max_age_us: 30 * 24 * 3600 * 1_000_000,
            reinforcement_cap: 100,
        }
    }
}

/// Strategy-specific configuration for recall.
#[derive(Debug, Clone, Default)]
pub struct StrategyConfig {
    pub strategy: RecallStrategy,
    pub entity_id: Option<String>,
    pub time_range: Option<(u64, u64)>,
    pub seed_memory_id: Option<Ulid>,
    pub edge_types: Vec<EdgeType>,
    pub max_depth: Option<u32>,
    pub top_k: Option<u32>,
    pub ef_search: Option<u32>,
    pub analogical_alpha: Option<f32>,
}

impl StrategyConfig {
    pub fn similarity() -> Self {
        Self {
            strategy: RecallStrategy::Similarity,
            ..Default::default()
        }
    }

    pub fn temporal(entity_id: impl Into<String>) -> Self {
        Self {
            strategy: RecallStrategy::Temporal,
            entity_id: Some(entity_id.into()),
            ..Default::default()
        }
    }

    pub fn causal(seed: Ulid) -> Self {
        Self {
            strategy: RecallStrategy::Causal,
            seed_memory_id: Some(seed),
            ..Default::default()
        }
    }

    pub fn analogical() -> Self {
        Self {
            strategy: RecallStrategy::Analogical,
            ..Default::default()
        }
    }
}

/// Options for `recall()`.
#[derive(Debug, Clone)]
pub struct RecallOptions {
    pub cue: String,
    pub strategies: Vec<StrategyConfig>,
    pub top_k: Option<u32>,
    pub scoring_weights: Option<ScoringWeights>,
    pub cue_context: Option<HashMap<String, serde_json::Value>>,
}

impl RecallOptions {
    pub fn new(cue: impl Into<String>) -> Self {
        Self {
            cue: cue.into(),
            strategies: vec![StrategyConfig::similarity()],
            top_k: None,
            scoring_weights: None,
            cue_context: None,
        }
    }

    pub fn strategy(mut self, s: StrategyConfig) -> Self {
        self.strategies = vec![s];
        self
    }

    pub fn strategies(mut self, s: Vec<StrategyConfig>) -> Self {
        self.strategies = s;
        self
    }

    pub fn top_k(mut self, k: u32) -> Self {
        self.top_k = Some(k);
        self
    }

    pub fn scoring_weights(mut self, w: ScoringWeights) -> Self {
        self.scoring_weights = Some(w);
        self
    }

    pub fn cue_context(mut self, ctx: HashMap<String, serde_json::Value>) -> Self {
        self.cue_context = Some(ctx);
        self
    }
}

/// Per-strategy detail attached to a recall result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StrategyDetail {
    Similarity {
        distance: f32,
        relevance: f32,
    },
    Temporal {
        timestamp: u64,
        rank: u32,
        relevance: f32,
    },
    Causal {
        depth: u32,
        edge_type: EdgeType,
        seed_id: Ulid,
        relevance: f32,
    },
    Analogical {
        embedding_similarity: f32,
        structural_similarity: f32,
        relevance: f32,
        used_vector_analogy: bool,
    },
}

/// A single recall result with its scored memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    pub memory: Memory,
    pub score: f32,
    pub strategy_details: Vec<StrategyDetail>,
}

/// An error from a specific recall strategy (non-fatal in multi-strategy mode).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyError {
    pub strategy: RecallStrategy,
    pub message: String,
}

/// Output from `recall()`.
#[derive(Debug, Clone)]
pub struct RecallOutput {
    pub results: Vec<RecallResult>,
    pub strategy_errors: Vec<StrategyError>,
}

/// Options for `prime()`.
#[derive(Debug, Clone, Default)]
pub struct PrimeOptions {
    pub entity_id: String,
    pub context: Option<HashMap<String, serde_json::Value>>,
    pub max_memories: Option<u32>,
    pub recency_window_us: Option<u64>,
    pub similarity_cue: Option<String>,
    pub scoring_weights: Option<ScoringWeights>,
}

impl PrimeOptions {
    pub fn new(entity_id: impl Into<String>) -> Self {
        Self {
            entity_id: entity_id.into(),
            ..Default::default()
        }
    }

    pub fn max_memories(mut self, n: u32) -> Self {
        self.max_memories = Some(n);
        self
    }

    pub fn similarity_cue(mut self, cue: impl Into<String>) -> Self {
        self.similarity_cue = Some(cue.into());
        self
    }
}

/// Output from `prime()`.
#[derive(Debug, Clone)]
pub struct PrimeOutput {
    pub results: Vec<RecallResult>,
    pub temporal_count: u32,
    pub similarity_count: u32,
}

/// Options for `revise()`.
#[derive(Debug, Clone, Default)]
pub struct ReviseOptions {
    pub memory_id: Ulid,
    pub content: Option<String>,
    pub importance: Option<f32>,
    pub context: Option<HashMap<String, serde_json::Value>>,
    pub context_mode: ContextMode,
    pub entity_id: Option<String>,
    pub edges: Vec<RememberEdge>,
}

impl ReviseOptions {
    pub fn new(memory_id: Ulid) -> Self {
        Self {
            memory_id,
            ..Default::default()
        }
    }

    pub fn content(mut self, c: impl Into<String>) -> Self {
        self.content = Some(c.into());
        self
    }

    pub fn importance(mut self, v: f32) -> Self {
        self.importance = Some(v);
        self
    }

    pub fn context_replace(mut self, ctx: HashMap<String, serde_json::Value>) -> Self {
        self.context = Some(ctx);
        self.context_mode = ContextMode::Replace;
        self
    }

    pub fn context_merge(mut self, ctx: HashMap<String, serde_json::Value>) -> Self {
        self.context = Some(ctx);
        self.context_mode = ContextMode::Merge;
        self
    }

    pub fn entity_id(mut self, e: impl Into<String>) -> Self {
        self.entity_id = Some(e.into());
        self
    }

    pub fn edge(mut self, e: RememberEdge) -> Self {
        self.edges.push(e);
        self
    }
}

/// Criteria for `forget()`.
#[derive(Debug, Clone, Default)]
pub struct ForgetCriteria {
    pub memory_ids: Vec<Ulid>,
    pub entity_id: Option<String>,
    pub staleness_threshold_us: Option<u64>,
    pub access_count_floor: Option<u64>,
    pub memory_kind: Option<MemoryKind>,
    pub decay_score_floor: Option<f32>,
}

impl ForgetCriteria {
    pub fn by_id(id: Ulid) -> Self {
        Self {
            memory_ids: vec![id],
            ..Default::default()
        }
    }

    pub fn by_ids(ids: Vec<Ulid>) -> Self {
        Self {
            memory_ids: ids,
            ..Default::default()
        }
    }

    pub fn by_entity(entity_id: impl Into<String>) -> Self {
        Self {
            entity_id: Some(entity_id.into()),
            ..Default::default()
        }
    }
}

/// Output from `forget()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgetOutput {
    pub forgotten_count: u64,
    pub cascade_count: u64,
    pub truncated: bool,
    pub tombstone_count: u64,
}

/// Options for `subscribe()`.
#[derive(Debug, Clone)]
pub struct SubscribeOptions {
    pub entity_id: Option<String>,
    pub kind_filter: Vec<MemoryKind>,
    pub confidence_threshold: f32,
    pub time_scope_us: Option<u64>,
    pub output_buffer_size: Option<u32>,
    pub coarse_threshold: Option<f32>,
}

impl Default for SubscribeOptions {
    fn default() -> Self {
        Self {
            entity_id: None,
            kind_filter: Vec::new(),
            confidence_threshold: 0.6,
            time_scope_us: None,
            output_buffer_size: None,
            coarse_threshold: None,
        }
    }
}

impl SubscribeOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entity_id(mut self, e: impl Into<String>) -> Self {
        self.entity_id = Some(e.into());
        self
    }

    pub fn confidence_threshold(mut self, t: f32) -> Self {
        self.confidence_threshold = t;
        self
    }
}

/// A push received from a subscription stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribePush {
    pub subscription_id: u64,
    pub memory: Memory,
    pub confidence: f32,
    pub push_timestamp_us: u64,
    pub sequence_number: u64,
}

/// Scope for reflection.
#[derive(Debug, Clone)]
pub enum ReflectScope {
    Entity {
        entity_id: String,
        since_us: Option<u64>,
    },
    Global {
        since_us: Option<u64>,
    },
}

/// Output from `reflect()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectOutput {
    pub insights_created: u64,
    pub clusters_found: u64,
    pub clusters_processed: u64,
    pub memories_processed: u64,
}

/// Filter for querying insights.
#[derive(Debug, Clone, Default)]
pub struct InsightsFilter {
    pub entity_id: Option<String>,
    pub min_confidence: Option<f32>,
    pub max_results: Option<u32>,
}

impl InsightsFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entity_id(mut self, e: impl Into<String>) -> Self {
        self.entity_id = Some(e.into());
        self
    }

    pub fn min_confidence(mut self, c: f32) -> Self {
        self.min_confidence = Some(c);
        self
    }

    pub fn max_results(mut self, n: u32) -> Self {
        self.max_results = Some(n);
        self
    }
}

/// Server health status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: ServingStatus,
    pub version: String,
    pub memory_count: u64,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServingStatus {
    Unknown,
    Serving,
    NotServing,
}

impl fmt::Display for ServingStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServingStatus::Unknown => write!(f, "UNKNOWN"),
            ServingStatus::Serving => write!(f, "SERVING"),
            ServingStatus::NotServing => write!(f, "NOT_SERVING"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_options_builder() {
        let opts = RememberOptions::new("test content")
            .importance(0.9)
            .entity_id("acme");
        assert_eq!(opts.content, "test content");
        assert_eq!(opts.importance, Some(0.9));
        assert_eq!(opts.entity_id, Some("acme".to_string()));
    }

    #[test]
    fn recall_options_default_similarity() {
        let opts = RecallOptions::new("query");
        assert_eq!(opts.strategies.len(), 1);
        assert_eq!(opts.strategies[0].strategy, RecallStrategy::Similarity);
    }

    #[test]
    fn forget_criteria_by_entity() {
        let c = ForgetCriteria::by_entity("customer_123");
        assert_eq!(c.entity_id, Some("customer_123".to_string()));
        assert!(c.memory_ids.is_empty());
    }

    #[test]
    fn revise_options_builder() {
        let id = Ulid::new();
        let opts = ReviseOptions::new(id)
            .content("new content")
            .importance(0.5);
        assert_eq!(opts.memory_id, id);
        assert_eq!(opts.content, Some("new content".to_string()));
        assert_eq!(opts.importance, Some(0.5));
    }

    #[test]
    fn subscribe_options_builder() {
        let opts = SubscribeOptions::new()
            .entity_id("acme")
            .confidence_threshold(0.8);
        assert_eq!(opts.entity_id, Some("acme".to_string()));
        assert!((opts.confidence_threshold - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn strategy_config_constructors() {
        let s = StrategyConfig::similarity();
        assert_eq!(s.strategy, RecallStrategy::Similarity);

        let t = StrategyConfig::temporal("ent");
        assert_eq!(t.strategy, RecallStrategy::Temporal);
        assert_eq!(t.entity_id, Some("ent".to_string()));

        let id = Ulid::new();
        let c = StrategyConfig::causal(id);
        assert_eq!(c.strategy, RecallStrategy::Causal);
        assert_eq!(c.seed_memory_id, Some(id));

        let a = StrategyConfig::analogical();
        assert_eq!(a.strategy, RecallStrategy::Analogical);
    }

    #[test]
    fn memory_kind_display() {
        assert_eq!(MemoryKind::Episode.to_string(), "Episode");
        assert_eq!(MemoryKind::Insight.to_string(), "Insight");
        assert_eq!(MemoryKind::Revision.to_string(), "Revision");
    }

    #[test]
    fn edge_type_display() {
        assert_eq!(EdgeType::CausedBy.to_string(), "caused_by");
        assert_eq!(EdgeType::RelatedTo.to_string(), "related_to");
    }

    #[test]
    fn insights_filter_builder() {
        let f = InsightsFilter::new()
            .entity_id("e1")
            .min_confidence(0.5)
            .max_results(10);
        assert_eq!(f.entity_id, Some("e1".to_string()));
        assert_eq!(f.min_confidence, Some(0.5));
        assert_eq!(f.max_results, Some(10));
    }

    #[test]
    fn prime_options_builder() {
        let opts = PrimeOptions::new("ent1")
            .max_memories(50)
            .similarity_cue("test cue");
        assert_eq!(opts.entity_id, "ent1");
        assert_eq!(opts.max_memories, Some(50));
        assert_eq!(opts.similarity_cue, Some("test cue".to_string()));
    }

    #[test]
    fn scoring_weights_defaults_sum_to_one() {
        let w = ScoringWeights::default();
        let sum = w.w_relevance + w.w_recency + w.w_importance + w.w_reinforcement;
        assert!((sum - 1.0).abs() < f32::EPSILON);
    }
}
