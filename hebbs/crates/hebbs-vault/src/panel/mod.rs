//! Memory Palace Control Panel: embedded web UI for visualizing the HEBBS memory graph.
//!
//! Starts an axum HTTP server on 127.0.0.1 serving a single-page application
//! with API endpoints for graph data, memory details, and vault management.

pub mod routes;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::State;
use axum::middleware::{self, Next};
use axum::Router;
use tracing::info;

use tokio::sync::broadcast;
use tokio::sync::Mutex as TokioMutex;

use hebbs_core::engine::Engine;
use hebbs_embed::Embedder;

use crate::daemon::vault_manager::VaultManager;

/// Real-time event pushed to connected panel WebSocket clients.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PanelEvent {
    /// A new memory was created.
    MemoryCreated { id: String },
    /// A memory was forgotten (deleted).
    MemoryForgotten { id: String },
    /// An ingest cycle completed with counts of remembered and revised sections.
    IngestComplete { remembered: usize, revised: usize },
    /// Vault configuration was reloaded from disk.
    ConfigReloaded,
}

/// Cached UMAP projection result for the graph view.
pub struct ProjectionCache {
    /// memory_id hex string -> (x, y) position
    pub positions: HashMap<String, (f32, f32)>,
    /// memory_id hex string -> cluster label (-1 = noise)
    pub clusters: HashMap<String, i32>,
    /// cluster_id -> human-readable label (term-frequency based)
    pub cluster_labels: HashMap<i32, String>,
    /// Number of clusters
    pub n_clusters: usize,
    /// Node count at time of computation (invalidation check)
    pub node_count: usize,
    /// memory_id hex string -> pinned (x, y) from user drag (persisted)
    pub pinned: HashMap<String, (f32, f32)>,
}

/// Shared state for all panel API handlers.
pub struct PanelState {
    pub engine: Arc<Engine>,
    pub embedder: Arc<dyn Embedder>,
    pub vault_root: PathBuf,
    /// Cached UMAP projection, recomputed when node count changes.
    pub projection_cache: Mutex<Option<ProjectionCache>>,
    /// Vault manager reference for vault switching (daemon mode only).
    pub vault_manager: Option<Arc<TokioMutex<VaultManager>>>,
    /// Shared idle timer from the daemon. Updated on each HTTP request
    /// so that panel activity prevents idle shutdown.
    pub last_request: Option<Arc<TokioMutex<Instant>>>,
    /// Notify handle for signaling the daemon to reload config from disk.
    pub config_notify: Option<Arc<tokio::sync::Notify>>,
    /// Broadcast sender for real-time panel events (WebSocket push).
    pub event_tx: broadcast::Sender<PanelEvent>,
}

/// Start the panel HTTP server and return the bound address.
///
/// Does NOT open the browser; the caller handles that.
pub async fn start_panel_server(
    engine: Engine,
    embedder: Arc<dyn Embedder>,
    vault_root: PathBuf,
    port: u16,
) -> Result<SocketAddr, String> {
    let (event_tx, _rx) = broadcast::channel(256);
    let state = Arc::new(PanelState {
        engine: Arc::new(engine),
        embedder,
        vault_root,
        projection_cache: Mutex::new(None),
        vault_manager: None,
        last_request: None,
        config_notify: None,
        event_tx,
    });

    bind_and_serve(state, port).await
}

/// Start the panel HTTP server from within the daemon process.
///
/// Uses a pre-opened engine from the VaultManager. The vault_manager reference
/// enables vault switching via the panel's API.
pub async fn start_panel_server_from_daemon(
    engine: Arc<Engine>,
    embedder: Arc<dyn Embedder>,
    vault_root: PathBuf,
    port: u16,
    vault_manager: Arc<TokioMutex<VaultManager>>,
    last_request: Arc<TokioMutex<Instant>>,
    config_notify: Arc<tokio::sync::Notify>,
    event_tx: broadcast::Sender<PanelEvent>,
) -> Result<SocketAddr, String> {
    let state = Arc::new(PanelState {
        engine,
        embedder,
        vault_root,
        projection_cache: Mutex::new(None),
        vault_manager: Some(vault_manager),
        last_request: Some(last_request),
        config_notify: Some(config_notify),
        event_tx,
    });

    bind_and_serve(state, port).await
}

/// Middleware that resets the daemon's idle timer on each HTTP request.
async fn touch_idle_timer(
    State(state): State<Arc<PanelState>>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> axum::response::Response {
    if let Some(ref last_req) = state.last_request {
        *last_req.lock().await = Instant::now();
    }
    next.run(request).await
}

/// Bind the panel HTTP server and spawn the axum serve task.
async fn bind_and_serve(state: Arc<PanelState>, port: u16) -> Result<SocketAddr, String> {
    let app = Router::new()
        .merge(routes::static_routes())
        .merge(routes::api_routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            touch_idle_timer,
        ))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("failed to bind {}: {}", addr, e))?;
    let bound_addr = listener
        .local_addr()
        .map_err(|e| format!("failed to get local addr: {}", e))?;

    info!("panel server listening on http://{}", bound_addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    Ok(bound_addr)
}
