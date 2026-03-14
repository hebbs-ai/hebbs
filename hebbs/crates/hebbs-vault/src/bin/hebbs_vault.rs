use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tracing_subscriber::{fmt, EnvFilter};

use hebbs_vault::config::VaultConfig;
use hebbs_vault::error::VaultError;

#[derive(Parser)]
#[command(name = "hebbs-vault")]
#[command(about = "HEBBS vault: file-first markdown sync")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new vault (.hebbs/ directory)
    Init {
        /// Path to the vault directory
        vault_path: PathBuf,

        /// Reinitialize even if .hebbs/ already exists
        #[arg(long)]
        force: bool,
    },

    /// Index all markdown files in the vault
    Index {
        /// Path to the vault directory
        vault_path: PathBuf,
    },

    /// Watch vault for file changes and sync in real-time
    Watch {
        /// Path to the vault directory
        vault_path: PathBuf,
    },

    /// Delete .hebbs/ and rebuild index from scratch
    Rebuild {
        /// Path to the vault directory
        vault_path: PathBuf,
    },

    /// Show vault status (files, sections, sync state)
    Status {
        /// Path to the vault directory
        vault_path: PathBuf,
    },

    /// Recall memories using one or more strategies
    Recall {
        /// Path to the vault directory
        vault_path: PathBuf,

        /// The query/cue to search for
        #[arg(short, long)]
        query: String,

        /// Maximum number of results
        #[arg(short = 'k', long, default_value = "5")]
        top_k: usize,

        /// Strategies: similarity, temporal, causal, analogical (comma-separated)
        #[arg(short, long, default_value = "similarity")]
        strategy: String,

        /// Entity ID (required for temporal, optional for others)
        #[arg(long)]
        entity_id: Option<String>,

        /// Relevance weight (0.0-1.0, default 0.5)
        #[arg(long)]
        w_relevance: Option<f32>,

        /// Recency weight (0.0-1.0, default 0.2)
        #[arg(long)]
        w_recency: Option<f32>,

        /// Importance weight (0.0-1.0, default 0.2)
        #[arg(long)]
        w_importance: Option<f32>,

        /// Reinforcement weight (0.0-1.0, default 0.1)
        #[arg(long)]
        w_reinforcement: Option<f32>,

        /// HNSW ef_search override (higher = better recall, slower)
        #[arg(long)]
        ef_search: Option<usize>,

        /// Max graph traversal depth for causal strategy (default 5)
        #[arg(long)]
        max_depth: Option<usize>,

        /// Causal direction: forward, backward, both (default both)
        #[arg(long)]
        causal_direction: Option<String>,

        /// Seed memory ID (ULID) for causal graph traversal start
        #[arg(long)]
        seed_id: Option<String>,
    },

    /// List all indexed files and their sections
    List {
        /// Path to the vault directory
        vault_path: PathBuf,

        /// Show section details (headings, memory IDs, states)
        #[arg(long)]
        sections: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    let filter = match cli.verbose {
        0 => "hebbs_vault=info",
        1 => "hebbs_vault=debug",
        _ => "hebbs_vault=trace",
    };

    fmt()
        .with_env_filter(EnvFilter::new(filter))
        .with_target(false)
        .init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");

    let exit_code = rt.block_on(run(cli));
    std::process::exit(exit_code);
}

