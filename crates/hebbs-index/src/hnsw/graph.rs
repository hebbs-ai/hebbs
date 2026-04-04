use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use rand::Rng;

use super::distance::inner_product_distance;
use super::node::HnswNode;
use super::params::HnswParams;
use super::quantize::{quantize, quantized_inner_product_distance, QuantizedVector};
use super::rotation::HadamardRotation;
use crate::error::{IndexError, Result};

/// Ordered entry for priority queues.
/// Wraps distance + memory ID with total ordering.
/// NaN distances are treated as infinitely far.
#[derive(Clone, Copy)]
struct DistEntry {
    distance: f32,
    id: [u8; 16],
}

impl PartialEq for DistEntry {
    fn eq(&self, other: &Self) -> bool {
        self.distance.to_bits() == other.distance.to_bits() && self.id == other.id
    }
}

impl Eq for DistEntry {}

impl PartialOrd for DistEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DistEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance
            .partial_cmp(&other.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| self.id.cmp(&other.id))
    }
}

/// In-memory HNSW graph structure.
///
/// ## Thread safety
///
/// This struct is NOT internally synchronized. The caller (IndexManager)
/// wraps it in `RwLock<HnswGraph>`:
/// - Search: shared read lock (concurrent reads)
/// - Insert/delete: exclusive write lock
///
/// ## Complexity
///
/// | Operation | Complexity |
/// |-----------|-----------|
/// | insert | O(log n * ef_construction) |
/// | search | O(log n * ef_search) |
/// | delete (tombstone) | O(1) |
/// | rebuild | O(n * m_avg) |
pub struct HnswGraph {
    nodes: HashMap<[u8; 16], HnswNode>,
    entry_point: Option<[u8; 16]>,
    max_layer: usize,
    params: HnswParams,
    tombstone_count: usize,
    rng: rand::rngs::StdRng,
    /// Walsh-Hadamard rotation for quantization quality (Phase B).
    /// Present when params.use_quantization && params.use_rotation.
    rotation: Option<HadamardRotation>,
}

impl HnswGraph {
    /// Create a new empty HNSW graph.
    pub fn new(params: HnswParams) -> Self {
        use rand::SeedableRng;
        let rotation = if params.use_quantization && params.use_rotation {
            Some(HadamardRotation::new(params.dimensions, 0))
        } else {
            None
        };
        Self {
            nodes: HashMap::new(),
            entry_point: None,
            max_layer: 0,
            params,
            tombstone_count: 0,
            rng: rand::rngs::StdRng::from_entropy(),
            rotation,
        }
    }

    /// Create with a fixed seed for deterministic testing.
    pub fn new_with_seed(params: HnswParams, seed: u64) -> Self {
        use rand::SeedableRng;
        let rotation = if params.use_quantization && params.use_rotation {
            Some(HadamardRotation::new(
                params.dimensions,
                seed.wrapping_add(1),
            ))
        } else {
            None
        };
        Self {
            nodes: HashMap::new(),
            entry_point: None,
            max_layer: 0,
            params,
            tombstone_count: 0,
            rng: rand::rngs::StdRng::seed_from_u64(seed),
            rotation,
        }
    }

    /// Create with a specific rotation seed (for rebuilds from persisted seed).
    pub fn new_with_seeds(params: HnswParams, rng_seed: u64, rotation_seed: u64) -> Self {
        use rand::SeedableRng;
        let rotation = if params.use_quantization && params.use_rotation {
            Some(HadamardRotation::new(params.dimensions, rotation_seed))
        } else {
            None
        };
        Self {
            nodes: HashMap::new(),
            entry_point: None,
            max_layer: 0,
            params,
            tombstone_count: 0,
            rng: rand::rngs::StdRng::seed_from_u64(rng_seed),
            rotation,
        }
    }

    pub fn params(&self) -> &HnswParams {
        &self.params
    }

