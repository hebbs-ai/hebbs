# TASK-20: Panel Polish -- Deferred Items from TASK-16

Parent: [TASK-16](./done/TASK-16-memory-palace-control-panel.md)
Plan: [PLAN-16](./plans/PLAN-16.md)

## Purpose

TASK-16 shipped all four phases of the Memory Palace control panel. Several items were deferred because they depend on infrastructure that does not yet exist (UMAP crate, RocksDB projections column family, daemon IPC, hebbs-core forget events, LLM reflect config, scoring/decay fields in VaultConfig). This task tracks those items so they can be picked up once their dependencies land.

---

## What shipped in TASK-16

- `hebbs panel` command, embedded axum HTTP server, vanilla JS + Canvas 2D
- Force-directed graph home view, side panel with score breakdown, vault dropdown
- Search with recall API, weight sliders, strategy toggles, filters, decay mode, health detail
- Timeline scrubber with sparklines and snapshot filtering
- Config editor (CRUD for config.toml), PNG export
- Layout fix (graph centered via flex min-height:0 + absolute canvas)

---

## What needs to be done

### 1. Server-side UMAP projection

**Current state:** Node positions use a Fibonacci spiral as initial layout, then force simulation settles them. Positions are not semantically meaningful -- they emerge from spring physics, not from embedding space.

**Target:** Compute 2D UMAP projection of the 384-dim embedding vectors server-side. Return `(x, y)` per node from `/api/panel/graph`. The force simulation then only handles fine-tuning and edge attraction, not the primary layout.

**Dependencies:**
- A Rust UMAP implementation (e.g., `linfa-reduction` or port of umap-rs)
- OR: compute in Python as a CLI helper and cache results

**Files:** `hebbs-vault/src/panel/routes.rs` (graph_data handler), potentially a new `projection.rs`

---

### 2. Spatial position persistence

**PLAN-16 Step 22.** Persist node `(x, y)` positions in a new RocksDB column family (`projections`) so they survive across sessions and vault switches.

**Behavior:**
- On panel open: load positions from projections CF. Render immediately if found.
- On user drag: save dragged position, mark as "pinned" so UMAP does not override it.
- On full re-layout: preserve pinned positions, recompute unpinned.

**Dependencies:**
- New `projections` column family in `hebbs-storage`
- API: `GET /api/panel/positions`, `PUT /api/panel/positions/:id`

---

### 3. Cluster detection and auto-labeling

**PLAN-16 Steps 5 + 23.** Group nodes into clusters based on embedding proximity, label each cluster.

**Phase A (term-frequency):** After UMAP, run HDBSCAN or k-means on 2D positions. Label each cluster with the most frequent heading terms from its members.

**Phase B (LLM upgrade, Step 23):** When reflect is enabled, sample up to 5 representative memories per cluster, send to LLM for a 2-4 word label. Cache labels in projections CF. Regenerate when cluster membership changes by >30%.

**Dependencies:**
- Phase A: UMAP (item 1 above)
- Phase B: LLM reflect config, `hebbs-reflect` crate

---

### 4. WebSocket live events

**PLAN-16 Step 8.** Push real-time events to the panel: new memories indexed, watcher activity, decay updates.

**Dependencies:**
- Daemon mode (PLAN-daemon Milestone 6) for a long-lived process
- Event bus or channel in `hebbs-core` / `hebbs-vault` daemon

---

### 5. Forget log in timeline

**PLAN-16 Step 19.** Show forgotten/auto-decayed memories in the timeline scrubber so users can see what faded away over time.

**Dependencies:**
- `hebbs-core` needs to record forget events with timestamps (currently `engine.forget()` does not emit events)
- New storage: forget event log (append-only)

---

### 6. Scoring and decay fields in config

**Current state:** `VaultConfig` has sections for chunking, embedding, watch, and output. The plan specifies scoring weights (`w_relevance`, `w_recency`, `w_importance`, `w_reinforcement`) and decay settings (`half_life_days`, `auto_forget_threshold`, `reinforcement_cap`) as part of the config editor, but these fields do not exist in `VaultConfig` today. The engine uses hardcoded defaults in `ScoringWeights::default()`.

**Target:** Add `scoring` and `decay` sections to `VaultConfig`. The config editor already has the UI scaffolding -- it just needs to map to real config fields once they exist. The engine's `ScoringWeights::default()` should read from config.

