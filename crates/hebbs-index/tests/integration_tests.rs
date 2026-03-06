use std::collections::HashSet;
use std::sync::Arc;

use hebbs_index::graph::EdgeType;
use hebbs_index::hnsw::distance::brute_force_search;
use hebbs_index::hnsw::{HnswGraph, HnswParams};
use hebbs_index::{EdgeInput, IndexManager, TemporalIndex, TemporalOrder};
use hebbs_storage::{ColumnFamilyName, InMemoryBackend, StorageBackend};

fn normalized_vec(dims: usize, seed: u64) -> Vec<f32> {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut v: Vec<f32> = (0..dims)
        .map(|_| {
            use rand::Rng;
            rng.gen_range(-1.0..1.0)
        })
        .collect();
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

fn make_id(val: u32) -> [u8; 16] {
    let mut id = [0u8; 16];
    id[..4].copy_from_slice(&val.to_be_bytes());
    id
}

// ─── Full lifecycle tests ────────────────────────────────────────────

#[test]
fn full_lifecycle_insert_search_delete() {
    let storage = Arc::new(InMemoryBackend::new());
    let dims = 32;
    let params = HnswParams::with_m(dims, 4);
    let mgr = IndexManager::new_with_seed(storage.clone(), params, 42).unwrap();

    let n = 100;
    let mut embeddings: Vec<Vec<f32>> = Vec::new();

    for i in 0..n {
        let id = make_id(i);
        let embedding = normalized_vec(dims, i as u64);
        let entity = format!("entity_{}", i % 5);

        let (ops, _) = mgr
            .prepare_insert(
                &id,
                &embedding,
                &embedding,
                Some(&entity),
                i as u64 * 1000,
                &[],
            )
            .unwrap();
        storage.write_batch(&ops).unwrap();
        mgr.commit_insert(id, embedding.clone()).unwrap();
        embeddings.push(embedding);
    }

    // Verify HNSW search
    for i in [0u32, 25, 50, 75, 99] {
        let results = mgr.search_vector(&embeddings[i as usize], 1, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].0,
            make_id(i),
            "exact match failed for node {}",
            i
        );
    }

    // Verify temporal queries
    for entity_idx in 0..5u32 {
        let entity = format!("entity_{}", entity_idx);
        let results = mgr
            .query_temporal(&entity, 0, u64::MAX, TemporalOrder::Chronological, 100)
            .unwrap();
        assert_eq!(
            results.len(),
            20,
            "entity {} should have 20 memories",
            entity
        );
    }

    // Delete half the nodes
    for i in (0..n).step_by(2) {
        let id = make_id(i);
        let entity = format!("entity_{}", i % 5);
        let ops = mgr
            .prepare_delete(&id, Some(&entity), i as u64 * 1000)
            .unwrap();
        storage.write_batch(&ops).unwrap();
        mgr.commit_delete(&id);
    }

    // Verify deleted nodes don't appear in search
    let results = mgr.search_vector(&embeddings[0], 100, Some(200)).unwrap();
    for (id, _) in &results {
        let idx = u32::from_be_bytes([id[0], id[1], id[2], id[3]]);
        assert!(
            idx % 2 != 0,
            "deleted node {} appeared in search results",
            idx
        );
    }

    // Verify temporal entries removed for deleted nodes
    assert_eq!(mgr.hnsw_active_count(), 50);
}

// ─── HNSW recall quality test ────────────────────────────────────────

#[test]
fn hnsw_recall_at_various_scales() {
    for &n in &[100, 500, 2000] {
        let dims = 32;
        let params = HnswParams::with_m(dims, 8);
        let mut graph = HnswGraph::new_with_seed(params, 12345);

        let mut vectors = Vec::new();
        for i in 0..n {
            let v = normalized_vec(dims, i as u64 + 10000);
            graph.insert(make_id(i), v.clone()).unwrap();
            vectors.push((make_id(i), v));
        }

        let k = 10;
        let mut total_recall = 0.0;
        let num_queries = 50;

        for q in 0..num_queries {
            let query = normalized_vec(dims, q as u64 + 99999);
            let hnsw_results = graph.search(&query, k, Some(100)).unwrap();
            let hnsw_ids: HashSet<_> = hnsw_results.iter().map(|(id, _)| *id).collect();

            let bf_results =
                brute_force_search(&query, vectors.iter().map(|(id, v)| (id, v.as_slice())), k);
            let bf_ids: HashSet<_> = bf_results.iter().map(|(id, _)| *id).collect();

            let overlap = hnsw_ids.intersection(&bf_ids).count();
            total_recall += overlap as f64 / k as f64;
        }

        let avg_recall = total_recall / num_queries as f64;
        assert!(
            avg_recall > 0.80,
            "recall@{} at n={}: {:.1}% (expected > 80%)",
            k,
            n,
            avg_recall * 100.0
        );
    }
}

