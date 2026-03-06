use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;

use hebbs_storage::{BatchOperation, ColumnFamilyName, StorageBackend};

use crate::error::Result;
use crate::graph::EdgeType;
use crate::hnsw::{HnswGraph, HnswNode, HnswParams};

const DEFAULT_TENANT: &str = "default";
#[cfg(test)]
const HEBBIAN_LR: f32 = 0.1;
const TYPE_OFFSETS_META_KEY: &[u8] = b"hebbian:type_offsets";

/// L2-normalize a vector in place. Returns the norm.
fn l2_normalize(v: &mut [f32]) -> f32 {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    norm
}

/// L2-normalize a slice into a new Vec.
fn normalize_vec(v: &[f32]) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        v.iter().map(|x| x / norm).collect()
    } else {
        v.to_vec()
    }
}

/// Manages per-tenant HNSW graphs for associative (Hebbian) embeddings,
/// plus per-edge-type offset vectors learned from edge creation.
///
/// ## Design
///
/// Each memory has a second embedding `a_i` (associative) that starts equal
/// to its content embedding and evolves via Hebbian updates as edges are created.
///
/// Per-type offset vectors are learned from edges:
/// - When edge source→target is created: `offset[type] += lr * (a_target - a_source)`, then normalize.
///
/// Bidirectional causal traversal without BFS:
/// - Forward: query `normalize(a_seed + offset[type])` → finds typical targets
/// - Backward: query `normalize(a_seed - offset[type])` → finds typical sources
///
/// Analogy: given A:B::C:?
/// - `dir = normalize(a_B - a_A)`
/// - `target = normalize(a_C + dir)`
/// - Search assoc HNSW for `target`
pub struct AssociativeIndex {
    hnsw_params: HnswParams,
    /// Per-tenant associative HNSW graphs: tenant_id → (graph, last_access_time)
    assoc_graphs: RwLock<HashMap<String, (HnswGraph, Instant)>>,
    /// Learned per-type offset vectors.
    type_offsets: RwLock<HashMap<EdgeType, Vec<f32>>>,
    storage: Arc<dyn StorageBackend>,
}

impl AssociativeIndex {
    /// Create a new AssociativeIndex, rebuilding the default tenant's graph
    /// from the VectorsAssociative CF and loading type offsets from Meta CF.
    pub fn new(storage: Arc<dyn StorageBackend>, hnsw_params: HnswParams) -> Result<Self> {
        let type_offsets = Self::load_type_offsets(&storage, hnsw_params.dimensions)?;

        let mut default_graph = HnswGraph::new(hnsw_params.clone());
        let rebuild_count = Self::rebuild_assoc_hnsw(&storage, &mut default_graph, &hnsw_params)?;
        let _ = rebuild_count;

        let mut graphs = HashMap::new();
        graphs.insert(DEFAULT_TENANT.to_string(), (default_graph, Instant::now()));

        Ok(Self {
            hnsw_params,
            assoc_graphs: RwLock::new(graphs),
            type_offsets: RwLock::new(type_offsets),
            storage,
        })
    }

    /// Create with a fixed seed for deterministic testing.
    pub fn new_with_seed(
        storage: Arc<dyn StorageBackend>,
        hnsw_params: HnswParams,
        seed: u64,
    ) -> Result<Self> {
        let type_offsets = Self::load_type_offsets(&storage, hnsw_params.dimensions)?;

        let default_graph = HnswGraph::new_with_seed(hnsw_params.clone(), seed);

        let mut graphs = HashMap::new();
        graphs.insert(DEFAULT_TENANT.to_string(), (default_graph, Instant::now()));

        Ok(Self {
            hnsw_params,
            assoc_graphs: RwLock::new(graphs),
            type_offsets: RwLock::new(type_offsets),
            storage,
        })
    }

    /// Prepare a BatchOperation to persist the associative embedding to VectorsAssociative CF.
    ///
    /// This is pure computation — no I/O, no HNSW insertion.
    pub fn prepare_assoc_insert(
        &self,
        memory_id: &[u8; 16],
        assoc_embedding: &[f32],
    ) -> BatchOperation {
        let temp_node = HnswNode {
            memory_id: *memory_id,
            vector: assoc_embedding.to_vec(),
            layer: 0,
            neighbors: vec![vec![]],
            deleted: false,
        };
        BatchOperation::Put {
            cf: ColumnFamilyName::VectorsAssociative,
            key: memory_id.to_vec(),
            value: temp_node.serialize(),
        }
    }

