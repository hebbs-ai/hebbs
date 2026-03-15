//! Axum route handlers for the Memory Palace Control Panel.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post, put};
use axum::Router;
use serde::{Deserialize, Serialize};
use tracing::debug;

use hebbs_core::forget::{ForgetCriteria, Tombstone, tombstone_prefix};
use hebbs_core::memory::MemoryKind;
use hebbs_core::recall::{RecallInput, RecallStrategy, ScoringWeights};
use hebbs_index::graph::EdgeType;
use hebbs_storage::ColumnFamilyName;

use super::PanelState;
use crate::config::VaultConfig;
use crate::manifest::{Manifest, SectionState};

type AppState = Arc<PanelState>;

// ═══════════════════════════════════════════════════════════════════════
//  Static asset serving (embedded via include_str!)
// ═══════════════════════════════════════════════════════════════════════

const INDEX_HTML: &str = include_str!("static/index.html");
const APP_JS: &str = include_str!("static/app.js");
const GRAPH_JS: &str = include_str!("static/graph.js");
const PANEL_CSS: &str = include_str!("static/panel.css");

pub fn static_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(serve_index))
        .route("/static/app.js", get(serve_app_js))
        .route("/static/graph.js", get(serve_graph_js))
        .route("/static/panel.css", get(serve_panel_css))
}

async fn serve_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn serve_app_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        APP_JS,
    )
}

async fn serve_graph_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        GRAPH_JS,
    )
}

async fn serve_panel_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css")], PANEL_CSS)
}

// ═══════════════════════════════════════════════════════════════════════
//  API routes
// ═══════════════════════════════════════════════════════════════════════

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/api/panel/vaults", get(list_vaults))
        .route("/api/panel/status", get(vault_status))
        .route("/api/panel/graph", get(graph_data))
        .route("/api/panel/memories/:id", get(memory_detail))
        .route("/api/panel/recall", post(recall_search))
        .route("/api/panel/health", get(health_detail))
        .route("/api/panel/health/actions", post(health_action))
        .route("/api/panel/timeline", get(timeline_data))
        .route("/api/panel/timeline/snapshot", get(timeline_snapshot))
        .route("/api/panel/timeline/forgotten", get(forgotten_timeline))
        .route("/api/panel/config", get(get_config).put(update_config))
        .route("/api/panel/config/reset", post(reset_config))
        .route("/api/panel/config/export", get(export_config))
        .route("/api/panel/dashboard", get(dashboard_data))
        .route("/api/panel/memories", get(list_memories))
        .route("/api/panel/positions/:id", put(pin_position))
        .route("/api/panel/positions/:id/unpin", post(unpin_position))
        .route("/api/panel/ws", get(ws_handler))
}

// ── Vault listing ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct VaultEntry {
    path: String,
    label: String,
    active: bool,
}

async fn list_vaults(State(state): State<AppState>) -> Json<Vec<VaultEntry>> {
    let mut vaults = Vec::new();

    // Read ~/.hebbs/vaults.json
    if let Some(home) = dirs::home_dir() {
        let registry_path = home.join(".hebbs").join("vaults.json");
        if let Ok(content) = std::fs::read_to_string(&registry_path) {
            if let Ok(registry) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(arr) = registry.get("vaults").and_then(|v| v.as_array()) {
                    let active_path = state.vault_root.canonicalize().ok();
                    for entry in arr {
                        let path = entry
                            .get("path")
                            .and_then(|p| p.as_str())
                            .unwrap_or("")
                            .to_string();
                        let label = entry
                            .get("label")
                            .and_then(|l| l.as_str())
                            .unwrap_or("")
                            .to_string();
                        let entry_canonical =
                            std::path::Path::new(&path).canonicalize().ok();
                        let active = match (&active_path, &entry_canonical) {
                            (Some(a), Some(e)) => a == e,
                            _ => false,
                        };
                        vaults.push(VaultEntry {
                            path,
                            label,
                            active,
                        });
                    }
                }
            }
        }
    }

    Json(vaults)
}

// ── Vault status ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    vault_path: String,
    memory_count: usize,
    insight_count: usize,
    file_count: usize,
    section_count: usize,
    synced: usize,
    stale: usize,
    orphaned: usize,
    sync_percentage: f32,
}

async fn vault_status(State(state): State<AppState>) -> Result<Json<StatusResponse>, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let manifest = Manifest::load(&hebbs_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (synced, stale, orphaned) = manifest.section_counts();
    let total = synced + stale + orphaned;
    let sync_pct = if total > 0 {
        synced as f32 / total as f32 * 100.0
    } else {
        100.0
    };

    // Count insights vs episodes by checking engine
    let mut memory_count = 0usize;
    let mut insight_count = 0usize;
    for file_entry in manifest.files.values() {
        for section in &file_entry.sections {
            if section.state == SectionState::Orphaned {
                continue;
            }
            if let Ok(id_bytes) = parse_memory_id(&section.memory_id) {
                if let Ok(mem) = state.engine.get(&id_bytes) {
                    match mem.kind {
                        MemoryKind::Insight => insight_count += 1,
                        _ => memory_count += 1,
                    }
                } else {
                    memory_count += 1;
                }
            } else {
                memory_count += 1;
            }
        }
    }

    Ok(Json(StatusResponse {
        vault_path: state.vault_root.display().to_string(),
        memory_count,
        insight_count,
        file_count: manifest.files.len(),
        section_count: total,
        synced,
        stale,
        orphaned,
        sync_percentage: sync_pct,
    }))
}

// ── Graph data ─────────────────────────────────────────────────────────

#[derive(Serialize)]
struct GraphResponse {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    has_projection: bool,
    n_clusters: usize,
    /// cluster_id -> human-readable label
    cluster_labels: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
struct GraphNode {
    id: String,
    label: String,
    file_path: String,
    heading_path: Vec<String>,
    kind: String,
    importance: f32,
    recency: f32,
    reinforcement: f32,
    decay_score: f32,
    access_count: u64,
    state: String,
    created_at: u64,
    content_preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cluster: Option<i32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pinned: bool,
}

#[derive(Serialize)]
struct GraphEdge {
    source: String,
    target: String,
    #[serde(rename = "type")]
    edge_type: String,
    weight: f32,
}

async fn graph_data(State(state): State<AppState>) -> Result<Json<GraphResponse>, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let manifest = Manifest::load(&hebbs_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    let max_age_us: u64 = 30 * 24 * 3600 * 1_000_000; // 30 days

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut id_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut embeddings: Vec<(String, Vec<f32>)> = Vec::new();

    // Collect nodes from manifest
    for (file_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            if section.state == SectionState::Orphaned {
                continue;
            }

            let mem_id = section.memory_id.clone();
            if !id_set.insert(mem_id.clone()) {
                continue;
            }

            // Try to get memory from engine for rich metadata
            let id_bytes = match parse_memory_id(&mem_id) {
                Ok(b) => b,
                Err(_) => continue,
            };

            let (kind, importance, decay_score, access_count, created_at, content_preview, embedding, confidence) =
                match state.engine.get(&id_bytes) {
                    Ok(mem) => {
                        let k = match mem.kind {
                            MemoryKind::Episode => "episode",
                            MemoryKind::Insight => "insight",
                            MemoryKind::Revision => "revision",
                        };
                        let preview = if mem.content.len() > 200 {
                            format!("{}...", &mem.content[..200])
                        } else {
                            mem.content.clone()
                        };
                        // Extract confidence from context for insights
                        let conf = if mem.kind == MemoryKind::Insight {
                            mem.context().ok().and_then(|ctx| {
                                ctx.get("hebbs-confidence")
                                    .and_then(|v| v.as_f64())
                                    .map(|f| f as f32)
                            })
                        } else {
                            None
                        };
                        (
                            k,
                            mem.importance,
                            mem.decay_score,
                            mem.access_count,
                            mem.created_at,
                            preview,
                            mem.embedding.clone(),
                            conf,
                        )
                    }
                    Err(_) => {
                        // Memory not in engine (maybe not yet embedded)
                        ("episode", 0.5, 1.0, 0u64, 0u64, String::new(), None, None)
                    }
                };

            // Compute recency signal [0, 1]
            let age_us = now_us.saturating_sub(created_at);
            let recency = 1.0 - (age_us as f32 / max_age_us as f32).min(1.0);

            // Compute reinforcement signal [0, 1]
            let reinforcement = (1.0 + access_count as f32).ln() / (1.0 + 100.0_f32).ln();

            let label = if !section.heading_path.is_empty() {
                section.heading_path.last().cloned().unwrap_or_default()
            } else {
                file_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(file_path)
                    .trim_end_matches(".md")
                    .to_string()
            };

            let state_str = match section.state {
                SectionState::Synced => "synced",
                SectionState::ContentStale => "stale",
                SectionState::Orphaned => "orphaned",
            };

            if let Some(emb) = embedding {
                embeddings.push((mem_id.clone(), emb));
            }

            nodes.push(GraphNode {
                id: mem_id,
                label,
                file_path: file_path.clone(),
                heading_path: section.heading_path.clone(),
                kind: kind.to_string(),
                importance,
                recency,
                reinforcement: reinforcement.min(1.0),
                decay_score,
                access_count,
                state: state_str.to_string(),
                created_at,
                content_preview,
                confidence,
                x: None,
                y: None,
                cluster: None,
                pinned: false,
            });
        }
    }

