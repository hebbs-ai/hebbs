use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use hebbs_core::engine::{Engine, RememberInput};
use hebbs_core::recall::{RecallInput, RecallStrategy};
use hebbs_embed::MockEmbedder;
use hebbs_storage::RocksDbBackend;

use crate::dataset;
use crate::Tier;

#[derive(serde::Serialize)]
pub struct ScalabilityResults {
    pub scale_points: Vec<ScalePoint>,
}

#[derive(serde::Serialize)]
pub struct ScalePoint {
    pub memory_count: usize,
    pub recall_similarity_p99_us: u64,
    pub recall_temporal_p99_us: u64,
    pub recall_similarity_p50_us: u64,
    pub recall_temporal_p50_us: u64,
}

pub fn run(tier: &Tier, data_dir: Option<&Path>, seed: u64) -> ScalabilityResults {
    let scale_points_config: Vec<usize> = match tier {
        Tier::Quick => vec![1_000, 5_000, 10_000],
        Tier::Standard => vec![1_000, 10_000, 50_000, 100_000],
        Tier::Full => vec![1_000, 10_000, 100_000, 500_000, 1_000_000],
    };

    let max_scale = *scale_points_config.last().unwrap();
    let measure_runs = 500;
    let dims = 8;

    let dir = match data_dir {
        Some(p) => {
            std::fs::create_dir_all(p).ok();
            tempfile::tempdir_in(p).expect("failed to create temp dir")
        }
        None => tempfile::tempdir().expect("failed to create temp dir"),
    };

    let storage: Arc<dyn hebbs_storage::StorageBackend> =
        Arc::new(RocksDbBackend::open(dir.path().to_str().unwrap()).unwrap());
    let embedder: Arc<dyn hebbs_embed::Embedder> = Arc::new(MockEmbedder::new(dims));
    let engine = Engine::new(storage, embedder).unwrap();

    let inputs = dataset::generate_memories(max_scale, seed);
    let mut inserted = 0;
    let mut results = Vec::new();

    for &target in &scale_points_config {
        while inserted < target && inserted < inputs.len() {
            engine
                .remember(RememberInput {
                    content: inputs[inserted].content.clone(),
                    importance: inputs[inserted].importance,
                    context: inputs[inserted].context.clone(),
                    entity_id: inputs[inserted].entity_id.clone(),
                    edges: Vec::new(),
            kind: None,
        })
                .unwrap();
            inserted += 1;
        }
        println!("  Scale point: {} memories", inserted);

        let cues = dataset::generate_memories(measure_runs, seed + target as u64);

        let mut sim_timings = Vec::with_capacity(measure_runs);
        for cue in &cues {
            let start = Instant::now();
            engine
                .recall(RecallInput::new(&cue.content, RecallStrategy::Similarity))
                .ok();
            sim_timings.push(start.elapsed().as_micros() as u64);
        }
        sim_timings.sort();

        let mut temp_timings = Vec::with_capacity(measure_runs);
        for _ in 0..measure_runs {
            let mut inp = RecallInput::new("temporal query", RecallStrategy::Temporal);
            inp.entity_id = Some("entity_alpha".to_string());
            let start = Instant::now();
            engine.recall(inp).ok();
            temp_timings.push(start.elapsed().as_micros() as u64);
        }
        temp_timings.sort();

        let p99_idx = (measure_runs as f64 * 0.99) as usize;
        let p50_idx = measure_runs / 2;

        results.push(ScalePoint {
            memory_count: inserted,
            recall_similarity_p99_us: sim_timings[p99_idx.min(sim_timings.len() - 1)],
            recall_temporal_p99_us: temp_timings[p99_idx.min(temp_timings.len() - 1)],
            recall_similarity_p50_us: sim_timings[p50_idx.min(sim_timings.len() - 1)],
            recall_temporal_p50_us: temp_timings[p50_idx.min(temp_timings.len() - 1)],
        });
    }

    ScalabilityResults {
        scale_points: results,
    }
}
