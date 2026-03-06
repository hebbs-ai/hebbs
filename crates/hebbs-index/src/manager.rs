use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;

use hebbs_storage::{BatchOperation, ColumnFamilyName, StorageBackend};

use crate::associative::AssociativeIndex;
use crate::error::{IndexError, Result};
use crate::graph::{EdgeMetadata, EdgeType, GraphIndex, TraversalEntry};
use crate::hnsw::{HnswGraph, HnswNode, HnswParams};
use crate::temporal::{TemporalIndex, TemporalOrder};

const DEFAULT_TENANT: &str = "default";

/// Input describing an edge to create during remember().
#[derive(Debug, Clone)]
pub struct EdgeInput {
    pub target_id: [u8; 16],
    pub edge_type: EdgeType,
    pub confidence: f32,
}

/// The unified index manager that coordinates all three indexes.
///
/// ## Responsibilities
///
/// 1. Produce WriteBatch operations for atomic multi-index writes
/// 2. Manage per-tenant in-memory HNSW graphs (insert after commit, tombstone on delete)
/// 3. Expose query methods for each index type
/// 4. Manage HNSW startup recovery from the vectors CF
///
/// ## Multi-tenancy
///
/// Each tenant gets its own HNSW graph, lazily created on first access.
/// Tenant graphs are tracked with last-access timestamps for LRU eviction.
/// Methods without `_for_tenant` suffix operate on the `"default"` tenant.
///
/// ## Thread safety
///
/// Query methods take `&self` and are safe for concurrent use.
/// The per-tenant HNSW map uses `RwLock` for reader-writer separation:
/// - Readers (search): shared read lock on the outer map, then read the graph
/// - Writers (insert, delete): write lock on the outer map to get/create entry
///
/// Temporal and graph queries go directly to RocksDB (thread-safe internally).
///
/// ## Two-phase pattern
///
/// ```text
/// prepare_insert() → Vec<BatchOperation>   [pure computation, no I/O]
/// [caller executes WriteBatch]
/// commit_insert()                          [updates in-memory HNSW]
/// ```
///
/// If the WriteBatch fails, commit_insert is never called. Consistency is maintained.
pub struct IndexManager {
    storage: Arc<dyn StorageBackend>,
    hnsw_params: HnswParams,
    /// Per-tenant HNSW graphs: tenant_id → (graph, last_access_time)
    hnsw_graphs: RwLock<HashMap<String, (HnswGraph, Instant)>>,
    temporal: TemporalIndex,
    graph: GraphIndex,
    /// Per-tenant associative HNSW graphs with Hebbian-learned embeddings.
    pub assoc_index: AssociativeIndex,
}

impl IndexManager {
    /// Create a new IndexManager and rebuild the default tenant's HNSW from the vectors CF.
    ///
    /// Complexity: O(n) scan + O(n * m_avg) neighbor restoration.
    pub fn new(storage: Arc<dyn StorageBackend>, hnsw_params: HnswParams) -> Result<Self> {
        let temporal = TemporalIndex::new(storage.clone());
        let graph = GraphIndex::new(storage.clone());
        let mut hnsw = HnswGraph::new(hnsw_params.clone());

        // Rebuild default tenant's HNSW from vectors CF
        let rebuild_count = Self::rebuild_hnsw(&storage, &mut hnsw)?;

        if rebuild_count > 0 {
            let vector_count = storage
                .prefix_iterator(ColumnFamilyName::Vectors, &[])?
                .len();
            if vector_count != rebuild_count {
                eprintln!(
                    "HNSW rebuild inconsistency: vectors CF has {} entries, rebuilt {} nodes",
                    vector_count, rebuild_count
                );
            }
        }

        let assoc_index = AssociativeIndex::new(storage.clone(), hnsw_params.clone())?;

        let mut graphs = HashMap::new();
        graphs.insert(DEFAULT_TENANT.to_string(), (hnsw, Instant::now()));

        Ok(Self {
            storage,
            hnsw_params,
            hnsw_graphs: RwLock::new(graphs),
            temporal,
            graph,
            assoc_index,
        })
    }

    /// Create with a seeded HNSW for deterministic testing.
    pub fn new_with_seed(
        storage: Arc<dyn StorageBackend>,
        hnsw_params: HnswParams,
        seed: u64,
    ) -> Result<Self> {
        let temporal = TemporalIndex::new(storage.clone());
        let graph = GraphIndex::new(storage.clone());
        let hnsw = HnswGraph::new_with_seed(hnsw_params.clone(), seed);

        let assoc_index =
            AssociativeIndex::new_with_seed(storage.clone(), hnsw_params.clone(), seed)?;

        let mut graphs = HashMap::new();
        graphs.insert(DEFAULT_TENANT.to_string(), (hnsw, Instant::now()));

        Ok(Self {
            storage,
            hnsw_params,
            hnsw_graphs: RwLock::new(graphs),
            temporal,
            graph,
            assoc_index,
        })
    }

    // ─── Prepare/Commit for INSERT ───────────────────────────────────