    // Compute similarity edges using HNSW
    // For each node with an embedding, find top-3 nearest neighbors
    let index_manager = state.engine.index_manager();
    for (mem_id, embedding) in &embeddings {
        match index_manager.search_vector(embedding, 4, None) {
            Ok(neighbors) => {
                for (neighbor_id, distance) in neighbors {
                    let neighbor_hex = bytes_to_ulid_string(&neighbor_id);
                    // Skip self
                    if neighbor_hex == *mem_id {
                        continue;
                    }
                    // Only include if the neighbor is in our node set
                    if !id_set.contains(&neighbor_hex) {
                        continue;
                    }
                    let similarity = 1.0 - distance.min(2.0) / 2.0;
                    if similarity > 0.5 {
                        edges.push(GraphEdge {
                            source: mem_id.clone(),
                            target: neighbor_hex,
                            edge_type: "similarity".to_string(),
                            weight: similarity,
                        });
                    }
                }
            }
            Err(_) => continue,
        }
    }

    // Collect graph-CF edges (wiki-links, insight-from, etc.)
    for (mem_id, _) in &embeddings {
        let id_bytes = match parse_memory_id(mem_id) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let id_16: [u8; 16] = match id_bytes.as_slice().try_into() {
            Ok(a) => a,
            Err(_) => continue,
        };
        if let Ok(out_edges) = index_manager.outgoing_edges(&id_16) {
            for (edge_type, target_id, metadata) in out_edges {
                let target_hex = bytes_to_ulid_string(&target_id);
                if !id_set.contains(&target_hex) {
                    continue;
                }
                let type_str = edge_type_str(&edge_type);
                edges.push(GraphEdge {
                    source: mem_id.clone(),
                    target: target_hex,
                    edge_type: type_str.to_string(),
                    weight: metadata.confidence,
                });
            }
        }
    }

    // Deduplicate edges (A->B and B->A become one)
    let mut seen_edges: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    edges.retain(|e| {
        let key = if e.source < e.target {
            (e.source.clone(), e.target.clone())
        } else {
            (e.target.clone(), e.source.clone())
        };
        seen_edges.insert(key)
    });

    // Compute or retrieve cached UMAP projection
    let node_count = nodes.len();
    let mut has_projection = false;
    let mut n_clusters = 0_usize;
    let mut cluster_labels = std::collections::HashMap::new();

    if node_count >= 3 {
        // Check cache validity
        let needs_recompute = {
            let cache = state.projection_cache.lock().unwrap_or_else(|e| e.into_inner());
            match &*cache {
                Some(c) if c.node_count == node_count => false,
                _ => true,
            }
        };

        if needs_recompute {
            let index_manager = state.engine.index_manager();
            let k = 15.min(node_count.saturating_sub(1)).max(1);
            let snapshot = index_manager.extract_neighborhood_snapshot(k);

            if !snapshot.ids.is_empty() {
                let params = hebbs_index::ProjectionParams {
                    n_neighbors: k,
                    ..Default::default()
                };
                let proj = hebbs_index::neighborhood::compute_projection(&snapshot, &params);

                // Build lookup maps keyed by hex memory ID
                let mut positions = std::collections::HashMap::with_capacity(snapshot.ids.len());
                let mut clusters = std::collections::HashMap::with_capacity(snapshot.ids.len());

                for (i, id_bytes) in snapshot.ids.iter().enumerate() {
                    let hex_id = bytes_to_ulid_string(id_bytes);
                    positions.insert(hex_id.clone(), proj.positions[i]);
                    clusters.insert(hex_id, proj.clusters[i]);
                }

                let nc = proj.n_clusters;

                // Load pinned positions from storage
                let pinned = load_pinned_positions(state.engine.storage());

                // Compute cluster labels: try LLM first, fall back to TF heuristic.
                let labels = compute_cluster_labels_llm(&nodes, &clusters, &state.vault_root)
                    .unwrap_or_else(|| compute_cluster_labels(&nodes, &clusters));

                let mut cache = state.projection_cache.lock().unwrap_or_else(|e| e.into_inner());
                *cache = Some(super::ProjectionCache {
                    positions,
                    clusters,
                    cluster_labels: labels,
                    n_clusters: nc,
                    node_count,
                    pinned,
                });
            }
        }

        // Apply cached projection to nodes
        let cache = state.projection_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref proj) = *cache {
            has_projection = true;
            n_clusters = proj.n_clusters;
            for node in &mut nodes {
                // Pinned positions override UMAP positions
                if let Some(&(x, y)) = proj.pinned.get(&node.id) {
                    node.x = Some(x);
                    node.y = Some(y);
                    node.pinned = true;
                } else if let Some(&(x, y)) = proj.positions.get(&node.id) {
                    node.x = Some(x);
                    node.y = Some(y);
                }
                if let Some(&c) = proj.clusters.get(&node.id) {
                    node.cluster = Some(c);
                }
            }
            // Convert cluster labels for JSON (i32 key -> string key)
            for (&cid, label) in &proj.cluster_labels {
                cluster_labels.insert(cid.to_string(), label.clone());
            }
        }
    }

    Ok(Json(GraphResponse {
        nodes,
        edges,
        has_projection,
        n_clusters,
        cluster_labels,
    }))
}

// ── Position persistence ───────────────────────────────────────────────

/// Storage key prefix for pinned positions in the Meta column family.
const PIN_PREFIX: &[u8] = b"panel_pin:";

/// Load all pinned positions from Meta CF.
/// Key format: `panel_pin:{memory_id_hex}` -> 8 bytes (f32 x, f32 y).
fn load_pinned_positions(storage: &dyn hebbs_storage::StorageBackend) -> HashMap<String, (f32, f32)> {
    let mut pinned = HashMap::new();
    if let Ok(iter) = storage.prefix_iterator(ColumnFamilyName::Meta, PIN_PREFIX) {
        for (key, value) in iter {
            if value.len() == 8 {
                let id = String::from_utf8_lossy(&key[PIN_PREFIX.len()..]).to_string();
                let x = f32::from_le_bytes([value[0], value[1], value[2], value[3]]);
                let y = f32::from_le_bytes([value[4], value[5], value[6], value[7]]);
                pinned.insert(id, (x, y));
            }
        }
    }
    pinned
}

