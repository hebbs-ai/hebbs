# TASK-16: Memory Palace Control Panel

Parent: [TASK-12](./TASK-12-markdown-obsidian-cognitive-layer.md)
Analysis: [ANALYSIS_TASK12_UX_CONTROLPANEL.md](./ANALYSIS_TASK12_UX_CONTROLPANEL.md)

## Goal

Ship a visual control panel (`hebbs panel`) embedded in the hebbs binary that makes the engine's memory visible, interactive, and trustworthy. The memory graph is the home view. Everything else opens as panels within it. The user sees their memory palace the moment they open the panel.

---

## Core Concept: Memory Palace

The method of loci (memory palace) works because spatial memory is the strongest memory system humans have. Three days after hearing information, people remember 10%. Place it in a spatial context and recall jumps to 65%.

HEBBS's engine already creates the palace. UMAP/t-SNE projection of the 384-dim embedding space produces natural spatial clusters: Rust notes cluster together, meeting notes cluster together, architecture docs cluster together. These clusters ARE the rooms. They emerge from meaning, not from folders or manual links.

The control panel makes this visible. The graph is not a feature the user navigates to. It is where the user lives.

---

## Vault Navigation

The panel always reads `~/.hebbs/vaults.json` for the list of registered vaults. Every `hebbs init <path>` adds that path to the registry. The panel opens with one vault selected and provides a dropdown to switch to any other registered vault without restarting.

**Default vault selection:**

- `hebbs panel` inside a project dir with `.hebbs/`: opens with that project vault selected
- `hebbs panel --global`: opens with the global vault (`~/.hebbs/`) selected
- `hebbs panel` outside any project: opens with the global vault selected

**Vault registry (`~/.hebbs/vaults.json`):**

```json
{
  "vaults": [
    { "path": "/Users/jasen/.hebbs", "label": "global" },
    { "path": "/Users/jasen/projects/foo", "label": "foo" },
    { "path": "/Users/jasen/projects/bar", "label": "bar" },
    { "path": "/Users/jasen/notes", "label": "notes" }
  ]
}
```

The panel header shows a dropdown:

```
+------------------------------------------------------------------+
|  HEBBS Memory  [vault: foo v]  [Search: ____________]            |
+------------------------------------------------------------------+
```

Switching vaults reloads the graph and all data from the selected vault's engine. No restart needed; the panel's API endpoints accept a vault path parameter, and the backend opens the corresponding engine.

---

## Architecture

The palace is one primary surface with contextual panels:

```
+------------------------------------------------------------------+
|  HEBBS Memory  [vault: foo v]                                    |
|  [Search: ___________________]  [Sliders: rel/rec/imp/rnf]      |
|  Strategies: [x]Sim [x]Temp [ ]Causal [ ]Analog  top_k:[5v]     |
|  Filters: [State: All v] [File: All v] [Importance: 0.0--1.0]   |
|                                                                  |
|  +-------------------------------+  +------------------------+  |
|  |                               |  |  SIDE PANEL            |  |
|  |     MEMORY GRAPH (home)       |  |  (memory explorer,     |  |
|  |                               |  |   score breakdown,     |  |
|  |  Clusters = rooms             |  |   file content,        |  |
|  |  o  Memory section            |  |   config editor)       |  |
|  |  *o* Insight (AI-generated)   |  |                        |  |
|  |  Size = importance            |  |  Opens on node click   |  |
|  |  Color = recency              |  |  Closes on escape      |  |
|  |  Brightness = reinforcement   |  |                        |  |
|  |  --- Wiki-link edge           |  +------------------------+  |
|  |  ... Similarity edge (>0.7)   |                               |
|  |                               |  +------------------------+  |
|  |  [Health: 89% synced          |  | STATS BADGE            |  |
|  |   4 stale | 1 orphaned        |  | 847 memories           |  |
|  |   3 decay candidates]         |  | 23 insights            |  |
|  |                               |  | Latency: 3.2ms         |  |
|  +-------------------------------+  +------------------------+  |
|  [|<---  Timeline scrubber  --->|]  [Growth: +12 today, +0.03]  |
+------------------------------------------------------------------+
```

### What Collapsed

