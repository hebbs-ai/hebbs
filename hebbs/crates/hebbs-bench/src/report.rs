use std::path::Path;

use crate::degradation::DegradationResults;
use crate::latency::LatencyResults;
use crate::resources::ResourceResults;
use crate::scalability::ScalabilityResults;

pub fn build_report(
    tier: &str,
    latency: &impl serde::Serialize,
    scalability: Option<&ScalabilityResults>,
    resources: Option<&ResourceResults>,
    cognitive: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut report = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "tier": tier,
        "timestamp": chrono_timestamp(),
        "system": system_info(),
    });

    report["results"] = serde_json::json!({});
    report["results"]["latency"] = serde_json::to_value(latency).unwrap_or_default();
    if let Some(s) = scalability {
        report["results"]["scalability"] = serde_json::to_value(s).unwrap_or_default();
    }
    if let Some(r) = resources {
        report["results"]["resources"] = serde_json::to_value(r).unwrap_or_default();
    }
    if let Some(c) = cognitive {
        report["results"]["cognitive"] = c.clone();
    }

    report
}

fn chrono_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    format!("{}", d.as_secs())
}

fn system_info() -> serde_json::Value {
    serde_json::json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

pub fn print_latency_report(results: &LatencyResults) {
    println!("  ┌────────────────────────┬──────────┬──────────┬──────────┬──────────┬──────────┐");
    println!("  │ Operation              │   p50 µs │   p95 µs │   p99 µs │  p999 µs │ mean  µs │");
    println!("  ├────────────────────────┼──────────┼──────────┼──────────┼──────────┼──────────┤");
    for op in &results.operations {
        println!(
            "  │ {:<22} │ {:>8} │ {:>8} │ {:>8} │ {:>8} │ {:>8} │",
            op.name, op.p50_us, op.p95_us, op.p99_us, op.p999_us, op.mean_us,
        );
    }
    println!("  └────────────────────────┴──────────┴──────────┴──────────┴──────────┴──────────┘");
    println!();
}

pub fn print_scalability_report(results: &ScalabilityResults) {
    println!("  Scalability — Recall p99 by Scale");
    println!("  ┌──────────────┬────────────────────┬────────────────────┐");
    println!("  │   Memories   │ Similarity p99 µs  │  Temporal p99 µs   │");
    println!("  ├──────────────┼────────────────────┼────────────────────┤");
    for sp in &results.scale_points {
        println!(
            "  │ {:>12} │ {:>18} │ {:>18} │",
            format_count(sp.memory_count),
            sp.recall_similarity_p99_us,
            sp.recall_temporal_p99_us,
        );
    }
    println!("  └──────────────┴────────────────────┴────────────────────┘");
    println!();
}

pub fn print_resources_report(results: &ResourceResults) {
    println!("  Resources — Disk and RSS by Scale");
    println!("  ┌──────────────┬──────────────┬──────────────┬──────────────┐");
    println!("  │   Memories   │    Disk MB   │    RSS MB    │  Bytes/Mem   │");
    println!("  ├──────────────┼──────────────┼──────────────┼──────────────┤");
    for m in &results.measurements {
        println!(
            "  │ {:>12} │ {:>12} │ {:>12} │ {:>12} │",
            format_count(m.memory_count),
            m.disk_bytes / 1_048_576,
            m.rss_bytes / 1_048_576,
            m.bytes_per_memory,
        );
    }
    println!("  └──────────────┴──────────────┴──────────────┴──────────────┘");
    println!();
}

pub fn print_degradation_report(results: &DegradationResults) {
    // Table 1: Degradation with deltas
    let col_w = 16;
    let strategies = ["Similarity", "Temporal", "Causal", "Analogical"];
    let strat_headers = [
        "Similarity p99",
        "Temporal p99",
        "Causal p99",
        "Analogic p99",
    ];

    println!("  Degradation Curve — Latency (µs)");

    // Header
    print!("  ┌──────────┬──────────────────");
    for _ in &strategies {
        print!("┬{}", "─".repeat(col_w + 2));
    }
    println!("┐");

    print!("  │ {:>8} │ {:>16} ", "Memories", "Remember p99");
    for h in &strat_headers {
        print!("│ {:>width$} ", h, width = col_w);
    }
    println!("│");

    print!("  ├──────────┼──────────────────");
    for _ in &strategies {
        print!("┼{}", "─".repeat(col_w + 2));
    }
    println!("┤");

    // Rows
    for cp in &results.checkpoints {
        // Remember column
        let rem_delta = match &cp.deltas {
            Some(d) => format!("{:+.1}%", d.remember_p99_delta_pct),
            None => "—".to_string(),
        };
        print!(
            "  │ {:>8} │ {:>8} {:>7} ",
            cp.memory_count, cp.remember.p99_us, rem_delta
        );

        // Strategy columns
        for (i, strat_name) in strategies.iter().enumerate() {
            let p99 = cp
                .strategies
                .iter()
                .find(|s| s.strategy == *strat_name)
                .map(|s| s.p99_us)
                .unwrap_or(0);

            let delta_str = match &cp.deltas {
                Some(d) => d
                    .strategy_deltas
                    .iter()
                    .find(|sd| sd.strategy == *strat_name)
                    .map(|sd| format!("{:+.1}%", sd.p99_delta_pct))
                    .unwrap_or_else(|| "—".to_string()),
                None => "—".to_string(),
            };
            let _ = i; // suppress unused warning
            print!("│ {:>8} {:>7} ", p99, delta_str);
        }
        println!("│");
    }

    print!("  └──────────┴──────────────────");
    for _ in &strategies {
        print!("┴{}", "─".repeat(col_w + 2));
    }
    println!("┘");
    println!();

    // Table 2: Phase breakdown (top 8 by mean_us at last checkpoint)
    print_phase_breakdown(results);
}

fn print_phase_breakdown(results: &DegradationResults) {
    let last = match results.checkpoints.last() {
        Some(cp) => cp,
        None => return,
    };

    if last.phases.is_empty() {
        return;
    }

    // Pick top 8 phases by mean_us at the last checkpoint
    let mut phase_list: Vec<(&String, &crate::degradation::PhaseStats)> =
        last.phases.iter().collect();
    phase_list.sort_by(|a, b| b.1.mean_us.partial_cmp(&a.1.mean_us).unwrap());
    phase_list.truncate(8);

    let phase_names: Vec<String> = phase_list.iter().map(|(name, _)| (*name).clone()).collect();

    if phase_names.is_empty() {
        return;
    }

    println!("  Phase Breakdown — Mean µs");

    let col_w = 10;

    // Header
    print!("  ┌──────────");
    for _ in &phase_names {
        print!("┬{}", "─".repeat(col_w + 2));
    }
    println!("┐");

    print!("  │ {:>8} ", "Memories");
    for name in &phase_names {
        // Abbreviate to 8 chars
        let abbrev: String = name.chars().take(8).collect();
        print!("│ {:>width$} ", abbrev, width = col_w);
    }
    println!("│");

    print!("  ├──────────");
    for _ in &phase_names {
        print!("┼{}", "─".repeat(col_w + 2));
    }
    println!("┤");

    // Rows
    for cp in &results.checkpoints {
        print!("  │ {:>8} ", cp.memory_count);
        for name in &phase_names {
            let val = cp.phases.get(name).map(|ps| ps.mean_us).unwrap_or(0.0);
            print!("│ {:>10.1} ", val);
        }
        println!("│");
    }

    print!("  └──────────");
    for _ in &phase_names {
        print!("┴{}", "─".repeat(col_w + 2));
    }
    println!("┘");
    println!();
}

fn format_count(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{}M", n / 1_000_000)
    } else if n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        format!("{}", n)
    }
}

