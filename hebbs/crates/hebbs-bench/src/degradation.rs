use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use hebbs_core::engine::{Engine, RememberEdge, RememberInput};
use hebbs_core::recall::{RecallInput, RecallStrategy};
use hebbs_embed::{EmbedderConfig, MockEmbedder, OnnxEmbedder};
use hebbs_index::graph::EdgeType;
use hebbs_storage::RocksDbBackend;
use tracing_subscriber::layer::SubscriberExt;

use crate::dataset;
use crate::EmbedderChoice;

// ---------------------------------------------------------------------------
// SpanCollector — tracing Layer that captures (span_name, duration_us)
// ---------------------------------------------------------------------------

struct SpanTiming;

struct SpanCollector {
    records: Arc<Mutex<Vec<(String, u64)>>>,
}

impl SpanCollector {
    fn new() -> (Self, Arc<Mutex<Vec<(String, u64)>>>) {
        let records = Arc::new(Mutex::new(Vec::new()));
        (
            SpanCollector {
                records: records.clone(),
            },
            records,
        )
    }
}

impl<S> tracing_subscriber::Layer<S> for SpanCollector
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        _attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanTiming);
            // Store creation time — we use Instant::now() in on_close relative to entry
        }
    }

    fn on_enter(&self, id: &tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Instant::now());
        }
    }

    fn on_close(&self, id: tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        if let Some(span) = ctx.span(&id) {
            let extensions = span.extensions();
            if let Some(entered) = extensions.get::<Instant>() {
                let elapsed_us = entered.elapsed().as_micros() as u64;
                let name = span.name().to_string();
                self.records.lock().unwrap().push((name, elapsed_us));
            }
        }
    }
}

fn drain_records(records: &Arc<Mutex<Vec<(String, u64)>>>) -> Vec<(String, u64)> {
    std::mem::take(&mut *records.lock().unwrap())
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct DegradationConfig {
    pub total_memories: usize,
    pub checkpoint_interval: usize,
    pub measure_runs: usize,
}

#[derive(serde::Serialize)]
pub struct DegradationResults {
    pub config: DegradationConfig,
    pub checkpoints: Vec<Checkpoint>,
}

#[derive(serde::Serialize)]
pub struct Checkpoint {
    pub memory_count: usize,
    pub remember: OpStats,
    pub strategies: Vec<StrategyStats>,
    pub phases: HashMap<String, PhaseStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deltas: Option<Deltas>,
}

#[derive(serde::Serialize, Clone)]
pub struct PhaseStats {
    pub mean_us: f64,
    pub p99_us: u64,
    pub count: usize,
}

#[derive(serde::Serialize, Clone)]
pub struct Deltas {
    pub remember_p99_delta_us: i64,
    pub remember_p99_delta_pct: f64,
    pub strategy_deltas: Vec<StrategyDelta>,
}

#[derive(serde::Serialize, Clone)]
pub struct StrategyDelta {
    pub strategy: String,
    pub p99_delta_us: i64,
    pub p99_delta_pct: f64,
}

#[derive(serde::Serialize)]
pub struct OpStats {
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub mean_us: u64,
    pub errors: usize,
}

#[derive(serde::Serialize)]
pub struct StrategyStats {
    pub strategy: String,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub mean_us: u64,
    pub errors: usize,
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64) * p / 100.0) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn compute_op_stats(timings: &mut Vec<u64>, errors: usize) -> OpStats {
    timings.sort();
    let sum: u64 = timings.iter().sum();
    let mean = if timings.is_empty() {
        0
    } else {
        sum / timings.len() as u64
    };
    OpStats {
        p50_us: percentile(timings, 50.0),
        p95_us: percentile(timings, 95.0),
        p99_us: percentile(timings, 99.0),
        mean_us: mean,
        errors,
    }
}

fn compute_phase_stats(records: &[(String, u64)]) -> HashMap<String, PhaseStats> {
    let mut grouped: HashMap<String, Vec<u64>> = HashMap::new();
    for (name, us) in records {
        grouped.entry(name.clone()).or_default().push(*us);
    }
    let mut result = HashMap::with_capacity(grouped.len());
    for (name, mut timings) in grouped {
        timings.sort();
        let count = timings.len();
        let sum: u64 = timings.iter().sum();
        let mean_us = if count == 0 {
            0.0
        } else {
            sum as f64 / count as f64
        };
        let p99_us = percentile(&timings, 99.0);
        result.insert(
            name,
            PhaseStats {
                mean_us,
                p99_us,
                count,
            },
        );
    }
    result
}