**Files:** `hebbs-vault/src/config.rs`, `hebbs-core/src/recall.rs` (ScoringWeights), `hebbs-vault/src/panel/routes.rs` (config handlers), `hebbs-vault/src/panel/static/app.js` (config editor render)

---

### 7. Config input validation

**Current state:** `PUT /api/panel/config` accepts any values without validation. Invalid values (e.g., negative debounce, empty split_on) are silently written.

**Target:** Validate on PUT, return 400 with field-specific error messages. Per PLAN-16 Step 20:
- Weights must be >= 0
- half_life_days must be > 0
- ef_search must be >= 1
- ignore_patterns must be valid glob patterns

---

### 8. Daemon config reload notification

**Current state:** After `PUT /api/panel/config` writes config.toml, the running watcher/engine does not pick up the new values until restart.

**Target:** After config write, notify the daemon to reload. The watcher picks up new debounce values; the engine picks up new scoring weight defaults.

**Dependencies:** Daemon IPC (PLAN-daemon)

---

### 9. SVG graph export

**Current state:** PNG export works (2400x1260 at 2x DPI). SVG export was planned but skipped.

**Target:** Re-render the current graph view using a temporary SVG layer (d3 SVG renderer) with the same node positions. One-time render, not the live view.

**Priority:** Low. PNG covers the primary use case (shareable screenshots).

---

### 10. Contradiction visualization

**Depends on:** [PLAN-contradiction](./plans/PLAN-contradiction.md) Steps 1-6 completing first.

Once `CONTRADICTS` edges exist in the graph, the panel needs:
- Red/dashed rendering for CONTRADICTS edges in graph.js
- Warning badge on nodes that have contradictions
- Contradiction list in side panel with resolution actions (dismiss, mark as revision)
- Optional: dedicated Contradictions tab

---

## Priority order

| Priority | Item | Blocked by | Status |
|----------|------|------------|--------|
| **High** | 6. Scoring/decay config fields | Nothing | **DONE** |
| **High** | 7. Config input validation | Nothing | **DONE** |
| **High** | 1. UMAP projection | Nothing | **DONE** |
| **High** | 2. Position persistence | Nothing | **DONE** |
| **High** | 3. Clustering + labels (Phase A) | Nothing | **DONE** |
| **Medium** | 10. Contradiction visualization | PLAN-contradiction | **DONE** (basic) |
| **Medium** | 5. Forget log | hebbs-core forget events | **DONE** |
| **Medium** | 3b. Clustering labels (Phase B, LLM) | hebbs-reflect | **DONE** |
| **Low** | 4. WebSocket live events | Nothing (daemon M6 done) | **DONE** |
| **Low** | 8. Daemon config reload | Nothing (daemon M6 done) | **DONE** |
| **Low** | 9. SVG export | Nothing | **DONE** |

---

## Status

Items 1, 2, 3 (Phase A), 6, 7, and 10 (basic) completed (2026-03-15).
Items 3b, 4, 5, 8, and 9 completed (2026-03-15). **All TASK-20 items are now DONE.**

### Item 1: Server-side UMAP projection (DONE)
- Built custom UMAP implementation in `hebbs-index/src/neighborhood.rs` (~450 lines)
- Reuses existing HNSW layer-0 neighbors as free k-NN approximation (no new dependencies)
- Algorithm: k-NN extraction, fuzzy simplicial set (sigma binary search), symmetrization, SGD with attractive/repulsive forces, normalize to [-200, 200]
- Projection cached in-memory with node count invalidation, served in ~7ms on cache hit
- Force simulation adapts: minimal forces when UMAP positions provided, full physics as fallback
- 10 unit tests covering snapshot extraction, projection, determinism, clustering, sigma convergence

### Item 2: Position persistence (DONE)
- Pinned positions stored in RocksDB Meta CF with `panel_pin:` prefix keys (8 bytes: f32 x, f32 y)
- API: `PUT /api/panel/positions/:id` to pin, `POST /api/panel/positions/:id/unpin` to unpin
- Dragged nodes automatically persist via fetch() call on mouseup
- Double-click a pinned node to unpin (reverts to UMAP position)
- Pinned positions override UMAP on load, survive restarts

### Item 3: Clustering + labels, Phase A (DONE)
- Union-find clustering on k-NN graph with distance threshold (65th percentile)
- Convex hull rendering (Graham scan) with padded, color-filled hulls per cluster
- Auto-labeling: term-frequency extraction from node headings, top 2-3 terms as cluster label
- Labels rendered above hull centroids in matching muted color
- 8 cluster color palette (amber, blue, emerald, purple, red, sky, orange, green)