    /// Prepare WriteBatch operations for inserting a memory into all indexes.
    ///
    /// This is pure computation — no I/O, no side effects.
    /// The caller adds these operations to a WriteBatch alongside the
    /// default CF put for the memory record.
    ///
    /// Returns the HNSW node that will be committed after the WriteBatch succeeds.
    ///
    /// ## Operations produced
    ///
    /// - Temporal CF: `(entity_id, timestamp) → memory_id` (if entity_id present)
    /// - Vectors CF: `memory_id → serialized HNSW node`
    /// - Graph CF: forward + reverse keys for each edge (if edges present)
    pub fn prepare_insert(
        &self,
        memory_id: &[u8; 16],
        embedding: &[f32],
        assoc_embedding: &[f32],
        entity_id: Option<&str>,
        created_at: u64,
        edges: &[EdgeInput],
    ) -> Result<(Vec<BatchOperation>, HnswNode)> {
        let dimensions = self.hnsw_params.dimensions;

        if embedding.len() != dimensions {
            return Err(IndexError::DimensionMismatch {
                expected: dimensions,
                actual: embedding.len(),
            });
        }

        // Debug assertion: verify L2 normalization
        #[cfg(debug_assertions)]
        {
            let norm_sq: f32 = embedding.iter().map(|x| x * x).sum();
            let norm = norm_sq.sqrt();
            debug_assert!(
                (norm - 1.0).abs() < 0.01,
                "embedding not L2-normalized: norm = {}",
                norm
            );
        }

        let mut ops = Vec::new();

        // 1. Temporal CF entry
        if let Some(eid) = entity_id {
            let temporal_key = TemporalIndex::encode_key(eid, created_at);
            ops.push(BatchOperation::Put {
                cf: ColumnFamilyName::Temporal,
                key: temporal_key,
                value: memory_id.to_vec(),
            });
        }

        // 2. Vectors CF entry — create a temporary node for serialization
        // The actual HNSW insertion (with neighbor computation) happens in commit_insert.
        // For persistence, we store the vector and an initial empty node structure.
        // On restart rebuild, the HNSW algorithm re-inserts nodes.
        let temp_node = HnswNode {
            memory_id: *memory_id,
            vector: embedding.to_vec(),
            layer: 0,                // placeholder, updated in commit_insert
            neighbors: vec![vec![]], // placeholder
            deleted: false,
        };

        ops.push(BatchOperation::Put {
            cf: ColumnFamilyName::Vectors,
            key: memory_id.to_vec(),
            value: temp_node.serialize(),
        });

        // 3. VectorsAssociative CF entry
        ops.push(
            self.assoc_index
                .prepare_assoc_insert(memory_id, assoc_embedding),
        );

        // 4. Graph CF entries
        let metadata = EdgeMetadata::new(1.0, created_at);
        let meta_bytes = metadata.to_bytes();

        for edge in edges {
            let fwd_key =
                GraphIndex::encode_forward_key(memory_id, edge.edge_type, &edge.target_id);
            let rev_key =
                GraphIndex::encode_reverse_key(memory_id, edge.edge_type, &edge.target_id);
            let edge_meta = EdgeMetadata::new(edge.confidence, created_at).to_bytes();

            ops.push(BatchOperation::Put {
                cf: ColumnFamilyName::Graph,
                key: fwd_key,
                value: edge_meta.clone(),
            });
            ops.push(BatchOperation::Put {
                cf: ColumnFamilyName::Graph,
                key: rev_key,
                value: edge_meta,
            });
        }

        drop(meta_bytes);
        Ok((ops, temp_node))
    }

    /// Commit the in-memory HNSW insert after the WriteBatch succeeds.
    /// Operates on the `"default"` tenant.
    ///
    /// Complexity: O(log n * ef_construction).
    pub fn commit_insert(&self, memory_id: [u8; 16], embedding: Vec<f32>) -> Result<()> {
        self.commit_insert_for_tenant(DEFAULT_TENANT, memory_id, embedding)
    }

    /// Commit the in-memory HNSW insert for a specific tenant.
    ///
    /// This updates the tenant's in-memory HNSW graph with proper neighbor computation.
    /// After insertion, the node is persisted again with correct neighbor lists.
    ///
    /// Acquires exclusive write lock on the HNSW graphs map.
    ///
    /// Complexity: O(log n * ef_construction).
    pub fn commit_insert_for_tenant(
        &self,
        tenant_id: &str,
        memory_id: [u8; 16],
        embedding: Vec<f32>,
    ) -> Result<()> {
        let node = {
            let mut graphs = self.hnsw_graphs.write();
            let (graph, last_access) = graphs
                .entry(tenant_id.to_string())
                .or_insert_with(|| (HnswGraph::new(self.hnsw_params.clone()), Instant::now()));
            *last_access = Instant::now();
            graph.insert(memory_id, embedding)?
        };

        let serialized = node.serialize();
        self.storage
            .put(ColumnFamilyName::Vectors, &memory_id, &serialized)?;

        Ok(())
    }

    // ─── Associative index methods ────────────────────────────────────

