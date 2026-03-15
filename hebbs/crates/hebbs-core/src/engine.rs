use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Instrumentation span that compiles to a no-op unless `bench-instrument` is enabled.
/// Zero cost in production builds. Returns an entered span guard; use `bench_span_drop!`
/// to end the span early before the variable goes out of scope.
macro_rules! bench_span {
    ($name:expr) => {{
        #[cfg(feature = "bench-instrument")]
        let _guard = tracing::debug_span!($name).entered();
        #[cfg(not(feature = "bench-instrument"))]
        let _guard = ();
        _guard
    }};
}

macro_rules! bench_span_drop {
    ($guard:ident) => {
        #[cfg(feature = "bench-instrument")]
        drop($guard);
    };
}

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use ulid::Generator;

use hebbs_embed::normalize::cosine_similarity;
use hebbs_embed::Embedder;
use hebbs_index::{EdgeInput, EdgeType, HnswParams, IndexManager, TemporalOrder, TraversalEntry};
use hebbs_storage::{BatchOperation, ColumnFamilyName, StorageBackend, TenantScopedStorage};

use crate::contradict;
use crate::decay::{
    clear_auto_forget_candidates, read_auto_forget_candidates, spawn_decay_worker, DecayConfig,
    DecayHandle,
};
use crate::error::{HebbsError, Result};
use crate::forget::{
    encode_tombstone_key, ForgetConfig, ForgetCriteria, ForgetOutput, Tombstone,
    MAX_FORGET_BATCH_SIZE,
};
use crate::keys;
use crate::memory::{Memory, MemoryKind, DEFAULT_IMPORTANCE, MAX_CONTENT_LENGTH, MAX_CONTEXT_SIZE};
use crate::recall::{
    AnalogicalWeights, CausalDirection, PrimeInput, PrimeOutput, RecallInput, RecallOutput,
    RecallResult, RecallStrategy, ScoringWeights, StrategyDetail, StrategyError, StrategyOutcome,
    StrategyResult, DEFAULT_PRIME_RECENCY_WINDOW_US, MAX_PRIME_MEMORIES, MAX_TOP_K,
    MAX_TRAVERSAL_DEPTH,
};
use crate::reflect::{
    self, spawn_reflect_worker, InsightsFilter, ReflectConfig, ReflectHandle, ReflectRunOutput,
    ReflectScope,
};
use crate::revise::{ContextMode, ReviseInput};
use crate::subscribe::{
    create_subscription, SubscribeConfig, SubscriptionHandle, SubscriptionRegistry,
};
use crate::tenant::TenantContext;

/// Current schema version. Written to the `meta` CF on first open.
/// Incremented only on breaking serialization changes (which should
/// never happen thanks to append-only schema evolution).
pub const SCHEMA_VERSION: u32 = 1;

/// Over-fetch multiplier for HNSW searches when entity_id filtering
/// is active. The global HNSW index does not partition by entity, so
/// we fetch `top_k * ENTITY_OVERSAMPLE` candidates, post-filter by
/// entity, and truncate to the requested `top_k`.
const ENTITY_OVERSAMPLE: usize = 4;

/// Upper bound on entity memories scanned during prime's similarity
/// phase.  The temporal index is queried with full time range to find
/// all memory IDs for the entity, then their embeddings are loaded and
/// ranked by cosine similarity with the cue.  This avoids the global
/// HNSW + entity post-filter approach which fails when the entity is a
/// small fraction of total memories.
///
/// Bounded per Principle 4 to cap the O(n * d) brute-force scan.
const PRIME_ENTITY_SCAN_LIMIT: usize = 500;

/// Input for the `remember()` operation.
///
/// Separating input from the stored `Memory` struct keeps the public
/// API stable even as internal fields evolve.
pub struct RememberInput {
    pub content: String,
    pub importance: Option<f32>,
    pub context: Option<HashMap<String, serde_json::Value>>,
    pub entity_id: Option<String>,
    /// Graph edges to create. Each edge connects this new memory to an existing memory.
    /// Added in Phase 3 for causal/relational context.
    pub edges: Vec<RememberEdge>,
}

/// An edge to create during remember().
pub struct RememberEdge {
    pub target_id: [u8; 16],
    pub edge_type: EdgeType,
    pub confidence: Option<f32>,
}

/// Output from a timed `remember` operation, returning the stored memory
/// alongside instrumentation data (embedding duration).
#[derive(Debug)]
pub struct RememberOutput {
    pub memory: Memory,
    /// Time spent in the embedder, in microseconds.
    pub embed_duration_us: u64,
}

/// The HEBBS cognitive memory engine.
///
/// All public methods take `&self` (shared reference) — never `&mut self`.
/// The storage backend handles its own concurrency (RocksDB is internally
/// thread-safe; in-memory backend uses per-CF `RwLock`).
///
/// ## Phase 3 changes
///
/// The engine now coordinates with `IndexManager` for atomic multi-index writes.
/// `remember()` writes to all four column families (default, temporal, vectors, graph)
/// in a single WriteBatch. `delete()` removes from all four atomically.
pub struct Engine {
    storage: Arc<dyn StorageBackend>,
    /// Embedding provider. All returned vectors are L2-normalized with
    /// dimensionality reported by `embedder.dimensions()`.
    embedder: Arc<dyn Embedder>,
    /// Index manager: coordinates temporal, vector, and graph indexes.
    /// Wrapped in Arc for sharing with subscription worker threads.
    index_manager: Arc<IndexManager>,
    /// Monotonic ULID generator. Increments the random component if
    /// the same millisecond is hit, guaranteeing strict ordering of
    /// IDs generated within the same process.
    ulid_gen: Mutex<Generator>,
    /// Forget operation configuration.
    forget_config: ForgetConfig,
    /// Decay engine handle (optional, started via `start_decay()`).
    decay_handle: Mutex<Option<DecayHandle>>,
    /// Phase 6: subscription registry tracking active subscriptions.
    /// Shared with SubscriptionHandles for lifecycle management.
    subscribe_registry: Arc<SubscriptionRegistry>,
    /// Phase 7: reflection pipeline handle (optional, started via `start_reflect()`).
    reflect_handle: Mutex<Option<ReflectHandle>>,
    /// Session store for agent-driven two-step reflect (prepare/commit).
    reflect_session_store: Arc<reflect::ReflectSessionStore>,
    /// Maximum predecessor snapshots retained per memory during revisions.
    /// When exceeded, the oldest snapshots are pruned atomically.
    max_snapshots_per_memory: u32,
}

/// Default maximum number of predecessor snapshots kept per memory.
pub const DEFAULT_MAX_SNAPSHOTS_PER_MEMORY: u32 = 100;

impl Engine {
    /// Create a new engine backed by the given storage and embedder.
    ///
    /// On first open, writes `schema_version` to the `meta` column family.
    /// On subsequent opens, validates that the stored schema version is
    /// compatible with this binary.
    ///
    /// The IndexManager is initialized here, which triggers HNSW rebuild
    /// from the vectors CF if existing data is present.
    pub fn new(storage: Arc<dyn StorageBackend>, embedder: Arc<dyn Embedder>) -> Result<Self> {
        let hnsw_params = HnswParams::new(embedder.dimensions());
        let index_manager = Arc::new(IndexManager::new(storage.clone(), hnsw_params)?);

        let engine = Self {
            storage,
            embedder,
            index_manager,
            ulid_gen: Mutex::new(Generator::new()),
            forget_config: ForgetConfig::default(),
            decay_handle: Mutex::new(None),
            subscribe_registry: Arc::new(SubscriptionRegistry::new(100)),
            reflect_handle: Mutex::new(None),
            reflect_session_store: Arc::new(reflect::ReflectSessionStore::new()),
            max_snapshots_per_memory: DEFAULT_MAX_SNAPSHOTS_PER_MEMORY,
        };
        engine.init_schema()?;
        Ok(engine)
    }

    /// Create with custom HNSW parameters and a fixed seed (for testing).
    pub fn new_with_params(
        storage: Arc<dyn StorageBackend>,
        embedder: Arc<dyn Embedder>,
        hnsw_params: HnswParams,
        seed: u64,
    ) -> Result<Self> {
        let index_manager = Arc::new(IndexManager::new_with_seed(
            storage.clone(),
            hnsw_params,
            seed,
        )?);

        let engine = Self {
            storage,
            embedder,
            index_manager,
            ulid_gen: Mutex::new(Generator::new()),
            forget_config: ForgetConfig::default(),
            decay_handle: Mutex::new(None),
            subscribe_registry: Arc::new(SubscriptionRegistry::new(100)),
            reflect_handle: Mutex::new(None),
            reflect_session_store: Arc::new(reflect::ReflectSessionStore::new()),
            max_snapshots_per_memory: DEFAULT_MAX_SNAPSHOTS_PER_MEMORY,
        };
        engine.init_schema()?;
        Ok(engine)
    }

    /// Set the forget configuration.
    pub fn set_forget_config(&mut self, config: ForgetConfig) {
        self.forget_config = config;
    }

    /// Set the maximum number of predecessor snapshots retained per memory.
    ///
    /// When a `revise()` operation causes the snapshot count to exceed this
    /// limit, the oldest snapshots (by ULID / creation time) are pruned
    /// atomically within the same WriteBatch.
    pub fn set_max_snapshots_per_memory(&mut self, max: u32) {
        self.max_snapshots_per_memory = max;
    }

    /// Returns a reference to the underlying storage backend.
    pub fn storage(&self) -> &dyn StorageBackend {
        self.storage.as_ref()
    }

    /// Returns a reference to the index manager.
    pub fn index_manager(&self) -> &IndexManager {
        &self.index_manager
    }

    /// Create a tenant-scoped storage wrapper that transparently prefixes
    /// all keys with the tenant identifier.
    ///
    /// For the "default" tenant, returns the raw storage backend unchanged
    /// so that existing (un-prefixed) data remains accessible and the
    /// IndexManager (which shares the raw backend) stays consistent until
    /// it gains its own per-tenant support.
    fn scoped_storage(&self, tenant: &TenantContext) -> Arc<dyn StorageBackend> {
        if tenant.tenant_id() == "default" {
            self.storage.clone()
        } else {
            Arc::new(TenantScopedStorage::new(
                self.storage.clone(),
                tenant.tenant_id(),
            ))
        }
    }

    /// Retrieve a memory by ID using a caller-supplied storage backend.
    /// Shared implementation for both default and tenant-scoped paths.
    fn get_from_storage(storage: &dyn StorageBackend, memory_id: &[u8]) -> Result<Memory> {
        if memory_id.len() != 16 {
            return Err(HebbsError::InvalidInput {
                operation: "get",
                message: format!(
                    "memory_id must be exactly 16 bytes, got {}",
                    memory_id.len()
                ),
            });
        }
        let key = keys::encode_memory_key(memory_id);
        let value = storage
            .get(ColumnFamilyName::Default, &key)?
            .ok_or_else(|| HebbsError::MemoryNotFound {
                memory_id: hex::encode(memory_id),
            })?;
        Memory::from_bytes(&value).map_err(|e| HebbsError::Serialization { message: e })
    }

    /// Write a new memory. Validates input, embeds, serializes, persists atomically.
    ///
    /// ## Phase 3 pipeline
    ///
    /// ```text
    /// validate → embed(content) → construct Memory → serialize
    ///     → IndexManager.prepare_insert()
    ///         → temporal CF entry (if entity_id present)
    ///         → vectors CF entry (HNSW node data)
    ///         → graph CF entries (if edges present)
    ///     → WriteBatch [default CF put + temporal + vectors + graph]
    ///     → execute WriteBatch atomically
    ///     → IndexManager.commit_insert() [update in-memory HNSW]
    ///     → return
    /// ```
    ///
    /// ## Latency: ~4.7ms p99 at 1M memories
    ///
    /// embed(3ms) + serialize(400ns) + index_prep(100ns) +
    /// WriteBatch(1ms) + HNSW insert(200µs) = ~4.7ms
    pub fn remember(&self, input: RememberInput) -> Result<Memory> {
        self.remember_for_tenant(&TenantContext::default(), input)
    }

    /// Like [`remember`](Self::remember) but returns timing instrumentation
    /// alongside the stored memory.
    pub fn remember_timed(&self, input: RememberInput) -> Result<RememberOutput> {
        self.remember_for_tenant_timed(&TenantContext::default(), input)
    }

    /// Tenant-aware version of [`remember`](Self::remember).
    pub fn remember_for_tenant(
        &self,
        tenant: &TenantContext,
        input: RememberInput,
    ) -> Result<Memory> {
        self.remember_for_tenant_timed(tenant, input)
            .map(|out| out.memory)
    }

    /// Tenant-aware version of [`remember_timed`](Self::remember_timed).
    pub fn remember_for_tenant_timed(
        &self,
        tenant: &TenantContext,
        input: RememberInput,
    ) -> Result<RememberOutput> {
        let storage = self.scoped_storage(tenant);

        let _validate_span = bench_span!("remember.validate");
        if input.content.is_empty() {
            return Err(HebbsError::InvalidInput {
                operation: "remember",
                message: "content must not be empty".to_string(),
            });
        }
        if input.content.len() > MAX_CONTENT_LENGTH {
            return Err(HebbsError::InvalidInput {
                operation: "remember",
                message: format!(
                    "content length {} exceeds maximum {}",
                    input.content.len(),
                    MAX_CONTENT_LENGTH
                ),
            });
        }

        let importance = input.importance.unwrap_or(DEFAULT_IMPORTANCE);
        if !(0.0..=1.0).contains(&importance) {
            return Err(HebbsError::InvalidInput {
                operation: "remember",
                message: format!("importance {} is out of range [0.0, 1.0]", importance),
            });
        }

        let context = input.context.unwrap_or_default();
        let context_bytes = Memory::serialize_context(&context)
            .map_err(|e| HebbsError::Serialization { message: e })?;
        if context_bytes.len() > MAX_CONTEXT_SIZE {
            return Err(HebbsError::InvalidInput {
                operation: "remember",
                message: format!(
                    "context size {} exceeds maximum {}",
                    context_bytes.len(),
                    MAX_CONTEXT_SIZE
                ),
            });
        }

        for edge in &input.edges {
            if edge.target_id == [0u8; 16] {
                return Err(HebbsError::InvalidInput {
                    operation: "remember",
                    message: "edge target_id must not be all zeros".to_string(),
                });
            }
        }
        bench_span_drop!(_validate_span);

        let _embed_span = bench_span!("remember.embed");
        let embed_start = Instant::now();
        let embedding = self.embedder.embed(&input.content)?;
        let embed_duration_us = embed_start.elapsed().as_micros() as u64;
        bench_span_drop!(_embed_span);

        // Associative embedding starts equal to content embedding.
        let assoc_embedding = embedding.clone();

        let _build_span = bench_span!("remember.build_memory");
        let ulid = self
            .ulid_gen
            .lock()
            .generate()
            .map_err(|e| HebbsError::Internal {
                operation: "remember",
                message: format!("ULID generation overflow: {}", e),
            })?;
        let now_us = now_microseconds();
        let memory_id_bytes = ulid.to_bytes();
        let mut memory_id = [0u8; 16];
        memory_id.copy_from_slice(&memory_id_bytes);

        let memory = Memory {
            memory_id: memory_id.to_vec(),
            content: input.content,
            importance,
            context_bytes,
            entity_id: input.entity_id,
            embedding: Some(embedding.clone()),
            created_at: now_us,
            updated_at: now_us,
            last_accessed_at: now_us,
            access_count: 0,
            decay_score: importance,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: Some(assoc_embedding.clone()),
        };

        let edge_inputs: Vec<EdgeInput> = input
            .edges
            .into_iter()
            .map(|e| EdgeInput {
                target_id: e.target_id,
                edge_type: e.edge_type,
                confidence: e.confidence.unwrap_or(1.0),
            })
            .collect();
        bench_span_drop!(_build_span);

        let _prep_span = bench_span!("remember.prepare_insert");
        let (index_ops, _temp_node) = self.index_manager.prepare_insert(
            &memory_id,
            &embedding,
            &assoc_embedding,
            memory.entity_id.as_deref(),
            now_us,
            &edge_inputs,
        )?;
        bench_span_drop!(_prep_span);

        let _ser_span = bench_span!("remember.serialize");
        let memory_value = memory.to_bytes();
        let memory_key = keys::encode_memory_key(&memory_id);

        let mut all_ops = Vec::with_capacity(1 + index_ops.len());
        all_ops.push(BatchOperation::Put {
            cf: ColumnFamilyName::Default,
            key: memory_key,
            value: memory_value,
        });
        all_ops.extend(index_ops);
        bench_span_drop!(_ser_span);

        let _write_span = bench_span!("remember.write_batch");
        storage.write_batch(&all_ops)?;
        bench_span_drop!(_write_span);

        // Main HNSW is global — similarity isolation is enforced by TenantScopedStorage at read
        // time (cross-tenant memory IDs fail to load and are silently skipped).
        // Assoc HNSW is per-tenant — causal/analogy traversal must not leak across tenants.
        let _hnsw_span = bench_span!("remember.hnsw_commit");
        self.index_manager
            .commit_insert(memory_id, embedding.clone())?;
        self.index_manager.commit_assoc_insert_for_tenant(
            tenant.tenant_id(),
            memory_id,
            assoc_embedding.clone(),
        )?;
        bench_span_drop!(_hnsw_span);

        // Hebbian update: for each edge, learn the type offset from source to target.
        let _hebbian_span = bench_span!("remember.hebbian_update");
        for edge in &edge_inputs {
            if let Ok(target_mem) = Self::get_from_storage(&*storage, &edge.target_id) {
                let target_assoc = target_mem
                    .associative_embedding
                    .as_deref()
                    .or(target_mem.embedding.as_deref());
                if let Some(ta) = target_assoc {
                    let _ = self.index_manager.update_type_offset_from_edge(
                        edge.edge_type,
                        &assoc_embedding,
                        ta,
                    );
                }
            }
        }
        bench_span_drop!(_hebbian_span);

        self.subscribe_registry.notify_new_write(memory_id);

        Ok(RememberOutput {
            memory,
            embed_duration_us,
        })
    }

    /// Check a newly remembered memory for contradictions against existing memories.
    ///
    /// Uses HNSW to find semantically similar memories and classifies pairs
    /// as contradiction, revision, or neutral. Detected contradictions are
    /// stored as bidirectional `CONTRADICTS` edges.
    ///
    /// Auto-selects LLM or heuristic mode based on `llm_provider`.
    pub fn check_contradictions(
        &self,
        memory_id: &[u8; 16],
        config: &contradict::ContradictionConfig,
        llm_provider: Option<&dyn hebbs_reflect::LlmProvider>,
    ) -> Result<Vec<contradict::Contradiction>> {
        self.check_contradictions_for_tenant(
            &TenantContext::default(),
            memory_id,
            config,
            llm_provider,
        )
    }

    /// Tenant-aware version of [`check_contradictions`](Self::check_contradictions).
    pub fn check_contradictions_for_tenant(
        &self,
        tenant: &TenantContext,
        memory_id: &[u8; 16],
        config: &contradict::ContradictionConfig,
        llm_provider: Option<&dyn hebbs_reflect::LlmProvider>,
    ) -> Result<Vec<contradict::Contradiction>> {
        let storage = self.scoped_storage(tenant);
        contradict::check_memory_contradictions(
            memory_id,
            storage,
            &self.index_manager,
            tenant,
            config,
            llm_provider,
        )
    }

    /// List all memories that contradict the given memory.
    pub fn contradictions(&self, memory_id: &[u8; 16]) -> Result<Vec<([u8; 16], f32)>> {
        let graph = hebbs_index::graph::GraphIndex::new(self.storage.clone());
        let edges = graph
            .outgoing_edges_of_type(memory_id, hebbs_index::graph::EdgeType::Contradicts)?;
        Ok(edges
            .into_iter()
            .map(|(target, meta)| (target, meta.confidence))
            .collect())
    }

    /// Retrieve a memory by its ULID bytes.
    ///
    /// Complexity: O(log n) with bloom filter shortcut for misses.
    pub fn get(&self, memory_id: &[u8]) -> Result<Memory> {
        self.get_for_tenant(&TenantContext::default(), memory_id)
    }

