//! Semantic neighborhood index: k-NN graph extraction, UMAP projection, and clustering.
//!
//! Extracts the k-nearest-neighbor structure from the HNSW index,
//! projects to 2D using UMAP (Uniform Manifold Approximation and Projection),
//! and identifies clusters via union-find on the k-NN graph.
//!
//! ## Architecture
//!
//! The computation is split into two phases to minimize lock contention:
//!
//! 1. **Snapshot extraction** (`extract_snapshot`): holds a read lock on the HNSW graph
//!    briefly to clone vectors and neighbor lists. O(n * d).
//!
//! 2. **Projection computation** (`compute_projection`): pure CPU computation on the
//!    snapshot with no locks held. O(n * k * (d + n_epochs * negative_sample_rate)).
//!
//! ## Complexity
//!
//! | Stage | Complexity |
//! |-------|-----------|
//! | Snapshot extraction | O(n * (d + M_max)) |
//! | k-NN refinement | O(n * k * d) |
//! | Smooth k-NN distances | O(n * k) |
//! | Fuzzy simplicial set | O(n * k) |
//! | UMAP SGD | O(n_epochs * n_edges * negative_sample_rate) |
//! | Clustering | O(n * k * alpha(n)) |

use std::collections::HashMap;

use rand::Rng;
use rand::SeedableRng;

use crate::hnsw::graph::HnswGraph;
use crate::hnsw::node::HnswNode;

// ═══════════════════════════════════════════════════════════════════════
//  Public types
// ═══════════════════════════════════════════════════════════════════════

/// Parameters for projection computation.
#[derive(Debug, Clone)]
pub struct ProjectionParams {
    /// Number of nearest neighbors for the k-NN graph. Default: 15.
    pub n_neighbors: usize,
    /// Minimum distance between points in 2D layout. Default: 0.1.
    pub min_dist: f32,
    /// SGD epochs. 0 = auto-select based on dataset size. Default: 0.
    pub n_epochs: usize,
    /// Random seed for reproducible layouts. Default: 42.
    pub seed: u64,
    /// Distance percentile for cluster edge cutting. Default: 0.65.
    pub cluster_threshold: f32,
    /// Minimum cluster size; smaller clusters become noise. Default: 3.
    pub min_cluster_size: usize,
    /// Negative samples per positive edge in SGD. Default: 5.
    pub negative_sample_rate: usize,
}

impl Default for ProjectionParams {
    fn default() -> Self {
        Self {
            n_neighbors: 15,
            min_dist: 0.1,
            n_epochs: 0,
            seed: 42,
            cluster_threshold: 0.65,
            min_cluster_size: 3,
            negative_sample_rate: 5,
        }
    }
}

/// Lock-free snapshot of HNSW vectors and neighbors for offline computation.
pub struct NeighborhoodSnapshot {
    /// Node memory IDs, indexed by position.
    pub ids: Vec<[u8; 16]>,
    /// Flat vector data: ids.len() * dimensions floats.
    pub vectors_flat: Vec<f32>,
    /// Per-node k-NN adjacency: Vec of (neighbor_index, distance) pairs.
    pub neighbors: Vec<Vec<(usize, f32)>>,
    /// Embedding dimensionality.
    pub dimensions: usize,
}

/// 2D projection result with cluster assignments.
pub struct Projection {
    /// (x, y) per node, indexed same as NeighborhoodSnapshot.ids.
    pub positions: Vec<(f32, f32)>,
    /// Cluster label per node. -1 = noise/unclustered.
    pub clusters: Vec<i32>,
    /// Number of distinct clusters found (excluding noise).
    pub n_clusters: usize,
}

// ═══════════════════════════════════════════════════════════════════════
//  Phase 1: Snapshot extraction (under HNSW read lock)
// ═══════════════════════════════════════════════════════════════════════

