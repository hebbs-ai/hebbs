# PLAN-16: Memory Palace Control Panel

Parent: [TASK-16](../done/TASK-16-memory-palace-control-panel.md)
Daemon integration: [PLAN-daemon](./PLAN-daemon.md) (Milestone 6)

---

## Status

| Phase | Status | Notes |
|-------|--------|-------|
| Phase 1: Graph home + side panel + vault navigation | **DONE** | MVP. Steps 1-10. Vanilla JS + Canvas 2D force-directed graph, side panel with score breakdown, metadata, similar memories. Route fix: axum 0.7 uses `:id` not `{id}`. |
| Phase 2: Search overlay + weight sliders + filters | **DONE** | Steps 11-16. Search with recall API (scored results, latency badge), weight sliders with presets, strategy toggles, filters, decay mode with health detail panel. |
| Phase 3: Timeline scrubber + growth | **DONE** | Steps 17-18. Timeline data endpoint, sparklines, scrubber with snapshot filtering. |
| Phase 4: Config editor + persistence + polish | **DONE** | Steps 20-21 (config CRUD endpoints + editor UI), Step 24 (PNG export). Layout fix: graph centered via flex `min-height:0` + absolute canvas positioning. |

### Completed steps

| Step | Description | Status |
|------|-------------|--------|
| 1-10 | Crate scaffold, vaults, status, graph, memory detail, WebSocket, frontend SPA, CSS | **DONE** |
| 11 | Recall search endpoint (`POST /api/panel/recall`) | **DONE** |
| 12 | Search overlay + graph highlighting (amber glow on match, fade on non-match) | **DONE** |
| 13 | Weight sliders (relevance/recency/importance/reinforcement) with presets | **DONE** |
| 14 | Strategy toggles (Similarity/Temporal/Causal/Analogical) + top_k selector | **DONE** |
| 15 | Filters (state/file/importance range) | **DONE** |
| 16 | Decay monitor toggle + health detail panel (stale files, orphaned, decay candidates) | **DONE** |
| 17 | Timeline data endpoint (`GET /api/panel/timeline`, `/timeline/snapshot`) | **DONE** |
| 18 | Timeline scrubber component + sparklines | **DONE** |
| 20 | Config endpoints (`GET/PUT /api/panel/config`, reset, export) | **DONE** |
| 21 | Config editor UI (chunking, embedding, watcher, ignore patterns, output, save/reset/export) | **DONE** |
| 24 | Graph export as PNG (2400x1260 at 2x DPI with title overlay) | **DONE** |

### Pending steps (deferred to [TASK-20](../TASK-20-panel-polish.md))

| Step | Description | Why deferred |
|------|-------------|--------------|
| 4 (partial) | UMAP projection (server-side) | Currently using Fibonacci spiral + force simulation. Real UMAP needs `linfa-reduction` or equivalent Rust crate. |
| 5 (partial) | Cluster auto-labeling (term-frequency) | No clustering logic yet; nodes laid out by force simulation only. |
| 8 | WebSocket live events | Requires daemon integration (PLAN-daemon Milestone 6). |
| 19 | Forget log in timeline | Requires `hebbs-core` to emit forget events with timestamps. |
| 22 | Spatial position persistence (projections CF, pinned nodes) | Needs new RocksDB column family for `(memory_id -> x, y)`. |
| 23 | Cluster auto-labeling (LLM upgrade) | Depends on Step 5 + LLM reflect config. |
| 24 (partial) | SVG export | Needs SVG re-render from Canvas 2D node positions. PNG done. |
| -- | Scoring weights in config (decay half_life, auto_forget_threshold, reinforcement_cap) | `VaultConfig` does not yet include scoring/decay fields. Needs `hebbs-core` config integration. |
| -- | Input validation on config PUT (400 with field-specific errors) | Currently accepts any values without validation. |
| -- | Daemon config reload notification after config write | Requires daemon IPC (PLAN-daemon). |

---

## Tech Stack Decisions

### Graph renderer: force-graph (2D WebGL)