    pub fn rotation(&self) -> Option<&HadamardRotation> {
        self.rotation.as_ref()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn active_count(&self) -> usize {
        self.nodes.values().filter(|n| !n.deleted).count()
    }

    pub fn tombstone_count(&self) -> usize {
        self.tombstone_count
    }

    /// Prepare a vector for HNSW storage: rotate (if enabled) then quantize.
    /// Returns the quantized vector suitable for distance comparison.
    fn prepare_vector(&self, vector: &[f32]) -> QuantizedVector {
        if let Some(ref rotation) = self.rotation {
            let rotated = rotation.rotate(vector);
            quantize(&rotated)
        } else {
            quantize(vector)
        }
    }

    /// Compute distance between a query (already prepared) and a stored node.
    #[inline]
    fn node_distance(&self, query: &QuantizedVector, node: &HnswNode) -> f32 {
        if let Some(ref q) = node.quantized {
            quantized_inner_product_distance(query, q)
        } else if let Some(ref v) = node.vector {
            // Fallback for unquantized nodes (should not happen in normal operation)
            let q = self.prepare_vector(v);
            quantized_inner_product_distance(query, &q)
        } else {
            f32::MAX
        }
    }

    /// Compute distance between a query and a node using f32 vectors directly.
    /// Used when quantization is disabled.
    #[inline]
    fn node_distance_f32(&self, query: &[f32], node: &HnswNode) -> f32 {
        if let Some(ref v) = node.vector {
            inner_product_distance(query, v)
        } else {
            f32::MAX
        }
    }

    /// Assign a random layer for a new node using the HNSW geometric distribution.
    /// level = floor(-ln(uniform(0,1)) * ml)
    fn random_layer(&mut self) -> usize {
        let uniform: f64 = self.rng.gen_range(f64::MIN_POSITIVE..1.0);
        (-uniform.ln() * self.params.ml).floor() as usize
    }

    /// Insert a node into the in-memory graph.
    ///
    /// This is called AFTER the WriteBatch succeeds (commit_insert pattern).
    /// The node data is already persisted in the vectors CF.
    ///
    /// Complexity: O(log n * ef_construction).
    pub fn insert(&mut self, memory_id: [u8; 16], vector: Vec<f32>) -> Result<HnswNode> {
        if vector.len() != self.params.dimensions {
            return Err(IndexError::DimensionMismatch {
                expected: self.params.dimensions,
                actual: vector.len(),
            });
        }

        let node_layer = self.random_layer();

        if self.params.use_quantization {
            self.insert_quantized(memory_id, vector, node_layer)
        } else {
            self.insert_f32(memory_id, vector, node_layer)
        }
    }

    /// Insert with int8 quantized traversal.
    fn insert_quantized(
        &mut self,
        memory_id: [u8; 16],
        vector: Vec<f32>,
        node_layer: usize,
    ) -> Result<HnswNode> {
        let quantized = self.prepare_vector(&vector);

        if self.entry_point.is_none() {
            let neighbors = (0..=node_layer).map(|_| Vec::new()).collect();
            let node = HnswNode {
                memory_id,
                quantized: Some(quantized),
                vector: None, // f32 not kept in memory
                layer: node_layer as u8,
                neighbors,
                deleted: false,
            };
            self.entry_point = Some(memory_id);
            self.max_layer = node_layer;
            self.nodes.insert(memory_id, node.clone());
            return Ok(node);
        }

        let query_quantized = self.prepare_vector(&vector);
        let mut ep_id = self.entry_point.unwrap();

        // Phase 1: Greedily descend from top layer to node_layer + 1
        for layer in (node_layer + 1..=self.max_layer).rev() {
            ep_id = self.greedy_closest_quantized(&query_quantized, ep_id, layer);
        }

        // Phase 2: Search and connect at each layer
        let search_top = node_layer.min(self.max_layer);
        let mut ep_ids = vec![ep_id];
        let mut all_layer_neighbors: Vec<Vec<[u8; 16]>> = Vec::with_capacity(search_top + 1);

        for layer in (0..=search_top).rev() {
            let candidates = self.search_layer_quantized(
                &query_quantized,
                &ep_ids,
                self.params.ef_construction,
                layer,
            );

            let max_conn = self.params.max_neighbors(layer);
            let selected = self.select_neighbors_heuristic_quantized(&candidates, max_conn);

            all_layer_neighbors.push(selected.clone());
            ep_ids = candidates.iter().map(|e| e.id).collect();
        }

        all_layer_neighbors.reverse();

        while all_layer_neighbors.len() <= node_layer {
            all_layer_neighbors.push(Vec::new());
        }

        let node = HnswNode {
            memory_id,
            quantized: Some(quantized),
            vector: None,
            layer: node_layer as u8,
            neighbors: all_layer_neighbors.clone(),
            deleted: false,
        };

        self.nodes.insert(memory_id, node.clone());

        for (layer, selected) in all_layer_neighbors.iter().enumerate() {
            for &neighbor_id in selected {
                if let Some(neighbor_node) = self.nodes.get_mut(&neighbor_id) {
                    if layer < neighbor_node.neighbors.len() {
                        neighbor_node.neighbors[layer].push(memory_id);

                        let max_conn = self.params.max_neighbors(layer);
                        if neighbor_node.neighbors[layer].len() > max_conn {
                            self.prune_neighbors_for_quantized(neighbor_id, layer, max_conn);
                        }
                    }
                }
            }
        }

        if node_layer > self.max_layer {
            self.entry_point = Some(memory_id);
            self.max_layer = node_layer;
        }

        Ok(node)
    }

    /// Insert with f32 vectors (quantization disabled).
    fn insert_f32(
        &mut self,
        memory_id: [u8; 16],
        vector: Vec<f32>,
        node_layer: usize,
    ) -> Result<HnswNode> {
        if self.entry_point.is_none() {
            let neighbors = (0..=node_layer).map(|_| Vec::new()).collect();
            let node = HnswNode {
                memory_id,
                quantized: None,
                vector: Some(vector),
                layer: node_layer as u8,
                neighbors,
                deleted: false,
            };
            self.entry_point = Some(memory_id);
            self.max_layer = node_layer;
            self.nodes.insert(memory_id, node.clone());
            return Ok(node);
        }

        let mut ep_id = self.entry_point.unwrap();

        for layer in (node_layer + 1..=self.max_layer).rev() {
            ep_id = self.greedy_closest_f32(&vector, ep_id, layer);
        }

        let search_top = node_layer.min(self.max_layer);
        let mut ep_ids = vec![ep_id];
        let mut all_layer_neighbors: Vec<Vec<[u8; 16]>> = Vec::with_capacity(search_top + 1);

        for layer in (0..=search_top).rev() {
            let candidates =
                self.search_layer_f32(&vector, &ep_ids, self.params.ef_construction, layer);

            let max_conn = self.params.max_neighbors(layer);
            let selected =
                self.select_neighbors_heuristic_f32(&vector, &candidates, max_conn, layer);

            all_layer_neighbors.push(selected.clone());
            ep_ids = candidates.iter().map(|e| e.id).collect();
        }

        all_layer_neighbors.reverse();

        while all_layer_neighbors.len() <= node_layer {
            all_layer_neighbors.push(Vec::new());
        }

        let node = HnswNode {
            memory_id,
            quantized: None,
            vector: Some(vector),
            layer: node_layer as u8,
            neighbors: all_layer_neighbors.clone(),
            deleted: false,
        };

        self.nodes.insert(memory_id, node.clone());

        for (layer, selected) in all_layer_neighbors.iter().enumerate() {
            for &neighbor_id in selected {
                if let Some(neighbor_node) = self.nodes.get_mut(&neighbor_id) {
                    if layer < neighbor_node.neighbors.len() {
                        neighbor_node.neighbors[layer].push(memory_id);

                        let max_conn = self.params.max_neighbors(layer);
                        if neighbor_node.neighbors[layer].len() > max_conn {
                            self.prune_neighbors_for_f32(neighbor_id, layer, max_conn);
                        }
                    }
                }
            }
        }

        if node_layer > self.max_layer {
            self.entry_point = Some(memory_id);
            self.max_layer = node_layer;
        }

        Ok(node)
    }

    /// Insert a node during rebuild -- restores stored neighbor lists directly
    /// without running the HNSW insert algorithm.
    ///
    /// Complexity: O(1) per node.
    pub fn insert_restored(&mut self, node: HnswNode) {
        let node_layer = node.layer as usize;

        if self.entry_point.is_none() || node_layer > self.max_layer {
            self.entry_point = Some(node.memory_id);
            self.max_layer = node_layer;
        }

        self.nodes.insert(node.memory_id, node);
    }

    /// Top-K nearest neighbor search.
    ///
    /// Returns (memory_id, distance) pairs sorted by distance ascending.
    /// Tombstoned nodes are excluded from results.
    /// When quantization is enabled, distances are approximate (from int8).
    ///
    /// Complexity: O(log n * ef_search).
    pub fn search(
        &self,
        query: &[f32],
        k: usize,
        ef_search: Option<usize>,
    ) -> Result<Vec<([u8; 16], f32)>> {
        if query.len() != self.params.dimensions {
            return Err(IndexError::DimensionMismatch {
                expected: self.params.dimensions,
                actual: query.len(),
            });
        }

        if self.entry_point.is_none() {
            return Ok(Vec::new());
        }

        let ef = ef_search.unwrap_or(self.params.ef_search).max(k);

        if self.params.use_quantization {
            self.search_quantized(query, k, ef)
        } else {
            self.search_f32(query, k, ef)
        }
    }

    fn search_quantized(&self, query: &[f32], k: usize, ef: usize) -> Result<Vec<([u8; 16], f32)>> {
        let query_quantized = self.prepare_vector(query);
        let mut ep_id = self.entry_point.unwrap();

        for layer in (1..=self.max_layer).rev() {
            ep_id = self.greedy_closest_quantized(&query_quantized, ep_id, layer);
        }

        let candidates = self.search_layer_quantized(&query_quantized, &[ep_id], ef, 0);

        let mut results: Vec<([u8; 16], f32)> = candidates
            .into_iter()
            .filter(|e| self.nodes.get(&e.id).is_some_and(|n| !n.deleted))
            .map(|e| (e.id, e.distance))
            .collect();

        results.truncate(k);
        Ok(results)
    }

    fn search_f32(&self, query: &[f32], k: usize, ef: usize) -> Result<Vec<([u8; 16], f32)>> {
        let mut ep_id = self.entry_point.unwrap();

        for layer in (1..=self.max_layer).rev() {
            ep_id = self.greedy_closest_f32(query, ep_id, layer);
        }

        let candidates = self.search_layer_f32(query, &[ep_id], ef, 0);

        let mut results: Vec<([u8; 16], f32)> = candidates
            .into_iter()
            .filter(|e| self.nodes.get(&e.id).is_some_and(|n| !n.deleted))
            .map(|e| (e.id, e.distance))
            .collect();

        results.truncate(k);
        Ok(results)
    }

    /// Mark a node as deleted (tombstone).
    /// The node remains in the graph but is excluded from search results.
    ///
    /// Complexity: O(1).
    pub fn mark_deleted(&mut self, memory_id: &[u8; 16]) -> bool {
        if let Some(node) = self.nodes.get_mut(memory_id) {
            if !node.deleted {
                node.deleted = true;
                self.tombstone_count += 1;

                if self.entry_point == Some(*memory_id) {
                    self.find_new_entry_point();
                }

                return true;
            }
        }
        false
    }

    /// Check if tombstone cleanup should be triggered.
    /// Default threshold: 10% of total nodes.
    pub fn needs_cleanup(&self, threshold_ratio: f32) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        (self.tombstone_count as f32 / self.nodes.len() as f32) > threshold_ratio
    }