// ─── Graph traversal tests ──────────────────────────────────────────

#[test]
fn graph_edges_survive_insert_delete_cycle() {
    let storage = Arc::new(InMemoryBackend::new());
    let dims = 8;
    let params = HnswParams::with_m(dims, 4);
    let mgr = IndexManager::new_with_seed(storage.clone(), params, 42).unwrap();

    // Insert node A
    let a = make_id(1);
    let v_a = normalized_vec(dims, 1);
    let (ops, _) = mgr
        .prepare_insert(&a, &v_a, &v_a, Some("e"), 1000, &[])
        .unwrap();
    storage.write_batch(&ops).unwrap();
    mgr.commit_insert(a, v_a).unwrap();

    // Insert node B with edge to A
    let b = make_id(2);
    let v_b = normalized_vec(dims, 2);
    let edges = vec![EdgeInput {
        target_id: a,
        edge_type: EdgeType::CausedBy,
        confidence: 0.9,
    }];
    let (ops, _) = mgr
        .prepare_insert(&b, &v_b, &v_b, Some("e"), 2000, &edges)
        .unwrap();
    storage.write_batch(&ops).unwrap();
    mgr.commit_insert(b, v_b).unwrap();

    // Verify edge exists
    let outgoing = mgr.outgoing_edges(&b).unwrap();
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].1, a);

    let incoming = mgr.incoming_edges(&a).unwrap();
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0].1, b);

    // Delete node B — should remove edges too
    let ops = mgr.prepare_delete(&b, Some("e"), 2000).unwrap();
    storage.write_batch(&ops).unwrap();
    mgr.commit_delete(&b);

    // Verify edges cleaned up
    let outgoing = mgr.outgoing_edges(&b).unwrap();
    assert!(outgoing.is_empty());

    let incoming = mgr.incoming_edges(&a).unwrap();
    assert!(incoming.is_empty());
}

// ─── Temporal ordering tests ─────────────────────────────────────────

#[test]
fn temporal_ordering_preserved_across_entities() {
    let storage = Arc::new(InMemoryBackend::new());
    let index = TemporalIndex::new(storage.clone());

    let entities = ["alice", "bob", "charlie"];
    let timestamps = [300, 100, 500, 200, 400];

    for entity in &entities {
        for &ts in &timestamps {
            let key = TemporalIndex::encode_key(entity, ts);
            storage
                .put(ColumnFamilyName::Temporal, &key, &[ts as u8; 16])
                .unwrap();
        }
    }

    for entity in &entities {
        let results = index
            .query_entity(entity, TemporalOrder::Chronological, 100)
            .unwrap();
        let timestamps: Vec<u64> = results.iter().map(|(_, ts)| *ts).collect();
        assert_eq!(timestamps, vec![100, 200, 300, 400, 500]);

        let results = index
            .query_entity(entity, TemporalOrder::ReverseChronological, 100)
            .unwrap();
        let timestamps: Vec<u64> = results.iter().map(|(_, ts)| *ts).collect();
        assert_eq!(timestamps, vec![500, 400, 300, 200, 100]);
    }
}

// ─── HNSW rebuild consistency test ──────────────────────────────────

#[test]
fn hnsw_rebuild_preserves_search_quality() {
    let storage = Arc::new(InMemoryBackend::new());
    let dims = 32;
    let params = HnswParams::with_m(dims, 4);

    let n = 200;
    let query = normalized_vec(dims, 99999);

    // Phase 1: Insert nodes, search, record results
    let original_results;
    {
        let mgr = IndexManager::new_with_seed(storage.clone(), params.clone(), 42).unwrap();
        for i in 0..n {
            let id = make_id(i);
            let v = normalized_vec(dims, i as u64);
            let (ops, _) = mgr.prepare_insert(&id, &v, &v, None, 1000, &[]).unwrap();
            storage.write_batch(&ops).unwrap();
            mgr.commit_insert(id, v).unwrap();
        }
        original_results = mgr.search_vector(&query, 10, Some(100)).unwrap();
    }

    // Phase 2: Create new IndexManager (triggers rebuild)
    let mgr2 = IndexManager::new(storage, params).unwrap();
    assert_eq!(mgr2.hnsw_node_count(), n as usize);

    let rebuilt_results = mgr2.search_vector(&query, 10, Some(100)).unwrap();

    // Verify that the top-1 result matches
    assert_eq!(original_results[0].0, rebuilt_results[0].0);

    // Verify overlap is reasonable (rebuild uses different insertion order)
    let original_ids: HashSet<_> = original_results.iter().map(|(id, _)| *id).collect();
    let rebuilt_ids: HashSet<_> = rebuilt_results.iter().map(|(id, _)| *id).collect();
    let overlap = original_ids.intersection(&rebuilt_ids).count();
    assert!(
        overlap >= 5,
        "rebuilt HNSW overlap with original: {}/10",
        overlap
    );
}