[force-graph](https://github.com/vasturiano/force-graph) renders via WebGL canvas. At 50 nodes it looks stunning with smooth physics, zoom, pan, hover. At 100K+ nodes it maintains 60fps where D3-SVG dies at ~5K. WebGL gives native support for the amber glow, translucency, and brightness effects the spec requires.

The 3D variant (`3d-force-graph`, same author, nearly identical API) is a one-day migration if the memory palace metaphor calls for depth later.

### UI framework: Preact + HTM (no build step)

The panel has real state: active vault, selected node, search query, four slider values, filter state, decay mode toggle, side panel open/closed. Managing that without components means spaghetti by Phase 2.

Preact (4KB gzipped) + HTM (700B) gives:
- Tagged template literals instead of JSX: **no transpiler, no bundler, no node_modules**
- `useState`/`useEffect` hooks for reactive slider updates
- Component lifecycle for side panel mount/unmount
- Directly vendored into `include_dir!` as plain `.js` files

```js
import { h, render } from './vendor/preact.module.js';
import { useState } from './vendor/preact-hooks.module.js';
import htm from './vendor/htm.module.js';
const html = htm.bind(h);
```

### Dimensionality reduction: Server-side UMAP, cached in RocksDB

Precompute 2D projections on ingest. Store `(memory_id -> x, y)` in a new `projections` column family. The `/api/panel/graph` endpoint reads cached positions, never recomputes on request.

Incremental strategy: new memories get placed near their nearest neighbors (approximate position from HNSW neighbor lookup), then a lightweight local optimization refines. Full re-layout runs as a background task on configurable interval or manual trigger.

### Scale strategy: Level-of-detail rendering

| Node count | Rendering mode |
|---|---|
| <500 | Everything: full labels, edges, glow effects |
| 500-10K | Cluster rendering: cluster bubbles at default zoom, individual nodes on zoom-in, viewport-culled edges |
| 10K-100K+ | Hierarchical clustering (precomputed server-side from HNSW graph): ~50 mega-clusters at top level, drill to sub-clusters, drill to individual nodes |

force-graph supports custom node rendering, so the same code path handles all three tiers. The server provides the hierarchy; the client renders what is visible.

### Physics: Web Worker from day 1

force-graph runs physics on the main thread by default. At scale this blocks UI. Move the force simulation to a Web Worker using `d3-force` in a worker with message passing. Costs an afternoon to set up; saves a rewrite later.

---

## Architecture

### New crate: `hebbs-panel`

Lives at `hebbs/crates/hebbs-panel/`. Contains:
- Axum HTTP handlers for panel-specific API endpoints
- Static SPA assets (compiled in via `include_dir!`)
- UMAP projection engine
- Graph clustering logic

```
hebbs-panel/
  src/
    lib.rs              -- public API: panel_router(), ProjectionEngine
    routes.rs           -- Axum handlers for /api/panel/* endpoints
    projection.rs       -- UMAP projection, incremental updates, caching
    clustering.rs       -- hierarchical clustering from HNSW graph structure
    graph.rs            -- graph data assembly (nodes, edges, clusters)
    timeline.rs         -- temporal event aggregation for scrubber
    assets.rs           -- include_dir! static file serving
  static/               -- SPA source files (no build step)
    index.html
    app.js              -- Preact app entry point
    components/
      Palace.js         -- main graph + layout container
      Graph.js          -- force-graph WebGL wrapper
      SidePanel.js      -- memory/insight detail panel
      Header.js         -- vault dropdown + search bar
      HealthBadge.js    -- sync %, stale/orphaned/decay counts
      Legend.js         -- node/edge type legend
      Sliders.js        -- weight sliders (Phase 2)
      Filters.js        -- state/file/importance filters (Phase 2)
      Timeline.js       -- scrubber + growth sparklines (Phase 3)
      ConfigEditor.js   -- config.toml editor (Phase 4)
    lib/
      state.js          -- global state management (Preact signals or context)
      api.js            -- fetch wrappers for all /api/panel/* endpoints
      worker.js         -- Web Worker for force simulation
      lod.js            -- level-of-detail rendering logic
    vendor/
      preact.module.js        -- Preact 10.x ESM bundle (~4KB gzip)
      preact-hooks.module.js  -- Preact hooks
      htm.module.js           -- HTM tagged templates (~700B)
      force-graph.module.js   -- force-graph WebGL renderer
      d3-force.module.js      -- d3-force (used in Web Worker)
    styles/
      panel.css         -- dark theme, HEBBS brand colors
  Cargo.toml
```

### Integration with daemon

Per PLAN-daemon Milestone 6, the daemon serves the panel on a local HTTP port (default 6381). `hebbs-panel` exposes a `panel_router()` function returning an Axum `Router` that the daemon mounts at `/`.

```rust
// In daemon setup (hebbs-vault/src/daemon.rs)
let panel = hebbs_panel::panel_router(engine.clone(), vault_manager.clone());
let app = Router::new()
    .merge(panel)
    // ... other daemon routes
    ;
axum::serve(listener, app).await?;
```

The panel router receives `Arc<Engine>` and `Arc<VaultManager>` via Axum state. All panel API calls go through the in-process engine; no socket hop.

### `hebbs panel` command

A convenience command in `hebbs-vault/src/bin/hebbs.rs`:

1. Call `ensure_daemon()` (same as other commands)
2. Open `http://127.0.0.1:6381` in the default browser (`open` on macOS, `xdg-open` on Linux, `start` on Windows)
3. Print `Panel running at http://127.0.0.1:6381`
4. Exit (the daemon keeps serving)

Flags:
- `hebbs panel` inside a project dir with `.hebbs/`: opens with that vault selected
- `hebbs panel --global`: opens with the global vault selected
- `hebbs panel` outside any project: opens with the global vault selected
- `hebbs panel --port 8080`: override HTTP port

The vault selection is passed as a query parameter: `http://127.0.0.1:6381?vault=/path/to/project`

---

## API Endpoints

All panel endpoints live under `/api/panel/` to avoid collision with existing server REST API (`/v1/*`).

| Endpoint | Method | Returns | Phase |
|---|---|---|---|
| `/` | GET | Static SPA (index.html) | 1 |
| `/static/*` | GET | JS/CSS/vendor assets | 1 |
| `/api/panel/vaults` | GET | Vault list from `~/.hebbs/vaults.json` | 1 |
| `/api/panel/vaults/switch` | POST | Switch active vault, return new vault stats | 1 |
| `/api/panel/status` | GET | Active vault stats: memory count, insight count, sync %, stale, orphaned, decay candidates | 1 |
| `/api/panel/graph` | GET | UMAP-projected nodes + edges + cluster hierarchy | 1 |
| `/api/panel/graph/clusters` | GET | Cluster metadata with labels and bounding boxes | 1 |
| `/api/panel/memories/:id` | GET | Full memory detail: content, scores, decay curve, edges, access history | 1 |
| `/api/panel/insights/:id` | GET | Insight detail: confidence, source memories, generation timestamp | 1 |
| `/api/panel/files/:path` | GET | Source file content (read from disk, not stored content) | 1 |
| `/api/panel/recall` | POST | Recall with full score breakdown per signal per result | 2 |
| `/api/panel/health` | GET | Health detail: stale files list, orphaned memories, decay candidates | 2 |
| `/api/panel/health/actions` | POST | Re-index, dismiss, reinforce actions on health items | 2 |
| `/api/panel/timeline` | GET | Temporal event history for scrubber (created_at timestamps, growth data) | 3 |
| `/api/panel/timeline/snapshot` | GET | Graph state at a specific timestamp (for scrubber playback) | 3 |
| `/api/panel/config` | GET | Current config.toml as structured JSON | 4 |
| `/api/panel/config` | PUT | Write config.toml changes | 4 |
| `/api/panel/config/reset` | POST | Reset to factory defaults | 4 |
| `/api/panel/config/export` | GET | Download config.toml as file | 4 |
| `/ws/panel` | WebSocket | Live events: new memories, watcher activity, decay updates | 1 |

### Response shapes

**`GET /api/panel/graph`**

```json
{
  "nodes": [
    {
      "id": "01JABC...",
      "x": 0.42,
      "y": -0.18,
      "kind": "episode",
      "importance": 0.75,
      "recency": 0.92,
      "reinforcement": 0.3,
      "decay_score": 0.68,
      "label": "Vendor evaluation notes",
      "file_path": "notes/meeting-jan.md",
      "heading_path": ["Vendor evaluation"],
      "cluster_id": "cluster_003",
      "state": "synced"
    },
    {
      "id": "01JABD...",
      "kind": "insight",
      "confidence": 0.82,
      "source_ids": ["01JABC...", "01JABE..."],
      ...
    }
  ],
  "edges": [
    {"source": "01JABC...", "target": "01JABD...", "type": "insight_from", "confidence": 0.82},
    {"source": "01JABC...", "target": "01JABF...", "type": "similarity", "weight": 0.78}
  ],
  "clusters": [
    {"id": "cluster_003", "label": "Vendor notes", "node_count": 12, "centroid_x": 0.40, "centroid_y": -0.15}
  ],
  "lod": {
    "total_nodes": 847,
    "rendered_nodes": 847,
    "level": "full"
  }
}
```

**`GET /api/panel/memories/:id`**

```json
{
  "memory_id": "01JABC...",
  "content": "The vendor delivered on time...",
  "file_path": "notes/meeting-jan.md",
  "heading_path": ["Vendor evaluation"],
  "kind": "episode",
  "importance": 0.75,
  "created_at": 1710000000000,
  "updated_at": 1710000000000,
  "last_accessed_at": 1710100000000,
  "access_count": 12,
  "decay_score": 0.68,
  "projected_fade_date": 1720000000000,
  "state": "synced",
  "scores": {
    "relevance": null,
    "recency": {"raw": 0.92, "weight": 0.20, "weighted": 0.184},
    "importance": {"raw": 0.75, "weight": 0.20, "weighted": 0.150},
    "reinforcement": {"raw": 0.30, "weight": 0.10, "weighted": 0.030}
  },
  "edges": [
    {"target_id": "01JABD...", "type": "related_to", "confidence": 0.9},
    {"target_id": "01JABE...", "type": "insight_from", "confidence": 0.82}
  ],
  "neighbors": [
    {"id": "01JABF...", "similarity": 0.78, "label": "Q3 vendor review"}
  ]
}
```

**`POST /api/panel/recall`** (Phase 2)

```json
// Request
{
  "query": "vendor performance",
  "weights": {"w_relevance": 0.5, "w_recency": 0.2, "w_importance": 0.2, "w_reinforcement": 0.1},
  "strategies": ["similarity", "temporal"],
  "top_k": 10,
  "filters": {
    "state": "active",
    "file_path": null,
    "importance_min": 0.0,
    "importance_max": 1.0
  }
}

// Response
{
  "results": [
    {
      "memory_id": "01JABC...",
      "label": "Vendor evaluation notes",
      "composite_score": 0.782,
      "scores": {
        "relevance": {"raw": 0.87, "weight": 0.50, "weighted": 0.435},
        "recency": {"raw": 0.92, "weight": 0.20, "weighted": 0.184},
        "importance": {"raw": 0.75, "weight": 0.20, "weighted": 0.150},
        "reinforcement": {"raw": 0.13, "weight": 0.10, "weighted": 0.013}
      },
      "strategy": "similarity"
    }
  ],
  "latency_ms": 3.2,
  "total_candidates": 847
}
```

---

## Phase 1: Graph Home + Side Panel + Vault Navigation (MVP)

### Step 1: New crate scaffold (`hebbs-panel`)

Create `hebbs/crates/hebbs-panel/` with:
- `Cargo.toml`: depends on `hebbs-core`, `hebbs-vault`, `hebbs-storage`, `hebbs-index`, `axum`, `tower-http` (static files), `include_dir`, `serde_json`, `tokio-tungstenite`
- `src/lib.rs`: exports `panel_router(state: PanelState) -> Router`
- `src/assets.rs`: `include_dir!("static")` serving with proper MIME types and cache headers
- Add to workspace `Cargo.toml` members list

**PanelState** (Axum state):

```rust
pub struct PanelState {
    pub vault_manager: Arc<VaultManager>,     // from daemon
    pub projection_cache: Arc<ProjectionCache>, // 2D positions, cluster hierarchy
    pub event_tx: broadcast::Sender<PanelEvent>, // WebSocket events
}
```

**Tests**: crate compiles, static assets served, `/` returns index.html with correct content-type.

---

### Step 2: Vault endpoints

Implement `/api/panel/vaults` and `/api/panel/vaults/switch`.

**`GET /api/panel/vaults`**:
1. Read `~/.hebbs/vaults.json`
2. For each vault, check if `.hebbs/` exists and compute basic stats (file count from manifest)
3. Return list with `path`, `label`, `healthy` flag, `memory_count`

**`POST /api/panel/vaults/switch`**:
1. Accept `{"vault_path": "/path/to/project"}`
2. Call `vault_manager.open_vault(path)` (opens RocksDB if not already open)
3. Return vault stats

The daemon's `VaultManager` (from PLAN-daemon) already handles open/close/idle-evict. The panel just calls into it.

**Tests**:
- List vaults returns correct entries from vaults.json
- Switch to valid vault returns stats
- Switch to nonexistent vault returns 404 with clear message
- Switch to uninitialized path (no .hebbs/) returns 400

---

### Step 3: Status endpoint

Implement `/api/panel/status`.

Reads from the active vault's manifest and engine:

```rust
pub struct VaultStatus {
    pub vault_path: String,
    pub memory_count: u64,           // total memories in engine
    pub insight_count: u64,          // memories with kind == Insight
    pub file_count: u64,             // files in manifest
    pub section_count: u64,          // total sections across all files
    pub synced_count: u64,           // sections in Synced state
    pub stale_count: u64,            // sections in ContentStale state
    pub orphaned_count: u64,         // sections in Orphaned state
    pub decay_candidates: u64,       // memories with decay_score < threshold * 10 (approaching auto-forget)
    pub sync_percentage: f32,        // synced_count / section_count * 100
    pub last_activity: u64,          // most recent created_at or updated_at
}
```

**Decay candidate detection**: iterate memories, count those with `decay_score < auto_forget_threshold * 10` (within 10x of auto-forget). This is a scan, but bounded by total memory count and cached.

**Caching**: status is computed on first request and invalidated by WebSocket events (new memory, forget, re-index). Avoids re-scanning on every poll.

**Tests**:
- Empty vault returns zeroes
- After ingesting files, counts match manifest
- After modifying a file (without re-index), stale count increments

---

### Step 4: UMAP projection engine (`projection.rs`)

The core of the visual experience. Implements dimensionality reduction from 384-dim embedding space to 2D coordinates.

**Algorithm**: simplified UMAP implementation in Rust.

Full UMAP has two phases:
1. **Graph construction**: find k-nearest neighbors, compute fuzzy simplicial set (edge weights from distances)
2. **Layout optimization**: stochastic gradient descent to place points in 2D while preserving the neighbor graph

For HEBBS, phase 1 is nearly free because **the HNSW index already provides approximate nearest neighbors**. We skip the separate kNN computation entirely.

```rust
pub struct ProjectionEngine {
    positions: HashMap<Vec<u8>, (f32, f32)>,   // memory_id -> (x, y)
    params: ProjectionParams,
}

pub struct ProjectionParams {
    pub n_neighbors: usize,        // 15 (how many neighbors influence layout)
    pub min_dist: f32,             // 0.1 (minimum distance in 2D)
    pub spread: f32,               // 1.0
    pub learning_rate: f32,        // 1.0
    pub n_epochs: usize,           // 200 (full recompute), 50 (incremental)
    pub seed: u64,                 // deterministic layout
}
```

**Full projection** (runs on `hebbs panel` first open, or on manual trigger):

1. Fetch all memory embeddings from vectors CF (batch read)
2. For each memory, query HNSW for k-nearest neighbors (reuse existing index, O(log n) per query)
3. Compute edge weights: `w_ij = exp(-max(0, d_ij - rho_i) / sigma_i)` where `rho_i` is distance to nearest neighbor
4. SGD optimization: 200 epochs, attractive force pulls neighbors together, repulsive force pushes non-neighbors apart
5. Normalize to [-1, 1] range
6. Store positions in projections CF (new RocksDB column family)

**Time complexity**:
- kNN from HNSW: O(n * log n) total
- SGD: O(n_epochs * n * n_neighbors) = O(200 * n * 15) = O(3000n)
- Total: O(n log n) dominated by kNN at scale

**Performance targets**:
- 100 memories: <50ms (instant)
- 1K memories: <500ms (acceptable on first open)
- 10K memories: <5s (background, show loading indicator)
- 100K memories: <60s (background, use cached positions while recomputing)

**Incremental projection** (runs when new memories are added):

1. New memory arrives with embedding
2. Query HNSW for its k-nearest neighbors
3. Initialize position as centroid of neighbor positions (O(k))
4. Run 50 SGD epochs with only this node movable (neighbors' positions fixed)
5. Store position in projections CF
6. Broadcast position update via WebSocket

Incremental cost: O(k * n_epochs) = O(15 * 50) = O(750) per new memory. Sub-millisecond.

**Stability**: positions must be stable between sessions. Users build spatial familiarity (the memory palace concept). Full re-layout uses a deterministic seed and only runs when explicitly triggered or when >20% of memories are new since last full layout.

**New RocksDB column family**: `projections`
- Key: `memory_id` (16 bytes)
- Value: `(f32, f32)` (8 bytes) = x, y coordinates
- Also stores cluster assignments: `cluster:{memory_id}` -> `cluster_id`

**Tests**:
- Project 100 random 384-dim vectors: all positions within [-1, 1]
- Similar vectors (cosine > 0.9) placed closer than dissimilar vectors in 2D
- Incremental add: new point placed near its neighbors
- Deterministic: same input + same seed = same output
- Round-trip: save to RocksDB, read back, positions identical

---

### Step 5: Hierarchical clustering (`clustering.rs`)

Produces the "rooms" of the memory palace. Clusters emerge from embedding proximity, not manual tags.

**Algorithm**: agglomerative clustering on 2D projected positions (not 384-dim embeddings, because clustering should match what the user sees).

```rust
pub struct ClusterHierarchy {
    pub levels: Vec<ClusterLevel>,
}

pub struct ClusterLevel {
    pub clusters: Vec<Cluster>,
    pub target_count: usize,    // how many clusters at this level
}

pub struct Cluster {
    pub id: String,
    pub label: String,                    // auto-generated from content
    pub node_ids: Vec<Vec<u8>>,           // memory_ids in this cluster
    pub centroid: (f32, f32),             // average position
    pub radius: f32,                      // bounding circle radius
    pub child_clusters: Vec<String>,      // sub-clusters (next level down)
}
```

**Three levels** for level-of-detail:

| Level | Target cluster count | Used when |
|---|---|---|
| 0 (top) | ~20-50 | >10K nodes, default zoom |
| 1 (mid) | ~100-200 | 500-10K nodes, or zoomed into a top-level cluster |
| 2 (leaf) | individual nodes | <500 nodes, or zoomed into a mid-level cluster |

**Label generation**: for each cluster, collect all memory content, extract the 3 most frequent significant terms (exclude stop words), join as label. Example: "Vendor, Performance, Q3". This is cheap (no LLM) and good enough for navigation. Phase 4 upgrades to LLM-generated labels.

**Recomputation**: runs after full UMAP projection. Incremental: when a new memory is added, assign it to the nearest existing cluster. If a cluster grows beyond 2x its mean size, split it.

**Tests**:
- 100 memories in 3 natural groups: produces 3 clusters at top level
- Each cluster's centroid is near its members' average position
- Labels contain terms from cluster members
- Hierarchy: top-level cluster contains mid-level clusters

---

### Step 6: Graph data assembly (`graph.rs`)

Assembles the full graph response for `/api/panel/graph`.

**Responsibilities**:
1. Read all memory records from storage (paginated for large vaults)
2. Read 2D positions from projections CF
3. Read cluster assignments
4. Compute edges:
   - **Similarity edges**: for each node, its HNSW neighbors with distance < 0.3 (similarity > 0.7)
   - **Wiki-link edges**: `RELATED_TO` edges from graph CF
   - **Insight edges**: `INSIGHT_FROM` edges from graph CF
   - **Causal edges**: `CAUSED_BY`, `FOLLOWED_BY` edges from graph CF
5. Compute visual properties:
   - Node size = importance (0.0 to 1.0, mapped to pixel radius)
   - Node color = recency (recent = bright amber #F59E0B, old = dim amber #78350F)
   - Node brightness = reinforcement (log(1 + access_count) / log(1 + reinforcement_cap))
   - Insight nodes: hexagonal shape flag, amber glow border
6. Return assembled graph with LOD metadata

**Pagination**: for vaults >10K memories, the initial response sends cluster-level data. Individual nodes load on zoom-in via a separate endpoint (`/api/panel/graph/clusters/:id/nodes`).

**Edge filtering**: similarity edges can explode combinatorially. Limit to top-3 strongest edges per node (excluding graph-CF edges which are always included). This keeps the visual clean and the payload bounded.

**Caching**: graph response is cached and invalidated on memory add/remove/modify. Cache key is `(vault_path, last_modified_timestamp)`.

**Tests**:
- Empty vault: returns empty nodes/edges
- 10 memories with known embeddings: correct positions, correct similarity edges
- Insight node has `kind: "insight"` and `confidence` field
- Edge count bounded by 3 * node_count + graph_edges

---

### Step 7: Memory and insight detail endpoints

**`GET /api/panel/memories/:id`**:
1. Read memory record from engine
2. Read content from source file (file-first, same as `query.rs` in hebbs-vault)
3. Compute score breakdown (without a query, relevance is null; recency/importance/reinforcement are always available)
4. Compute decay curve: current score, projected scores at +7d, +30d, +90d
5. Read edges from graph CF (both forward and reverse)
6. Read HNSW neighbors (top 5 by similarity)
7. Look up manifest state (synced/stale/orphaned)

**Projected fade date calculation**:
```
decay(t) = importance * 2^(-(age + t) / half_life_days) * log(1 + access_count)
fade_date = solve for t where decay(t) < auto_forget_threshold
```

Closed-form: `t = half_life_days * log2(importance * log(1 + access_count) / auto_forget_threshold) - current_age`

**`GET /api/panel/insights/:id`**:
Same as memory detail, plus:
- `confidence` field (from memory record or frontmatter)
- `source_ids` (memories connected by `INSIGHT_FROM` edges, resolved to labels)
- `generation_timestamp` (from `created_at`)

**`GET /api/panel/files/:path`**:
1. Validate path is within the active vault directory (security: prevent directory traversal)
2. Read file content from disk
3. Return as plain text with file metadata (size, modified time)

Path validation: resolve the absolute path, verify it starts with the vault's root directory. Reject any path containing `..` or that resolves outside the vault.

**Tests**:
- Get existing memory: all fields present, content matches file
- Get insight: confidence and source_ids present
- Get nonexistent memory: 404
- File path traversal attempt (`../../../etc/passwd`): 400 error
- Decay curve: projected fade date matches manual calculation

---

### Step 8: WebSocket live events

**`/ws/panel`**: WebSocket endpoint for real-time updates.

**Event types**:
```json
{"type": "memory_added", "id": "01JABC...", "x": 0.42, "y": -0.18, "cluster_id": "cluster_003"}
{"type": "memory_updated", "id": "01JABC..."}
{"type": "memory_removed", "id": "01JABC..."}
{"type": "watcher_phase1", "files_parsed": 3}
{"type": "watcher_phase2", "sections_embedded": 12}
{"type": "status_changed", "stale_count": 4, "orphaned_count": 1}
{"type": "vault_switched", "vault_path": "/project-b"}
```

**Source**: the daemon's watcher already processes file events. Add a `broadcast::Sender<PanelEvent>` channel. The watcher sends events after each phase 1/phase 2 completion. The projection engine sends events after incremental position updates.

**Client**: reconnects automatically on disconnect (exponential backoff, max 30s). Displays a "reconnecting..." indicator in the UI.

**Tests**:
- Connect WebSocket, add a memory via engine, receive `memory_added` event
- Disconnect and reconnect: no missed events after reconnect (events are ephemeral, client re-fetches graph on reconnect)

---

### Step 9: Frontend SPA (Phase 1 components)

**`index.html`**: minimal shell, loads app.js as ES module.

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>HEBBS Memory Palace</title>
  <link rel="stylesheet" href="/static/styles/panel.css">
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/static/app.js"></script>
</body>
</html>
```

**`app.js`**: Preact entry point.

```js
import { h, render } from './vendor/preact.module.js';
import { useState, useEffect, useRef } from './vendor/preact-hooks.module.js';
import htm from './vendor/htm.module.js';
import { Palace } from './components/Palace.js';
import { connectWebSocket } from './lib/api.js';

const html = htm.bind(h);

function App() {
  const [vault, setVault] = useState(null);
  const [vaults, setVaults] = useState([]);
  const [status, setStatus] = useState(null);

  useEffect(() => {
    // Load vaults, select initial vault from URL param or auto-detect
    // Connect WebSocket
    // Fetch initial graph
  }, []);

  return html`
    <${Palace}
      vault=${vault}
      vaults=${vaults}
      status=${status}
      onVaultSwitch=${setVault}
    />
  `;
}

render(html`<${App} />`, document.getElementById('app'));
```

**Component tree (Phase 1)**:

```
App
  Palace
    Header (vault dropdown, logo)
    Graph (force-graph WebGL canvas)
    SidePanel (memory/insight detail, opens on node click)
    HealthBadge (stats overlay, bottom-right)
    Legend (node/edge type legend, bottom-left)
```

**`Graph.js`**: wraps force-graph library.

Key behaviors:
- Initialize force-graph on mount with dark background (#0A0A0B)
- Custom node rendering:
  - Episodes: circle, radius proportional to importance, color interpolated from dim amber (#78350F) to bright amber (#F59E0B) by recency
  - Insights: hexagonal shape (drawn via custom canvas path), amber glow border (2px #F59E0B with shadow blur)
  - Brightness: alpha channel modulated by reinforcement signal
- Custom edge rendering:
  - Wiki-link/graph edges: solid line, opacity proportional to confidence
  - Similarity edges: dashed line, opacity proportional to weight
- Click handler: calls `onNodeClick(nodeId)` to open side panel
- Hover handler: shows tooltip with label + kind
- Zoom handler: at zoom level <0.3, switch to cluster view; >0.3, show individual nodes
- WebSocket integration: on `memory_added`, add node to graph with animation; on `memory_removed`, remove with fade

**`SidePanel.js`**: slides in from right on node click.

Content for episode memories:
- File path (clickable, triggers file content display)
- Heading path breadcrumb
- Content preview (first 500 chars, expandable)
- Score breakdown: recency, importance, reinforcement as horizontal bars with values
- Decay curve: current score, projected fade date
- Access count and last accessed timestamp
- Edges: listed with type badges, clickable (highlights target on graph)
- "Recall from here" button (Phase 2)

Content for insights (additional):
- Confidence score (prominently displayed)
- Source memories (clickable list, highlights on graph)
- Generation timestamp

**`HealthBadge.js`**: always-visible overlay.

```
847 memories | 23 insights | 89% synced | 4 stale | 1 orphaned | 3 decaying
```

Polls `/api/panel/status` every 30s, updates immediately on WebSocket `status_changed` events.

**`Legend.js`**: collapsible legend overlay.

Shows: circle = episode, hexagon = insight, solid line = wiki-link, dashed line = similarity. Color scale from dim to bright = recency. Size = importance.

---

### Step 10: CSS theme and brand

**`panel.css`**: HEBBS brand dark theme.

```css
:root {
  --bg-primary: #0A0A0B;
  --bg-secondary: #141415;
  --bg-tertiary: #1E1E20;
  --text-primary: #E5E5E5;
  --text-secondary: #9CA3AF;
  --text-muted: #6B7280;
  --amber-bright: #F59E0B;
  --amber-mid: #D97706;
  --amber-dim: #78350F;
  --amber-glow: rgba(245, 158, 11, 0.3);
  --border: #2D2D30;
  --error: #EF4444;
  --success: #10B981;
  --radius: 8px;
  --font-mono: 'JetBrains Mono', 'Fira Code', monospace;
  --font-sans: 'Inter', -apple-system, system-ui, sans-serif;
}
```

Font loading: system fonts with fallbacks. No external font requests (local-first constraint). If Inter or JetBrains Mono are installed, great; otherwise system fonts.

Responsive: the panel is primarily a desktop tool (developers use it alongside their editor), but the layout should not break on tablet-width screens. Below 768px, the side panel becomes a bottom sheet.

---

## Phase 2: Search Overlay + Weight Sliders + Filters + Strategies

### Step 11: Recall endpoint with score decomposition

Implement `POST /api/panel/recall`.

**Handler**:
1. Parse request: query text, weights, strategies, top_k, filters
2. Embed query text via engine's embedder
3. For each selected strategy, run recall:
   - Similarity: HNSW search with custom weights
   - Temporal: time-range query with entity scope
   - Causal: graph traversal from seed (requires seed_id in request)
   - Analogical: hybrid embedding + structural
4. Merge results across strategies (deduplicate by memory_id, keep highest score)
5. Apply filters (state, file_path, importance range)
6. For each result, compute full score decomposition: each signal's raw value, weight, and weighted contribution
7. Record latency (start to finish, microsecond precision)
8. Return results with latency

**Custom weights**: the recall request accepts a `ScoringWeights` override. This does NOT modify the vault's config; it is per-request. The engine's `recall_for_tenant` already accepts scoring weights.

**Tests**:
- Recall with default weights: results match engine recall
- Recall with custom weights (w_recency=1.0, others=0.0): most recent memory ranked first
- Strategy toggle: similarity-only vs temporal-only return different orderings
- Latency field present and reasonable (<100ms for 1K memories)
- Filter by state: only matching states returned

---

### Step 12: Search bar + graph highlighting

**`Header.js` update**: add search input field.

On query input (debounced 300ms):
1. Call `/api/panel/recall` with current weights, strategies, filters
2. Receive result set with memory_ids and scores
3. Pass to Graph component

**`Graph.js` update**: search highlighting mode.

When search results are active:
- Matching nodes glow amber (#F59E0B) with intensity proportional to composite score
- Non-matching nodes fade to 20% opacity (gray)
- Edges connected to matching nodes remain visible; others fade
- Results listed in side panel (replacing memory detail) with full score bars

When search is cleared: all nodes return to normal rendering.

**Latency display**: show `3.2ms` badge next to search bar after each recall.

---

### Step 13: Weight sliders

**`Sliders.js`**: four horizontal sliders in a collapsible panel below the search bar.

| Slider | Label | Range | Default | Maps to |
|---|---|---|---|---|
| Relevance | Semantic match | 0.0-1.0 | 0.50 | w_relevance |
| Recency | How recent | 0.0-1.0 | 0.20 | w_recency |
| Importance | Intrinsic value | 0.0-1.0 | 0.20 | w_importance |
| Reinforcement | Access frequency | 0.0-1.0 | 0.10 | w_reinforcement |

On slider change (debounced 150ms): re-run the current search query with updated weights. Graph highlighting updates in real-time.

**Normalization**: weights do not need to sum to 1.0. The engine normalizes internally. But the UI shows the effective percentage: `w_i / sum(weights) * 100%`.

**Preset buttons**: `[Pure relevance]` sets (1.0, 0, 0, 0). `[Recency boost]` sets (0.3, 0.5, 0.1, 0.1). `[High importance]` sets (0.2, 0.1, 0.6, 0.1). Clicking a preset updates sliders and re-runs search.

---

### Step 14: Strategy toggles and top_k

**Strategy toggles**: four checkboxes next to the sliders.

```
[x] Similarity  [x] Temporal  [ ] Causal  [ ] Analogical
```

Default: Similarity and Temporal checked. Changing toggles re-runs the search.

Causal requires a seed memory. If Causal is toggled on and no memory is selected, show tooltip: "Click a memory to use as causal seed."

**top_k selector**: dropdown with values 5, 10, 20, 50. Default: 10. Changes re-run search.

---

### Step 15: Filter bar

**`Filters.js`**: collapsible bar below strategy toggles.

| Filter | Type | Options |
|---|---|---|
| State | Dropdown | All, Active (synced), Stale, Orphaned, Decaying |
| File | Dropdown | All, then list of all source files from manifest |
| Importance | Range slider | 0.0 to 1.0, dual-thumb |

Filters apply to both graph highlighting and side panel results. When a filter is active, non-matching nodes are hidden (not just faded).

**Implementation**: filters are applied client-side on the already-fetched graph data for State and Importance. File filter is passed to the server in the recall request (avoids loading all file lists client-side for huge vaults).

---

### Step 16: Decay monitor + health detail

**Decay monitor toggle**: button in the header: `[Show decay]`.

When active:
- Nodes with `decay_score < auto_forget_threshold * 10` turn red (translucent, #EF4444 at 40% opacity)
- Nodes with `decay_score < auto_forget_threshold * 2` pulse red (animation)
- Stats badge updates: "12 below threshold, 3 auto-forget candidates"
- All other visual modes (search highlighting, filters) are paused

**`GET /api/panel/health`**: returns detailed health data.

```json
{
  "stale_files": [
    {"path": "notes/meeting.md", "sections_stale": 2, "last_modified": 1710000000}
  ],
  "orphaned_memories": [
    {"memory_id": "01JABC...", "original_file": "deleted-file.md", "content_preview": "The vendor..."}
  ],
  "decay_candidates": [
    {"memory_id": "01JABD...", "decay_score": 0.02, "projected_fade_days": 5, "label": "Old meeting notes"}
  ]
}
```

**`POST /api/panel/health/actions`**: handles re-index, dismiss, reinforce.

```json
{"action": "reindex", "file_path": "notes/meeting.md"}
{"action": "dismiss", "memory_id": "01JABC..."}
{"action": "reinforce", "memory_id": "01JABD...", "importance_boost": 0.2}
```

- `reindex`: triggers phase 1 + phase 2 for the specified file
- `dismiss`: calls `engine.forget()` for the memory, removes from manifest
- `reinforce`: increments access_count (which boosts reinforcement signal and decay score)

**Health badge click-through**: clicking the health badge opens a health detail panel (replaces side panel content) listing stale files, orphaned memories, and decay candidates with action buttons.

---

## Phase 3: Timeline Scrubber + Growth

### Step 17: Timeline data endpoint

**`GET /api/panel/timeline`**: aggregates temporal data for the scrubber.

```json
{
  "range": {"start": 1709000000, "end": 1710100000},
  "daily_counts": [
    {"date": "2026-03-01", "memories_added": 15, "insights_added": 2, "memories_forgotten": 0},
    {"date": "2026-03-02", "memories_added": 8, "insights_added": 1, "memories_forgotten": 1}
  ],
  "growth": {
    "total_memories": 847,
    "total_insights": 23,
    "avg_composite_score": 0.72,
    "daily_delta_memories": 12,
    "daily_delta_score": 0.03
  },
  "events": [
    {"timestamp": 1710000000, "type": "added", "memory_id": "01JABC..."},
    {"timestamp": 1710050000, "type": "forgotten", "memory_id": "01JABD..."}
  ]
}
```

**Implementation**: scan all memories by `created_at` timestamp, bucket by day. For forgotten memories, this requires tracking forget events. Add a `forget_log` key prefix in the meta CF: `forget_log:{timestamp}:{memory_id}` -> `{}`. Written by `engine.forget()`. Bounded: auto-pruned after 90 days.

**`GET /api/panel/timeline/snapshot?at=1709500000`**: returns the set of memory_ids that existed at a given timestamp. Used by the scrubber to show the graph at a point in time.

Implementation: all memories with `created_at <= at` minus all memories forgotten before `at` (from forget_log). Return as a set of memory_ids that the client uses to filter the cached graph nodes.

---

### Step 18: Timeline scrubber component

**`Timeline.js`**: horizontal slider at the bottom of the panel.

**Scrubber bar**: full width, range from vault creation date to now. Dragging left shows older state; releasing snaps to current.

**Behavior on drag**:
1. Debounce 100ms
2. Call `/api/panel/timeline/snapshot?at={timestamp}`
3. Receive memory_id set
4. Graph.js: show only nodes in the set, others hidden with fade animation
5. Nodes appear/disappear as the user drags, creating the "brain growing" effect

**Growth sparklines**: small inline charts next to the scrubber.
- Memory count over time (line chart)
- Insight count over time (line chart)
- Average composite score trend (line chart)
- Daily delta badge: "+12 today, +0.03 avg score"

Sparklines rendered via canvas (no charting library; sparklines are ~30 lines of canvas code).

---

### Step 19: Forget log integration

**Engine change**: modify `engine.forget()` to write a forget_log entry before deleting the memory.

```rust
// In hebbs-core/src/engine.rs, forget() method
fn forget(&self, memory_id: &[u8], tenant: &str) -> Result<()> {
    let now = current_time_us();
    let key = format!("forget_log:{}:{}", now, hex::encode(memory_id));
    // Write to meta CF in the same WriteBatch as the delete
    batch.put_cf(meta_cf, key.as_bytes(), &[]);
    // ... existing forget logic
}
```

**Pruning**: on daemon startup, delete forget_log entries older than 90 days. This keeps the meta CF bounded.

**Tests**:
- Forget a memory, verify forget_log entry exists
- Timeline snapshot at time before forget: memory appears
- Timeline snapshot at time after forget: memory absent
- Prune: entries older than 90 days removed

---

## Phase 4: Config Editor + Persistence + Polish

### Step 20: Config endpoints

**`GET /api/panel/config`**: reads active vault's `.hebbs/config.toml` and returns as structured JSON.

```json
{
  "scoring": {
    "w_relevance": 0.5,
    "w_recency": 0.2,
    "w_importance": 0.2,
    "w_reinforcement": 0.1
  },
  "decay": {
    "half_life_days": 30,
    "auto_forget_threshold": 0.01,
    "reinforcement_cap": 100
  },
  "watcher": {
    "phase1_debounce_ms": 500,
    "phase2_debounce_ms": 3000,
    "burst_threshold": 20,
    "burst_debounce_ms": 5000
  },
  "embedding": {
    "model": "bge-small-en-v1.5",
    "dimensions": 384,
    "ef_search": 50,
    "ef_construction": 200
  },
  "chunking": {
    "split_on": "##",
    "min_section_length": 50
  },
  "ignore_patterns": [".hebbs/", ".git/", ".obsidian/", "node_modules/"]
}
```

**`PUT /api/panel/config`**: writes changes to config.toml. Accepts partial updates (only the fields present in the request body are modified).

Validation:
- Weights must be >= 0
- half_life_days must be > 0
- ef_search must be >= 1
- ignore_patterns must be valid glob patterns

After writing, notify the daemon to reload the config for this vault. The watcher picks up new debounce values. The engine picks up new scoring weights as defaults.

**`POST /api/panel/config/reset`**: overwrites config.toml with factory defaults.

**`GET /api/panel/config/export`**: returns config.toml as a downloadable file (Content-Disposition: attachment).

**Tests**:
- Read config: matches file on disk
- Write partial config: only specified fields changed, others preserved
- Invalid values: 400 error with field-specific message
- Reset: config matches factory defaults
- Export: valid TOML that can be parsed back

---

### Step 21: Config editor component

**`ConfigEditor.js`**: opens in side panel when gear icon is clicked.

Sections matching the config structure:

**Scoring weights**: four sliders (same style as Phase 2 recall sliders, but these persist to config.toml).

**Decay settings**: number inputs for half_life_days, auto_forget_threshold, reinforcement_cap. Each has a tooltip explaining the parameter in plain language.

**Watcher settings**: number inputs for all debounce values.

**Embedding info** (read-only): model name, dimensions, ef_search (editable), ef_construction (editable).

**Ignore patterns**: list with [x] remove buttons and [Add pattern] input.

**Actions row**:
- [Save] writes to config.toml via PUT
- [Reset to factory] confirms then calls POST reset
- [Export] triggers file download

All inputs have plain-language tooltips:
- `half_life_days`: "How many days until a memory's importance halves. Lower = faster forgetting."
- `ef_search`: "Higher values improve search accuracy but increase latency. Range: 10-500."
- `burst_threshold`: "Number of file changes in quick succession that triggers burst mode."

---

### Step 22: Spatial position persistence

Positions already persist in the projections CF (Step 4). This step ensures they survive across sessions and vault switches.

**Behavior**:
- On panel open: load positions from projections CF. If positions exist, render immediately (no recompute). If absent (first open), run full projection.
- On vault switch: cache current vault's positions (already in RocksDB), load new vault's positions.
- On user drag (if we enable manual node repositioning): save dragged position to projections CF, mark as "pinned" so UMAP does not override it on re-layout.
- On full re-layout: preserve pinned positions, only recompute unpinned nodes.

**Pinned positions**: stored as `pinned:{memory_id}` -> `(f32, f32)` in projections CF. Full re-layout treats pinned nodes as fixed constraints.

---

### Step 23: Cluster auto-labeling (LLM upgrade)

Phase 1-3 use term-frequency labels (Step 5). Phase 4 upgrades to LLM-generated labels for clusters.

**Implementation**: when reflect is enabled (LLM config present), after clustering:
1. For each cluster, sample up to 5 representative memories (closest to centroid)
2. Send to LLM: "Given these related notes, generate a 2-4 word label for this group."
3. Cache labels in projections CF: `cluster_label:{cluster_id}` -> label string
4. Labels regenerate when cluster membership changes by >30%

If reflect is not configured, fall back to term-frequency labels. No LLM dependency for basic panel functionality.

---

### Step 24: Graph export

**Export button** in header: `[Export PNG]` / `[Export SVG]`.

**PNG**: use `HTMLCanvasElement.toBlob()` on the force-graph's WebGL canvas. Add a title overlay and legend before export.

**SVG**: not natively supported by WebGL. For SVG export, re-render the current view using a temporary SVG layer (d3 SVG renderer) with the same node positions. This is a one-time render, not the live view.

Both formats: 1200x630 at 2x DPI for social sharing (the "Spotify Wrapped moment").

---

## Dependency Graph

```
Step 1  (crate scaffold)    ──> Step 2 (vaults) ──> Step 3 (status)
                              └──> Step 4 (UMAP) ──> Step 5 (clustering) ──> Step 6 (graph)
                              └──> Step 7 (memory detail)
                              └──> Step 8 (WebSocket)
                              └──> Step 9 (frontend SPA) ──> Step 10 (CSS)
                                        ^^^ Phase 1 ^^^

Step 11 (recall endpoint)   ──> Step 12 (search + highlight)
                              └──> Step 13 (sliders) ──> Step 14 (strategies + top_k)
                              └──> Step 15 (filters)
                              └──> Step 16 (decay monitor + health detail)
                                        ^^^ Phase 2 ^^^

Step 17 (timeline data)     ──> Step 18 (scrubber component)
Step 19 (forget log)        ──/
                                        ^^^ Phase 3 ^^^

Step 20 (config endpoints)  ──> Step 21 (config editor)
Step 22 (position persist)
Step 23 (cluster labels)
Step 24 (graph export)
                                        ^^^ Phase 4 ^^^
```

Within each phase, most steps can be parallelized. Steps 4-8 are independent of each other and can be built in parallel. Steps 11-16 are independent. Steps 20-24 are independent.

Between phases: Phase 2 depends on Phase 1 (needs graph rendering). Phase 3 depends on Phase 2 (needs the filter/highlight infrastructure). Phase 4 is mostly independent of Phase 3 (Steps 20-22 could start as soon as Phase 1 is complete).

---

## New Dependencies (requires `cargo audit` validation)

| Crate | Purpose | Justification |
|---|---|---|
| `include_dir` | Embed static SPA files in binary | Standard approach for single-binary web UIs. Small, no transitive deps. |
| `tower-http` | Static file serving, CORS, compression | Already in axum ecosystem. Needed for proper MIME types and gzip. |
| `tokio-tungstenite` | WebSocket support | Standard async WebSocket for tokio/axum. Already a transitive dep of axum. |

Frontend vendor libraries (no Cargo deps, vendored as JS files):
- `preact` 10.x (~4KB gzipped)
- `preact/hooks` (bundled with preact)
- `htm` 3.x (~700B gzipped)
- `force-graph` 1.x (~50KB gzipped, includes d3-force)

No UMAP crate dependency. The projection algorithm is implemented directly in `projection.rs` (~300 lines). The kNN phase reuses the existing HNSW index, and the SGD layout phase is straightforward.

---

## New RocksDB Column Family

**`projections`**: stores 2D coordinates, cluster assignments, and cluster labels.

| Key pattern | Value | Purpose |
|---|---|---|
| `pos:{memory_id}` | `(f32, f32)` bitcode | 2D position |
| `pinned:{memory_id}` | `(f32, f32)` bitcode | User-pinned position (Phase 4) |
| `cluster:{memory_id}` | `cluster_id` bytes | Cluster assignment |
| `cluster_meta:{cluster_id}` | `ClusterMeta` bitcode | Centroid, radius, label, child IDs |
| `cluster_label:{cluster_id}` | UTF-8 string | LLM-generated label (Phase 4) |
| `projection_meta` | `ProjectionMeta` bitcode | Last full layout timestamp, seed, node count |

Adding a column family requires updating `RocksDbBackend::open()` in `hebbs-storage` to include `"projections"` in the CF list. Existing databases auto-migrate (RocksDB creates new CFs on open if they do not exist, no data migration needed).

---

## Engine Changes Summary

| Change | Crate | Phase | Description |
|---|---|---|---|
| Projections CF | `hebbs-storage` | 1 | Add "projections" column family to RocksDB open |
| Forget log | `hebbs-core` | 3 | Write forget_log entry in meta CF before deleting memory |
| Forget log pruning | `hebbs-core` | 3 | Prune entries >90 days on startup |
| Score decomposition | `hebbs-core` | 2 | Expose per-signal raw values in recall response (may already be available internally) |

These changes are minimal and backward-compatible. No changes to proto, gRPC, REST v1, or any SDK.

---

## Principles Compliance

| Principle | How this plan complies |
|---|---|
| P1 (Hot Path) | Panel serves cached projections and pre-assembled graph. No UMAP computation on the read path. Recall uses existing engine path. |
| P2 (Single Binary) | SPA compiled into binary via `include_dir!`. No external processes, no npm, no build step at runtime. |
| P3 (Cognition) | Graph visualization IS the cognition layer made visible. Insights, decay, reinforcement all surfaced. |
| P4 (Bounded) | Edge count bounded (top-3 similarity per node). Pagination for >10K nodes. Timeline limited to 90 days. Cluster hierarchy capped at 3 levels. |
| P5 (Background) | UMAP projection runs in background. Incremental updates are O(750) per memory. Full re-layout is async with cached results served during recompute. |
| P6 (Lineage) | Insight source memories shown with clickable links. Graph edges show all relationship types. |
| P9 (Measure) | Recall latency displayed per query. Status endpoint exposes sync %, stale/orphaned/decay counts. |
| P10 (API Elegance) | Panel endpoints are panel-specific (`/api/panel/*`), do not pollute the engine API. No new operations added to the engine. |
| P11 (Correctness) | Projections stored in same RocksDB instance (WriteBatch atomicity). Config writes validated before persisting. File path traversal prevented. |
| P12 (Security) | Bound to 127.0.0.1 only. File endpoint validates paths within vault. No secrets in panel responses. No external network requests from SPA. |

---

## Relative Sizing

| Step | Size | Phase | Notes |
|------|------|-------|-------|
| 1. Crate scaffold | Small | 1 | Cargo.toml, lib.rs, include_dir setup |
| 2. Vault endpoints | Small | 1 | Read vaults.json, call vault_manager |
| 3. Status endpoint | Small | 1 | Manifest scan, caching |
| 4. UMAP projection | Large | 1 | Core algorithm, HNSW integration, incremental updates |
| 5. Hierarchical clustering | Medium | 1 | Agglomerative clustering, label generation |
| 6. Graph data assembly | Medium | 1 | Node/edge assembly, LOD, caching |
| 7. Memory/insight detail | Medium | 1 | Score decomposition, decay curve, file reads |
| 8. WebSocket events | Small | 1 | Broadcast channel, event types |
| 9. Frontend SPA | Large | 1 | Preact app, force-graph, side panel, all Phase 1 components |
| 10. CSS theme | Small | 1 | Dark theme, brand colors |
| 11. Recall endpoint | Medium | 2 | Score decomposition, multi-strategy, filters |
| 12. Search + highlighting | Medium | 2 | Graph glow mode, result list |
| 13. Weight sliders | Small | 2 | Slider UI, debounced re-query |
| 14. Strategy toggles + top_k | Small | 2 | Checkboxes, dropdown, re-query |
| 15. Filter bar | Medium | 2 | State/file/importance filters, client + server side |
| 16. Decay monitor + health | Medium | 2 | Decay visualization, health detail panel, actions |
| 17. Timeline data | Medium | 3 | Temporal aggregation, snapshot endpoint |
| 18. Scrubber component | Medium | 3 | Slider, sparklines, graph filtering animation |
| 19. Forget log | Small | 3 | Engine change, pruning |
| 20. Config endpoints | Medium | 4 | Read/write/validate config.toml |
| 21. Config editor | Medium | 4 | Full form UI, tooltips, actions |
| 22. Position persistence | Small | 4 | Pinned positions, drag handling |
| 23. Cluster auto-labeling | Small | 4 | LLM call, cache, fallback |
| 24. Graph export | Small | 4 | Canvas toBlob, SVG fallback |