### Item 6: Scoring/decay config fields (DONE)
- Added `ScoringConfig` (w_relevance, w_recency, w_importance, w_reinforcement) and `DecayConfig` (half_life_days, auto_forget_threshold, reinforcement_cap) to `VaultConfig`
- Settings tab now has Scoring Weights and Decay sections with editable fields
- Dashboard uses vault config scoring weights for composite score calculation
- Config round-trips through TOML (save/load/export/reset all work)

### Item 7: Config input validation (DONE)
- `VaultConfig::validate()` returns field-specific error map
- `PUT /api/panel/config` validates before writing, returns 400 with JSON error map on failure
- Validated: split_on not empty and starts with #, batch_size >= 1, debounce >= 50, burst_threshold >= 1, glob pattern validity, scoring weights >= 0, half_life_days > 0, auto_forget_threshold 0-1, reinforcement_cap >= 1
- Settings tab displays validation errors inline

### Item 10: Contradiction visualization (DONE - basic)
- Full contradiction detection pipeline implemented in [PLAN-contradiction](./plans/PLAN-contradiction.md) (Steps 1-4, 6-7)
- `Contradicts = 0x06` edge type added across all crates (~12 files)
- Heuristic classifier: negation asymmetry, antonym pairs, numeric disagreement, revision markers
- LLM classifier: structured prompt via `hebbs_reflect::LlmProvider` (auto-detected from config)
- Pipeline hooks into ingest: runs `check_contradictions()` after `engine.remember()`
- `ContradictionConfig` added to `VaultConfig` (enabled, candidates_k, min_similarity, min_confidence)
- Panel: CONTRADICTS edges rendered as red dashed lines in graph.js (canvas + PNG export)
- 37 tests total (12 unit + 25 integration), 922 workspace tests passing
- Deferred: warning badge on nodes, contradiction side panel list, resolution actions, dedicated tab

### Item 3b: Clustering labels, Phase B -- LLM upgrade (DONE)
- `compute_cluster_labels_llm()` in `routes.rs` (~80 lines): collects up to 5 representative headings per cluster, sends structured prompt to LLM via `hebbs_reflect::LlmProvider`
- LLM returns JSON array of 2-4 word labels per cluster
- Graceful fallback: if `ReflectLlmConfig` is not configured or LLM call fails, falls back to term-frequency labels (Phase A)
- Uses existing `ReflectLlmConfig` from `VaultConfig` -- no new config needed
- Zero new dependencies

### Item 4: WebSocket live events (DONE)
- `PanelEvent` enum (`MemoryCreated`, `MemoryForgotten`, `IngestComplete`, `ConfigReloaded`) with `Serialize`
- `tokio::sync::broadcast` channel (capacity 64) in `PanelState`, wired into both `start_panel_server` and `start_panel_server_from_daemon`
- `ws_handler` + `ws_connection` in `routes.rs`: upgrades HTTP to WebSocket, streams `PanelEvent` as JSON
- JS client in `app.js`: connects to `/api/panel/ws`, debounced graph refresh on events (500ms), auto-reconnect every 2s
- Route: `GET /api/panel/ws`

### Item 5: Forget log in timeline (DONE)
- `GET /api/panel/timeline/forgotten` endpoint scans RocksDB Meta CF for tombstone keys (`tombstone:` prefix)
- Returns `ForgottenResponse` with array of `ForgottenEntry` (memory_id hex, forgotten_at timestamp)
- Timeline tab in `app.js` renders forgotten memories section below the existing timeline sparkline
- No new storage needed -- reuses existing tombstone records from `engine.forget()`

### Item 8: Daemon config reload notification (DONE)
- `config_notify: Option<Arc<tokio::sync::Notify>>` added to `PanelState`
- `update_config` and `reset_config` route handlers call `config_notify.notify_one()` after successful config write
- Daemon watches the `Notify` and can reload config without restart
- Wired in both `start_panel_server` (None) and `start_panel_server_from_daemon` (Some(notify))

### Item 9: SVG graph export (DONE)
- `exportSVG()` method in `graph.js` (~130 lines): pure client-side SVG string generation
- Renders: background rect, cluster hulls (filled polygons), edges (with red dashed stroke for contradictions), nodes (circles for episodes, hexagons for insights), labels, title
- Downloads as `hebbs-memory-palace.svg` via Blob + click
- SVG export button added to `index.html` toolbar next to PNG button
- Click listener in `app.js` calls `state.graph.exportSVG()`