/// Save a pinned position to Meta CF.
fn save_pinned_position(
    storage: &dyn hebbs_storage::StorageBackend,
    memory_id: &str,
    x: f32,
    y: f32,
) -> Result<(), StatusCode> {
    let mut key = PIN_PREFIX.to_vec();
    key.extend_from_slice(memory_id.as_bytes());
    let mut value = Vec::with_capacity(8);
    value.extend_from_slice(&x.to_le_bytes());
    value.extend_from_slice(&y.to_le_bytes());
    storage
        .put(ColumnFamilyName::Meta, &key, &value)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Delete a pinned position from Meta CF.
fn delete_pinned_position(
    storage: &dyn hebbs_storage::StorageBackend,
    memory_id: &str,
) -> Result<(), StatusCode> {
    let mut key = PIN_PREFIX.to_vec();
    key.extend_from_slice(memory_id.as_bytes());
    storage
        .delete(ColumnFamilyName::Meta, &key)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
struct PinPositionRequest {
    x: f32,
    y: f32,
}

/// Pin a node position (after user drag).
async fn pin_position(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PinPositionRequest>,
) -> Result<StatusCode, StatusCode> {
    save_pinned_position(state.engine.storage(), &id, body.x, body.y)?;
    // Update in-memory cache
    let mut cache = state.projection_cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref mut proj) = *cache {
        proj.pinned.insert(id, (body.x, body.y));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Unpin a node position (let UMAP recompute it).
async fn unpin_position(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    delete_pinned_position(state.engine.storage(), &id)?;
    // Update in-memory cache
    let mut cache = state.projection_cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref mut proj) = *cache {
        proj.pinned.remove(&id);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── Cluster labeling ──────────────────────────────────────────────────

/// Stop words to exclude from cluster labels.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "of", "in", "on", "to", "for", "with",
    "is", "it", "at", "by", "from", "as", "be", "was", "are", "has", "had",
    "not", "but", "if", "so", "no", "do", "my", "we", "up", "how", "what",
];

/// Attempt to generate cluster labels using a configured LLM provider.
///
/// Loads the vault config, checks for a configured reflect LLM provider,
/// and sends a single request asking for 2-4 word labels for each cluster.
/// Returns `None` on any failure (no config, network error, parse error)
/// so the caller can fall back to TF-based labels. O(c * k) where c = cluster
/// count and k = max nodes sampled per cluster (bounded at 10).
fn compute_cluster_labels_llm(
    nodes: &[GraphNode],
    clusters: &HashMap<String, i32>,
    vault_root: &std::path::Path,
) -> Option<HashMap<i32, String>> {
    let hebbs_dir = vault_root.join(".hebbs");
    let vault_config = crate::config::VaultConfig::load(&hebbs_dir).ok()?;
    let llm_cfg = &vault_config.reflect_llm;

    if !llm_cfg.is_configured() {
        return None;
    }

    let provider_name = llm_cfg.provider.as_deref()?;
    let model = llm_cfg.model.as_deref()?;

    // Ollama does not require an API key; all cloud providers do.
    let provider_type = hebbs_reflect::ProviderType::from_name(provider_name);
    let needs_key = !matches!(provider_type, hebbs_reflect::ProviderType::Ollama);
    let api_key = llm_cfg.resolved_api_key();

    if needs_key && api_key.is_none() {
        return None;
    }

    let config = hebbs_reflect::LlmProviderConfig {
        provider_type,
        api_key,
        base_url: llm_cfg.base_url.clone(),
        model: model.to_string(),
        timeout_secs: 30,
        max_retries: 1,
        retry_backoff_ms: 500,
    };

    let provider = hebbs_reflect::create_provider(&config).ok()?;

    // Group node labels by cluster (max 10 per cluster to bound prompt size).
    let mut cluster_headings: HashMap<i32, Vec<&str>> = HashMap::new();
    for node in nodes {
        if let Some(&c) = clusters.get(&node.id) {
            if c >= 0 {
                let entry = cluster_headings.entry(c).or_default();
                if entry.len() < 10 {
                    entry.push(&node.label);
                }
            }
        }
    }

    if cluster_headings.is_empty() {
        return None;
    }

    // Build prompt listing each cluster's headings.
    let mut cluster_list = String::new();
    let mut cluster_ids: Vec<i32> = cluster_headings.keys().copied().collect();
    cluster_ids.sort_unstable();

    for &cid in &cluster_ids {
        if let Some(headings) = cluster_headings.get(&cid) {
            cluster_list.push_str(&format!("Cluster {}:\n", cid));
            for h in headings {
                let truncated = if h.len() > 80 { &h[..80] } else { h };
                cluster_list.push_str(&format!("  - {}\n", truncated));
            }
        }
    }

    let system_message = "You are a labeling assistant. Given groups of document headings \
        organized into clusters, generate a short 2-4 word label for each cluster that \
        captures its theme. Respond with valid JSON only: an object mapping cluster ID \
        (as string key) to the label string. Example: {\"0\": \"Sales Objections\", \"1\": \"API Design\"}. \
        No markdown, no explanation, just the JSON object."
        .to_string();

    let user_message = format!(
        "Generate a 2-4 word label for each cluster:\n\n{}",
        cluster_list
    );

    let request = hebbs_reflect::LlmRequest {
        system_message,
        user_message,
        max_tokens: 512,
        temperature: 0.2,
        response_format: hebbs_reflect::ResponseFormat::Json,
        metadata: HashMap::new(),
    };

    let response = provider.complete(request).ok()?;

    // Parse JSON response: {"0": "Label", "1": "Label", ...}
    let parsed: serde_json::Value = serde_json::from_str(&response.content).ok()?;
    let obj = parsed.as_object()?;

    let mut labels = HashMap::with_capacity(obj.len());
    for (key, value) in obj {
        if let Ok(cluster_id) = key.parse::<i32>() {
            if let Some(label) = value.as_str() {
                if !label.is_empty() {
                    labels.insert(cluster_id, label.to_string());
                }
            }
        }
    }

    // Only return if we got labels for at least half the clusters.
    if labels.len() >= cluster_ids.len() / 2 {
        Some(labels)
    } else {
        None
    }
}

/// Compute a human-readable label for each cluster from node headings.
///
/// Extracts the most frequent meaningful words from headings within each
/// cluster, picks the top 2-3 terms as the label. O(n) where n = node count.
fn compute_cluster_labels(
    nodes: &[GraphNode],
    clusters: &HashMap<String, i32>,
) -> HashMap<i32, String> {
    let mut cluster_headings: HashMap<i32, Vec<&str>> = HashMap::new();
    for node in nodes {
        if let Some(&c) = clusters.get(&node.id) {
            if c >= 0 {
                cluster_headings.entry(c).or_default().push(&node.label);
            }
        }
    }

    let stop_set: std::collections::HashSet<&str> = STOP_WORDS.iter().copied().collect();

    let mut labels = HashMap::new();
    for (cluster_id, headings) in &cluster_headings {
        let mut word_counts: HashMap<String, usize> = HashMap::new();
        for heading in headings {
            for word in heading.split_whitespace() {
                let clean: String = word.chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                    .collect::<String>()
                    .to_lowercase();
                if clean.len() >= 2 && !stop_set.contains(clean.as_str()) {
                    *word_counts.entry(clean).or_insert(0) += 1;
                }
            }
        }

        let mut ranked: Vec<(String, usize)> = word_counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let top_n = if ranked.len() >= 3 && ranked[2].1 == ranked[1].1 { 3 } else { 2.min(ranked.len()) };
        let label_words: Vec<String> = ranked.into_iter()
            .take(top_n)
            .map(|(w, _)| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().to_string() + c.as_str(),
                }
            })
            .collect();

        if !label_words.is_empty() {
            labels.insert(*cluster_id, label_words.join(" & "));
        }
    }

    labels
}

// ── Memory detail ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct MemoryDetailResponse {
    memory_id: String,
    content: String,
    file_path: Option<String>,
    heading_path: Vec<String>,
    kind: String,
    importance: f32,
    created_at: u64,
    updated_at: u64,
    last_accessed_at: u64,
    access_count: u64,
    decay_score: f32,
    state: Option<String>,
    scores: ScoreBreakdown,
    edges: Vec<EdgeInfo>,
    neighbors: Vec<NeighborInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_ids: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ScoreBreakdown {
    recency: SignalScore,
    importance: SignalScore,
    reinforcement: SignalScore,
}

#[derive(Serialize)]
struct SignalScore {
    raw: f32,
    weight: f32,
    weighted: f32,
}

#[derive(Serialize)]
struct EdgeInfo {
    target_id: String,
    #[serde(rename = "type")]
    edge_type: String,
    confidence: f32,
}

#[derive(Serialize)]
struct NeighborInfo {
    id: String,
    similarity: f32,
    label: String,
}

async fn memory_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MemoryDetailResponse>, StatusCode> {
    let id_bytes = parse_memory_id(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let mem = state
        .engine
        .get(&id_bytes)
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let now_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    let max_age_us: u64 = 30 * 24 * 3600 * 1_000_000;

    let age_us = now_us.saturating_sub(mem.created_at);
    let recency_raw = 1.0 - (age_us as f32 / max_age_us as f32).min(1.0);
    let reinforcement_raw =
        ((1.0 + mem.access_count as f32).ln() / (1.0 + 100.0_f32).ln()).min(1.0);

    // Default scoring weights
    let w_recency = 0.2;
    let w_importance = 0.2;
    let w_reinforcement = 0.1;

    let scores = ScoreBreakdown {
        recency: SignalScore {
            raw: recency_raw,
            weight: w_recency,
            weighted: recency_raw * w_recency,
        },
        importance: SignalScore {
            raw: mem.importance,
            weight: w_importance,
            weighted: mem.importance * w_importance,
        },
        reinforcement: SignalScore {
            raw: reinforcement_raw,
            weight: w_reinforcement,
            weighted: reinforcement_raw * w_reinforcement,
        },
    };

    let kind_str = match mem.kind {
        MemoryKind::Episode => "episode",
        MemoryKind::Insight => "insight",
        MemoryKind::Revision => "revision",
    };

    // Get edges from graph index
    let mut edges_out = Vec::new();
    let index_manager = state.engine.index_manager();
    let id_16: [u8; 16] = match id_bytes.as_slice().try_into() {
        Ok(a) => a,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };
    if let Ok(out) = index_manager.outgoing_edges(&id_16) {
        for (edge_type, target_id, metadata) in out {
            edges_out.push(EdgeInfo {
                target_id: bytes_to_ulid_string(&target_id),
                edge_type: edge_type_str(&edge_type).to_string(),
                confidence: metadata.confidence,
            });
        }
    }

    // Get HNSW neighbors
    let mut neighbors = Vec::new();
    if let Some(ref embedding) = mem.embedding {
        if let Ok(results) = index_manager.search_vector(embedding, 6, None) {
            for (nid, distance) in results {
                let nid_hex = bytes_to_ulid_string(&nid);
                if nid_hex == id {
                    continue;
                }
                let similarity = 1.0 - distance.min(2.0) / 2.0;
                let label = state
                    .engine
                    .get(&nid)
                    .ok()
                    .map(|m| {
                        if m.content.len() > 60 {
                            format!("{}...", &m.content[..60])
                        } else {
                            m.content.clone()
                        }
                    })
                    .unwrap_or_default();
                neighbors.push(NeighborInfo {
                    id: nid_hex,
                    similarity,
                    label,
                });
            }
        }
    }

    // Look up file info from manifest
    let hebbs_dir = state.vault_root.join(".hebbs");
    let (file_path, heading_path, section_state) =
        if let Ok(manifest) = Manifest::load(&hebbs_dir) {
            find_section_info(&manifest, &id)
        } else {
            (None, vec![], None)
        };

    // Use engine content (section-level), not the full file
    let content = mem.content.clone();

    // Insight-specific fields
    let confidence = if mem.kind == MemoryKind::Insight {
        mem.context().ok().and_then(|ctx| {
            ctx.get("hebbs-confidence")
                .and_then(|v| v.as_f64())
                .map(|f| f as f32)
        })
    } else {
        None
    };

    let source_ids = if mem.kind == MemoryKind::Insight {
        let mut sources = Vec::new();
        for edge in &edges_out {
            if edge.edge_type == "insight_from" {
                sources.push(edge.target_id.clone());
            }
        }
        if sources.is_empty() {
            None
        } else {
            Some(sources)
        }
    } else {
        None
    };

    Ok(Json(MemoryDetailResponse {
        memory_id: id,
        content,
        file_path,
        heading_path,
        kind: kind_str.to_string(),
        importance: mem.importance,
        created_at: mem.created_at,
        updated_at: mem.updated_at,
        last_accessed_at: mem.last_accessed_at,
        access_count: mem.access_count,
        decay_score: mem.decay_score,
        state: section_state.map(|s| match s {
            SectionState::Synced => "synced".to_string(),
            SectionState::ContentStale => "stale".to_string(),
            SectionState::Orphaned => "orphaned".to_string(),
        }),
        scores,
        edges: edges_out,
        neighbors,
        confidence,
        source_ids,
    }))
}

// ── Recall search (Phase 2, Step 11) ────────────────────────────────────

#[derive(Deserialize)]
struct RecallWeightsRequest {
    relevance: Option<f32>,
    recency: Option<f32>,
    importance: Option<f32>,
    reinforcement: Option<f32>,
}

#[derive(Deserialize)]
struct RecallFiltersRequest {
    state: Option<String>,
    file_path: Option<String>,
    importance_min: Option<f32>,
    importance_max: Option<f32>,
}

#[derive(Deserialize)]
struct RecallRequest {
    query: String,
    weights: Option<RecallWeightsRequest>,
    strategies: Option<Vec<String>>,
    top_k: Option<usize>,
    filters: Option<RecallFiltersRequest>,
}

#[derive(Serialize)]
struct RecallResultEntry {
    memory_id: String,
    content: String,
    file_path: Option<String>,
    heading_path: Vec<String>,
    kind: String,
    score: f32,
    relevance: f32,
    recency: f32,
    importance: f32,
    reinforcement: f32,
    state: Option<String>,
}

#[derive(Serialize)]
struct RecallResponse {
    results: Vec<RecallResultEntry>,
    latency_us: u64,
    total_results: usize,
}

async fn recall_search(
    State(state): State<AppState>,
    Json(req): Json<RecallRequest>,
) -> Result<Json<RecallResponse>, StatusCode> {
    let start = std::time::Instant::now();

    // Map strategy strings to RecallStrategy enum values.
    let strategies: Vec<RecallStrategy> = req
        .strategies
        .unwrap_or_else(|| vec!["similarity".to_string()])
        .iter()
        .filter_map(|s| match s.as_str() {
            "similarity" => Some(RecallStrategy::Similarity),
            "temporal" => Some(RecallStrategy::Temporal),
            "causal" => Some(RecallStrategy::Causal),
            "analogical" => Some(RecallStrategy::Analogical),
            _ => None,
        })
        .collect();

    if strategies.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut input = RecallInput::multi(req.query, strategies);
    input.top_k = Some(req.top_k.unwrap_or(10).min(1000));

    if let Some(w) = req.weights {
        let defaults = ScoringWeights::default();
        input.scoring_weights = Some(ScoringWeights {
            w_relevance: w.relevance.unwrap_or(defaults.w_relevance),
            w_recency: w.recency.unwrap_or(defaults.w_recency),
            w_importance: w.importance.unwrap_or(defaults.w_importance),
            w_reinforcement: w.reinforcement.unwrap_or(defaults.w_reinforcement),
            max_age_us: defaults.max_age_us,
            reinforcement_cap: defaults.reinforcement_cap,
        });
    }

    let output = state
        .engine
        .recall(input)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Load manifest for file path and state lookups.
    let hebbs_dir = state.vault_root.join(".hebbs");
    let manifest = Manifest::load(&hebbs_dir).ok();

    let now_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    let max_age_us: u64 = 30 * 24 * 3600 * 1_000_000;

    let filters = req.filters;

    let mut results = Vec::with_capacity(output.results.len());
    for r in &output.results {
        let mem = &r.memory;
        let mem_id_str = bytes_to_ulid_string(&mem.memory_id);

        // Look up manifest info for file_path, heading_path, and state.
        let (fp, hp, ss) = if let Some(ref m) = manifest {
            find_section_info(m, &mem_id_str)
        } else {
            (None, vec![], None)
        };

        let state_str = ss.map(|s| match s {
            SectionState::Synced => "synced".to_string(),
            SectionState::ContentStale => "stale".to_string(),
            SectionState::Orphaned => "orphaned".to_string(),
        });

        // Apply client-side filters.
        if let Some(ref f) = filters {
            if let Some(ref filter_state) = f.state {
                if state_str.as_deref() != Some(filter_state.as_str()) {
                    continue;
                }
            }
            if let Some(ref filter_fp) = f.file_path {
                match &fp {
                    Some(p) if p == filter_fp => {}
                    _ => continue,
                }
            }
            if let Some(min) = f.importance_min {
                if mem.importance < min {
                    continue;
                }
            }
            if let Some(max) = f.importance_max {
                if mem.importance > max {
                    continue;
                }
            }
        }

        let kind_str = match mem.kind {
            MemoryKind::Episode => "episode",
            MemoryKind::Insight => "insight",
            MemoryKind::Revision => "revision",
        };

        // Compute signal components.
        let relevance = r
            .strategy_details
            .first()
            .map(|d| d.relevance())
            .unwrap_or(0.0);

        let age_us = now_us.saturating_sub(mem.created_at);
        let recency = 1.0 - (age_us as f32 / max_age_us as f32).min(1.0);
        let reinforcement =
            ((1.0 + mem.access_count as f32).ln() / (1.0 + 100.0_f32).ln()).min(1.0);

        results.push(RecallResultEntry {
            memory_id: mem_id_str,
            content: mem.content.clone(),
            file_path: fp,
            heading_path: hp,
            kind: kind_str.to_string(),
            score: r.score,
            relevance,
            recency,
            importance: mem.importance,
            reinforcement,
            state: state_str,
        });
    }

    let total_results = results.len();
    let latency_us = start.elapsed().as_micros() as u64;

    Ok(Json(RecallResponse {
        results,
        latency_us,
        total_results,
    }))
}

// ── Health detail (Phase 2, Step 16) ────────────────────────────────────

#[derive(Serialize)]
struct StaleFileEntry {
    path: String,
    sections_stale: usize,
}

#[derive(Serialize)]
struct OrphanedMemoryEntry {
    memory_id: String,
    content_preview: String,
}

#[derive(Serialize)]
struct DecayCandidateEntry {
    memory_id: String,
    decay_score: f32,
    label: String,
}

#[derive(Serialize)]
struct HealthResponse {
    stale_files: Vec<StaleFileEntry>,
    orphaned_memories: Vec<OrphanedMemoryEntry>,
    decay_candidates: Vec<DecayCandidateEntry>,
    auto_forget_threshold: f32,
}

async fn health_detail(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let manifest = Manifest::load(&hebbs_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Scan manifest for stale files: files with at least one ContentStale section.
    let mut stale_files = Vec::new();
    for (file_path, file_entry) in &manifest.files {
        let stale_count = file_entry
            .sections
            .iter()
            .filter(|s| s.state == SectionState::ContentStale)
            .count();
        if stale_count > 0 {
            stale_files.push(StaleFileEntry {
                path: file_path.clone(),
                sections_stale: stale_count,
            });
        }
    }

    // Collect orphaned memories: sections marked Orphaned in manifest.
    let mut orphaned_memories = Vec::new();
    for (_file_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            if section.state != SectionState::Orphaned {
                continue;
            }
            let preview = if let Ok(id_bytes) = parse_memory_id(&section.memory_id) {
                state
                    .engine
                    .get(&id_bytes)
                    .ok()
                    .map(|m| {
                        if m.content.len() > 120 {
                            format!("{}...", &m.content[..120])
                        } else {
                            m.content.clone()
                        }
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };
            orphaned_memories.push(OrphanedMemoryEntry {
                memory_id: section.memory_id.clone(),
                content_preview: preview,
            });
        }
    }

    // Find low decay_score memories (candidates for auto-forget).
    let auto_forget_threshold: f32 = 0.01;
    let mut decay_candidates = Vec::new();
    for (_file_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            if section.state == SectionState::Orphaned {
                continue;
            }
            if let Ok(id_bytes) = parse_memory_id(&section.memory_id) {
                if let Ok(mem) = state.engine.get(&id_bytes) {
                    if mem.decay_score <= auto_forget_threshold {
                        let label = if mem.content.len() > 80 {
                            format!("{}...", &mem.content[..80])
                        } else {
                            mem.content.clone()
                        };
                        decay_candidates.push(DecayCandidateEntry {
                            memory_id: section.memory_id.clone(),
                            decay_score: mem.decay_score,
                            label,
                        });
                    }
                }
            }
        }
    }

    Ok(Json(HealthResponse {
        stale_files,
        orphaned_memories,
        decay_candidates,
        auto_forget_threshold,
    }))
}

// ── Health actions (Phase 2, Step 16) ───────────────────────────────────

#[derive(Deserialize)]
struct HealthActionRequest {
    action: String,
    memory_id: String,
}

async fn health_action(
    State(state): State<AppState>,
    Json(req): Json<HealthActionRequest>,
) -> Result<StatusCode, StatusCode> {
    let id_bytes = parse_memory_id(&req.memory_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    match req.action.as_str() {
        "dismiss" => {
            let criteria = ForgetCriteria::by_ids(vec![id_bytes]);
            state
                .engine
                .forget(criteria)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(StatusCode::OK)
        }
        "reinforce" => {
            // Retrieve the memory, then re-remember it to increment access_count.
            let mem = state
                .engine
                .get(&id_bytes)
                .map_err(|_| StatusCode::NOT_FOUND)?;
            let input = hebbs_core::engine::RememberInput {
                content: mem.content.clone(),
                importance: Some(mem.importance),
                context: None,
                entity_id: None,
                edges: Vec::new(),
            };
            state
                .engine
                .remember(input)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(StatusCode::OK)
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

// ── Timeline data (Phase 3, Step 17) ────────────────────────────────────

#[derive(Serialize)]
struct DailyCount {
    date: String,
    memories_added: usize,
    insights_added: usize,
}

#[derive(Serialize)]
struct TimelineGrowth {
    total_memories: usize,
    total_insights: usize,
}

#[derive(Serialize)]
struct TimelineRange {
    start: u64,
    end: u64,
}

#[derive(Serialize)]
struct TimelineResponse {
    range: TimelineRange,
    daily_counts: Vec<DailyCount>,
    growth: TimelineGrowth,
}

async fn timeline_data(
    State(state): State<AppState>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let manifest = Manifest::load(&hebbs_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Bucket memories by day using created_at (microsecond timestamps).
    let mut day_buckets: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    let mut total_memories: usize = 0;
    let mut total_insights: usize = 0;
    let mut min_ts: u64 = u64::MAX;
    let mut max_ts: u64 = 0;

    for (_file_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            if section.state == SectionState::Orphaned {
                continue;
            }
            let id_bytes = match parse_memory_id(&section.memory_id) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let mem = match state.engine.get(&id_bytes) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let created_at = mem.created_at;
            if created_at < min_ts {
                min_ts = created_at;
            }
            if created_at > max_ts {
                max_ts = created_at;
            }

            // Convert microsecond timestamp to date string (YYYY-MM-DD).
            let secs = created_at / 1_000_000;
            let days_since_epoch = secs / 86400;
            let date_str = epoch_days_to_date_string(days_since_epoch);

            let entry = day_buckets.entry(date_str).or_insert((0, 0));
            match mem.kind {
                MemoryKind::Insight => {
                    entry.1 += 1;
                    total_insights += 1;
                }
                _ => {
                    entry.0 += 1;
                    total_memories += 1;
                }
            }
        }
    }

    if min_ts == u64::MAX {
        min_ts = 0;
    }

    let daily_counts: Vec<DailyCount> = day_buckets
        .into_iter()
        .map(|(date, (memories, insights))| DailyCount {
            date,
            memories_added: memories,
            insights_added: insights,
        })
        .collect();

    Ok(Json(TimelineResponse {
        range: TimelineRange {
            start: min_ts,
            end: max_ts,
        },
        daily_counts,
        growth: TimelineGrowth {
            total_memories,
            total_insights,
        },
    }))
}

// ── Timeline snapshot (Phase 3, Step 17) ────────────────────────────────

#[derive(Deserialize)]
struct SnapshotQuery {
    at: u64,
}

#[derive(Serialize)]
struct SnapshotResponse {
    memory_ids: Vec<String>,
    count: usize,
}

async fn timeline_snapshot(
    State(state): State<AppState>,
    Query(query): Query<SnapshotQuery>,
) -> Result<Json<SnapshotResponse>, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let manifest = Manifest::load(&hebbs_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let at_us = query.at;
    let mut memory_ids = Vec::new();

    for (_file_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            let id_bytes = match parse_memory_id(&section.memory_id) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let mem = match state.engine.get(&id_bytes) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if mem.created_at <= at_us {
                memory_ids.push(bytes_to_ulid_string(&mem.memory_id));
            }
        }
    }

    let count = memory_ids.len();
    Ok(Json(SnapshotResponse { memory_ids, count }))
}

// ── Forgotten timeline (TASK-20, Item 5) ────────────────────────────────

#[derive(Serialize)]
struct ForgottenEntry {
    memory_id: String,
    forgotten_at: u64,
    forgotten_at_human: String,
    criteria: String,
    entity_id: Option<String>,
    cascade_count: u32,
}

#[derive(Serialize)]
struct ForgottenResponse {
    total_forgotten: usize,
    recent: Vec<ForgottenEntry>,
}

async fn forgotten_timeline(
    State(state): State<AppState>,
) -> Result<Json<ForgottenResponse>, StatusCode> {
    let storage = state.engine.storage();
    let prefix = tombstone_prefix();

    let entries = storage
        .prefix_iterator(ColumnFamilyName::Meta, &prefix)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut tombstones: Vec<ForgottenEntry> = Vec::new();

    for (_key, value) in &entries {
        match Tombstone::from_bytes(value) {
            Ok(ts) => {
                let secs = ts.forget_timestamp_us / 1_000_000;
                let days = secs / 86400;
                let time_of_day = secs % 86400;
                let hours = time_of_day / 3600;
                let minutes = (time_of_day % 3600) / 60;
                let date_str = epoch_days_to_date_string(days);
                let human = format!("{} {:02}:{:02} UTC", date_str, hours, minutes);

                tombstones.push(ForgottenEntry {
                    memory_id: bytes_to_ulid_string(&ts.memory_id),
                    forgotten_at: ts.forget_timestamp_us,
                    forgotten_at_human: human,
                    criteria: ts.criteria_description.clone(),
                    entity_id: ts.entity_id.clone(),
                    cascade_count: ts.cascade_count,
                });
            }
            Err(_) => continue,
        }
    }

    let total_forgotten = tombstones.len();

    // Sort by timestamp descending (most recent first).
    tombstones.sort_by(|a, b| b.forgotten_at.cmp(&a.forgotten_at));

    // Keep only the most recent 100.
    tombstones.truncate(100);

    Ok(Json(ForgottenResponse {
        total_forgotten,
        recent: tombstones,
    }))
}

// ── Config endpoints (Phase 4, Steps 20-21) ─────────────────────────────

async fn get_config(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let config = VaultConfig::load(&hebbs_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let val = serde_json::to_value(&config).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(val))
}

#[derive(Deserialize)]
struct ConfigUpdateRequest {
    chunking: Option<serde_json::Value>,
    embedding: Option<serde_json::Value>,
    watch: Option<serde_json::Value>,
    output: Option<serde_json::Value>,
    scoring: Option<serde_json::Value>,
    decay: Option<serde_json::Value>,
}

async fn update_config(
    State(state): State<AppState>,
    Json(req): Json<ConfigUpdateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let mut config =
        VaultConfig::load(&hebbs_dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Merge partial updates into existing config
    if let Some(c) = req.chunking {
        if let Some(v) = c.get("split_on").and_then(|v| v.as_str()) {
            config.chunking.split_on = v.to_string();
        }
        if let Some(v) = c.get("min_section_length").and_then(|v| v.as_u64()) {
            config.chunking.min_section_length = v as usize;
        }
    }

    if let Some(e) = req.embedding {
        if let Some(v) = e.get("batch_size").and_then(|v| v.as_u64()) {
            config.embedding.batch_size = v as usize;
        }
    }

    if let Some(w) = req.watch {
        if let Some(v) = w.get("phase1_debounce_ms").and_then(|v| v.as_u64()) {
            config.watch.phase1_debounce_ms = v;
        }
        if let Some(v) = w.get("phase2_debounce_ms").and_then(|v| v.as_u64()) {
            config.watch.phase2_debounce_ms = v;
        }
        if let Some(v) = w.get("burst_threshold").and_then(|v| v.as_u64()) {
            config.watch.burst_threshold = v as usize;
        }
        if let Some(v) = w.get("burst_debounce_ms").and_then(|v| v.as_u64()) {
            config.watch.burst_debounce_ms = v;
        }
        if let Some(arr) = w.get("ignore_patterns").and_then(|v| v.as_array()) {
            let patterns: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !patterns.is_empty() {
                config.watch.ignore_patterns = patterns;
            }
        }
    }

    if let Some(o) = req.output {
        if let Some(v) = o.get("insight_dir").and_then(|v| v.as_str()) {
            config.output.insight_dir = v.to_string();
        }
        if let Some(v) = o.get("exclude_insight_dir_from_reflect").and_then(|v| v.as_bool()) {
            config.output.exclude_insight_dir_from_reflect = v;
        }
    }

    if let Some(s) = req.scoring {
        if let Some(v) = s.get("w_relevance").and_then(|v| v.as_f64()) {
            config.scoring.w_relevance = v as f32;
        }
        if let Some(v) = s.get("w_recency").and_then(|v| v.as_f64()) {
            config.scoring.w_recency = v as f32;
        }
        if let Some(v) = s.get("w_importance").and_then(|v| v.as_f64()) {
            config.scoring.w_importance = v as f32;
        }
        if let Some(v) = s.get("w_reinforcement").and_then(|v| v.as_f64()) {
            config.scoring.w_reinforcement = v as f32;
        }
    }

    if let Some(d) = req.decay {
        if let Some(v) = d.get("half_life_days").and_then(|v| v.as_f64()) {
            config.decay.half_life_days = v as f32;
        }
        if let Some(v) = d.get("auto_forget_threshold").and_then(|v| v.as_f64()) {
            config.decay.auto_forget_threshold = v as f32;
        }
        if let Some(v) = d.get("reinforcement_cap").and_then(|v| v.as_u64()) {
            config.decay.reinforcement_cap = v;
        }
    }

    // Validate before saving
    let errors = config.validate();
    if !errors.is_empty() {
        let error_json = serde_json::to_string(&errors).unwrap_or_default();
        return Err((StatusCode::BAD_REQUEST, error_json));
    }

    config
        .save(&hebbs_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Signal the daemon to reload config from disk
    if let Some(ref notify) = state.config_notify {
        notify.notify_one();
    }

    let val =
        serde_json::to_value(&config).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(val))
}

async fn reset_config(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let config = VaultConfig::default();
    config
        .save(&hebbs_dir)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Signal the daemon to reload config from disk
    if let Some(ref notify) = state.config_notify {
        notify.notify_one();
    }

    let val = serde_json::to_value(&config).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(val))
}

async fn export_config(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let config = VaultConfig::load(&hebbs_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let toml_str =
        toml::to_string_pretty(&config).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/toml"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"config.toml\"",
            ),
        ],
        toml_str,
    ))
}

// ── Dashboard data (aggregated overview) ─────────────────────────────────

#[derive(Serialize)]
struct DashboardTopMemory {
    memory_id: String,
    label: String,
    file_path: String,
    kind: String,
    composite_score: f32,
    importance: f32,
    recency: f32,
    reinforcement: f32,
}

#[derive(Serialize)]
struct DashboardRecentEntry {
    memory_id: String,
    label: String,
    file_path: String,
    kind: String,
    created_at: u64,
}

#[derive(Serialize)]
struct DashboardResponse {
    total_memories: usize,
    total_insights: usize,
    total_files: usize,
    total_sections: usize,
    synced: usize,
    stale: usize,
    orphaned: usize,
    sync_percentage: f32,
    top_memories: Vec<DashboardTopMemory>,
    recent_activity: Vec<DashboardRecentEntry>,
    scoring_defaults: ScoringDefaultsResponse,
}

#[derive(Serialize)]
struct ScoringDefaultsResponse {
    w_relevance: f32,
    w_recency: f32,
    w_importance: f32,
    w_reinforcement: f32,
}

async fn dashboard_data(
    State(state): State<AppState>,
) -> Result<Json<DashboardResponse>, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let manifest = Manifest::load(&hebbs_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (synced, stale, orphaned) = manifest.section_counts();
    let total = synced + stale + orphaned;
    let sync_pct = if total > 0 {
        synced as f32 / total as f32 * 100.0
    } else {
        100.0
    };

    let now_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    // Read scoring/decay config for this vault
    let vault_config = VaultConfig::load(&hebbs_dir).unwrap_or_default();
    let max_age_us: u64 = (vault_config.decay.half_life_days as f64 * 24.0 * 3600.0 * 1_000_000.0) as u64;
    let sc = &vault_config.scoring;

    struct MemEntry {
        memory_id: String,
        label: String,
        file_path: String,
        kind: String,
        composite_score: f32,
        importance: f32,
        recency: f32,
        reinforcement: f32,
        created_at: u64,
    }

    let mut all_memories: Vec<MemEntry> = Vec::new();
    let mut total_memories = 0usize;
    let mut total_insights = 0usize;

    for (file_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            if section.state == SectionState::Orphaned {
                continue;
            }
            let id_bytes = match parse_memory_id(&section.memory_id) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let mem = match state.engine.get(&id_bytes) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let kind_str = match mem.kind {
                MemoryKind::Insight => {
                    total_insights += 1;
                    "insight"
                }
                _ => {
                    total_memories += 1;
                    "episode"
                }
            };

            let age_us = now_us.saturating_sub(mem.created_at);
            let recency = 1.0 - (age_us as f32 / max_age_us as f32).min(1.0);
            let reinforcement =
                ((1.0 + mem.access_count as f32).ln() / (1.0 + 100.0_f32).ln()).min(1.0);

            let composite = sc.w_recency * recency
                + sc.w_importance * mem.importance
                + sc.w_reinforcement * reinforcement;

            let label = if !section.heading_path.is_empty() {
                section.heading_path.last().cloned().unwrap_or_default()
            } else {
                file_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(file_path)
                    .trim_end_matches(".md")
                    .to_string()
            };

            all_memories.push(MemEntry {
                memory_id: section.memory_id.clone(),
                label,
                file_path: file_path.clone(),
                kind: kind_str.to_string(),
                composite_score: composite,
                importance: mem.importance,
                recency,
                reinforcement,
                created_at: mem.created_at,
            });
        }
    }

    // Top memories by composite score
    all_memories.sort_by(|a, b| b.composite_score.partial_cmp(&a.composite_score).unwrap_or(std::cmp::Ordering::Equal));
    let top_memories: Vec<DashboardTopMemory> = all_memories
        .iter()
        .take(10)
        .map(|m| DashboardTopMemory {
            memory_id: m.memory_id.clone(),
            label: m.label.clone(),
            file_path: m.file_path.clone(),
            kind: m.kind.clone(),
            composite_score: m.composite_score,
            importance: m.importance,
            recency: m.recency,
            reinforcement: m.reinforcement,
        })
        .collect();

    // Recent activity (most recently created)
    all_memories.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let recent_activity: Vec<DashboardRecentEntry> = all_memories
        .iter()
        .take(15)
        .map(|m| DashboardRecentEntry {
            memory_id: m.memory_id.clone(),
            label: m.label.clone(),
            file_path: m.file_path.clone(),
            kind: m.kind.clone(),
            created_at: m.created_at,
        })
        .collect();

    Ok(Json(DashboardResponse {
        total_memories,
        total_insights,
        total_files: manifest.files.len(),
        total_sections: total,
        synced,
        stale,
        orphaned,
        sync_percentage: sync_pct,
        top_memories,
        recent_activity,
        scoring_defaults: ScoringDefaultsResponse {
            w_relevance: sc.w_relevance,
            w_recency: sc.w_recency,
            w_importance: sc.w_importance,
            w_reinforcement: sc.w_reinforcement,
        },
    }))
}

// ── Memory list (paginated, filterable) ──────────────────────────────────

#[derive(Deserialize)]
struct MemoryListQuery {
    page: Option<usize>,
    per_page: Option<usize>,
    sort_by: Option<String>,
    sort_dir: Option<String>,
    filter_state: Option<String>,
    filter_file: Option<String>,
    search: Option<String>,
}

#[derive(Serialize)]
struct MemoryListEntry {
    memory_id: String,
    label: String,
    file_path: String,
    heading_path: Vec<String>,
    kind: String,
    importance: f32,
    recency: f32,
    reinforcement: f32,
    decay_score: f32,
    access_count: u64,
    created_at: u64,
    state: String,
    content_preview: String,
}

#[derive(Serialize)]
struct MemoryListResponse {
    memories: Vec<MemoryListEntry>,
    total: usize,
    page: usize,
    per_page: usize,
    total_pages: usize,
}

async fn list_memories(
    State(state): State<AppState>,
    Query(query): Query<MemoryListQuery>,
) -> Result<Json<MemoryListResponse>, StatusCode> {
    let hebbs_dir = state.vault_root.join(".hebbs");
    let manifest = Manifest::load(&hebbs_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    let max_age_us: u64 = 30 * 24 * 3600 * 1_000_000;

    let search_lower = query.search.as_deref().unwrap_or("").to_lowercase();

    let mut entries: Vec<MemoryListEntry> = Vec::new();

    for (file_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            let state_str = match section.state {
                SectionState::Synced => "synced",
                SectionState::ContentStale => "stale",
                SectionState::Orphaned => "orphaned",
            };

            // Apply filters
            if let Some(ref fs) = query.filter_state {
                if state_str != fs.as_str() {
                    continue;
                }
            }
            if let Some(ref ff) = query.filter_file {
                if file_path != ff {
                    continue;
                }
            }

            let id_bytes = match parse_memory_id(&section.memory_id) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let mem = match state.engine.get(&id_bytes) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let kind_str = match mem.kind {
                MemoryKind::Episode => "episode",
                MemoryKind::Insight => "insight",
                MemoryKind::Revision => "revision",
            };

            let label = if !section.heading_path.is_empty() {
                section.heading_path.last().cloned().unwrap_or_default()
            } else {
                file_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(file_path)
                    .trim_end_matches(".md")
                    .to_string()
            };

            // Search filter
            if !search_lower.is_empty() {
                let matches = label.to_lowercase().contains(&search_lower)
                    || mem.content.to_lowercase().contains(&search_lower)
                    || file_path.to_lowercase().contains(&search_lower);
                if !matches {
                    continue;
                }
            }

            let age_us = now_us.saturating_sub(mem.created_at);
            let recency = 1.0 - (age_us as f32 / max_age_us as f32).min(1.0);
            let reinforcement =
                ((1.0 + mem.access_count as f32).ln() / (1.0 + 100.0_f32).ln()).min(1.0);

            let preview = if mem.content.len() > 150 {
                format!("{}...", &mem.content[..150])
            } else {
                mem.content.clone()
            };

            entries.push(MemoryListEntry {
                memory_id: section.memory_id.clone(),
                label,
                file_path: file_path.clone(),
                heading_path: section.heading_path.clone(),
                kind: kind_str.to_string(),
                importance: mem.importance,
                recency,
                reinforcement: reinforcement.min(1.0),
                decay_score: mem.decay_score,
                access_count: mem.access_count,
                created_at: mem.created_at,
                state: state_str.to_string(),
                content_preview: preview,
            });
        }
    }

    // Sort
    let sort_by = query.sort_by.as_deref().unwrap_or("created_at");
    let sort_desc = query.sort_dir.as_deref().unwrap_or("desc") == "desc";

    entries.sort_by(|a, b| {
        let cmp = match sort_by {
            "importance" => a.importance.partial_cmp(&b.importance).unwrap_or(std::cmp::Ordering::Equal),
            "recency" => a.recency.partial_cmp(&b.recency).unwrap_or(std::cmp::Ordering::Equal),
            "decay_score" => a.decay_score.partial_cmp(&b.decay_score).unwrap_or(std::cmp::Ordering::Equal),
            "access_count" => a.access_count.cmp(&b.access_count),
            "label" => a.label.cmp(&b.label),
            _ => a.created_at.cmp(&b.created_at),
        };
        if sort_desc { cmp.reverse() } else { cmp }
    });

    let total = entries.len();
    let per_page = query.per_page.unwrap_or(50).min(200);
    let page = query.page.unwrap_or(1).max(1);
    let total_pages = if total == 0 { 1 } else { (total + per_page - 1) / per_page };
    let start = (page - 1) * per_page;
    let page_entries: Vec<MemoryListEntry> = entries.into_iter().skip(start).take(per_page).collect();

    Ok(Json(MemoryListResponse {
        memories: page_entries,
        total,
        page,
        per_page,
        total_pages,
    }))
}

// ═══════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════

/// Parse a memory ID string (ULID or hex) into bytes.
fn parse_memory_id(id: &str) -> Result<Vec<u8>, String> {
    let trimmed = id.trim();
    // Try ULID first (26 chars)
    if trimmed.len() == 26 {
        if let Ok(ulid) = ulid::Ulid::from_string(trimmed) {
            return Ok(ulid.0.to_be_bytes().to_vec());
        }
    }
    // Try hex (32 chars = 16 bytes)
    if trimmed.len() == 32 {
        if let Ok(bytes) = hex::decode(trimmed) {
            if bytes.len() == 16 {
                return Ok(bytes);
            }
        }
    }
    Err(format!("invalid memory ID: {}", trimmed))
}

/// Convert 16-byte ULID bytes to the canonical ULID string format.
fn bytes_to_ulid_string(bytes: &[u8]) -> String {
    if bytes.len() == 16 {
        let mut arr = [0u8; 16];
        arr.copy_from_slice(bytes);
        ulid::Ulid::from_bytes(arr).to_string()
    } else {
        hex::encode(bytes)
    }
}

fn edge_type_str(et: &EdgeType) -> &'static str {
    match et {
        EdgeType::CausedBy => "caused_by",
        EdgeType::RelatedTo => "related_to",
        EdgeType::FollowedBy => "followed_by",
        EdgeType::RevisedFrom => "revised_from",
        EdgeType::InsightFrom => "insight_from",
        EdgeType::Contradicts => "contradicts",
    }
}

/// Find the file path, heading path, and state for a memory ID in the manifest.
fn find_section_info(
    manifest: &Manifest,
    memory_id: &str,
) -> (Option<String>, Vec<String>, Option<SectionState>) {
    for (file_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            if section.memory_id == memory_id {
                return (
                    Some(file_path.clone()),
                    section.heading_path.clone(),
                    Some(section.state),
                );
            }
        }
    }
    (None, vec![], None)
}

