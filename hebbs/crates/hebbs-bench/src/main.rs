use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

mod dataset;
mod degradation;
mod latency;
mod report;
mod resources;
mod scalability;

#[derive(Parser)]
#[command(name = "hebbs-bench")]
#[command(about = "Benchmark suite for HEBBS cognitive memory engine")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Measure p50/p95/p99 latency for each hot-path operation
    Latency {
        #[arg(long, default_value = "quick")]
        tier: Tier,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        baseline: Option<PathBuf>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, default_value = "42")]
        seed: u64,
    },
    /// Measure recall latency at increasing scale points
    Scalability {
        #[arg(long, default_value = "quick")]
        tier: Tier,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, default_value = "42")]
        seed: u64,
    },
    /// Measure RAM and disk consumption at scale
    Resources {
        #[arg(long, default_value = "quick")]
        tier: Tier,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, default_value = "42")]
        seed: u64,
    },
    /// Measure latency degradation as memory count grows
    DegradationCurve {
        /// Total memories to insert (default 1000)
        #[arg(long, default_value = "1000")]
        total: usize,
        /// Checkpoint interval — measure every N memories (default 50)
        #[arg(long, default_value = "50")]
        interval: usize,
        /// Number of probe operations per checkpoint (default 20)
        #[arg(long, default_value = "20")]
        measure_runs: usize,
        /// Embedder backend: mock (fast, 8-dim) or onnx (real model)
        #[arg(long, default_value = "mock")]
        embedder: EmbedderChoice,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, default_value = "42")]
        seed: u64,
        /// Enable tracing spans to stderr (JSON format)
        #[arg(long)]
        verbose: bool,
    },
    /// Run all benchmark categories
    All {
        #[arg(long, default_value = "quick")]
        tier: Tier,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        baseline: Option<PathBuf>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, default_value = "42")]
        seed: u64,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum EmbedderChoice {
    Mock,
    Onnx,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Tier {
    Quick,
    Standard,
    Full,
}

impl Tier {
    pub fn memory_count(&self) -> usize {
        match self {
            Tier::Quick => 10_000,
            Tier::Standard => 100_000,
            Tier::Full => 1_000_000,
        }
    }

    pub fn runs_per_op(&self) -> usize {
        match self {
            Tier::Quick => 1_000,
            Tier::Standard => 10_000,
            Tier::Full => 100_000,
        }
    }

    pub fn warmup_runs(&self) -> usize {
        match self {
            Tier::Quick => 100,
            Tier::Standard => 1_000,
            Tier::Full => 10_000,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Tier::Quick => "quick",
            Tier::Standard => "standard",
            Tier::Full => "full",
        }
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Latency {
            tier,
            output,
            baseline,
            data_dir,
            seed,
        } => {
            let results = latency::run(&tier, data_dir.as_deref(), seed);
            let report = report::build_report(tier.name(), &results, None, None, None);
            report::print_latency_report(&results);
            if let Some(baseline_path) = &baseline {
                report::compare_with_baseline(&results, baseline_path);
            }
            if let Some(output_path) = &output {
                report::write_json(&report, output_path);
            }
        }
        Commands::Scalability {
            tier,
            output,
            data_dir,
            seed,
        } => {
            let results = scalability::run(&tier, data_dir.as_deref(), seed);
            report::print_scalability_report(&results);
            if let Some(output_path) = &output {
                let report = report::build_report(
                    tier.name(),
                    &serde_json::Value::Null,
                    Some(&results),
                    None,
                    None,
                );
                report::write_json(&report, output_path);
            }
        }
        Commands::Resources {
            tier,
            output,
            data_dir,
            seed,
        } => {
            let results = resources::run(&tier, data_dir.as_deref(), seed);
            report::print_resources_report(&results);
            if let Some(output_path) = &output {
                let report = report::build_report(
                    tier.name(),
                    &serde_json::Value::Null,
                    None,
                    Some(&results),
                    None,
                );
                report::write_json(&report, output_path);
            }
        }
        Commands::DegradationCurve {
            total,
            interval,
            measure_runs,
            embedder,
            output,
            data_dir,
            seed,
            verbose,
        } => {
            let results =
                degradation::run(total, interval, measure_runs, embedder, data_dir.as_deref(), seed, verbose);
            report::print_degradation_report(&results);
            if let Some(output_path) = &output {
                let report = report::build_report(
                    "degradation",
                    &serde_json::Value::Null,
                    None,
                    None,
                    None,
                );
                let mut report = report;
                report["results"]["degradation"] =
                    serde_json::to_value(&results).unwrap_or_default();
                report::write_json(&report, output_path);
            }
        }
        Commands::All {
            tier,
            output,
            baseline,
            data_dir,
            seed,
        } => {
            println!("══════════════════════════════════════════════════════");
            println!("  HEBBS Benchmark Suite — Tier: {}", tier.name());
            println!("══════════════════════════════════════════════════════\n");

            let latency_results = latency::run(&tier, data_dir.as_deref(), seed);
            report::print_latency_report(&latency_results);

            let scale_results = scalability::run(&tier, data_dir.as_deref(), seed);
            report::print_scalability_report(&scale_results);

            let resource_results = resources::run(&tier, data_dir.as_deref(), seed);
            report::print_resources_report(&resource_results);

            if let Some(baseline_path) = &baseline {
                report::compare_with_baseline(&latency_results, baseline_path);
            }

            if let Some(output_path) = &output {
                let report = report::build_report(
                    tier.name(),
                    &latency_results,
                    Some(&scale_results),
                    Some(&resource_results),
                    None,
                );
                report::write_json(&report, output_path);
            }
        }
    }
}