| Original plan (ANALYSIS 12) | Palace equivalent | Nothing lost? |
|---|---|---|
| View 1: Dashboard (separate page) | Health badge + stats area, bottom-right corner of graph. Stale/orphaned/decay counts visible. Click-through to health detail panel. | Yes: all health monitoring preserved |
| View 2: Memory Explorer (separate page) | Side panel on node click + filter bar (state/file/importance). Score math visible per-signal. | Yes: filters and score breakdown preserved |
| View 3: Recall Playground (separate page) | Search bar + weight sliders + strategy toggles + top_k + latency display overlaid on graph | Yes: strategies, parameters, and latency preserved |
| View 4: Memory Graph (a tab) | THE home view. Insight nodes visually distinct. Legend for node/edge types. | Yes: insight distinction and legend added |
| View 5: Timeline (separate page) | Scrubber bar + growth sparklines + decay monitor toggle | Yes: growth charts and decay overview preserved |
| View 6: Engine Config (separate page) | Full config editor in side panel: weights, decay, watcher, embedding, ignore patterns, reset/export | Yes: all parameters and actions preserved |

Six views collapse to one surface. Every piece of information from the original analysis is accessible. Nothing was lost; it was reorganized.

---

## Key Interactions

### 1. Open the panel

```bash
hebbs panel
```

Browser opens. User sees their memory graph immediately. Not stats. Not a dashboard. The memory palace. Clusters labeled. Importance and recency visible. Vault dropdown in the header shows which vault they're viewing. The "whoa" moment is the first thing they see.

### 2. Explore clusters (rooms)

Clusters form naturally from embedding proximity. Each cluster is labeled (most frequent heading terms or tags in that group). Click a cluster to zoom in and see individual memory nodes. Zoom out to see the whole palace. Spatial positions persist between sessions (users build spatial familiarity).

### 3. Click a memory

Side panel opens showing:
- File path and heading (click to open file)
- Content preview
- Score breakdown with math: `relevance 0.87 x 0.50 = 0.435` per signal (visual bar)
- Composite score total
- Decay curve (current decay score, projected fade date)
- Access count and last accessed time
- Wiki-links (if any, shown as overlay edges)
- "Recall from here" button (uses this memory as context)

### 3b. Click an insight

Insights are AI-generated memories. Side panel shows everything above plus:
- Confidence score (e.g., 0.82)
- Source memories that generated this insight (clickable, highlights source nodes on graph)
- Generation timestamp
- Distinct visual treatment on graph: hexagonal node shape, amber glow border

### 4. Search and tune

Search bar overlaid on the graph. Type a query:
- Matching nodes glow amber
- Non-matching nodes fade to gray
- Results ranked in side panel with full score decomposition (per-signal math visible)
- Latency displayed per recall (e.g., `3.2ms`) to build confidence in speed
- Drag the weight sliders (relevance/recency/importance/reinforcement) and watch glow patterns shift in real-time

**Strategy toggles** next to weight sliders:
- [x] Similarity  [x] Temporal  [ ] Causal  [ ] Analogical
- Select which recall strategies to combine for this query
- Exposes all four HEBBS strategies as a core differentiator

**Result count:** `top_k` selector (5, 10, 20) controls how many results to return.

**Filters** (collapsible bar below search):
- State: [All | Active | Stale | Orphaned | Decaying]
- File: dropdown of all source files
- Importance range: slider from 0.0 to 1.0
- Filters apply to both graph highlighting and side panel results

Preset buttons: [Pure relevance] [Recency boost] [High importance]

### 5. Timeline scrubber

Slider at the bottom. Drag left to see yesterday's brain. Drag further for last week. Nodes appear and disappear as the vault grew. The palace grows before your eyes.

**Growth sparkline** next to scrubber shows aggregate trends:
- Memory count over time (e.g., `0 -> 847 over 3 days`)
- Insight count over time
- Average composite score trend (e.g., `0.45 -> 0.72`)
- Daily delta badge (e.g., `+12 today`)

These answer "how fast is my brain growing?" and "is my memory quality improving?" at a glance.

### 5b. Decay monitor

Toggle "Show decay" mode on the graph:
- Nodes at risk of auto-forget turn red/translucent
- Stats badge updates to show: `12 below threshold, 3 auto-forget candidates`
- Click a decaying node to review in side panel
- Side panel shows projected fade date and option to reinforce (boost importance) or dismiss

This is vault-wide decay health, not just per-memory. No competitor shows this. Users see their brain naturally forgetting, just like a real brain, and can intervene.

### 6. Health badge

Always visible in the bottom-right stats area. Shows at a glance:
- Total memories and insights count
- Sync percentage with health bar
- Content-stale count (files changed since last index)
- Orphaned memory count (memories whose source file was deleted)
- Decay candidates count (memories approaching auto-forget threshold)

Clicking the health badge opens a health detail panel listing stale files, orphaned memories, and decay candidates with actions: re-index, dismiss, reinforce.

This directly addresses the #1 user fear from research: silent data loss. The health badge is the "your brain is intact" signal.

### 7. Configure

Gear icon in side panel opens config editor:

