use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use hebbs_core::engine::{Engine, RememberInput};
use hebbs_core::forget::ForgetCriteria;
use hebbs_core::recall::{PrimeInput, RecallInput, RecallStrategy};
use hebbs_core::revise::ReviseInput;
use hebbs_embed::MockEmbedder;
use hebbs_storage::RocksDbBackend;

use crate::dataset;
use crate::Tier;

#[derive(serde::Serialize)]
pub struct LatencyResults {
    pub operations: Vec<OperationResult>,
}

#[derive(serde::Serialize)]
pub struct OperationResult {
    pub name: String,
    pub runs: usize,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub p999_us: u64,
    pub min_us: u64,
    pub max_us: u64,
    pub mean_us: u64,
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64) * p / 100.0) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn compute_stats(name: &str, timings: &mut [u64]) -> OperationResult {
    timings.sort();
    let sum: u64 = timings.iter().sum();
    let mean = if timings.is_empty() {
        0
    } else {
        sum / timings.len() as u64
    };

    OperationResult {
        name: name.to_string(),
        runs: timings.len(),
        p50_us: percentile(timings, 50.0),
        p95_us: percentile(timings, 95.0),
        p99_us: percentile(timings, 99.0),
        p999_us: percentile(timings, 99.9),
        min_us: *timings.first().unwrap_or(&0),
        max_us: *timings.last().unwrap_or(&0),
        mean_us: mean,
    }
}