    /// Tenant-aware version of [`get`](Self::get).
    pub fn get_for_tenant(&self, tenant: &TenantContext, memory_id: &[u8]) -> Result<Memory> {
        let storage = self.scoped_storage(tenant);
        Self::get_from_storage(&*storage, memory_id)
    }

    /// Delete a memory and remove from all indexes atomically.
    ///
    /// ## Phase 3 pipeline
    ///
    /// ```text
    /// validate ID → get memory (need entity_id, created_at)
    ///     → IndexManager.prepare_delete()
    ///         → delete from default CF
    ///         → delete from temporal CF
    ///         → delete from vectors CF
    ///         → scan + delete all graph edges
    ///     → WriteBatch [all deletes]
    ///     → execute WriteBatch atomically
    ///     → IndexManager.commit_delete() [tombstone in-memory HNSW]
    ///     → return
    /// ```
    pub fn delete(&self, memory_id: &[u8]) -> Result<()> {
        self.delete_for_tenant(&TenantContext::default(), memory_id)
    }

    /// Tenant-aware version of [`delete`](Self::delete).
    pub fn delete_for_tenant(&self, tenant: &TenantContext, memory_id: &[u8]) -> Result<()> {
        let storage = self.scoped_storage(tenant);

        if memory_id.len() != 16 {
            return Err(HebbsError::InvalidInput {
                operation: "delete",
                message: format!(
                    "memory_id must be exactly 16 bytes, got {}",
                    memory_id.len()
                ),
            });
        }

        let memory = Self::get_from_storage(&*storage, memory_id)?;

        let mut id_arr = [0u8; 16];
        id_arr.copy_from_slice(memory_id);

        let ops = self.index_manager.prepare_delete(
            &id_arr,
            memory.entity_id.as_deref(),
            memory.created_at,
        )?;

        storage.write_batch(&ops)?;

        self.index_manager.commit_delete(&id_arr);

        Ok(())
    }

    /// HNSW top-K nearest neighbor search.
    ///
    /// Returns `(memory_id, distance)` pairs sorted by distance ascending.
    ///
    /// Complexity: O(log n * ef_search).
    pub fn search_similar(
        &self,
        query_embedding: &[f32],
        k: usize,
        ef_search: Option<usize>,
    ) -> Result<Vec<([u8; 16], f32)>> {
        self.search_similar_for_tenant(&TenantContext::default(), query_embedding, k, ef_search)
    }

    /// Tenant-aware version of [`search_similar`](Self::search_similar).
    pub fn search_similar_for_tenant(
        &self,
        _tenant: &TenantContext,
        query_embedding: &[f32],
        k: usize,
        ef_search: Option<usize>,
    ) -> Result<Vec<([u8; 16], f32)>> {
        Ok(self
            .index_manager
            .search_vector(query_embedding, k, ef_search)?)
    }

    /// Query memories by entity and time range.
    ///
    /// Returns `(memory_id, timestamp)` pairs in the requested order.
    ///
    /// Complexity: O(log n + k).
    pub fn query_temporal(
        &self,
        entity_id: &str,
        start_us: u64,
        end_us: u64,
        order: TemporalOrder,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, u64)>> {
        self.query_temporal_for_tenant(
            &TenantContext::default(),
            entity_id,
            start_us,
            end_us,
            order,
            limit,
        )
    }

    /// Tenant-aware version of [`query_temporal`](Self::query_temporal).
    pub fn query_temporal_for_tenant(
        &self,
        _tenant: &TenantContext,
        entity_id: &str,
        start_us: u64,
        end_us: u64,
        order: TemporalOrder,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, u64)>> {
        Ok(self
            .index_manager
            .query_temporal(entity_id, start_us, end_us, order, limit)?)
    }

    /// Graph bounded traversal from a seed memory.
    ///
    /// Returns connected memories up to `max_depth` hops.
    ///
    /// Complexity: O(branching_factor^max_depth).
    pub fn traverse_graph(
        &self,
        seed_id: &[u8; 16],
        edge_types: &[EdgeType],
        max_depth: usize,
        max_results: usize,
    ) -> Result<(Vec<TraversalEntry>, bool)> {
        self.traverse_graph_for_tenant(
            &TenantContext::default(),
            seed_id,
            edge_types,
            max_depth,
            max_results,
        )
    }

    /// Tenant-aware version of [`traverse_graph`](Self::traverse_graph).
    pub fn traverse_graph_for_tenant(
        &self,
        _tenant: &TenantContext,
        seed_id: &[u8; 16],
        edge_types: &[EdgeType],
        max_depth: usize,
        max_results: usize,
    ) -> Result<(Vec<TraversalEntry>, bool)> {
        Ok(self
            .index_manager
            .traverse(seed_id, edge_types, max_depth, max_results)?)
    }

    /// List memories by entity_id using the temporal index.
    ///
    /// Phase 3 upgrade: uses temporal index O(log n + k) instead of
    /// Phase 1's O(n) full scan with deserialization.
    pub fn list_by_entity(&self, entity_id: &str, limit: usize) -> Result<Vec<Memory>> {
        self.list_by_entity_for_tenant(&TenantContext::default(), entity_id, limit)
    }