/// Convert days since Unix epoch to a YYYY-MM-DD date string.
///
/// Uses a direct arithmetic algorithm (no external date crate). Handles
/// the Gregorian calendar correctly for dates from 1970 onward.
/// Complexity: O(1).
fn epoch_days_to_date_string(days: u64) -> String {
    // Algorithm: convert days since 1970-01-01 to (year, month, day).
    // Based on Howard Hinnant's civil_from_days algorithm.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // day of era [0, 146096]
    let yoe =
        (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month index [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

// ═══════════════════════════════════════════════════════════════════════
//  WebSocket: real-time panel events
// ═══════════════════════════════════════════════════════════════════════

/// Upgrade HTTP to WebSocket and stream `PanelEvent`s as JSON text frames.
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_connection(socket, state))
}

/// Per-connection WebSocket loop: subscribe to the broadcast channel and
/// forward each `PanelEvent` as a JSON text frame until the client
/// disconnects or the channel is closed.
async fn ws_connection(mut socket: WebSocket, state: AppState) {
    let mut rx = state.event_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                let json = match serde_json::to_string(&event) {
                    Ok(j) => j,
                    Err(e) => {
                        debug!("ws: failed to serialize event: {}", e);
                        continue;
                    }
                };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                debug!("ws: client lagged, skipped {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}
