use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::{fmt, EnvFilter};

use hebbs_core::engine::{Engine, RememberEdge, RememberInput};
use hebbs_core::forget::ForgetCriteria;
use hebbs_core::memory::MemoryKind;
use hebbs_core::recall::{PrimeInput, RecallInput, RecallStrategy, ScoringWeights};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig, ReflectScope};
use hebbs_core::tenant::TenantContext;
use hebbs_embed::Embedder;
use hebbs_index::graph::EdgeType;
use hebbs_vault::config::VaultConfig;
use hebbs_vault::daemon::client;
use hebbs_vault::daemon::protocol::{
    Command as DaemonCommand, DaemonRequest, DaemonResponse, ResponseStatus,
};
use hebbs_vault::error::VaultError;

// ═══════════════════════════════════════════════════════════════════════
//  CLI Definition
// ═══════════════════════════════════════════════════════════════════════

#[derive(Parser)]
#[command(name = "hebbs")]
#[command(version)]
#[command(about = "HEBBS: the cognitive memory engine")]
struct Cli {
    /// Vault path (overrides brain discovery)
    #[arg(long, global = true, env = "HEBBS_VAULT")]
    vault: Option<PathBuf>,

    /// Use global brain (~/.hebbs/) instead of project brain
    #[arg(long, global = true)]
    global: bool,

    /// Server gRPC endpoint (enables remote mode)
    #[arg(long, global = true, env = "HEBBS_ENDPOINT")]
    endpoint: Option<String>,

    /// Server HTTP port (for metrics, remote mode)
    #[arg(long, global = true, env = "HEBBS_HTTP_PORT")]
    http_port: Option<u16>,

    /// Request timeout in milliseconds (remote mode)
    #[arg(long, global = true, env = "HEBBS_TIMEOUT")]
    timeout: Option<u64>,

    /// Output format
    #[arg(long, global = true, value_enum)]
    format: Option<FormatArg>,

    /// Color output
    #[arg(long, global = true, value_enum)]
    color: Option<ColorArg>,

    /// API key for authentication (remote mode)
    #[arg(long, global = true, env = "HEBBS_API_KEY")]
    api_key: Option<String>,

    /// Explicit tenant ID
    #[arg(long, global = true, env = "HEBBS_TENANT")]
    tenant: Option<String>,

    /// Verbose mode (-v for debug, -vv for trace)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
enum FormatArg {
    Human,
    Json,
    Raw,
}

#[derive(Clone, ValueEnum)]
enum ColorArg {
    Always,
    Never,
    Auto,
}

#[derive(Subcommand)]
enum Commands {
    // ── Vault commands (local only) ────────────────────────────────
    /// Initialize a new vault (.hebbs/ directory)
    Init {
        /// Path to the vault directory
        vault_path: Option<PathBuf>,
        /// Reinitialize even if .hebbs/ already exists
        #[arg(long)]
        force: bool,
        /// LLM provider: anthropic, openai, gemini, ollama
        #[arg(long)]
        provider: Option<String>,
        /// LLM model identifier (e.g. claude-haiku-4-5-20251001)
        #[arg(long)]
        model: Option<String>,
        /// Environment variable name holding the API key
        #[arg(long)]
        api_key_env: Option<String>,
        /// LLM API key (prefer --api-key-env for security)
        #[arg(long)]
        api_key: Option<String>,
        /// Base URL override for LLM provider
        #[arg(long)]
        base_url: Option<String>,
        /// Store LLM config in this vault only (default: save to global ~/.hebbs/config.toml)
        #[arg(long)]
        local: bool,
    },

    /// View or modify vault configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Index all markdown files in the vault
    Index {
        /// Path to the vault directory
        vault_path: Option<PathBuf>,
    },

    /// Watch vault for file changes and sync in real-time
    Watch {
        /// Path to the vault directory
        vault_path: Option<PathBuf>,
    },

    /// Delete .hebbs/ and rebuild index from scratch
    Rebuild {
        /// Path to the vault directory
        vault_path: Option<PathBuf>,
    },

    /// List all indexed files and their sections
    List {
        /// Path to the vault directory
        vault_path: Option<PathBuf>,
        /// Show section details (headings, memory IDs, states)
        #[arg(long)]
        sections: bool,
    },

    /// Start the daemon (serves all vaults via Unix socket + panel HTTP)
    Serve {
        /// Run in foreground (do not daemonize)
        #[arg(long)]
        foreground: bool,
        /// Idle shutdown timeout in seconds (0 to disable)
        #[arg(long, default_value = "300")]
        idle_timeout: u64,
        /// HTTP port for the Memory Palace panel (0 to disable)
        #[arg(long, default_value = "6381")]
        panel_port: u16,
        /// Initial vault path for the panel to open on startup
        #[arg(long)]
        initial_vault: Option<PathBuf>,
    },

    /// Open the Memory Palace control panel in a browser
    Panel {
        /// Path to the vault directory (used in standalone mode if daemon is not running)
        vault_path: Option<PathBuf>,
        /// HTTP port for the panel server
        #[arg(long, default_value = "6381")]
        port: u16,
    },

    // ── Memory commands (both modes) ──────────────────────────────
    /// Store a new memory
    Remember {
        /// Content text (reads from stdin if not provided and stdin is a pipe)
        content: Option<String>,
        /// Importance score (0.0 to 1.0)
        #[arg(short, long)]
        importance: Option<f32>,
        /// Context as JSON object
        #[arg(short, long)]
        context: Option<String>,
        /// Entity ID for scoping
        #[arg(short, long)]
        entity_id: Option<String>,
        /// Graph edges (repeatable). Format: TARGET_ID:EDGE_TYPE[:CONFIDENCE]
        #[arg(long)]
        edge: Vec<String>,
    },

    /// Retrieve a memory by ID
    Get {
        /// Memory ID (ULID string or hex)
        id: String,
    },

    /// Recall memories using one or more strategies
    Recall {
        /// Search cue text
        cue: Option<String>,
        /// Recall strategy
        #[arg(short, long, value_enum)]
        strategy: Option<StrategyArg>,
        /// Maximum results to return
        #[arg(short = 'k', long, default_value = "5")]
        top_k: u32,
        /// Entity ID (required for temporal strategy)
        #[arg(short, long)]
        entity_id: Option<String>,
        /// Maximum graph traversal depth (causal strategy)
        #[arg(long)]
        max_depth: Option<u32>,
        /// Seed memory ID for causal strategy
        #[arg(long)]
        seed: Option<String>,
        /// Scoring weights as "relevance:recency:importance:reinforcement"
        #[arg(short, long, value_name = "R:T:I:F")]
        weights: Option<String>,
        /// Override HNSW ef_search for this query
        #[arg(long)]
        ef_search: Option<u32>,
        /// Edge types to follow in causal traversal (comma-separated)
        #[arg(long, value_delimiter = ',')]
        edge_types: Option<Vec<String>>,
        /// Time range for temporal strategy as START_US:END_US
        #[arg(long, value_name = "START:END")]
        time_range: Option<String>,
        /// Analogical alpha: blends embedding similarity (1.0) vs structural (0.0)
        #[arg(long)]
        analogical_alpha: Option<f32>,
        /// Context as JSON object
        #[arg(short, long)]
        context: Option<String>,
        /// Search both project and global vaults, merge results by score
        #[arg(long)]
        all: bool,
    },

    /// Update an existing memory
    Revise {
        /// Memory ID to revise
        id: String,
        /// New content
        #[arg(short = 'C', long)]
        content: Option<String>,
        /// New importance
        #[arg(short, long)]
        importance: Option<f32>,
        /// Context as JSON object
        #[arg(short = 'x', long)]
        context: Option<String>,
        /// Context merge mode
        #[arg(long, value_enum, default_value = "merge")]
        context_mode: ContextModeArg,
        /// New entity ID
        #[arg(short, long)]
        entity_id: Option<String>,
        /// Graph edges (repeatable)
        #[arg(long)]
        edge: Vec<String>,
    },

    /// Remove memories matching criteria
    Forget {
        /// Specific memory IDs to forget
        #[arg(short, long)]
        ids: Vec<String>,
        /// Entity ID scope
        #[arg(short, long)]
        entity_id: Option<String>,
        /// Staleness threshold in microseconds
        #[arg(long)]
        staleness_us: Option<u64>,
        /// Minimum access count floor
        #[arg(long)]
        access_floor: Option<u64>,
        /// Memory kind filter
        #[arg(long, value_enum)]
        kind: Option<KindArg>,
        /// Decay score floor
        #[arg(long)]
        decay_floor: Option<f32>,
    },

    /// Pre-load context for an entity
    Prime {
        /// Entity ID
        entity_id: String,
        /// Context as JSON object
        #[arg(short, long)]
        context: Option<String>,
        /// Maximum memories to return
        #[arg(short, long)]
        max_memories: Option<u32>,
        /// Recency window in microseconds
        #[arg(long)]
        recency_us: Option<u64>,
        /// Similarity cue text
        #[arg(long)]
        similarity_cue: Option<String>,
        /// Search both project and global vaults, merge results by score
        #[arg(long)]
        all: bool,
    },

    // ── Subscription commands (remote only) ───────────────────────
    /// Start a subscription stream
    Subscribe {
        /// Entity ID scope
        #[arg(short, long)]
        entity_id: Option<String>,
        /// Confidence threshold (0.0 to 1.0)
        #[arg(short, long, default_value = "0.5")]
        confidence: f32,
    },

    /// Feed text to an active subscription
    Feed {
        /// Subscription ID
        subscription_id: u64,
        /// Text to feed
        text: String,
    },

    // ── Reflect commands (both modes) ─────────────────────────────
    /// Trigger reflection pipeline
    Reflect {
        /// Entity ID scope (omit for global)
        #[arg(short, long)]
        entity_id: Option<String>,
        /// Only process memories since this timestamp (microseconds)
        #[arg(long)]
        since_us: Option<u64>,
    },

    /// Prepare reflection data for agent-driven two-step reflect
    ReflectPrepare {
        /// Entity ID scope (omit for global)
        #[arg(short, long)]
        entity_id: Option<String>,
        /// Only process memories since this timestamp (microseconds)
        #[arg(long)]
        since_us: Option<u64>,
    },

    /// Commit agent-produced insights from a previous reflect-prepare session
    ReflectCommit {
        /// Session ID from reflect-prepare output
        #[arg(short, long)]
        session_id: String,
        /// JSON array of insights to commit
        #[arg(short, long)]
        insights: String,
    },

    /// List pending contradiction candidates for AI review
    ContradictionPrepare {},

    /// Commit AI-reviewed contradiction verdicts
    ContradictionCommit {
        /// JSON array of verdicts
        #[arg(short, long)]
        results: String,
    },

    /// Query consolidated insights
    Insights {
        /// Entity ID filter
        #[arg(short, long)]
        entity_id: Option<String>,
        /// Minimum confidence threshold
        #[arg(short, long)]
        min_confidence: Option<f32>,
        /// Maximum results
        #[arg(short = 'k', long)]
        max_results: Option<u32>,
    },

    // ── Utility commands ──────────────────────────────────────────
    /// Show brain status (vault status in local mode, server health in remote mode)
    Status {
        /// Path to the vault directory (local mode only)
        vault_path: Option<PathBuf>,
    },

    /// Inspect a memory (detail + graph edges + neighbors)
    Inspect {
        /// Memory ID
        id: String,
    },

    /// Export memories as JSONL
    Export {
        /// Entity ID scope
        #[arg(short, long)]
        entity_id: Option<String>,
        /// Maximum memories to export
        #[arg(short, long, default_value = "1000")]
        limit: u32,
    },

    /// View the query audit log
    Queries {
        /// Maximum entries to return
        #[arg(short, long, default_value = "20")]
        limit: u32,
        /// Offset for pagination
        #[arg(long, default_value = "0")]
        offset: u32,
        /// Filter by caller name (e.g. "cli", "hebbs-panel", "mcp:cursor")
        #[arg(short, long)]
        caller: Option<String>,
        /// Filter by operation type (recall, prime)
        #[arg(short = 'o', long)]
        operation: Option<String>,
    },

    /// Display server metrics (remote mode)
    Metrics,

    /// Stop the running daemon
    Stop,