// ─── Concurrent access test ─────────────────────────────────────────

#[test]
fn concurrent_search_during_insert() {
    use std::sync::Arc;
    use std::thread;

    let storage = Arc::new(InMemoryBackend::new());
    let dims = 16;
    let params = HnswParams::with_m(dims, 4);
    let mgr = Arc::new(IndexManager::new_with_seed(storage.clone(), params, 42).unwrap());

    // Insert initial batch
    for i in 0..50u32 {
        let id = make_id(i);
        let v = normalized_vec(dims, i as u64);
        let (ops, _) = mgr.prepare_insert(&id, &v, &v, None, 1000, &[]).unwrap();
        storage.write_batch(&ops).unwrap();
        mgr.commit_insert(id, v).unwrap();
    }

    let mgr_write = mgr.clone();
    let storage_write = storage.clone();
    let writer = thread::spawn(move || {
        for i in 50..100u32 {
            let id = make_id(i);
            let v = normalized_vec(dims, i as u64);
            let (ops, _) = mgr_write
                .prepare_insert(&id, &v, &v, None, 1000, &[])
                .unwrap();
            storage_write.write_batch(&ops).unwrap();
            mgr_write.commit_insert(id, v).unwrap();
        }
    });

    let mgr_read = mgr.clone();
    let reader = thread::spawn(move || {
        let mut success_count = 0;
        for q in 0..100u32 {
            let query = normalized_vec(dims, q as u64 + 50000);
            let results = mgr_read.search_vector(&query, 5, None).unwrap();
            if !results.is_empty() {
                success_count += 1;
            }
        }
        success_count
    });

    writer.join().unwrap();
    let searches = reader.join().unwrap();

    // All searches should have returned results (initial 50 nodes available)
    assert_eq!(searches, 100);
    assert_eq!(mgr.hnsw_node_count(), 100);
}

// ─── Graph multi-hop traversal ──────────────────────────────────────

#[test]
fn multi_hop_graph_traversal() {
    let storage = Arc::new(InMemoryBackend::new());
    let dims = 8;
    let params = HnswParams::with_m(dims, 4);
    let mgr = IndexManager::new_with_seed(storage.clone(), params, 42).unwrap();

    // Build a chain: A → B → C → D → E
    let ids: Vec<[u8; 16]> = (1..=5).map(make_id).collect();

    for (i, &id) in ids.iter().enumerate() {
        let v = normalized_vec(dims, i as u64);
        let edges = if i > 0 {
            vec![EdgeInput {
                target_id: ids[i - 1],
                edge_type: EdgeType::FollowedBy,
                confidence: 1.0,
            }]
        } else {
            vec![]
        };
        let (ops, _) = mgr
            .prepare_insert(&id, &v, &v, None, (i as u64 + 1) * 1000, &edges)
            .unwrap();
        storage.write_batch(&ops).unwrap();
        mgr.commit_insert(id, v).unwrap();
    }

    // Traverse from E (id 5) with depth 4
    let (results, truncated) = mgr
        .traverse(&ids[4], &[EdgeType::FollowedBy], 4, 100)
        .unwrap();
    assert!(!truncated);
    assert_eq!(results.len(), 4); // D, C, B, A

    // Traverse from E with depth 2
    let (results, _) = mgr
        .traverse(&ids[4], &[EdgeType::FollowedBy], 2, 100)
        .unwrap();
    assert_eq!(results.len(), 2); // D, C
}

// ─── HNSW tombstone cleanup test ────────────────────────────────────

#[test]
fn tombstone_cleanup_produces_valid_graph() {
    let dims = 16;
    let params = HnswParams::with_m(dims, 4);
    let mut graph = HnswGraph::new_with_seed(params, 42);

    // Insert 50 nodes
    for i in 0..50u32 {
        graph
            .insert(make_id(i), normalized_vec(dims, i as u64))
            .unwrap();
    }

    // Delete every third node
    for i in (0..50u32).filter(|x| x % 3 == 0) {
        graph.mark_deleted(&make_id(i));
    }

    let removed = graph.cleanup_tombstones();
    assert!(removed > 0);
    assert_eq!(graph.tombstone_count(), 0);

    // Verify search still works correctly on the pruned graph
    let query = normalized_vec(dims, 1);
    let results = graph.search(&query, 10, Some(50)).unwrap();
    assert_eq!(results.len(), 10);

    // No deleted nodes in results
    for (id, _) in &results {
        let idx = u32::from_be_bytes([id[0], id[1], id[2], id[3]]);
        assert!(
            idx % 3 != 0,
            "deleted node {} in results after cleanup",
            idx
        );
    }
}