    /// Commit the in-memory assoc HNSW insert for a specific tenant.
    ///
    /// Called after WriteBatch succeeds.
    pub fn insert_for_tenant(
        &self,
        tenant_id: &str,
        memory_id: [u8; 16],
        assoc_embedding: Vec<f32>,
    ) -> Result<()> {
        let node = {
            let mut graphs = self.assoc_graphs.write();
            let (graph, last_access) = graphs
                .entry(tenant_id.to_string())
                .or_insert_with(|| (HnswGraph::new(self.hnsw_params.clone()), Instant::now()));
            *last_access = Instant::now();
            graph.insert(memory_id, assoc_embedding)?
        };

        let serialized = node.serialize();
        self.storage.put(
            ColumnFamilyName::VectorsAssociative,
            &memory_id,
            &serialized,
        )?;

        Ok(())
    }

    /// Hebbian update for a specific edge type.
    ///
    /// `offset[type] += lr * (a_target - a_source)`, then L2-normalize and persist.
    pub fn update_type_offset(
        &self,
        edge_type: EdgeType,
        a_source: &[f32],
        a_target: &[f32],
        lr: f32,
    ) -> Result<()> {
        if a_source.len() != a_target.len() || a_source.is_empty() {
            return Ok(());
        }

        {
            let mut offsets = self.type_offsets.write();
            let offset = offsets
                .entry(edge_type)
                .or_insert_with(|| vec![0.0f32; a_source.len()]);

            if offset.len() != a_source.len() {
                return Ok(());
            }

            for i in 0..offset.len() {
                offset[i] += lr * (a_target[i] - a_source[i]);
            }
            l2_normalize(offset);
        }

        self.persist_type_offsets()
    }

    /// Forward causal search: query = normalize(a_seed + offset[type]).
    ///
    /// Finds memories that are typical "targets" when starting from seed's associative space.
    pub fn search_causal_forward(
        &self,
        tenant_id: &str,
        seed_assoc: &[f32],
        edge_type: EdgeType,
        k: usize,
        ef: Option<usize>,
    ) -> Result<Vec<([u8; 16], f32)>> {
        let query = {
            let offsets = self.type_offsets.read();
            if let Some(offset) = offsets.get(&edge_type) {
                if offset.len() == seed_assoc.len() {
                    let raw: Vec<f32> = seed_assoc
                        .iter()
                        .zip(offset.iter())
                        .map(|(s, o)| s + o)
                        .collect();
                    normalize_vec(&raw)
                } else {
                    seed_assoc.to_vec()
                }
            } else {
                seed_assoc.to_vec()
            }
        };

        self.search_assoc(tenant_id, &query, k, ef)
    }

    /// Backward causal search: query = normalize(a_seed - offset[type]).
    ///
    /// Finds memories that are typical "sources" pointing toward seed's associative space.
    pub fn search_causal_backward(
        &self,
        tenant_id: &str,
        seed_assoc: &[f32],
        edge_type: EdgeType,
        k: usize,
        ef: Option<usize>,
    ) -> Result<Vec<([u8; 16], f32)>> {
        let query = {
            let offsets = self.type_offsets.read();
            if let Some(offset) = offsets.get(&edge_type) {
                if offset.len() == seed_assoc.len() {
                    let raw: Vec<f32> = seed_assoc
                        .iter()
                        .zip(offset.iter())
                        .map(|(s, o)| s - o)
                        .collect();
                    normalize_vec(&raw)
                } else {
                    seed_assoc.to_vec()
                }
            } else {
                seed_assoc.to_vec()
            }
        };

        self.search_assoc(tenant_id, &query, k, ef)
    }

    /// Analogy search: A is to B as C is to ?.
    ///
    /// `dir = normalize(a_B - a_A)`, `target = normalize(a_C + dir)`
    pub fn search_analogy(
        &self,
        tenant_id: &str,
        a_a: &[f32],
        a_b: &[f32],
        a_c: &[f32],
        k: usize,
        ef: Option<usize>,
    ) -> Result<Vec<([u8; 16], f32)>> {
        if a_a.len() != a_b.len() || a_b.len() != a_c.len() || a_a.is_empty() {
            return Ok(Vec::new());
        }

        let diff: Vec<f32> = a_b.iter().zip(a_a.iter()).map(|(b, a)| b - a).collect();
        let dir = normalize_vec(&diff);

        let raw: Vec<f32> = a_c.iter().zip(dir.iter()).map(|(c, d)| c + d).collect();
        let target = normalize_vec(&raw);

        self.search_assoc(tenant_id, &target, k, ef)
    }

    /// Tombstone a node in the assoc HNSW for a specific tenant.
    pub fn commit_delete_for_tenant(&self, tenant_id: &str, memory_id: &[u8; 16]) {
        let mut graphs = self.assoc_graphs.write();
        if let Some((graph, last_access)) = graphs.get_mut(tenant_id) {
            *last_access = Instant::now();
            graph.mark_deleted(memory_id);
        }
    }

