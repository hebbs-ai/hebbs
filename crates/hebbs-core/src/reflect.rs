use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, Sender};
use ulid::Generator;

use hebbs_embed::Embedder;
use hebbs_index::{EdgeInput, EdgeType, IndexManager};
use hebbs_reflect::{
    LlmProvider, LlmProviderConfig, MemoryEntry, PipelineConfig, ReflectInput, ReflectPipeline,
};
use hebbs_storage::{BatchOperation, ColumnFamilyName, StorageBackend};

use crate::error::{HebbsError, Result};
use crate::keys;
use crate::memory::{Memory, MemoryKind};
use crate::subscribe::SubscriptionRegistry;

// ── Meta CF keys for cursor persistence ───────────────────────────

const META_REFLECT_CURSOR_PREFIX: &str = "reflect_cursor:";
const META_STALE_INSIGHTS_PREFIX: &str = "stale_insight:";

// ── Public types ──────────────────────────────────────────────────

/// Full configuration for the reflection subsystem.
#[derive(Debug, Clone)]
pub struct ReflectConfig {
    pub max_memories_per_reflect: usize,
    pub min_memories_for_reflect: usize,
    pub min_cluster_size: usize,
    pub max_clusters: usize,
    pub clustering_seed: u64,
    pub max_iterations: usize,
    pub proposal_max_tokens: usize,
    pub validation_max_tokens: usize,
    pub insight_importance_weight: f32,

    pub proposal_provider_config: LlmProviderConfig,
    pub validation_provider_config: LlmProviderConfig,

    pub trigger_check_interval_us: u64,
    pub threshold_trigger_count: usize,
    pub schedule_trigger_interval_us: u64,
    pub enabled: bool,
}

impl Default for ReflectConfig {
    fn default() -> Self {
        Self {
            max_memories_per_reflect: 500,
            min_memories_for_reflect: 5,
            min_cluster_size: 3,
            max_clusters: 50,
            clustering_seed: 42,
            max_iterations: 50,
            proposal_max_tokens: 4000,
            validation_max_tokens: 6000,
            insight_importance_weight: 0.7,
            proposal_provider_config: LlmProviderConfig::default(),
            validation_provider_config: LlmProviderConfig::default(),
            trigger_check_interval_us: 60_000_000,
            threshold_trigger_count: 50,
            schedule_trigger_interval_us: 3_600_000_000,
            enabled: true,
        }
    }
}

impl ReflectConfig {
    pub fn validated(mut self) -> Self {
        self.max_memories_per_reflect = self.max_memories_per_reflect.clamp(10, 10_000);
        self.min_memories_for_reflect = self.min_memories_for_reflect.clamp(3, 1000);
        self.min_cluster_size = self.min_cluster_size.clamp(2, 100);
        self.max_clusters = self.max_clusters.clamp(2, 200);
        self.max_iterations = self.max_iterations.clamp(5, 200);
        self.insight_importance_weight = self.insight_importance_weight.clamp(0.0, 1.0);
        self.trigger_check_interval_us = self.trigger_check_interval_us.max(1_000_000);
        self
    }

    fn to_pipeline_config(&self) -> PipelineConfig {
        PipelineConfig {
            min_cluster_size: self.min_cluster_size,
            max_clusters: self.max_clusters,
            clustering_seed: self.clustering_seed,
            max_iterations: self.max_iterations,
            proposal_max_tokens: self.proposal_max_tokens,
            validation_max_tokens: self.validation_max_tokens,
            insight_importance_weight: self.insight_importance_weight,
        }
    }
}

/// Scope for a reflect run.
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

/// Output from a reflect() invocation.
#[derive(Debug)]
pub struct ReflectRunOutput {
    pub insights_created: usize,
    pub clusters_found: usize,
    pub clusters_processed: usize,
    pub memories_processed: usize,
}

/// Filter for the insights() query.
#[derive(Debug, Clone, Default)]
pub struct InsightsFilter {
    pub entity_id: Option<String>,
    pub min_confidence: Option<f32>,
    pub max_results: Option<usize>,
}

// ── Background worker ─────────────────────────────────────────────

#[derive(Debug)]
pub(crate) enum ReflectSignal {
    Resume,
    Pause,
    Shutdown,
    Reconfigure(Box<ReflectConfig>),
    TriggerNow(ReflectScope),
}

