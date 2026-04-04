use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::HashSet;
use std::sync::Arc;

use hebbs_index::hnsw::distance::{brute_force_search, inner_product_distance};
use hebbs_index::hnsw::{HnswGraph, HnswParams};
use hebbs_index::{IndexManager, TemporalIndex, TemporalOrder};
use hebbs_storage::InMemoryBackend;

use rand::Rng;
use rand::SeedableRng;

fn normalized_vec(dims: usize, seed: u64) -> Vec<f32> {
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

fn make_id(val: u32) -> [u8; 16] {
    let mut id = [0u8; 16];
    id[..4].copy_from_slice(&val.to_be_bytes());
    id
}

fn bench_distance_384(c: &mut Criterion) {
    let a = normalized_vec(384, 1);
    let b = normalized_vec(384, 2);

    c.bench_function("distance/inner_product_384dim", |bencher| {
        bencher.iter(|| inner_product_distance(&a, &b));
    });
}

fn bench_hnsw_insert(c: &mut Criterion) {
    let dims = 384;
    let mut group = c.benchmark_group("hnsw_insert");

    for &size in &[1_000, 10_000] {
        group.bench_with_input(BenchmarkId::new("single", size), &size, |b, &n| {
            let params = HnswParams::new(dims);
            let mut graph = HnswGraph::new_with_seed(params, 42);

            for i in 0..n as u32 {
                let v = normalized_vec(dims, i as u64 + 10000);
                graph.insert(make_id(i), v).unwrap();
            }

            let mut counter = n as u64;
            b.iter(|| {
                counter += 1;
                let v = normalized_vec(dims, counter + 99999);
                graph.insert(make_id(counter as u32), v).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_hnsw_search(c: &mut Criterion) {
    let dims = 384;
    let mut group = c.benchmark_group("hnsw_search");

    for &size in &[1_000, 10_000] {
        let params = HnswParams::new(dims);
        let mut graph = HnswGraph::new_with_seed(params, 42);

        for i in 0..size as u32 {
            let v = normalized_vec(dims, i as u64 + 10000);
            graph.insert(make_id(i), v).unwrap();
        }

        group.bench_with_input(BenchmarkId::new("top10", size), &size, |b, _| {
            let mut counter = 0u64;
            b.iter(|| {
                counter += 1;
                let query = normalized_vec(dims, counter + 999999);
                graph.search(&query, 10, None).unwrap()
            });
        });
    }
    group.finish();
}

fn bench_temporal_query(c: &mut Criterion) {
    let storage = Arc::new(InMemoryBackend::new());
    let index = TemporalIndex::new(storage.clone());

    for i in 0..10_000u64 {
        let key = TemporalIndex::encode_key("entity_bench", i * 1000);
        storage
            .put(
                hebbs_storage::ColumnFamilyName::Temporal,
                &key,
                &make_id(i as u32),
            )
            .unwrap();
    }

    c.bench_function("temporal/range_100_of_10k", |b| {
        b.iter(|| {
            index
                .query_range(
                    "entity_bench",
                    5000 * 1000,
                    5100 * 1000,
                    TemporalOrder::Chronological,
                    100,
                )
                .unwrap()
        });
    });
}

fn bench_hnsw_rebuild(c: &mut Criterion) {
    let dims = 384;
    let storage = Arc::new(InMemoryBackend::new());
    let params = HnswParams::with_m(dims, 8);

    // Populate vectors CF
    {
        let mgr = IndexManager::new_with_seed(storage.clone(), params.clone(), 42).unwrap();
        for i in 0..1_000u32 {
            let id = make_id(i);
            let embedding = normalized_vec(dims, i as u64);
            let (ops, _) = mgr
                .prepare_insert(&id, &embedding, &embedding, None, 1000, &[])
                .unwrap();
            storage.write_batch(&ops).unwrap();
            mgr.commit_insert(id, embedding).unwrap();
        }
    }

    c.bench_function("hnsw/rebuild_1k_nodes", |b| {
        b.iter(|| IndexManager::new(storage.clone(), params.clone()).unwrap());
    });
}

use hebbs_storage::StorageBackend;

/// Recall@k quality benchmark: measures what fraction of true top-k neighbors
/// the HNSW index returns, compared to brute-force exact search.
///
/// This is the quality baseline for quantization work. Any change to the
/// distance computation or vector storage must keep recall@10 within 2%
/// of the f32 baseline measured here.
fn bench_hnsw_recall_quality(c: &mut Criterion) {
    let dims = 384;
    let n = 5000u32;
    let num_queries = 200u64;
    let params = HnswParams::new(dims);
    let mut graph = HnswGraph::new_with_seed(params, 42);

    let mut vectors: Vec<([u8; 16], Vec<f32>)> = Vec::with_capacity(n as usize);
    for i in 0..n {
        let v = normalized_vec(dims, i as u64 + 10000);
        graph.insert(make_id(i), v.clone()).unwrap();
        vectors.push((make_id(i), v));
    }

    let mut group = c.benchmark_group("hnsw_recall");
    group.sample_size(10);

    for &k in &[1, 5, 10] {
        group.bench_with_input(BenchmarkId::new("recall_at", k), &k, |b, &k| {
            b.iter(|| {
                let mut total_recall = 0.0f64;
                for q in 0..num_queries {
                    let query = normalized_vec(dims, q + 999999);
                    let hnsw_results = graph.search(&query, k, Some(100)).unwrap();
                    let hnsw_ids: HashSet<_> = hnsw_results.iter().map(|(id, _)| *id).collect();

                    let bf_results = brute_force_search(
                        &query,
                        vectors.iter().map(|(id, v)| (id, v.as_slice())),
                        k,
                    );
                    let bf_ids: HashSet<_> = bf_results.iter().map(|(id, _)| *id).collect();

                    let overlap = hnsw_ids.intersection(&bf_ids).count();
                    total_recall += overlap as f64 / k as f64;
                }
                total_recall / num_queries as f64
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_distance_384,
    bench_hnsw_insert,
    bench_hnsw_search,
    bench_temporal_query,
    bench_hnsw_rebuild,
    bench_hnsw_recall_quality,
);
criterion_main!(benches);