    /// Remove all tombstoned nodes and clean up neighbor references.
    /// This is an expensive operation meant to run in the background.
    ///
    /// Complexity: O(n * m_avg) where n = total nodes, m_avg = avg connections.
    pub fn cleanup_tombstones(&mut self) -> usize {
        let deleted_ids: HashSet<[u8; 16]> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.deleted)
            .map(|(id, _)| *id)
            .collect();

        if deleted_ids.is_empty() {
            return 0;
        }

        for id in &deleted_ids {
            self.nodes.remove(id);
        }

        for node in self.nodes.values_mut() {
            for layer_neighbors in &mut node.neighbors {
                layer_neighbors.retain(|n| !deleted_ids.contains(n));
            }
        }

        let removed = deleted_ids.len();
        self.tombstone_count = self.tombstone_count.saturating_sub(removed);

        if let Some(ep) = self.entry_point {
            if deleted_ids.contains(&ep) {
                self.find_new_entry_point();
            }
        }

        removed
    }

    // ─── Quantized traversal methods ─────────────────────────────────

    fn search_layer_quantized(
        &self,
        query: &QuantizedVector,
        entry_points: &[[u8; 16]],
        ef: usize,
        layer: usize,
    ) -> Vec<DistEntry> {
        let mut visited: HashSet<[u8; 16]> = HashSet::new();
        let mut candidates: BinaryHeap<Reverse<DistEntry>> = BinaryHeap::new();
        let mut results: BinaryHeap<DistEntry> = BinaryHeap::new();

        for &ep_id in entry_points {
            if let Some(ep_node) = self.nodes.get(&ep_id) {
                let dist = self.node_distance(query, ep_node);
                let entry = DistEntry {
                    distance: dist,
                    id: ep_id,
                };
                visited.insert(ep_id);
                candidates.push(Reverse(entry));
                results.push(entry);
            }
        }

        while let Some(Reverse(closest)) = candidates.pop() {
            let farthest_dist = results.peek().map_or(f32::MAX, |e| e.distance);
            if closest.distance > farthest_dist {
                break;
            }

            let node = match self.nodes.get(&closest.id) {
                Some(n) => n,
                None => continue,
            };

            if layer < node.neighbors.len() {
                for &neighbor_id in &node.neighbors[layer] {
                    if !visited.insert(neighbor_id) {
                        continue;
                    }

                    let neighbor = match self.nodes.get(&neighbor_id) {
                        Some(n) => n,
                        None => continue,
                    };

                    let dist = self.node_distance(query, neighbor);
                    let farthest_dist = results.peek().map_or(f32::MAX, |e| e.distance);

                    if dist < farthest_dist || results.len() < ef {
                        let entry = DistEntry {
                            distance: dist,
                            id: neighbor_id,
                        };
                        candidates.push(Reverse(entry));
                        results.push(entry);

                        if results.len() > ef {
                            results.pop();
                        }
                    }
                }
            }
        }

        let mut sorted: Vec<DistEntry> = results.into_vec();
        sorted.sort();
        sorted
    }

    fn greedy_closest_quantized(
        &self,
        query: &QuantizedVector,
        ep_id: [u8; 16],
        layer: usize,
    ) -> [u8; 16] {
        let mut current_id = ep_id;
        let mut current_dist = self
            .nodes
            .get(&ep_id)
            .map(|n| self.node_distance(query, n))
            .unwrap_or(f32::MAX);

        loop {
            let mut improved = false;

            if let Some(node) = self.nodes.get(&current_id) {
                if layer < node.neighbors.len() {
                    for &neighbor_id in &node.neighbors[layer] {
                        if let Some(neighbor) = self.nodes.get(&neighbor_id) {
                            let dist = self.node_distance(query, neighbor);
                            if dist < current_dist {
                                current_dist = dist;
                                current_id = neighbor_id;
                                improved = true;
                            }
                        }
                    }
                }
            }

            if !improved {
                break;
            }
        }

        current_id
    }

    fn select_neighbors_heuristic_quantized(
        &self,
        candidates: &[DistEntry],
        max_conn: usize,
    ) -> Vec<[u8; 16]> {
        if candidates.len() <= max_conn {
            return candidates.iter().map(|e| e.id).collect();
        }

        let mut selected: Vec<DistEntry> = Vec::with_capacity(max_conn);
        let mut remaining: Vec<DistEntry> = candidates.to_vec();
        remaining.sort();

        for candidate in remaining {
            if selected.len() >= max_conn {
                break;
            }

            let dominated = selected.iter().any(|s| {
                if let (Some(c_node), Some(s_node)) =
                    (self.nodes.get(&candidate.id), self.nodes.get(&s.id))
                {
                    let dist_cs = if let (Some(ref cq), Some(ref sq)) =
                        (&c_node.quantized, &s_node.quantized)
                    {
                        quantized_inner_product_distance(cq, sq)
                    } else {
                        f32::MAX
                    };
                    dist_cs < candidate.distance
                } else {
                    false
                }
            });

            if !dominated || selected.len() < max_conn / 2 {
                selected.push(candidate);
            }
        }

        if selected.len() < max_conn {
            let selected_ids: HashSet<[u8; 16]> = selected.iter().map(|e| e.id).collect();
            let mut sorted_candidates: Vec<DistEntry> = candidates.to_vec();
            sorted_candidates.sort();
            for c in sorted_candidates {
                if selected.len() >= max_conn {
                    break;
                }
                if !selected_ids.contains(&c.id) {
                    selected.push(c);
                }
            }
        }

        selected.iter().map(|e| e.id).collect()
    }

    fn prune_neighbors_for_quantized(&mut self, node_id: [u8; 16], layer: usize, max_conn: usize) {
        let node_quantized = match self.nodes.get(&node_id) {
            Some(n) => match &n.quantized {
                Some(q) => q.clone(),
                None => return,
            },
            None => return,
        };

        let neighbors = match self.nodes.get(&node_id) {
            Some(n) if layer < n.neighbors.len() => n.neighbors[layer].clone(),
            _ => return,
        };

        if neighbors.len() <= max_conn {
            return;
        }

        let mut scored: Vec<([u8; 16], f32)> = neighbors
            .iter()
            .filter_map(|&nid| {
                self.nodes.get(&nid).and_then(|n| {
                    n.quantized
                        .as_ref()
                        .map(|q| (nid, quantized_inner_product_distance(&node_quantized, q)))
                })
            })
            .collect();

        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(max_conn);

        let pruned: Vec<[u8; 16]> = scored.into_iter().map(|(id, _)| id).collect();

        if let Some(node) = self.nodes.get_mut(&node_id) {
            if layer < node.neighbors.len() {
                node.neighbors[layer] = pruned;
            }
        }
    }

    // ─── F32 traversal methods (quantization disabled) ───────────────

    fn search_layer_f32(
        &self,
        query: &[f32],
        entry_points: &[[u8; 16]],
        ef: usize,
        layer: usize,
    ) -> Vec<DistEntry> {
        let mut visited: HashSet<[u8; 16]> = HashSet::new();
        let mut candidates: BinaryHeap<Reverse<DistEntry>> = BinaryHeap::new();
        let mut results: BinaryHeap<DistEntry> = BinaryHeap::new();

        for &ep_id in entry_points {
            if let Some(ep_node) = self.nodes.get(&ep_id) {
                let dist = self.node_distance_f32(query, ep_node);
                let entry = DistEntry {
                    distance: dist,
                    id: ep_id,
                };
                visited.insert(ep_id);
                candidates.push(Reverse(entry));
                results.push(entry);
            }
        }

        while let Some(Reverse(closest)) = candidates.pop() {
            let farthest_dist = results.peek().map_or(f32::MAX, |e| e.distance);
            if closest.distance > farthest_dist {
                break;
            }

            let node = match self.nodes.get(&closest.id) {
                Some(n) => n,
                None => continue,
            };

            if layer < node.neighbors.len() {
                for &neighbor_id in &node.neighbors[layer] {
                    if !visited.insert(neighbor_id) {
                        continue;
                    }

                    let neighbor = match self.nodes.get(&neighbor_id) {
                        Some(n) => n,
                        None => continue,
                    };

                    let dist = self.node_distance_f32(query, neighbor);
                    let farthest_dist = results.peek().map_or(f32::MAX, |e| e.distance);

                    if dist < farthest_dist || results.len() < ef {
                        let entry = DistEntry {
                            distance: dist,
                            id: neighbor_id,
                        };
                        candidates.push(Reverse(entry));
                        results.push(entry);

                        if results.len() > ef {
                            results.pop();
                        }
                    }
                }
            }
        }

        let mut sorted: Vec<DistEntry> = results.into_vec();
        sorted.sort();
        sorted
    }

    fn greedy_closest_f32(&self, query: &[f32], ep_id: [u8; 16], layer: usize) -> [u8; 16] {
        let mut current_id = ep_id;
        let mut current_dist = self
            .nodes
            .get(&ep_id)
            .map(|n| self.node_distance_f32(query, n))
            .unwrap_or(f32::MAX);

        loop {
            let mut improved = false;

            if let Some(node) = self.nodes.get(&current_id) {
                if layer < node.neighbors.len() {
                    for &neighbor_id in &node.neighbors[layer] {
                        if let Some(neighbor) = self.nodes.get(&neighbor_id) {
                            let dist = self.node_distance_f32(query, neighbor);
                            if dist < current_dist {
                                current_dist = dist;
                                current_id = neighbor_id;
                                improved = true;
                            }
                        }
                    }
                }
            }

            if !improved {
                break;
            }
        }

        current_id
    }

    fn select_neighbors_heuristic_f32(
        &self,
        _query: &[f32],
        candidates: &[DistEntry],
        max_conn: usize,
        _layer: usize,
    ) -> Vec<[u8; 16]> {
        if candidates.len() <= max_conn {
            return candidates.iter().map(|e| e.id).collect();
        }

        let mut selected: Vec<DistEntry> = Vec::with_capacity(max_conn);
        let mut remaining: Vec<DistEntry> = candidates.to_vec();
        remaining.sort();

        for candidate in remaining {
            if selected.len() >= max_conn {
                break;
            }

            let dominated = selected.iter().any(|s| {
                if let (Some(c_node), Some(s_node)) =
                    (self.nodes.get(&candidate.id), self.nodes.get(&s.id))
                {
                    if let (Some(ref cv), Some(ref sv)) = (&c_node.vector, &s_node.vector) {
                        let dist_cs = inner_product_distance(cv, sv);
                        dist_cs < candidate.distance
                    } else {
                        false
                    }
                } else {
                    false
                }
            });

            if !dominated || selected.len() < max_conn / 2 {
                selected.push(candidate);
            }
        }

        if selected.len() < max_conn {
            let selected_ids: HashSet<[u8; 16]> = selected.iter().map(|e| e.id).collect();
            let mut sorted_candidates: Vec<DistEntry> = candidates.to_vec();
            sorted_candidates.sort();
            for c in sorted_candidates {
                if selected.len() >= max_conn {
                    break;
                }
                if !selected_ids.contains(&c.id) {
                    selected.push(c);
                }
            }
        }

        selected.iter().map(|e| e.id).collect()
    }

    fn prune_neighbors_for_f32(&mut self, node_id: [u8; 16], layer: usize, max_conn: usize) {
        let node_vector = match self.nodes.get(&node_id) {
            Some(n) => match &n.vector {
                Some(v) => v.clone(),
                None => return,
            },
            None => return,
        };

        let neighbors = match self.nodes.get(&node_id) {
            Some(n) if layer < n.neighbors.len() => n.neighbors[layer].clone(),
            _ => return,
        };

        if neighbors.len() <= max_conn {
            return;
        }

        let mut scored: Vec<([u8; 16], f32)> = neighbors
            .iter()
            .filter_map(|&nid| {
                self.nodes.get(&nid).and_then(|n| {
                    n.vector
                        .as_ref()
                        .map(|v| (nid, inner_product_distance(&node_vector, v)))
                })
            })
            .collect();

        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(max_conn);

        let pruned: Vec<[u8; 16]> = scored.into_iter().map(|(id, _)| id).collect();

        if let Some(node) = self.nodes.get_mut(&node_id) {
            if layer < node.neighbors.len() {
                node.neighbors[layer] = pruned;
            }
        }
    }

    /// Find a new entry point after the current one is deleted.
    fn find_new_entry_point(&mut self) {
        let mut best: Option<(usize, [u8; 16])> = None;
        for (id, node) in &self.nodes {
            if node.deleted {
                continue;
            }
            let layer = node.layer as usize;
            if best.is_none() || layer > best.unwrap().0 {
                best = Some((layer, *id));
            }
        }
        match best {
            Some((layer, id)) => {
                self.entry_point = Some(id);
                self.max_layer = layer;
            }
            None => {
                self.entry_point = None;
                self.max_layer = 0;
            }
        }
    }

    /// Get a reference to a node by ID.
    pub fn get_node(&self, id: &[u8; 16]) -> Option<&HnswNode> {
        self.nodes.get(id)
    }

    /// Iterate over all nodes (for persistence/rebuild).
    pub fn iter_nodes(&self) -> impl Iterator<Item = (&[u8; 16], &HnswNode)> {
        self.nodes.iter()
    }

    /// Check if a node exists (including tombstoned).
    pub fn contains(&self, id: &[u8; 16]) -> bool {
        self.nodes.contains_key(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_params(dims: usize) -> HnswParams {
        let mut params = HnswParams::with_m(dims, 4);
        params.use_quantization = false;
        params.use_rotation = false;
        params
    }

    fn make_params_quantized(dims: usize) -> HnswParams {
        let mut params = HnswParams::with_m(dims, 4);
        params.use_quantization = true;
        params.use_rotation = false;
        params
    }

    fn make_params_quantized_rotated(dims: usize) -> HnswParams {
        HnswParams::with_m(dims, 4) // defaults: quantization=true, rotation=true
    }

    fn normalized_vector(dims: usize, seed: u64) -> Vec<f32> {
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
    fn empty_graph_search_returns_empty() {
        let graph = HnswGraph::new(make_params(4));
        let query = vec![1.0, 0.0, 0.0, 0.0];
        let results = graph.search(&query, 10, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn single_insert_and_search() {
        let mut graph = HnswGraph::new_with_seed(make_params(4), 42);
        let v = vec![1.0, 0.0, 0.0, 0.0];
        graph.insert(make_id(1), v.clone()).unwrap();

        let results = graph.search(&v, 1, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, make_id(1));
        assert!(results[0].1 < 0.01);
    }

    #[test]
    fn single_insert_and_search_quantized() {
        let mut graph = HnswGraph::new_with_seed(make_params_quantized(4), 42);
        let v = vec![1.0, 0.0, 0.0, 0.0];
        graph.insert(make_id(1), v.clone()).unwrap();

        let results = graph.search(&v, 1, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, make_id(1));
        assert!(
            results[0].1 < 0.1,
            "quantized self-distance too high: {}",
            results[0].1
        );
    }

    #[test]
    fn nearest_neighbor_correct() {
        let dims = 16;
        let mut graph = HnswGraph::new_with_seed(make_params(dims), 42);

        for i in 0..100u8 {
            let v = normalized_vector(dims, i as u64);
            graph.insert(make_id(i), v).unwrap();
        }

        let query = normalized_vector(dims, 42);
        let results = graph.search(&query, 1, Some(50)).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, make_id(42), "exact match should be top-1");
    }

    #[test]
    fn search_returns_k_results() {
        let dims = 16;
        let mut graph = HnswGraph::new_with_seed(make_params(dims), 42);

        for i in 0..50u8 {
            let v = normalized_vector(dims, i as u64);
            graph.insert(make_id(i), v).unwrap();
        }

        let query = normalized_vector(dims, 100);
        let results = graph.search(&query, 10, None).unwrap();
        assert_eq!(results.len(), 10);

        for window in results.windows(2) {
            assert!(window[0].1 <= window[1].1);
        }
    }

    #[test]
    fn deleted_nodes_excluded_from_search() {
        let dims = 8;
        let mut graph = HnswGraph::new_with_seed(make_params(dims), 42);

        for i in 0..20u8 {
            let v = normalized_vector(dims, i as u64);
            graph.insert(make_id(i), v).unwrap();
        }

        assert!(graph.mark_deleted(&make_id(5)));

        let query = normalized_vector(dims, 5);
        let results = graph.search(&query, 20, Some(100)).unwrap();
        assert!(results.iter().all(|(id, _)| *id != make_id(5)));
    }

    #[test]
    fn dimension_mismatch_rejected() {
        let mut graph = HnswGraph::new(make_params(4));
        let wrong_dims = vec![1.0, 0.0];
        let result = graph.insert(make_id(1), wrong_dims);
        assert!(matches!(result, Err(IndexError::DimensionMismatch { .. })));

        graph.insert(make_id(1), vec![1.0, 0.0, 0.0, 0.0]).unwrap();
        let result = graph.search(&[1.0, 0.0], 1, None);
        assert!(matches!(result, Err(IndexError::DimensionMismatch { .. })));
    }

    #[test]
    fn tombstone_count_tracking() {
        let dims = 4;
        let mut graph = HnswGraph::new_with_seed(make_params(dims), 42);

        for i in 0..10u8 {
            graph
                .insert(make_id(i), normalized_vector(dims, i as u64))
                .unwrap();
        }

        assert_eq!(graph.tombstone_count(), 0);
        graph.mark_deleted(&make_id(3));
        assert_eq!(graph.tombstone_count(), 1);
        graph.mark_deleted(&make_id(7));
        assert_eq!(graph.tombstone_count(), 2);

        assert!(!graph.mark_deleted(&make_id(3)));
        assert_eq!(graph.tombstone_count(), 2);
    }

    #[test]
    fn cleanup_removes_tombstones() {
        let dims = 8;
        let mut graph = HnswGraph::new_with_seed(make_params(dims), 42);

        for i in 0..20u8 {
            graph
                .insert(make_id(i), normalized_vector(dims, i as u64))
                .unwrap();
        }

        for i in 0..5u8 {
            graph.mark_deleted(&make_id(i));
        }
        assert_eq!(graph.tombstone_count(), 5);
        assert_eq!(graph.len(), 20);

        let removed = graph.cleanup_tombstones();
        assert_eq!(removed, 5);
        assert_eq!(graph.len(), 15);
        assert_eq!(graph.tombstone_count(), 0);
    }

    #[test]
    fn needs_cleanup_threshold() {
        let dims = 4;
        let mut graph = HnswGraph::new_with_seed(make_params(dims), 42);

        for i in 0..10u8 {
            graph
                .insert(make_id(i), normalized_vector(dims, i as u64))
                .unwrap();
        }

        assert!(!graph.needs_cleanup(0.1));
        graph.mark_deleted(&make_id(0));
        assert!(!graph.needs_cleanup(0.1));
        graph.mark_deleted(&make_id(1));
        assert!(graph.needs_cleanup(0.1));
    }

    #[test]
    fn insert_restored_preserves_neighbors() {
        let dims = 4;
        let mut graph = HnswGraph::new(make_params(dims));

        let neighbor_id = make_id(2);
        let node = HnswNode {
            memory_id: make_id(1),
            quantized: None,
            vector: Some(vec![1.0, 0.0, 0.0, 0.0]),
            layer: 0,
            neighbors: vec![vec![neighbor_id]],
            deleted: false,
        };

        graph.insert_restored(node);
        let retrieved = graph.get_node(&make_id(1)).unwrap();
        assert_eq!(retrieved.neighbors[0], vec![neighbor_id]);
    }

    #[test]
    fn recall_quality_at_scale() {
        use super::super::distance::brute_force_search;

        let dims = 32;
        let n = 1000;
        let k = 10;
        let num_queries = 50;

        let params = HnswParams::with_m(dims, 8);
        // Test with quantization disabled to verify f32 path still works
        let mut f32_params = params.clone();
        f32_params.use_quantization = false;
        f32_params.use_rotation = false;
        let mut graph = HnswGraph::new_with_seed(f32_params, 12345);

        let mut vectors: HashMap<[u8; 16], Vec<f32>> = HashMap::new();
        for i in 0..n {
            let id = {
                let mut arr = [0u8; 16];
                arr[..2].copy_from_slice(&(i as u16).to_be_bytes());
                arr
            };
            let v = normalized_vector(dims, i as u64 + 10000);
            vectors.insert(id, v.clone());
            graph.insert(id, v).unwrap();
        }

        let mut total_recall = 0.0;
        for q in 0..num_queries {
            let query = normalized_vector(dims, q as u64 + 99999);

            let hnsw_results = graph.search(&query, k, Some(100)).unwrap();
            let hnsw_ids: HashSet<[u8; 16]> = hnsw_results.iter().map(|(id, _)| *id).collect();

            let bf_results =
                brute_force_search(&query, vectors.iter().map(|(id, v)| (id, v.as_slice())), k);
            let bf_ids: HashSet<[u8; 16]> = bf_results.iter().map(|(id, _)| *id).collect();

            let overlap = hnsw_ids.intersection(&bf_ids).count();
            total_recall += overlap as f64 / k as f64;
        }

        let avg_recall = total_recall / num_queries as f64;
        assert!(
            avg_recall > 0.85,
            "recall@{} = {:.2}% (expected > 85%)",
            k,
            avg_recall * 100.0
        );
    }

    #[test]
    fn quantized_recall_at_scale() {
        use super::super::distance::brute_force_search;

        let dims = 32;
        let n = 1000;
        let k = 10;
        let num_queries = 50;

        let mut params = HnswParams::with_m(dims, 8);
        params.use_quantization = true;
        params.use_rotation = false;
        let mut graph = HnswGraph::new_with_seed(params, 12345);

        let mut vectors: HashMap<[u8; 16], Vec<f32>> = HashMap::new();
        for i in 0..n {
            let id = {
                let mut arr = [0u8; 16];
                arr[..2].copy_from_slice(&(i as u16).to_be_bytes());
                arr
            };
            let v = normalized_vector(dims, i as u64 + 10000);
            vectors.insert(id, v.clone());
            graph.insert(id, v).unwrap();
        }

        let mut total_recall = 0.0;
        for q in 0..num_queries {
            let query = normalized_vector(dims, q as u64 + 99999);

            let hnsw_results = graph.search(&query, k, Some(100)).unwrap();
            let hnsw_ids: HashSet<[u8; 16]> = hnsw_results.iter().map(|(id, _)| *id).collect();

            let bf_results =
                brute_force_search(&query, vectors.iter().map(|(id, v)| (id, v.as_slice())), k);
            let bf_ids: HashSet<[u8; 16]> = bf_results.iter().map(|(id, _)| *id).collect();

            let overlap = hnsw_ids.intersection(&bf_ids).count();
            total_recall += overlap as f64 / k as f64;
        }

        let avg_recall = total_recall / num_queries as f64;
        assert!(
            avg_recall > 0.70,
            "quantized recall@{} = {:.2}% (expected > 70%)",
            k,
            avg_recall * 100.0
        );
    }

    #[test]
    fn quantized_rotated_recall_at_scale() {
        use super::super::distance::brute_force_search;

        let dims = 32;
        let n = 1000;
        let k = 10;
        let num_queries = 50;

        let params = make_params_quantized_rotated(dims);
        let mut graph = HnswGraph::new_with_seed(params, 12345);

        let mut vectors: HashMap<[u8; 16], Vec<f32>> = HashMap::new();
        for i in 0..n {
            let id = {
                let mut arr = [0u8; 16];
                arr[..2].copy_from_slice(&(i as u16).to_be_bytes());
                arr
            };
            let v = normalized_vector(dims, i as u64 + 10000);
            vectors.insert(id, v.clone());
            graph.insert(id, v).unwrap();
        }

        let mut total_recall = 0.0;
        for q in 0..num_queries {
            let query = normalized_vector(dims, q as u64 + 99999);

            let hnsw_results = graph.search(&query, k, Some(100)).unwrap();
            let hnsw_ids: HashSet<[u8; 16]> = hnsw_results.iter().map(|(id, _)| *id).collect();

            let bf_results =
                brute_force_search(&query, vectors.iter().map(|(id, v)| (id, v.as_slice())), k);
            let bf_ids: HashSet<[u8; 16]> = bf_results.iter().map(|(id, _)| *id).collect();

            let overlap = hnsw_ids.intersection(&bf_ids).count();
            total_recall += overlap as f64 / k as f64;
        }

        let avg_recall = total_recall / num_queries as f64;
        assert!(
            avg_recall > 0.70,
            "quantized+rotated recall@{} = {:.2}% (expected > 70%)",
            k,
            avg_recall * 100.0
        );
    }
}