pub(crate) struct ReflectHandle {
    tx: Sender<ReflectSignal>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ReflectHandle {
    pub fn resume(&self) {
        let _ = self.tx.send(ReflectSignal::Resume);
    }

    pub fn pause(&self) {
        let _ = self.tx.send(ReflectSignal::Pause);
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(ReflectSignal::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    pub fn reconfigure(&self, config: ReflectConfig) {
        let _ = self.tx.send(ReflectSignal::Reconfigure(Box::new(config)));
    }

    pub fn trigger_now(&self, scope: ReflectScope) {
        let _ = self.tx.send(ReflectSignal::TriggerNow(scope));
    }
}

pub(crate) fn spawn_reflect_worker(
    storage: Arc<dyn StorageBackend>,
    embedder: Arc<dyn Embedder>,
    index_manager: Arc<IndexManager>,
    subscribe_registry: Arc<SubscriptionRegistry>,
    config: ReflectConfig,
) -> ReflectHandle {
    let (tx, rx) = crossbeam_channel::unbounded();

    let thread = thread::Builder::new()
        .name("hebbs-reflect".into())
        .spawn(move || {
            reflect_worker_loop(
                storage,
                embedder,
                index_manager,
                subscribe_registry,
                config,
                rx,
            );
        })
        .expect("failed to spawn reflect worker thread");

    ReflectHandle {
        tx,
        thread: Some(thread),
    }
}

fn reflect_worker_loop(
    storage: Arc<dyn StorageBackend>,
    embedder: Arc<dyn Embedder>,
    index_manager: Arc<IndexManager>,
    subscribe_registry: Arc<SubscriptionRegistry>,
    mut config: ReflectConfig,
    rx: Receiver<ReflectSignal>,
) {
    let mut paused = true;
    let mut last_scheduled_us: u64 = 0;
    let mut writes_since_last: usize = 0;

    loop {
        let timeout = std::time::Duration::from_micros(config.trigger_check_interval_us);
        match rx.recv_timeout(timeout) {
            Ok(ReflectSignal::Shutdown) => return,
            Ok(ReflectSignal::Pause) => {
                paused = true;
                continue;
            }
            Ok(ReflectSignal::Resume) => {
                paused = false;
                continue;
            }
            Ok(ReflectSignal::Reconfigure(new_config)) => {
                config = (*new_config).validated();
                continue;
            }
            Ok(ReflectSignal::TriggerNow(scope)) => {
                if !paused && config.enabled {
                    let _ = run_reflect_background(
                        &storage,
                        &embedder,
                        &index_manager,
                        &subscribe_registry,
                        &config,
                        &scope,
                    );
                }
                continue;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
        }

        if paused || !config.enabled {
            continue;
        }

        let now_us = now_microseconds();

        let threshold_triggered = writes_since_last >= config.threshold_trigger_count;
        let schedule_triggered =
            now_us.saturating_sub(last_scheduled_us) >= config.schedule_trigger_interval_us;

        if threshold_triggered || schedule_triggered {
            let scope = ReflectScope::Global { since_us: None };
            if run_reflect_background(
                &storage,
                &embedder,
                &index_manager,
                &subscribe_registry,
                &config,
                &scope,
            )
            .is_ok()
            {
                writes_since_last = 0;
                last_scheduled_us = now_us;
            }
        }
    }
}

// ── Shared reflect logic (used by Engine::reflect and background worker) ──

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_reflect_shared(
    storage: &Arc<dyn StorageBackend>,
    embedder: &Arc<dyn Embedder>,
    index_manager: &Arc<IndexManager>,
    subscribe_registry: &Arc<SubscriptionRegistry>,
    config: &ReflectConfig,
    scope: &ReflectScope,
    proposal_provider: &dyn LlmProvider,
    validation_provider: &dyn LlmProvider,
) -> Result<ReflectRunOutput> {
    let memories = scope_memories(storage, scope, config)?;
    let memories_count = memories.len();

    if memories_count < config.min_memories_for_reflect {
        return Ok(ReflectRunOutput {
            insights_created: 0,
            clusters_found: 0,
            clusters_processed: 0,
            memories_processed: memories_count,
        });
    }

    let existing_insights = load_existing_insights(storage)?;

    let entries: Vec<MemoryEntry> = memories.iter().map(memory_to_entry).collect();

    let existing_entries: Vec<MemoryEntry> =
        existing_insights.iter().map(memory_to_entry).collect();

    let input = ReflectInput {
        memories: entries,
        existing_insights: existing_entries,
        config: config.to_pipeline_config(),
    };

    let output = ReflectPipeline::run(input, proposal_provider, validation_provider)?;

    let clusters_found = output.clusters.len();
    let clusters_processed = output
        .clusters
        .iter()
        .filter(|c| matches!(c.status, hebbs_reflect::ClusterStatus::Success { .. }))
        .count();

    let mut ulid_gen = Generator::new();
    let mut insights_created = 0;

    let scope_entity_id = match scope {
        ReflectScope::Entity { entity_id, .. } => Some(entity_id.clone()),
        ReflectScope::Global { .. } => None,
    };

    for produced in &output.insights {
        match store_insight(
            storage,
            embedder,
            index_manager,
            subscribe_registry,
            &mut ulid_gen,
            produced,
            scope_entity_id.as_deref(),
        ) {
            Ok(_) => insights_created += 1,
            Err(_) => continue,
        }
    }

    update_reflect_cursor(storage, scope)?;

    Ok(ReflectRunOutput {
        insights_created,
        clusters_found,
        clusters_processed,
        memories_processed: memories_count,
    })
}

fn run_reflect_background(
    storage: &Arc<dyn StorageBackend>,
    embedder: &Arc<dyn Embedder>,
    index_manager: &Arc<IndexManager>,
    subscribe_registry: &Arc<SubscriptionRegistry>,
    config: &ReflectConfig,
    scope: &ReflectScope,
) -> Result<ReflectRunOutput> {
    let proposal_provider: Box<dyn LlmProvider> =
        hebbs_reflect::create_provider(&config.proposal_provider_config)?;
    let validation_provider: Box<dyn LlmProvider> =
        hebbs_reflect::create_provider(&config.validation_provider_config)?;

    run_reflect_shared(
        storage,
        embedder,
        index_manager,
        subscribe_registry,
        config,
        scope,
        proposal_provider.as_ref(),
        validation_provider.as_ref(),
    )
}

// ── Scoping ───────────────────────────────────────────────────────

fn scope_memories(
    storage: &Arc<dyn StorageBackend>,
    scope: &ReflectScope,
    config: &ReflectConfig,
) -> Result<Vec<Memory>> {
    let all = storage
        .prefix_iterator(ColumnFamilyName::Default, &[])
        .map_err(HebbsError::Storage)?;

    let since_us = match scope {
        ReflectScope::Entity { since_us, .. } => *since_us,
        ReflectScope::Global { since_us } => *since_us,
    };

    let entity_filter = match scope {
        ReflectScope::Entity { entity_id, .. } => Some(entity_id.as_str()),
        ReflectScope::Global { .. } => None,
    };

    let cursor_key = cursor_key_for_scope(scope);
    let cursor_us = read_cursor(storage, &cursor_key);
    let effective_since = since_us.unwrap_or(0).max(cursor_us);

    let mut memories = Vec::new();
    for (_key, value) in all {
        let mem = Memory::from_bytes(&value).map_err(|e| HebbsError::Serialization {
            message: format!("failed to deserialize memory during reflect scope: {e}"),
        })?;

        if mem.kind != MemoryKind::Episode {
            continue;
        }

        if mem.created_at < effective_since {
            continue;
        }

        if let Some(eid) = entity_filter {
            if mem.entity_id.as_deref() != Some(eid) {
                continue;
            }
        }

        if mem.embedding.is_none() {
            continue;
        }

        memories.push(mem);
        if memories.len() >= config.max_memories_per_reflect {
            break;
        }
    }

    Ok(memories)
}

fn load_existing_insights(storage: &Arc<dyn StorageBackend>) -> Result<Vec<Memory>> {
    let all = storage
        .prefix_iterator(ColumnFamilyName::Default, &[])
        .map_err(HebbsError::Storage)?;

    let mut insights = Vec::new();
    for (_key, value) in all {
        if let Ok(mem) = Memory::from_bytes(&value) {
            if mem.kind == MemoryKind::Insight && mem.embedding.is_some() {
                insights.push(mem);
            }
        }
    }
    Ok(insights)
}

fn memory_to_entry(m: &Memory) -> MemoryEntry {
    let mut id = [0u8; 16];
    let len = m.memory_id.len().min(16);
    id[..len].copy_from_slice(&m.memory_id[..len]);
    let content_emb = m.embedding.clone().unwrap_or_default();
    let assoc_emb = m.associative_embedding.clone().unwrap_or_else(|| content_emb.clone());
    MemoryEntry {
        id,
        content: m.content.clone(),
        importance: m.importance,
        entity_id: m.entity_id.clone(),
        embedding: content_emb,
        created_at: m.created_at,
        assoc_embedding: assoc_emb,
    }
}

// ── Consolidation: store a produced insight as a Memory ──────────

fn store_insight(
    storage: &Arc<dyn StorageBackend>,
    embedder: &Arc<dyn Embedder>,
    index_manager: &Arc<IndexManager>,
    subscribe_registry: &Arc<SubscriptionRegistry>,
    ulid_gen: &mut Generator,
    produced: &hebbs_reflect::ProducedInsight,
    scope_entity_id: Option<&str>,
) -> Result<Memory> {
    let embedding = embedder.embed(&produced.content)?;

    let ulid = ulid_gen.generate().map_err(|e| HebbsError::Internal {
        operation: "reflect_consolidate",
        message: format!("ULID generation overflow: {e}"),
    })?;
    let now_us = now_microseconds();
    let memory_id_bytes = ulid.to_bytes();
    let mut memory_id = [0u8; 16];
    memory_id.copy_from_slice(&memory_id_bytes);

    let mut context = HashMap::new();
    context.insert(
        "reflect_cluster_id".to_string(),
        serde_json::Value::Number(serde_json::Number::from(produced.cluster_id as u64)),
    );
    context.insert(
        "reflect_confidence".to_string(),
        serde_json::json!(produced.confidence),
    );
    context.insert(
        "reflect_source_count".to_string(),
        serde_json::Value::Number(serde_json::Number::from(
            produced.source_memory_ids.len() as u64
        )),
    );
    if !produced.tags.is_empty() {
        context.insert("reflect_tags".to_string(), serde_json::json!(produced.tags));
    }
    let context_bytes =
        Memory::serialize_context(&context).map_err(|e| HebbsError::Serialization {
            message: format!("failed to serialize insight context: {e}"),
        })?;

    let entity_id = scope_entity_id.map(|s| s.to_string());

    let memory = Memory {
        memory_id: memory_id.to_vec(),
        content: produced.content.clone(),
        importance: produced.confidence,
        context_bytes,
        entity_id: entity_id.clone(),
        embedding: Some(embedding.clone()),
        created_at: now_us,
        updated_at: now_us,
        last_accessed_at: now_us,
        access_count: 0,
        decay_score: produced.confidence,
        kind: MemoryKind::Insight,
        device_id: None,
        logical_clock: 0,
        associative_embedding: Some(embedding.clone()),
    };

    let edge_inputs: Vec<EdgeInput> = produced
        .source_memory_ids
        .iter()
        .map(|&source_id| EdgeInput {
            target_id: source_id,
            edge_type: EdgeType::InsightFrom,
            confidence: produced.confidence,
        })
        .collect();

    let (index_ops, _temp_node) = index_manager.prepare_insert(
        &memory_id,
        &embedding,
        &embedding,
        entity_id.as_deref(),
        now_us,
        &edge_inputs,
    )?;

    let memory_value = memory.to_bytes();
    let memory_key = keys::encode_memory_key(&memory_id);

    let mut all_ops = Vec::with_capacity(1 + index_ops.len());
    all_ops.push(BatchOperation::Put {
        cf: ColumnFamilyName::Default,
        key: memory_key,
        value: memory_value,
    });
    all_ops.extend(index_ops);

    storage.write_batch(&all_ops)?;
    index_manager.commit_insert(memory_id, embedding)?;
    subscribe_registry.notify_new_write(memory_id);

    Ok(memory)
}

// ── Insight invalidation ──────────────────────────────────────────

/// Mark insights that reference the given source memory as stale.
/// Called from revise() and forget() paths.
pub(crate) fn mark_insights_stale_for_source(
    storage: &Arc<dyn StorageBackend>,
    index_manager: &Arc<IndexManager>,
    source_memory_id: &[u8; 16],
) {
    if let Ok(incoming) = index_manager.incoming_edges(source_memory_id) {
        for (edge_type, from_id, _meta) in incoming {
            if edge_type == EdgeType::InsightFrom {
                let stale_key = keys::encode_meta_key(&format!(
                    "{}{}",
                    META_STALE_INSIGHTS_PREFIX,
                    hex::encode(from_id)
                ));
                let _ = storage.put(ColumnFamilyName::Meta, &stale_key, &[1]);
            }
        }
    }
}

/// Read all stale insight IDs (used by background re-validation).
#[allow(dead_code)]
pub(crate) fn read_stale_insight_ids(storage: &Arc<dyn StorageBackend>) -> Vec<[u8; 16]> {
    let prefix = keys::encode_meta_key(META_STALE_INSIGHTS_PREFIX);
    let entries = storage
        .prefix_iterator(ColumnFamilyName::Meta, &prefix)
        .unwrap_or_default();

    entries
        .iter()
        .filter_map(|(key, _value)| {
            let key_str = std::str::from_utf8(key).ok()?;
            let hex_id = key_str.strip_prefix(META_STALE_INSIGHTS_PREFIX)?;
            let bytes = hex::decode(hex_id).ok()?;
            if bytes.len() == 16 {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes);
                Some(arr)
            } else {
                None
            }
        })
        .collect()
}

// ── Cursor management ─────────────────────────────────────────────

fn cursor_key_for_scope(scope: &ReflectScope) -> String {
    match scope {
        ReflectScope::Entity { entity_id, .. } => {
            format!("{}{}", META_REFLECT_CURSOR_PREFIX, entity_id)
        }
        ReflectScope::Global { .. } => format!("{}global", META_REFLECT_CURSOR_PREFIX),
    }
}

fn read_cursor(storage: &Arc<dyn StorageBackend>, key: &str) -> u64 {
    let meta_key = keys::encode_meta_key(key);
    storage
        .get(ColumnFamilyName::Meta, &meta_key)
        .ok()
        .flatten()
        .and_then(|bytes| {
            if bytes.len() == 8 {
                Some(u64::from_be_bytes(bytes.try_into().ok()?))
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn update_reflect_cursor(storage: &Arc<dyn StorageBackend>, scope: &ReflectScope) -> Result<()> {
    let key = cursor_key_for_scope(scope);
    let meta_key = keys::encode_meta_key(&key);
    let now_us = now_microseconds();
    storage
        .put(ColumnFamilyName::Meta, &meta_key, &now_us.to_be_bytes())
        .map_err(HebbsError::Storage)
}

fn now_microseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

// ── Insights query ────────────────────────────────────────────────

pub(crate) fn query_insights(
    storage: &Arc<dyn StorageBackend>,
    filter: &InsightsFilter,
) -> Result<Vec<Memory>> {
    let all = storage
        .prefix_iterator(ColumnFamilyName::Default, &[])
        .map_err(HebbsError::Storage)?;

    let max = filter.max_results.unwrap_or(100);
    let mut results = Vec::new();

    for (_key, value) in all {
        if let Ok(mem) = Memory::from_bytes(&value) {
            if mem.kind != MemoryKind::Insight {
                continue;
            }

            if let Some(ref eid) = filter.entity_id {
                if mem.entity_id.as_deref() != Some(eid.as_str()) {
                    continue;
                }
            }

            if let Some(min_conf) = filter.min_confidence {
                if mem.importance < min_conf {
                    continue;
                }
            }

            results.push(mem);
            if results.len() >= max {
                break;
            }
        }
    }

    results.sort_by(|a, b| {
        b.importance
            .partial_cmp(&a.importance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflect_config_validation_clamps() {
        let config = ReflectConfig {
            max_memories_per_reflect: 0,
            min_memories_for_reflect: 0,
            min_cluster_size: 0,
            max_clusters: 0,
            max_iterations: 0,
            insight_importance_weight: -1.0,
            trigger_check_interval_us: 0,
            ..Default::default()
        };
        let validated = config.validated();
        assert_eq!(validated.max_memories_per_reflect, 10);
        assert_eq!(validated.min_memories_for_reflect, 3);
        assert_eq!(validated.min_cluster_size, 2);
        assert_eq!(validated.max_clusters, 2);
        assert_eq!(validated.max_iterations, 5);
        assert_eq!(validated.insight_importance_weight, 0.0);
        assert_eq!(validated.trigger_check_interval_us, 1_000_000);
    }

    #[test]
    fn cursor_key_for_entity_scope() {
        let scope = ReflectScope::Entity {
            entity_id: "customer_123".into(),
            since_us: None,
        };
        let key = cursor_key_for_scope(&scope);
        assert!(key.contains("customer_123"));
    }

    #[test]
    fn cursor_key_for_global_scope() {
        let scope = ReflectScope::Global { since_us: None };
        let key = cursor_key_for_scope(&scope);
        assert!(key.contains("global"));
    }
}