fn compute_deltas(prev: &Checkpoint, curr: &Checkpoint) -> Deltas {
    let remember_p99_delta_us = curr.remember.p99_us as i64 - prev.remember.p99_us as i64;
    let remember_p99_delta_pct = if prev.remember.p99_us > 0 {
        (remember_p99_delta_us as f64 / prev.remember.p99_us as f64) * 100.0
    } else {
        0.0
    };

    let strategy_deltas = curr
        .strategies
        .iter()
        .map(|s| {
            let prev_p99 = prev
                .strategies
                .iter()
                .find(|ps| ps.strategy == s.strategy)
                .map(|ps| ps.p99_us)
                .unwrap_or(0);
            let delta = s.p99_us as i64 - prev_p99 as i64;
            let pct = if prev_p99 > 0 {
                (delta as f64 / prev_p99 as f64) * 100.0
            } else {
                0.0
            };
            StrategyDelta {
                strategy: s.strategy.clone(),
                p99_delta_us: delta,
                p99_delta_pct: pct,
            }
        })
        .collect();

    Deltas {
        remember_p99_delta_us,
        remember_p99_delta_pct,
        strategy_deltas,
    }
}

// ---------------------------------------------------------------------------
// Main run function
// ---------------------------------------------------------------------------

pub fn run(
    total_memories: usize,
    checkpoint_interval: usize,
    measure_runs: usize,
    embedder_choice: EmbedderChoice,
    data_dir: Option<&Path>,
    seed: u64,
    verbose: bool,
) -> DegradationResults {
    // Set up tracing: SpanCollector always active, optional fmt layer for --verbose
    let (collector, records) = SpanCollector::new();

    let registry = tracing_subscriber::registry().with(collector);

    if verbose {
        use tracing_subscriber::fmt;
        let fmt_layer = fmt::layer()
            .json()
            .with_span_events(fmt::format::FmtSpan::CLOSE)
            .with_writer(std::io::stderr);
        let subscriber = registry.with(fmt_layer);
        tracing::subscriber::set_global_default(subscriber).ok();
    } else {
        tracing::subscriber::set_global_default(registry).ok();
    }

    let dir = match data_dir {
        Some(p) => {
            std::fs::create_dir_all(p).ok();
            tempfile::tempdir_in(p).expect("failed to create temp dir")
        }
        None => tempfile::tempdir().expect("failed to create temp dir"),
    };

    let storage: Arc<dyn hebbs_storage::StorageBackend> =
        Arc::new(RocksDbBackend::open(dir.path().to_str().unwrap()).unwrap());

    let embedder: Arc<dyn hebbs_embed::Embedder> = match embedder_choice {
        EmbedderChoice::Mock => Arc::new(MockEmbedder::new(8)),
        EmbedderChoice::Onnx => {
            println!("  Initializing ONNX embedder (downloads model on first run)...");
            let model_dir = data_dir.unwrap_or(Path::new("."));
            Arc::new(
                OnnxEmbedder::new(EmbedderConfig::default_bge_small(model_dir))
                    .expect("failed to create ONNX embedder"),
            )
        }
    };

    let engine = Engine::new(storage, embedder).unwrap();

    println!("  Generating {} memories...", total_memories);
    let inputs = dataset::generate_memories(total_memories, seed);

    // Track memory IDs with FollowedBy edges for causal seed queries.
    // Every 5th memory gets a FollowedBy edge to the previous memory.
    let mut all_memory_ids: Vec<[u8; 16]> = Vec::with_capacity(total_memories);
    let mut causal_seed_ids: Vec<[u8; 16]> = Vec::new();

    let num_checkpoints = total_memories / checkpoint_interval;
    let mut checkpoints = Vec::with_capacity(num_checkpoints);
    let mut inserted = 0;

    let probe_cues = dataset::generate_memories(measure_runs * num_checkpoints, seed + 7000);
    let remember_probes = dataset::generate_memories(measure_runs * num_checkpoints, seed + 8000);
    let mut probe_offset = 0;
    let mut remember_offset = 0;

    println!(
        "  Running degradation curve: {} → {} (interval {})\n",
        checkpoint_interval, total_memories, checkpoint_interval
    );

    // Drain any records from engine init / dataset generation
    drain_records(&records);

    for checkpoint_idx in 0..num_checkpoints {
        let target = (checkpoint_idx + 1) * checkpoint_interval;

        // Insert batch up to target
        while inserted < target && inserted < inputs.len() {
            let inp = &inputs[inserted];
            let mut edges = Vec::new();

            // Every 5th memory: add FollowedBy edge to previous
            if inserted > 0 && inserted % 5 == 0 {
                edges.push(RememberEdge {
                    target_id: all_memory_ids[inserted - 1],
                    edge_type: EdgeType::FollowedBy,
                    confidence: Some(1.0),
                });
            }

            let out = engine
                .remember(RememberInput {
                    content: inp.content.clone(),
                    importance: inp.importance,
                    context: inp.context.clone(),
                    entity_id: inp.entity_id.clone(),
                    edges,
                })
                .unwrap();

            let mut id = [0u8; 16];
            id.copy_from_slice(&out.memory_id);
            all_memory_ids.push(id);

            // Track causal seeds (memories that have edges)
            if inserted > 0 && inserted % 5 == 0 {
                causal_seed_ids.push(id);
            }

            inserted += 1;
        }

        println!("  Checkpoint: {} memories", inserted);

        // Drain insertion spans — we only want measurement spans
        drain_records(&records);

        // --- Measure remember latency ---
        let mut remember_timings = Vec::with_capacity(measure_runs);
        let mut remember_errors = 0;
        for i in 0..measure_runs {
            let idx = remember_offset + i;
            if idx >= remember_probes.len() {
                break;
            }
            let inp = &remember_probes[idx];
            let start = Instant::now();
            match engine.remember(RememberInput {
                content: inp.content.clone(),
                importance: inp.importance,
                context: inp.context.clone(),
                entity_id: inp.entity_id.clone(),
                edges: Vec::new(),
            }) {
                Ok(_) => remember_timings.push(start.elapsed().as_micros() as u64),
                Err(_) => remember_errors += 1,
            }
        }
        remember_offset += measure_runs;

        // --- Measure recall similarity ---
        let sim_stats = measure_strategy(
            &engine,
            &probe_cues,
            probe_offset,
            measure_runs,
            |cue| RecallInput::new(cue, RecallStrategy::Similarity),
            "Similarity",
        );

        // --- Measure recall temporal ---
        let temp_stats = measure_strategy(
            &engine,
            &probe_cues,
            probe_offset,
            measure_runs,
            |cue| {
                let mut inp = RecallInput::new(cue, RecallStrategy::Temporal);
                inp.entity_id = Some("entity_alpha".to_string());
                inp
            },
            "Temporal",
        );

        // --- Measure recall causal ---
        let causal_stats = measure_causal(&engine, &causal_seed_ids, measure_runs);

        // --- Measure recall analogical ---
        let analog_stats = measure_strategy(
            &engine,
            &probe_cues,
            probe_offset,
            measure_runs,
            |cue| {
                let mut inp = RecallInput::new(cue, RecallStrategy::Analogical);
                let mut ctx = HashMap::new();
                ctx.insert("domain".to_string(), serde_json::json!("sales"));
                inp.cue_context = Some(ctx);
                inp
            },
            "Analogical",
        );

        probe_offset += measure_runs;

        // Collect phase data from measurement spans
        let phase_records = drain_records(&records);
        let phases = compute_phase_stats(&phase_records);

        checkpoints.push(Checkpoint {
            memory_count: inserted,
            remember: compute_op_stats(&mut remember_timings, remember_errors),
            strategies: vec![sim_stats, temp_stats, causal_stats, analog_stats],
            phases,
            deltas: None,
        });
    }

    // Second pass: compute deltas between consecutive checkpoints
    for i in 1..checkpoints.len() {
        let deltas = compute_deltas(&checkpoints[i - 1], &checkpoints[i]);
        checkpoints[i].deltas = Some(deltas);
    }

    DegradationResults {
        config: DegradationConfig {
            total_memories,
            checkpoint_interval,
            measure_runs,
        },
        checkpoints,
    }
}