    /// Commit the associative HNSW insert for a specific tenant.
    pub fn commit_assoc_insert_for_tenant(
        &self,
        tenant_id: &str,
        memory_id: [u8; 16],
        assoc_embedding: Vec<f32>,
    ) -> Result<()> {
        self.assoc_index
            .insert_for_tenant(tenant_id, memory_id, assoc_embedding)
    }

    /// Hebbian update for a specific edge type.
    pub fn update_type_offset_from_edge(
        &self,
        edge_type: EdgeType,
        a_source: &[f32],
        a_target: &[f32],
    ) -> Result<()> {
        self.assoc_index
            .update_type_offset(edge_type, a_source, a_target, 0.1)
    }

    // ─── Prepare/Commit for DELETE ───────────────────────────────────

    /// Prepare WriteBatch operations for deleting a memory from all indexes.
    ///
    /// Requires the memory's metadata to reconstruct index keys.
    ///
    /// ## Operations produced
    ///
    /// - Default CF: delete memory record
    /// - Temporal CF: delete `(entity_id, timestamp)` key (if entity_id present)
    /// - Vectors CF: delete HNSW node
    /// - Graph CF: delete all forward + reverse edges touching this memory
    pub fn prepare_delete(
        &self,
        memory_id: &[u8; 16],
        entity_id: Option<&str>,
        created_at: u64,
    ) -> Result<Vec<BatchOperation>> {
        let mut ops = Vec::new();

        // 1. Default CF
        ops.push(BatchOperation::Delete {
            cf: ColumnFamilyName::Default,
            key: memory_id.to_vec(),
        });

        // 2. Temporal CF
        if let Some(eid) = entity_id {
            let temporal_key = TemporalIndex::encode_key(eid, created_at);
            ops.push(BatchOperation::Delete {
                cf: ColumnFamilyName::Temporal,
                key: temporal_key,
            });
        }

        // 3. Vectors CF
        ops.push(BatchOperation::Delete {
            cf: ColumnFamilyName::Vectors,
            key: memory_id.to_vec(),
        });

        // 4. VectorsAssociative CF
        ops.push(self.assoc_index.prepare_delete(memory_id));

        // 5. Graph CF — collect all edge keys touching this memory
        let edge_keys = self.graph.collect_edge_keys_for_delete(memory_id)?;
        for key in edge_keys {
            ops.push(BatchOperation::Delete {
                cf: ColumnFamilyName::Graph,
                key,
            });
        }

        Ok(ops)
    }

    /// Commit the in-memory HNSW delete (tombstone) after the WriteBatch succeeds.
    /// Operates on the `"default"` tenant.
    ///
    /// Complexity: O(1).
    pub fn commit_delete(&self, memory_id: &[u8; 16]) {
        self.commit_delete_for_tenant(DEFAULT_TENANT, memory_id);
    }

    /// Commit the in-memory HNSW delete (tombstone) for a specific tenant.
    ///
    /// Acquires exclusive write lock on the HNSW graphs map.
    ///
    /// Complexity: O(1).
    pub fn commit_delete_for_tenant(&self, tenant_id: &str, memory_id: &[u8; 16]) {
        let mut graphs = self.hnsw_graphs.write();
        if let Some((graph, last_access)) = graphs.get_mut(tenant_id) {
            *last_access = Instant::now();
            graph.mark_deleted(memory_id);
        }
        self.assoc_index
            .commit_delete_for_tenant(tenant_id, memory_id);
    }

    // ─── Prepare/Commit for UPDATE (Phase 5) ────────────────────────