**Scoring weights** (sliders with plain-language labels):
- `w_relevance`: semantic match strength [slider] 0.50
- `w_recency`: how recently accessed [slider] 0.20
- `w_importance`: intrinsic value [slider] 0.20
- `w_reinforcement`: access frequency [slider] 0.10

**Decay settings:**
- `half_life_days`: [30]
- `auto_forget_threshold`: [0.01]
- `reinforcement_cap`: [100]

**Watcher settings:**
- `phase1_debounce`: [500ms]
- `phase2_debounce`: [3000ms]
- `burst_threshold`: [20 events]
- `burst_debounce`: [10000ms]

**Embedding info** (read-only display):
- Model: BGE-small-en-v1.5 (ONNX, 384-dim)
- `ef_search`: [50] (tunable)
- `ef_construction`: [200] (tunable)

**Ignore patterns:**
- Current list: `.hebbs/`, `.git/`, `node_modules/`, `.obsidian/`
- [Add pattern] [Remove]

**Actions:**
- [Save as default] writes to config.toml
- [Reset to factory] restores built-in defaults
- [Export config.toml] downloads current config

All changes write to config.toml. Every parameter has a tooltip explaining what it does in plain language.

---

## Why the Engine Makes Tagging Unnecessary

Obsidian's graph shows manual links: connections the user explicitly created. If two notes are never linked, they're invisible to each other.

HEBBS's engine already knows:
- Every memory's position in 384-dim embedding space (semantic proximity)
- Composite scores across four signals
- Which memories get co-recalled (reinforcement patterns)
- Temporal clustering (what was written together)
- Decay curves (what's fading, what's strengthening)

The HNSW index IS the spatial map. Similarity edges are computed from the engine's own distance metric, not from file tags. The graph shows what the engine discovered, not what the user manually connected. This is more powerful and more honest.

Wiki-links add value as a human-intent signal overlay (toggle on/off), but the engine's understanding is the primary layer.

---

## Technical Approach

### Embedded in the binary

```
hebbs binary
  +-- panel command
       +-- Reads ~/.hebbs/vaults.json for vault registry
       +-- Embedded HTTP server (axum, binds 127.0.0.1)
       +-- Static SPA (compiled in via include_dir!)
       +-- WebSocket for live watcher events
       +-- Can open any registered vault's engine on demand
```

- Zero setup. `hebbs panel` opens a browser.
- Cross-platform. Same binary on macOS, Linux, Windows.
- Local-first by construction. No network exposure. No cloud.
- Vault switcher: dropdown populated from registry, switch without restart.

### API endpoints

| Endpoint | Method | Returns |
|---|---|---|
| `/api/vaults` | GET | Registered vaults from `~/.hebbs/vaults.json` |
| `/api/vaults/switch` | POST | Switch active vault (opens new engine) |
| `/api/status` | GET | Active vault stats, sync state, last activity |
| `/api/memories` | GET | Paginated memory list with scores |
| `/api/recall` | POST | Recall results with full score breakdown |
| `/api/graph` | GET | UMAP-projected nodes + similarity edges |
| `/api/timeline` | GET | Event history for scrubber |
| `/api/config` | GET/PUT | Read/write config.toml |
| `/api/files/{path}` | GET | Source file content |
| `/ws` | WebSocket | Live watcher events |

### Frontend

