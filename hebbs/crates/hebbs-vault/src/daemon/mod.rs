//! Unified daemon: single long-lived process serving all vaults on the machine.
//!
//! Listens on `~/.hebbs/daemon.sock` (Unix domain socket). The ONNX embedding
//! model is loaded once and shared across all vault handles. Vault RocksDB
//! instances are opened on demand and closed after idle.
//!
//! See `docs/plans/PLAN-daemon.md` for the full design.

pub mod client;
pub mod protocol;
pub mod vault_manager;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use globset::GlobSet;
use notify::EventKind;
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use hebbs_core::engine::{RememberEdge, RememberInput};
use hebbs_core::forget::ForgetCriteria;
use hebbs_core::memory::{Memory, MemoryKind};
use hebbs_core::recall::{PrimeInput, RecallInput, RecallStrategy, ScoringWeights};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig, ReflectScope};
use hebbs_core::tenant::TenantContext;
use hebbs_embed::Embedder;
use hebbs_index::graph::EdgeType;

use tokio::sync::broadcast;

use crate::config::VaultConfig;
use crate::daemon::protocol::*;
use crate::daemon::vault_manager::{VaultFsEvent, VaultManager};
use crate::ingest::{phase1_delete, phase1_ingest, phase2_ingest};
use crate::manifest::Manifest;
use crate::panel::PanelEvent;
use crate::watcher::{build_ignore_set, find_changed_files, is_relevant_md};

/// Default idle shutdown timeout: 5 minutes with no requests.
const DEFAULT_IDLE_SHUTDOWN_SECS: u64 = 300;

/// Health check interval: 30 seconds.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Idle eviction sweep interval: 60 seconds.
const EVICTION_INTERVAL: Duration = Duration::from_secs(60);

/// Default HTTP port for the Memory Palace control panel.
const DEFAULT_PANEL_PORT: u16 = 6381;

/// Snapshot of in-progress indexing for a vault.
/// Stored in shared state so `hebbs status` can report live progress.
#[derive(Clone)]
struct IndexingSnapshot {
    /// Total files being indexed.
    total_files: usize,
    /// Current phase: 1 or 2.
    phase: u8,
    /// Files completed in the current phase.
    files_done: usize,
    /// Currently processing file name (display only).
    current_file: String,
}

/// Daemon configuration passed to `run_daemon`.
pub struct DaemonConfig {
    /// Path to the Unix socket (default: `~/.hebbs/daemon.sock`).
    pub socket_path: PathBuf,
    /// Path to the PID file (default: `~/.hebbs/daemon.pid`).
    pub pid_path: PathBuf,
    /// Idle shutdown timeout in seconds. `0` disables idle shutdown.
    pub idle_timeout_secs: u64,
    /// Run in foreground (do not daemonize).
    pub foreground: bool,
    /// HTTP port for the Memory Palace control panel. `0` disables panel.
    pub panel_port: u16,
    /// Embedding model name (e.g. "bge-small-en-v1.5", "embeddinggemma-300m").
    pub embedding_model: String,
    /// Initial vault path for the panel to open on startup.
    /// When set, the panel opens this vault instead of the home directory vault.
    /// Passed by `hebbs panel <path>` or `hebbs init <path>`.
    pub initial_vault: Option<PathBuf>,
}

impl DaemonConfig {
    /// Default daemon config using `~/.hebbs/` as the runtime directory.
    pub fn default_config() -> Option<Self> {
        let home = dirs::home_dir()?;
        let runtime_dir = home.join(".hebbs");
        Some(Self {
            socket_path: runtime_dir.join("daemon.sock"),
            pid_path: runtime_dir.join("daemon.pid"),
            idle_timeout_secs: DEFAULT_IDLE_SHUTDOWN_SECS,
            foreground: false,
            panel_port: DEFAULT_PANEL_PORT,
            embedding_model: "bge-small-en-v1.5".to_string(),
            initial_vault: None,
        })
    }
}