    /// Prepare WriteBatch operations for updating a memory across all indexes.
    ///
    /// Combines a delete of old index entries with an insert of new entries.
    /// Used by `revise()` to atomically re-index a memory after content change.
    ///
    /// ## Operations produced
    ///
    /// - Delete: old temporal CF entry, old vectors CF entry
    /// - Insert: new temporal CF entry, new vectors CF entry
    /// - Insert: graph edges (RevisedFrom + any additional edges)
    ///
    /// The default CF updates (old record, snapshot, updated record) are the
    /// caller's responsibility — this method handles only index-layer operations.
    ///
    /// Complexity: O(1) computation + O(edges) for graph entries.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_update(
        &self,
        memory_id: &[u8; 16],
        old_entity_id: Option<&str>,
        old_created_at: u64,
        new_embedding: &[f32],
        new_entity_id: Option<&str>,
        new_created_at: u64,
        edges: &[EdgeInput],
    ) -> Result<(Vec<BatchOperation>, HnswNode)> {
        let dimensions = self.hnsw_params.dimensions;

        if new_embedding.len() != dimensions {
            return Err(IndexError::DimensionMismatch {
                expected: dimensions,
                actual: new_embedding.len(),
            });
        }

        let mut ops = Vec::new();

        // --- DELETE old index entries ---

        // Old temporal CF entry
        if let Some(eid) = old_entity_id {
            let old_temporal_key = TemporalIndex::encode_key(eid, old_created_at);
            ops.push(BatchOperation::Delete {
                cf: ColumnFamilyName::Temporal,
                key: old_temporal_key,
            });
        }

        // Old vectors CF entry
        ops.push(BatchOperation::Delete {
            cf: ColumnFamilyName::Vectors,
            key: memory_id.to_vec(),
        });

        // --- INSERT new index entries ---

        // New temporal CF entry
        if let Some(eid) = new_entity_id {
            let new_temporal_key = TemporalIndex::encode_key(eid, new_created_at);
            ops.push(BatchOperation::Put {
                cf: ColumnFamilyName::Temporal,
                key: new_temporal_key,
                value: memory_id.to_vec(),
            });
        }

        // New vectors CF entry (placeholder, updated after HNSW commit)
        let temp_node = HnswNode {
            memory_id: *memory_id,
            vector: new_embedding.to_vec(),
            layer: 0,
            neighbors: vec![vec![]],
            deleted: false,
        };

        ops.push(BatchOperation::Put {
            cf: ColumnFamilyName::Vectors,
            key: memory_id.to_vec(),
            value: temp_node.serialize(),
        });

        // --- Graph edges ---
        for edge in edges {
            let fwd_key =
                GraphIndex::encode_forward_key(memory_id, edge.edge_type, &edge.target_id);
            let rev_key =
                GraphIndex::encode_reverse_key(memory_id, edge.edge_type, &edge.target_id);
            let edge_meta = EdgeMetadata::new(edge.confidence, new_created_at).to_bytes();

            ops.push(BatchOperation::Put {
                cf: ColumnFamilyName::Graph,
                key: fwd_key,
                value: edge_meta.clone(),
            });
            ops.push(BatchOperation::Put {
                cf: ColumnFamilyName::Graph,
                key: rev_key,
                value: edge_meta,
            });
        }

        Ok((ops, temp_node))
    }

    // ─── Query methods ───────────────────────────────────────────────

    /// HNSW top-K nearest neighbor search.
    /// Operates on the `"default"` tenant.
    ///
    /// Complexity: O(log n * ef_search).
    pub fn search_vector(
        &self,
        query: &[f32],
        k: usize,
        ef_search: Option<usize>,
    ) -> Result<Vec<([u8; 16], f32)>> {
        self.search_vector_for_tenant(DEFAULT_TENANT, query, k, ef_search)
    }

    /// HNSW top-K nearest neighbor search for a specific tenant.
    ///
    /// Returns `(memory_id, distance)` pairs sorted by distance ascending.
    /// Tombstoned nodes are excluded. Lazily creates the tenant's graph if
    /// it doesn't exist yet.
    ///
    /// Complexity: O(log n * ef_search).
    pub fn search_vector_for_tenant(
        &self,
        tenant_id: &str,
        query: &[f32],
        k: usize,
        ef_search: Option<usize>,
    ) -> Result<Vec<([u8; 16], f32)>> {
        {
            let graphs = self.hnsw_graphs.read();
            if let Some((graph, _)) = graphs.get(tenant_id) {
                let result = graph.search(query, k, ef_search);
                drop(graphs);
                // Update last-access time outside the read lock
                let mut graphs_w = self.hnsw_graphs.write();
                if let Some((_, last_access)) = graphs_w.get_mut(tenant_id) {
                    *last_access = Instant::now();
                }
                return result;
            }
        }
        // Tenant not found — create an empty graph
        let mut graphs = self.hnsw_graphs.write();
        let (graph, _) = graphs
            .entry(tenant_id.to_string())
            .or_insert_with(|| (HnswGraph::new(self.hnsw_params.clone()), Instant::now()));
        graph.search(query, k, ef_search)
    }

    /// Temporal range query.
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
        self.temporal
            .query_range(entity_id, start_us, end_us, order, limit)
    }

    /// Graph bounded traversal.
    ///
    /// Returns connected memories up to `max_depth` hops with cycle detection.
    ///
    /// Complexity: O(branching_factor^max_depth).
    pub fn traverse(
        &self,
        seed_id: &[u8; 16],
        edge_types: &[EdgeType],
        max_depth: usize,
        max_results: usize,
    ) -> Result<(Vec<TraversalEntry>, bool)> {
        self.graph
            .traverse(seed_id, edge_types, max_depth, max_results)
    }

    /// Get outgoing edges from a memory.
    pub fn outgoing_edges(
        &self,
        memory_id: &[u8; 16],
    ) -> Result<Vec<(EdgeType, [u8; 16], EdgeMetadata)>> {
        self.graph.outgoing_edges(memory_id)
    }

    /// Get incoming edges to a memory.
    pub fn incoming_edges(
        &self,
        memory_id: &[u8; 16],
    ) -> Result<Vec<(EdgeType, [u8; 16], EdgeMetadata)>> {
        self.graph.incoming_edges(memory_id)
    }

    // ─── Status and maintenance ──────────────────────────────────────

    /// Number of nodes in the default tenant's HNSW graph (including tombstones).
    pub fn hnsw_node_count(&self) -> usize {
        let graphs = self.hnsw_graphs.read();
        graphs.get(DEFAULT_TENANT).map_or(0, |(g, _)| g.len())
    }

    /// Number of active (non-tombstoned) nodes in the default tenant's HNSW.
    pub fn hnsw_active_count(&self) -> usize {
        let graphs = self.hnsw_graphs.read();
        graphs
            .get(DEFAULT_TENANT)
            .map_or(0, |(g, _)| g.active_count())
    }

    /// Number of tombstoned nodes in the default tenant's HNSW.
    pub fn hnsw_tombstone_count(&self) -> usize {
        let graphs = self.hnsw_graphs.read();
        graphs
            .get(DEFAULT_TENANT)
            .map_or(0, |(g, _)| g.tombstone_count())
    }

    /// Check if the default tenant's HNSW tombstone cleanup should be triggered.
    pub fn hnsw_needs_cleanup(&self) -> bool {
        let graphs = self.hnsw_graphs.read();
        graphs
            .get(DEFAULT_TENANT)
            .is_some_and(|(g, _)| g.needs_cleanup(0.1))
    }

    /// Run HNSW tombstone cleanup on the default tenant. Returns count of removed nodes.
    pub fn hnsw_cleanup(&self) -> usize {
        let mut graphs = self.hnsw_graphs.write();
        graphs
            .get_mut(DEFAULT_TENANT)
            .map_or(0, |(g, _)| g.cleanup_tombstones())
    }

    /// Number of currently loaded tenant HNSW graphs.
    pub fn loaded_tenant_count(&self) -> usize {
        self.hnsw_graphs.read().len()
    }

    /// Reference to the underlying storage.
    pub fn storage(&self) -> &dyn StorageBackend {
        self.storage.as_ref()
    }

    /// Reference to the shared HNSW params.
    pub fn hnsw_params(&self) -> &HnswParams {
        &self.hnsw_params
    }

    // ─── LRU eviction ─────────────────────────────────────────────────

    /// Evict idle tenant HNSW graphs from memory.
    ///
    /// - Tenants idle longer than `max_idle_secs` are candidates for eviction.
    /// - If the total loaded count exceeds `max_loaded`, the least recently used
    ///   tenants are evicted until the count is within bounds.
    /// - The `"default"` tenant is never evicted.
    ///
    /// Returns the number of tenants evicted.
    pub fn evict_idle_tenants(&self, max_idle_secs: u64, max_loaded: usize) -> usize {
        let mut graphs = self.hnsw_graphs.write();
        let now = Instant::now();
        let idle_threshold = std::time::Duration::from_secs(max_idle_secs);

        // Phase 1: evict tenants that exceeded the idle timeout
        let idle_keys: Vec<String> = graphs
            .iter()
            .filter(|(k, (_, last_access))| {
                k.as_str() != DEFAULT_TENANT && now.duration_since(*last_access) > idle_threshold
            })
            .map(|(k, _)| k.clone())
            .collect();

        let mut evicted = 0;
        for key in &idle_keys {
            graphs.remove(key);
            evicted += 1;
        }

        // Phase 2: if still over max_loaded, evict LRU (excluding default)
        if graphs.len() > max_loaded {
            let mut candidates: Vec<(String, Instant)> = graphs
                .iter()
                .filter(|(k, _)| k.as_str() != DEFAULT_TENANT)
                .map(|(k, (_, t))| (k.clone(), *t))
                .collect();
            candidates.sort_by_key(|(_, t)| *t);

            let to_evict = graphs.len() - max_loaded;
            for (key, _) in candidates.into_iter().take(to_evict) {
                graphs.remove(&key);
                evicted += 1;
            }
        }

        evicted
    }

    // ─── Startup recovery ────────────────────────────────────────────

    /// Rebuild the in-memory HNSW from the vectors CF.
    ///
    /// Scans all entries in the vectors CF, deserializes each node,
    /// and re-inserts it into the HNSW graph with the stored vector.
    /// Neighbor lists from persistence are NOT restored — the HNSW
    /// insert algorithm computes fresh neighbor connections.
    ///
    /// This ensures the in-memory graph is correct even if the persisted
    /// neighbor lists were stale (e.g., due to crash between WriteBatch
    /// commit and in-memory update).
    ///
    /// Complexity: O(n * log n * ef_construction) for full re-insert.
    fn rebuild_hnsw(storage: &Arc<dyn StorageBackend>, hnsw: &mut HnswGraph) -> Result<usize> {
        let entries = storage.prefix_iterator(ColumnFamilyName::Vectors, &[])?;

        if entries.is_empty() {
            return Ok(0);
        }

        let dimensions = hnsw.params().dimensions;
        let mut count = 0;

        for (key, value) in &entries {
            if key.len() != 16 {
                continue;
            }

            let mut memory_id = [0u8; 16];
            memory_id.copy_from_slice(key);

            match HnswNode::deserialize(memory_id, value, dimensions) {
                Ok(node) => {
                    // Re-insert with the HNSW algorithm for correct neighbor computation
                    if let Err(e) = hnsw.insert(memory_id, node.vector) {
                        eprintln!(
                            "HNSW rebuild: failed to insert node {}: {}",
                            hex_id(&memory_id),
                            e
                        );
                        continue;
                    }
                    count += 1;
                }
                Err(e) => {
                    eprintln!(
                        "HNSW rebuild: failed to deserialize node {}: {}",
                        hex_id(&memory_id),
                        e
                    );
                }
            }
        }

        Ok(count)
    }
}