    /// Tenant-aware version of [`list_by_entity`](Self::list_by_entity).
    pub fn list_by_entity_for_tenant(
        &self,
        tenant: &TenantContext,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let storage = self.scoped_storage(tenant);
        let temporal_results = self.index_manager.query_temporal(
            entity_id,
            0,
            u64::MAX,
            TemporalOrder::Chronological,
            limit,
        )?;

        let mut memories = Vec::with_capacity(temporal_results.len());
        for (memory_id, _ts) in temporal_results {
            match Self::get_from_storage(&*storage, &memory_id) {
                Ok(mem) => memories.push(mem),
                Err(HebbsError::MemoryNotFound { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(memories)
    }

    /// Return total number of memories stored.
    /// Uses HNSW node count as a fast approximation for indexed memories.
    pub fn count(&self) -> Result<usize> {
        self.count_for_tenant(&TenantContext::default())
    }

    /// Tenant-aware version of [`count`](Self::count).
    pub fn count_for_tenant(&self, tenant: &TenantContext) -> Result<usize> {
        let storage = self.scoped_storage(tenant);
        let all = storage.prefix_iterator(ColumnFamilyName::Default, &[])?;
        Ok(all.len())
    }

    // ═══════════════════════════════════════════════════════════════════
    //  Phase 4: Recall Engine
    // ═══════════════════════════════════════════════════════════════════

    /// Recall memories using one or more retrieval strategies.
    ///
    /// ## Phase 4 pipeline
    ///
    /// ```text
    /// validate → embed cue (if needed) → dispatch strategies (parallel)
    ///     → collect results → deduplicate by memory_id → composite score
    ///     → rank → truncate to top_k → reinforce → return
    /// ```
    ///
    /// ## Multi-strategy execution
    ///
    /// When multiple strategies are specified, they execute in parallel via
    /// `std::thread::scope`. The cue is embedded at most once on the main
    /// thread and shared across strategies that need it. Results are merged
    /// with deduplication by memory_id and ranked by composite score.
    ///
    /// ## Partial failure semantics
    ///
    /// If one strategy fails in multi-strategy recall, successful strategy
    /// results are still returned. The `strategy_errors` field in the output
    /// reports which strategies failed and why.
    pub fn recall(&self, input: RecallInput) -> Result<RecallOutput> {
        self.recall_for_tenant(&TenantContext::default(), input)
    }

    /// Tenant-aware version of [`recall`](Self::recall).
    pub fn recall_for_tenant(
        &self,
        tenant: &TenantContext,
        input: RecallInput,
    ) -> Result<RecallOutput> {
        let storage = self.scoped_storage(tenant);

        let _validate_span = bench_span!("recall.validate");
        if input.cue.is_empty() {
            return Err(HebbsError::InvalidInput {
                operation: "recall",
                message: "cue must not be empty".to_string(),
            });
        }
        if input.cue.len() > MAX_CONTENT_LENGTH {
            return Err(HebbsError::InvalidInput {
                operation: "recall",
                message: format!(
                    "cue length {} exceeds maximum {}",
                    input.cue.len(),
                    MAX_CONTENT_LENGTH
                ),
            });
        }
        if input.strategies.is_empty() {
            return Err(HebbsError::InvalidInput {
                operation: "recall",
                message: "at least one strategy must be specified".to_string(),
            });
        }

        let top_k = input.top_k.unwrap_or(10).min(MAX_TOP_K);
        let max_depth = input.max_depth.unwrap_or(5).min(MAX_TRAVERSAL_DEPTH);
        let weights = input.scoring_weights.unwrap_or_default();
        let now_us = now_microseconds();
        bench_span_drop!(_validate_span);

        let needs_embedding = input.strategies.iter().any(|s| {
            matches!(
                s,
                RecallStrategy::Similarity | RecallStrategy::Analogical | RecallStrategy::Causal
            )
        });

        let mut embed_duration_us: Option<u64> = None;
        let cue_embedding = if needs_embedding {
            let _embed_span = bench_span!("recall.embed");
            let embed_start = Instant::now();
            match self.embedder.embed(&input.cue) {
                Ok(emb) => {
                    embed_duration_us = Some(embed_start.elapsed().as_micros() as u64);
                    Some(emb)
                }
                Err(e) => {
                    embed_duration_us = Some(embed_start.elapsed().as_micros() as u64);
                    let non_embedding_strategies: Vec<_> = input
                        .strategies
                        .iter()
                        .filter(|s| matches!(s, RecallStrategy::Temporal))
                        .cloned()
                        .collect();

                    if non_embedding_strategies.is_empty() {
                        return Err(HebbsError::Embedding(e));
                    }
                    None
                }
            }
        } else {
            None
        };

        let ctx = StrategyContext {
            cue: &input.cue,
            cue_embedding: cue_embedding.as_deref(),
            entity_id: input.entity_id.as_deref(),
            time_range: input.time_range,
            edge_types: input.edge_types.as_deref(),
            max_depth,
            top_k,
            ef_search: input.ef_search,
            cue_context: input.cue_context.as_ref(),
            tenant_id: tenant.tenant_id(),
            causal_direction: input.causal_direction.unwrap_or_default(),
            analogy_a_id: input.analogy_a_id,
            analogy_b_id: input.analogy_b_id,
            seed_memory_id: input.seed_memory_id,
            analogical_alpha: input.analogical_alpha,
        };

        let _exec_span = bench_span!("recall.execute_strategies");
        let outcomes = if input.strategies.len() == 1 {
            vec![self.execute_strategy(&*storage, &input.strategies[0], &ctx)]
        } else {
            self.execute_strategies_parallel(&*storage, &input.strategies, &ctx)
        };
        bench_span_drop!(_exec_span);

        let mut strategy_errors = Vec::new();
        let mut truncated = HashMap::new();
        let mut all_results: Vec<StrategyResult> = Vec::new();

        for outcome in outcomes {
            match outcome {
                StrategyOutcome::Ok(results) => {
                    all_results.extend(results);
                }
                StrategyOutcome::Err(strategy, message) => {
                    strategy_errors.push(StrategyError { strategy, message });
                }
            }
        }

        for strategy in &input.strategies {
            let count = all_results
                .iter()
                .filter(|r| r.detail.strategy() == *strategy)
                .count();
            truncated.insert(strategy.clone(), count >= top_k);
        }

        let _merge_span = bench_span!("recall.merge_and_rank");
        let merged = self.merge_and_rank(all_results, &weights, now_us, top_k);
        bench_span_drop!(_merge_span);

        let _reinforce_span = bench_span!("recall.reinforce");
        Self::reinforce_memories(&*storage, &merged, now_us);
        bench_span_drop!(_reinforce_span);

        Ok(RecallOutput {
            results: merged,
            strategy_errors,
            truncated,
            embed_duration_us,
        })
    }

    /// Pre-load relevant context for an agent turn.
    ///
    /// Combines temporal recency (recent entity history) with similarity
    /// relevance (knowledge matching the current context).
    ///
    /// ## Pipeline
    ///
    /// 1. Temporal: recent memories for the entity within recency_window.
    /// 2. Similarity: embed similarity_cue (or synthetic cue), HNSW search.
    /// 3. Merge, deduplicate, score.
    /// 4. Reinforce all returned memories.
    /// 5. Return: temporal results in chronological order, then similarity by relevance.
    pub fn prime(&self, input: PrimeInput) -> Result<PrimeOutput> {
        self.prime_for_tenant(&TenantContext::default(), input)
    }

    /// Tenant-aware version of [`prime`](Self::prime).
    pub fn prime_for_tenant(
        &self,
        tenant: &TenantContext,
        input: PrimeInput,
    ) -> Result<PrimeOutput> {
        let storage = self.scoped_storage(tenant);

        if input.entity_id.is_empty() {
            return Err(HebbsError::InvalidInput {
                operation: "prime",
                message: "entity_id must not be empty".to_string(),
            });
        }

        let max_memories = input.max_memories.unwrap_or(20).min(MAX_PRIME_MEMORIES);
        let recency_window = input
            .recency_window_us
            .unwrap_or(DEFAULT_PRIME_RECENCY_WINDOW_US);
        let weights = input.scoring_weights.unwrap_or_default();
        let now_us = now_microseconds();
        let temporal_limit = max_memories / 2;
        let similarity_limit = max_memories - temporal_limit;

        let start_us = now_us.saturating_sub(recency_window);
        let temporal_raw = self.index_manager.query_temporal(
            &input.entity_id,
            start_us,
            now_us,
            TemporalOrder::ReverseChronological,
            temporal_limit,
        )?;

        let mut temporal_memories: Vec<(Memory, f32, u64, usize)> =
            Vec::with_capacity(temporal_raw.len());
        let temporal_count_raw = temporal_raw.len();
        for (i, (memory_id, timestamp)) in temporal_raw.iter().enumerate() {
            match Self::get_from_storage(&*storage, memory_id) {
                Ok(mem) => {
                    let relevance = if temporal_count_raw > 1 {
                        1.0 - (i as f32 / temporal_count_raw as f32)
                    } else {
                        1.0
                    };
                    temporal_memories.push((mem, relevance, *timestamp, i));
                }
                Err(HebbsError::MemoryNotFound { .. }) => continue,
                Err(e) => return Err(e),
            }
        }

        let similarity_cue = if let Some(ref cue) = input.similarity_cue {
            cue.clone()
        } else {
            self.build_synthetic_cue(&input.entity_id, input.context.as_ref())
        };

        let mut similarity_memories: Vec<(Memory, f32)> = Vec::new();

        if !similarity_cue.is_empty() {
            let cue_embedding = self.embedder.embed(&similarity_cue)?;

            // Entity-scoped similarity: query the temporal index for ALL
            // entity memory IDs (full time range), load their embeddings,
            // and rank by cosine similarity with the cue.  This avoids the
            // global HNSW + entity post-filter which returns 0 results
            // when the entity is a small fraction of total memories.
            let entity_ids = self.index_manager.query_temporal(
                &input.entity_id,
                0,
                now_us,
                TemporalOrder::ReverseChronological,
                PRIME_ENTITY_SCAN_LIMIT,
            )?;

            // Score each entity memory by cosine similarity with the cue.
            // O(n * d) where n ≤ PRIME_ENTITY_SCAN_LIMIT, d = embedding dim.
            let mut scored: Vec<(Memory, f32)> =
                Vec::with_capacity(entity_ids.len().min(similarity_limit));

            for (memory_id, _timestamp) in &entity_ids {
                match Self::get_from_storage(&*storage, memory_id) {
                    Ok(mem) => {
                        if let Some(ref emb) = mem.embedding {
                            let relevance = cosine_similarity(&cue_embedding, emb).max(0.0);
                            scored.push((mem, relevance));
                        }
                    }
                    Err(HebbsError::MemoryNotFound { .. }) => continue,
                    Err(e) => return Err(e),
                }
            }

            // Sort by relevance descending, take top similarity_limit.
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(similarity_limit);
            similarity_memories = scored;
        }

        let mut seen = HashSet::new();
        let mut final_results: Vec<RecallResult> = Vec::new();

        let temporal_count;
        {
            let mut temporal_scored: Vec<RecallResult> = Vec::new();
            for (mem, relevance, timestamp, rank) in temporal_memories.into_iter().rev() {
                let id_key = mem.memory_id.clone();
                if !seen.insert(id_key) {
                    continue;
                }
                let score = compute_composite_score(relevance, &mem, &weights, now_us);
                temporal_scored.push(RecallResult {
                    memory: mem,
                    score,
                    strategy_details: vec![StrategyDetail::Temporal {
                        timestamp,
                        rank,
                        relevance,
                    }],
                });
            }
            temporal_count = temporal_scored.len();
            final_results.extend(temporal_scored);
        }

        let similarity_count;
        {
            let mut sim_scored: Vec<RecallResult> = Vec::new();
            for (mem, relevance) in similarity_memories {
                let id_key = mem.memory_id.clone();
                if !seen.insert(id_key) {
                    continue;
                }
                let score = compute_composite_score(relevance, &mem, &weights, now_us);
                sim_scored.push(RecallResult {
                    memory: mem,
                    score,
                    strategy_details: vec![StrategyDetail::Similarity {
                        distance: 1.0 - relevance,
                        relevance,
                    }],
                });
            }
            sim_scored.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            similarity_count = sim_scored.len();
            final_results.extend(sim_scored);
        }

        Self::reinforce_memories(&*storage, &final_results, now_us);

        Ok(PrimeOutput {
            results: final_results,
            temporal_count,
            similarity_count,
        })
    }

    // ═══════════════════════════════════════════════════════════════════
    //  Phase 5: Write Path Completion (Revise, Forget, Decay)
    // ═══════════════════════════════════════════════════════════════════

    /// Revise a memory: update content, re-embed, re-index, preserve lineage.
    ///
    /// ## Model B: In-Place Update with Predecessor Snapshot
    ///
    /// The memory_id is stable — external references, graph edges, and causal
    /// chains all see the updated content at the same address. The old state is
    /// captured in a predecessor snapshot (stored in default CF, not indexed).
    ///
    /// ## Pipeline
    ///
    /// ```text
    /// validate → load existing memory → verify not a snapshot
    ///     → embed new content (if content changed)
    ///     → create predecessor snapshot (new ULID, copy old fields)
    ///     → apply updates to memory in-place
    ///     → IndexManager.prepare_update()
    ///     → WriteBatch:
    ///         [1] Put snapshot into default CF
    ///         [2] Put updated memory into default CF (same key)
    ///         [3] Delete/Put temporal entries (if entity_id changed)
    ///         [4] Delete/Put vectors entries (re-indexed)
    ///         [5] Put RevisedFrom edge (forward + reverse)
    ///     → execute WriteBatch atomically
    ///     → commit HNSW: tombstone old, insert new (same memory_id)
    /// ```
    pub fn revise(&self, input: ReviseInput) -> Result<Memory> {
        self.revise_for_tenant(&TenantContext::default(), input)
    }

    /// Tenant-aware version of [`revise`](Self::revise).
    pub fn revise_for_tenant(&self, tenant: &TenantContext, input: ReviseInput) -> Result<Memory> {
        let storage = self.scoped_storage(tenant);

        if input.memory_id.len() != 16 {
            return Err(HebbsError::InvalidInput {
                operation: "revise",
                message: format!(
                    "memory_id must be exactly 16 bytes, got {}",
                    input.memory_id.len()
                ),
            });
        }

        if input.is_noop() {
            return Err(HebbsError::InvalidInput {
                operation: "revise",
                message: "at least one revisable field must be provided".to_string(),
            });
        }

        if let Some(ref content) = input.content {
            if content.is_empty() {
                return Err(HebbsError::InvalidInput {
                    operation: "revise",
                    message: "content must not be empty".to_string(),
                });
            }
            if content.len() > MAX_CONTENT_LENGTH {
                return Err(HebbsError::InvalidInput {
                    operation: "revise",
                    message: format!(
                        "content length {} exceeds maximum {}",
                        content.len(),
                        MAX_CONTENT_LENGTH
                    ),
                });
            }
        }

        if let Some(imp) = input.importance {
            if !(0.0..=1.0).contains(&imp) {
                return Err(HebbsError::InvalidInput {
                    operation: "revise",
                    message: format!("importance {} is out of range [0.0, 1.0]", imp),
                });
            }
        }

        for edge in &input.edges {
            if edge.target_id == [0u8; 16] {
                return Err(HebbsError::InvalidInput {
                    operation: "revise",
                    message: "edge target_id must not be all zeros".to_string(),
                });
            }
        }

        let existing = Self::get_from_storage(&*storage, &input.memory_id)?;

        if existing.kind == MemoryKind::Revision {
            // Check if this is a predecessor snapshot (snapshots have kind=Episode
            // from the original, but we need a different mechanism to detect snapshots).
            // Snapshots are stored with is_snapshot metadata — for now, we check
            // if there are any incoming RevisedFrom edges pointing to this memory,
            // which would make it a snapshot.
            // Actually, per the architecture doc: snapshots are NOT indexed in HNSW.
            // So we just check if this memory has no embedding in the HNSW index.
            // For simplicity and correctness, we allow revising Revision-kind memories
            // since they are the primary (active) versions.
        }

        let now_us = now_microseconds();
        let mut memory_id_arr = [0u8; 16];
        memory_id_arr.copy_from_slice(&input.memory_id);

        // --- Create predecessor snapshot ---
        let snapshot_ulid = self
            .ulid_gen
            .lock()
            .generate()
            .map_err(|e| HebbsError::Internal {
                operation: "revise",
                message: format!("ULID generation overflow for snapshot: {}", e),
            })?;
        let snapshot_id = snapshot_ulid.to_bytes();

        let snapshot = Memory {
            memory_id: snapshot_id.to_vec(),
            content: existing.content.clone(),
            importance: existing.importance,
            context_bytes: existing.context_bytes.clone(),
            entity_id: existing.entity_id.clone(),
            embedding: existing.embedding.clone(),
            created_at: existing.created_at,
            updated_at: existing.updated_at,
            last_accessed_at: existing.last_accessed_at,
            access_count: existing.access_count,
            decay_score: existing.decay_score,
            kind: existing.kind,
            device_id: existing.device_id.clone(),
            logical_clock: existing.logical_clock,
            associative_embedding: existing.associative_embedding.clone(),
        };

        // --- Apply updates to create the revised memory ---
        let new_content = input.content.unwrap_or_else(|| existing.content.clone());
        let content_changed = new_content != existing.content;

        let new_embedding = if content_changed {
            self.embedder.embed(&new_content)?
        } else {
            existing
                .embedding
                .clone()
                .ok_or_else(|| HebbsError::Internal {
                    operation: "revise",
                    message: "existing memory has no embedding".to_string(),
                })?
        };

        let new_importance = input.importance.unwrap_or(existing.importance);

        let new_context_bytes = if let Some(ref new_ctx) = input.context {
            match input.context_mode {
                ContextMode::Replace => Memory::serialize_context(new_ctx)
                    .map_err(|e| HebbsError::Serialization { message: e })?,
                ContextMode::Merge => {
                    let mut merged = existing
                        .context()
                        .map_err(|e| HebbsError::Serialization { message: e })?;
                    for (k, v) in new_ctx {
                        merged.insert(k.clone(), v.clone());
                    }
                    Memory::serialize_context(&merged)
                        .map_err(|e| HebbsError::Serialization { message: e })?
                }
            }
        } else {
            existing.context_bytes.clone()
        };

        if new_context_bytes.len() > MAX_CONTEXT_SIZE {
            return Err(HebbsError::InvalidInput {
                operation: "revise",
                message: format!(
                    "context size {} exceeds maximum {}",
                    new_context_bytes.len(),
                    MAX_CONTEXT_SIZE
                ),
            });
        }

        let new_entity_id = match input.entity_id {
            Some(eid) => eid,
            None => existing.entity_id.clone(),
        };

        let revised_memory = Memory {
            memory_id: input.memory_id.clone(),
            content: new_content,
            importance: new_importance,
            context_bytes: new_context_bytes,
            entity_id: new_entity_id.clone(),
            embedding: Some(new_embedding.clone()),
            created_at: existing.created_at,
            updated_at: now_us,
            last_accessed_at: existing.last_accessed_at,
            access_count: existing.access_count,
            decay_score: new_importance,
            kind: MemoryKind::Revision,
            device_id: existing.device_id.clone(),
            logical_clock: existing.logical_clock.saturating_add(1),
            associative_embedding: existing.associative_embedding.clone(),
        };

        // --- Prepare index operations ---
        let mut revision_edges: Vec<EdgeInput> = vec![EdgeInput {
            target_id: {
                let mut id = [0u8; 16];
                id.copy_from_slice(&snapshot_id);
                id
            },
            edge_type: EdgeType::RevisedFrom,
            confidence: 1.0,
        }];

        for edge in input.edges {
            revision_edges.push(EdgeInput {
                target_id: edge.target_id,
                edge_type: edge.edge_type,
                confidence: edge.confidence.unwrap_or(1.0),
            });
        }

        let (index_ops, _temp_node) = self.index_manager.prepare_update(
            &memory_id_arr,
            existing.entity_id.as_deref(),
            existing.created_at,
            &new_embedding,
            new_entity_id.as_deref(),
            existing.created_at,
            &revision_edges,
        )?;

        // --- Build atomic WriteBatch ---
        let snapshot_key = keys::encode_memory_key(&snapshot_id);
        let memory_key = keys::encode_memory_key(&input.memory_id);

        let mut all_ops = Vec::with_capacity(2 + index_ops.len());

        // [1] Put snapshot into default CF
        all_ops.push(BatchOperation::Put {
            cf: ColumnFamilyName::Default,
            key: snapshot_key,
            value: snapshot.to_bytes(),
        });

        // [2] Put updated memory into default CF (same key, new value)
        all_ops.push(BatchOperation::Put {
            cf: ColumnFamilyName::Default,
            key: memory_key,
            value: revised_memory.to_bytes(),
        });

        // [3..] All index operations (temporal delete/put, vector delete/put, graph edges)
        all_ops.extend(index_ops);

        // --- Prune oldest snapshots if exceeding retention limit ---
        let mut pruned_snapshot_ids: Vec<[u8; 16]> = Vec::new();
        if self.max_snapshots_per_memory > 0 {
            let existing_snapshots = self.find_revision_snapshots(&*storage, &memory_id_arr)?;
            let total = existing_snapshots.len() + 1; // +1 for the new snapshot
            if total > self.max_snapshots_per_memory as usize {
                let excess = total - self.max_snapshots_per_memory as usize;
                let mut sorted = existing_snapshots;
                sorted.sort(); // ULID byte order == chronological order
                for snap_id_vec in sorted.into_iter().take(excess) {
                    let mut snap_arr = [0u8; 16];
                    snap_arr.copy_from_slice(&snap_id_vec);
                    // entity_id=None skips temporal delete (snapshots aren't indexed there)
                    let snap_delete_ops = self.index_manager.prepare_delete(&snap_arr, None, 0)?;
                    all_ops.extend(snap_delete_ops);
                    pruned_snapshot_ids.push(snap_arr);
                }
            }
        }

        storage.write_batch(&all_ops)?;

        self.index_manager.commit_delete(&memory_id_arr);
        self.index_manager
            .commit_insert(memory_id_arr, new_embedding)?;

        for snap_arr in &pruned_snapshot_ids {
            self.index_manager.commit_delete(snap_arr);
        }

        reflect::mark_insights_stale_for_source(&storage, &self.index_manager, &memory_id_arr);

        Ok(revised_memory)
    }

    /// Forget memories matching the given criteria.
    ///
    /// Removes matching memories from all five column families and in-memory HNSW.
    /// Creates tombstone records in the meta CF for audit trail.
    ///
    /// ## Batch processing
    ///
    /// Processes candidates in bounded batches (configurable via `ForgetConfig`).
    /// Returns a truncation flag if more candidates remain.
    ///
    /// ## Cascade policy
    ///
    /// When a memory is forgotten, its predecessor snapshots (linked via RevisedFrom
    /// edges) are also deleted if `cascade_snapshots` is true. Snapshots that have
    /// non-RevisedFrom incoming edges are preserved.
    pub fn forget(&self, criteria: ForgetCriteria) -> Result<ForgetOutput> {
        self.forget_for_tenant(&TenantContext::default(), criteria)
    }

    /// Tenant-aware version of [`forget`](Self::forget).
    pub fn forget_for_tenant(
        &self,
        tenant: &TenantContext,
        criteria: ForgetCriteria,
    ) -> Result<ForgetOutput> {
        let storage = self.scoped_storage(tenant);

        if criteria.is_empty() {
            return Err(HebbsError::InvalidInput {
                operation: "forget",
                message: "at least one criterion must be specified".to_string(),
            });
        }

        let max_batch = self.forget_config.max_batch_size.min(MAX_FORGET_BATCH_SIZE);
        let now_us = now_microseconds();

        let candidates = self.resolve_forget_candidates(&*storage, &criteria, max_batch)?;
        let truncated = candidates.len() >= max_batch;

        if candidates.is_empty() {
            return Ok(ForgetOutput {
                forgotten_count: 0,
                cascade_count: 0,
                truncated: false,
                tombstone_count: 0,
            });
        }

        let mut forgotten_count = 0usize;
        let mut cascade_count = 0usize;
        let mut tombstone_count = 0usize;
        let mut all_ops = Vec::new();
        let mut ids_to_tombstone_hnsw: Vec<[u8; 16]> = Vec::new();

        let criteria_desc = self.describe_criteria(&criteria);

        for memory in &candidates {
            let mut mem_id = [0u8; 16];
            if memory.memory_id.len() != 16 {
                continue;
            }
            mem_id.copy_from_slice(&memory.memory_id);

            let delete_ops = self.index_manager.prepare_delete(
                &mem_id,
                memory.entity_id.as_deref(),
                memory.created_at,
            )?;
            all_ops.extend(delete_ops);
            ids_to_tombstone_hnsw.push(mem_id);

            let mut mem_cascade_count = 0u32;
            if self.forget_config.cascade_snapshots {
                let snapshot_ids = self.find_revision_snapshots(&*storage, &mem_id)?;
                for snap_id in &snapshot_ids {
                    if let Ok(snap) = Self::get_from_storage(&*storage, snap_id) {
                        let mut snap_arr = [0u8; 16];
                        snap_arr.copy_from_slice(snap_id);

                        let snap_ops = self.index_manager.prepare_delete(
                            &snap_arr,
                            snap.entity_id.as_deref(),
                            snap.created_at,
                        )?;
                        all_ops.extend(snap_ops);
                        ids_to_tombstone_hnsw.push(snap_arr);
                        mem_cascade_count += 1;
                        cascade_count += 1;
                    }
                }
            }

            let content_hash = Sha256::digest(memory.content.as_bytes()).to_vec();
            let tombstone = Tombstone {
                memory_id: memory.memory_id.clone(),
                entity_id: memory.entity_id.clone(),
                forget_timestamp_us: now_us,
                criteria_description: criteria_desc.clone(),
                cascade_count: mem_cascade_count,
                content_hash,
            };

            let tombstone_key = encode_tombstone_key(now_us, &memory.memory_id);
            all_ops.push(BatchOperation::Put {
                cf: ColumnFamilyName::Meta,
                key: tombstone_key,
                value: tombstone.to_bytes(),
            });
            tombstone_count += 1;
            forgotten_count += 1;
        }

        if !all_ops.is_empty() {
            storage.write_batch(&all_ops)?;
        }

        for id in &ids_to_tombstone_hnsw {
            self.index_manager.commit_delete(id);
        }

        for id in &ids_to_tombstone_hnsw {
            reflect::mark_insights_stale_for_source(&storage, &self.index_manager, id);
        }

        if self.index_manager.hnsw_needs_cleanup() {
            self.index_manager.hnsw_cleanup();
        }

        if self.forget_config.trigger_compaction {
            let _ = storage.compact(ColumnFamilyName::Default);
            let _ = storage.compact(ColumnFamilyName::Temporal);
            let _ = storage.compact(ColumnFamilyName::Vectors);
            let _ = storage.compact(ColumnFamilyName::Graph);
        }

        Ok(ForgetOutput {
            forgotten_count,
            cascade_count,
            truncated,
            tombstone_count,
        })
    }

    /// Start the background decay engine.
    ///
    /// Spawns a dedicated OS thread that periodically sweeps all memories,
    /// recalculating decay scores and identifying auto-forget candidates.
    /// The thread starts paused and must be resumed via the returned handle
    /// or by calling `resume_decay()`.
    pub fn start_decay(&self, config: DecayConfig) {
        let config = config.validated();
        let handle = spawn_decay_worker(self.storage.clone(), config);
        handle.resume();
        let mut guard = self.decay_handle.lock();
        *guard = Some(handle);
    }

    /// Pause the decay engine. It finishes its current batch then idles.
    pub fn pause_decay(&self) {
        let guard = self.decay_handle.lock();
        if let Some(ref handle) = *guard {
            handle.pause();
        }
    }

    /// Resume the decay engine.
    pub fn resume_decay(&self) {
        let guard = self.decay_handle.lock();
        if let Some(ref handle) = *guard {
            handle.resume();
        }
    }

    /// Stop the decay engine and join the worker thread.
    pub fn stop_decay(&self) {
        let mut guard = self.decay_handle.lock();
        if let Some(ref mut handle) = *guard {
            handle.shutdown();
        }
        *guard = None;
    }

    /// Reconfigure the decay engine. Changes take effect before the next batch.
    pub fn reconfigure_decay(&self, config: DecayConfig) {
        let guard = self.decay_handle.lock();
        if let Some(ref handle) = *guard {
            handle.reconfigure(config);
        }
    }

    /// Execute auto-forget: read decay candidates and forget them.
    ///
    /// This is the policy execution stage. In Phase 5, the default policy
    /// is "forget all candidates below the threshold." Phase 7 may provide
    /// more sophisticated policies.
    pub fn auto_forget(&self) -> Result<ForgetOutput> {
        self.auto_forget_for_tenant(&TenantContext::default())
    }

    /// Tenant-aware version of [`auto_forget`](Self::auto_forget).
    pub fn auto_forget_for_tenant(&self, tenant: &TenantContext) -> Result<ForgetOutput> {
        let storage = self.scoped_storage(tenant);
        let candidates = read_auto_forget_candidates(storage.as_ref());
        if candidates.is_empty() {
            return Ok(ForgetOutput {
                forgotten_count: 0,
                cascade_count: 0,
                truncated: false,
                tombstone_count: 0,
            });
        }

        let result = self.forget_for_tenant(tenant, ForgetCriteria::by_ids(candidates))?;

        clear_auto_forget_candidates(storage.as_ref());

        Ok(result)
    }

    /// Garbage-collect tombstones older than the configured TTL.
    pub fn gc_tombstones(&self) -> Result<usize> {
        self.gc_tombstones_for_tenant(&TenantContext::default())
    }

    /// Tenant-aware version of [`gc_tombstones`](Self::gc_tombstones).
    pub fn gc_tombstones_for_tenant(&self, tenant: &TenantContext) -> Result<usize> {
        let storage = self.scoped_storage(tenant);
        let now_us = now_microseconds();
        let ttl = self.forget_config.tombstone_ttl_us;
        let cutoff = now_us.saturating_sub(ttl);

        let prefix = crate::forget::tombstone_prefix();
        let entries = storage.prefix_iterator(ColumnFamilyName::Meta, &prefix)?;

        let mut ops = Vec::new();
        for (key, _value) in entries {
            if key.len() < prefix.len() + 8 {
                continue;
            }
            let ts_bytes = &key[prefix.len()..prefix.len() + 8];
            if ts_bytes.len() == 8 {
                let ts = u64::from_be_bytes([
                    ts_bytes[0],
                    ts_bytes[1],
                    ts_bytes[2],
                    ts_bytes[3],
                    ts_bytes[4],
                    ts_bytes[5],
                    ts_bytes[6],
                    ts_bytes[7],
                ]);
                if ts < cutoff {
                    ops.push(BatchOperation::Delete {
                        cf: ColumnFamilyName::Meta,
                        key,
                    });
                }
            }
        }

        let count = ops.len();
        if !ops.is_empty() {
            storage.write_batch(&ops)?;
        }

        Ok(count)
    }

    // ═══════════════════════════════════════════════════════════════════
    //  Phase 6: Subscribe
    // ═══════════════════════════════════════════════════════════════════

    /// Open a subscription that streams relevant memories in real-time.
    ///
    /// The subscription accepts text chunks via `SubscriptionHandle::feed()`
    /// and pushes relevant memories back via `try_recv()` / `recv_timeout()`
    /// when confidence exceeds the configured threshold.
    ///
    /// Each subscription spawns a dedicated worker thread with a hierarchical
    /// filtering pipeline (bloom → coarse centroid → fine HNSW).
    ///
    /// ## Latency budget: 8ms p99 per chunk
    ///
    /// bloom(0.1ms) + embed(3ms) + coarse(0.01ms) + HNSW(3ms) + deser(0.5ms) + push(0.1ms)
    pub fn subscribe(&self, config: SubscribeConfig) -> Result<SubscriptionHandle> {
        self.subscribe_for_tenant(&TenantContext::default(), config)
    }

    /// Tenant-aware version of [`subscribe`](Self::subscribe).
    pub fn subscribe_for_tenant(
        &self,
        tenant: &TenantContext,
        config: SubscribeConfig,
    ) -> Result<SubscriptionHandle> {
        let storage = self.scoped_storage(tenant);
        create_subscription(
            config,
            storage,
            self.embedder.clone(),
            self.index_manager.clone(),
            self.subscribe_registry.clone(),
        )
    }

    /// Returns the number of active subscriptions.
    pub fn active_subscriptions(&self) -> usize {
        self.subscribe_registry.active_count()
    }

    /// Query outgoing graph edges from a memory.
    ///
    /// Returns `(edge_type, target_id, metadata)` triples.
    pub fn outgoing_edges(
        &self,
        memory_id: &[u8; 16],
    ) -> Result<Vec<(EdgeType, [u8; 16], hebbs_index::EdgeMetadata)>> {
        self.outgoing_edges_for_tenant(&TenantContext::default(), memory_id)
    }

    /// Tenant-aware version of [`outgoing_edges`](Self::outgoing_edges).
    pub fn outgoing_edges_for_tenant(
        &self,
        _tenant: &TenantContext,
        memory_id: &[u8; 16],
    ) -> Result<Vec<(EdgeType, [u8; 16], hebbs_index::EdgeMetadata)>> {
        self.index_manager
            .outgoing_edges(memory_id)
            .map_err(HebbsError::Index)
    }

    /// Query incoming graph edges to a memory.
    ///
    /// Returns `(edge_type, source_id, metadata)` triples.
    pub fn incoming_edges(
        &self,
        memory_id: &[u8; 16],
    ) -> Result<Vec<(EdgeType, [u8; 16], hebbs_index::EdgeMetadata)>> {
        self.incoming_edges_for_tenant(&TenantContext::default(), memory_id)
    }

    /// Tenant-aware version of [`incoming_edges`](Self::incoming_edges).
    pub fn incoming_edges_for_tenant(
        &self,
        _tenant: &TenantContext,
        memory_id: &[u8; 16],
    ) -> Result<Vec<(EdgeType, [u8; 16], hebbs_index::EdgeMetadata)>> {
        self.index_manager
            .incoming_edges(memory_id)
            .map_err(HebbsError::Index)
    }

    // ─── Phase 7: Reflection Pipeline ─────────────────────────────

    /// Run the reflection pipeline synchronously.
    ///
    /// Scopes episode memories, clusters them, proposes insights via the
    /// configured LLM providers, validates, and consolidates accepted
    /// insights as `MemoryKind::Insight` with `InsightFrom` graph edges.
    ///
    /// Uses blocking LLM calls — safe to call from the main thread or
    /// a dedicated background thread.
    pub fn reflect(
        &self,
        scope: ReflectScope,
        config: &ReflectConfig,
        proposal_provider: &dyn hebbs_reflect::LlmProvider,
        validation_provider: &dyn hebbs_reflect::LlmProvider,
    ) -> Result<ReflectRunOutput> {
        self.reflect_for_tenant(
            &TenantContext::default(),
            scope,
            config,
            proposal_provider,
            validation_provider,
        )
    }

    /// Tenant-aware version of [`reflect`](Self::reflect).
    pub fn reflect_for_tenant(
        &self,
        tenant: &TenantContext,
        scope: ReflectScope,
        config: &ReflectConfig,
        proposal_provider: &dyn hebbs_reflect::LlmProvider,
        validation_provider: &dyn hebbs_reflect::LlmProvider,
    ) -> Result<ReflectRunOutput> {
        let storage = self.scoped_storage(tenant);
        reflect::run_reflect_shared(
            &storage,
            &self.embedder,
            &self.index_manager,
            &self.subscribe_registry,
            config,
            &scope,
            proposal_provider,
            validation_provider,
        )
    }

    /// Query stored insights.
    ///
    /// Returns `MemoryKind::Insight` memories sorted by importance
    /// (descending), filtered by the provided criteria.
    pub fn insights(&self, filter: InsightsFilter) -> Result<Vec<Memory>> {
        self.insights_for_tenant(&TenantContext::default(), filter)
    }

    /// Tenant-aware version of [`insights`](Self::insights).
    pub fn insights_for_tenant(
        &self,
        tenant: &TenantContext,
        filter: InsightsFilter,
    ) -> Result<Vec<Memory>> {
        let storage = self.scoped_storage(tenant);
        reflect::query_insights(&storage, &filter)
    }

    /// Prepare reflection data for agent-driven two-step reflect.
    ///
    /// Gathers memories, clusters them, and builds LLM prompts without
    /// calling any LLM. Returns cluster data, prompts, and a session ID
    /// that must be passed to [`reflect_commit_for_tenant`](Self::reflect_commit_for_tenant).
    pub fn reflect_prepare_for_tenant(
        &self,
        tenant: &TenantContext,
        scope: ReflectScope,
        config: &ReflectConfig,
    ) -> Result<reflect::ReflectPrepareResult> {
        let storage = self.scoped_storage(tenant);
        reflect::reflect_prepare_shared(
            &storage,
            &self.embedder,
            config,
            &scope,
            &self.reflect_session_store,
        )
    }

    /// Commit agent-produced insights from a previous `reflect_prepare` call.
    ///
    /// Validates the session and source memory IDs, then embeds, indexes,
    /// and stores each insight. No LLM is called.
    pub fn reflect_commit_for_tenant(
        &self,
        _tenant: &TenantContext,
        session_id: &str,
        insights: Vec<hebbs_reflect::ProducedInsight>,
    ) -> Result<reflect::ReflectCommitResult> {
        reflect::reflect_commit_shared(
            &self.storage,
            &self.embedder,
            &self.index_manager,
            &self.subscribe_registry,
            &self.reflect_session_store,
            session_id,
            insights,
        )
    }

    /// Start the background reflect monitor with the given configuration.
    ///
    /// The monitor checks triggers on a configurable interval and runs
    /// the reflection pipeline autonomously when a trigger fires.
    pub fn start_reflect(&self, config: ReflectConfig) {
        let config = config.validated();
        let handle = spawn_reflect_worker(
            self.storage.clone(),
            self.embedder.clone(),
            self.index_manager.clone(),
            self.subscribe_registry.clone(),
            config,
        );
        handle.resume();
        let mut guard = self.reflect_handle.lock();
        *guard = Some(handle);
    }

    /// Pause the background reflect monitor.
    pub fn pause_reflect(&self) {
        let guard = self.reflect_handle.lock();
        if let Some(ref handle) = *guard {
            handle.pause();
        }
    }

    /// Resume the background reflect monitor.
    pub fn resume_reflect(&self) {
        let guard = self.reflect_handle.lock();
        if let Some(ref handle) = *guard {
            handle.resume();
        }
    }

    /// Stop and join the background reflect monitor.
    pub fn stop_reflect(&self) {
        let mut guard = self.reflect_handle.lock();
        if let Some(ref mut handle) = *guard {
            handle.shutdown();
        }
        *guard = None;
    }

    /// Reconfigure the background reflect monitor.
    pub fn reconfigure_reflect(&self, config: ReflectConfig) {
        let guard = self.reflect_handle.lock();
        if let Some(ref handle) = *guard {
            handle.reconfigure(config);
        }
    }

    /// Trigger an immediate reflect run on the background worker.
    pub fn trigger_reflect(&self, scope: ReflectScope) {
        let guard = self.reflect_handle.lock();
        if let Some(ref handle) = *guard {
            handle.trigger_now(scope);
        }
    }

    // ─── Phase 5 private helpers ────────────────────────────────────

    /// Resolve forget criteria to a list of candidate memories.
    ///
    /// When entity_id is combined with other criteria, the temporal index
    /// narrows the scan first (O(log n + k) vs O(n)).
    fn resolve_forget_candidates(
        &self,
        storage: &dyn StorageBackend,
        criteria: &ForgetCriteria,
        max_batch: usize,
    ) -> Result<Vec<Memory>> {
        if criteria.is_id_only() {
            return Self::resolve_by_ids(storage, &criteria.memory_ids, max_batch);
        }

        let candidates = if let Some(ref entity_id) = criteria.entity_id {
            let temporal_results = self.index_manager.query_temporal(
                entity_id,
                0,
                u64::MAX,
                TemporalOrder::Chronological,
                max_batch * 2,
            )?;

            let mut mems = Vec::with_capacity(temporal_results.len());
            for (memory_id, _ts) in &temporal_results {
                match Self::get_from_storage(storage, memory_id) {
                    Ok(mem) => mems.push(mem),
                    Err(HebbsError::MemoryNotFound { .. }) => continue,
                    Err(e) => return Err(e),
                }
            }
            mems
        } else if !criteria.memory_ids.is_empty() {
            let mut mems = Vec::with_capacity(criteria.memory_ids.len());
            for id in &criteria.memory_ids {
                match Self::get_from_storage(storage, id) {
                    Ok(mem) => mems.push(mem),
                    Err(HebbsError::MemoryNotFound { .. }) => continue,
                    Err(e) => return Err(e),
                }
            }
            mems
        } else {
            let all_entries = storage.prefix_iterator(ColumnFamilyName::Default, &[])?;

            let mut mems = Vec::new();
            for (_key, value) in all_entries {
                if mems.len() >= max_batch * 2 {
                    break;
                }
                match Memory::from_bytes(&value) {
                    Ok(mem) => mems.push(mem),
                    Err(_) => continue,
                }
            }
            mems
        };

        let filtered: Vec<Memory> = candidates
            .into_iter()
            .filter(|mem| self.matches_forget_criteria(mem, criteria))
            .take(max_batch)
            .collect();

        Ok(filtered)
    }

    fn resolve_by_ids(
        storage: &dyn StorageBackend,
        ids: &[Vec<u8>],
        max_batch: usize,
    ) -> Result<Vec<Memory>> {
        let mut result = Vec::with_capacity(ids.len().min(max_batch));
        for id in ids {
            if result.len() >= max_batch {
                break;
            }
            match Self::get_from_storage(storage, id) {
                Ok(mem) => result.push(mem),
                Err(HebbsError::MemoryNotFound { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(result)
    }

    /// Check if a memory matches all specified forget criteria (AND semantics).
    fn matches_forget_criteria(&self, memory: &Memory, criteria: &ForgetCriteria) -> bool {
        if !criteria.memory_ids.is_empty() && !criteria.memory_ids.contains(&memory.memory_id) {
            return false;
        }

        if let Some(ref entity_id) = criteria.entity_id {
            match &memory.entity_id {
                Some(eid) if eid == entity_id => {}
                _ => return false,
            }
        }

        if let Some(threshold) = criteria.staleness_threshold_us {
            if memory.last_accessed_at >= threshold {
                return false;
            }
        }

        if let Some(floor) = criteria.access_count_floor {
            if memory.access_count >= floor {
                return false;
            }
        }

        if let Some(kind) = criteria.memory_kind {
            if memory.kind != kind {
                return false;
            }
        }

        if let Some(floor) = criteria.decay_score_floor {
            if memory.decay_score >= floor {
                return false;
            }
        }

        true
    }

    /// Find predecessor snapshot IDs for a memory by following RevisedFrom edges.
    ///
    /// Returns only snapshot IDs that are safe to cascade-delete:
    /// snapshots with ONLY RevisedFrom incoming edges and no other edge types.
    fn find_revision_snapshots(
        &self,
        _storage: &dyn StorageBackend,
        memory_id: &[u8; 16],
    ) -> Result<Vec<Vec<u8>>> {
        let outgoing = self.index_manager.outgoing_edges(memory_id)?;

        let mut snapshot_ids = Vec::new();
        for (edge_type, target_id, _meta) in &outgoing {
            if *edge_type != EdgeType::RevisedFrom {
                continue;
            }

            let incoming = self.index_manager.incoming_edges(target_id)?;
            let has_non_lineage_edges = incoming
                .iter()
                .any(|(et, _, _)| *et != EdgeType::RevisedFrom);

            if !has_non_lineage_edges {
                snapshot_ids.push(target_id.to_vec());
            }
        }

        Ok(snapshot_ids)
    }

    /// Build a human-readable description of forget criteria for tombstone records.
    fn describe_criteria(&self, criteria: &ForgetCriteria) -> String {
        let mut parts = Vec::new();

        if !criteria.memory_ids.is_empty() {
            parts.push(format!("ids:{}", criteria.memory_ids.len()));
        }
        if let Some(ref eid) = criteria.entity_id {
            parts.push(format!("entity:{}", eid));
        }
        if let Some(ts) = criteria.staleness_threshold_us {
            parts.push(format!("staleness<{}", ts));
        }
        if let Some(floor) = criteria.access_count_floor {
            parts.push(format!("access<{}", floor));
        }
        if let Some(kind) = criteria.memory_kind {
            parts.push(format!("kind:{:?}", kind));
        }
        if let Some(floor) = criteria.decay_score_floor {
            parts.push(format!("decay<{:.3}", floor));
        }

        if parts.is_empty() {
            "unknown".to_string()
        } else {
            parts.join(",")
        }
    }

    // ─── Strategy execution ──────────────────────────────────────────

    /// Execute a single strategy, returning its raw results.
    fn execute_strategy(
        &self,
        storage: &dyn StorageBackend,
        strategy: &RecallStrategy,
        ctx: &StrategyContext<'_>,
    ) -> StrategyOutcome {
        match strategy {
            RecallStrategy::Similarity => self.execute_similarity(
                storage,
                ctx.cue_embedding,
                ctx.top_k,
                ctx.ef_search,
                ctx.entity_id,
            ),
            RecallStrategy::Temporal => {
                self.execute_temporal(storage, ctx.entity_id, ctx.time_range, ctx.top_k)
            }
            RecallStrategy::Causal => self.execute_causal(
                storage,
                ctx.cue,
                ctx.cue_embedding,
                ctx.edge_types,
                ctx.max_depth,
                ctx.top_k,
                ctx.tenant_id,
                &ctx.causal_direction,
                ctx.entity_id,
                ctx.seed_memory_id,
            ),
            RecallStrategy::Analogical => self.execute_analogical(
                storage,
                ctx.cue_embedding,
                ctx.top_k,
                ctx.ef_search,
                ctx.cue_context,
                ctx.tenant_id,
                ctx.analogy_a_id,
                ctx.analogy_b_id,
                ctx.entity_id,
                ctx.analogical_alpha,
            ),
        }
    }

    fn execute_strategies_parallel(
        &self,
        storage: &(dyn StorageBackend + Sync),
        strategies: &[RecallStrategy],
        ctx: &StrategyContext<'_>,
    ) -> Vec<StrategyOutcome> {
        std::thread::scope(|s| {
            let handles: Vec<_> = strategies
                .iter()
                .map(|strategy| s.spawn(move || self.execute_strategy(storage, strategy, ctx)))
                .collect();

            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        StrategyOutcome::Err(
                            RecallStrategy::Similarity,
                            "strategy thread panicked".to_string(),
                        )
                    })
                })
                .collect()
        })
    }

    /// Similarity strategy: embed cue, HNSW top-K, rank by distance.
    ///
    /// When `entity_id` is `Some`, over-fetches from HNSW by
    /// [`ENTITY_OVERSAMPLE`] and post-filters by entity to enforce
    /// tenant isolation (the HNSW index is global).
    ///
    /// Complexity: O(log n * ef_search) + O(k * ENTITY_OVERSAMPLE * point_lookup).
    fn execute_similarity(
        &self,
        storage: &dyn StorageBackend,
        cue_embedding: Option<&[f32]>,
        top_k: usize,
        ef_search: Option<usize>,
        entity_id: Option<&str>,
    ) -> StrategyOutcome {
        let embedding = match cue_embedding {
            Some(e) => e,
            None => {
                return StrategyOutcome::Err(
                    RecallStrategy::Similarity,
                    "cue embedding required for similarity strategy but embedding failed"
                        .to_string(),
                );
            }
        };

        let fetch_k = if entity_id.is_some() {
            top_k.saturating_mul(ENTITY_OVERSAMPLE).max(top_k)
        } else {
            top_k
        };

        let _hnsw_span = bench_span!("recall.similarity.hnsw_search");
        let search_results = match self
            .index_manager
            .search_vector(embedding, fetch_k, ef_search)
        {
            Ok(r) => r,
            Err(e) => {
                return StrategyOutcome::Err(
                    RecallStrategy::Similarity,
                    format!("HNSW search failed: {}", e),
                );
            }
        };
        bench_span_drop!(_hnsw_span);

        let _load_span = bench_span!("recall.similarity.load_memories");
        let mut results = Vec::with_capacity(top_k.min(search_results.len()));
        for (memory_id, distance) in search_results {
            match Self::get_from_storage(storage, &memory_id) {
                Ok(mem) => {
                    if let Some(eid) = entity_id {
                        if mem.entity_id.as_deref() != Some(eid) {
                            continue;
                        }
                    }
                    let relevance = (1.0 - distance).max(0.0);
                    results.push(StrategyResult {
                        memory: mem,
                        relevance,
                        detail: StrategyDetail::Similarity {
                            distance,
                            relevance,
                        },
                    });
                    if results.len() >= top_k {
                        break;
                    }
                }
                Err(HebbsError::MemoryNotFound { .. }) => continue,
                Err(_) => continue,
            }
        }
        bench_span_drop!(_load_span);

        StrategyOutcome::Ok(results)
    }

    /// Temporal strategy: query temporal index by entity and time range.
    ///
    /// Complexity: O(log n + k).
    fn execute_temporal(
        &self,
        storage: &dyn StorageBackend,
        entity_id: Option<&str>,
        time_range: Option<(u64, u64)>,
        top_k: usize,
    ) -> StrategyOutcome {
        let eid = match entity_id {
            Some(id) if !id.is_empty() => id,
            _ => {
                return StrategyOutcome::Err(
                    RecallStrategy::Temporal,
                    "entity_id is required for temporal strategy".to_string(),
                );
            }
        };

        let (start_us, end_us) = time_range.unwrap_or((0, u64::MAX));

        let _idx_span = bench_span!("recall.temporal.index_query");
        let temporal_results = match self.index_manager.query_temporal(
            eid,
            start_us,
            end_us,
            TemporalOrder::ReverseChronological,
            top_k,
        ) {
            Ok(r) => r,
            Err(e) => {
                return StrategyOutcome::Err(
                    RecallStrategy::Temporal,
                    format!("temporal query failed: {}", e),
                );
            }
        };
        bench_span_drop!(_idx_span);

        let _load_span = bench_span!("recall.temporal.load_memories");
        let total = temporal_results.len();
        let mut results = Vec::with_capacity(total);
        for (rank, (memory_id, timestamp)) in temporal_results.iter().enumerate() {
            match Self::get_from_storage(storage, memory_id) {
                Ok(mem) => {
                    let relevance = if total > 1 {
                        1.0 - (rank as f32 / total as f32)
                    } else {
                        1.0
                    };
                    results.push(StrategyResult {
                        memory: mem,
                        relevance,
                        detail: StrategyDetail::Temporal {
                            timestamp: *timestamp,
                            rank,
                            relevance,
                        },
                    });
                }
                Err(HebbsError::MemoryNotFound { .. }) => continue,
                Err(_) => continue,
            }
        }
        bench_span_drop!(_load_span);

        StrategyOutcome::Ok(results)
    }

    /// Causal strategy: find seed, traverse graph edges.
    ///
    /// Seed selection:
    /// - If the cue looks like a hex-encoded memory_id (32 hex chars), use it directly.
    /// - Otherwise, use the cue embedding to find the closest memory as seed.
    ///
    /// When `entity_id` is `Some`, the seed must belong to the target
    /// entity and traversal results are post-filtered by entity.
    ///
    /// Complexity: O(embed or point_lookup) + O(branching_factor^max_depth) + O(k * point_lookup).
    fn execute_causal(
        &self,
        storage: &dyn StorageBackend,
        cue: &str,
        cue_embedding: Option<&[f32]>,
        edge_types: Option<&[EdgeType]>,
        max_depth: usize,
        top_k: usize,
        tenant_id: &str,
        direction: &CausalDirection,
        entity_id: Option<&str>,
        seed_memory_id: Option<[u8; 16]>,
    ) -> StrategyOutcome {
        let _seed_span = bench_span!("recall.causal.seed_resolve");
        if let Some(seed) = seed_memory_id {
            match Self::get_from_storage(storage, &seed) {
                Ok(mem) => {
                    if let Some(eid) = entity_id {
                        if mem.entity_id.as_deref() != Some(eid) {
                            return StrategyOutcome::Err(
                                RecallStrategy::Causal,
                                format!(
                                    "causal seed memory belongs to a different entity (expected {})",
                                    eid
                                ),
                            );
                        }
                    }
                }
                Err(_) => {
                    return StrategyOutcome::Err(
                        RecallStrategy::Causal,
                        format!("causal recall seed memory not found: {}", hex::encode(seed)),
                    );
                }
            }
            return self.causal_from_seed(
                storage, seed, edge_types, max_depth, top_k, tenant_id, direction, entity_id,
            );
        }

        let seed_id = if cue.len() == 32 && cue.chars().all(|c| c.is_ascii_hexdigit()) {
            match hex::decode(cue) {
                Ok(bytes) if bytes.len() == 16 => {
                    let mut id = [0u8; 16];
                    id.copy_from_slice(&bytes);
                    match Self::get_from_storage(storage, &id) {
                        Ok(mem) => {
                            if let Some(eid) = entity_id {
                                if mem.entity_id.as_deref() != Some(eid) {
                                    return StrategyOutcome::Err(
                                        RecallStrategy::Causal,
                                        format!(
                                            "causal seed memory belongs to a different entity: {}",
                                            cue
                                        ),
                                    );
                                }
                            }
                        }
                        Err(_) => {
                            return StrategyOutcome::Err(
                                RecallStrategy::Causal,
                                format!("causal recall seed memory not found: {}", cue),
                            );
                        }
                    }
                    id
                }
                _ => {
                    return self.causal_by_embedding(
                        storage,
                        cue_embedding,
                        edge_types,
                        max_depth,
                        top_k,
                        tenant_id,
                        direction,
                        entity_id,
                    );
                }
            }
        } else {
            return self.causal_by_embedding(
                storage,
                cue_embedding,
                edge_types,
                max_depth,
                top_k,
                tenant_id,
                direction,
                entity_id,
            );
        };

        self.causal_from_seed(
            storage, seed_id, edge_types, max_depth, top_k, tenant_id, direction, entity_id,
        )
    }

    fn causal_by_embedding(
        &self,
        storage: &dyn StorageBackend,
        cue_embedding: Option<&[f32]>,
        edge_types: Option<&[EdgeType]>,
        max_depth: usize,
        top_k: usize,
        tenant_id: &str,
        direction: &CausalDirection,
        entity_id: Option<&str>,
    ) -> StrategyOutcome {
        let embedding = match cue_embedding {
            Some(e) => e,
            None => {
                return StrategyOutcome::Err(
                    RecallStrategy::Causal,
                    "cue embedding required for causal strategy with text seed but embedding failed"
                        .to_string(),
                );
            }
        };

        let fetch_k = if entity_id.is_some() {
            ENTITY_OVERSAMPLE
        } else {
            1
        };
        let candidates = match self.index_manager.search_vector(embedding, fetch_k, None) {
            Ok(r) if !r.is_empty() => r,
            Ok(_) => {
                return StrategyOutcome::Ok(Vec::new());
            }
            Err(e) => {
                return StrategyOutcome::Err(
                    RecallStrategy::Causal,
                    format!("HNSW search for causal seed failed: {}", e),
                );
            }
        };

        // Find the first seed that matches the entity filter
        for (candidate_id, _) in &candidates {
            if let Some(eid) = entity_id {
                match Self::get_from_storage(storage, candidate_id) {
                    Ok(mem) if mem.entity_id.as_deref() == Some(eid) => {}
                    Ok(_) => continue,
                    Err(_) => continue,
                }
            }
            return self.causal_from_seed(
                storage,
                *candidate_id,
                edge_types,
                max_depth,
                top_k,
                tenant_id,
                direction,
                entity_id,
            );
        }

        StrategyOutcome::Ok(Vec::new())
    }

    fn causal_from_seed(
        &self,
        storage: &dyn StorageBackend,
        seed_id: [u8; 16],
        edge_types: Option<&[EdgeType]>,
        max_depth: usize,
        top_k: usize,
        tenant_id: &str,
        direction: &CausalDirection,
        entity_id: Option<&str>,
    ) -> StrategyOutcome {
        let _traverse_span = bench_span!("recall.causal.graph_traverse");
        let default_edge_types = vec![
            EdgeType::CausedBy,
            EdgeType::RelatedTo,
            EdgeType::FollowedBy,
            EdgeType::RevisedFrom,
            EdgeType::InsightFrom,
            EdgeType::Contradicts,
        ];
        let types = edge_types.unwrap_or(&default_edge_types);

        let seed_mem = match Self::get_from_storage(storage, &seed_id) {
            Ok(m) => m,
            Err(e) => {
                return StrategyOutcome::Err(
                    RecallStrategy::Causal,
                    format!("causal seed memory load failed: {}", e),
                );
            }
        };
        let seed_assoc: Vec<f32> = seed_mem
            .associative_embedding
            .clone()
            .or_else(|| seed_mem.embedding.clone())
            .unwrap_or_default();

        let mut seen = std::collections::HashSet::new();
        seen.insert(seed_id);
        let mut results: Vec<StrategyResult> = Vec::new();

        if seed_assoc.is_empty() {
            let (traversal_entries, _truncated) = match self
                .index_manager
                .traverse(&seed_id, types, max_depth, top_k)
            {
                Ok(r) => r,
                Err(e) => {
                    return StrategyOutcome::Err(
                        RecallStrategy::Causal,
                        format!("graph traversal failed: {}", e),
                    );
                }
            };
            let max_depth_f32 = max_depth as f32;
            for entry in &traversal_entries {
                match Self::get_from_storage(storage, &entry.memory_id) {
                    Ok(mem) => {
                        if let Some(eid) = entity_id {
                            if mem.entity_id.as_deref() != Some(eid) {
                                continue;
                            }
                        }
                        let relevance = if max_depth_f32 > 0.0 {
                            (1.0 - (entry.depth as f32 / max_depth_f32)).max(0.0)
                        } else {
                            1.0
                        };
                        results.push(StrategyResult {
                            memory: mem,
                            relevance,
                            detail: StrategyDetail::Causal {
                                depth: entry.depth,
                                edge_type: entry.edge_type,
                                seed_id,
                                relevance,
                            },
                        });
                    }
                    Err(HebbsError::MemoryNotFound { .. }) => continue,
                    Err(_) => continue,
                }
            }
            return StrategyOutcome::Ok(results);
        }

        for &etype in types {
            let fwd_hits: Vec<([u8; 16], f32)> = match direction {
                CausalDirection::Backward => vec![],
                _ => self
                    .index_manager
                    .assoc_index
                    .search_causal_forward(tenant_id, &seed_assoc, etype, top_k, None)
                    .unwrap_or_default(),
            };
            let bwd_hits: Vec<([u8; 16], f32)> = match direction {
                CausalDirection::Forward => vec![],
                _ => self
                    .index_manager
                    .assoc_index
                    .search_causal_backward(tenant_id, &seed_assoc, etype, top_k, None)
                    .unwrap_or_default(),
            };

            for (mem_id, distance) in fwd_hits.into_iter().chain(bwd_hits) {
                if seen.contains(&mem_id) {
                    continue;
                }
                seen.insert(mem_id);
                match Self::get_from_storage(storage, &mem_id) {
                    Ok(mem) => {
                        if let Some(eid) = entity_id {
                            if mem.entity_id.as_deref() != Some(eid) {
                                continue;
                            }
                        }
                        let relevance = (1.0 - distance).max(0.0);
                        results.push(StrategyResult {
                            memory: mem,
                            relevance,
                            detail: StrategyDetail::Causal {
                                depth: 1,
                                edge_type: etype,
                                seed_id,
                                relevance,
                            },
                        });
                    }
                    Err(HebbsError::MemoryNotFound { .. }) => continue,
                    Err(_) => continue,
                }
            }
        }

        results.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(top_k);

        StrategyOutcome::Ok(results)
    }

    /// Analogical strategy: wider HNSW search + structural similarity re-ranking.
    ///
    /// When `entity_id` is `Some`, results are post-filtered by entity.
    /// When `None`, cross-entity search is allowed (the intentional
    /// use-case for analogical reasoning across domains).
    ///
    /// Complexity: O(log n * 2 * ef_search) + O(candidates * structural_compare).
    fn execute_analogical(
        &self,
        storage: &dyn StorageBackend,
        cue_embedding: Option<&[f32]>,
        top_k: usize,
        ef_search: Option<usize>,
        cue_context: Option<&HashMap<String, serde_json::Value>>,
        tenant_id: &str,
        analogy_a_id: Option<[u8; 16]>,
        analogy_b_id: Option<[u8; 16]>,
        entity_id: Option<&str>,
        analogical_alpha: Option<f32>,
    ) -> StrategyOutcome {
        let _hnsw_span = bench_span!("recall.analogical.hnsw_search");
        if let (Some(a_id), Some(b_id)) = (analogy_a_id, analogy_b_id) {
            let a_mem = Self::get_from_storage(storage, &a_id);
            let b_mem = Self::get_from_storage(storage, &b_id);
            if let (Ok(a_mem), Ok(b_mem)) = (a_mem, b_mem) {
                let a_assoc = a_mem
                    .associative_embedding
                    .as_deref()
                    .or(a_mem.embedding.as_deref())
                    .unwrap_or(&[]);
                let b_assoc = b_mem
                    .associative_embedding
                    .as_deref()
                    .or(b_mem.embedding.as_deref())
                    .unwrap_or(&[]);
                let c_assoc = cue_embedding.unwrap_or(&[]);
                if !a_assoc.is_empty() && !b_assoc.is_empty() && !c_assoc.is_empty() {
                    let fetch_k = if entity_id.is_some() {
                        top_k.saturating_mul(ENTITY_OVERSAMPLE).max(top_k)
                    } else {
                        top_k
                    };
                    let ef = ef_search.map(|e| e * 2).or(Some(200));
                    let hits = self
                        .index_manager
                        .assoc_index
                        .search_analogy(tenant_id, a_assoc, b_assoc, c_assoc, fetch_k, ef);
                    match hits {
                        Ok(search_results) => {
                            let mut scored: Vec<StrategyResult> =
                                Vec::with_capacity(top_k.min(search_results.len()));
                            for (memory_id, distance) in &search_results {
                                match Self::get_from_storage(storage, memory_id) {
                                    Ok(mem) => {
                                        if let Some(eid) = entity_id {
                                            if mem.entity_id.as_deref() != Some(eid) {
                                                continue;
                                            }
                                        }
                                        let relevance = (1.0 - distance).max(0.0);
                                        scored.push(StrategyResult {
                                            memory: mem,
                                            relevance,
                                            detail: StrategyDetail::Analogical {
                                                embedding_similarity: relevance,
                                                structural_similarity: 0.0,
                                                relevance,
                                                used_vector_analogy: true,
                                            },
                                        });
                                    }
                                    Err(HebbsError::MemoryNotFound { .. }) => continue,
                                    Err(_) => continue,
                                }
                            }
                            scored.sort_by(|a, b| {
                                b.relevance
                                    .partial_cmp(&a.relevance)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            });
                            scored.truncate(top_k);
                            return StrategyOutcome::Ok(scored);
                        }
                        Err(e) => {
                            return StrategyOutcome::Err(
                                RecallStrategy::Analogical,
                                format!("assoc HNSW analogy search failed: {}", e),
                            );
                        }
                    }
                }
            }
        }

        let embedding = match cue_embedding {
            Some(e) => e,
            None => {
                return StrategyOutcome::Err(
                    RecallStrategy::Analogical,
                    "cue embedding required for analogical strategy but embedding failed"
                        .to_string(),
                );
            }
        };

        let wider_ef = ef_search.map(|e| e * 2).or(Some(200));
        let base_candidate_count = (top_k * 3).max(30);
        let candidate_count = if entity_id.is_some() {
            base_candidate_count.saturating_mul(ENTITY_OVERSAMPLE)
        } else {
            base_candidate_count
        };

        let search_results =
            match self
                .index_manager
                .search_vector(embedding, candidate_count, wider_ef)
            {
                Ok(r) => r,
                Err(e) => {
                    return StrategyOutcome::Err(
                        RecallStrategy::Analogical,
                        format!("HNSW search for analogical strategy failed: {}", e),
                    );
                }
            };

        let _load_span = bench_span!("recall.analogical.load_memories");
        let mut analogical_weights = AnalogicalWeights::default();
        if let Some(alpha) = analogical_alpha {
            analogical_weights.alpha = alpha.clamp(0.0, 1.0);
        }
        let cue_ctx = cue_context.cloned().unwrap_or_default();

        let mut scored_candidates: Vec<StrategyResult> = Vec::with_capacity(search_results.len());
        for (memory_id, distance) in &search_results {
            match Self::get_from_storage(storage, memory_id) {
                Ok(mem) => {
                    if let Some(eid) = entity_id {
                        if mem.entity_id.as_deref() != Some(eid) {
                            continue;
                        }
                    }
                    let embedding_similarity = (1.0 - distance).max(0.0);
                    let structural_similarity = compute_structural_similarity(
                        &cue_ctx,
                        &mem,
                        &analogical_weights,
                        entity_id,
                    );
                    let relevance = analogical_weights.alpha * embedding_similarity
                        + (1.0 - analogical_weights.alpha) * structural_similarity;

                    scored_candidates.push(StrategyResult {
                        memory: mem,
                        relevance,
                        detail: StrategyDetail::Analogical {
                            embedding_similarity,
                            structural_similarity,
                            relevance,
                            used_vector_analogy: false,
                        },
                    });
                }
                Err(HebbsError::MemoryNotFound { .. }) => continue,
                Err(_) => continue,
            }
        }

        bench_span_drop!(_load_span);

        let _rerank_span = bench_span!("recall.analogical.structural_rerank");
        scored_candidates.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored_candidates.truncate(top_k);
        bench_span_drop!(_rerank_span);

        StrategyOutcome::Ok(scored_candidates)
    }

    // ─── Merge, score, rank ──────────────────────────────────────────

    /// Merge results from multiple strategies, deduplicate by memory_id,
    /// compute composite scores, rank, and truncate.
    ///
    /// Deduplication keeps the highest relevance and records all strategies
    /// that found the memory (multi-strategy provenance).
    ///
    /// Complexity: O(n) dedup + O(n * scoring) + O(n log n) sort + O(k) truncate
    /// where n ≤ strategies * top_k (bounded, typically ≤ 40).
    fn merge_and_rank(
        &self,
        results: Vec<StrategyResult>,
        weights: &ScoringWeights,
        now_us: u64,
        top_k: usize,
    ) -> Vec<RecallResult> {
        if results.is_empty() {
            return Vec::new();
        }

        // Deduplicate by memory_id, keeping highest relevance and merging strategy details
        let mut dedup_map: HashMap<Vec<u8>, (Memory, f32, Vec<StrategyDetail>)> =
            HashMap::with_capacity(results.len());

        for r in results {
            let key = r.memory.memory_id.clone();
            match dedup_map.get_mut(&key) {
                Some(existing) => {
                    if r.relevance > existing.1 {
                        existing.1 = r.relevance;
                    }
                    existing.2.push(r.detail);
                }
                None => {
                    dedup_map.insert(key, (r.memory, r.relevance, vec![r.detail]));
                }
            }
        }

        // Compute composite scores and build final results
        let mut ranked: Vec<RecallResult> = dedup_map
            .into_values()
            .map(|(memory, relevance, strategy_details)| {
                let score = compute_composite_score(relevance, &memory, weights, now_us);
                RecallResult {
                    memory,
                    score,
                    strategy_details,
                }
            })
            .collect();

        // Sort by composite score descending
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        ranked.truncate(top_k);
        ranked
    }

    // ─── Reinforcement ───────────────────────────────────────────────

    /// Update last_accessed_at and access_count for all recalled memories.
    ///
    /// Synchronous WriteBatch to the default CF only. Temporal, vector, and
    /// graph indexes are not affected by reinforcement updates.
    ///
    /// If the WriteBatch fails, it is logged but does not prevent results
    /// from being returned. Reinforcement is best-effort.
    fn reinforce_memories(storage: &dyn StorageBackend, results: &[RecallResult], now_us: u64) {
        if results.is_empty() {
            return;
        }

        let mut ops = Vec::with_capacity(results.len());
        for result in results {
            let mut updated = result.memory.clone();
            updated.last_accessed_at = now_us;
            updated.access_count = updated.access_count.saturating_add(1);

            let key = keys::encode_memory_key(&updated.memory_id);
            let value = updated.to_bytes();

            ops.push(BatchOperation::Put {
                cf: ColumnFamilyName::Default,
                key,
                value,
            });
        }

        if let Err(e) = storage.write_batch(&ops) {
            tracing::error!(
                error = %e,
                count = results.len(),
                "recall reinforcement WriteBatch failed (non-fatal)"
            );
        }
    }

    // ─── Prime helpers ───────────────────────────────────────────────

    /// Build a synthetic cue from entity_id and context values.
    ///
    /// Concatenates entity_id and context values into a single string
    /// suitable for embedding. Low-quality but functional when no explicit
    /// similarity_cue is provided.
    fn build_synthetic_cue(
        &self,
        entity_id: &str,
        context: Option<&HashMap<String, serde_json::Value>>,
    ) -> String {
        let mut parts = vec![entity_id.to_string()];
        if let Some(ctx) = context {
            for (key, val) in ctx {
                match val {
                    serde_json::Value::String(s) => parts.push(format!("{}: {}", key, s)),
                    serde_json::Value::Number(n) => parts.push(format!("{}: {}", key, n)),
                    serde_json::Value::Bool(b) => parts.push(format!("{}: {}", key, b)),
                    _ => parts.push(format!("{}: {}", key, val)),
                }
            }
        }
        parts.join(" ")
    }

    /// Initialize or validate the schema version in the `meta` CF.
    fn init_schema(&self) -> Result<()> {
        let key = keys::encode_meta_key("schema_version");
        match self.storage.get(ColumnFamilyName::Meta, &key)? {
            Some(bytes) => {
                if bytes.len() != 4 {
                    return Err(HebbsError::Internal {
                        operation: "init_schema",
                        message: format!(
                            "schema_version has invalid length {} (expected 4)",
                            bytes.len()
                        ),
                    });
                }
                let stored = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                if stored > SCHEMA_VERSION {
                    return Err(HebbsError::Internal {
                        operation: "init_schema",
                        message: format!(
                            "database schema version {} is newer than binary version {} — upgrade the binary",
                            stored, SCHEMA_VERSION
                        ),
                    });
                }
                Ok(())
            }
            None => {
                self.storage
                    .put(ColumnFamilyName::Meta, &key, &SCHEMA_VERSION.to_be_bytes())?;
                Ok(())
            }
        }
    }
}

/// Shared context passed to strategy execution functions.
/// Avoids passing many individual parameters through execute_strategy/execute_strategies_parallel.
struct StrategyContext<'a> {
    cue: &'a str,
    cue_embedding: Option<&'a [f32]>,
    entity_id: Option<&'a str>,
    time_range: Option<(u64, u64)>,
    edge_types: Option<&'a [EdgeType]>,
    max_depth: usize,
    top_k: usize,
    ef_search: Option<usize>,
    cue_context: Option<&'a HashMap<String, serde_json::Value>>,
    tenant_id: &'a str,
    causal_direction: CausalDirection,
    analogy_a_id: Option<[u8; 16]>,
    analogy_b_id: Option<[u8; 16]>,
    seed_memory_id: Option<[u8; 16]>,
    analogical_alpha: Option<f32>,
}

/// Returns the current time as microseconds since the Unix epoch.
pub(crate) fn now_microseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before Unix epoch")
        .as_micros() as u64
}

/// Compute the composite score for a recalled memory.
///
/// ```text
/// composite = w_relevance * relevance
///           + w_recency  * recency_signal
///           + w_importance * importance
///           + w_reinforcement * reinforcement_signal
/// ```
///
/// All components are in [0.0, 1.0]. The output range is [0.0, sum_of_weights].
///
/// Complexity: O(1).
pub fn compute_composite_score(
    relevance: f32,
    memory: &Memory,
    weights: &ScoringWeights,
    now_us: u64,
) -> f32 {
    let recency_signal = if weights.max_age_us > 0 && now_us > memory.created_at {
        let age = now_us - memory.created_at;
        (1.0 - (age as f64 / weights.max_age_us as f64) as f32).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let importance = memory.importance.clamp(0.0, 1.0);

    let reinforcement_signal = if weights.reinforcement_cap > 0 {
        let log_access = (1.0 + memory.access_count as f64).log2();
        let log_cap = (1.0 + weights.reinforcement_cap as f64).log2();
        (log_access / log_cap).min(1.0) as f32
    } else {
        0.0
    };

    weights.w_relevance * relevance.clamp(0.0, 1.0)
        + weights.w_recency * recency_signal
        + weights.w_importance * importance
        + weights.w_reinforcement * reinforcement_signal
}

/// Compute structural similarity between a cue's context and a candidate memory.
///
/// Measures overlap in metadata shape, not content:
/// - Key overlap: fraction of context keys shared (weight: 0.4)
/// - Value type match: for shared keys, same JSON type (weight: 0.3)
/// - Kind match: same MemoryKind (weight: 0.2)
/// - Entity pattern match: similar entity_id structure (weight: 0.1)
///
/// Returns a score normalized to [0.0, 1.0].
///
/// Complexity: O(|keys_cue| + |keys_mem|).
fn compute_structural_similarity(
    cue_context: &HashMap<String, serde_json::Value>,
    memory: &Memory,
    weights: &AnalogicalWeights,
    cue_entity_id: Option<&str>,
) -> f32 {
    let mem_context = memory.context().unwrap_or_default();

    // Key overlap and type match: require non-empty context on both sides.
    let (key_overlap, type_match) = if cue_context.is_empty() || mem_context.is_empty() {
        (0.0_f32, 0.0_f32)
    } else {
        // Key overlap: Jaccard-like coefficient
        let cue_keys: HashSet<&String> = cue_context.keys().collect();
        let mem_keys: HashSet<&String> = mem_context.keys().collect();
        let intersection = cue_keys.intersection(&mem_keys).count();
        let union = cue_keys.union(&mem_keys).count();
        let ko = if union > 0 {
            intersection as f32 / union as f32
        } else {
            0.0
        };

        // Value type match: for shared keys, do the JSON types match?
        let shared_keys: Vec<&&String> = cue_keys.intersection(&mem_keys).collect();
        let tm = if !shared_keys.is_empty() {
            let matches = shared_keys
                .iter()
                .filter(|k| json_type_matches(&cue_context[k.as_str()], &mem_context[k.as_str()]))
                .count();
            matches as f32 / shared_keys.len() as f32
        } else {
            0.0
        };

        (ko, tm)
    };

    // Kind match: compare against cue_context["kind"] if present.
    let kind_match = match cue_context.get("kind").and_then(|v| v.as_str()) {
        Some(cue_kind) => {
            let mem_kind_str = match memory.kind {
                MemoryKind::Episode => "episode",
                MemoryKind::Insight => "insight",
                MemoryKind::Revision => "revision",
            };
            if cue_kind == mem_kind_str {
                1.0
            } else {
                0.0
            }
        }
        None => 0.5, // neutral when cue doesn't specify kind
    };

    // Entity pattern match: compare entity IDs.
    let entity_pattern = match (cue_entity_id, memory.entity_id.as_deref()) {
        (Some(cue_eid), Some(mem_eid)) => {
            if cue_eid == mem_eid {
                1.0
            } else {
                0.0
            }
        }
        (None, None) => 0.5, // both unscoped — neutral
        _ => 0.0,            // one scoped, one not — mismatch
    };

    weights.key_overlap_weight * key_overlap
        + weights.value_type_match_weight * type_match
        + weights.kind_match_weight * kind_match
        + weights.entity_pattern_weight * entity_pattern
}

/// Check if two JSON values have the same type (not value).
fn json_type_matches(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    matches!(
        (a, b),
        (serde_json::Value::Null, serde_json::Value::Null)
            | (serde_json::Value::Bool(_), serde_json::Value::Bool(_))
            | (serde_json::Value::Number(_), serde_json::Value::Number(_))
            | (serde_json::Value::String(_), serde_json::Value::String(_))
            | (serde_json::Value::Array(_), serde_json::Value::Array(_))
            | (serde_json::Value::Object(_), serde_json::Value::Object(_))
    )
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop_reflect();
        self.subscribe_registry.shutdown_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forget::{ForgetConfig, ForgetCriteria};
    use crate::recall::DEFAULT_MAX_AGE_US;
    use crate::revise::{ContextMode, ReviseInput};
    use hebbs_embed::MockEmbedder;
    use hebbs_storage::InMemoryBackend;

    fn test_engine() -> Engine {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 4);
        Engine::new_with_params(backend, embedder, params, 42).unwrap()
    }

    fn test_engine_with_larger_hnsw() -> Engine {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 16);
        Engine::new_with_params(backend, embedder, params, 42).unwrap()
    }

    fn simple_input(content: &str) -> RememberInput {
        RememberInput {
            content: content.to_string(),
            importance: None,
            context: None,
            entity_id: None,
            edges: vec![],
        }
    }

    fn entity_input(content: &str, entity: &str) -> RememberInput {
        RememberInput {
            content: content.to_string(),
            importance: None,
            context: None,
            entity_id: Some(entity.to_string()),
            edges: vec![],
        }
    }

    #[allow(dead_code)]
    fn entity_input_with_importance(content: &str, entity: &str, importance: f32) -> RememberInput {
        RememberInput {
            content: content.to_string(),
            importance: Some(importance),
            context: None,
            entity_id: Some(entity.to_string()),
            edges: vec![],
        }
    }

    #[test]
    fn remember_basic() {
        let engine = test_engine();
        let memory = engine
            .remember(RememberInput {
                content: "test memory content".to_string(),
                importance: Some(0.8),
                context: None,
                entity_id: Some("customer_1".to_string()),
                edges: vec![],
            })
            .unwrap();

        assert_eq!(memory.content, "test memory content");
        assert_eq!(memory.importance, 0.8);
        assert_eq!(memory.entity_id, Some("customer_1".to_string()));
        assert_eq!(memory.access_count, 0);
        assert_eq!(memory.kind, MemoryKind::Episode);
        assert!(memory.embedding.is_some());
        assert_eq!(memory.embedding.as_ref().unwrap().len(), 384);
        assert_eq!(memory.memory_id.len(), 16);
    }

    #[test]
    fn remember_defaults() {
        let engine = test_engine();
        let memory = engine.remember(simple_input("minimal")).unwrap();

        assert_eq!(memory.importance, DEFAULT_IMPORTANCE);
        assert!(memory.context_bytes.is_empty());
        assert!(memory.entity_id.is_none());
    }

    #[test]
    fn remember_and_get_roundtrip() {
        let engine = test_engine();
        let original = engine
            .remember(RememberInput {
                content: "roundtrip test".to_string(),
                importance: Some(0.7),
                context: None,
                entity_id: None,
                edges: vec![],
            })
            .unwrap();

        let retrieved = engine.get(&original.memory_id).unwrap();
        assert_eq!(original.content, retrieved.content);
        assert_eq!(original.importance, retrieved.importance);
        assert_eq!(original.memory_id, retrieved.memory_id);
    }

    #[test]
    fn remember_writes_to_all_indexes() {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 4);
        let engine = Engine::new_with_params(backend.clone(), embedder, params, 42).unwrap();

        let memory = engine
            .remember(RememberInput {
                content: "indexed memory".to_string(),
                importance: Some(0.8),
                context: None,
                entity_id: Some("entity_1".to_string()),
                edges: vec![],
            })
            .unwrap();

        // Verify default CF
        let key = keys::encode_memory_key(&memory.memory_id);
        assert!(backend
            .get(ColumnFamilyName::Default, &key)
            .unwrap()
            .is_some());

        // Verify temporal CF
        let temporal_results = engine
            .query_temporal("entity_1", 0, u64::MAX, TemporalOrder::Chronological, 10)
            .unwrap();
        assert_eq!(temporal_results.len(), 1);

        // Verify vectors CF (HNSW search)
        let embedding = memory.embedding.as_ref().unwrap();
        let search_results = engine.search_similar(embedding, 1, None).unwrap();
        assert_eq!(search_results.len(), 1);
    }