/// Extract vectors and layer-0 neighbors from the HNSW graph.
///
/// This function should be called under a read lock. It clones vector data
/// so the caller can release the lock before running the expensive projection.
///
/// Complexity: O(n * (d + M_max * d)) where d = dimensions, M_max = max layer-0 neighbors.
pub fn extract_snapshot(graph: &HnswGraph, k: usize) -> NeighborhoodSnapshot {
    let dims = graph.params().dimensions;

    // Collect all active nodes, sorted by ID for deterministic ordering
    let mut active_nodes: Vec<(&[u8; 16], &HnswNode)> = graph
        .iter_nodes()
        .filter(|(_, node)| !node.deleted)
        .collect();
    active_nodes.sort_by_key(|(id, _)| **id);

    let mut ids: Vec<[u8; 16]> = Vec::with_capacity(active_nodes.len());
    let mut vectors_flat: Vec<f32> = Vec::with_capacity(active_nodes.len() * dims);
    let mut id_to_index: HashMap<[u8; 16], usize> = HashMap::with_capacity(active_nodes.len());

    for (mem_id, node) in &active_nodes {
        let idx = ids.len();
        id_to_index.insert(**mem_id, idx);
        ids.push(**mem_id);
        vectors_flat.extend_from_slice(&node.vector);
    }

    let n = ids.len();

    // Build k-NN adjacency from layer-0 neighbors with computed distances
    let mut neighbors: Vec<Vec<(usize, f32)>> = Vec::with_capacity(n);

    for i in 0..n {
        let node: &HnswNode = match graph.get_node(&ids[i]) {
            Some(n) => n,
            None => {
                neighbors.push(Vec::new());
                continue;
            }
        };

        let vec_i = &vectors_flat[i * dims..(i + 1) * dims];
        let mut adj: Vec<(usize, f32)> = Vec::with_capacity(k);

        // Use layer-0 neighbors (HNSW construction neighbors, up to M_max=32)
        if !node.neighbors.is_empty() {
            for &neighbor_id in &node.neighbors[0] {
                if let Some(&j) = id_to_index.get(&neighbor_id) {
                    let vec_j = &vectors_flat[j * dims..(j + 1) * dims];
                    let dist = ip_distance(vec_i, vec_j);
                    adj.push((j, dist));
                }
            }
        }

        // Sort by distance, take top k
        adj.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        adj.truncate(k);

        // If we have fewer than k neighbors from layer-0, supplement with HNSW search
        if adj.len() < k && n > k {
            let needed = k - adj.len();
            let existing: std::collections::HashSet<usize> = adj.iter().map(|&(j, _)| j).collect();

            if let Ok(search_results) = graph.search(vec_i, k + 1, None) {
                for (result_id, dist) in search_results {
                    if adj.len() >= k {
                        break;
                    }
                    if let Some(&j) = id_to_index.get(&result_id) {
                        if j != i && !existing.contains(&j) {
                            adj.push((j, dist));
                        }
                    }
                }
            }
            let _ = needed; // suppress unused warning
        }

        neighbors.push(adj);
    }

    NeighborhoodSnapshot {
        ids,
        vectors_flat,
        neighbors,
        dimensions: dims,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Phase 2: UMAP projection (no locks needed)
// ═══════════════════════════════════════════════════════════════════════

/// Compute 2D UMAP projection and cluster assignments from a neighborhood snapshot.
///
/// Complexity: O(n_epochs * n * k * negative_sample_rate).
pub fn compute_projection(
    snapshot: &NeighborhoodSnapshot,
    params: &ProjectionParams,
) -> Projection {
    let n = snapshot.ids.len();

    if n == 0 {
        return Projection {
            positions: Vec::new(),
            clusters: Vec::new(),
            n_clusters: 0,
        };
    }

    if n == 1 {
        return Projection {
            positions: vec![(0.0, 0.0)],
            clusters: vec![0],
            n_clusters: 1,
        };
    }

    // Step 1: Smooth k-NN distances (find sigma per point)
    let target = (params.n_neighbors as f32).max(1.0).log2();
    let mut rhos: Vec<f32> = Vec::with_capacity(n);
    let mut sigmas: Vec<f32> = Vec::with_capacity(n);

    for adj in &snapshot.neighbors {
        if adj.is_empty() {
            rhos.push(0.0);
            sigmas.push(1.0);
            continue;
        }
        let rho = adj[0].1; // nearest neighbor distance
        let dists: Vec<f32> = adj.iter().map(|&(_, d)| d).collect();
        let sigma = find_sigma(&dists, rho, target);
        rhos.push(rho);
        sigmas.push(sigma);
    }

    // Step 2: Build fuzzy simplicial set (directed edge weights)
    let mut directed_edges: HashMap<(usize, usize), f32> = HashMap::new();

    for (i, adj) in snapshot.neighbors.iter().enumerate() {
        let rho = rhos[i];
        let sigma = sigmas[i];

        for &(j, dist) in adj {
            let w = if dist <= rho {
                1.0
            } else {
                (-(dist - rho) / sigma).exp()
            };
            directed_edges.insert((i, j), w);
        }
    }

    // Symmetrize: w_sym = w_ij + w_ji - w_ij * w_ji
    // Collect unique undirected edges in sorted order for deterministic SGD
    let mut edge_pairs: Vec<(usize, usize)> = directed_edges
        .keys()
        .map(|&(i, j)| if i < j { (i, j) } else { (j, i) })
        .collect();
    edge_pairs.sort();
    edge_pairs.dedup();

    let mut sym_edges: Vec<(usize, usize, f32)> = Vec::with_capacity(edge_pairs.len());
    for (i, j) in edge_pairs {
        let w_ij = directed_edges.get(&(i, j)).copied().unwrap_or(0.0);
        let w_ji = directed_edges.get(&(j, i)).copied().unwrap_or(0.0);
        let w_sym = w_ij + w_ji - w_ij * w_ji;
        if w_sym > 0.0 {
            sym_edges.push((i, j, w_sym));
        }
    }

    // Step 3: Compute epochs_per_sample for weighted edge sampling
    let max_weight = sym_edges.iter().map(|&(_, _, w)| w).fold(0.0_f32, f32::max);

    let n_epochs = if params.n_epochs > 0 {
        params.n_epochs
    } else if n <= 200 {
        500
    } else if n <= 2000 {
        300
    } else {
        200
    };

    let epochs_per_sample: Vec<f32> = if max_weight > 0.0 {
        sym_edges
            .iter()
            .map(|&(_, _, w)| {
                let ratio = max_weight / w.max(1e-10);
                ratio.min(n_epochs as f32)
            })
            .collect()
    } else {
        vec![1.0; sym_edges.len()]
    };

    // Step 4: Initialize positions (random, seeded)
    let mut rng = rand::rngs::StdRng::seed_from_u64(params.seed);
    let mut positions: Vec<(f32, f32)> = (0..n)
        .map(|_| (rng.gen_range(-10.0..10.0), rng.gen_range(-10.0..10.0)))
        .collect();

    // Step 5: SGD optimization
    // UMAP curve parameters for min_dist
    let (a, b) = fit_ab(params.min_dist);
    let neg_rate = params.negative_sample_rate;

    // Track which epoch each edge should next be sampled
    let mut next_sample: Vec<f32> = epochs_per_sample.clone();

    for epoch in 0..n_epochs {
        // Learning rate decays linearly
        let alpha = 1.0 - (epoch as f32 / n_epochs as f32);
        let lr = alpha.max(0.0001);

        for (edge_idx, &(i, j, _w)) in sym_edges.iter().enumerate() {
            // Only sample this edge when its epoch counter says so
            if next_sample[edge_idx] > epoch as f32 {
                continue;
            }
            next_sample[edge_idx] += epochs_per_sample[edge_idx];

            // Attractive force
            let (xi, yi) = positions[i];
            let (xj, yj) = positions[j];
            let dx = xi - xj;
            let dy = yi - yj;
            let dist_sq = (dx * dx + dy * dy).max(1e-10);

            let grad_coeff = -2.0 * a * b * dist_sq.powf(b - 1.0) / (1.0 + a * dist_sq.powf(b));
            let grad_coeff = grad_coeff.clamp(-4.0, 4.0);

            positions[i].0 += lr * grad_coeff * dx;
            positions[i].1 += lr * grad_coeff * dy;
            positions[j].0 -= lr * grad_coeff * dx;
            positions[j].1 -= lr * grad_coeff * dy;

            // Repulsive forces (negative sampling)
            for _ in 0..neg_rate {
                let k = rng.gen_range(0..n);
                if k == i {
                    continue;
                }
                let (xk, yk) = positions[k];
                let dx = xi - xk;
                let dy = yi - yk;
                let dist_sq = (dx * dx + dy * dy).max(1e-10);

                let grad_coeff = 2.0 * b / ((0.001 + dist_sq) * (1.0 + a * dist_sq.powf(b)));
                let grad_coeff = grad_coeff.clamp(-4.0, 4.0);

                // Re-read position[i] as it may have changed
                positions[i].0 += lr * grad_coeff * dx;
                positions[i].1 += lr * grad_coeff * dy;
            }
        }
    }

    // Step 6: Normalize positions to a consistent range
    normalize_positions(&mut positions);

    // Step 7: Cluster via union-find on k-NN graph
    let clusters = cluster_union_find(
        &snapshot.neighbors,
        params.cluster_threshold,
        params.min_cluster_size,
    );
    let n_clusters = clusters
        .iter()
        .copied()
        .filter(|&c| c >= 0)
        .collect::<std::collections::HashSet<i32>>()
        .len();

    Projection {
        positions,
        clusters,
        n_clusters,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Internal helpers
// ═══════════════════════════════════════════════════════════════════════

/// Inner-product distance: 1.0 - dot(a, b). Range [0, 2] for L2-normalized vectors.
/// O(d).
#[inline]
fn ip_distance(a: &[f32], b: &[f32]) -> f32 {
    let mut sum = 0.0_f32;
    for i in 0..a.len() {
        sum += a[i] * b[i];
    }
    1.0 - sum
}

/// Binary search for sigma such that sum(exp(-(d - rho) / sigma)) = target.
/// 64 iterations of bisection, tolerance 1e-5.
/// O(k * 64) = O(k).
fn find_sigma(distances: &[f32], rho: f32, target: f32) -> f32 {
    let mut lo = 1e-5_f32;
    let mut hi = 1000.0_f32;
    let mut sigma = 1.0_f32;

    for _ in 0..64 {
        sigma = (lo + hi) / 2.0;
        let sum: f32 = distances
            .iter()
            .map(|&d| {
                let shifted = (d - rho).max(0.0);
                (-shifted / sigma).exp()
            })
            .sum();

        if (sum - target).abs() < 1e-5 {
            break;
        }
        if sum > target {
            hi = sigma;
        } else {
            lo = sigma;
        }
    }

    sigma
}

/// Compute UMAP curve parameters (a, b) from min_dist.
/// The membership function is phi(d) = 1 / (1 + a * d^(2b)).
fn fit_ab(min_dist: f32) -> (f32, f32) {
    // Precomputed values from Python umap-learn curve fitting
    if min_dist <= 0.001 {
        return (1.929, 0.7915);
    }
    if (min_dist - 0.1).abs() < 0.02 {
        return (1.929, 0.7915);
    }
    if (min_dist - 0.25).abs() < 0.05 {
        return (1.597, 0.8951);
    }
    if (min_dist - 0.5).abs() < 0.05 {
        return (1.277, 1.0);
    }
    // General approximation
    let b = 0.8_f32;
    let a = (1.0 / min_dist.powf(2.0 * b)).max(0.1);
    (a, b)
}

/// Scale positions to [-200, 200] range for consistent rendering.
fn normalize_positions(positions: &mut [(f32, f32)]) {
    if positions.is_empty() {
        return;
    }

    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for &(x, y) in positions.iter() {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }

    let range_x = (max_x - min_x).max(1e-10);
    let range_y = (max_y - min_y).max(1e-10);
    let range = range_x.max(range_y);

    for pos in positions.iter_mut() {
        pos.0 = (pos.0 - (min_x + max_x) / 2.0) / range * 400.0;
        pos.1 = (pos.1 - (min_y + max_y) / 2.0) / range * 400.0;
    }
}

/// Cluster nodes using union-find on the k-NN graph.
///
/// Edges with distance above the given percentile threshold are cut.
/// Connected components below min_cluster_size are labeled as noise (-1).
///
/// Complexity: O(n * k * alpha(n)) where alpha is the inverse Ackermann function.
fn cluster_union_find(
    neighbors: &[Vec<(usize, f32)>],
    threshold_percentile: f32,
    min_cluster_size: usize,
) -> Vec<i32> {
    let n = neighbors.len();
    if n == 0 {
        return Vec::new();
    }

    // Collect all edge distances to find the threshold
    let mut all_distances: Vec<f32> = Vec::with_capacity(n * 15);
    for adj in neighbors {
        for &(_, d) in adj {
            all_distances.push(d);
        }
    }

    if all_distances.is_empty() {
        return vec![-1; n];
    }

    all_distances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let threshold_idx = ((all_distances.len() as f32 * threshold_percentile) as usize)
        .min(all_distances.len().saturating_sub(1));
    let threshold = all_distances[threshold_idx];

    // Union-Find with path compression and union by rank
    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<usize> = vec![0; n];

    for (i, adj) in neighbors.iter().enumerate() {
        for &(j, d) in adj {
            if d <= threshold {
                uf_union(&mut parent, &mut rank, i, j);
            }
        }
    }

    // Map roots to sequential cluster labels
    let mut root_to_label: HashMap<usize, i32> = HashMap::new();
    let mut next_label = 0_i32;
    let mut labels: Vec<i32> = Vec::with_capacity(n);

    for i in 0..n {
        let root = uf_find(&mut parent, i);
        let label = *root_to_label.entry(root).or_insert_with(|| {
            let l = next_label;
            next_label += 1;
            l
        });
        labels.push(label);
    }

    // Count cluster sizes; mark small clusters as noise
    let mut cluster_sizes: HashMap<i32, usize> = HashMap::new();
    for &l in &labels {
        *cluster_sizes.entry(l).or_insert(0) += 1;
    }

    for l in &mut labels {
        if cluster_sizes.get(l).copied().unwrap_or(0) < min_cluster_size {
            *l = -1;
        }
    }

    // Re-label sequentially (skip noise)
    let mut remap: HashMap<i32, i32> = HashMap::new();
    let mut new_label = 0_i32;
    for l in &mut labels {
        if *l < 0 {
            continue;
        }
        let mapped = *remap.entry(*l).or_insert_with(|| {
            let m = new_label;
            new_label += 1;
            m
        });
        *l = mapped;
    }

    labels
}

fn uf_find(parent: &mut [usize], i: usize) -> usize {
    if parent[i] != i {
        parent[i] = uf_find(parent, parent[i]);
    }
    parent[i]
}

fn uf_union(parent: &mut [usize], rank: &mut [usize], a: usize, b: usize) {
    let ra = uf_find(parent, a);
    let rb = uf_find(parent, b);
    if ra == rb {
        return;
    }
    if rank[ra] < rank[rb] {
        parent[ra] = rb;
    } else if rank[ra] > rank[rb] {
        parent[rb] = ra;
    } else {
        parent[rb] = ra;
        rank[ra] += 1;
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hnsw::{HnswGraph, HnswParams};

    fn normalized_vector(dims: usize, seed: u64) -> Vec<f32> {
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

    fn make_test_graph(n: usize, dims: usize) -> HnswGraph {
        let params = HnswParams::with_m(dims, 8);
        let mut graph = HnswGraph::new_with_seed(params, 12345);
        for i in 0..n {
            let mut id = [0u8; 16];
            id[0] = (i >> 8) as u8;
            id[1] = (i & 0xFF) as u8;
            let v = normalized_vector(dims, i as u64 + 1000);
            graph.insert(id, v).unwrap();
        }
        graph
    }

    #[test]
    fn extract_snapshot_basic() {
        let graph = make_test_graph(20, 32);
        let snapshot = extract_snapshot(&graph, 5);

        assert_eq!(snapshot.ids.len(), 20);
        assert_eq!(snapshot.vectors_flat.len(), 20 * 32);
        assert_eq!(snapshot.neighbors.len(), 20);
        assert_eq!(snapshot.dimensions, 32);

        // Each node should have at most 5 neighbors
        for adj in &snapshot.neighbors {
            assert!(adj.len() <= 5, "expected <= 5 neighbors, got {}", adj.len());
        }
    }

    #[test]
    fn extract_snapshot_empty_graph() {
        let params = HnswParams::with_m(16, 4);
        let graph = HnswGraph::new(params);
        let snapshot = extract_snapshot(&graph, 5);

        assert!(snapshot.ids.is_empty());
        assert!(snapshot.vectors_flat.is_empty());
    }

    #[test]
    fn compute_projection_basic() {
        let graph = make_test_graph(50, 32);
        let snapshot = extract_snapshot(&graph, 10);
        let params = ProjectionParams {
            n_epochs: 50,
            ..Default::default()
        };

        let proj = compute_projection(&snapshot, &params);

        assert_eq!(proj.positions.len(), 50);
        assert_eq!(proj.clusters.len(), 50);

        // All positions should be finite
        for &(x, y) in &proj.positions {
            assert!(x.is_finite(), "non-finite x: {}", x);
            assert!(y.is_finite(), "non-finite y: {}", y);
        }
    }

    #[test]
    fn compute_projection_deterministic() {
        let graph = make_test_graph(30, 16);
        let snapshot = extract_snapshot(&graph, 5);
        let params = ProjectionParams {
            n_epochs: 30,
            seed: 99,
            ..Default::default()
        };

        let proj1 = compute_projection(&snapshot, &params);
        let proj2 = compute_projection(&snapshot, &params);

        // Same seed should produce identical results
        for i in 0..proj1.positions.len() {
            assert_eq!(proj1.positions[i].0, proj2.positions[i].0);
            assert_eq!(proj1.positions[i].1, proj2.positions[i].1);
        }
    }

    #[test]
    fn compute_projection_single_node() {
        let graph = make_test_graph(1, 16);
        let snapshot = extract_snapshot(&graph, 5);
        let proj = compute_projection(&snapshot, &ProjectionParams::default());

        assert_eq!(proj.positions.len(), 1);
        assert_eq!(proj.positions[0], (0.0, 0.0));
    }

    #[test]
    fn cluster_finds_components() {
        // Two disconnected groups: nodes 0-4 connected, nodes 5-9 connected
        let mut neighbors: Vec<Vec<(usize, f32)>> = Vec::new();
        for i in 0..10 {
            let mut adj = Vec::new();
            if i < 5 {
                // Group A: connect to adjacent nodes within group
                if i > 0 {
                    adj.push((i - 1, 0.1));
                }
                if i < 4 {
                    adj.push((i + 1, 0.1));
                }
            } else {
                // Group B
                if i > 5 {
                    adj.push((i - 1, 0.1));
                }
                if i < 9 {
                    adj.push((i + 1, 0.1));
                }
            }
            neighbors.push(adj);
        }

        let labels = cluster_union_find(&neighbors, 0.9, 3);
        assert_eq!(labels.len(), 10);

        // Nodes 0-4 should share one label, nodes 5-9 another
        let label_a = labels[0];
        let label_b = labels[5];
        assert_ne!(label_a, label_b);
        assert!(label_a >= 0);
        assert!(label_b >= 0);

        for i in 0..5 {
            assert_eq!(labels[i], label_a);
        }
        for i in 5..10 {
            assert_eq!(labels[i], label_b);
        }
    }

    #[test]
    fn find_sigma_converges() {
        let distances = vec![0.1, 0.2, 0.3, 0.5, 0.8];
        let rho = 0.1;
        let target = (5.0_f32).log2();
        let sigma = find_sigma(&distances, rho, target);

        assert!(sigma > 0.0);
        assert!(sigma < 1000.0);

        // Verify the sum is close to target
        let sum: f32 = distances
            .iter()
            .map(|&d| (-(d - rho).max(0.0) / sigma).exp())
            .sum();
        assert!(
            (sum - target).abs() < 0.01,
            "sum {} not close to target {}",
            sum,
            target
        );
    }

    #[test]
    fn normalize_positions_centers_output() {
        let mut positions = vec![(10.0, 20.0), (30.0, 40.0), (50.0, 60.0)];
        normalize_positions(&mut positions);

        // Center should be near (0, 0)
        let cx: f32 = positions.iter().map(|p| p.0).sum::<f32>() / positions.len() as f32;
        let cy: f32 = positions.iter().map(|p| p.1).sum::<f32>() / positions.len() as f32;
        assert!(cx.abs() < 1.0, "center x {} not near 0", cx);
        assert!(cy.abs() < 1.0, "center y {} not near 0", cy);

        // All positions should be within [-200, 200]
        for &(x, y) in &positions {
            assert!(x.abs() <= 200.1);
            assert!(y.abs() <= 200.1);
        }
    }

    #[test]
    fn ip_distance_self_is_zero() {
        let v = vec![0.6, 0.8];
        assert!(ip_distance(&v, &v) < 1e-6);
    }

    #[test]
    fn ip_distance_orthogonal_is_one() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((ip_distance(&a, &b) - 1.0).abs() < 1e-6);
    }
}