pub fn compare_with_baseline(results: &LatencyResults, baseline_path: &Path) {
    let baseline_data = match std::fs::read_to_string(baseline_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  Warning: could not read baseline: {}", e);
            return;
        }
    };
    let baseline: serde_json::Value = match serde_json::from_str(&baseline_data) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("  Warning: could not parse baseline JSON: {}", e);
            return;
        }
    };

    println!("  Comparison with Baseline");
    println!("  ┌────────────────────────┬──────────┬──────────┬──────────┐");
    println!("  │ Operation              │ Old p99  │ New p99  │  Change  │");
    println!("  ├────────────────────────┼──────────┼──────────┼──────────┤");

    let mut has_regression = false;

    if let Some(ops) = baseline["results"]["latency"]["operations"].as_array() {
        for new_op in &results.operations {
            if let Some(old) = ops
                .iter()
                .find(|o| o["name"].as_str() == Some(&new_op.name))
            {
                if let Some(old_p99) = old["p99_us"].as_u64() {
                    let change = if old_p99 > 0 {
                        ((new_op.p99_us as f64 - old_p99 as f64) / old_p99 as f64) * 100.0
                    } else {
                        0.0
                    };
                    let marker = if change > 10.0 {
                        "REGRESSION"
                    } else if change < -10.0 {
                        "IMPROVED"
                    } else {
                        "OK"
                    };
                    if change > 10.0 {
                        has_regression = true;
                    }
                    println!(
                        "  │ {:<22} │ {:>8} │ {:>8} │ {:>+7.1}% │ {}",
                        new_op.name, old_p99, new_op.p99_us, change, marker,
                    );
                }
            }
        }
    }

    println!("  └────────────────────────┴──────────┴──────────┴──────────┘");

    if has_regression {
        println!("\n  ⚠  REGRESSION DETECTED: One or more operations regressed >10%");
        std::process::exit(1);
    } else {
        println!("\n  ✓  No regressions detected.");
    }
}

pub fn write_json(report: &serde_json::Value, path: &Path) {
    let json = serde_json::to_string_pretty(report).expect("failed to serialize report");
    std::fs::write(path, json).expect("failed to write report file");
    println!("  Report written to: {}", path.display());
}