    /// Prepare a BatchOperation to delete the associative node from VectorsAssociative CF.
    pub fn prepare_delete(&self, memory_id: &[u8; 16]) -> BatchOperation {
        BatchOperation::Delete {
            cf: ColumnFamilyName::VectorsAssociative,
            key: memory_id.to_vec(),
        }
    }

    // ─── Internal helpers ───────────────────────────────────────────

    fn search_assoc(
        &self,
        tenant_id: &str,
        query: &[f32],
        k: usize,
        ef: Option<usize>,
    ) -> Result<Vec<([u8; 16], f32)>> {
        {
            let graphs = self.assoc_graphs.read();
            if let Some((graph, _)) = graphs.get(tenant_id) {
                let result = graph.search(query, k, ef)?;
                drop(graphs);
                let mut graphs_w = self.assoc_graphs.write();
                if let Some((_, last_access)) = graphs_w.get_mut(tenant_id) {
                    *last_access = Instant::now();
                }
                return Ok(result);
            }
        }
        // Tenant not found — create an empty graph, return no results
        let mut graphs = self.assoc_graphs.write();
        let (graph, _) = graphs
            .entry(tenant_id.to_string())
            .or_insert_with(|| (HnswGraph::new(self.hnsw_params.clone()), Instant::now()));
        graph.search(query, k, ef)
    }