pub fn run(tier: &Tier, data_dir: Option<&Path>, seed: u64) -> LatencyResults {
    let dir = match data_dir {
        Some(p) => {
            std::fs::create_dir_all(p).ok();
            tempfile::tempdir_in(p).expect("failed to create temp dir")
        }
        None => tempfile::tempdir().expect("failed to create temp dir"),
    };

    let dims = 8;
    let storage: Arc<dyn hebbs_storage::StorageBackend> =
        Arc::new(RocksDbBackend::open(dir.path().to_str().unwrap()).unwrap());
    let embedder: Arc<dyn hebbs_embed::Embedder> = Arc::new(MockEmbedder::new(dims));
    let engine = Engine::new(storage, embedder).unwrap();

    let populate_count = tier.memory_count();
    let runs = tier.runs_per_op();
    let warmup = tier.warmup_runs();

    println!("  Populating {} memories...", populate_count);
    let inputs = dataset::generate_memories(populate_count, seed);
    let mut memory_ids: Vec<Vec<u8>> = Vec::new();
    for input in &inputs {
        let m = engine
            .remember(RememberInput {
                content: input.content.clone(),
                importance: input.importance,
                context: input.context.clone(),
                entity_id: input.entity_id.clone(),
                edges: Vec::new(),
                kind: None,
            })
            .unwrap();
        memory_ids.push(m.memory_id.clone());
    }
    println!("  Population complete. Running benchmarks...\n");

    let mut results = Vec::new();

    // --- remember ---
    {
        let extra = dataset::generate_memories(warmup + runs, seed + 1000);
        for inp in extra.iter().take(warmup) {
            engine
                .remember(RememberInput {
                    content: inp.content.clone(),
                    importance: inp.importance,
                    context: inp.context.clone(),
                    entity_id: inp.entity_id.clone(),
                    edges: Vec::new(),
                    kind: None,
                })
                .ok();
        }
        let mut timings = Vec::with_capacity(runs);
        for inp in extra.iter().skip(warmup).take(runs) {
            let start = Instant::now();
            engine
                .remember(RememberInput {
                    content: inp.content.clone(),
                    importance: inp.importance,
                    context: inp.context.clone(),
                    entity_id: inp.entity_id.clone(),
                    edges: Vec::new(),
                    kind: None,
                })
                .ok();
            timings.push(start.elapsed().as_micros() as u64);
        }
        results.push(compute_stats("remember", &mut timings));
    }

    // --- get ---
    {
        for _ in 0..warmup.min(memory_ids.len()) {
            engine.get(&memory_ids[0]).ok();
        }
        let mut timings = Vec::with_capacity(runs);
        for i in 0..runs {
            let id = &memory_ids[i % memory_ids.len()];
            let start = Instant::now();
            engine.get(id).ok();
            timings.push(start.elapsed().as_micros() as u64);
        }
        results.push(compute_stats("get", &mut timings));
    }

    // --- recall similarity ---
    {
        let cues = dataset::generate_memories(warmup + runs, seed + 2000);
        for cue in cues.iter().take(warmup) {
            engine
                .recall(RecallInput::new(&cue.content, RecallStrategy::Similarity))
                .ok();
        }
        let mut timings = Vec::with_capacity(runs);
        for cue in cues.iter().skip(warmup).take(runs) {
            let start = Instant::now();
            engine
                .recall(RecallInput::new(&cue.content, RecallStrategy::Similarity))
                .ok();
            timings.push(start.elapsed().as_micros() as u64);
        }
        results.push(compute_stats("recall_similarity", &mut timings));
    }

    // --- recall temporal ---
    {
        for _ in 0..warmup {
            let mut inp = RecallInput::new("temporal query", RecallStrategy::Temporal);
            inp.entity_id = Some("entity_alpha".to_string());
            engine.recall(inp).ok();
        }
        let mut timings = Vec::with_capacity(runs);
        for _ in 0..runs {
            let mut inp = RecallInput::new("temporal query", RecallStrategy::Temporal);
            inp.entity_id = Some("entity_alpha".to_string());
            let start = Instant::now();
            engine.recall(inp).ok();
            timings.push(start.elapsed().as_micros() as u64);
        }
        results.push(compute_stats("recall_temporal", &mut timings));
    }

    // --- prime ---
    {
        for _ in 0..warmup {
            let mut inp = PrimeInput::new("entity_alpha");
            inp.max_memories = Some(50);
            engine.prime(inp).ok();
        }
        let mut timings = Vec::with_capacity(runs);
        for _ in 0..runs {
            let mut inp = PrimeInput::new("entity_alpha");
            inp.max_memories = Some(50);
            let start = Instant::now();
            engine.prime(inp).ok();
            timings.push(start.elapsed().as_micros() as u64);
        }
        results.push(compute_stats("prime", &mut timings));
    }

    // --- revise ---
    {
        let revise_ids: Vec<_> = memory_ids.iter().take(warmup + runs).cloned().collect();
        for id in revise_ids.iter().take(warmup) {
            engine
                .revise(ReviseInput::new_content(
                    id.clone(),
                    "revised content warmup",
                ))
                .ok();
        }
        let mut timings = Vec::with_capacity(runs);
        for i in 0..runs {
            let id = &revise_ids[i % revise_ids.len()];
            let start = Instant::now();
            engine
                .revise(ReviseInput::new_content(
                    id.clone(),
                    format!("revised content run {}", i),
                ))
                .ok();
            timings.push(start.elapsed().as_micros() as u64);
        }
        results.push(compute_stats("revise", &mut timings));
    }

    // --- forget (single) ---
    {
        let forget_inputs = dataset::generate_memories(warmup + runs, seed + 5000);
        let mut forget_ids = Vec::new();
        for inp in &forget_inputs {
            let m = engine
                .remember(RememberInput {
                    content: inp.content.clone(),
                    importance: inp.importance,
                    context: inp.context.clone(),
                    entity_id: inp.entity_id.clone(),
                    edges: Vec::new(),
                    kind: None,
                })
                .unwrap();
            forget_ids.push(m.memory_id);
        }
        for id in forget_ids.iter().take(warmup) {
            engine.forget(ForgetCriteria::by_ids(vec![id.clone()])).ok();
        }
        let mut timings = Vec::with_capacity(runs);
        for id in forget_ids.iter().skip(warmup).take(runs) {
            let start = Instant::now();
            engine.forget(ForgetCriteria::by_ids(vec![id.clone()])).ok();
            timings.push(start.elapsed().as_micros() as u64);
        }
        results.push(compute_stats("forget_single", &mut timings));
    }

    // --- count ---
    {
        for _ in 0..warmup {
            engine.count().ok();
        }
        let mut timings = Vec::with_capacity(runs);
        for _ in 0..runs {
            let start = Instant::now();
            engine.count().ok();
            timings.push(start.elapsed().as_micros() as u64);
        }
        results.push(compute_stats("count", &mut timings));
    }

    LatencyResults {
        operations: results,
    }
}