    /// Print version
    Version,
}

#[derive(Clone, ValueEnum)]
enum StrategyArg {
    Similarity,
    Temporal,
    Causal,
    Analogical,
}

#[derive(Clone, ValueEnum)]
enum ContextModeArg {
    Merge,
    Replace,
}

#[derive(Clone, ValueEnum)]
enum KindArg {
    Episode,
    Insight,
    Revision,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set a configuration value (dot-notation: llm.provider, llm.model)
    Set {
        /// Configuration key (dot-notation)
        key: String,
        /// Value to set
        value: String,
        /// Apply to global config (~/.hebbs/config.toml)
        #[arg(long)]
        global: bool,
    },
    /// Get a configuration value
    Get {
        /// Configuration key (dot-notation)
        key: String,
        /// Read from global config (~/.hebbs/config.toml)
        #[arg(long)]
        global: bool,
    },
    /// Show full configuration
    Show {
        /// Show global config only (~/.hebbs/config.toml)
        #[arg(long)]
        global: bool,
    },
}

// ═══════════════════════════════════════════════════════════════════════
//  Entry Point
// ═══════════════════════════════════════════════════════════════════════

fn main() {
    let cli = Cli::parse();

    init_tracing(cli.verbose);

    // The daemon needs more worker threads than the CLI because it handles
    // concurrent connections with blocking ONNX embedding calls. With only 2
    // threads, concurrent embeds starve the idle timer and other async tasks.
    let worker_threads = match &cli.command {
        Commands::Serve { .. } => 4,
        _ => 2,
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");

    let exit_code = rt.block_on(run(cli));
    std::process::exit(exit_code);
}

fn init_tracing(verbosity: u8) {
    let filter = match verbosity {
        0 => "hebbs_vault=info,hebbs=info",
        1 => "hebbs_vault=debug,hebbs=debug,hebbs_core=debug",
        _ => "hebbs_vault=trace,hebbs=trace,hebbs_core=trace",
    };

    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_target(false)
        .with_writer(io::stderr)
        .init();
}

// ═══════════════════════════════════════════════════════════════════════
//  Mode Detection & Dispatch
// ═══════════════════════════════════════════════════════════════════════

/// Determine whether to use remote, daemon, or direct-local mode and dispatch.
async fn run(cli: Cli) -> i32 {
    // Remote mode: --endpoint or HEBBS_ENDPOINT is set
    if cli.endpoint.is_some() {
        return run_remote(cli).await;
    }

    // Commands that bypass the daemon entirely
    match &cli.command {
        Commands::Init { .. }
        | Commands::Config { .. }
        | Commands::Version
        | Commands::Serve { .. }
        | Commands::Panel { .. }
        | Commands::Stop => {
            return run_local(cli).await;
        }
        _ => {}
    }

    // HEBBS_NO_DAEMON=1 forces direct local mode (useful for testing)
    if std::env::var("HEBBS_NO_DAEMON").unwrap_or_default() == "1" {
        return run_local(cli).await;
    }

    // Daemon mode: route through the daemon for warm engine access
    run_daemon_mode(cli).await
}

// ═══════════════════════════════════════════════════════════════════════
//  Brain Discovery (Local Mode)
// ═══════════════════════════════════════════════════════════════════════

/// Resolve the vault path for local mode commands.
/// Priority: --global flag > explicit positional arg > --vault flag > walk up for .hebbs/ > ~/.hebbs/
fn resolve_vault_path(
    explicit: Option<&PathBuf>,
    cli_vault: Option<&PathBuf>,
    use_global: bool,
) -> Option<PathBuf> {
    // 0. --global flag: go straight to ~/.hebbs/
    if use_global {
        if let Some(home) = dirs::home_dir() {
            let global = home.join(".hebbs");
            if global.exists() {
                return Some(home.canonicalize().unwrap_or(home));
            }
        }
        return None;
    }

    // 1. Explicit positional arg from the command
    if let Some(p) = explicit {
        // Canonicalize so the daemon gets an absolute path regardless of CWD
        return Some(p.canonicalize().unwrap_or_else(|_| p.clone()));
    }

    // 2. Global --vault flag or HEBBS_VAULT env var
    if let Some(p) = cli_vault {
        return Some(p.canonicalize().unwrap_or_else(|_| p.clone()));
    }

    // 3. Walk up from current directory looking for .hebbs/
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            if dir.join(".hebbs").exists() {
                return Some(dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()));
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }

    // 4. Fall back to ~/.hebbs/ (global brain)
    if let Some(home) = dirs::home_dir() {
        let global = home.join(".hebbs");
        if global.exists() {
            return Some(home.canonicalize().unwrap_or(home));
        }
    }

    None
}

/// Resolve vault path, returning an error message if not found.
fn require_vault_path(
    explicit: Option<&PathBuf>,
    cli_vault: Option<&PathBuf>,
    use_global: bool,
) -> Result<PathBuf, i32> {
    match resolve_vault_path(explicit, cli_vault, use_global) {
        Some(p) => Ok(p),
        None => {
            eprintln!("No brain found. Run: hebbs init <path>");
            Err(1)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Engine Setup
// ═══════════════════════════════════════════════════════════════════════

async fn setup_engine(
    vault_path: &Path,
) -> Result<(Engine, Arc<dyn Embedder>), Box<dyn std::error::Error>> {
    let hebbs_dir = vault_path.join(".hebbs");
    if !hebbs_dir.exists() {
        return Err(Box::new(VaultError::NotInitialized {
            path: vault_path.to_path_buf(),
        }));
    }

    let config = VaultConfig::load(&hebbs_dir)?;

    let db_path = hebbs_dir.join("index").join("db");
    std::fs::create_dir_all(&db_path)?;
    let storage = Arc::new(hebbs_storage::RocksDbBackend::open(&db_path)?);

    // Use OS cache dir for model files (macOS: ~/Library/Caches/hebbs, Linux: ~/.cache/hebbs).
    // This survives vault and daemon cleanup, so model is only downloaded once ever.
    let embed_config = hebbs_embed::EmbedderConfig::from_model_name_cached(&config.embedding.model);
    std::fs::create_dir_all(&embed_config.model_dir)?;
    let embedder: Arc<dyn Embedder> = Arc::new(hebbs_embed::OnnxEmbedder::new(embed_config)?);

    let engine = Engine::new(storage, embedder.clone())?;

    Ok((engine, embedder))
}

// ═══════════════════════════════════════════════════════════════════════
//  Vault Registry (~/.hebbs/vaults.json)
// ═══════════════════════════════════════════════════════════════════════

/// Register a vault path in `~/.hebbs/vaults.json`.
///
/// The registry is a simple JSON file that tracks all known vault paths
/// so the control panel can offer a vault switcher dropdown.
/// Best-effort: failures are logged but do not block init.
fn register_vault(vault_path: &Path) {
    let canonical = match vault_path.canonicalize() {
        Ok(p) => p,
        Err(_) => vault_path.to_path_buf(),
    };

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            tracing::debug!("no home directory found, skipping vault registration");
            return;
        }
    };

    let global_hebbs = home.join(".hebbs");
    if !global_hebbs.exists() {
        // Create ~/.hebbs/ for the vault registry. This directory only
        // contains vaults.json and daemon state -- it is not a vault
        // itself (no config.toml), so discovery logic won't mistake it
        // for one.
        if let Err(e) = std::fs::create_dir_all(&global_hebbs) {
            tracing::debug!("failed to create ~/.hebbs for registry: {}", e);
            return;
        }
    }

    let registry_path = global_hebbs.join("vaults.json");

    // Read existing registry or create empty
    let mut registry: serde_json::Value = if registry_path.exists() {
        match std::fs::read_to_string(&registry_path) {
            Ok(content) => serde_json::from_str(&content)
                .unwrap_or_else(|_| serde_json::json!({ "vaults": [] })),
            Err(_) => serde_json::json!({ "vaults": [] }),
        }
    } else {
        serde_json::json!({ "vaults": [] })
    };

    let vaults = registry.get_mut("vaults").and_then(|v| v.as_array_mut());

    let vaults = match vaults {
        Some(v) => v,
        None => {
            registry = serde_json::json!({ "vaults": [] });
            registry["vaults"].as_array_mut().unwrap()
        }
    };

    let canonical_str = canonical.display().to_string();

    // Derive a label from the directory name
    let canonical_home = home.canonicalize().unwrap_or_else(|_| home.clone());
    let label = if canonical == canonical_home {
        "global".to_string()
    } else {
        canonical
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| canonical_str.clone())
    };

    // Check if already registered (by path)
    let already = vaults
        .iter()
        .any(|entry| entry.get("path").and_then(|p| p.as_str()) == Some(&canonical_str));

    if !already {
        vaults.push(serde_json::json!({
            "path": canonical_str,
            "label": label
        }));

        match serde_json::to_string_pretty(&registry) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&registry_path, json) {
                    tracing::debug!("could not write vault registry: {}", e);
                }
            }
            Err(e) => {
                tracing::debug!("could not serialize vault registry: {}", e);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Local Mode
// ═══════════════════════════════════════════════════════════════════════

async fn run_local(cli: Cli) -> i32 {
    match cli.command {
        // ── Vault lifecycle ───────────────────────────────────────
        Commands::Init {
            ref vault_path,
            force,
            ref provider,
            ref model,
            ref api_key_env,
            ref api_key,
            ref base_url,
            local,
        } => {
            let path = match vault_path.as_ref().or(cli.vault.as_ref()) {
                Some(p) => p.clone(),
                None => {
                    eprintln!("Error: vault path required. Usage: hebbs init <path>");
                    return 1;
                }
            };

            // Check if global config already has LLM configured
            let global_llm = VaultConfig::load_global()
                .ok()
                .filter(|g| g.llm.is_configured())
                .map(|g| g.llm);

            let (llm_config, save_to_global) = if provider.is_some() || model.is_some() {
                // Flags provided, use them directly.
                let resolved_key = api_key.clone().or_else(|| {
                    api_key_env
                        .as_ref()
                        .and_then(|env_name| std::env::var(env_name).ok())
                });
                let cfg = hebbs_vault::config::LlmConfig {
                    provider: provider.clone().unwrap_or_default(),
                    model: model.clone().unwrap_or_default(),
                    api_key: resolved_key,
                    api_key_env: api_key_env.clone(),
                    base_url: base_url.clone(),
                };
                // Save to global by default unless --local is specified
                (Some(cfg), !local)
            } else if global_llm.is_some() {
                // Global LLM exists: skip wizard, inherit from global.
                // Pass None so vault config has no [llm] section (inherits).
                println!("  Using LLM from global config (~/.hebbs/config.toml)");
                (None, false)
            } else if std::io::stdin().is_terminal() {
                // Interactive mode: ask the user which LLM provider to use.
                let cfg = interactive_llm_setup();
                // Save to global by default unless --local
                (cfg, !local)
            } else {
                // Non-interactive (piped stdin): LLM is required.
                eprintln!("Error: LLM provider is required. Use --provider and --model flags, or configure globally with `hebbs config set llm.provider <provider> --global`.");
                return 1;
            };

            // LLM is required (either explicit or inherited from global)
            if llm_config.is_none() && global_llm.is_none() {
                eprintln!("Error: LLM provider is required for HEBBS. Please select a provider.");
                return 1;
            }

            // Save LLM to global config if requested
            if save_to_global {
                if let Some(ref llm) = llm_config {
                    let mut global_cfg = VaultConfig::load_global().unwrap_or_default();
                    global_cfg.llm = llm.clone();
                    if let Err(e) = global_cfg.save_global() {
                        eprintln!("Warning: could not save global config: {}", e);
                        eprintln!("LLM config will be saved to vault config instead.");
                    } else {
                        println!("  Saved LLM config to ~/.hebbs/config.toml");
                    }
                }
            }

            // If saving to global, pass None so vault has no [llm] (inherits).
            // If --local, pass the config so it's written to the vault.
            let vault_llm = if save_to_global { None } else { llm_config };
            match hebbs_vault::init_with_llm(&path, force, vault_llm) {
                Ok(()) => {
                    register_vault(&path);
                    println!("Initialized vault at {}", path.display());

                    // Pre-download embedding model into OS cache dir so daemon starts instantly.
                    // ~/Library/Caches/hebbs on macOS — survives all vault/daemon cleanups.
                    let config = VaultConfig::load(&path.join(".hebbs")).unwrap_or_default();
                    let embed_config = hebbs_embed::EmbedderConfig::from_model_name_cached(
                        &config.embedding.model,
                    );
                    std::fs::create_dir_all(&embed_config.model_dir).ok();
                    println!("Ensuring embedding model ({})...", config.embedding.model);
                    match hebbs_embed::ensure_model_files(&embed_config) {
                        Ok(_) => println!("Embedding model ready."),
                        Err(e) => {
                            eprintln!("Warning: model download failed: {}", e);
                            eprintln!("The daemon will retry on first start.");
                        }
                    }

                    // Count markdown files so user knows what's coming
                    let md_count = count_md_files(&path);

                    // Start daemon (but do NOT auto-index; user runs `hebbs index` explicitly)
                    println!("Starting daemon...");
                    match client::ensure_daemon().await {
                        Ok(_) => {
                            println!();
                            println!("  Your vault is live. {} file(s) found.", md_count);
                            println!();
                            println!(
                                "  Run `hebbs index {}` to index your vault.",
                                path.display()
                            );
                            println!("  Then: hebbs recall \"your question here\"");
                        }
                        Err(e) => {
                            eprintln!("Warning: could not start daemon: {}", e);
                            eprintln!("Run `hebbs serve` then `hebbs index` to index your vault.");
                        }
                    }

                    0
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }

        Commands::Index { ref vault_path } => {
            let path = match require_vault_path(vault_path.as_ref(), cli.vault.as_ref(), cli.global)
            {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
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
                        hebbs_vault::IndexProgress::Phase2Started {
                            sections_to_process,
                        } => {
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
                                sections_embedded,
                                sections_remembered,
                                sections_revised,
                                sections_forgotten
                            );
                        }
                    };

                    match hebbs_vault::index(&path, &engine, &embedder, Some(&progress_cb)).await {
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

        Commands::Watch { ref vault_path } => {
            let path = match require_vault_path(vault_path.as_ref(), cli.vault.as_ref(), cli.global)
            {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, embedder)) => {
                    let cancel = tokio_util::sync::CancellationToken::new();
                    let cancel_clone = cancel.clone();

                    tokio::spawn(async move {
                        tokio::signal::ctrl_c().await.ok();
                        println!("\nShutting down...");
                        cancel_clone.cancel();
                    });

                    match hebbs_vault::watcher::watch_vault(
                        path,
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

        Commands::Rebuild { ref vault_path } => {
            let path = match require_vault_path(vault_path.as_ref(), cli.vault.as_ref(), cli.global)
            {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, embedder)) => {
                    println!("Rebuilding vault at {}...", path.display());
                    match hebbs_vault::rebuild(&path, &engine, &embedder, None).await {
                        Ok(result) => {
                            println!(
                                "Rebuilt: {} files, {} sections indexed",
                                result.total_files, result.phase2.sections_embedded
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

        Commands::List {
            ref vault_path,
            sections,
        } => {
            let path = match require_vault_path(vault_path.as_ref(), cli.vault.as_ref(), cli.global)
            {
                Ok(p) => p,
                Err(code) => return code,
            };
            let hebbs_dir = path.join(".hebbs");
            if !hebbs_dir.exists() {
                eprintln!("Error: vault not initialized at {}", path.display());
                return 1;
            }
            match hebbs_vault::manifest::Manifest::load(&hebbs_dir) {
                Ok(manifest) => {
                    let mut files: Vec<_> = manifest.files.iter().collect();
                    files.sort_by_key(|(p, _)| (*p).clone());

                    println!("Vault: {}\n", path.display());
                    for (fp, entry) in &files {
                        let section_count = entry.sections.len();
                        let synced = entry
                            .sections
                            .iter()
                            .filter(|s| {
                                matches!(s.state, hebbs_vault::manifest::SectionState::Synced)
                            })
                            .count();
                        println!("  {} ({} sections, {} synced)", fp, section_count, synced);

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
                    println!(
                        "\nTotal: {} files, {} sections",
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

        Commands::Status { ref vault_path } => {
            let path = match require_vault_path(vault_path.as_ref(), cli.vault.as_ref(), cli.global)
            {
                Ok(p) => p,
                Err(code) => return code,
            };
            match hebbs_vault::status(&path) {
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
                    if s.llm_provider.is_some() || s.llm_model.is_some() {
                        println!();
                        println!("LLM:");
                        if let Some(ref p) = s.llm_provider {
                            println!("  provider: {}", p);
                        }
                        if let Some(ref m) = s.llm_model {
                            println!("  model:    {}", m);
                        }
                    }
                    0
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }

        Commands::Config { ref action } => {
            match action {
                ConfigAction::Show { global } => {
                    if *global {
                        match VaultConfig::load_global() {
                            Ok(config) => match toml::to_string_pretty(&config) {
                                Ok(s) => {
                                    println!("{}", s);
                                    0
                                }
                                Err(e) => {
                                    eprintln!("Error serializing config: {}", e);
                                    1
                                }
                            },
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                1
                            }
                        }
                    } else {
                        let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                            Ok(p) => p,
                            Err(code) => return code,
                        };
                        let hebbs_dir = path.join(".hebbs");
                        // Show effective (merged) config with provenance annotations
                        let global_cfg = VaultConfig::load_global().ok();
                        let local_cfg = VaultConfig::load_local_only(&hebbs_dir).ok();
                        match VaultConfig::load(&hebbs_dir) {
                            Ok(effective) => {
                                config_show_with_provenance(
                                    &effective,
                                    global_cfg.as_ref(),
                                    local_cfg.as_ref(),
                                );
                                0
                            }
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                1
                            }
                        }
                    }
                }
                ConfigAction::Get { key, global } => {
                    if *global {
                        match VaultConfig::load_global() {
                            Ok(config) => match config_get(&config, key) {
                                Some(val) => {
                                    println!("{}", val);
                                    0
                                }
                                None => {
                                    eprintln!("Unknown config key: {}", key);
                                    1
                                }
                            },
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                1
                            }
                        }
                    } else {
                        let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                            Ok(p) => p,
                            Err(code) => return code,
                        };
                        let hebbs_dir = path.join(".hebbs");
                        match VaultConfig::load(&hebbs_dir) {
                            Ok(config) => match config_get(&config, key) {
                                Some(val) => {
                                    println!("{}", val);
                                    0
                                }
                                None => {
                                    eprintln!("Unknown config key: {}", key);
                                    1
                                }
                            },
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                1
                            }
                        }
                    }
                }
                ConfigAction::Set { key, value, global } => {
                    if *global {
                        match VaultConfig::load_global() {
                            Ok(mut config) => match config_set(&mut config, key, value) {
                                Ok(()) => match config.save_global() {
                                    Ok(()) => {
                                        println!("Set {} = {} (global)", key, value);
                                        0
                                    }
                                    Err(e) => {
                                        eprintln!("Error saving global config: {}", e);
                                        1
                                    }
                                },
                                Err(msg) => {
                                    eprintln!("Error: {}", msg);
                                    1
                                }
                            },
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                1
                            }
                        }
                    } else {
                        let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                            Ok(p) => p,
                            Err(code) => return code,
                        };
                        let hebbs_dir = path.join(".hebbs");
                        match VaultConfig::load_local_only(&hebbs_dir) {
                            Ok(mut config) => match config_set(&mut config, key, value) {
                                Ok(()) => match config.save(&hebbs_dir) {
                                    Ok(()) => {
                                        println!("Set {} = {}", key, value);
                                        0
                                    }
                                    Err(e) => {
                                        eprintln!("Error saving config: {}", e);
                                        1
                                    }
                                },
                                Err(msg) => {
                                    eprintln!("Error: {}", msg);
                                    1
                                }
                            },
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                1
                            }
                        }
                    }
                }
            }
        }

        // ── Memory commands (local mode) ──────────────────────────
        Commands::Remember {
            ref content,
            importance,
            ref context,
            ref entity_id,
            ref edge,
        } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let content_str = match read_content(content.clone()) {
                        Ok(c) => c,
                        Err(msg) => {
                            eprintln!("Error: {}", msg);
                            return 1;
                        }
                    };

                    let ctx = match parse_context(context.as_deref()) {
                        Ok(c) => c,
                        Err(msg) => {
                            eprintln!("Error: {}", msg);
                            return 1;
                        }
                    };

                    let edges = match parse_edges(edge) {
                        Ok(e) => e,
                        Err(msg) => {
                            eprintln!("Error: {}", msg);
                            return 1;
                        }
                    };

                    let input = RememberInput {
                        content: content_str,
                        importance,
                        context: ctx,
                        entity_id: entity_id.clone(),
                        edges,
                        kind: None,
                    };

                    match engine.remember(input) {
                        Ok(memory) => {
                            if is_json_format(&cli.format) {
                                let json = memory_to_json(&memory);
                                println!("{}", serde_json::to_string(&json).unwrap_or_default());
                            } else {
                                let preview = truncate(&memory.content, 80);
                                println!("Remembered: \"{}\"", preview);
                            }
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

        Commands::Get { ref id } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let memory_id = match parse_memory_id(id) {
                        Ok(id) => id,
                        Err(msg) => {
                            eprintln!("Error: {}", msg);
                            return 1;
                        }
                    };
                    match engine.get(&memory_id) {
                        Ok(memory) => {
                            if is_json_format(&cli.format) {
                                let json = memory_to_json(&memory);
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&json).unwrap_or_default()
                                );
                            } else {
                                print_memory_detail(&memory);
                            }
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
            ref cue,
            ref strategy,
            top_k,
            ref entity_id,
            max_depth,
            ref seed,
            ref weights,
            ef_search,
            ref edge_types,
            ..
        } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let cue_str = cue.clone().unwrap_or_default();
                    let strat = match strategy {
                        Some(StrategyArg::Similarity) | None => RecallStrategy::Similarity,
                        Some(StrategyArg::Temporal) => RecallStrategy::Temporal,
                        Some(StrategyArg::Causal) => RecallStrategy::Causal,
                        Some(StrategyArg::Analogical) => RecallStrategy::Analogical,
                    };

                    let scoring_weights = match weights {
                        Some(w) => match parse_scoring_weights(w) {
                            Ok(sw) => Some(sw),
                            Err(msg) => {
                                eprintln!("Error: {}", msg);
                                return 1;
                            }
                        },
                        None => None,
                    };

                    let seed_memory_id = seed.as_deref().and_then(|s| {
                        parse_memory_id(s).ok().map(|id| {
                            let mut arr = [0u8; 16];
                            arr.copy_from_slice(&id);
                            arr
                        })
                    });

                    let input = RecallInput {
                        cue: cue_str.clone(),
                        strategies: vec![strat],
                        top_k: Some(top_k as usize),
                        entity_id: entity_id.clone(),
                        time_range: None,
                        edge_types: edge_types.as_ref().and(None), // edge type parsing omitted for brevity in local
                        max_depth: max_depth.map(|d| d as usize),
                        ef_search: ef_search.map(|e| e as usize),
                        scoring_weights,
                        cue_context: None,
                        causal_direction: None,
                        analogy_a_id: None,
                        analogy_b_id: None,
                        seed_memory_id,
                        analogical_alpha: None,
                    };

                    match engine.recall(input) {
                        Ok(output) => {
                            if is_json_format(&cli.format) {
                                let results: Vec<serde_json::Value> = output
                                    .results
                                    .iter()
                                    .map(|r| {
                                        let mut m = memory_to_json(&r.memory);
                                        m["score"] = serde_json::json!(r.score);
                                        m
                                    })
                                    .collect();
                                println!("{}", serde_json::to_string(&results).unwrap_or_default());
                            } else {
                                if output.results.is_empty() {
                                    println!("No results found for: \"{}\"", cue_str);
                                } else {
                                    println!();
                                    for (i, r) in output.results.iter().enumerate() {
                                        let content_preview = truncate(&r.memory.content, 200);
                                        // Extract source from context
                                        let source = r.memory.context().ok().and_then(|ctx| {
                                            let file = ctx
                                                .get("file_path")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string());
                                            let heading = ctx
                                                .get("heading_path")
                                                .and_then(|v| v.as_array())
                                                .map(|arr| {
                                                    arr.iter()
                                                        .filter_map(|v| v.as_str())
                                                        .collect::<Vec<_>>()
                                                        .join(" > ")
                                                });
                                            match (file, heading) {
                                                (Some(f), Some(h)) if !h.is_empty() => {
                                                    Some(format!("{} > {}", f, h))
                                                }
                                                (Some(f), _) => Some(f),
                                                _ => None,
                                            }
                                        });
                                        println!("  {}. {}", i + 1, content_preview);
                                        if let Some(src) = source {
                                            println!("     Source: {}", src);
                                        }
                                        println!();
                                    }
                                }
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

        Commands::Forget {
            ref ids,
            ref entity_id,
            staleness_us,
            access_floor,
            ref kind,
            decay_floor,
        } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let memory_ids: Result<Vec<Vec<u8>>, String> =
                        ids.iter().map(|id| parse_memory_id(id)).collect();
                    let memory_ids = match memory_ids {
                        Ok(ids) => ids,
                        Err(msg) => {
                            eprintln!("Error: {}", msg);
                            return 1;
                        }
                    };

                    if memory_ids.is_empty()
                        && entity_id.is_none()
                        && staleness_us.is_none()
                        && access_floor.is_none()
                        && kind.is_none()
                        && decay_floor.is_none()
                    {
                        eprintln!("Error: at least one forget criteria is required");
                        return 1;
                    }

                    let memory_kind = kind.as_ref().map(|k| match k {
                        KindArg::Episode => MemoryKind::Episode,
                        KindArg::Insight => MemoryKind::Insight,
                        KindArg::Revision => MemoryKind::Revision,
                    });

                    let criteria = ForgetCriteria {
                        memory_ids,
                        entity_id: entity_id.clone(),
                        staleness_threshold_us: staleness_us,
                        access_count_floor: access_floor,
                        memory_kind,
                        decay_score_floor: decay_floor,
                    };

                    match engine.forget(criteria) {
                        Ok(output) => {
                            if is_json_format(&cli.format) {
                                println!(
                                    "{}",
                                    serde_json::json!({
                                        "forgotten_count": output.forgotten_count,
                                        "cascade_count": output.cascade_count,
                                        "truncated": output.truncated,
                                    })
                                );
                            } else {
                                println!(
                                    "Forgotten: {} memories ({} cascade)",
                                    output.forgotten_count, output.cascade_count
                                );
                                if output.truncated {
                                    println!("(truncated; more candidates remain)");
                                }
                            }
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

        Commands::Prime {
            ref entity_id,
            ref context,
            max_memories,
            recency_us,
            ref similarity_cue,
            ..
        } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let ctx = match parse_context(context.as_deref()) {
                        Ok(c) => c,
                        Err(msg) => {
                            eprintln!("Error: {}", msg);
                            return 1;
                        }
                    };

                    let input = PrimeInput {
                        entity_id: entity_id.clone(),
                        context: ctx,
                        max_memories: max_memories.map(|m| m as usize),
                        recency_window_us: recency_us,
                        similarity_cue: similarity_cue.clone(),
                        scoring_weights: None,
                    };

                    match engine.prime(input) {
                        Ok(output) => {
                            if is_json_format(&cli.format) {
                                let results: Vec<serde_json::Value> = output
                                    .results
                                    .iter()
                                    .map(|r| {
                                        let mut m = memory_to_json(&r.memory);
                                        m["score"] = serde_json::json!(r.score);
                                        m
                                    })
                                    .collect();
                                println!("{}", serde_json::to_string(&results).unwrap_or_default());
                            } else {
                                println!(
                                    "Primed {} memories ({} temporal, {} similarity)",
                                    output.results.len(),
                                    output.temporal_count,
                                    output.similarity_count,
                                );
                                for (i, r) in output.results.iter().enumerate() {
                                    println!(
                                        "  {}. [score {:.3}] {}",
                                        i + 1,
                                        r.score,
                                        truncate(&r.memory.content, 100)
                                    );
                                }
                            }
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

        Commands::ReflectPrepare {
            ref entity_id,
            since_us,
        } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let scope = build_reflect_scope(entity_id.clone(), since_us);
                    let config = ReflectConfig::default();
                    match engine.reflect_prepare_for_tenant(
                        &TenantContext::default(),
                        scope,
                        &config,
                    ) {
                        Ok(result) => {
                            if is_json_format(&cli.format) {
                                let clusters: Vec<serde_json::Value> = result
                                    .clusters
                                    .iter()
                                    .map(|c| {
                                        serde_json::json!({
                                            "cluster_id": c.cluster_id,
                                            "member_count": c.member_count,
                                            "system_prompt": c.proposal_system_prompt,
                                            "user_prompt": c.proposal_user_prompt,
                                            "memory_ids": c.memory_ids.iter().map(|id| format_memory_id(id)).collect::<Vec<_>>(),
                                        })
                                    })
                                    .collect();
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&serde_json::json!({
                                        "session_id": result.session_id,
                                        "memories_processed": result.memories_processed,
                                        "clusters": clusters,
                                    }))
                                    .unwrap_or_default()
                                );
                            } else {
                                println!("Session: {}", result.session_id);
                                println!(
                                    "Processed: {} memories, {} clusters",
                                    result.memories_processed,
                                    result.clusters.len()
                                );
                                for c in &result.clusters {
                                    println!(
                                        "  Cluster {}: {} members",
                                        c.cluster_id, c.member_count
                                    );
                                }
                            }
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

        Commands::ReflectCommit {
            ref session_id,
            ref insights,
        } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let parsed_insights = match parse_produced_insights(insights) {
                        Ok(i) => i,
                        Err(msg) => {
                            eprintln!("Error: {}", msg);
                            return 1;
                        }
                    };

                    match engine.reflect_commit_for_tenant(
                        &TenantContext::default(),
                        session_id,
                        parsed_insights,
                    ) {
                        Ok(result) => {
                            if is_json_format(&cli.format) {
                                println!(
                                    "{}",
                                    serde_json::json!({
                                        "insights_created": result.insights_created,
                                    })
                                );
                            } else {
                                println!("Committed: {} insights created", result.insights_created);
                            }
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

        Commands::ContradictionPrepare {} => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => match engine.contradiction_prepare() {
                    Ok(candidates) => {
                        if is_json_format(&cli.format) {
                            let items: Vec<serde_json::Value> = candidates
                                .iter()
                                .map(|c| {
                                    serde_json::json!({
                                        "id": hex::encode(c.id),
                                        "memory_id_a": hex::encode(c.memory_id_a),
                                        "memory_id_b": hex::encode(c.memory_id_b),
                                        "content_a_snippet": c.content_a_snippet,
                                        "content_b_snippet": c.content_b_snippet,
                                        "classifier_score": c.classifier_score,
                                        "similarity": c.similarity,
                                    })
                                })
                                .collect();
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "candidates": items,
                                    "count": candidates.len(),
                                }))
                                .unwrap_or_default()
                            );
                        } else {
                            println!(
                                "Found {} pending contradiction candidate(s)",
                                candidates.len()
                            );
                            for (i, c) in candidates.iter().enumerate() {
                                println!(
                                    "\n--- Candidate {} (score: {:.3}) ---",
                                    i + 1,
                                    c.classifier_score
                                );
                                println!("  Memory A: {}", hex::encode(c.memory_id_a));
                                println!("  Memory B: {}", hex::encode(c.memory_id_b));
                                println!("  Snippet A: {}", truncate(&c.content_a_snippet, 120));
                                println!("  Snippet B: {}", truncate(&c.content_b_snippet, 120));
                            }
                        }
                        0
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        1
                    }
                },
                Err(e) => {
                    eprintln!("Error setting up engine: {}", e);
                    1
                }
            }
        }

        Commands::ContradictionCommit { ref results } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let verdicts: Vec<hebbs_core::contradict::ContradictionVerdict> =
                        match serde_json::from_str(results) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("Error parsing verdicts JSON: {}", e);
                                return 1;
                            }
                        };

                    match engine.contradiction_commit(&verdicts) {
                        Ok(result) => {
                            if is_json_format(&cli.format) {
                                println!(
                                    "{}",
                                    serde_json::json!({
                                        "contradictions_confirmed": result.contradictions_confirmed,
                                        "revisions_created": result.revisions_created,
                                        "dismissed": result.dismissed,
                                    })
                                );
                            } else {
                                println!(
                                    "Committed: {} confirmed, {} revised, {} dismissed",
                                    result.contradictions_confirmed,
                                    result.revisions_created,
                                    result.dismissed
                                );
                            }
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

        Commands::Insights {
            ref entity_id,
            min_confidence,
            max_results,
        } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let filter = InsightsFilter {
                        entity_id: entity_id.clone(),
                        min_confidence,
                        max_results: max_results.map(|m| m as usize),
                    };

                    match engine.insights(filter) {
                        Ok(memories) => {
                            if is_json_format(&cli.format) {
                                let results: Vec<serde_json::Value> =
                                    memories.iter().map(memory_to_json).collect();
                                println!("{}", serde_json::to_string(&results).unwrap_or_default());
                            } else if memories.is_empty() {
                                println!("No insights found.");
                            } else {
                                println!("Found {} insight(s):\n", memories.len());
                                for (i, m) in memories.iter().enumerate() {
                                    println!(
                                        "  {}. [imp {:.2}] {}",
                                        i + 1,
                                        m.importance,
                                        truncate(&m.content, 150)
                                    );
                                }
                            }
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

        Commands::Inspect { ref id } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let memory_id = match parse_memory_id(id) {
                        Ok(id) => id,
                        Err(msg) => {
                            eprintln!("Error: {}", msg);
                            return 1;
                        }
                    };
                    match engine.get(&memory_id) {
                        Ok(memory) => {
                            println!("=== Memory Detail ===");
                            print_memory_detail(&memory);

                            // Show graph edges
                            println!("\n=== Graph Neighbors (depth 1) ===");
                            let mut arr = [0u8; 16];
                            arr.copy_from_slice(&memory_id);
                            match engine.outgoing_edges(&arr) {
                                Ok(edges) => {
                                    if edges.is_empty() {
                                        println!("No outgoing edges.");
                                    } else {
                                        for (edge_type, target, meta) in &edges {
                                            println!(
                                                "  -> {} ({:?}, conf={:.2})",
                                                format_memory_id(target),
                                                edge_type,
                                                meta.confidence
                                            );
                                        }
                                    }
                                }
                                Err(_) => println!("(graph query not available)"),
                            }
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

        Commands::Export {
            ref entity_id,
            limit,
        } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let capped_limit = limit.min(10000) as usize;

                    // Use temporal recall to get all memories
                    let input = RecallInput {
                        cue: String::new(),
                        strategies: vec![RecallStrategy::Temporal],
                        top_k: Some(capped_limit),
                        entity_id: entity_id.clone(),
                        time_range: None,
                        edge_types: None,
                        max_depth: None,
                        ef_search: None,
                        scoring_weights: None,
                        cue_context: None,
                        causal_direction: None,
                        analogy_a_id: None,
                        analogy_b_id: None,
                        seed_memory_id: None,
                        analogical_alpha: None,
                    };

                    match engine.recall(input) {
                        Ok(output) => {
                            let mut stdout = io::stdout();
                            for r in &output.results {
                                let json = memory_to_json(&r.memory);
                                let line = serde_json::to_string(&json).unwrap_or_default();
                                writeln!(stdout, "{}", line).ok();
                            }
                            eprintln!(
                                "Exported {} memories{}",
                                output.results.len(),
                                if output.results.len() >= capped_limit {
                                    " (limit reached)"
                                } else {
                                    ""
                                }
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

        Commands::Revise { ref id, .. } => {
            eprintln!("Revise is not yet supported in local mode. Use --endpoint for remote mode.");
            let _ = id;
            1
        }

        Commands::Reflect {
            ref entity_id,
            since_us,
        } => {
            let path = match require_vault_path(None, cli.vault.as_ref(), cli.global) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match setup_engine(&path).await {
                Ok((engine, _)) => {
                    let hebbs_dir = path.join(".hebbs");
                    let vault_config = match VaultConfig::load(&hebbs_dir) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("Error loading vault config: {}", e);
                            return 1;
                        }
                    };
                    if !vault_config.llm.is_configured() {
                        eprintln!("Error: LLM not configured. Set [llm] in .hebbs/config.toml or run `hebbs init` with --provider/--model.");
                        return 1;
                    }
                    let provider_config = vault_config.llm.to_provider_config();
                    let proposal_provider = match hebbs_reflect::create_provider(&provider_config) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("Error creating proposal LLM provider: {}", e);
                            return 1;
                        }
                    };
                    let validation_provider = match hebbs_reflect::create_provider(&provider_config)
                    {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("Error creating validation LLM provider: {}", e);
                            return 1;
                        }
                    };

                    let scope = build_reflect_scope(entity_id.clone(), since_us);
                    let config = ReflectConfig {
                        proposal_provider_config: provider_config.clone(),
                        validation_provider_config: provider_config,
                        ..ReflectConfig::default()
                    };

                    match engine.reflect(
                        scope,
                        &config,
                        proposal_provider.as_ref(),
                        validation_provider.as_ref(),
                    ) {
                        Ok(result) => {
                            if is_json_format(&cli.format) {
                                let json = serde_json::json!({
                                    "insights_created": result.insights_created,
                                    "clusters_found": result.clusters_found,
                                    "clusters_processed": result.clusters_processed,
                                    "memories_processed": result.memories_processed,
                                });
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&json).unwrap_or_default()
                                );
                            } else {
                                println!(
                                    "Reflect complete: {} insights created from {} clusters ({} processed), {} memories scanned.",
                                    result.insights_created,
                                    result.clusters_found,
                                    result.clusters_processed,
                                    result.memories_processed,
                                );
                            }
                            0
                        }
                        Err(e) => {
                            eprintln!("Reflect error: {}", e);
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

        Commands::Subscribe { .. } | Commands::Feed { .. } => {
            eprintln!(
                "Subscribe/Feed requires remote mode. Use --endpoint to connect to a server."
            );
            1
        }

        Commands::Metrics => {
            eprintln!("Metrics requires remote mode. Use --endpoint to connect to a server.");
            1
        }

        Commands::Panel { vault_path, port } => {
            // Resolve the vault path to open in the panel on startup.
            // This is passed to the daemon via --initial-vault so the panel
            // opens the correct vault from frame one (no racy POST switch).
            let initial_vault = vault_path.as_ref().or(cli.vault.as_ref()).and_then(|vp| {
                let abs_path = std::fs::canonicalize(vp).unwrap_or_else(|_| vp.clone());
                if abs_path.join(".hebbs").exists() {
                    Some(abs_path)
                } else {
                    eprintln!(
                        "Warning: vault not initialized at {}. Panel will use default vault.",
                        abs_path.display()
                    );
                    None
                }
            });

            let opts = client::DaemonStartOpts {
                panel_port: Some(port),
                initial_vault,
            };
            match client::ensure_daemon_full(opts).await {
                Ok(_daemon) => {
                    let url = format!("http://127.0.0.1:{}", port);
                    println!("Memory Palace running at {}", url);
                    open_browser(&url);
                    println!("Press Ctrl+C to stop.");
                    tokio::signal::ctrl_c().await.ok();
                    0
                }
                Err(e) => {
                    eprintln!("Error starting daemon for panel: {}", e);
                    1
                }
            }
        }

        Commands::Serve {
            foreground,
            idle_timeout,
            panel_port,
            ref initial_vault,
        } => {
            let daemon_config = match hebbs_vault::daemon::DaemonConfig::default_config() {
                Some(mut c) => {
                    c.foreground = foreground;
                    c.idle_timeout_secs = idle_timeout;
                    c.panel_port = panel_port;
                    c.initial_vault = initial_vault.clone();
                    // Try to pick embedding model from current vault's config
                    if let Ok(cwd) = std::env::current_dir() {
                        if let Ok(vc) = VaultConfig::load(&cwd.join(".hebbs")) {
                            c.embedding_model = vc.embedding.model.clone();
                        }
                    }
                    c
                }
                None => {
                    eprintln!("Error: cannot determine home directory");
                    return 1;
                }
            };
            match hebbs_vault::daemon::run_daemon(daemon_config).await {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("Daemon error: {}", e);
                    1
                }
            }
        }

        Commands::Queries { .. } => {
            eprintln!("Query log requires the daemon. Do not set HEBBS_NO_DAEMON=1.");
            1
        }

        Commands::Stop => {
            let socket_path = match client::default_socket_path() {
                Some(p) => p,
                None => {
                    eprintln!("Error: cannot determine home directory");
                    return 1;
                }
            };
            if !socket_path.exists() {
                eprintln!("Daemon is not running (no socket found).");
                return 1;
            }
            match client::DaemonClient::connect(&socket_path).await {
                Ok(mut daemon) => {
                    let request = DaemonRequest {
                        command: DaemonCommand::Shutdown,
                        vault_path: None,
                        vault_paths: None,
                        caller: "cli".to_string(),
                    };
                    match daemon.send(&request).await {
                        Ok(_) => {
                            println!("Daemon stopped.");
                            0
                        }
                        Err(e) => {
                            eprintln!("Error stopping daemon: {}", e);
                            1
                        }
                    }
                }
                Err(_) => {
                    eprintln!("Daemon is not running (cannot connect).");
                    1
                }
            }
        }

        Commands::Version => {
            println!(
                "hebbs {} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH
            );
            0
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Daemon Mode (CLI -> Unix socket -> daemon)
// ═══════════════════════════════════════════════════════════════════════

/// Route commands through the daemon for warm engine access.
/// Falls back to direct local mode if the daemon cannot be reached.
async fn run_daemon_mode(cli: Cli) -> i32 {
    let mut daemon = match client::ensure_daemon().await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("daemon unavailable ({}), falling back to direct mode", e);
            return run_local(cli).await;
        }
    };

    // Resolve vault path for the request
    let vault_path = match &cli.command {
        Commands::Index { vault_path }
        | Commands::Watch { vault_path }
        | Commands::Rebuild { vault_path }
        | Commands::List { vault_path, .. }
        | Commands::Status { vault_path } => {
            match require_vault_path(vault_path.as_ref(), cli.vault.as_ref(), cli.global) {
                Ok(p) => Some(p),
                Err(code) => return code,
            }
        }
        _ => match resolve_vault_path(None, cli.vault.as_ref(), cli.global) {
            Some(p) => Some(p),
            None => {
                eprintln!("No brain found. Run: hebbs init <path>");
                return 1;
            }
        },
    };

    // Resolve additional vault paths for --all flag (multi-vault recall/prime)
    let vault_paths = match &cli.command {
        Commands::Recall { all: true, .. } | Commands::Prime { all: true, .. } => {
            // For --all: query both the project vault AND the global vault.
            // vault_path is already the project vault (or global if --global).
            // Add the global vault as a second path if it's different.
            let mut extra = Vec::new();
            if let Some(home) = dirs::home_dir() {
                let global_root = home.clone();
                if global_root.join(".hebbs").exists() {
                    // Only add global if it's different from the primary vault
                    if vault_path.as_ref() != Some(&global_root) {
                        extra.push(global_root);
                    }
                }
            }
            if extra.is_empty() {
                None
            } else {
                Some(extra)
            }
        }
        _ => None,
    };

    let command = match build_daemon_command(&cli) {
        Some(cmd) => cmd,
        None => {
            // Command not supported via daemon, fall back to direct local mode
            return run_local(cli).await;
        }
    };

    let request = DaemonRequest {
        command,
        vault_path,
        vault_paths,
        caller: "cli".to_string(),
    };

    let response = match daemon.send(&request).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Daemon error: {}", e);
            return 1;
        }
    };

    handle_daemon_response(&cli, response)
}

/// Convert CLI command to daemon protocol command.
fn build_daemon_command(cli: &Cli) -> Option<DaemonCommand> {
    match &cli.command {
        Commands::Remember {
            content,
            importance,
            context,
            entity_id,
            edge,
        } => {
            let content_str = match read_content(content.clone()) {
                Ok(c) => c,
                Err(_) => return None,
            };
            let edges: Vec<hebbs_vault::daemon::protocol::EdgeSpec> = edge
                .iter()
                .filter_map(|spec| {
                    let parts: Vec<&str> = spec.split(':').collect();
                    if parts.len() < 2 {
                        return None;
                    }
                    Some(hebbs_vault::daemon::protocol::EdgeSpec {
                        target_id: parts[0].to_string(),
                        edge_type: parts[1].to_string(),
                        confidence: parts.get(2).and_then(|c| c.parse().ok()),
                    })
                })
                .collect();
            Some(DaemonCommand::Remember {
                content: content_str,
                importance: *importance,
                context: context.clone(),
                entity_id: entity_id.clone(),
                edges,
            })
        }
        Commands::Get { id } => Some(DaemonCommand::Get { id: id.clone() }),
        Commands::Recall {
            cue,
            strategy,
            top_k,
            entity_id,
            max_depth,
            seed,
            weights,
            ef_search,
            edge_types,
            time_range,
            analogical_alpha,
            context,
            all: _,
        } => Some(DaemonCommand::Recall {
            cue: cue.clone(),
            strategy: strategy.as_ref().map(|s| match s {
                StrategyArg::Similarity => "similarity".to_string(),
                StrategyArg::Temporal => "temporal".to_string(),
                StrategyArg::Causal => "causal".to_string(),
                StrategyArg::Analogical => "analogical".to_string(),
            }),
            top_k: *top_k,
            entity_id: entity_id.clone(),
            max_depth: *max_depth,
            seed: seed.clone(),
            weights: weights.clone(),
            ef_search: *ef_search,
            edge_types: edge_types.clone(),
            time_range: time_range.clone(),
            analogical_alpha: *analogical_alpha,
            context: context.clone(),
        }),
        Commands::Forget {
            ids,
            entity_id,
            staleness_us,
            access_floor,
            kind,
            decay_floor,
        } => Some(DaemonCommand::Forget {
            ids: ids.clone(),
            entity_id: entity_id.clone(),
            staleness_us: *staleness_us,
            access_floor: *access_floor,
            kind: kind.as_ref().map(|k| match k {
                KindArg::Episode => "episode".to_string(),
                KindArg::Insight => "insight".to_string(),
                KindArg::Revision => "revision".to_string(),
            }),
            decay_floor: *decay_floor,
        }),
        Commands::Prime {
            entity_id,
            context,
            max_memories,
            recency_us,
            similarity_cue,
            all: _,
        } => Some(DaemonCommand::Prime {
            entity_id: entity_id.clone(),
            context: context.clone(),
            max_memories: *max_memories,
            recency_us: *recency_us,
            similarity_cue: similarity_cue.clone(),
        }),
        Commands::Inspect { id } => Some(DaemonCommand::Inspect { id: id.clone() }),
        Commands::Export { entity_id, limit } => Some(DaemonCommand::Export {
            entity_id: entity_id.clone(),
            limit: *limit,
        }),
        Commands::Status { .. } => Some(DaemonCommand::Status),
        Commands::Index { .. } => Some(DaemonCommand::Index),
        Commands::List { sections, .. } => Some(DaemonCommand::List {
            sections: *sections,
        }),
        Commands::Watch { .. } => {
            // Watch is merged into serve (Milestone 3). The daemon watches
            // all open vaults automatically. Just ping to confirm it's alive.
            Some(DaemonCommand::Ping)
        }
        Commands::Rebuild { .. } => {
            // Rebuild requires re-init; fall back to direct mode
            None
        }
        Commands::ReflectPrepare {
            entity_id,
            since_us,
        } => Some(DaemonCommand::ReflectPrepare {
            entity_id: entity_id.clone(),
            since_us: *since_us,
        }),
        Commands::ReflectCommit {
            session_id,
            insights,
        } => Some(DaemonCommand::ReflectCommit {
            session_id: session_id.clone(),
            insights: insights.clone(),
        }),
        Commands::ContradictionPrepare {} => Some(DaemonCommand::ContradictionPrepare {}),
        Commands::ContradictionCommit { results } => Some(DaemonCommand::ContradictionCommit {
            results: results.clone(),
        }),
        Commands::Insights {
            entity_id,
            min_confidence,
            max_results,
        } => Some(DaemonCommand::Insights {
            entity_id: entity_id.clone(),
            min_confidence: *min_confidence,
            max_results: *max_results,
        }),
        Commands::Queries {
            limit,
            offset,
            caller,
            operation,
        } => Some(DaemonCommand::Queries {
            limit: Some(*limit),
            offset: Some(*offset),
            caller_filter: caller.clone(),
            operation_filter: operation.clone(),
        }),
        // These are handled before reaching daemon mode or not supported via daemon
        Commands::Init { .. }
        | Commands::Config { .. }
        | Commands::Version
        | Commands::Serve { .. }
        | Commands::Panel { .. }
        | Commands::Stop
        | Commands::Reflect { .. }
        | Commands::Revise { .. }
        | Commands::Subscribe { .. }
        | Commands::Feed { .. }
        | Commands::Metrics => None,
    }
}

/// Convert Rust error strings into user-friendly sentences.
fn humanize_error_with_code(err: &str, code: &str) -> String {
    hebbs_vault::error::humanize_error_with_code(err, code)
}

/// Render a daemon response for CLI output.
fn handle_daemon_response(cli: &Cli, response: DaemonResponse) -> i32 {
    if response.status == ResponseStatus::Error {
        let raw = response
            .error
            .unwrap_or_else(|| "unknown error".to_string());
        let code = response.status_code.as_deref().unwrap_or("ERR_UNKNOWN");
        let human = humanize_error_with_code(&raw, code);
        eprintln!("{}", human);
        return 1;
    }

    let data = response.data.unwrap_or(serde_json::json!(null));

    // JSON format: normalize daemon envelope to match local-mode output shape
    if is_json_format(&cli.format) {
        let normalized = match &cli.command {
            // Local mode outputs a plain array for recall and prime
            Commands::Recall { .. } | Commands::Prime { .. } => {
                if let Some(results) = data.get("results") {
                    results.clone()
                } else {
                    data.clone()
                }
            }
            _ => data.clone(),
        };
        println!("{}", serde_json::to_string(&normalized).unwrap_or_default());
        return 0;
    }

    // Human format: render based on command type
    match &cli.command {
        Commands::Remember { .. } => {
            let content = data.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let preview = truncate(content, 80);
            println!("Remembered: \"{}\"", preview);
        }
        Commands::Get { .. } | Commands::Inspect { .. } => {
            print_json_memory(&data);
        }
        Commands::Recall { .. } => {
            if let Some(results) = data.get("results").and_then(|v| v.as_array()) {
                if results.is_empty() {
                    println!("No results found.");
                } else {
                    println!();
                    for (i, r) in results.iter().enumerate() {
                        let content = r.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        // Extract source from context
                        let source = r.get("context").and_then(|ctx| {
                            let file = ctx.get("file_path").and_then(|v| v.as_str());
                            let heading =
                                ctx.get("heading_path")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str())
                                            .collect::<Vec<_>>()
                                            .join(" > ")
                                    });
                            match (file, heading) {
                                (Some(f), Some(h)) if !h.is_empty() => {
                                    Some(format!("{} > {}", f, h))
                                }
                                (Some(f), _) => Some(f.to_string()),
                                _ => None,
                            }
                        });
                        println!("  {}. {}", i + 1, truncate(content, 200));
                        if let Some(src) = source {
                            println!("     Source: {}", src);
                        }
                        println!();
                    }
                }
                // Show contradiction warnings when conflicting memories appear together
                if let Some(conflicts) = data.get("contradictions").and_then(|v| v.as_array()) {
                    if !conflicts.is_empty() {
                        println!(
                            "  Conflicting information found ({} conflict{}):",
                            conflicts.len(),
                            if conflicts.len() == 1 { "" } else { "s" }
                        );
                        println!("  Run `hebbs contradictions` to review.");
                        println!();
                    }
                }
                // Show indexing completeness hint when vault is still being indexed.
                if let Some(pct) = data.get("indexing_pct").and_then(|v| v.as_u64()) {
                    println!("  {}% of your vault is indexed. More results may appear as indexing completes.", pct);
                    println!();
                }
            }
        }
        Commands::Forget { .. } => {
            let count = data
                .get("forgotten_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cascade = data
                .get("cascade_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            println!("Forgotten: {} memories ({} cascade)", count, cascade);
            if data
                .get("truncated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                println!("(truncated; more candidates remain)");
            }
        }
        Commands::Prime { .. } => {
            if let Some(results) = data.get("results").and_then(|v| v.as_array()) {
                let temporal = data
                    .get("temporal_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let similarity = data
                    .get("similarity_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                println!(
                    "Primed {} memories ({} temporal, {} similarity)",
                    results.len(),
                    temporal,
                    similarity
                );
                for (i, r) in results.iter().enumerate() {
                    let score = r.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let content = r.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    println!(
                        "  {}. [score {:.3}] {}",
                        i + 1,
                        score,
                        truncate(content, 100)
                    );
                }
            }
        }
        Commands::Status { .. } => {
            if let Some(root) = data.get("vault_root").and_then(|v| v.as_str()) {
                println!("Vault: {}", root);
            }
            let total_files = data
                .get("total_files")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let _total_sections = data
                .get("total_sections")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let synced = data.get("synced").and_then(|v| v.as_u64()).unwrap_or(0);
            let content_stale = data
                .get("content_stale")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let orphaned = data.get("orphaned").and_then(|v| v.as_u64()).unwrap_or(0);
            let memories = data
                .get("total_memories")
                .and_then(|v| v.as_u64())
                .unwrap_or(synced);

            // Check for live indexing progress from the daemon
            let live_indexing = data.get("indexing");

            if live_indexing.is_some() || content_stale > 0 {
                let total_sections = synced + content_stale + orphaned;
                let pct = if total_sections > 0 {
                    synced * 100 / total_sections
                } else {
                    100
                };
                println!("Files:    {} ({}% indexed)", total_files, pct);
            } else {
                println!("Files:    {} (all indexed)", total_files);
            }
            println!("Memories: {}", memories);

            // Show live indexing progress (phase, file count)
            if let Some(idx) = live_indexing {
                let phase = idx.get("phase").and_then(|v| v.as_u64()).unwrap_or(0);
                let idx_total = idx.get("total_files").and_then(|v| v.as_u64()).unwrap_or(0);
                let files_done = idx.get("files_done").and_then(|v| v.as_u64()).unwrap_or(0);
                let current = idx
                    .get("current_file")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                print!("Indexing:  phase {}/2", phase);
                if idx_total > 0 {
                    print!(", {}/{} files", files_done, idx_total);
                }
                if !current.is_empty() {
                    print!(" ({})", current);
                }
                println!();
            } else if content_stale > 0 || orphaned > 0 {
                if content_stale > 0 {
                    let daemon_watching = data
                        .get("daemon_watching")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if daemon_watching {
                        println!("Indexing: waiting for next change to re-index");
                    } else {
                        println!("Indexing: incomplete. Run `hebbs index` to finish.");
                    }
                }
                if orphaned > 0 {
                    println!("Removed:  {} source file(s) deleted", orphaned);
                }
            }

            let llm_provider = data.get("llm_provider").and_then(|v| v.as_str());
            let llm_model = data.get("llm_model").and_then(|v| v.as_str());
            if let (Some(p), Some(m)) = (llm_provider, llm_model) {
                println!("LLM:      {}/{}", p, m);
            }

            println!();
            if live_indexing.is_none() && content_stale == 0 && orphaned == 0 {
                println!("Everything is up to date.");
            }
        }
        Commands::Watch { .. } => {
            // Watch is merged into serve (Milestone 3). The daemon auto-starts
            // and watches all open vaults. Just confirm it's running.
            println!("Daemon is running and watching all open vaults for file changes.");
            println!("File watching is built into `hebbs serve`. No separate watcher needed.");
        }
        Commands::Index { .. } => {
            let total = data
                .get("total_files")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let memories = data
                .get("sections_remembered")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let revised = data
                .get("sections_revised")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let forgotten = data
                .get("sections_forgotten")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            print!("Indexed {} file(s). {} memories created", total, memories);
            if revised > 0 {
                print!(", {} revised", revised);
            }
            if forgotten > 0 {
                print!(", {} removed", forgotten);
            }
            println!(".");
        }
        Commands::List { .. } => {
            if let Some(files) = data.get("files").and_then(|v| v.as_array()) {
                for f in files {
                    let path = f.get("path").and_then(|v| v.as_str()).unwrap_or("?");
                    let sections = f.get("section_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    let synced = f.get("synced").and_then(|v| v.as_u64()).unwrap_or(0);
                    println!("  {} ({} sections, {} synced)", path, sections, synced);
                }
                println!(
                    "\nTotal: {} files",
                    data.get("total_files")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                );
            }
        }
        Commands::Export { .. } => {
            if let Some(memories) = data.get("memories").and_then(|v| v.as_array()) {
                let mut stdout = io::stdout();
                for m in memories {
                    writeln!(stdout, "{}", serde_json::to_string(m).unwrap_or_default()).ok();
                }
                eprintln!("Exported {} memories", memories.len());
            }
        }
        Commands::ReflectPrepare { .. } => {
            if let Some(session_id) = data.get("session_id").and_then(|v| v.as_str()) {
                println!("Session: {}", session_id);
            }
            if let Some(processed) = data.get("memories_processed").and_then(|v| v.as_u64()) {
                if let Some(clusters) = data.get("clusters").and_then(|v| v.as_array()) {
                    println!(
                        "Processed: {} memories, {} clusters",
                        processed,
                        clusters.len()
                    );
                    for c in clusters {
                        let cid = c.get("cluster_id").and_then(|v| v.as_u64()).unwrap_or(0);
                        let members = c.get("member_count").and_then(|v| v.as_u64()).unwrap_or(0);
                        println!("  Cluster {}: {} members", cid, members);
                    }
                }
            }
        }
        Commands::ReflectCommit { .. } => {
            let created = data
                .get("insights_created")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            println!("Committed: {} insights created", created);
        }
        Commands::ContradictionPrepare { .. } => {
            if let Some(candidates) = data.get("candidates").and_then(|v| v.as_array()) {
                let count = candidates.len();
                println!("Found {} pending contradiction candidate(s)", count);
                for (i, c) in candidates.iter().enumerate() {
                    let id_a = c.get("memory_id_a").and_then(|v| v.as_str()).unwrap_or("?");
                    let id_b = c.get("memory_id_b").and_then(|v| v.as_str()).unwrap_or("?");
                    let snippet_a = c
                        .get("content_a_snippet")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let snippet_b = c
                        .get("content_b_snippet")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let score = c
                        .get("classifier_score")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    println!("\n--- Candidate {} (score: {:.3}) ---", i + 1, score);
                    println!("  Memory A: {}", id_a);
                    println!("  Memory B: {}", id_b);
                    println!("  Snippet A: {}", truncate(snippet_a, 120));
                    println!("  Snippet B: {}", truncate(snippet_b, 120));
                }
            }
        }
        Commands::ContradictionCommit { .. } => {
            let confirmed = data
                .get("contradictions_confirmed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let revised = data
                .get("revisions_created")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let dismissed = data.get("dismissed").and_then(|v| v.as_u64()).unwrap_or(0);
            println!(
                "Committed: {} confirmed, {} revised, {} dismissed",
                confirmed, revised, dismissed
            );
        }
        Commands::Queries { .. } => {
            if let Some(entries) = data.get("entries").and_then(|v| v.as_array()) {
                if entries.is_empty() {
                    println!("No query log entries.");
                } else {
                    println!("Query log ({} entries):\n", entries.len());
                    for e in entries {
                        let ts = e.get("timestamp_us").and_then(|v| v.as_u64()).unwrap_or(0);
                        let caller = e.get("caller").and_then(|v| v.as_str()).unwrap_or("?");
                        let op = e.get("operation").and_then(|v| v.as_str()).unwrap_or("?");
                        let query = e.get("query").and_then(|v| v.as_str()).unwrap_or("");
                        let results = e.get("result_count").and_then(|v| v.as_u64()).unwrap_or(0);
                        let latency = e.get("latency_us").and_then(|v| v.as_u64()).unwrap_or(0);
                        let latency_ms = latency as f64 / 1000.0;
                        // Convert microsecond timestamp to HH:MM:SS
                        let secs = ts / 1_000_000;
                        let h = (secs / 3600) % 24;
                        let m = (secs / 60) % 60;
                        let s = secs % 60;
                        let time_str = format!("{:02}:{:02}:{:02}", h, m, s);
                        println!(
                            "  {} {:12} {:6} {:50} {} results  {:.1}ms",
                            time_str,
                            caller,
                            op,
                            truncate(query, 50),
                            results,
                            latency_ms
                        );
                    }
                }
            }
        }
        Commands::Insights { .. } => {
            if let Some(insights) = data.get("insights").and_then(|v| v.as_array()) {
                if insights.is_empty() {
                    println!("No insights found.");
                } else {
                    println!("Found {} insight(s):\n", insights.len());
                    for (i, m) in insights.iter().enumerate() {
                        let imp = m.get("importance").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        println!("  {}. [imp {:.2}] {}", i + 1, imp, truncate(content, 150));
                    }
                }
            }
        }
        _ => {
            // Fallback: print raw JSON
            println!(
                "{}",
                serde_json::to_string_pretty(&data).unwrap_or_default()
            );
        }
    }

    0
}

fn print_json_memory(data: &serde_json::Value) {
    if let Some(id) = data.get("memory_id").and_then(|v| v.as_str()) {
        println!("ID:           {}", id);
    }
    if let Some(content) = data.get("content").and_then(|v| v.as_str()) {
        println!("Content:      {}", content);
    }
    if let Some(imp) = data.get("importance").and_then(|v| v.as_f64()) {
        println!("Importance:   {:.2}", imp);
    }
    if let Some(eid) = data.get("entity_id").and_then(|v| v.as_str()) {
        println!("Entity:       {}", eid);
    }
    if let Some(ac) = data.get("access_count").and_then(|v| v.as_u64()) {
        println!("Access count: {}", ac);
    }
    if let Some(ca) = data.get("created_at_us").and_then(|v| v.as_u64()) {
        println!("Created:      {}", ca);
    }
    if let Some(la) = data.get("last_accessed_at_us").and_then(|v| v.as_u64()) {
        println!("Last access:  {}", la);
    }
    if let Some(ctx) = data.get("context") {
        if !ctx.is_null() {
            println!("Context:      {}", ctx);
        }
    }
    if let Some(edges) = data.get("edges").and_then(|v| v.as_array()) {
        if !edges.is_empty() {
            println!("\n=== Graph Neighbors (depth 1) ===");
            for e in edges {
                let target = e.get("target_id").and_then(|v| v.as_str()).unwrap_or("?");
                let etype = e.get("edge_type").and_then(|v| v.as_str()).unwrap_or("?");
                let conf = e.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
                println!("  -> {} ({}, conf={:.2})", target, etype, conf);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Remote Mode
// ═══════════════════════════════════════════════════════════════════════

async fn run_remote(cli: Cli) -> i32 {
    let endpoint = cli
        .endpoint
        .unwrap_or_else(|| "http://localhost:6380".to_string());
    let endpoint = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint
    } else {
        format!("http://{}", endpoint)
    };

    let mut config = hebbs_cli::config::CliConfig::load();
    config.endpoint = endpoint;

    if let Some(hp) = cli.http_port {
        config.http_port = hp;
    }
    if let Some(tm) = cli.timeout {
        config.timeout_ms = tm;
    }
    if let Some(ref fmt) = cli.format {
        config.output_format = match fmt {
            FormatArg::Human => hebbs_cli::config::OutputFormat::Human,
            FormatArg::Json => hebbs_cli::config::OutputFormat::Json,
            FormatArg::Raw => hebbs_cli::config::OutputFormat::Raw,
        };
    }
    if let Some(ref c) = cli.color {
        config.color = match c {
            ColorArg::Always => hebbs_cli::config::ColorMode::Always,
            ColorArg::Never => hebbs_cli::config::ColorMode::Never,
            ColorArg::Auto => hebbs_cli::config::ColorMode::Auto,
        };
    }
    if cli.tenant.is_some() {
        config.tenant = cli.tenant.clone();
    }

    let is_tty = io::stdout().is_terminal();
    let use_color = config.should_color(is_tty);
    let renderer = hebbs_cli::format::Renderer::new(config.output_format, use_color);
    let mut conn =
        hebbs_cli::connection::ConnectionManager::new(config.endpoint.clone(), config.timeout_ms)
            .with_api_key(cli.api_key.clone());

    let tenant_id = config.tenant.as_deref();

    // Map unified Commands to hebbs_cli::cli::Commands
    let cli_cmd = match map_to_cli_command(cli.command) {
        Some(cmd) => cmd,
        None => {
            eprintln!("This command is not available in remote mode.");
            return 1;
        }
    };

    hebbs_cli::commands::execute(cli_cmd, &mut conn, &renderer, config.http_port, tenant_id).await
}

/// Map unified Commands to hebbs_cli::cli::Commands for remote mode dispatch.
fn map_to_cli_command(cmd: Commands) -> Option<hebbs_cli::cli::Commands> {
    match cmd {
        Commands::Remember {
            content,
            importance,
            context,
            entity_id,
            edge,
        } => Some(hebbs_cli::cli::Commands::Remember {
            content,
            importance,
            context,
            entity_id,
            edge,
        }),
        Commands::Get { id } => Some(hebbs_cli::cli::Commands::Get { id }),
        Commands::Recall {
            cue,
            strategy,
            top_k,
            entity_id,
            max_depth,
            seed,
            weights,
            ef_search,
            edge_types,
            time_range,
            analogical_alpha,
            context,
            all: _,
        } => Some(hebbs_cli::cli::Commands::Recall {
            cue,
            strategy: strategy.map(|s| match s {
                StrategyArg::Similarity => hebbs_cli::cli::StrategyArg::Similarity,
                StrategyArg::Temporal => hebbs_cli::cli::StrategyArg::Temporal,
                StrategyArg::Causal => hebbs_cli::cli::StrategyArg::Causal,
                StrategyArg::Analogical => hebbs_cli::cli::StrategyArg::Analogical,
            }),
            top_k,
            entity_id,
            max_depth,
            seed,
            weights,
            ef_search,
            edge_types,
            time_range,
            analogical_alpha,
            context,
        }),
        Commands::Revise {
            id,
            content,
            importance,
            context,
            context_mode,
            entity_id,
            edge,
        } => Some(hebbs_cli::cli::Commands::Revise {
            id,
            content,
            importance,
            context,
            context_mode: match context_mode {
                ContextModeArg::Merge => hebbs_cli::cli::ContextModeArg::Merge,
                ContextModeArg::Replace => hebbs_cli::cli::ContextModeArg::Replace,
            },
            entity_id,
            edge,
        }),
        Commands::Forget {
            ids,
            entity_id,
            staleness_us,
            access_floor,
            kind,
            decay_floor,
        } => Some(hebbs_cli::cli::Commands::Forget {
            ids,
            entity_id,
            staleness_us,
            access_floor,
            kind: kind.map(|k| match k {
                KindArg::Episode => hebbs_cli::cli::KindArg::Episode,
                KindArg::Insight => hebbs_cli::cli::KindArg::Insight,
                KindArg::Revision => hebbs_cli::cli::KindArg::Revision,
            }),
            decay_floor,
        }),
        Commands::Prime {
            entity_id,
            context,
            max_memories,
            recency_us,
            similarity_cue,
            all: _,
        } => Some(hebbs_cli::cli::Commands::Prime {
            entity_id,
            context,
            max_memories,
            recency_us,
            similarity_cue,
        }),
        Commands::Subscribe {
            entity_id,
            confidence,
        } => Some(hebbs_cli::cli::Commands::Subscribe {
            entity_id,
            confidence,
        }),
        Commands::Feed {
            subscription_id,
            text,
        } => Some(hebbs_cli::cli::Commands::Feed {
            subscription_id,
            text,
        }),
        Commands::Reflect {
            entity_id,
            since_us,
        } => Some(hebbs_cli::cli::Commands::Reflect {
            entity_id,
            since_us,
        }),
        Commands::ReflectPrepare {
            entity_id,
            since_us,
        } => Some(hebbs_cli::cli::Commands::ReflectPrepare {
            entity_id,
            since_us,
        }),
        Commands::ReflectCommit {
            session_id,
            insights,
        } => Some(hebbs_cli::cli::Commands::ReflectCommit {
            session_id,
            insights,
        }),
        Commands::Insights {
            entity_id,
            min_confidence,
            max_results,
        } => Some(hebbs_cli::cli::Commands::Insights {
            entity_id,
            min_confidence,
            max_results,
        }),
        Commands::Status { .. } => Some(hebbs_cli::cli::Commands::Status),
        Commands::Inspect { id } => Some(hebbs_cli::cli::Commands::Inspect { id }),
        Commands::Export { entity_id, limit } => {
            Some(hebbs_cli::cli::Commands::Export { entity_id, limit })
        }
        Commands::Metrics => Some(hebbs_cli::cli::Commands::Metrics),
        Commands::Version => Some(hebbs_cli::cli::Commands::Version),
        // Vault-only commands: not available in remote mode
        Commands::Init { .. }
        | Commands::Config { .. }
        | Commands::Index { .. }
        | Commands::Watch { .. }
        | Commands::Rebuild { .. }
        | Commands::List { .. }
        | Commands::Serve { .. }
        | Commands::Panel { .. }
        | Commands::Stop
        | Commands::Queries { .. }
        | Commands::ContradictionPrepare { .. }
        | Commands::ContradictionCommit { .. } => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════

/// Interactive LLM provider setup for `hebbs init` when no flags are given.
/// LLM is required; returns `None` only if stdin read fails.
fn interactive_llm_setup() -> Option<hebbs_vault::config::LlmConfig> {
    println!();
    println!("  HEBBS requires an LLM for extracting knowledge from your notes.");
    println!("  You can use a cloud provider or a local model via Ollama.");
    println!();
    println!("  [1] Ollama (local, free, private)");
    println!("  [2] Gemini (fast, cheap)");
    println!("  [3] Anthropic");
    println!("  [4] OpenAI");
    println!();
    print!("  Choice [1]: ");
    io::stdout().flush().ok();

    let mut choice = String::new();
    if io::stdin().read_line(&mut choice).is_err() {
        return None;
    }
    let choice = choice.trim();

    let (provider, default_model) = match choice {
        "" | "1" => ("ollama", "gemma3:1b"),
        "2" => ("gemini", "gemini-2.0-flash"),
        "3" => ("anthropic", "claude-haiku-4-5-20251001"),
        "4" => ("openai", "gpt-4o-mini"),
        _ => {
            eprintln!("  Unknown choice '{}'. LLM is required for HEBBS.", choice);
            return None;
        }
    };

    // Ask for model (with default)
    print!("  Model [{}]: ", default_model);
    io::stdout().flush().ok();
    let mut model_input = String::new();
    if io::stdin().read_line(&mut model_input).is_err() {
        return None;
    }
    let model = model_input.trim();
    let model = if model.is_empty() {
        default_model
    } else {
        model
    };

    // Ask for API key env var for cloud providers
    let api_key_env = if provider != "ollama" {
        let default_env = match provider {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "gemini" => "GEMINI_API_KEY",
            _ => "",
        };
        print!("  API key env var [{}]: ", default_env);
        io::stdout().flush().ok();
        let mut env_input = String::new();
        if io::stdin().read_line(&mut env_input).is_err() {
            return None;
        }
        let env_val = env_input.trim();
        Some(if env_val.is_empty() {
            default_env.to_string()
        } else {
            env_val.to_string()
        })
    } else {
        None
    };

    println!();
    println!("  Using {}/{}", provider, model);

    // Resolve env var to actual key so the daemon can use it
    let resolved_key = api_key_env
        .as_ref()
        .and_then(|env_name| std::env::var(env_name).ok());

    Some(hebbs_vault::config::LlmConfig {
        provider: provider.to_string(),
        model: model.to_string(),
        api_key: resolved_key,
        api_key_env,
        base_url: None,
    })
}

/// Recursively count .md files under `dir`, skipping internal/generated directories.
fn count_md_files(dir: &std::path::Path) -> usize {
    const SKIP_DIRS: &[&str] = &[
        ".hebbs",
        ".git",
        ".obsidian",
        "node_modules",
        "contradictions",
        "insights",
    ];
    fn walk(dir: &std::path::Path, count: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !SKIP_DIRS.contains(&name) {
                    walk(&path, count);
                }
            } else if path.extension().and_then(|x| x.to_str()) == Some("md") {
                *count += 1;
            }
        }
    }
    let mut n = 0;
    walk(dir, &mut n);
    n
}

fn config_get(config: &VaultConfig, key: &str) -> Option<String> {
    match key {
        "llm.provider" => Some(config.llm.provider.clone()),
        "llm.model" => Some(config.llm.model.clone()),
        "llm.api_key_env" => Some(config.llm.api_key_env.clone().unwrap_or_default()),
        "llm.base_url" => Some(config.llm.base_url.clone().unwrap_or_default()),
        "chunking.split_on" => Some(config.chunking.split_on.clone()),
        "chunking.min_section_length" => Some(config.chunking.min_section_length.to_string()),
        "embedding.model" => Some(config.embedding.model.clone()),
        "embedding.dimensions" => Some(config.embedding.dimensions.to_string()),
        "embedding.batch_size" => Some(config.embedding.batch_size.to_string()),
        "contradiction.enabled" => Some(config.contradiction.enabled.to_string()),
        "contradiction.candidates_k" => Some(config.contradiction.candidates_k.to_string()),
        "contradiction.min_similarity" => Some(config.contradiction.min_similarity.to_string()),
        "contradiction.min_confidence" => Some(config.contradiction.min_confidence.to_string()),
        "extraction.large_file_threshold" => {
            Some(config.extraction.large_file_threshold.to_string())
        }
        "extraction.max_propositions_per_file" => {
            Some(config.extraction.max_propositions_per_file.to_string())
        }
        _ => None,
    }
}

/// Set a config value by dot-notation key.
fn config_set(config: &mut VaultConfig, key: &str, value: &str) -> std::result::Result<(), String> {
    match key {
        "llm.provider" => config.llm.provider = value.to_string(),
        "llm.model" => config.llm.model = value.to_string(),
        "llm.api_key_env" => config.llm.api_key_env = Some(value.to_string()),
        "llm.base_url" => config.llm.base_url = Some(value.to_string()),
        "chunking.split_on" => config.chunking.split_on = value.to_string(),
        "chunking.min_section_length" => {
            config.chunking.min_section_length = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize: {}", value))?;
        }
        "embedding.model" => config.embedding.model = value.to_string(),
        "embedding.dimensions" => {
            config.embedding.dimensions = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize: {}", value))?;
        }
        "embedding.batch_size" => {
            config.embedding.batch_size = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize: {}", value))?;
        }
        "contradiction.enabled" => {
            config.contradiction.enabled = value
                .parse::<bool>()
                .map_err(|_| format!("invalid bool: {}", value))?;
        }
        "contradiction.candidates_k" => {
            config.contradiction.candidates_k = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize: {}", value))?;
        }
        "contradiction.min_similarity" => {
            config.contradiction.min_similarity = value
                .parse::<f32>()
                .map_err(|_| format!("invalid f32: {}", value))?;
        }
        "contradiction.min_confidence" => {
            config.contradiction.min_confidence = value
                .parse::<f32>()
                .map_err(|_| format!("invalid f32: {}", value))?;
        }
        "extraction.large_file_threshold" => {
            config.extraction.large_file_threshold = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize: {}", value))?;
        }
        "extraction.max_propositions_per_file" => {
            config.extraction.max_propositions_per_file = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize: {}", value))?;
        }
        _ => return Err(format!("unknown config key: {}", key)),
    }
    Ok(())
}

/// Show effective config with provenance annotations: (global) or (local).
fn config_show_with_provenance(
    effective: &VaultConfig,
    _global: Option<&VaultConfig>,
    local: Option<&VaultConfig>,
) {
    // Serialize the effective config as TOML
    let toml_str = toml::to_string_pretty(effective).unwrap_or_default();
    println!("{}", toml_str);

    // Show LLM provenance summary
    if effective.llm.is_configured() {
        let local_llm = local.map(|l| &l.llm);
        let fields: &[(&str, &str, &str)] = &[
            (
                "llm.provider",
                &effective.llm.provider,
                local_llm.map(|l| l.provider.as_str()).unwrap_or(""),
            ),
            (
                "llm.model",
                &effective.llm.model,
                local_llm.map(|l| l.model.as_str()).unwrap_or(""),
            ),
            (
                "llm.api_key_env",
                effective.llm.api_key_env.as_deref().unwrap_or(""),
                local_llm
                    .and_then(|l| l.api_key_env.as_deref())
                    .unwrap_or(""),
            ),
            (
                "llm.base_url",
                effective.llm.base_url.as_deref().unwrap_or(""),
                local_llm.and_then(|l| l.base_url.as_deref()).unwrap_or(""),
            ),
        ];
        println!("# LLM provenance:");
        for (key, eff_val, local_val) in fields {
            if eff_val.is_empty() {
                continue;
            }
            let source = if local_val.is_empty() {
                "global"
            } else {
                "local"
            };
            println!("#   {} = \"{}\" ({})", key, eff_val, source);
        }
    }
}

fn is_json_format(fmt: &Option<FormatArg>) -> bool {
    matches!(fmt, Some(FormatArg::Json))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn();
    }
}

fn format_memory_id(bytes: &[u8]) -> String {
    if bytes.len() == 16 {
        let mut arr = [0u8; 16];
        arr.copy_from_slice(bytes);
        ulid::Ulid::from_bytes(arr).to_string()
    } else {
        hex::encode(bytes)
    }
}

fn parse_memory_id(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    if trimmed.len() == 26 {
        if let Ok(ulid) = ulid::Ulid::from_string(trimmed) {
            return Ok(ulid.0.to_be_bytes().to_vec());
        }
    }
    if trimmed.len() == 32 {
        if let Ok(bytes) = hex::decode(trimmed) {
            if bytes.len() == 16 {
                return Ok(bytes);
            }
        }
    }
    Err(format!(
        "Invalid memory ID '{}'. Expected 26-char ULID or 32-char hex string.",
        trimmed
    ))
}

fn read_content(content: Option<String>) -> Result<String, String> {
    if let Some(c) = content {
        if c.is_empty() {
            return Err("Content cannot be empty".to_string());
        }
        return Ok(c);
    }
    if !io::stdin().is_terminal() {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("Failed to read stdin: {}", e))?;
        let trimmed = buf.trim_end().to_string();
        if trimmed.is_empty() {
            return Err("Content cannot be empty (stdin was empty)".to_string());
        }
        return Ok(trimmed);
    }
    Err("Content is required. Provide it as an argument or pipe via stdin.".to_string())
}

use std::io::Read as StdRead;

fn parse_context(
    json_str: Option<&str>,
) -> Result<Option<HashMap<String, serde_json::Value>>, String> {
    match json_str {
        Some(s) => {
            let map: HashMap<String, serde_json::Value> =
                serde_json::from_str(s).map_err(|e| format!("Invalid context JSON: {}", e))?;
            Ok(Some(map))
        }
        None => Ok(None),
    }
}

fn parse_edges(edge_specs: &[String]) -> Result<Vec<RememberEdge>, String> {
    edge_specs
        .iter()
        .map(|spec| {
            let parts: Vec<&str> = spec.split(':').collect();
            if parts.len() < 2 {
                return Err(format!(
                    "Edge spec '{}' must be TARGET_ID:EDGE_TYPE[:CONFIDENCE]",
                    spec
                ));
            }
            let target_bytes =
                parse_memory_id(parts[0]).map_err(|e| format!("Edge target: {}", e))?;
            let mut target = [0u8; 16];
            if target_bytes.len() != 16 {
                return Err(format!("Edge target must be 16 bytes, got {}", target_bytes.len()));
            }
            target.copy_from_slice(&target_bytes);

            let edge_type = match parts[1] {
                "caused_by" => EdgeType::CausedBy,
                "related_to" => EdgeType::RelatedTo,
                "followed_by" => EdgeType::FollowedBy,
                "revised_from" => EdgeType::RevisedFrom,
                "insight_from" => EdgeType::InsightFrom,
                "contradicts" => EdgeType::Contradicts,
                other => {
                    return Err(format!(
                        "Unknown edge type '{}'. Valid: caused_by, related_to, followed_by, revised_from, insight_from, contradicts",
                        other
                    ))
                }
            };

            let confidence = if parts.len() > 2 {
                Some(
                    parts[2]
                        .parse::<f32>()
                        .map_err(|_| format!("Invalid confidence '{}' in edge spec", parts[2]))?,
                )
            } else {
                None
            };

            Ok(RememberEdge {
                target_id: target,
                edge_type,
                confidence,
            })
        })
        .collect()
}

fn parse_scoring_weights(s: &str) -> Result<ScoringWeights, String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 4 {
        return Err(format!(
            "Weights must be 4 colon-separated floats (R:T:I:F), got {} parts",
            parts.len()
        ));
    }
    let parse = |part: &str, name: &str| -> Result<f32, String> {
        part.parse::<f32>()
            .map_err(|_| format!("Invalid {} weight '{}': must be a number", name, part))
    };
    Ok(ScoringWeights {
        w_relevance: parse(parts[0], "relevance")?,
        w_recency: parse(parts[1], "recency")?,
        w_importance: parse(parts[2], "importance")?,
        w_reinforcement: parse(parts[3], "reinforcement")?,
        ..ScoringWeights::default()
    })
}

fn build_reflect_scope(entity_id: Option<String>, since_us: Option<u64>) -> ReflectScope {
    match entity_id {
        Some(eid) => ReflectScope::Entity {
            entity_id: eid,
            since_us,
        },
        None => ReflectScope::Global { since_us },
    }
}

fn parse_produced_insights(json_str: &str) -> Result<Vec<hebbs_reflect::ProducedInsight>, String> {
    let parsed: Vec<serde_json::Value> = serde_json::from_str(json_str)
        .map_err(|e| format!("Invalid JSON for --insights: {}", e))?;

    parsed
        .iter()
        .map(|v| {
            let content = v["content"].as_str().unwrap_or_default().to_string();
            let confidence = v["confidence"].as_f64().unwrap_or(0.8) as f32;

            let source_memory_ids: Vec<[u8; 16]> = v["source_memory_ids"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| {
                            s.as_str().and_then(|id| {
                                parse_memory_id(id).ok().and_then(|bytes| {
                                    if bytes.len() == 16 {
                                        let mut arr = [0u8; 16];
                                        arr.copy_from_slice(&bytes);
                                        Some(arr)
                                    } else {
                                        None
                                    }
                                })
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            let tags: Vec<String> = v["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let cluster_id = v["cluster_id"].as_u64().unwrap_or(0) as usize;

            Ok(hebbs_reflect::ProducedInsight {
                content,
                confidence,
                source_memory_ids,
                tags,
                cluster_id,
            })
        })
        .collect()
}

fn memory_to_json(m: &hebbs_core::memory::Memory) -> serde_json::Value {
    // Local-mode fallback: uses default half-life since vault config is not
    // threaded here. Most commands route through the daemon which uses the
    // vault's configured values.
    let id = format_memory_id(&m.memory_id);
    let context: Option<serde_json::Value> = if m.context_bytes.is_empty() {
        None
    } else {
        serde_json::from_slice(&m.context_bytes).ok()
    };

    let now_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    let decay_score = hebbs_core::decay::compute_decay_score(
        m.importance,
        m.last_accessed_at,
        m.access_count,
        now_us,
        hebbs_core::decay::DEFAULT_HALF_LIFE_US,
        hebbs_core::decay::DEFAULT_REINFORCEMENT_CAP,
    );

    serde_json::json!({
        "memory_id": id,
        "content": m.content,
        "importance": m.importance,
        "entity_id": m.entity_id,
        "context": context,
        "created_at_us": m.created_at,
        "last_accessed_at_us": m.last_accessed_at,
        "access_count": m.access_count,
        "decay_score": decay_score,
    })
}

fn print_memory_detail(m: &hebbs_core::memory::Memory) {
    let id = format_memory_id(&m.memory_id);
    println!("ID:           {}", id);
    println!("Content:      {}", m.content);
    println!("Importance:   {:.2}", m.importance);
    if let Some(ref eid) = m.entity_id {
        println!("Entity:       {}", eid);
    }
    println!("Access count: {}", m.access_count);
    println!("Created:      {}", m.created_at);
    println!("Last access:  {}", m.last_accessed_at);
    if !m.context_bytes.is_empty() {
        if let Ok(ctx) = serde_json::from_slice::<serde_json::Value>(&m.context_bytes) {
            println!("Context:      {}", ctx);
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    // Strip markdown headers, collapse newlines/whitespace for clean display
    let collapsed: String = s
        .lines()
        .map(|line| line.trim_start_matches('#').trim_start_matches('*').trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.len() > max_len {
        format!("{}...", &collapsed[..max_len])
    } else {
        collapsed
    }
}