async fn run(cli: Cli) -> i32 {
    match cli.command {
        Commands::Init { vault_path, force } => {
            match hebbs_vault::init(&vault_path, force) {
                Ok(()) => {
                    println!("Initialized vault at {}", vault_path.display());
                    0
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }

        Commands::Index { vault_path } => {
            match setup_engine(&vault_path).await {
                Ok((engine, embedder)) => {
                    let progress_cb = |p: hebbs_vault::IndexProgress| match p {
                        hebbs_vault::IndexProgress::Phase1Started { total_files } => {
                            println!("[phase 1] parsing {} files...", total_files);
                        }
                        hebbs_vault::IndexProgress::Phase1Complete {
                            files_processed,
                            files_skipped,
                            sections_new,
                            sections_modified,
                        } => {
                            println!(
                                "[phase 1] complete: {} processed, {} skipped ({} new, {} modified sections)",
                                files_processed, files_skipped, sections_new, sections_modified
                            );
                        }
                        hebbs_vault::IndexProgress::Phase2Started { sections_to_process } => {
                            println!("[phase 2] embedding {} sections...", sections_to_process);
                        }
                        hebbs_vault::IndexProgress::Phase2Complete {
                            sections_embedded,
                            sections_remembered,
                            sections_revised,
                            sections_forgotten,
                        } => {
                            println!(
                                "[phase 2] complete: {} embedded, {} new, {} revised, {} forgotten",
                                sections_embedded, sections_remembered, sections_revised, sections_forgotten
                            );
                        }
                    };

                    match hebbs_vault::index(&vault_path, &engine, &embedder, Some(&progress_cb))
                        .await
                    {
                        Ok(result) => {
                            println!(
                                "\nIndexed {} files ({} total sections)",
                                result.total_files,
                                result.phase1.sections_new
                                    + result.phase1.sections_modified
                                    + result.phase1.sections_unchanged
                            );
                            0
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            1
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error setting up engine: {}", e);
                    1
                }
            }
        }

        Commands::Watch { vault_path } => {
            match setup_engine(&vault_path).await {
                Ok((engine, embedder)) => {
                    let cancel = tokio_util::sync::CancellationToken::new();
                    let cancel_clone = cancel.clone();

                    // Handle Ctrl-C
                    tokio::spawn(async move {
                        tokio::signal::ctrl_c().await.ok();
                        println!("\nShutting down...");
                        cancel_clone.cancel();
                    });

                    match hebbs_vault::watcher::watch_vault(
                        vault_path,
                        Arc::new(engine),
                        embedder,
                        cancel,
                    )
                    .await
                    {
                        Ok(stats) => {
                            println!(
                                "Watcher stopped. Events: {}, Phase 1 runs: {}, Phase 2 runs: {}, Bursts: {}",
                                stats.events_received, stats.phase1_runs, stats.phase2_runs, stats.burst_detections
                            );
                            0
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            1
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error setting up engine: {}", e);
                    1
                }
            }
        }

        Commands::Rebuild { vault_path } => {
            match setup_engine(&vault_path).await {
                Ok((engine, embedder)) => {
                    println!("Rebuilding vault at {}...", vault_path.display());
                    match hebbs_vault::rebuild(&vault_path, &engine, &embedder, None).await {
                        Ok(result) => {
                            println!(
                                "Rebuilt: {} files, {} sections indexed",
                                result.total_files,
                                result.phase2.sections_embedded
                            );
                            0
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            1
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error setting up engine: {}", e);
                    1
                }
            }
        }

        Commands::Recall {
            vault_path, query, top_k, strategy,
            entity_id, w_relevance, w_recency, w_importance, w_reinforcement,
            ef_search, max_depth, causal_direction, seed_id,
        } => {
            match setup_engine(&vault_path).await {
                Ok((engine, _embedder)) => {
                    let hebbs_dir = vault_path.join(".hebbs");
                    let manifest = match hebbs_vault::manifest::Manifest::load(&hebbs_dir) {
                        Ok(m) => m,
                        Err(e) => {
                            eprintln!("Error loading manifest: {}", e);
                            return 1;
                        }
                    };

                    let vault_query = hebbs_vault::query::VaultQuery::new(
                        &engine, &manifest, &vault_path,
                    );

                    // Parse strategies
                    let strategies: Vec<hebbs_core::recall::RecallStrategy> = strategy
                        .split(',')
                        .filter_map(|s| match s.trim().to_lowercase().as_str() {
                            "similarity" | "sim" => Some(hebbs_core::recall::RecallStrategy::Similarity),
                            "temporal" | "temp" => Some(hebbs_core::recall::RecallStrategy::Temporal),
                            "causal" => Some(hebbs_core::recall::RecallStrategy::Causal),
                            "analogical" | "analogy" => Some(hebbs_core::recall::RecallStrategy::Analogical),
                            other => {
                                eprintln!("Unknown strategy: '{}'. Options: similarity, temporal, causal, analogical", other);
                                None
                            }
                        })
                        .collect();

                    if strategies.is_empty() {
                        eprintln!("No valid strategies specified");
                        return 1;
                    }

                    // Build scoring weights if any overrides provided
                    let scoring_weights = if w_relevance.is_some() || w_recency.is_some()
                        || w_importance.is_some() || w_reinforcement.is_some()
                    {
                        let defaults = hebbs_core::recall::ScoringWeights::default();
                        let raw_rel = w_relevance.unwrap_or(defaults.w_relevance);
                        let raw_rec = w_recency.unwrap_or(defaults.w_recency);
                        let raw_imp = w_importance.unwrap_or(defaults.w_importance);
                        let raw_rnf = w_reinforcement.unwrap_or(defaults.w_reinforcement);
                        let sum = raw_rel + raw_rec + raw_imp + raw_rnf;
                        let norm = if sum > 0.0 { sum } else { 1.0 };
                        Some(hebbs_core::recall::ScoringWeights {
                            w_relevance: raw_rel / norm,
                            w_recency: raw_rec / norm,
                            w_importance: raw_imp / norm,
                            w_reinforcement: raw_rnf / norm,
                            ..defaults
                        })
                    } else {
                        None
                    };

                    // Parse causal direction
                    let causal_dir = causal_direction.as_deref().map(|d| match d.to_lowercase().as_str() {
                        "forward" | "fwd" => hebbs_core::recall::CausalDirection::Forward,
                        "backward" | "bwd" => hebbs_core::recall::CausalDirection::Backward,
                        _ => hebbs_core::recall::CausalDirection::Both,
                    });

                    // Parse seed memory ID
                    let seed_memory_id = seed_id.as_deref().and_then(|id| {
                        id.parse::<ulid::Ulid>().ok().map(|u| u.0.to_be_bytes())
                    });

                    // Print config
                    let strategy_names: Vec<&str> = strategies.iter().map(|s| match s {
                        hebbs_core::recall::RecallStrategy::Similarity => "similarity",
                        hebbs_core::recall::RecallStrategy::Temporal => "temporal",
                        hebbs_core::recall::RecallStrategy::Causal => "causal",
                        hebbs_core::recall::RecallStrategy::Analogical => "analogical",
                    }).collect();

                    let input = hebbs_core::recall::RecallInput {
                        cue: query.clone(),
                        strategies,
                        top_k: Some(top_k),
                        entity_id,
                        time_range: None,
                        edge_types: None,
                        max_depth,
                        ef_search,
                        scoring_weights,
                        cue_context: None,
                        causal_direction: causal_dir,
                        analogy_a_id: None,
                        analogy_b_id: None,
                        seed_memory_id,
                        analogical_alpha: None,
                    };

                    match engine.recall(input) {
                        Ok(output) => {
                            if output.results.is_empty() {
                                println!("No results found for: \"{}\"", query);
                                return 0;
                            }
                            println!("Query:      \"{}\"", query);
                            println!("Strategies: {}", strategy_names.join(", "));
                            if let Some(ref sw) = scoring_weights {
                                println!("Weights:    relevance={:.2} recency={:.2} importance={:.2} reinforcement={:.2}",
                                    sw.w_relevance, sw.w_recency, sw.w_importance, sw.w_reinforcement);
                            }
                            println!();
                            println!("Found {} result(s):\n", output.results.len());
                            for (i, result) in output.results.iter().enumerate() {
                                // Convert 16-byte ULID to Crockford base32 string (manifest format)
                                let memory_id_str = if result.memory.memory_id.len() == 16 {
                                    let mut bytes = [0u8; 16];
                                    bytes.copy_from_slice(&result.memory.memory_id);
                                    ulid::Ulid::from_bytes(bytes).to_string()
                                } else {
                                    hex::encode(&result.memory.memory_id)
                                };

                                let file_path = vault_query
                                    .file_path_for_memory(&memory_id_str)
                                    .unwrap_or("(unknown)");

                                // Try to get fresh content from file
                                let content = vault_query
                                    .read_section_content(&memory_id_str)
                                    .map(|(c, _stale)| c)
                                    .unwrap_or_else(|| result.memory.content.clone());

                                // Truncate for display
                                let display_content = if content.len() > 300 {
                                    format!("{}...", &content[..300])
                                } else {
                                    content
                                };

                                println!("--- Result {} (score: {:.4}) ---", i + 1, result.score);
                                println!("File:       {}", file_path);
                                println!("ID:         {}", &memory_id_str[..16]);
                                println!("Importance: {:.2}", result.memory.importance);
                                // Show all strategy details
                                for detail in &result.strategy_details {
                                    match detail {
                                        hebbs_core::recall::StrategyDetail::Similarity { distance, relevance } => {
                                            println!("Similarity: relevance={:.4} distance={:.4}", relevance, distance);
                                        }
                                        hebbs_core::recall::StrategyDetail::Temporal { timestamp, .. } => {
                                            println!("Temporal:   timestamp={}", timestamp);
                                        }
                                        hebbs_core::recall::StrategyDetail::Causal { depth, edge_type, .. } => {
                                            println!("Causal:     depth={} edge={:?}", depth, edge_type);
                                        }
                                        hebbs_core::recall::StrategyDetail::Analogical { structural_similarity, embedding_similarity, .. } => {
                                            println!("Analogical: structural={:.4} embedding={:.4}", structural_similarity, embedding_similarity);
                                        }
                                    }
                                }
                                println!("Content:    {}", display_content.trim());
                                println!();
                            }
                            if let Some(dur) = output.embed_duration_us {
                                println!("Embed latency: {} us", dur);
                            }
                            // Show strategy errors if any
                            for err in &output.strategy_errors {
                                eprintln!("Strategy error: {:?}", err);
                            }
                            0
                        }
                        Err(e) => {
                            eprintln!("Recall error: {}", e);
                            1
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error setting up engine: {}", e);
                    1
                }
            }
        }

        Commands::List { vault_path, sections } => {
            let hebbs_dir = vault_path.join(".hebbs");
            if !hebbs_dir.exists() {
                eprintln!("Error: vault not initialized at {}", vault_path.display());
                return 1;
            }
            match hebbs_vault::manifest::Manifest::load(&hebbs_dir) {
                Ok(manifest) => {
                    let mut files: Vec<_> = manifest.files.iter().collect();
                    files.sort_by_key(|(path, _)| (*path).clone());

                    println!("Vault: {}\n", vault_path.display());
                    for (path, entry) in &files {
                        let section_count = entry.sections.len();
                        let synced = entry.sections.iter()
                            .filter(|s| matches!(s.state, hebbs_vault::manifest::SectionState::Synced))
                            .count();
                        println!("  {} ({} sections, {} synced)", path, section_count, synced);

                        if sections {
                            for sec in &entry.sections {
                                let heading = if sec.heading_path.is_empty() {
                                    "(root)".to_string()
                                } else {
                                    sec.heading_path.join(" > ")
                                };
                                println!(
                                    "    [{:?}] {} (id: {}..., bytes: {}..{})",
                                    sec.state,
                                    heading,
                                    &sec.memory_id[..16],
                                    sec.byte_start,
                                    sec.byte_end,
                                );
                            }
                        }
                    }
                    println!("\nTotal: {} files, {} sections",
                        files.len(),
                        files.iter().map(|(_, e)| e.sections.len()).sum::<usize>(),
                    );
                    0
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }

        Commands::Status { vault_path } => {
            match hebbs_vault::status(&vault_path) {
                Ok(s) => {
                    println!("Vault: {}", s.vault_root.display());
                    println!();
                    println!("Files:    {} indexed", s.total_files);
                    println!("Sections: {} total", s.total_sections);
                    println!("  synced:        {}", s.synced);
                    println!("  content-stale: {}", s.content_stale);
                    println!("  orphaned:      {}", s.orphaned);
                    if let Some(lp) = s.last_parsed {
                        println!();
                        println!("Last phase 1: {}", lp.format("%Y-%m-%d %H:%M:%S UTC"));
                    }
                    if let Some(le) = s.last_embedded {
                        println!("Last phase 2: {}", le.format("%Y-%m-%d %H:%M:%S UTC"));
                    }
                    0
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }
    }
}

/// Set up the embedded engine for vault operations.
/// Uses the .hebbs/ directory inside the vault for RocksDB storage.
async fn setup_engine(
    vault_path: &std::path::Path,
) -> std::result::Result<(hebbs_core::engine::Engine, Arc<dyn hebbs_embed::Embedder>), Box<dyn std::error::Error>> {
    let hebbs_dir = vault_path.join(".hebbs");
    if !hebbs_dir.exists() {
        return Err(Box::new(VaultError::NotInitialized {
            path: vault_path.to_path_buf(),
        }));
    }

    let _config = VaultConfig::load(&hebbs_dir)?;

    // Set up RocksDB storage in .hebbs/index/
    let db_path = hebbs_dir.join("index").join("db");
    std::fs::create_dir_all(&db_path)?;
    let storage = Arc::new(hebbs_storage::RocksDbBackend::open(&db_path)?);

    // Set up embedder (model stored alongside the index)
    let model_dir = hebbs_dir.join("index");
    let embed_config = hebbs_embed::EmbedderConfig::default_bge_small(&model_dir);
    let embedder: Arc<dyn hebbs_embed::Embedder> =
        Arc::new(hebbs_embed::OnnxEmbedder::new(embed_config)?);

    let engine = hebbs_core::engine::Engine::new(storage, embedder.clone())?;

    Ok((engine, embedder))
}