/// Run the daemon event loop.
///
/// This function blocks until shutdown (signal, idle timeout, or explicit
/// shutdown command). It returns `Ok(())` on clean shutdown.
pub async fn run_daemon(config: DaemonConfig) -> Result<(), String> {
    let runtime_dir = config.socket_path.parent().ok_or("invalid socket path")?;
    std::fs::create_dir_all(runtime_dir)
        .map_err(|e| format!("failed to create runtime directory: {}", e))?;

    // Clean up stale socket/PID
    cleanup_stale(&config.socket_path, &config.pid_path)?;

    // Write PID file
    let pid = std::process::id();
    std::fs::write(&config.pid_path, pid.to_string())
        .map_err(|e| format!("failed to write PID file: {}", e))?;

    // Load ONNX embedder once from OS cache dir (survives vault/daemon cleanup).
    info!("loading embedding model...");
    let embed_config =
        hebbs_embed::EmbedderConfig::from_model_name_cached(&config.embedding_model);
    std::fs::create_dir_all(&embed_config.model_dir)
        .map_err(|e| format!("failed to create model directory: {}", e))?;
    let embedder: Arc<dyn Embedder> = Arc::new(
        hebbs_embed::OnnxEmbedder::new(embed_config)
            .map_err(|e| format!("failed to load embedder: {}", e))?,
    );
    info!(
        "embedding model loaded ({} dimensions)",
        embedder.dimensions()
    );

    // Create vault manager
    let idle_vault_timeout = if config.idle_timeout_secs > 0 {
        Some(Duration::from_secs(config.idle_timeout_secs))
    } else {
        None
    };
    let (mgr, watch_rx) = VaultManager::new(embedder.clone(), idle_vault_timeout);
    let vault_manager = Arc::new(Mutex::new(mgr));

    // Bind Unix socket
    if config.socket_path.exists() {
        std::fs::remove_file(&config.socket_path)
            .map_err(|e| format!("failed to remove stale socket: {}", e))?;
    }
    let listener = UnixListener::bind(&config.socket_path).map_err(|e| {
        format!(
            "failed to bind socket {}: {}",
            config.socket_path.display(),
            e
        )
    })?;
    info!("daemon listening on {}", config.socket_path.display());

    // Set socket permissions to owner-only (0o600)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&config.socket_path, perms).ok();
    }

    // Cancellation token for coordinated shutdown
    let cancel = CancellationToken::new();
    let last_request = Arc::new(Mutex::new(Instant::now()));
    let active_connections = Arc::new(AtomicUsize::new(0));

    // Shared indexing progress: tracks per-vault progress for `hebbs status` queries.
    // Key: canonical vault path, Value: current progress snapshot.
    let indexing_progress: Arc<Mutex<HashMap<PathBuf, IndexingSnapshot>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Signal handler: both SIGINT (Ctrl-C) and SIGTERM (kill)
    let cancel_signal = cancel.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("received SIGINT, shutting down");
                }
                _ = sigterm.recv() => {
                    info!("received SIGTERM, shutting down");
                }
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await.ok();
            info!("received shutdown signal");
        }
        cancel_signal.cancel();
    });

    // Idle shutdown timer -- runs on a dedicated OS thread so it is never
    // starved by blocking ONNX embedding calls on the tokio worker pool.
    if config.idle_timeout_secs > 0 {
        let cancel_idle = cancel.clone();
        let last_req = last_request.clone();
        let active = active_connections.clone();
        let timeout = Duration::from_secs(config.idle_timeout_secs);
        let check_interval = Duration::from_secs(30).min(timeout / 2);
        std::thread::Builder::new()
            .name("idle-timer".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(check_interval);
                    if cancel_idle.is_cancelled() {
                        break;
                    }
                    // Never idle-shutdown while connections are being served
                    let ac = active.load(Ordering::SeqCst);
                    if ac > 0 {
                        continue;
                    }
                    // Use try_lock to avoid blocking this thread on the mutex
                    let elapsed = match last_req.try_lock() {
                        Ok(guard) => guard.elapsed(),
                        Err(_) => continue, // mutex held = daemon is busy
                    };
                    if elapsed > timeout {
                        info!("idle shutdown: no requests for {}s", elapsed.as_secs());
                        cancel_idle.cancel();
                        break;
                    }
                }
            })
            .ok();
    }

    // Health check + eviction loop
    let cancel_health = cancel.clone();
    let vm_health = vault_manager.clone();
    tokio::spawn(async move {
        let mut health_tick = tokio::time::interval(HEALTH_CHECK_INTERVAL);
        let mut evict_tick = tokio::time::interval(EVICTION_INTERVAL);
        loop {
            tokio::select! {
                _ = cancel_health.cancelled() => break,
                _ = health_tick.tick() => {
                    let unhealthy = vm_health.lock().await.health_check();
                    for path in &unhealthy {
                        warn!("vault data directory missing: {}", path.display());
                    }
                }
                _ = evict_tick.tick() => {
                    let evicted = vm_health.lock().await.evict_idle();
                    if evicted > 0 {
                        info!("evicted {} idle vault(s)", evicted);
                    }
                }
            }
        }
    });

    // ── Config reload notification ─────────────────────────────────────
    // Shared between the panel (producer) and the watch loop (consumer).
    // When the panel saves config changes, it signals this Notify so the
    // watch loop reloads config from disk without a daemon restart.
    let config_notify = Arc::new(tokio::sync::Notify::new());

    // ── Panel event broadcast channel ────────────────────────────────
    // Created before the watch loop so both the watch loop (producer) and
    // the panel WebSocket handler (consumer) share the same channel.
    let (panel_event_tx, _panel_event_rx) = broadcast::channel::<PanelEvent>(256);

    // ── Proactive vault opening from vaults.json ──────────────────────
    // On startup, read vaults.json and open all registered vaults immediately
    // so their file watchers start right away.
    {
        let registered = read_vaults_json(runtime_dir);
        if !registered.is_empty() {
            info!(
                "proactively opening {} registered vault(s)",
                registered.len()
            );
            let mut vm = vault_manager.lock().await;
            for vault_path in &registered {
                if vault_path.join(".hebbs").exists() {
                    match vm.get_or_open(vault_path) {
                        Ok(_) => info!("proactively opened vault: {}", vault_path.display()),
                        Err(e) => {
                            warn!("failed to proactively open {}: {}", vault_path.display(), e)
                        }
                    }
                }
            }
        }
    }

    // ── vaults.json watcher ─────────────────────────────────────────────
    // Watch the ~/.hebbs/ directory for changes to vaults.json so we can
    // proactively open newly registered vaults without a daemon restart.
    let (vaults_json_tx, vaults_json_rx) = tokio::sync::mpsc::channel::<()>(16);
    let _vaults_json_watcher = {
        let tx = vaults_json_tx.clone();
        let mut watcher = notify::recommended_watcher(
            move |res: std::result::Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let is_vaults_json = event
                        .paths
                        .iter()
                        .any(|p| p.file_name().map(|n| n == "vaults.json").unwrap_or(false));
                    if is_vaults_json {
                        let _ = tx.blocking_send(());
                    }
                }
            },
        );
        match &mut watcher {
            Ok(w) => {
                use notify::Watcher;
                if let Err(e) = w.watch(runtime_dir, notify::RecursiveMode::NonRecursive) {
                    warn!(
                        "failed to watch {} for vaults.json changes: {}",
                        runtime_dir.display(),
                        e
                    );
                } else {
                    info!("watching {} for vaults.json changes", runtime_dir.display());
                }
            }
            Err(e) => {
                warn!("failed to create vaults.json watcher: {}", e);
            }
        }
        watcher.ok()
    };

    // ── Watch event processing (Milestone 3) ───────────────────────────
    // Receives filesystem events from per-vault watchers and runs the
    // two-phase ingest pipeline with debouncing, exactly like the old
    // standalone `hebbs watch` but for all open vaults simultaneously.
    let cancel_watch = cancel.clone();
    let vm_watch = vault_manager.clone();
    let config_notify_watch = config_notify.clone();
    let event_tx_watch = panel_event_tx.clone();
    let runtime_dir_watch = runtime_dir.to_path_buf();
    tokio::spawn(async move {
        run_watch_loop(
            watch_rx,
            vm_watch,
            cancel_watch,
            config_notify_watch,
            event_tx_watch,
            vaults_json_rx,
            runtime_dir_watch,
        )
        .await;
    });

    // ── Panel HTTP server (Milestone 6) ──────────────────────────────
    // Serves the Memory Palace control panel from the daemon process.
    // Opens the global vault's engine for the panel to use.
    if config.panel_port > 0 {
        // Determine which vault to show in the panel on startup:
        // 1. If an initial_vault was explicitly set (from `hebbs panel <path>`), use that.
        // 2. Otherwise fall back to the home directory vault.
        let panel_vault_root = config.initial_vault.clone().unwrap_or_else(|| {
            runtime_dir
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| runtime_dir.to_path_buf())
        });
        let panel_engine = if panel_vault_root.join(".hebbs").exists() {
            match vault_manager.lock().await.get_or_open(&panel_vault_root) {
                Ok((engine, panel_embedder)) => Some((engine, panel_embedder, panel_vault_root)),
                Err(e) => {
                    warn!("panel: failed to open vault at {}: {}, panel disabled", panel_vault_root.display(), e);
                    None
                }
            }
        } else {
            warn!(
                "panel: vault not initialized at {}, panel disabled",
                panel_vault_root.display()
            );
            None
        };

        if let Some((engine, panel_embedder, vault_root)) = panel_engine {
            match crate::panel::start_panel_server_from_daemon(
                engine,
                panel_embedder,
                vault_root,
                config.panel_port,
                vault_manager.clone(),
                last_request.clone(),
                config_notify.clone(),
                panel_event_tx.clone(),
            )
            .await
            {
                Ok(addr) => {
                    info!("panel server listening on http://{}", addr);
                }
                Err(e) => {
                    warn!("panel: failed to start HTTP server: {}", e);
                }
            }
        }
    }

    // Accept loop
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("shutting down daemon");
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let vm = vault_manager.clone();
                        let last_req = last_request.clone();
                        let cancel_conn = cancel.clone();
                        let active = active_connections.clone();
                        let idx_progress = indexing_progress.clone();
                        active.fetch_add(1, Ordering::SeqCst);
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, vm, last_req, idx_progress, cancel_conn).await {
                                warn!("connection error: {}", e);
                            }
                            active.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Err(e) => {
                        if cancel.is_cancelled() {
                            break;
                        }
                        error!("accept error: {}", e);
                    }
                }
            }
        }
    }

    // Cleanup
    vault_manager.lock().await.close_all();
    cleanup_files(&config.socket_path, &config.pid_path);
    info!("daemon stopped");
    Ok(())
}

/// Handle a single client connection (may send multiple requests).
async fn handle_connection(
    stream: tokio::net::UnixStream,
    vault_manager: Arc<Mutex<VaultManager>>,
    last_request: Arc<Mutex<Instant>>,
    indexing_progress: Arc<Mutex<HashMap<PathBuf, IndexingSnapshot>>>,
    cancel: CancellationToken,
) -> Result<(), String> {
    let (mut reader, mut writer) = tokio::io::split(stream);

    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }

        let request: Option<DaemonRequest> = read_message(&mut reader)
            .await
            .map_err(|e| format!("read error: {}", e))?;

        let request = match request {
            Some(r) => r,
            None => return Ok(()), // Client disconnected
        };

        // Update last request time
        *last_request.lock().await = Instant::now();

        // Check for shutdown command
        if matches!(request.command, Command::Shutdown) {
            let resp = DaemonResponse::ok(serde_json::json!({"shutdown": true}));
            write_message(&mut writer, &resp)
                .await
                .map_err(|e| e.to_string())?;
            cancel.cancel();
            return Ok(());
        }

        let response = dispatch_command(request, &vault_manager, &indexing_progress, &mut writer).await;

        write_message(&mut writer, &response)
            .await
            .map_err(|e| format!("write error: {}", e))?;
    }
}