    #[test]
    fn remember_with_edges() {
        let engine = test_engine();

        // First, create a target memory
        let target = engine.remember(simple_input("target memory")).unwrap();
        let mut target_id = [0u8; 16];
        target_id.copy_from_slice(&target.memory_id);

        // Create a memory with an edge to the target
        let source = engine
            .remember(RememberInput {
                content: "source with edge".to_string(),
                importance: None,
                context: None,
                entity_id: None,
                edges: vec![RememberEdge {
                    target_id,
                    edge_type: EdgeType::CausedBy,
                    confidence: Some(0.9),
                }],
            })
            .unwrap();

        // Verify the edge exists via traversal
        let mut source_id = [0u8; 16];
        source_id.copy_from_slice(&source.memory_id);

        let (results, _) = engine
            .traverse_graph(&source_id, &[EdgeType::CausedBy], 1, 10)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory_id, target_id);
    }

    #[test]
    fn delete_removes_from_all_indexes() {
        let engine = test_engine();
        let memory = engine
            .remember(RememberInput {
                content: "to be deleted".to_string(),
                importance: None,
                context: None,
                entity_id: Some("entity_del".to_string()),
                edges: vec![],
            })
            .unwrap();

        let embedding = memory.embedding.clone().unwrap();
        engine.delete(&memory.memory_id).unwrap();

        // Verify default CF
        let err = engine.get(&memory.memory_id).unwrap_err();
        assert!(matches!(err, HebbsError::MemoryNotFound { .. }));

        // Verify temporal
        let temporal = engine
            .query_temporal("entity_del", 0, u64::MAX, TemporalOrder::Chronological, 10)
            .unwrap();
        assert!(temporal.is_empty());

        // Verify HNSW (deleted nodes excluded from search)
        let results = engine.search_similar(&embedding, 10, None).unwrap();
        let mut id_arr = [0u8; 16];
        id_arr.copy_from_slice(&memory.memory_id);
        assert!(results.iter().all(|(id, _)| *id != id_arr));
    }

    #[test]
    fn delete_nonexistent_returns_error() {
        let engine = test_engine();
        let fake_id = [0u8; 16];
        let err = engine.delete(&fake_id).unwrap_err();
        assert!(matches!(err, HebbsError::MemoryNotFound { .. }));
    }

    #[test]
    fn reject_empty_content() {
        let engine = test_engine();
        let err = engine.remember(simple_input("")).unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    #[test]
    fn reject_oversized_content() {
        let engine = test_engine();
        let huge = "x".repeat(MAX_CONTENT_LENGTH + 1);
        let err = engine.remember(simple_input(&huge)).unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    #[test]
    fn reject_importance_out_of_range() {
        let engine = test_engine();
        for bad_val in [1.1, -0.1, f32::NAN, f32::INFINITY] {
            let err = engine
                .remember(RememberInput {
                    content: "test".to_string(),
                    importance: Some(bad_val),
                    context: None,
                    entity_id: None,
                    edges: vec![],
                })
                .unwrap_err();
            assert!(
                matches!(err, HebbsError::InvalidInput { .. }),
                "importance {} should be rejected",
                bad_val
            );
        }
    }

    #[test]
    fn reject_invalid_memory_id_length() {
        let engine = test_engine();
        let err = engine.get(&[0u8; 8]).unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    #[test]
    fn list_by_entity_uses_temporal_index() {
        let engine = test_engine();
        for i in 0..5 {
            engine
                .remember(RememberInput {
                    content: format!("alice memory {}", i),
                    importance: None,
                    context: None,
                    entity_id: Some("alice".to_string()),
                    edges: vec![],
                })
                .unwrap();
        }
        for i in 0..3 {
            engine
                .remember(RememberInput {
                    content: format!("bob memory {}", i),
                    importance: None,
                    context: None,
                    entity_id: Some("bob".to_string()),
                    edges: vec![],
                })
                .unwrap();
        }

        let alice_memories = engine.list_by_entity("alice", 100).unwrap();
        assert_eq!(alice_memories.len(), 5);

        let bob_memories = engine.list_by_entity("bob", 100).unwrap();
        assert_eq!(bob_memories.len(), 3);
    }

    #[test]
    fn list_by_entity_respects_limit() {
        let engine = test_engine();
        for i in 0..10 {
            engine
                .remember(RememberInput {
                    content: format!("memory {}", i),
                    importance: None,
                    context: None,
                    entity_id: Some("entity".to_string()),
                    edges: vec![],
                })
                .unwrap();
        }

        let limited = engine.list_by_entity("entity", 3).unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn schema_version_written_on_first_open() {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 4);
        let _engine = Engine::new_with_params(backend.clone(), embedder, params, 42).unwrap();
        let key = keys::encode_meta_key("schema_version");
        let val = backend.get(ColumnFamilyName::Meta, &key).unwrap().unwrap();
        let version = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn ulids_are_unique() {
        let engine = test_engine();
        let mut ids = std::collections::HashSet::new();
        for i in 0..100 {
            let mem = engine
                .remember(simple_input(&format!("memory {}", i)))
                .unwrap();
            assert!(ids.insert(mem.memory_id), "ULID collision detected");
        }
    }

    #[test]
    fn context_with_structured_data() {
        let engine = test_engine();
        let mut ctx = HashMap::new();
        ctx.insert(
            "tags".to_string(),
            serde_json::json!(["urgent", "follow-up"]),
        );
        ctx.insert("score".to_string(), serde_json::json!(42));

        let memory = engine
            .remember(RememberInput {
                content: "structured context test".to_string(),
                importance: Some(0.9),
                context: Some(ctx.clone()),
                entity_id: None,
                edges: vec![],
            })
            .unwrap();

        let retrieved = engine.get(&memory.memory_id).unwrap();
        let retrieved_ctx = retrieved.context().unwrap();
        assert_eq!(retrieved_ctx, ctx);
    }

    #[test]
    fn timestamps_are_set() {
        let engine = test_engine();
        let before = now_microseconds();
        let memory = engine.remember(simple_input("timestamp test")).unwrap();
        let after = now_microseconds();

        assert!(memory.created_at >= before);
        assert!(memory.created_at <= after);
        assert_eq!(memory.created_at, memory.updated_at);
        assert_eq!(memory.created_at, memory.last_accessed_at);
    }

    #[test]
    fn count_tracks_memories() {
        let engine = test_engine();
        assert_eq!(engine.count().unwrap(), 0);

        for i in 0..5 {
            engine
                .remember(simple_input(&format!("memory {}", i)))
                .unwrap();
        }
        assert_eq!(engine.count().unwrap(), 5);
    }

    #[test]
    fn search_returns_similar_memories() {
        let engine = test_engine();

        // Insert several memories
        let mut ids = Vec::new();
        for i in 0..20 {
            let mem = engine
                .remember(simple_input(&format!("test content number {}", i)))
                .unwrap();
            ids.push(mem);
        }

        // Search with the embedding of the first memory
        let query = ids[0].embedding.as_ref().unwrap();
        let results = engine.search_similar(query, 5, None).unwrap();
        assert_eq!(results.len(), 5);

        // First result should be the same memory (exact match)
        let mut first_id = [0u8; 16];
        first_id.copy_from_slice(&ids[0].memory_id);
        assert_eq!(results[0].0, first_id);
    }

    #[test]
    fn temporal_query_returns_ordered_results() {
        let engine = test_engine();

        for i in 0..10 {
            engine
                .remember(RememberInput {
                    content: format!("temporal memory {}", i),
                    importance: None,
                    context: None,
                    entity_id: Some("entity_t".to_string()),
                    edges: vec![],
                })
                .unwrap();
        }

        let results = engine
            .query_temporal("entity_t", 0, u64::MAX, TemporalOrder::Chronological, 100)
            .unwrap();
        assert_eq!(results.len(), 10);

        // Verify chronological order
        for window in results.windows(2) {
            assert!(window[0].1 <= window[1].1);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    //  Phase 4: Recall Engine Tests
    // ═══════════════════════════════════════════════════════════════════

    // --- Recall validation ---

    #[test]
    fn recall_rejects_empty_cue() {
        let engine = test_engine();
        let input = RecallInput::new("", RecallStrategy::Similarity);
        let err = engine.recall(input).unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    #[test]
    fn recall_rejects_oversized_cue() {
        let engine = test_engine();
        let huge = "x".repeat(MAX_CONTENT_LENGTH + 1);
        let input = RecallInput::new(huge, RecallStrategy::Similarity);
        let err = engine.recall(input).unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    #[test]
    fn recall_rejects_no_strategies() {
        let engine = test_engine();
        let input = RecallInput {
            cue: "test".to_string(),
            strategies: vec![],
            top_k: None,
            entity_id: None,
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
        };
        let err = engine.recall(input).unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    // --- Similarity strategy ---

    #[test]
    fn recall_similarity_returns_results() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..20 {
            engine
                .remember(entity_input(&format!("test content {}", i), "e1"))
                .unwrap();
        }

        let output = engine
            .recall(RecallInput::new(
                "test content 5",
                RecallStrategy::Similarity,
            ))
            .unwrap();

        assert!(!output.results.is_empty());
        assert!(output.strategy_errors.is_empty());
        // Results should be sorted by composite score descending
        for window in output.results.windows(2) {
            assert!(window[0].score >= window[1].score);
        }
    }

    #[test]
    fn recall_similarity_empty_index_returns_empty() {
        let engine = test_engine();
        let output = engine
            .recall(RecallInput::new("nothing here", RecallStrategy::Similarity))
            .unwrap();
        assert!(output.results.is_empty());
        assert!(output.strategy_errors.is_empty());
    }

    #[test]
    fn recall_similarity_respects_top_k() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..50 {
            engine
                .remember(entity_input(&format!("memory number {}", i), "e1"))
                .unwrap();
        }

        let mut input = RecallInput::new("memory number", RecallStrategy::Similarity);
        input.top_k = Some(5);
        let output = engine.recall(input).unwrap();
        assert!(output.results.len() <= 5);
    }

    // --- Temporal strategy ---

    #[test]
    fn recall_temporal_returns_ordered_results() {
        let engine = test_engine();
        for i in 0..10 {
            engine
                .remember(entity_input(&format!("event {}", i), "customer_1"))
                .unwrap();
        }

        let mut input = RecallInput::new("recent events", RecallStrategy::Temporal);
        input.entity_id = Some("customer_1".to_string());
        let output = engine.recall(input).unwrap();

        assert_eq!(output.results.len(), 10);
        assert!(output.strategy_errors.is_empty());
    }

    #[test]
    fn recall_temporal_without_entity_returns_error() {
        let engine = test_engine();
        engine.remember(entity_input("test", "e1")).unwrap();

        let input = RecallInput::new("query", RecallStrategy::Temporal);
        let output = engine.recall(input).unwrap();
        assert_eq!(output.strategy_errors.len(), 1);
        assert_eq!(output.strategy_errors[0].strategy, RecallStrategy::Temporal);
    }

    #[test]
    fn recall_temporal_with_time_range() {
        let engine = test_engine();
        for i in 0..10 {
            engine
                .remember(entity_input(&format!("event {}", i), "e1"))
                .unwrap();
        }

        let now = now_microseconds();
        let mut input = RecallInput::new("events", RecallStrategy::Temporal);
        input.entity_id = Some("e1".to_string());
        input.time_range = Some((0, now));
        let output = engine.recall(input).unwrap();
        assert!(!output.results.is_empty());
    }

    // --- Causal strategy ---

    #[test]
    fn recall_causal_traverses_edges() {
        let engine = test_engine();

        let target = engine
            .remember(entity_input("root cause event", "e1"))
            .unwrap();
        let mut target_id = [0u8; 16];
        target_id.copy_from_slice(&target.memory_id);

        let source = engine
            .remember(RememberInput {
                content: "consequence of root cause".to_string(),
                importance: Some(0.7),
                context: None,
                entity_id: Some("e1".to_string()),
                edges: vec![RememberEdge {
                    target_id,
                    edge_type: EdgeType::CausedBy,
                    confidence: Some(0.9),
                }],
            })
            .unwrap();

        let cue_hex = hex::encode(&source.memory_id);
        let mut input = RecallInput::new(cue_hex, RecallStrategy::Causal);
        input.max_depth = Some(3);
        let output = engine.recall(input).unwrap();

        assert!(!output.results.is_empty());
    }

    #[test]
    fn recall_causal_with_text_seed() {
        let engine = test_engine_with_larger_hnsw();

        let m1 = engine
            .remember(entity_input("budget freeze discussion", "e1"))
            .unwrap();
        let mut m1_id = [0u8; 16];
        m1_id.copy_from_slice(&m1.memory_id);

        engine
            .remember(RememberInput {
                content: "deal lost due to budget freeze".to_string(),
                importance: Some(0.8),
                context: None,
                entity_id: Some("e1".to_string()),
                edges: vec![RememberEdge {
                    target_id: m1_id,
                    edge_type: EdgeType::CausedBy,
                    confidence: Some(0.9),
                }],
            })
            .unwrap();

        let input = RecallInput::new("budget freeze", RecallStrategy::Causal);
        let output = engine.recall(input).unwrap();
        // Should find at least the text-seed memory itself or its connected memories
        assert!(output.strategy_errors.is_empty());
    }

    #[test]
    fn recall_causal_missing_seed_returns_error() {
        let engine = test_engine();
        engine.remember(simple_input("some memory")).unwrap();

        let fake_hex = "00".repeat(16);
        let input = RecallInput::new(fake_hex, RecallStrategy::Causal);
        let output = engine.recall(input).unwrap();
        assert_eq!(output.strategy_errors.len(), 1);
        assert_eq!(output.strategy_errors[0].strategy, RecallStrategy::Causal);
    }

    #[test]
    fn recall_causal_unconnected_seed_returns_empty() {
        let engine = test_engine();
        let mem = engine.remember(simple_input("isolated memory")).unwrap();

        let cue_hex = hex::encode(&mem.memory_id);
        let input = RecallInput::new(cue_hex, RecallStrategy::Causal);
        let output = engine.recall(input).unwrap();
        // An isolated memory has no traversal results (no edges)
        assert!(output.strategy_errors.is_empty());
    }

    // --- Analogical strategy ---

    #[test]
    fn recall_analogical_returns_results() {
        let engine = test_engine_with_larger_hnsw();

        let mut ctx1 = HashMap::new();
        ctx1.insert("stage".to_string(), serde_json::json!("discovery"));
        ctx1.insert("outcome".to_string(), serde_json::json!("positive"));

        for i in 0..20 {
            engine
                .remember(RememberInput {
                    content: format!("sales interaction {}", i),
                    importance: Some(0.7),
                    context: Some(ctx1.clone()),
                    entity_id: Some(format!("customer_{}", i)),
                    edges: vec![],
                })
                .unwrap();
        }

        let mut cue_ctx = HashMap::new();
        cue_ctx.insert("stage".to_string(), serde_json::json!("discovery"));
        cue_ctx.insert("outcome".to_string(), serde_json::json!("negative"));

        let mut input = RecallInput::new("sales interaction", RecallStrategy::Analogical);
        input.cue_context = Some(cue_ctx);
        let output = engine.recall(input).unwrap();

        assert!(!output.results.is_empty());
        assert!(output.strategy_errors.is_empty());
    }

    #[test]
    fn recall_analogical_different_ranking_than_similarity() {
        let engine = test_engine_with_larger_hnsw();

        let mut ctx_discovery = HashMap::new();
        ctx_discovery.insert("stage".to_string(), serde_json::json!("discovery"));
        ctx_discovery.insert("score".to_string(), serde_json::json!(85));

        let mut ctx_negotiation = HashMap::new();
        ctx_negotiation.insert("phase".to_string(), serde_json::json!("closing"));
        ctx_negotiation.insert("amount".to_string(), serde_json::json!(50000));

        for i in 0..15 {
            engine
                .remember(RememberInput {
                    content: format!("discovery phase event {}", i),
                    importance: Some(0.7),
                    context: Some(ctx_discovery.clone()),
                    entity_id: Some(format!("cust_{}", i)),
                    edges: vec![],
                })
                .unwrap();
        }

        for i in 0..15 {
            engine
                .remember(RememberInput {
                    content: format!("negotiation phase event {}", i),
                    importance: Some(0.6),
                    context: Some(ctx_negotiation.clone()),
                    entity_id: Some(format!("cust_{}", i + 100)),
                    edges: vec![],
                })
                .unwrap();
        }

        let mut sim_input = RecallInput::new("discovery event", RecallStrategy::Similarity);
        sim_input.top_k = Some(10);
        let sim_output = engine.recall(sim_input).unwrap();

        let mut ana_input = RecallInput::new("discovery event", RecallStrategy::Analogical);
        ana_input.top_k = Some(10);
        ana_input.cue_context = Some(ctx_discovery.clone());
        let ana_output = engine.recall(ana_input).unwrap();

        // Both should return results
        assert!(!sim_output.results.is_empty());
        assert!(!ana_output.results.is_empty());

        // Analogical results should have StrategyDetail::Analogical
        for result in &ana_output.results {
            assert!(result
                .strategy_details
                .iter()
                .any(|d| matches!(d, StrategyDetail::Analogical { .. })));
        }
    }

    // --- Multi-strategy recall ---

    #[test]
    fn recall_multi_strategy_deduplicates() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..30 {
            engine
                .remember(entity_input(&format!("event number {}", i), "e1"))
                .unwrap();
        }

        let mut input = RecallInput::multi(
            "event number 5",
            vec![RecallStrategy::Similarity, RecallStrategy::Temporal],
        );
        input.entity_id = Some("e1".to_string());
        input.top_k = Some(20);
        let output = engine.recall(input).unwrap();

        // No duplicates
        let ids: Vec<&Vec<u8>> = output.results.iter().map(|r| &r.memory.memory_id).collect();
        let unique: HashSet<&Vec<u8>> = ids.iter().cloned().collect();
        assert_eq!(
            ids.len(),
            unique.len(),
            "multi-strategy recall produced duplicates"
        );
    }

    #[test]
    fn recall_multi_strategy_with_partial_failure() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..10 {
            engine
                .remember(entity_input(&format!("memory {}", i), "e1"))
                .unwrap();
        }

        // Similarity + Temporal. Temporal without entity_id will fail.
        let input = RecallInput::multi(
            "memory query",
            vec![RecallStrategy::Similarity, RecallStrategy::Temporal],
        );
        let output = engine.recall(input).unwrap();

        // Similarity should succeed
        assert!(!output.results.is_empty());
        // Temporal should fail (no entity_id)
        assert_eq!(output.strategy_errors.len(), 1);
        assert_eq!(output.strategy_errors[0].strategy, RecallStrategy::Temporal);
    }

    #[test]
    fn recall_multi_strategy_results_contain_union() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..20 {
            engine
                .remember(entity_input(&format!("event {}", i), "e1"))
                .unwrap();
        }

        let mut sim_input = RecallInput::new("event 10", RecallStrategy::Similarity);
        sim_input.top_k = Some(5);
        let sim_output = engine.recall(sim_input).unwrap();
        let sim_ids: HashSet<Vec<u8>> = sim_output
            .results
            .iter()
            .map(|r| r.memory.memory_id.clone())
            .collect();

        let mut temp_input = RecallInput::new("events", RecallStrategy::Temporal);
        temp_input.entity_id = Some("e1".to_string());
        temp_input.top_k = Some(5);
        let temp_output = engine.recall(temp_input).unwrap();
        let temp_ids: HashSet<Vec<u8>> = temp_output
            .results
            .iter()
            .map(|r| r.memory.memory_id.clone())
            .collect();

        // Multi-strategy with generous top_k
        let mut multi_input = RecallInput::multi(
            "event 10",
            vec![RecallStrategy::Similarity, RecallStrategy::Temporal],
        );
        multi_input.entity_id = Some("e1".to_string());
        multi_input.top_k = Some(20);
        let multi_output = engine.recall(multi_input).unwrap();
        let multi_ids: HashSet<Vec<u8>> = multi_output
            .results
            .iter()
            .map(|r| r.memory.memory_id.clone())
            .collect();

        // Multi-strategy results should contain results from both individual strategies
        for id in &sim_ids {
            assert!(
                multi_ids.contains(id),
                "multi-strategy missing similarity result"
            );
        }
        for id in &temp_ids {
            assert!(
                multi_ids.contains(id),
                "multi-strategy missing temporal result"
            );
        }
    }

    // --- Composite scoring ---

    #[test]
    fn composite_score_monotonic_with_relevance() {
        let mem = Memory {
            memory_id: vec![0u8; 16],
            content: "test".to_string(),
            importance: 0.5,
            context_bytes: Vec::new(),
            entity_id: None,
            embedding: None,
            created_at: now_microseconds(),
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 5,
            decay_score: 0.5,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };
        let weights = ScoringWeights::default();
        let now = now_microseconds();

        let score_low = compute_composite_score(0.2, &mem, &weights, now);
        let score_high = compute_composite_score(0.8, &mem, &weights, now);
        assert!(
            score_high > score_low,
            "score should increase with relevance"
        );
    }

    #[test]
    fn composite_score_monotonic_with_recency() {
        let weights = ScoringWeights::default();
        let now = now_microseconds();

        let recent = Memory {
            memory_id: vec![0u8; 16],
            content: "recent".to_string(),
            importance: 0.5,
            context_bytes: Vec::new(),
            entity_id: None,
            embedding: None,
            created_at: now - 1_000_000, // 1 second ago
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 0,
            decay_score: 0.5,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };

        let old = Memory {
            created_at: now - DEFAULT_MAX_AGE_US + 1_000_000,
            ..recent.clone()
        };

        let score_recent = compute_composite_score(0.5, &recent, &weights, now);
        let score_old = compute_composite_score(0.5, &old, &weights, now);
        assert!(
            score_recent > score_old,
            "recent memory should score higher"
        );
    }

    #[test]
    fn composite_score_zero_weight_recency_independent() {
        let weights = ScoringWeights {
            w_recency: 0.0,
            ..ScoringWeights::default()
        };
        let now = now_microseconds();

        let recent = Memory {
            memory_id: vec![0u8; 16],
            content: "test".to_string(),
            importance: 0.5,
            context_bytes: Vec::new(),
            entity_id: None,
            embedding: None,
            created_at: now - 1_000_000,
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 5,
            decay_score: 0.5,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };

        let old = Memory {
            created_at: now - DEFAULT_MAX_AGE_US / 2,
            ..recent.clone()
        };

        let score_recent = compute_composite_score(0.5, &recent, &weights, now);
        let score_old = compute_composite_score(0.5, &old, &weights, now);
        assert!(
            (score_recent - score_old).abs() < 1e-6,
            "with zero recency weight, age should not affect score"
        );
    }

    // --- Reinforcement ---

    #[test]
    fn recall_updates_access_count() {
        let engine = test_engine_with_larger_hnsw();
        let mem = engine
            .remember(entity_input("reinforcement test", "e1"))
            .unwrap();
        assert_eq!(mem.access_count, 0);

        // Recall to trigger reinforcement
        let output = engine
            .recall(RecallInput::new(
                "reinforcement test",
                RecallStrategy::Similarity,
            ))
            .unwrap();
        assert!(!output.results.is_empty());

        // Verify access_count was updated
        let retrieved = engine.get(&mem.memory_id).unwrap();
        assert_eq!(
            retrieved.access_count, 1,
            "access_count should be 1 after one recall"
        );
        assert!(retrieved.last_accessed_at > mem.last_accessed_at);
    }

    #[test]
    fn recall_reinforcement_increments_sequentially() {
        let engine = test_engine_with_larger_hnsw();
        let mem = engine
            .remember(entity_input("repeated recall test", "e1"))
            .unwrap();

        for expected in 1..=5u64 {
            engine
                .recall(RecallInput::new(
                    "repeated recall test",
                    RecallStrategy::Similarity,
                ))
                .unwrap();
            let retrieved = engine.get(&mem.memory_id).unwrap();
            assert_eq!(
                retrieved.access_count, expected,
                "access_count should be {} after {} recalls",
                expected, expected
            );
        }
    }

    // --- Prime ---

    #[test]
    fn prime_returns_temporal_and_similarity() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..30 {
            engine
                .remember(entity_input(&format!("customer interaction {}", i), "acme"))
                .unwrap();
        }

        let output = engine.prime(PrimeInput::new("acme")).unwrap();
        assert!(!output.results.is_empty());
        assert!(output.temporal_count > 0 || output.similarity_count > 0);
    }

    #[test]
    fn prime_respects_max_memories() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..50 {
            engine
                .remember(entity_input(&format!("interaction {}", i), "acme"))
                .unwrap();
        }

        let mut input = PrimeInput::new("acme");
        input.max_memories = Some(5);
        let output = engine.prime(input).unwrap();
        assert!(output.results.len() <= 5);
    }

    #[test]
    fn prime_with_similarity_cue() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..20 {
            engine
                .remember(entity_input(&format!("pricing discussion {}", i), "acme"))
                .unwrap();
        }

        let mut input = PrimeInput::new("acme");
        input.similarity_cue = Some("pricing negotiation".to_string());
        let output = engine.prime(input).unwrap();
        assert!(!output.results.is_empty());
    }

    #[test]
    fn prime_deduplicates() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..20 {
            engine
                .remember(entity_input(&format!("meeting notes {}", i), "acme"))
                .unwrap();
        }

        let output = engine.prime(PrimeInput::new("acme")).unwrap();
        let ids: Vec<&Vec<u8>> = output.results.iter().map(|r| &r.memory.memory_id).collect();
        let unique: HashSet<&Vec<u8>> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique.len(), "prime() produced duplicates");
    }

    #[test]
    fn prime_rejects_empty_entity() {
        let engine = test_engine();
        let err = engine.prime(PrimeInput::new("")).unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    #[test]
    fn prime_reinforces_memories() {
        let engine = test_engine_with_larger_hnsw();
        let mem = engine
            .remember(entity_input("prime reinforcement", "acme"))
            .unwrap();
        assert_eq!(mem.access_count, 0);

        engine.prime(PrimeInput::new("acme")).unwrap();

        let retrieved = engine.get(&mem.memory_id).unwrap();
        assert!(
            retrieved.access_count >= 1,
            "prime should reinforce accessed memories"
        );
    }

    // --- Structural similarity ---

    #[test]
    fn structural_similarity_matching_contexts() {
        let mut cue_ctx = HashMap::new();
        cue_ctx.insert("stage".to_string(), serde_json::json!("discovery"));
        cue_ctx.insert("outcome".to_string(), serde_json::json!("positive"));

        let mut mem_ctx = HashMap::new();
        mem_ctx.insert("stage".to_string(), serde_json::json!("negotiation"));
        mem_ctx.insert("outcome".to_string(), serde_json::json!("negative"));

        let mem = Memory {
            memory_id: vec![0u8; 16],
            content: "test".to_string(),
            importance: 0.5,
            context_bytes: Memory::serialize_context(&mem_ctx).unwrap(),
            entity_id: None,
            embedding: None,
            created_at: 0,
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 0,
            decay_score: 0.5,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };

        let weights = AnalogicalWeights::default();
        let score = compute_structural_similarity(&cue_ctx, &mem, &weights, None);
        // Same keys, same types (both strings): key_overlap=1.0, type_match=1.0
        // kind_match=0.5 (no kind in cue), entity_pattern=0.5 (both None)
        // = 0.4*1.0 + 0.3*1.0 + 0.2*0.5 + 0.1*0.5 = 0.85
        assert!(
            score > 0.5,
            "matching context structure should score > 0.5, got {}",
            score
        );
    }

    #[test]
    fn structural_similarity_empty_contexts() {
        let cue_ctx = HashMap::new();
        let mem = Memory {
            memory_id: vec![0u8; 16],
            content: "test".to_string(),
            importance: 0.5,
            context_bytes: Vec::new(),
            entity_id: None,
            embedding: None,
            created_at: 0,
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 0,
            decay_score: 0.5,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };

        let weights = AnalogicalWeights::default();
        let score = compute_structural_similarity(&cue_ctx, &mem, &weights, None);
        // key_overlap=0, type_match=0, kind_match=0.5 (no kind in cue), entity_pattern=0.5 (both None)
        // = 0.4*0 + 0.3*0 + 0.2*0.5 + 0.1*0.5 = 0.15
        let expected = 0.2 * 0.5 + 0.1 * 0.5; // 0.15
        assert!(
            (score - expected).abs() < 0.001,
            "empty contexts should score {}, got {}",
            expected,
            score
        );
    }

    #[test]
    fn structural_similarity_kind_mismatch() {
        let mut cue_ctx = HashMap::new();
        cue_ctx.insert("kind".to_string(), serde_json::json!("episode"));

        let mem = Memory {
            memory_id: vec![0u8; 16],
            content: "test".to_string(),
            importance: 0.5,
            context_bytes: Vec::new(),
            entity_id: None,
            embedding: None,
            created_at: 0,
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 0,
            decay_score: 0.5,
            kind: MemoryKind::Insight,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };

        let weights = AnalogicalWeights::default();
        let score_mismatch = compute_structural_similarity(&cue_ctx, &mem, &weights, None);

        // Same test but with matching kind
        let mem_match = Memory {
            kind: MemoryKind::Episode,
            ..mem
        };
        let score_match = compute_structural_similarity(&cue_ctx, &mem_match, &weights, None);

        assert!(
            score_match > score_mismatch,
            "kind match ({}) should score higher than mismatch ({})",
            score_match,
            score_mismatch
        );
    }

    #[test]
    fn structural_similarity_entity_match_vs_mismatch() {
        let cue_ctx = HashMap::new();
        let weights = AnalogicalWeights::default();

        let mem_same = Memory {
            memory_id: vec![0u8; 16],
            content: "test".to_string(),
            importance: 0.5,
            context_bytes: Vec::new(),
            entity_id: Some("proj-alpha".to_string()),
            embedding: None,
            created_at: 0,
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 0,
            decay_score: 0.5,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };

        let mem_diff = Memory {
            entity_id: Some("proj-beta".to_string()),
            ..mem_same.clone()
        };

        let score_same =
            compute_structural_similarity(&cue_ctx, &mem_same, &weights, Some("proj-alpha"));
        let score_diff =
            compute_structural_similarity(&cue_ctx, &mem_diff, &weights, Some("proj-alpha"));

        assert!(
            score_same > score_diff,
            "same entity ({}) should score higher than different entity ({})",
            score_same,
            score_diff
        );
    }

    // --- Bounds ---

    #[test]
    fn recall_top_k_bounded_at_max() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..10 {
            engine.remember(simple_input(&format!("m {}", i))).unwrap();
        }

        let mut input = RecallInput::new("m", RecallStrategy::Similarity);
        input.top_k = Some(MAX_TOP_K + 100);
        let output = engine.recall(input).unwrap();
        // Should not crash; bounded internally
        assert!(output.results.len() <= MAX_TOP_K);
    }

    #[test]
    fn recall_max_depth_bounded_at_max() {
        let engine = test_engine();
        let mem = engine.remember(simple_input("seed")).unwrap();
        let cue_hex = hex::encode(&mem.memory_id);

        let mut input = RecallInput::new(cue_hex, RecallStrategy::Causal);
        input.max_depth = Some(MAX_TRAVERSAL_DEPTH + 5);
        let output = engine.recall(input).unwrap();
        assert!(output.strategy_errors.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════
    //  Phase 5: Revise, Forget, Decay Tests
    // ═══════════════════════════════════════════════════════════════════

    // --- Revise tests ---

    #[test]
    fn revise_basic_content() {
        let engine = test_engine();
        let original = engine
            .remember(entity_input("original content", "e1"))
            .unwrap();

        let revised = engine
            .revise(ReviseInput::new_content(
                original.memory_id.clone(),
                "revised content",
            ))
            .unwrap();

        assert_eq!(revised.memory_id, original.memory_id);
        assert_eq!(revised.content, "revised content");
        assert_eq!(revised.kind, MemoryKind::Revision);
        assert!(revised.updated_at > original.updated_at);
        assert_eq!(revised.logical_clock, original.logical_clock + 1);
    }

    #[test]
    fn revise_preserves_memory_id() {
        let engine = test_engine();
        let original = engine.remember(simple_input("stable id test")).unwrap();

        let revised = engine
            .revise(ReviseInput::new_content(
                original.memory_id.clone(),
                "updated",
            ))
            .unwrap();

        assert_eq!(revised.memory_id, original.memory_id);

        let retrieved = engine.get(&original.memory_id).unwrap();
        assert_eq!(retrieved.content, "updated");
    }

    #[test]
    fn revise_re_embeds_on_content_change() {
        let engine = test_engine();
        let original = engine
            .remember(simple_input("embedding will change"))
            .unwrap();
        let original_embedding = original.embedding.clone();

        let revised = engine
            .revise(ReviseInput::new_content(
                original.memory_id.clone(),
                "completely different content for embedding",
            ))
            .unwrap();

        assert_ne!(revised.embedding, original_embedding);
        assert!(revised.embedding.is_some());
    }

    #[test]
    fn revise_creates_predecessor_snapshot() {
        let engine = test_engine();
        let original = engine
            .remember(entity_input("snapshot test", "e1"))
            .unwrap();
        let original_content = original.content.clone();

        let _revised = engine
            .revise(ReviseInput::new_content(
                original.memory_id.clone(),
                "revised for snapshot",
            ))
            .unwrap();

        // Follow RevisedFrom edges to find snapshot
        let mut mem_id = [0u8; 16];
        mem_id.copy_from_slice(&original.memory_id);
        let outgoing = engine.index_manager().outgoing_edges(&mem_id).unwrap();

        let revised_from_edges: Vec<_> = outgoing
            .iter()
            .filter(|(et, _, _)| *et == EdgeType::RevisedFrom)
            .collect();

        assert_eq!(
            revised_from_edges.len(),
            1,
            "should have exactly one RevisedFrom edge"
        );

        let snapshot = engine.get(&revised_from_edges[0].1).unwrap();
        assert_eq!(snapshot.content, original_content);
    }

    #[test]
    fn revise_snapshot_not_in_similarity_search() {
        let engine = test_engine_with_larger_hnsw();
        let original = engine.remember(simple_input("searchable content")).unwrap();

        let _revised = engine
            .revise(ReviseInput::new_content(
                original.memory_id.clone(),
                "updated searchable content",
            ))
            .unwrap();

        // Search should find the revised memory, not the snapshot
        let results = engine
            .search_similar(original.embedding.as_ref().unwrap(), 10, None)
            .unwrap();

        // The old embedding should not match the revised memory strongly
        // (since content changed), but the snapshot should not appear
        for (id, _) in &results {
            let mem = engine.get(id).unwrap();
            // Verify none of the results are snapshots
            // Snapshots don't have vectors in HNSW, so they shouldn't appear
            assert_ne!(
                mem.content, "searchable content",
                "snapshot should not appear in search results"
            );
        }
    }

    #[test]
    fn revise_chain_creates_multiple_snapshots() {
        let engine = test_engine();
        let original = engine.remember(entity_input("version 1", "e1")).unwrap();

        let _v2 = engine
            .revise(ReviseInput::new_content(
                original.memory_id.clone(),
                "version 2",
            ))
            .unwrap();

        let _v3 = engine
            .revise(ReviseInput::new_content(
                original.memory_id.clone(),
                "version 3",
            ))
            .unwrap();

        let mut mem_id = [0u8; 16];
        mem_id.copy_from_slice(&original.memory_id);
        let outgoing = engine.index_manager().outgoing_edges(&mem_id).unwrap();

        let revised_from_count = outgoing
            .iter()
            .filter(|(et, _, _)| *et == EdgeType::RevisedFrom)
            .count();

        assert_eq!(
            revised_from_count, 2,
            "two revisions should create two RevisedFrom edges"
        );
    }

    #[test]
    fn revise_context_merge() {
        let engine = test_engine();
        let mut ctx = HashMap::new();
        ctx.insert("key1".to_string(), serde_json::json!("val1"));
        ctx.insert("key2".to_string(), serde_json::json!("val2"));

        let original = engine
            .remember(RememberInput {
                content: "context merge test".to_string(),
                importance: Some(0.5),
                context: Some(ctx),
                entity_id: None,
                edges: vec![],
            })
            .unwrap();

        let mut new_ctx = HashMap::new();
        new_ctx.insert("key2".to_string(), serde_json::json!("updated_val2"));
        new_ctx.insert("key3".to_string(), serde_json::json!("val3"));

        let revised = engine
            .revise(ReviseInput {
                memory_id: original.memory_id.clone(),
                content: None,
                importance: None,
                context: Some(new_ctx),
                context_mode: ContextMode::Merge,
                entity_id: None,
                edges: vec![],
            })
            .unwrap();

        let result_ctx = revised.context().unwrap();
        assert_eq!(result_ctx["key1"], serde_json::json!("val1"));
        assert_eq!(result_ctx["key2"], serde_json::json!("updated_val2"));
        assert_eq!(result_ctx["key3"], serde_json::json!("val3"));
    }

    #[test]
    fn revise_context_replace() {
        let engine = test_engine();
        let mut ctx = HashMap::new();
        ctx.insert("old_key".to_string(), serde_json::json!("old_val"));

        let original = engine
            .remember(RememberInput {
                content: "context replace test".to_string(),
                importance: Some(0.5),
                context: Some(ctx),
                entity_id: None,
                edges: vec![],
            })
            .unwrap();

        let mut new_ctx = HashMap::new();
        new_ctx.insert("new_key".to_string(), serde_json::json!("new_val"));

        let revised = engine
            .revise(ReviseInput {
                memory_id: original.memory_id.clone(),
                content: None,
                importance: None,
                context: Some(new_ctx),
                context_mode: ContextMode::Replace,
                entity_id: None,
                edges: vec![],
            })
            .unwrap();

        let result_ctx = revised.context().unwrap();
        assert!(!result_ctx.contains_key("old_key"));
        assert_eq!(result_ctx["new_key"], serde_json::json!("new_val"));
    }

    #[test]
    fn revise_resets_decay_score() {
        let engine = test_engine();
        let original = engine
            .remember(RememberInput {
                content: "decay reset test".to_string(),
                importance: Some(0.7),
                context: None,
                entity_id: None,
                edges: vec![],
            })
            .unwrap();

        let revised = engine
            .revise(ReviseInput {
                memory_id: original.memory_id.clone(),
                content: None,
                importance: Some(0.9),
                context: None,
                context_mode: ContextMode::Merge,
                entity_id: None,
                edges: vec![],
            })
            .unwrap();

        assert_eq!(
            revised.decay_score, 0.9,
            "decay_score should reset to new importance"
        );
    }

    #[test]
    fn revise_rejects_empty_content() {
        let engine = test_engine();
        let mem = engine.remember(simple_input("test")).unwrap();

        let err = engine
            .revise(ReviseInput::new_content(mem.memory_id.clone(), ""))
            .unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    #[test]
    fn revise_rejects_nonexistent_id() {
        let engine = test_engine();
        let fake_id = vec![0u8; 16];
        let err = engine
            .revise(ReviseInput::new_content(fake_id, "new"))
            .unwrap_err();
        assert!(matches!(err, HebbsError::MemoryNotFound { .. }));
    }

    #[test]
    fn revise_rejects_noop() {
        let engine = test_engine();
        let mem = engine.remember(simple_input("test")).unwrap();

        let err = engine
            .revise(ReviseInput {
                memory_id: mem.memory_id.clone(),
                content: None,
                importance: None,
                context: None,
                context_mode: ContextMode::Merge,
                entity_id: None,
                edges: vec![],
            })
            .unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    #[test]
    fn revise_importance_out_of_range() {
        let engine = test_engine();
        let mem = engine.remember(simple_input("test")).unwrap();

        let err = engine
            .revise(ReviseInput {
                memory_id: mem.memory_id.clone(),
                content: None,
                importance: Some(1.5),
                context: None,
                context_mode: ContextMode::Merge,
                entity_id: None,
                edges: vec![],
            })
            .unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    // --- Forget tests ---

    #[test]
    fn forget_by_id() {
        let engine = test_engine();
        let mem = engine.remember(simple_input("will be forgotten")).unwrap();

        let output = engine
            .forget(ForgetCriteria::by_ids(vec![mem.memory_id.clone()]))
            .unwrap();
        assert_eq!(output.forgotten_count, 1);

        let err = engine.get(&mem.memory_id).unwrap_err();
        assert!(matches!(err, HebbsError::MemoryNotFound { .. }));
    }

    #[test]
    fn forget_by_entity() {
        let engine = test_engine();
        for i in 0..5 {
            engine
                .remember(entity_input(&format!("alice {}", i), "alice"))
                .unwrap();
        }
        for i in 0..3 {
            engine
                .remember(entity_input(&format!("bob {}", i), "bob"))
                .unwrap();
        }

        let output = engine.forget(ForgetCriteria::by_entity("alice")).unwrap();
        assert_eq!(output.forgotten_count, 5);

        let alice = engine.list_by_entity("alice", 100).unwrap();
        assert!(alice.is_empty(), "alice's memories should be gone");

        let bob = engine.list_by_entity("bob", 100).unwrap();
        assert_eq!(bob.len(), 3, "bob's memories should be intact");
    }

    #[test]
    fn forget_creates_tombstone() {
        let engine = test_engine();
        let mem = engine.remember(simple_input("tombstone test")).unwrap();

        let output = engine
            .forget(ForgetCriteria::by_ids(vec![mem.memory_id.clone()]))
            .unwrap();
        assert_eq!(output.tombstone_count, 1);

        // Verify tombstone exists in meta CF
        let prefix = crate::forget::tombstone_prefix();
        let tombstones = engine
            .storage()
            .prefix_iterator(ColumnFamilyName::Meta, &prefix)
            .unwrap();
        assert!(!tombstones.is_empty(), "tombstone should exist in meta CF");
    }

    #[test]
    fn forget_removes_from_all_indexes() {
        let engine = test_engine();
        let mem = engine
            .remember(entity_input("indexed forget test", "e1"))
            .unwrap();
        let embedding = mem.embedding.clone().unwrap();

        engine
            .forget(ForgetCriteria::by_ids(vec![mem.memory_id.clone()]))
            .unwrap();

        // Default CF
        assert!(engine.get(&mem.memory_id).is_err());

        // Temporal
        let temporal = engine
            .query_temporal("e1", 0, u64::MAX, TemporalOrder::Chronological, 10)
            .unwrap();
        assert!(temporal.is_empty());

        // HNSW
        let search = engine.search_similar(&embedding, 10, None).unwrap();
        let mut id_arr = [0u8; 16];
        id_arr.copy_from_slice(&mem.memory_id);
        assert!(search.iter().all(|(id, _)| *id != id_arr));
    }

    #[test]
    fn forget_nonexistent_is_noop() {
        let engine = test_engine();
        let fake_id = vec![0u8; 16];
        let output = engine
            .forget(ForgetCriteria::by_ids(vec![fake_id]))
            .unwrap();
        assert_eq!(output.forgotten_count, 0);
    }

    #[test]
    fn forget_empty_criteria_rejected() {
        let engine = test_engine();
        let err = engine
            .forget(ForgetCriteria {
                memory_ids: vec![],
                entity_id: None,
                staleness_threshold_us: None,
                access_count_floor: None,
                memory_kind: None,
                decay_score_floor: None,
            })
            .unwrap_err();
        assert!(matches!(err, HebbsError::InvalidInput { .. }));
    }

    #[test]
    fn forget_cascade_deletes_snapshots() {
        let engine = test_engine();
        let original = engine
            .remember(entity_input("will revise then forget", "e1"))
            .unwrap();

        // Revise to create a snapshot
        let _revised = engine
            .revise(ReviseInput::new_content(
                original.memory_id.clone(),
                "revised before forget",
            ))
            .unwrap();

        // Find the snapshot
        let mut mem_id = [0u8; 16];
        mem_id.copy_from_slice(&original.memory_id);
        let outgoing = engine.index_manager().outgoing_edges(&mem_id).unwrap();
        let snapshot_targets: Vec<[u8; 16]> = outgoing
            .iter()
            .filter(|(et, _, _)| *et == EdgeType::RevisedFrom)
            .map(|(_, target, _)| *target)
            .collect();
        assert_eq!(snapshot_targets.len(), 1);

        // Forget the primary memory
        let output = engine
            .forget(ForgetCriteria::by_ids(vec![original.memory_id.clone()]))
            .unwrap();
        assert_eq!(output.forgotten_count, 1);
        assert_eq!(output.cascade_count, 1);

        // Snapshot should also be gone
        let snap_err = engine.get(&snapshot_targets[0]).unwrap_err();
        assert!(matches!(snap_err, HebbsError::MemoryNotFound { .. }));
    }

    #[test]
    fn forget_batch_limit() {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 4);
        let mut engine = Engine::new_with_params(backend, embedder, params, 42).unwrap();
        engine.set_forget_config(ForgetConfig {
            max_batch_size: 3,
            ..ForgetConfig::default()
        });

        let mut ids = Vec::new();
        for i in 0..10 {
            let mem = engine
                .remember(entity_input(&format!("batch {}", i), "batch_entity"))
                .unwrap();
            ids.push(mem.memory_id);
        }

        let output = engine
            .forget(ForgetCriteria::by_entity("batch_entity"))
            .unwrap();
        assert_eq!(output.forgotten_count, 3);
        assert!(output.truncated);
    }

    // --- Decay score tests (via the public compute function) ---

    #[test]
    fn revise_index_update_search_finds_new_content() {
        let engine = test_engine_with_larger_hnsw();
        let original = engine
            .remember(simple_input("old content for search"))
            .unwrap();

        let revised = engine
            .revise(ReviseInput::new_content(
                original.memory_id.clone(),
                "completely new revised content for search",
            ))
            .unwrap();

        // Search with the new embedding should find the revised memory
        let results = engine
            .search_similar(revised.embedding.as_ref().unwrap(), 5, None)
            .unwrap();
        assert!(!results.is_empty());

        let mut revised_id = [0u8; 16];
        revised_id.copy_from_slice(&revised.memory_id);
        assert_eq!(results[0].0, revised_id);
    }

    // --- Snapshot pruning tests ---

    #[test]
    fn snapshot_pruning_enforces_limit() {
        let mut engine = test_engine();
        engine.set_max_snapshots_per_memory(3);

        let original = engine.remember(entity_input("version 0", "e1")).unwrap();

        // Create 8 revisions (so 8 snapshots total, limit is 3)
        for i in 1..=8 {
            engine
                .revise(ReviseInput::new_content(
                    original.memory_id.clone(),
                    format!("version {}", i),
                ))
                .unwrap();
        }

        let mut mem_id = [0u8; 16];
        mem_id.copy_from_slice(&original.memory_id);
        let snapshots = engine
            .index_manager()
            .outgoing_edges(&mem_id)
            .unwrap()
            .into_iter()
            .filter(|(et, _, _)| *et == EdgeType::RevisedFrom)
            .collect::<Vec<_>>();

        assert_eq!(
            snapshots.len(),
            3,
            "should retain exactly max_snapshots_per_memory snapshots"
        );
    }

    #[test]
    fn snapshot_pruning_removes_oldest() {
        let mut engine = test_engine();
        engine.set_max_snapshots_per_memory(2);

        let original = engine.remember(entity_input("v0", "e1")).unwrap();

        // Build 4 revisions; after each, the newest 2 snapshots should survive
        let mut revision_contents = Vec::new();
        for i in 1..=4 {
            let content = format!("v{}", i);
            engine
                .revise(ReviseInput::new_content(
                    original.memory_id.clone(),
                    content.clone(),
                ))
                .unwrap();
            revision_contents.push(content);
        }

        let mut mem_id = [0u8; 16];
        mem_id.copy_from_slice(&original.memory_id);
        let snapshot_targets: Vec<[u8; 16]> = engine
            .index_manager()
            .outgoing_edges(&mem_id)
            .unwrap()
            .into_iter()
            .filter(|(et, _, _)| *et == EdgeType::RevisedFrom)
            .map(|(_, target, _)| target)
            .collect();

        assert_eq!(snapshot_targets.len(), 2);

        // The surviving snapshots should be the 2 most recent (v2 and v3
        // — snapshots of the state *before* revisions 3 and 4)
        let mut surviving_contents: Vec<String> = snapshot_targets
            .iter()
            .map(|id| engine.get(id).unwrap().content.clone())
            .collect();
        surviving_contents.sort();

        assert!(
            surviving_contents.contains(&"v2".to_string()),
            "v2 snapshot should survive, got: {:?}",
            surviving_contents,
        );
        assert!(
            surviving_contents.contains(&"v3".to_string()),
            "v3 snapshot should survive, got: {:?}",
            surviving_contents,
        );
    }

    #[test]
    fn snapshot_pruning_preserves_memory_record() {
        let mut engine = test_engine();
        engine.set_max_snapshots_per_memory(2);

        let original = engine.remember(entity_input("initial", "e1")).unwrap();

        for i in 1..=5 {
            engine
                .revise(ReviseInput::new_content(
                    original.memory_id.clone(),
                    format!("revision {}", i),
                ))
                .unwrap();
        }

        // The primary memory should still be accessible and have latest content
        let current = engine.get(&original.memory_id).unwrap();
        assert_eq!(current.content, "revision 5");
        assert_eq!(current.memory_id, original.memory_id);
    }

    // --- Lifecycle test ---

    #[test]
    fn full_lifecycle_remember_recall_revise_recall_forget_recall() {
        let engine = test_engine_with_larger_hnsw();

        // Remember
        let mem = engine
            .remember(entity_input("initial sales note", "acme"))
            .unwrap();

        // Recall (found)
        let output = engine
            .recall(RecallInput::new(
                "initial sales note",
                RecallStrategy::Similarity,
            ))
            .unwrap();
        assert!(!output.results.is_empty());

        // Revise
        let revised = engine
            .revise(ReviseInput::new_content(
                mem.memory_id.clone(),
                "updated sales note with new info",
            ))
            .unwrap();
        assert_eq!(revised.content, "updated sales note with new info");

        // Recall (found with new content)
        let output2 = engine
            .recall(RecallInput::new(
                "updated sales note",
                RecallStrategy::Similarity,
            ))
            .unwrap();
        assert!(!output2.results.is_empty());

        // Forget
        let forget_output = engine
            .forget(ForgetCriteria::by_ids(vec![mem.memory_id.clone()]))
            .unwrap();
        assert_eq!(forget_output.forgotten_count, 1);

        // Recall (empty — memory is gone)
        let output3 = engine
            .recall(RecallInput::new(
                "updated sales note",
                RecallStrategy::Similarity,
            ))
            .unwrap();
        // The forgotten memory should not appear
        let forgotten_id = mem.memory_id.clone();
        for result in &output3.results {
            assert_ne!(
                result.memory.memory_id, forgotten_id,
                "forgotten memory should not appear in recall"
            );
        }
    }

    // --- Entity isolation ---

    #[test]
    fn recall_similarity_isolates_entities() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..15 {
            engine
                .remember(entity_input(&format!("alpha memory {}", i), "alpha"))
                .unwrap();
        }
        for i in 0..15 {
            engine
                .remember(entity_input(&format!("beta memory {}", i), "beta"))
                .unwrap();
        }

        let mut input = RecallInput::new("alpha memory", RecallStrategy::Similarity);
        input.entity_id = Some("alpha".to_string());
        input.top_k = Some(10);
        let output = engine.recall(input).unwrap();

        for result in &output.results {
            assert_eq!(
                result.memory.entity_id.as_deref(),
                Some("alpha"),
                "similarity recall leaked a memory from another entity: {:?}",
                result.memory.content
            );
        }
    }

    #[test]
    fn recall_causal_isolates_entities() {
        let engine = test_engine_with_larger_hnsw();

        let alpha = engine
            .remember(entity_input("alpha cause", "alpha"))
            .unwrap();
        let beta = engine
            .remember(entity_input("beta effect", "beta"))
            .unwrap();

        let mut alpha_target = [0u8; 16];
        alpha_target.copy_from_slice(&alpha.memory_id);
        let mut beta_target = [0u8; 16];
        beta_target.copy_from_slice(&beta.memory_id);

        engine
            .remember(RememberInput {
                content: "alpha effect connected".to_string(),
                importance: None,
                context: None,
                entity_id: Some("alpha".to_string()),
                edges: vec![RememberEdge {
                    target_id: alpha_target,
                    edge_type: EdgeType::CausedBy,
                    confidence: None,
                }],
            })
            .unwrap();

        engine
            .remember(RememberInput {
                content: "beta cause connected".to_string(),
                importance: None,
                context: None,
                entity_id: Some("beta".to_string()),
                edges: vec![RememberEdge {
                    target_id: beta_target,
                    edge_type: EdgeType::CausedBy,
                    confidence: None,
                }],
            })
            .unwrap();

        let cue_hex = hex::encode(&alpha.memory_id);
        let mut input = RecallInput::new(&cue_hex, RecallStrategy::Causal);
        input.entity_id = Some("alpha".to_string());
        input.top_k = Some(10);
        let output = engine.recall(input).unwrap();

        for result in &output.results {
            assert_eq!(
                result.memory.entity_id.as_deref(),
                Some("alpha"),
                "causal recall leaked a memory from another entity: {:?}",
                result.memory.content
            );
        }
    }

    #[test]
    fn recall_analogical_isolates_entities() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..15 {
            engine
                .remember(entity_input(&format!("gamma pattern data {}", i), "gamma"))
                .unwrap();
        }
        for i in 0..15 {
            engine
                .remember(entity_input(&format!("delta pattern data {}", i), "delta"))
                .unwrap();
        }

        let mut input = RecallInput::new("gamma pattern", RecallStrategy::Analogical);
        input.entity_id = Some("gamma".to_string());
        input.top_k = Some(10);
        let output = engine.recall(input).unwrap();

        for result in &output.results {
            assert_eq!(
                result.memory.entity_id.as_deref(),
                Some("gamma"),
                "analogical recall leaked a memory from another entity: {:?}",
                result.memory.content
            );
        }
    }

    #[test]
    fn prime_isolates_entities() {
        let engine = test_engine_with_larger_hnsw();
        for i in 0..15 {
            engine
                .remember(entity_input(
                    &format!("epsilon memory for priming {}", i),
                    "epsilon",
                ))
                .unwrap();
        }
        for i in 0..15 {
            engine
                .remember(entity_input(
                    &format!("zeta memory for priming {}", i),
                    "zeta",
                ))
                .unwrap();
        }

        let output = engine.prime(PrimeInput::new("epsilon")).unwrap();

        for result in &output.results {
            assert_eq!(
                result.memory.entity_id.as_deref(),
                Some("epsilon"),
                "prime leaked a memory from another entity: {:?}",
                result.memory.content
            );
        }
    }
}