fn measure_strategy(
    engine: &Engine,
    probe_cues: &[RememberInput],
    offset: usize,
    measure_runs: usize,
    build_input: impl Fn(&str) -> RecallInput,
    name: &str,
) -> StrategyStats {
    let mut timings = Vec::with_capacity(measure_runs);
    let mut errors = 0;
    for i in 0..measure_runs {
        let idx = offset + i;
        let cue = if idx < probe_cues.len() {
            &probe_cues[idx].content
        } else {
            "fallback query cue for degradation test"
        };
        let inp = build_input(cue);
        let start = Instant::now();
        match engine.recall(inp) {
            Ok(_) => timings.push(start.elapsed().as_micros() as u64),
            Err(_) => errors += 1,
        }
    }
    timings.sort();
    let sum: u64 = timings.iter().sum();
    let mean = if timings.is_empty() {
        0
    } else {
        sum / timings.len() as u64
    };
    StrategyStats {
        strategy: name.to_string(),
        p50_us: percentile(&timings, 50.0),
        p95_us: percentile(&timings, 95.0),
        p99_us: percentile(&timings, 99.0),
        mean_us: mean,
        errors,
    }
}

fn measure_causal(
    engine: &Engine,
    causal_seed_ids: &[[u8; 16]],
    measure_runs: usize,
) -> StrategyStats {
    let mut timings = Vec::with_capacity(measure_runs);
    let mut errors = 0;
    for i in 0..measure_runs {
        if causal_seed_ids.is_empty() {
            // No causal seeds available yet — record as error
            errors += 1;
            continue;
        }
        let seed_id = causal_seed_ids[i % causal_seed_ids.len()];
        let mut inp = RecallInput::new("causal probe", RecallStrategy::Causal);
        inp.seed_memory_id = Some(seed_id);
        let start = Instant::now();
        match engine.recall(inp) {
            Ok(_) => timings.push(start.elapsed().as_micros() as u64),
            Err(_) => errors += 1,
        }
    }
    timings.sort();
    let sum: u64 = timings.iter().sum();
    let mean = if timings.is_empty() {
        0
    } else {
        sum / timings.len() as u64
    };
    StrategyStats {
        strategy: "Causal".to_string(),
        p50_us: percentile(&timings, 50.0),
        p95_us: percentile(&timings, 95.0),
        p99_us: percentile(&timings, 99.0),
        mean_us: mean,
        errors,
    }
}