/// Dispatch a single command to the appropriate handler.
///
/// The writer is passed so that long-running commands (e.g. Index) can stream
/// `Progress` messages to the client before sending the final response.
async fn dispatch_command(
    request: DaemonRequest,
    vault_manager: &Arc<Mutex<VaultManager>>,
    indexing_progress: &Arc<Mutex<HashMap<PathBuf, IndexingSnapshot>>>,
    writer: &mut tokio::io::WriteHalf<tokio::net::UnixStream>,
) -> DaemonResponse {
    let extra_vault_paths = request.vault_paths.clone();
    match request.command {
        Command::Ping => DaemonResponse::ok(serde_json::json!({"pong": true})),

        Command::Shutdown => {
            // Handled in handle_connection
            DaemonResponse::ok(serde_json::json!({"shutdown": true}))
        }

        Command::Remember {
            content,
            importance,
            context,
            entity_id,
            edges,
        } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let ctx = match parse_context_json(context.as_deref()) {
                Ok(c) => c,
                Err(e) => return DaemonResponse::err(e),
            };
            let parsed_edges = match parse_edge_specs(&edges) {
                Ok(e) => e,
                Err(e) => return DaemonResponse::err(e),
            };

            let input = RememberInput {
                content,
                importance,
                context: ctx,
                entity_id,
                edges: parsed_edges,
            kind: None,
        };

            match engine.remember(input) {
                Ok(memory) => DaemonResponse::ok(memory_to_json(&memory)),
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::Get { id } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let memory_id = match parse_memory_id(&id) {
                Ok(id) => id,
                Err(e) => return DaemonResponse::err(e),
            };

            match engine.get(&memory_id) {
                Ok(memory) => DaemonResponse::ok(memory_to_json(&memory)),
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::Recall {
            cue,
            strategy,
            top_k,
            entity_id,
            max_depth,
            seed,
            weights,
            ef_search,
            ..
        } => {
            let recall_start = Instant::now();
            let caller = request.caller.clone();
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };

            let strat = match strategy.as_deref() {
                Some("temporal") => RecallStrategy::Temporal,
                Some("causal") => RecallStrategy::Causal,
                Some("analogical") => RecallStrategy::Analogical,
                _ => RecallStrategy::Similarity,
            };

            let strat_str = strategy.as_deref().unwrap_or("similarity");

            let scoring_weights = weights
                .as_deref()
                .and_then(|w| parse_scoring_weights(w).ok());

            let seed_memory_id = seed.as_deref().and_then(|s| {
                parse_memory_id(s).ok().and_then(|id| {
                    if id.len() == 16 {
                        let mut arr = [0u8; 16];
                        arr.copy_from_slice(&id);
                        Some(arr)
                    } else {
                        None
                    }
                })
            });

            let build_input = |eid: Option<String>| RecallInput {
                cue: cue.clone().unwrap_or_default(),
                strategies: vec![strat.clone()],
                top_k: Some(top_k as usize),
                entity_id: eid,
                time_range: None,
                edge_types: None,
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

            // Collect all vault paths to query
            let mut all_vault_paths = vec![vault_path.clone()];
            if let Some(extras) = extra_vault_paths {
                all_vault_paths.extend(extras);
            }

            // Open all vault engines
            let mut engines = Vec::new();
            {
                let mut mgr = vault_manager.lock().await;
                for vp in &all_vault_paths {
                    match mgr.get_or_open(vp) {
                        Ok((engine, _)) => engines.push(engine),
                        Err(e) => return DaemonResponse::err(e),
                    }
                }
            }

            // Run recall on all engines and merge results
            let mut all_results: Vec<(serde_json::Value, f64)> = Vec::new();
            for engine in &engines {
                let input = build_input(entity_id.clone());
                match engine.recall(input) {
                    Ok(output) => {
                        for r in &output.results {
                            let mut m = memory_to_json(&r.memory);
                            m["score"] = serde_json::json!(r.score);
                            all_results.push((m, r.score as f64));
                        }
                    }
                    Err(e) => return DaemonResponse::err(format!("{}", e)),
                }
            }

            // Sort by score descending, truncate to top_k
            all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            all_results.truncate(top_k as usize);

            let results: Vec<serde_json::Value> = all_results.into_iter().map(|(m, _)| m).collect();
            let count = results.len();

            // Query audit log: fire-and-forget, never degrades recall latency
            let latency_us = recall_start.elapsed().as_micros() as u64;
            if let Some(engine) = engines.first() {
                let result_ids: Vec<String> = results
                    .iter()
                    .filter_map(|r| {
                        r.get("memory_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
                let top_score = results
                    .first()
                    .and_then(|r| r.get("score").and_then(|v| v.as_f64()))
                    .unwrap_or(0.0) as f32;
                let entry = crate::query_log::build_recall_entry(
                    &caller,
                    cue.as_deref().unwrap_or(""),
                    Some(strat_str),
                    top_k,
                    entity_id.as_deref(),
                    count as u32,
                    result_ids,
                    top_score,
                    latency_us,
                    Some(&vault_path.to_string_lossy()),
                );
                if let Err(e) = crate::query_log::append_to_storage(engine.storage(), &entry) {
                    warn!("failed to write query log: {}", e);
                }
            }

            // Check for contradictions among returned results.
            // O(k * log n): for each result, query its contradiction edges and
            // check if any target is also in the result set.
            let mut contradictions: Vec<serde_json::Value> = Vec::new();
            if count >= 2 {
                // Build set of returned memory IDs (raw 16-byte form)
                let result_id_set: HashSet<[u8; 16]> = results.iter().filter_map(|r| {
                    r.get("memory_id").and_then(|v| v.as_str()).and_then(|id_str| {
                        parse_memory_id(id_str).ok().and_then(|bytes| {
                            if bytes.len() == 16 {
                                let mut arr = [0u8; 16];
                                arr.copy_from_slice(&bytes);
                                Some(arr)
                            } else {
                                None
                            }
                        })
                    })
                }).collect();

                if let Some(engine) = engines.first() {
                    let mut seen_pairs: HashSet<([u8; 16], [u8; 16])> = HashSet::new();
                    for id in &result_id_set {
                        if let Ok(edges) = engine.contradictions(id) {
                            for (target, confidence) in edges {
                                if result_id_set.contains(&target) {
                                    // Deduplicate bidirectional edges
                                    let pair = if *id < target { (*id, target) } else { (target, *id) };
                                    if seen_pairs.insert(pair) {
                                        contradictions.push(serde_json::json!({
                                            "memory_a": format_memory_id(id),
                                            "memory_b": format_memory_id(&target),
                                            "confidence": confidence,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Include indexing completeness so the CLI can show "N% indexed" hints.
            // O(1): reads manifest section counts only, no engine query.
            let indexing_pct: Option<usize> = crate::status(&vault_path).ok().and_then(|s| {
                let total = s.synced + s.content_stale + s.orphaned;
                if total > 0 && s.content_stale > 0 {
                    Some(s.synced * 100 / total)
                } else {
                    None // omit field when fully indexed
                }
            });

            let mut resp = serde_json::json!({
                "results": results,
                "count": count,
            });
            if !contradictions.is_empty() {
                resp["contradictions"] = serde_json::json!(contradictions);
            }
            if let Some(pct) = indexing_pct {
                resp["indexing_pct"] = serde_json::json!(pct);
            }
            DaemonResponse::ok(resp)
        }

        Command::Forget {
            ids,
            entity_id,
            staleness_us,
            access_floor,
            kind,
            decay_floor,
        } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let memory_ids: Result<Vec<Vec<u8>>, String> =
                ids.iter().map(|id| parse_memory_id(id)).collect();
            let memory_ids = match memory_ids {
                Ok(ids) => ids,
                Err(e) => return DaemonResponse::err(e),
            };

            let memory_kind = kind.as_deref().and_then(|k| match k {
                "episode" => Some(MemoryKind::Episode),
                "insight" => Some(MemoryKind::Insight),
                "revision" => Some(MemoryKind::Revision),
                _ => None,
            });

            let criteria = ForgetCriteria {
                memory_ids,
                entity_id,
                staleness_threshold_us: staleness_us,
                access_count_floor: access_floor,
                memory_kind,
                decay_score_floor: decay_floor,
            };

            match engine.forget(criteria) {
                Ok(output) => DaemonResponse::ok(serde_json::json!({
                    "forgotten_count": output.forgotten_count,
                    "cascade_count": output.cascade_count,
                    "truncated": output.truncated,
                })),
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::Prime {
            entity_id,
            context,
            max_memories,
            recency_us,
            similarity_cue,
        } => {
            let prime_start = Instant::now();
            let caller = request.caller.clone();
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };

            let ctx = match parse_context_json(context.as_deref()) {
                Ok(c) => c,
                Err(e) => return DaemonResponse::err(e),
            };

            // Collect all vault paths to query
            let mut all_vault_paths = vec![vault_path.clone()];
            if let Some(extras) = extra_vault_paths {
                all_vault_paths.extend(extras);
            }

            // Open all vault engines
            let mut engines = Vec::new();
            {
                let mut mgr = vault_manager.lock().await;
                for vp in &all_vault_paths {
                    match mgr.get_or_open(vp) {
                        Ok((engine, _)) => engines.push(engine),
                        Err(e) => return DaemonResponse::err(e),
                    }
                }
            }

            // Run prime on all engines and merge results
            let mut all_results: Vec<(serde_json::Value, f64)> = Vec::new();
            let mut total_temporal = 0u64;
            let mut total_similarity = 0u64;
            let max_mem = max_memories.map(|m| m as usize);

            for engine in &engines {
                let input = PrimeInput {
                    entity_id: entity_id.clone(),
                    context: ctx.clone(),
                    max_memories: max_mem,
                    recency_window_us: recency_us,
                    similarity_cue: similarity_cue.clone(),
                    scoring_weights: None,
                };

                match engine.prime(input) {
                    Ok(output) => {
                        total_temporal += output.temporal_count as u64;
                        total_similarity += output.similarity_count as u64;
                        for r in &output.results {
                            let mut m = memory_to_json(&r.memory);
                            m["score"] = serde_json::json!(r.score);
                            all_results.push((m, r.score as f64));
                        }
                    }
                    Err(e) => return DaemonResponse::err(format!("{}", e)),
                }
            }

            // Sort by score descending, truncate to max_memories
            all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            if let Some(max) = max_mem {
                all_results.truncate(max);
            }

            let results: Vec<serde_json::Value> = all_results.into_iter().map(|(m, _)| m).collect();

            // Query audit log: fire-and-forget
            let latency_us = prime_start.elapsed().as_micros() as u64;
            if let Some(engine) = engines.first() {
                let result_ids: Vec<String> = results
                    .iter()
                    .filter_map(|r| {
                        r.get("memory_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
                let top_score = results
                    .first()
                    .and_then(|r| r.get("score").and_then(|v| v.as_f64()))
                    .unwrap_or(0.0) as f32;
                let entry = crate::query_log::build_prime_entry(
                    &caller,
                    &entity_id,
                    similarity_cue.as_deref(),
                    max_memories.unwrap_or(20),
                    results.len() as u32,
                    result_ids,
                    top_score,
                    latency_us,
                    Some(&vault_path.to_string_lossy()),
                );
                if let Err(e) = crate::query_log::append_to_storage(engine.storage(), &entry) {
                    warn!("failed to write query log: {}", e);
                }
            }

            DaemonResponse::ok(serde_json::json!({
                "results": results,
                "temporal_count": total_temporal,
                "similarity_count": total_similarity,
            }))
        }

        Command::Inspect { id } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let memory_id = match parse_memory_id(&id) {
                Ok(id) => id,
                Err(e) => return DaemonResponse::err(e),
            };

            match engine.get(&memory_id) {
                Ok(memory) => {
                    let mut result = memory_to_json(&memory);
                    // Add graph edges
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(&memory_id);
                    if let Ok(edges) = engine.outgoing_edges(&arr) {
                        let edge_list: Vec<serde_json::Value> = edges
                            .iter()
                            .map(|(edge_type, target, meta)| {
                                serde_json::json!({
                                    "target_id": format_memory_id(target),
                                    "edge_type": format!("{:?}", edge_type),
                                    "confidence": meta.confidence,
                                })
                            })
                            .collect();
                        result["edges"] = serde_json::json!(edge_list);
                    }
                    DaemonResponse::ok(result)
                }
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::Export { entity_id, limit } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let capped_limit = limit.min(10000) as usize;
            let input = RecallInput {
                cue: String::new(),
                strategies: vec![RecallStrategy::Temporal],
                top_k: Some(capped_limit),
                entity_id,
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
                    let memories: Vec<serde_json::Value> = output
                        .results
                        .iter()
                        .map(|r| memory_to_json(&r.memory))
                        .collect();
                    DaemonResponse::ok(serde_json::json!({
                        "memories": memories,
                        "count": output.results.len(),
                        "truncated": output.results.len() >= capped_limit,
                    }))
                }
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::Status => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let is_watched = vault_manager.lock().await.is_open(&vault_path);
            let idx_snap = indexing_progress.lock().await.get(&vault_path).cloned();
            match crate::status(&vault_path) {
                Ok(s) => {
                    let mut resp = serde_json::json!({
                        "vault_root": s.vault_root.display().to_string(),
                        "total_files": s.total_files,
                        "total_sections": s.total_sections,
                        "total_memories": s.synced,
                        "synced": s.synced,
                        "content_stale": s.content_stale,
                        "orphaned": s.orphaned,
                        "llm_provider": s.llm_provider,
                        "llm_model": s.llm_model,
                        "daemon_watching": is_watched,
                    });
                    // Include live indexing progress if indexing is in progress
                    if let Some(snap) = idx_snap {
                        resp["indexing"] = serde_json::json!({
                            "phase": snap.phase,
                            "total_files": snap.total_files,
                            "files_done": snap.files_done,
                            "current_file": snap.current_file,
                        });
                    }
                    DaemonResponse::ok(resp)
                }
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::Index => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };

            // Helper: send a progress message (ignore write errors — client may have disconnected)
            macro_rules! send_progress {
                ($msg:expr) => {
                    let _ = write_message(writer, &DaemonResponse::progress($msg)).await;
                };
            }

            send_progress!("Opening vault...");
            let (engine, embedder) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let hebbs_dir = vault_path.join(".hebbs");
            let config = match crate::config::VaultConfig::load(&hebbs_dir) {
                Ok(c) => c,
                Err(e) => return DaemonResponse::err(format!("failed to load config: {}", e)),
            };
            let mut manifest = match crate::manifest::Manifest::load(&hebbs_dir) {
                Ok(m) => m,
                Err(e) => return DaemonResponse::err(format!("failed to load manifest: {}", e)),
            };

            // Collect files
            let all_files = match crate::watcher::collect_md_files(&vault_path, &config) {
                Ok(f) => f,
                Err(e) => return DaemonResponse::err(format!("failed to collect files: {}", e)),
            };
            let total_files = all_files.len();

            // Set initial indexing progress
            {
                let mut prog = indexing_progress.lock().await;
                prog.insert(vault_path.clone(), IndexingSnapshot {
                    total_files,
                    phase: 1,
                    files_done: 0,
                    current_file: String::new(),
                });
            }

            send_progress!(format!("Phase 1/2: parsing {} file(s)...", total_files));

            // Phase 1: parse
            let p1_stats = match crate::ingest::phase1_ingest(&all_files, &vault_path, &mut manifest, &config) {
                Ok(s) => s,
                Err(e) => {
                    indexing_progress.lock().await.remove(&vault_path);
                    return DaemonResponse::err(format!("phase1 failed: {}", e));
                }
            };
            manifest.save(&hebbs_dir).ok();

            // Update progress: phase 1 complete, entering phase 2
            let (synced, stale, orphaned) = manifest.section_counts();
            let sections_to_process = stale + orphaned;
            {
                let mut prog = indexing_progress.lock().await;
                if let Some(snap) = prog.get_mut(&vault_path) {
                    snap.phase = 2;
                    snap.files_done = 0;
                }
            }

            send_progress!(format!(
                "Phase 2/2: embedding {} section(s)...",
                sections_to_process
            ));

            // Phase 2: LLM extraction (file-first pipeline)
            // Skip contradiction detection on first full index
            // (no existing memories to contradict against).
            let is_first_index = synced == 0;
            let run_contradictions = !is_first_index;
            let idx_progress = indexing_progress.clone();
            let vault_path_progress = vault_path.clone();
            let progress_cb: Option<Box<dyn Fn(usize, usize, &str) + Send>> = Some(Box::new(move |done, total, file| {
                if let Ok(mut prog) = idx_progress.try_lock() {
                    if let Some(snap) = prog.get_mut(&vault_path_progress) {
                        snap.files_done = done;
                        snap.total_files = total;
                        snap.current_file = file.to_string();
                    }
                }
            }));
            let p2_stats = crate::ingest::phase2_ingest_with_progress(
                &vault_path, &mut manifest, &engine, &embedder, &config,
                run_contradictions, progress_cb,
            ).await;
            let p2_stats = match p2_stats {
                Ok(s) => s,
                Err(e) => {
                    indexing_progress.lock().await.remove(&vault_path);
                    return DaemonResponse::err(format!("phase2 failed: {}", e));
                }
            };
            manifest.save(&hebbs_dir).ok();

            // Clear indexing progress (done)
            indexing_progress.lock().await.remove(&vault_path);

            DaemonResponse::ok(serde_json::json!({
                "total_files": total_files,
                "sections_new": p1_stats.sections_new,
                "sections_modified": p1_stats.sections_modified,
                "sections_embedded": p2_stats.sections_embedded,
                "sections_remembered": p2_stats.sections_remembered,
                "sections_revised": p2_stats.sections_revised,
                "sections_forgotten": p2_stats.sections_forgotten,
            }))
        }

        Command::List { sections } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let hebbs_dir = vault_path.join(".hebbs");
            if !hebbs_dir.exists() {
                return DaemonResponse::err(format!(
                    "vault not initialized at {}",
                    vault_path.display()
                ));
            }
            match crate::manifest::Manifest::load(&hebbs_dir) {
                Ok(manifest) => {
                    let mut files: Vec<serde_json::Value> = manifest
                        .files
                        .iter()
                        .map(|(fp, entry)| {
                            let mut file_obj = serde_json::json!({
                                "path": fp,
                                "section_count": entry.sections.len(),
                                "synced": entry.sections.iter()
                                    .filter(|s| matches!(s.state, crate::manifest::SectionState::Synced))
                                    .count(),
                            });
                            if sections {
                                let sec_list: Vec<serde_json::Value> = entry
                                    .sections
                                    .iter()
                                    .map(|s| {
                                        serde_json::json!({
                                            "state": format!("{:?}", s.state),
                                            "heading_path": s.heading_path.join(" > "),
                                            "memory_id": &s.memory_id[..16.min(s.memory_id.len())],
                                            "byte_start": s.byte_start,
                                            "byte_end": s.byte_end,
                                        })
                                    })
                                    .collect();
                                file_obj["sections"] = serde_json::json!(sec_list);
                            }
                            file_obj
                        })
                        .collect();
                    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
                    DaemonResponse::ok(serde_json::json!({
                        "files": files,
                        "total_files": manifest.files.len(),
                    }))
                }
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::ReflectPrepare {
            entity_id,
            since_us,
        } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let scope = match entity_id {
                Some(eid) => ReflectScope::Entity {
                    entity_id: eid,
                    since_us,
                },
                None => ReflectScope::Global { since_us },
            };
            let config = ReflectConfig::default();

            match engine.reflect_prepare_for_tenant(&TenantContext::default(), scope, &config) {
                Ok(result) => {
                    let clusters: Vec<serde_json::Value> = result
                        .clusters
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "cluster_id": c.cluster_id,
                                "member_count": c.member_count,
                                "system_prompt": c.proposal_system_prompt,
                                "user_prompt": c.proposal_user_prompt,
                                "memory_ids": c.memory_ids.iter()
                                    .map(|id| format_memory_id(id))
                                    .collect::<Vec<_>>(),
                            })
                        })
                        .collect();
                    DaemonResponse::ok(serde_json::json!({
                        "session_id": result.session_id,
                        "memories_processed": result.memories_processed,
                        "clusters": clusters,
                    }))
                }
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::ReflectCommit {
            session_id,
            insights,
        } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let parsed = match parse_produced_insights_json(&insights) {
                Ok(i) => i,
                Err(e) => return DaemonResponse::err(e),
            };

            match engine.reflect_commit_for_tenant(&TenantContext::default(), &session_id, parsed) {
                Ok(result) => DaemonResponse::ok(serde_json::json!({
                    "insights_created": result.insights_created,
                })),
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::ContradictionPrepare {} => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            match engine.contradiction_prepare() {
                Ok(candidates) => {
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
                    DaemonResponse::ok(serde_json::json!({
                        "candidates": items,
                        "count": candidates.len(),
                    }))
                }
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::ContradictionCommit { results } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let verdicts: Vec<hebbs_core::contradict::ContradictionVerdict> =
                match serde_json::from_str(&results) {
                    Ok(v) => v,
                    Err(e) => {
                        return DaemonResponse::err(format!("failed to parse verdicts JSON: {}", e))
                    }
                };

            match engine.contradiction_commit(&verdicts) {
                Ok(result) => {
                    // Write contradiction markdown files for confirmed contradictions
                    if !result.confirmed.is_empty() {
                        let hebbs_dir = vault_path.join(".hebbs");
                        let config_res = VaultConfig::load(&hebbs_dir);
                        let manifest_res = Manifest::load(&hebbs_dir);
                        if let (Ok(config), Ok(manifest)) = (config_res, manifest_res) {
                            let mut outputs: Vec<crate::contradiction_writer::ContradictionOutput> =
                                Vec::new();
                            for c in &result.confirmed {
                                let content_a = engine
                                    .get(&c.memory_id_a)
                                    .map(|m| m.content.clone())
                                    .unwrap_or_default();
                                let content_b = engine
                                    .get(&c.memory_id_b)
                                    .map(|m| m.content.clone())
                                    .unwrap_or_default();
                                outputs.push(crate::contradiction_writer::ContradictionOutput {
                                    content_a,
                                    content_b,
                                    memory_id_a: c.memory_id_a,
                                    memory_id_b: c.memory_id_b,
                                    confidence: c.confidence,
                                    method: c.method,
                                });
                            }
                            let writer = crate::contradiction_writer::ContradictionWriter::new(
                                &vault_path,
                                &manifest,
                                &config,
                            );
                            if let Err(e) = writer.write_contradictions(&outputs) {
                                warn!("failed to write contradiction files: {}", e);
                            }
                        }
                    }

                    DaemonResponse::ok(serde_json::json!({
                        "contradictions_confirmed": result.contradictions_confirmed,
                        "revisions_created": result.revisions_created,
                        "dismissed": result.dismissed,
                    }))
                }
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::Insights {
            entity_id,
            min_confidence,
            max_results,
        } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let filter = InsightsFilter {
                entity_id,
                min_confidence,
                max_results: max_results.map(|m| m as usize),
            };

            match engine.insights(filter) {
                Ok(memories) => {
                    let results: Vec<serde_json::Value> =
                        memories.iter().map(memory_to_json).collect();
                    DaemonResponse::ok(serde_json::json!({
                        "insights": results,
                        "count": memories.len(),
                    }))
                }
                Err(e) => DaemonResponse::err(format!("{}", e)),
            }
        }

        Command::Queries {
            limit,
            offset,
            caller_filter,
            operation_filter,
        } => {
            let vault_path = match require_vault_path(&request.vault_path) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            let (engine, _) = match vault_manager.lock().await.get_or_open(&vault_path) {
                Ok(pair) => pair,
                Err(e) => return DaemonResponse::err(e),
            };

            let operation = operation_filter.as_deref().and_then(|op| match op {
                "recall" => Some(crate::query_log::QueryOperation::Recall),
                "prime" => Some(crate::query_log::QueryOperation::Prime),
                _ => None,
            });

            let params = crate::query_log::QueryLogListParams {
                limit,
                offset,
                caller: caller_filter,
                operation,
                ..Default::default()
            };

            let store =
                crate::query_log::QueryLogStore::new(std::sync::Arc::new(StorageRef(engine)));
            match store.list(&params) {
                Ok(entries) => {
                    let count = entries.len();
                    DaemonResponse::ok(serde_json::json!({
                        "entries": entries,
                        "count": count,
                    }))
                }
                Err(e) => DaemonResponse::err(e),
            }
        }
    }
}

/// Adapter that delegates StorageBackend calls to the Engine's storage.
struct StorageRef(Arc<hebbs_core::engine::Engine>);

impl hebbs_storage::StorageBackend for StorageRef {
    fn put(
        &self,
        cf: hebbs_storage::ColumnFamilyName,
        key: &[u8],
        value: &[u8],
    ) -> hebbs_storage::Result<()> {
        self.0.storage().put(cf, key, value)
    }
    fn get(
        &self,
        cf: hebbs_storage::ColumnFamilyName,
        key: &[u8],
    ) -> hebbs_storage::Result<Option<Vec<u8>>> {
        self.0.storage().get(cf, key)
    }
    fn delete(&self, cf: hebbs_storage::ColumnFamilyName, key: &[u8]) -> hebbs_storage::Result<()> {
        self.0.storage().delete(cf, key)
    }
    fn write_batch(
        &self,
        operations: &[hebbs_storage::BatchOperation],
    ) -> hebbs_storage::Result<()> {
        self.0.storage().write_batch(operations)
    }
    fn prefix_iterator(
        &self,
        cf: hebbs_storage::ColumnFamilyName,
        prefix: &[u8],
    ) -> hebbs_storage::Result<Vec<(Vec<u8>, Vec<u8>)>> {
        self.0.storage().prefix_iterator(cf, prefix)
    }
    fn range_iterator(
        &self,
        cf: hebbs_storage::ColumnFamilyName,
        start: &[u8],
        end: &[u8],
    ) -> hebbs_storage::Result<Vec<(Vec<u8>, Vec<u8>)>> {
        self.0.storage().range_iterator(cf, start, end)
    }
    fn compact(&self, cf: hebbs_storage::ColumnFamilyName) -> hebbs_storage::Result<()> {
        self.0.storage().compact(cf)
    }
}

// ── Watch event processing (Milestone 3) ─────────────────────────────

/// Per-vault state for the two-phase debounced ingest pipeline.
struct VaultWatchState {
    vault_root: PathBuf,
    config: VaultConfig,
    manifest: Manifest,
    ignore_set: GlobSet,
    pending_creates: HashSet<PathBuf>,
    pending_deletes: HashSet<PathBuf>,
    events_in_window: usize,
    phase1_armed: bool,
    has_stale_sections: bool,
    phase2_deadline: Option<tokio::time::Instant>,
    /// Whether we've run the startup catch-up scan for this vault.
    caught_up: bool,
}

impl VaultWatchState {
    /// Reload manifest from disk to pick up changes made by other code paths
    /// (e.g., the Index command handler). This prevents the watch loop from
    /// overwriting a good manifest with a stale in-memory copy.
    fn reload_manifest(&mut self) {
        let hebbs_dir = self.vault_root.join(".hebbs");
        match Manifest::load(&hebbs_dir) {
            Ok(m) => self.manifest = m,
            Err(e) => warn!(
                "[watch:{}] failed to reload manifest: {}",
                self.vault_root.display(),
                e
            ),
        }
    }

    fn new(vault_root: PathBuf) -> Option<Self> {
        let hebbs_dir = vault_root.join(".hebbs");
        let config = VaultConfig::load(&hebbs_dir).ok()?;
        let manifest = Manifest::load(&hebbs_dir).ok()?;
        let ignore_set = build_ignore_set(&config.effective_ignore_patterns()).ok()?;
        Some(Self {
            vault_root,
            config,
            manifest,
            ignore_set,
            pending_creates: HashSet::new(),
            pending_deletes: HashSet::new(),
            events_in_window: 0,
            phase1_armed: false,
            has_stale_sections: false,
            phase2_deadline: None,
            caught_up: false,
        })
    }

    fn hebbs_dir(&self) -> PathBuf {
        self.vault_root.join(".hebbs")
    }
}

/// Main watch loop: receives events from all vault watchers, debounces,
/// and runs the two-phase ingest pipeline per vault.
async fn run_watch_loop(
    mut rx: tokio::sync::mpsc::Receiver<VaultFsEvent>,
    vault_manager: Arc<Mutex<VaultManager>>,
    cancel: CancellationToken,
    config_notify: Arc<tokio::sync::Notify>,
    panel_event_tx: broadcast::Sender<PanelEvent>,
    mut vaults_json_rx: tokio::sync::mpsc::Receiver<()>,
    runtime_dir: PathBuf,
) {
    let mut states: HashMap<PathBuf, VaultWatchState> = HashMap::new();

    // Use a 200ms tick for checking phase1/phase2 timers across all vaults.
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                // On shutdown, only save if we have pending work (phase1_armed or has_stale_sections).
                // Otherwise, the on-disk manifest is already up to date and saving our
                // in-memory copy could overwrite updates from the Index command handler.
                for state in states.values() {
                    if state.phase1_armed || state.has_stale_sections {
                        if let Err(e) = state.manifest.save(&state.hebbs_dir()) {
                            warn!("failed to save manifest for {} on shutdown: {}", state.vault_root.display(), e);
                        }
                    }
                }
                break;
            }

            _ = config_notify.notified() => {
                // Reload config from disk for all tracked vaults
                for state in states.values_mut() {
                    let hebbs_dir = state.hebbs_dir();
                    match VaultConfig::load(&hebbs_dir) {
                        Ok(new_config) => {
                            // Rebuild ignore set from updated patterns
                            match build_ignore_set(&new_config.effective_ignore_patterns()) {
                                Ok(new_ignore) => {
                                    state.ignore_set = new_ignore;
                                }
                                Err(e) => {
                                    warn!("[watch:{}] failed to rebuild ignore set: {}", state.vault_root.display(), e);
                                }
                            }
                            state.config = new_config;
                            info!("[watch:{}] config reloaded", state.vault_root.display());
                            let _ = panel_event_tx.send(PanelEvent::ConfigReloaded);
                        }
                        Err(e) => {
                            warn!("[watch:{}] failed to reload config: {}", state.vault_root.display(), e);
                        }
                    }
                }
            }

            _ = vaults_json_rx.recv() => {
                // vaults.json changed: open any newly registered vaults.
                // Removed vaults are left to idle-eviction to avoid disrupting
                // in-progress operations.
                let registered = read_vaults_json(&runtime_dir);
                let mut vm = vault_manager.lock().await;
                for vault_path in &registered {
                    let canonical = vault_path.canonicalize()
                        .unwrap_or_else(|_| vault_path.clone());
                    if !vm.is_open(&canonical) && canonical.join(".hebbs").exists() {
                        match vm.get_or_open(&canonical) {
                            Ok(_) => info!("[vaults.json] proactively opened new vault: {}", canonical.display()),
                            Err(e) => warn!("[vaults.json] failed to open {}: {}", canonical.display(), e),
                        }
                    }
                }
            }

            event = rx.recv() => {
                let event = match event {
                    Some(e) => e,
                    None => break, // Channel closed
                };

                let canonical = event.vault_path.canonicalize()
                    .unwrap_or_else(|_| event.vault_path.clone());

                // Get or create per-vault state
                let state = states.entry(canonical.clone()).or_insert_with(|| {
                    match VaultWatchState::new(canonical.clone()) {
                        Some(s) => s,
                        None => {
                            warn!("failed to load vault state for {}", canonical.display());
                            // Return a dummy that won't match any files
                            VaultWatchState {
                                vault_root: canonical.clone(),
                                config: VaultConfig::default(),
                                manifest: Manifest::default(),
                                ignore_set: globset::GlobSetBuilder::new().build().unwrap(),
                                pending_creates: HashSet::new(),
                                pending_deletes: HashSet::new(),
                                events_in_window: 0,
                                phase1_armed: false,
                                has_stale_sections: false,
                                phase2_deadline: None,
                                caught_up: true, // skip catch-up for dummy
                            }
                        }
                    }
                });

                // Run startup catch-up on first event for this vault
                if !state.caught_up {
                    state.caught_up = true;
                    // Reload manifest from disk in case Index handler updated it
                    state.reload_manifest();
                    match find_changed_files(&state.vault_root, &state.manifest, &state.ignore_set) {
                        Ok(changed) if !changed.is_empty() => {
                            info!("[watch] catch-up for {}: {} files changed", state.vault_root.display(), changed.len());
                            if let Ok(p1) = phase1_ingest(&changed, &state.vault_root, &mut state.manifest, &state.config) {
                                info!("[watch] catch-up phase1: {} processed, {} new, {} modified",
                                    p1.files_processed, p1.sections_new, p1.sections_modified);
                                let _ = state.manifest.save(&state.hebbs_dir());
                                // Only arm phase2 if there are modified sections (not first-time index).
                                // For fresh vaults, the explicit `hebbs index` command handles phase2.
                                if p1.sections_modified > 0 {
                                    let phase2_ms = state.config.watch.phase2_debounce_ms;
                                    state.phase2_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(phase2_ms));
                                    state.has_stale_sections = true;
                                } else {
                                    info!("[watch] catch-up: all sections new, skipping phase2 (explicit index will handle)");
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(e) => warn!("[watch] catch-up error for {}: {}", state.vault_root.display(), e),
                    }

                    // Also check for stale sections left by interrupted phase2
                    // (only from a genuinely interrupted previous session, not fresh init)
                    if !state.has_stale_sections {
                        let (synced, stale, _) = state.manifest.section_counts();
                        // Only arm if there are BOTH synced and stale sections (partial index).
                        // If synced == 0 and stale > 0, it's a fresh vault -- explicit index handles it.
                        if stale > 0 && synced > 0 {
                            info!(
                                "[watch] catch-up: {} stale sections from interrupted phase2, arming re-embed",
                                stale
                            );
                            let phase2_ms = state.config.watch.phase2_debounce_ms;
                            state.phase2_deadline = Some(
                                tokio::time::Instant::now() + Duration::from_millis(phase2_ms),
                            );
                            state.has_stale_sections = true;
                        } else if stale > 0 {
                            info!("[watch] catch-up: {} stale sections on fresh vault, deferring to explicit index", stale);
                        }
                    }
                }

                // Filter and accumulate events
                for path in &event.event.paths {
                    if !is_relevant_md(path, &state.vault_root, &state.ignore_set) {
                        continue;
                    }
                    state.events_in_window += 1;

                    match event.event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            // On macOS (FSEvents), a Remove event may be followed
                            // by stale Modify events in the same batch. Only treat
                            // as create/modify if the file actually exists.
                            if path.exists() {
                                state.pending_deletes.remove(path);
                                state.pending_creates.insert(path.clone());
                            } else if !state.pending_deletes.contains(path) {
                                // File doesn't exist and isn't already marked for delete
                                state.pending_deletes.insert(path.clone());
                            }
                        }
                        EventKind::Remove(_) => {
                            state.pending_creates.remove(path);
                            state.pending_deletes.insert(path.clone());
                        }
                        _ => {}
                    }
                    state.phase1_armed = true;
                }
            }

            _ = tick.tick() => {
                // Check each vault's timers
                let vault_keys: Vec<PathBuf> = states.keys().cloned().collect();
                for key in vault_keys {
                    let state = match states.get_mut(&key) {
                        Some(s) => s,
                        None => continue,
                    };

                    // Phase 1 check: enough time elapsed since last event?
                    if state.phase1_armed
                        && (!state.pending_creates.is_empty() || !state.pending_deletes.is_empty())
                    {
                        // Reload manifest from disk before modifying to avoid
                        // overwriting updates from the Index command handler.
                        state.reload_manifest();

                        let creates: Vec<PathBuf> = state.pending_creates.drain().collect();
                        let deletes: Vec<PathBuf> = state.pending_deletes.drain().collect();

                        // Phase 1: parse creates/modifies
                        if !creates.is_empty() {
                            match phase1_ingest(&creates, &state.vault_root, &mut state.manifest, &state.config) {
                                Ok(p1) => {
                                    info!(
                                        "[watch:{}] phase1: {} files ({} new, {} modified, {} unchanged)",
                                        state.vault_root.file_name().unwrap_or_default().to_string_lossy(),
                                        p1.files_processed, p1.sections_new, p1.sections_modified, p1.sections_unchanged
                                    );
                                }
                                Err(e) => warn!("[watch] phase1 error: {}", e),
                            }
                        }

                        // Phase 1: handle deletes
                        for path in &deletes {
                            match phase1_delete(path, &state.vault_root, &mut state.manifest) {
                                Ok(n) if n > 0 => {
                                    info!("[watch] deleted {}: {} sections orphaned", path.display(), n);
                                }
                                Ok(_) => {}
                                Err(e) => warn!("[watch] delete error for {}: {}", path.display(), e),
                            }
                        }

                        // Save manifest after phase 1
                        if let Err(e) = state.manifest.save(&state.hebbs_dir()) {
                            warn!("[watch] failed to save manifest: {}", e);
                        }

                        // Arm phase 2
                        let is_burst = state.events_in_window > state.config.watch.burst_threshold;
                        let delay = if is_burst {
                            info!("[watch] burst detected ({} events)", state.events_in_window);
                            Duration::from_millis(state.config.watch.burst_debounce_ms)
                        } else {
                            Duration::from_millis(state.config.watch.phase2_debounce_ms)
                        };
                        state.phase2_deadline = Some(tokio::time::Instant::now() + delay);
                        state.has_stale_sections = true;
                        state.events_in_window = 0;
                        state.phase1_armed = false;
                    }

                    // Phase 2 check: deadline passed?
                    if state.has_stale_sections {
                        if let Some(deadline) = state.phase2_deadline {
                            if tokio::time::Instant::now() >= deadline {
                                info!("[watch:{}] phase2: starting embed + index",
                                    state.vault_root.file_name().unwrap_or_default().to_string_lossy());

                                // Reload manifest from disk before phase2 to avoid
                                // overwriting updates from the Index command handler.
                                state.reload_manifest();

                                // Need engine + embedder from vault manager
                                let pair = vault_manager.lock().await
                                    .get_or_open(&state.vault_root);
                                match pair {
                                    Ok((engine, embedder_ref)) => {
                                        match phase2_ingest(
                                            &state.vault_root,
                                            &mut state.manifest,
                                            &engine,
                                            &embedder_ref,
                                            &state.config,
                                        ).await {
                                            Ok(p2) => {
                                                info!(
                                                    "[watch:{}] phase2: {} embedded, {} remembered, {} revised, {} forgotten",
                                                    state.vault_root.file_name().unwrap_or_default().to_string_lossy(),
                                                    p2.sections_embedded, p2.sections_remembered,
                                                    p2.sections_revised, p2.sections_forgotten
                                                );
                                                // Only save manifest after successful phase2
                                                if let Err(e) = state.manifest.save(&state.hebbs_dir()) {
                                                    warn!("[watch] failed to save manifest after phase2: {}", e);
                                                }
                                                // Run auto-forget after phase2 to clean up decayed memories
                                                match engine.auto_forget() {
                                                    Ok(fg) if fg.forgotten_count > 0 => {
                                                        info!(
                                                            "[watch:{}] auto-forget: {} forgotten, {} cascaded",
                                                            state.vault_root.file_name().unwrap_or_default().to_string_lossy(),
                                                            fg.forgotten_count, fg.cascade_count
                                                        );
                                                    }
                                                    Ok(_) => {} // nothing to forget
                                                    Err(e) => warn!("[watch] auto-forget error: {}", e),
                                                }
                                                // Notify panel WebSocket clients.
                                                let _ = panel_event_tx.send(PanelEvent::IngestComplete {
                                                    remembered: p2.sections_remembered,
                                                    revised: p2.sections_revised,
                                                });
                                            }
                                            Err(e) => {
                                                warn!("[watch] phase2 error: {}", e);
                                                // Do NOT save manifest on failure to avoid
                                                // overwriting good state with stale data.
                                            }
                                        }
                                    }
                                    Err(e) => warn!("[watch] failed to open vault for phase2: {}", e),
                                }

                                state.phase2_deadline = None;
                                state.has_stale_sections = false;
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn require_vault_path(path: &Option<PathBuf>) -> Result<PathBuf, DaemonResponse> {
    path.clone()
        .ok_or_else(|| DaemonResponse::err("vault_path is required for this command"))
}

/// Read the vaults.json registry and return all registered vault paths.
fn read_vaults_json(runtime_dir: &Path) -> Vec<PathBuf> {
    let registry_path = runtime_dir.join("vaults.json");
    let content = match std::fs::read_to_string(&registry_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    json.get("vaults")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    entry
                        .get("path")
                        .and_then(|p| p.as_str())
                        .map(PathBuf::from)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cleanup_stale(socket_path: &Path, pid_path: &Path) -> Result<(), String> {
    if pid_path.exists() {
        let pid_str = std::fs::read_to_string(pid_path)
            .map_err(|e| format!("failed to read PID file: {}", e))?;
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            // Check if the process is still alive
            #[cfg(unix)]
            {
                let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
                if alive {
                    return Err(format!(
                        "daemon already running (PID {}). Stop it first or delete {}",
                        pid,
                        pid_path.display()
                    ));
                }
            }
        }
        std::fs::remove_file(pid_path).ok();
    }
    if socket_path.exists() {
        std::fs::remove_file(socket_path).ok();
    }
    Ok(())
}

fn cleanup_files(socket_path: &Path, pid_path: &Path) {
    std::fs::remove_file(socket_path).ok();
    std::fs::remove_file(pid_path).ok();
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
        "invalid memory ID '{}': expected 26-char ULID or 32-char hex",
        trimmed
    ))
}

fn memory_to_json(m: &Memory) -> serde_json::Value {
    let id = format_memory_id(&m.memory_id);
    let context: Option<serde_json::Value> = if m.context_bytes.is_empty() {
        None
    } else {
        serde_json::from_slice(&m.context_bytes).ok()
    };

    serde_json::json!({
        "memory_id": id,
        "content": m.content,
        "importance": m.importance,
        "entity_id": m.entity_id,
        "context": context,
        "created_at_us": m.created_at,
        "last_accessed_at_us": m.last_accessed_at,
        "access_count": m.access_count,
    })
}

fn parse_context_json(
    json_str: Option<&str>,
) -> Result<Option<HashMap<String, serde_json::Value>>, String> {
    match json_str {
        Some(s) => serde_json::from_str(s)
            .map(Some)
            .map_err(|e| format!("invalid context JSON: {}", e)),
        None => Ok(None),
    }
}

fn parse_edge_specs(edges: &[EdgeSpec]) -> Result<Vec<RememberEdge>, String> {
    edges
        .iter()
        .map(|spec| {
            let target_bytes = parse_memory_id(&spec.target_id)?;
            let mut target = [0u8; 16];
            if target_bytes.len() != 16 {
                return Err(format!(
                    "edge target must be 16 bytes, got {}",
                    target_bytes.len()
                ));
            }
            target.copy_from_slice(&target_bytes);

            let edge_type = match spec.edge_type.as_str() {
                "caused_by" => EdgeType::CausedBy,
                "related_to" => EdgeType::RelatedTo,
                "followed_by" => EdgeType::FollowedBy,
                "revised_from" => EdgeType::RevisedFrom,
                "insight_from" => EdgeType::InsightFrom,
                "contradicts" => EdgeType::Contradicts,
                other => {
                    return Err(format!(
                        "unknown edge type '{}': valid types are caused_by, related_to, followed_by, revised_from, insight_from, contradicts",
                        other
                    ))
                }
            };

            Ok(RememberEdge {
                target_id: target,
                edge_type,
                confidence: spec.confidence,
            })
        })
        .collect()
}

fn parse_scoring_weights(s: &str) -> Result<ScoringWeights, String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 4 {
        return Err(format!(
            "weights must be 4 colon-separated floats (R:T:I:F), got {} parts",
            parts.len()
        ));
    }
    let parse = |part: &str, name: &str| -> Result<f32, String> {
        part.parse::<f32>()
            .map_err(|_| format!("invalid {} weight '{}': must be a number", name, part))
    };
    Ok(ScoringWeights {
        w_relevance: parse(parts[0], "relevance")?,
        w_recency: parse(parts[1], "recency")?,
        w_importance: parse(parts[2], "importance")?,
        w_reinforcement: parse(parts[3], "reinforcement")?,
        ..ScoringWeights::default()
    })
}

fn parse_produced_insights_json(
    json_str: &str,
) -> Result<Vec<hebbs_reflect::ProducedInsight>, String> {
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(json_str).map_err(|e| format!("invalid JSON for insights: {}", e))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_vaults_json_valid() {
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "vaults": [
                {"path": "/home/user/notes", "label": "notes"},
                {"path": "/home/user/work", "label": "work"}
            ]
        });
        std::fs::write(dir.path().join("vaults.json"), json.to_string()).unwrap();

        let paths = read_vaults_json(dir.path());
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/home/user/notes"));
        assert_eq!(paths[1], PathBuf::from("/home/user/work"));
    }

    #[test]
    fn test_read_vaults_json_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = read_vaults_json(dir.path());
        assert!(paths.is_empty());
    }

    #[test]
    fn test_read_vaults_json_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("vaults.json"), "not json").unwrap();
        let paths = read_vaults_json(dir.path());
        assert!(paths.is_empty());
    }

    #[test]
    fn test_read_vaults_json_empty_vaults() {
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({ "vaults": [] });
        std::fs::write(dir.path().join("vaults.json"), json.to_string()).unwrap();
        let paths = read_vaults_json(dir.path());
        assert!(paths.is_empty());
    }

    #[test]
    fn test_read_vaults_json_missing_path_field() {
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "vaults": [
                {"label": "no-path"},
                {"path": "/valid/path", "label": "has-path"}
            ]
        });
        std::fs::write(dir.path().join("vaults.json"), json.to_string()).unwrap();
        let paths = read_vaults_json(dir.path());
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/valid/path"));
    }

    #[test]
    fn test_read_vaults_json_no_vaults_key() {
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({ "other": "data" });
        std::fs::write(dir.path().join("vaults.json"), json.to_string()).unwrap();
        let paths = read_vaults_json(dir.path());
        assert!(paths.is_empty());
    }
}