    /// Persist type_offsets to Meta CF using binary format:
    /// `[num_types: u8] [edge_type_byte: u8] [dims: u32 LE] [f32 LE × dims] ...`
    fn persist_type_offsets(&self) -> Result<()> {
        let offsets = self.type_offsets.read();
        let mut buf = Vec::new();
        buf.push(offsets.len() as u8);
        for (edge_type, offset) in offsets.iter() {
            buf.push(edge_type.as_byte());
            let dims = offset.len() as u32;
            buf.extend_from_slice(&dims.to_le_bytes());
            for &v in offset {
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
        drop(offsets);
        self.storage
            .put(ColumnFamilyName::Meta, TYPE_OFFSETS_META_KEY, &buf)?;
        Ok(())
    }

    /// Load type_offsets from Meta CF on startup.
    fn load_type_offsets(
        storage: &Arc<dyn StorageBackend>,
        expected_dims: usize,
    ) -> Result<HashMap<EdgeType, Vec<f32>>> {
        let mut result = HashMap::new();
        let data = match storage.get(ColumnFamilyName::Meta, TYPE_OFFSETS_META_KEY)? {
            Some(d) => d,
            None => return Ok(result),
        };

        if data.is_empty() {
            return Ok(result);
        }

        let num_types = data[0] as usize;
        let mut pos = 1;

        for _ in 0..num_types {
            if pos >= data.len() {
                break;
            }
            let edge_byte = data[pos];
            pos += 1;

            if pos + 4 > data.len() {
                break;
            }
            let dims = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                as usize;
            pos += 4;

            if dims != expected_dims || pos + dims * 4 > data.len() {
                break;
            }

            let mut offset = Vec::with_capacity(dims);
            for _ in 0..dims {
                let v =
                    f32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                offset.push(v);
                pos += 4;
            }

            if let Ok(edge_type) = EdgeType::from_byte(edge_byte) {
                result.insert(edge_type, offset);
            }
        }

        Ok(result)
    }

    /// Rebuild the default tenant's assoc HNSW from VectorsAssociative CF.
    fn rebuild_assoc_hnsw(
        storage: &Arc<dyn StorageBackend>,
        hnsw: &mut HnswGraph,
        params: &HnswParams,
    ) -> Result<usize> {
        let entries = storage.prefix_iterator(ColumnFamilyName::VectorsAssociative, &[])?;
        if entries.is_empty() {
            return Ok(0);
        }

        let mut count = 0;
        for (key, value) in &entries {
            if key.len() != 16 {
                continue;
            }
            let mut memory_id = [0u8; 16];
            memory_id.copy_from_slice(key);

            match HnswNode::deserialize(memory_id, value, params.dimensions) {
                Ok(node) => {
                    if let Err(e) = hnsw.insert(memory_id, node.vector) {
                        eprintln!(
                            "Assoc HNSW rebuild: failed to insert node {}: {}",
                            hex_id(&memory_id),
                            e
                        );
                        continue;
                    }
                    count += 1;
                }
                Err(e) => {
                    eprintln!(
                        "Assoc HNSW rebuild: failed to deserialize node {}: {}",
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
    use hebbs_storage::InMemoryBackend;
    use std::sync::Arc;

    fn test_params(dims: usize) -> HnswParams {
        HnswParams::with_m(dims, 4)
    }

    fn normalized_vec(dims: usize, seed: u64) -> Vec<f32> {
        use rand::Rng;
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut v: Vec<f32> = (0..dims).map(|_| rng.gen_range(-1.0f32..1.0)).collect();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }

    #[test]
    fn insert_and_search() {
        let storage = Arc::new(InMemoryBackend::new());
        let idx = AssociativeIndex::new_with_seed(storage, test_params(8), 42).unwrap();

        let id = [1u8; 16];
        let emb = normalized_vec(8, 1);

        idx.insert_for_tenant("default", id, emb.clone()).unwrap();

        let results = idx.search_assoc("default", &emb, 5, None).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, id);
    }

    #[test]
    fn type_offsets_persist_and_load() {
        let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryBackend::new());
        let dims = 8;

        {
            let idx = AssociativeIndex::new_with_seed(Arc::clone(&storage), test_params(dims), 42)
                .unwrap();
            let src = normalized_vec(dims, 1);
            let tgt = normalized_vec(dims, 2);
            idx.update_type_offset(EdgeType::CausedBy, &src, &tgt, 0.5)
                .unwrap();
        }

        // Reload
        let idx2 = AssociativeIndex::new(Arc::clone(&storage), test_params(dims)).unwrap();
        let offsets = idx2.type_offsets.read();
        assert!(offsets.contains_key(&EdgeType::CausedBy));
    }

    #[test]
    fn causal_forward_finds_target_direction() {
        let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryBackend::new());
        let dims = 8;
        let idx =
            AssociativeIndex::new_with_seed(Arc::clone(&storage), test_params(dims), 42).unwrap();

        let id_a = [1u8; 16];
        let id_b = [2u8; 16];
        let id_c = [3u8; 16];
        let emb_a = normalized_vec(dims, 1);
        let emb_b = normalized_vec(dims, 2);
        let emb_c = normalized_vec(dims, 3);

        // Insert all three
        idx.insert_for_tenant("default", id_a, emb_a.clone())
            .unwrap();
        idx.insert_for_tenant("default", id_b, emb_b.clone())
            .unwrap();
        idx.insert_for_tenant("default", id_c, emb_c.clone())
            .unwrap();

        // Learn edge: B -CausedBy-> A (source=B, target=A)
        idx.update_type_offset(EdgeType::CausedBy, &emb_b, &emb_a, HEBBIAN_LR)
            .unwrap();

        // Forward from B should find A-ish region
        let results = idx
            .search_causal_forward("default", &emb_b, EdgeType::CausedBy, 3, None)
            .unwrap();
        // With 3 nodes in HNSW and k=3, should find all 3
        assert!(!results.is_empty());
    }

    #[test]
    fn analogy_search_returns_results() {
        let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryBackend::new());
        let dims = 8;
        let idx =
            AssociativeIndex::new_with_seed(Arc::clone(&storage), test_params(dims), 42).unwrap();

        let id_a = [1u8; 16];
        let id_b = [2u8; 16];
        let id_c = [3u8; 16];
        let emb_a = normalized_vec(dims, 1);
        let emb_b = normalized_vec(dims, 2);
        let emb_c = normalized_vec(dims, 3);

        idx.insert_for_tenant("default", id_a, emb_a.clone())
            .unwrap();
        idx.insert_for_tenant("default", id_b, emb_b.clone())
            .unwrap();
        idx.insert_for_tenant("default", id_c, emb_c.clone())
            .unwrap();

        let results = idx
            .search_analogy("default", &emb_a, &emb_b, &emb_c, 3, None)
            .unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn tenant_isolation() {
        let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryBackend::new());
        let idx =
            AssociativeIndex::new_with_seed(Arc::clone(&storage), test_params(8), 42).unwrap();

        let id_a = [1u8; 16];
        let id_b = [2u8; 16];
        let emb_a = normalized_vec(8, 1);
        let emb_b = normalized_vec(8, 2);

        idx.insert_for_tenant("tenant_a", id_a, emb_a.clone())
            .unwrap();
        idx.insert_for_tenant("tenant_b", id_b, emb_b.clone())
            .unwrap();

        // Tenant A should only find its own memory
        let res_a = idx.search_assoc("tenant_a", &emb_a, 10, None).unwrap();
        assert_eq!(res_a.len(), 1);
        assert_eq!(res_a[0].0, id_a);

        // Tenant B should only find its own memory
        let res_b = idx.search_assoc("tenant_b", &emb_b, 10, None).unwrap();
        assert_eq!(res_b.len(), 1);
        assert_eq!(res_b[0].0, id_b);
    }
}