- D3.js or force-graph (WebGL) for the memory graph
- UMAP projection computed server-side, cached
- Vanilla JS or Preact (minimal, no build step)
- CSS: dark theme matching HEBBS brand (#0A0A0B bg, #F59E0B amber)

---

## Phased Delivery

### Phase 1: Graph home + side panel + vault navigation

- `hebbs panel` command, embedded HTTP server
- Vault registry (`~/.hebbs/vaults.json`), auto-populated by `hebbs init`
- Vault dropdown in header, switch between vaults without restart
- Memory graph as home (UMAP projection of all memories)
- **Insight nodes visually distinct** (hexagonal shape, amber glow border, confidence score on hover)
- **Graph legend** showing node types (memory vs insight) and edge types (wiki-link vs similarity)
- Click node to open side panel with score breakdown (per-signal math) and content
- Click insight node to see confidence score and source memories
- **Health badge** in stats area: memory count, insight count, sync %, stale count, orphaned count
- Target: MVP

### Phase 2: Search overlay + weight sliders + filters + strategies

- Search bar on graph, matching nodes glow
- Drag sliders, watch glow patterns shift
- **Strategy toggles**: Similarity, Temporal, Causal, Analogical checkboxes
- **top_k selector** (5, 10, 20)
- **Latency display** per recall result
- **Filter bar**: state (All/Active/Stale/Orphaned/Decaying), file dropdown, importance range slider
- Preset weight profiles
- **Decay monitor**: toggle "Show decay" mode, at-risk nodes turn red/translucent, vault-wide decay stats
- **Health badge click-through**: stale files list, orphaned memories, decay candidates with actions
- Target: +1 phase after Phase 1

### Phase 3: Timeline scrubber + growth

- Bottom slider showing brain evolution
- Nodes appear/disappear as you scrub time
- **Growth sparklines**: memory count over time, insight count, avg score trend
- **Daily delta badge** (e.g., +12 today, +0.03 avg score)
- Target: +1 phase after Phase 2

### Phase 4: Config editor + persistence + polish

- **Full config editor** in side panel: scoring weights, decay settings (half_life, threshold, reinforcement_cap), watcher settings, embedding info, ignore patterns
- **ef_search and ef_construction** tunable from config panel
- **[Reset to factory]** and **[Export config.toml]** actions
- Spatial positions persist between sessions
- Cluster auto-labeling
- Graph export as PNG/SVG (shareable)
- Target: +1 phase after Phase 3

---

## Success Criteria

1. User opens `hebbs panel`, sees their memory graph in under 2 seconds
2. User switches vaults from the dropdown without restarting
3. User clicks a memory, sees exactly why it scores what it scores (per-signal math: `relevance 0.87 x 0.50 = 0.435`)
4. User clicks an insight, sees confidence score and which source memories generated it
5. User types a query, sees matching memories glow on the graph with latency displayed
6. User toggles recall strategies and sees different results for the same query
7. User filters by state ("show stale") and sees only stale memories highlighted on the graph
8. User drags a weight slider, sees the glow pattern change in real-time
9. User checks the health badge, sees "4 stale, 1 orphaned, 3 decaying" and clicks through to review
10. User toggles decay mode, sees at-risk memories turn red on the graph
11. User scrubs the timeline, watches their memory grow over time with growth sparklines
12. User opens config, tunes ef_search, resets to factory defaults
13. User screenshots their memory graph and shares it (the Spotify Wrapped moment)

---

## Related Tasks

- [TASK-18: Query Audit Log](./TASK-18-query-audit-log.md): Records every recall operation with caller identity, query text, results, and latency. Surfaces in the panel as a query log slide-out with caller filter chips, result highlighting on the graph, and a query heatmap toggle. Phases align with TASK-16 Phase 2-4.

---

## Status

**COMPLETE.** All core panel functionality shipped. See [PLAN-16](./plans/PLAN-16.md) for step-by-step status.

Remaining polish items (UMAP projection, clustering, WebSocket live events, position persistence, SVG export) deferred to [TASK-20: Panel Polish](./TASK-20-panel-polish.md). Scoring/decay config fields and config validation were completed as part of TASK-20.

### What shipped

#### Phases 1-4: Core panel
- `hebbs panel` command with embedded HTTP server (axum, vanilla JS, Canvas 2D)
- Force-directed memory graph as home view with node visual encoding (size=importance, color=recency, shape=kind)
- Side panel: memory detail with score breakdown, content, edges, neighbors
- Vault dropdown from `~/.hebbs/vaults.json`
- Search with recall API, latency badge, graph highlighting
- Weight sliders with presets, strategy toggles, top-k, filters (state/file/importance)
- Decay mode toggle with health detail (stale files, orphaned memories, decay candidates)
- Timeline scrubber with sparklines and snapshot filtering
- Config editor (chunking, embedding, watcher, ignore patterns, output) with save/reset/export
- PNG graph export (2400x1260 at 2x DPI)

#### 6-tab UI expansion
- Restructured from single-surface graph view to 6-tab SPA: Dashboard, Explorer, Recall, Graph, Timeline, Settings
- Dashboard: stat cards (total memories, insights, avg score, decay candidates), health bar, top memories, recent activity, scoring defaults from vault config
- Explorer: paginated, filterable, sortable memory list with search, state/file filters via `/api/panel/memories`
- Recall: standalone recall playground with strategy checkboxes, weight sliders, presets, full score breakdown per result
- Graph: original force-directed graph (unchanged)
- Timeline: growth stats, sparklines, daily activity bars, decay candidates list
- Settings: full config editor with all 6 config sections (chunking, embedding, watch, output, scoring, decay), inline validation errors, save/reset/export
- Cross-tab navigation: clicking a memory in any tab switches to Graph and selects that node
- Tab lazy-loading: data fetched only on first tab visit

#### TASK-20 items completed
- Scoring/decay config fields (`ScoringConfig`, `DecayConfig` in `VaultConfig`)
- Config input validation (`VaultConfig::validate()` with field-specific error map, 400 responses on invalid PUT)