fn hex_id(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hnsw::HnswParams;
    use hebbs_storage::InMemoryBackend;
    use rand::Rng;

    fn test_manager(dims: usize) -> IndexManager {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(dims, 4);
        IndexManager::new_with_seed(storage, params, 42).unwrap()
    }

    fn normalized_vec(dims: usize, seed: u64) -> Vec<f32> {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut v: Vec<f32> = (0..dims).map(|_| rng.gen_range(-1.0..1.0)).collect();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }

    fn make_id(val: u8) -> [u8; 16] {
        [val; 16]
    }

    #[test]
    fn prepare_insert_produces_correct_ops() {
        let mgr = test_manager(8);
        let id = make_id(1);
        let embedding = normalized_vec(8, 1);

        let (ops, _node) = mgr
            .prepare_insert(&id, &embedding, &embedding, Some("entity_1"), 1000, &[])
            .unwrap();

        // Should have: temporal CF put + vectors CF put + assoc CF put
        assert_eq!(ops.len(), 3);
        let cfs: Vec<ColumnFamilyName> = ops
            .iter()
            .map(|op| match op {
                BatchOperation::Put { cf, .. } => *cf,
                BatchOperation::Delete { cf, .. } => *cf,
            })
            .collect();
        assert!(cfs.contains(&ColumnFamilyName::Temporal));
        assert!(cfs.contains(&ColumnFamilyName::Vectors));
    }

    #[test]
    fn prepare_insert_without_entity_id_skips_temporal() {
        let mgr = test_manager(8);
        let id = make_id(1);
        let embedding = normalized_vec(8, 1);

        let (ops, _) = mgr
            .prepare_insert(&id, &embedding, &embedding, None, 1000, &[])
            .unwrap();

        // vectors CF + assoc CF
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn prepare_insert_with_edges() {
        let mgr = test_manager(8);
        let id = make_id(1);
        let embedding = normalized_vec(8, 1);
        let edges = vec![
            EdgeInput {
                target_id: make_id(2),
                edge_type: EdgeType::CausedBy,
                confidence: 0.9,
            },
            EdgeInput {
                target_id: make_id(3),
                edge_type: EdgeType::RelatedTo,
                confidence: 0.8,
            },
        ];

        let (ops, _) = mgr
            .prepare_insert(&id, &embedding, &embedding, Some("e1"), 1000, &edges)
            .unwrap();

        // temporal(1) + vectors(1) + assoc(1) + edges(2 * 2 forward+reverse) = 7
        assert_eq!(ops.len(), 7);
    }

    #[test]
    fn full_insert_lifecycle() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(16, 4);
        let mgr = IndexManager::new_with_seed(storage.clone(), params, 42).unwrap();

        let id = make_id(1);
        let embedding = normalized_vec(16, 1);

        // Prepare
        let (ops, _) = mgr
            .prepare_insert(&id, &embedding, &embedding, Some("entity_1"), 1000, &[])
            .unwrap();

        // Execute WriteBatch (plus default CF put, which the engine handles)
        storage.write_batch(&ops).unwrap();

        // Commit in-memory HNSW
        mgr.commit_insert(id, embedding.clone()).unwrap();

        // Verify search works
        let results = mgr.search_vector(&embedding, 1, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id);

        // Verify temporal query works
        let temporal = mgr
            .query_temporal("entity_1", 0, u64::MAX, TemporalOrder::Chronological, 10)
            .unwrap();
        assert_eq!(temporal.len(), 1);
        assert_eq!(temporal[0].0, id.to_vec());
    }

    #[test]
    fn full_delete_lifecycle() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(16, 4);
        let mgr = IndexManager::new_with_seed(storage.clone(), params, 42).unwrap();

        // Insert
        let id = make_id(1);
        let embedding = normalized_vec(16, 1);
        let (ops, _) = mgr
            .prepare_insert(&id, &embedding, &embedding, Some("entity_1"), 1000, &[])
            .unwrap();

        // Also write to default CF (simulating engine)
        let mut all_ops = vec![BatchOperation::Put {
            cf: ColumnFamilyName::Default,
            key: id.to_vec(),
            value: b"memory_data".to_vec(),
        }];
        all_ops.extend(ops);
        storage.write_batch(&all_ops).unwrap();
        mgr.commit_insert(id, embedding.clone()).unwrap();

        // Delete
        let delete_ops = mgr.prepare_delete(&id, Some("entity_1"), 1000).unwrap();
        storage.write_batch(&delete_ops).unwrap();
        mgr.commit_delete(&id);

        // Verify memory removed from default CF
        assert!(storage
            .get(ColumnFamilyName::Default, &id)
            .unwrap()
            .is_none());

        // Verify removed from temporal
        let temporal = mgr
            .query_temporal("entity_1", 0, u64::MAX, TemporalOrder::Chronological, 10)
            .unwrap();
        assert!(temporal.is_empty());

        // Verify removed from HNSW search results
        let results = mgr.search_vector(&embedding, 1, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn multiple_inserts_and_search() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(16, 4);
        let mgr = IndexManager::new_with_seed(storage.clone(), params, 42).unwrap();

        for i in 0..50u8 {
            let id = make_id(i);
            let embedding = normalized_vec(16, i as u64);

            let (ops, _) = mgr
                .prepare_insert(
                    &id,
                    &embedding,
                    &embedding,
                    Some("entity"),
                    i as u64 * 100,
                    &[],
                )
                .unwrap();
            storage.write_batch(&ops).unwrap();
            mgr.commit_insert(id, embedding).unwrap();
        }

        // Search for vector similar to node 25
        let query = normalized_vec(16, 25);
        let results = mgr.search_vector(&query, 5, None).unwrap();
        assert_eq!(results.len(), 5);
        assert_eq!(results[0].0, make_id(25)); // exact match should be top-1

        // Temporal query
        let temporal = mgr
            .query_temporal("entity", 0, u64::MAX, TemporalOrder::Chronological, 100)
            .unwrap();
        assert_eq!(temporal.len(), 50);

        // Verify chronological order
        for window in temporal.windows(2) {
            assert!(window[0].1 <= window[1].1);
        }
    }

    #[test]
    fn dimension_mismatch_rejected() {
        let mgr = test_manager(8);
        let id = make_id(1);
        let wrong_dims = vec![1.0; 16]; // 16 dims, expect 8

        let result = mgr.prepare_insert(&id, &wrong_dims, &wrong_dims, None, 1000, &[]);
        assert!(matches!(result, Err(IndexError::DimensionMismatch { .. })));
    }

    #[test]
    fn hnsw_status_tracking() {
        let mgr = test_manager(8);

        assert_eq!(mgr.hnsw_node_count(), 0);
        assert_eq!(mgr.hnsw_active_count(), 0);

        // Insert 5 nodes
        let storage = mgr.storage.clone();
        for i in 0..5u8 {
            let id = make_id(i);
            let embedding = normalized_vec(8, i as u64);
            let (ops, _) = mgr
                .prepare_insert(&id, &embedding, &embedding, None, 1000, &[])
                .unwrap();
            storage.write_batch(&ops).unwrap();
            mgr.commit_insert(id, embedding).unwrap();
        }

        assert_eq!(mgr.hnsw_node_count(), 5);
        assert_eq!(mgr.hnsw_active_count(), 5);
        assert_eq!(mgr.hnsw_tombstone_count(), 0);

        // Delete 1
        mgr.commit_delete(&make_id(2));
        assert_eq!(mgr.hnsw_node_count(), 5);
        assert_eq!(mgr.hnsw_active_count(), 4);
        assert_eq!(mgr.hnsw_tombstone_count(), 1);
    }

    #[test]
    fn rebuild_from_empty_vectors_cf() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(8, 4);
        let mgr = IndexManager::new(storage, params).unwrap();
        assert_eq!(mgr.hnsw_node_count(), 0);
    }

    #[test]
    fn rebuild_from_persisted_vectors() {
        let storage = Arc::new(InMemoryBackend::new());
        let dims = 16;
        let params = HnswParams::with_m(dims, 4);

        // Phase 1: Insert some nodes
        {
            let mgr = IndexManager::new_with_seed(storage.clone(), params.clone(), 42).unwrap();
            for i in 0..20u8 {
                let id = make_id(i);
                let embedding = normalized_vec(dims, i as u64);
                let (ops, _) = mgr
                    .prepare_insert(&id, &embedding, &embedding, None, 1000, &[])
                    .unwrap();
                storage.write_batch(&ops).unwrap();
                mgr.commit_insert(id, embedding).unwrap();
            }
        }

        // Phase 2: Create a new IndexManager (simulates restart)
        let mgr2 = IndexManager::new(storage, params).unwrap();

        // Verify all 20 nodes were rebuilt
        assert_eq!(mgr2.hnsw_node_count(), 20);

        // Verify search still works
        let query = normalized_vec(dims, 10);
        let results = mgr2.search_vector(&query, 1, Some(50)).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, make_id(10));
    }

    // ─── Multi-tenant tests ──────────────────────────────────────────

    #[test]
    fn tenant_isolation() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(16, 4);
        let mgr = IndexManager::new_with_seed(storage.clone(), params, 42).unwrap();

        let id_a = make_id(1);
        let id_b = make_id(2);
        let emb_a = normalized_vec(16, 1);
        let emb_b = normalized_vec(16, 2);

        mgr.commit_insert_for_tenant("tenant_a", id_a, emb_a.clone())
            .unwrap();
        mgr.commit_insert_for_tenant("tenant_b", id_b, emb_b.clone())
            .unwrap();

        let results_a = mgr
            .search_vector_for_tenant("tenant_a", &emb_a, 10, None)
            .unwrap();
        assert_eq!(results_a.len(), 1);
        assert_eq!(results_a[0].0, id_a);

        let results_b = mgr
            .search_vector_for_tenant("tenant_b", &emb_b, 10, None)
            .unwrap();
        assert_eq!(results_b.len(), 1);
        assert_eq!(results_b[0].0, id_b);

        // Default tenant should be empty (no inserts to it)
        let results_default = mgr.search_vector(&emb_a, 10, None).unwrap();
        assert!(results_default.is_empty());
    }

    #[test]
    fn tenant_lazy_creation() {
        let mgr = test_manager(8);
        assert_eq!(mgr.loaded_tenant_count(), 1); // only "default"

        let query = normalized_vec(8, 1);
        let results = mgr
            .search_vector_for_tenant("new_tenant", &query, 5, None)
            .unwrap();
        assert!(results.is_empty());

        assert_eq!(mgr.loaded_tenant_count(), 2); // "default" + "new_tenant"
    }

    #[test]
    fn tenant_commit_delete() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(16, 4);
        let mgr = IndexManager::new_with_seed(storage.clone(), params, 42).unwrap();

        let id = make_id(1);
        let emb = normalized_vec(16, 1);

        mgr.commit_insert_for_tenant("t1", id, emb.clone()).unwrap();

        let results = mgr.search_vector_for_tenant("t1", &emb, 1, None).unwrap();
        assert_eq!(results.len(), 1);

        mgr.commit_delete_for_tenant("t1", &id);

        let results = mgr.search_vector_for_tenant("t1", &emb, 1, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn evict_idle_tenants_by_timeout() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(8, 4);
        let mgr = IndexManager::new_with_seed(storage, params, 42).unwrap();

        let emb = normalized_vec(8, 1);
        mgr.commit_insert_for_tenant("t1", make_id(1), emb.clone())
            .unwrap();
        mgr.commit_insert_for_tenant("t2", make_id(2), emb.clone())
            .unwrap();

        assert_eq!(mgr.loaded_tenant_count(), 3); // default + t1 + t2

        // Backdate t1's last access to simulate idle time
        {
            let mut graphs = mgr.hnsw_graphs.write();
            if let Some((_, last_access)) = graphs.get_mut("t1") {
                *last_access = Instant::now() - std::time::Duration::from_secs(120);
            }
        }

        let evicted = mgr.evict_idle_tenants(60, usize::MAX);
        assert_eq!(evicted, 1);
        assert_eq!(mgr.loaded_tenant_count(), 2); // default + t2
    }

    #[test]
    fn evict_idle_tenants_by_max_loaded() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(8, 4);
        let mgr = IndexManager::new_with_seed(storage, params, 42).unwrap();

        let emb = normalized_vec(8, 1);
        for i in 0..5u8 {
            let tenant = format!("t{}", i);
            mgr.commit_insert_for_tenant(&tenant, make_id(i), emb.clone())
                .unwrap();
        }

        assert_eq!(mgr.loaded_tenant_count(), 6); // default + t0..t4

        // max_loaded=3 should evict 3 tenants (keeping default + 2 most recent)
        let evicted = mgr.evict_idle_tenants(u64::MAX, 3);
        assert_eq!(evicted, 3);
        assert_eq!(mgr.loaded_tenant_count(), 3);

        // Default should still be present
        assert_eq!(mgr.hnsw_node_count(), 0);
    }

    #[test]
    fn default_tenant_never_evicted() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(8, 4);
        let mgr = IndexManager::new_with_seed(storage, params, 42).unwrap();

        // Backdate default tenant
        {
            let mut graphs = mgr.hnsw_graphs.write();
            if let Some((_, last_access)) = graphs.get_mut(DEFAULT_TENANT) {
                *last_access = Instant::now() - std::time::Duration::from_secs(9999);
            }
        }

        let evicted = mgr.evict_idle_tenants(0, 0);
        assert_eq!(evicted, 0);
        assert_eq!(mgr.loaded_tenant_count(), 1); // default survives
    }

    #[test]
    fn hnsw_params_accessor() {
        let mgr = test_manager(8);
        assert_eq!(mgr.hnsw_params().dimensions, 8);
        assert_eq!(mgr.hnsw_params().m, 4);
    }

    #[test]
    fn default_methods_delegate_to_for_tenant() {
        let storage = Arc::new(InMemoryBackend::new());
        let params = HnswParams::with_m(16, 4);
        let mgr = IndexManager::new_with_seed(storage.clone(), params, 42).unwrap();

        let id = make_id(1);
        let emb = normalized_vec(16, 1);

        // commit_insert (default) should land in "default" tenant
        mgr.commit_insert(id, emb.clone()).unwrap();

        // search_vector (default) should find it
        let results = mgr.search_vector(&emb, 1, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id);

        // search_vector_for_tenant("default") should also find it
        let results2 = mgr
            .search_vector_for_tenant("default", &emb, 1, None)
            .unwrap();
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].0, id);

        // commit_delete (default) should tombstone it
        mgr.commit_delete(&id);
        let results3 = mgr.search_vector(&emb, 1, None).unwrap();
        assert!(results3.is_empty());
    }
}
