# ANALYSIS: HEBBS Memory Control Panel

**Date**: 2026-03-14
**Parent**: [TASK-12](./TASK-12-markdown-obsidian-cognitive-layer.md)
**Scope**: Research, competitive analysis, and UX direction for a visual memory control panel that ships with HEBBS Vault.

---

## Executive Summary

Every AI memory system today fails on the same axis: users cannot see, understand, or trust what the AI knows about them. ChatGPT hides a dossier behind a flat list. Mem0 exposes an API but no visualization. Obsidian's graph view reveals connections but has no AI intelligence. No tool bridges "what the AI knows" with "why it used that knowledge."

HEBBS has a structural advantage: the content plane is plain markdown (human-readable, human-editable), the cognition plane is a rebuildable index, and the engine exposes composite scoring with four tunable signals. A control panel that makes this visible, interactive, and trustworthy becomes the product moat.

The goal: make HEBBS the first thing installed after the OS. The memory layer that every agent on the machine reads from and writes to. The brain you can see.

---

## Part 1: What Users Are Saying

### The Core Frustrations (with real user voices)

**"I can't see what my AI remembers."** ChatGPT's memory dossier (discovered by Simon Willison via prompt extraction, April 2025) contains categories like "User Interaction Metadata" tracking device type, timezone, message length averages, and usage frequency. None of this is surfaced in the UI. Users only see a "Saved Memories" flat list. On HN, a user complained that memory "gets all in a muddle" and they have "NO idea what's enabled and what's disabled." On the OpenAI forum, another reported the feature is "enabled in name only but completely non-functional in practice."

**"My AI keeps forgetting things."** A February 2025 backend update at OpenAI reportedly wiped years of accumulated user data. Over 300 complaint threads appeared on r/ChatGPTPro by July 2025. One user: "My AI feels like it's far gone in dementia." Another described losing eight months of therapeutic progress, comparing it to "a trusted counselor suddenly forgetting their sessions." Startup founders on r/LocalLLaMA describe their agents as "brilliant goldfish." Developer trust in AI accuracy dropped from 43% (2024) to 33% (2025, Stack Overflow Developer Survey).

**"I don't trust it."** 81% of Americans assume organizations will use their personal information in ways that would make them uncomfortable (MIT Technology Review, Jan 2026). HN commenters fear memory data will be exploited for advertising, citing Netflix's ad pivot as precedent: "Netflix used to think they don't want to show ads either." Multiple users report "context pollution," where a developer was asked about windshield wipers when discussing driving visibility. Willison's concern: "context collapse," where data from different life spheres (work, family, hobbies) blends uncontrollably.

**"Memory is lock-in."** An arxiv paper ("The Memory Wars") frames AI memory as evolving from "a UX improvement and economic lock-in, through psychological risks, into a powerful strategic infrastructure posing geopolitical threats." Users who build up months of context in ChatGPT face cognitive lock-in. Anthropic's memory import (March 2026) is a direct counter-move.

**"I want to edit, not just delete."** ChatGPT does not allow direct memory editing. To update a memory, users must delete it and re-share the correct information in a new conversation. Users report a frustrating loop where deleted memories keep coming back, especially near the memory capacity limit (97-99%). Claude's editable memory summary receives praise. Willison: "glad to hear it's fully transparent and can be edited by the user."

**"Show me why."** The recurring ask across Reddit, HN, and X: show me which memories influenced this response and where they came from. Zero products do this for end users. LangSmith does it for developers (trace trees), but there is no consumer-grade equivalent.

**"I want to see connections."** Obsidian users want AI that can analyze entire vaults (not just individual notes), find missing connections, and answer graph queries like "How many hops between these two concepts?" Smart Connections plugin has 600K+ downloads. Users want AI conversations to become "first-class citizens" in their knowledge graph, not isolated silos.

### Sentiment Summary

| Category | Intensity | Representative Quote |
|---|---|---|
| Opacity frustration | Very High | "I have NO idea what's enabled and what's disabled" |
| Fear of data loss | Very High | "My AI feels like it's far gone in dementia" |
| Desire for direct editing | High | Praise for Claude's editable memory summary |
| Distrust of provider motives | High | "As soon as money plays into what's shown, the LLM is no longer aligned with the user" |
| Want for graph visualization | Moderate-High | 600K downloads of Smart Connections plugin |
| Demand for explainability | Moderate | Want to know why X was recalled, not just that it was |
| Need for memory portability | Moderate | Claude's import feature praised but insufficient |

### What Builders Want

@teej_m on X captured it: "Is there a good tops-down explanation of what great 'memory' looks like for agents? The problem feels under-specified. I think people want: 1) Agents can lookup specific facts about me. 2) Agents can follow instructions in context."

The open-source memory landscape (Mem0, Letta/MemGPT, Supermemory) is racing to solve the API layer. Nobody is solving the visualization layer. Mem0's community has explicitly requested a visual dashboard (GitHub Discussion #3599). Supermemory hit #2 on Product Hunt with "Your memories are in ChatGPT... But nowhere else." The market knows memory is broken.

### The Trust Architecture

Simon Willison praised Claude's memory implementation specifically because it uses visible tool calls: "you can see exactly when and how it is accessing previous context." Claude starts every conversation with a blank slate. ChatGPT automatically preloads profiles. HN commenters viewed Claude's approach as more trustworthy because it does not build a hidden profile.

The pattern: **transparency of mechanism, not information dumping.** Research from Taylor & Francis found that transparency without structure overwhelms users. Systems that "explain just enough" feel responsible. Users care about consistency, predictability, and emotional safety.

Local-first is a trust architecture, not just a feature. The Ink & Switch "Local-first software" essay: "With cloud-based software, there is a total loss of ownership and control." When data is local, the architecture itself enforces privacy. No policy document needed. No trust in the provider required.

---

## Part 2: Competitive Landscape

### The Gap Map

| Tool | See Memory | Understand Why | Control It | Trust It | Visualize Connections |
|---|---|---|---|---|---|
| ChatGPT Memory | Flat list (hidden dossier) | No | Toggle + delete | Low (opaque) | No |
| Mem0 | API + basic dashboard | No | API CRUD | Medium (self-host option) | No (graph DB internal only) |
| Limitless/Rewind | Transcripts + search | No | Search only | Low (acquired by Meta) | Timeline only |
| Obsidian Graph | Manual links visible | No (no AI) | Full vault control | High (local files) | Force-directed graph |
| Smart Connections | Embedding similarity | Partial | Plugin settings | Medium | Visualizer plugin |
| Khoj | Ask to discover | No | No browse/edit | Medium (self-host) | None |
| LangSmith | Full traces (dev only) | Yes (for devs) | N/A | High (for devs) | Trace trees |
| Neo4j Bloom | Full graph exploration | Partial | Perspective filtering | High | Excellent interactive graph |
| Qdrant UI | Collection + vector browse | No | CRUD | Medium | Basic t-SNE/UMAP scatter |
| **HEBBS (target)** | **Full + file-backed** | **Yes (scoring breakdown)** | **Full (edit files)** | **High (local, rebuildable)** | **Graph + timeline + scores** |

### Key Gaps HEBBS Can Fill

**Gap 1: No tool bridges "what the AI knows" with "why it used that knowledge."** ChatGPT shows flat lists. LangSmith shows developer traces. Nothing shows end users an interactive view of "here is what influenced this response, here is where it came from, here is the score breakdown."

**Gap 2: Memory visualization is either too simple (flat lists) or too complex (full knowledge graphs).** No tool offers progressive disclosure where casual users see simple summaries but power users drill into relationship graphs, scoring weights, and decay curves.

**Gap 3: AI-discovered connections are invisible.** Smart Connections for Obsidian hints at this, but no memory system shows users "we noticed these three memories are related because X" with an explanation.

**Gap 4: No provenance or confidence scoring visible to users.** No tool shows "this memory was derived from 3 files, with this importance score, this decay rate, last accessed on this date." Memories are treated as binary (exists/doesn't) rather than living artifacts with lineage.

**Gap 5: Temporal memory evolution is invisible.** No tool visualizes how memories change over time. How importance scores drift. How sections get revised. How the brain evolves.

**Gap 6: No tool provides tunable recall parameters to non-developers.** HEBBS already has strategy selection, weight tuning, ef_search, max_depth. No product exposes these as interactive controls.

**Gap 7: No memory health monitoring.** After OpenAI's February 2025 silent data wipe (300+ complaint threads), users want assurance their memories are intact. No tool offers memory integrity checks, backup/restore, or change history. HEBBS's file-backed architecture makes this trivially solvable: the files ARE the backup.

### Open Source Memory Visualization Landscape

The open source ecosystem is building memory APIs, but visualization remains an afterthought:

| Project | Memory Approach | Visualization |
|---|---|---|
| **Mem0 + OpenMemory** | Hybrid vector + graph DB, MCP server | Dashboard with memory browser, bulk actions, ACL, audit logs. Basic list view only. |
| **Supermemory** | Dynamic knowledge graphs, multimodal | **Embeddable React graph component** (`@supermemory/memory-graph`). Documents as rectangular nodes, memories as hexagonal, with relationship edges. Most complete open-source memory viz. |
| **Graphiti (Zep)** | Temporal context graph, entity/relationship validity windows | Graph visualization, MCP server. Internal temporal knowledge graph not exposed to users. |
| **Cognee** | Knowledge graph from data | GraphRAG visualization |
| **Letta (MemGPT)** | Dual-layer memory (in-context + out-of-context) | No visualization |
| **AutoMem** | Graph-vector service (Qdrant + FalkorDB) | Relational memory visualization |

**Key finding:** Supermemory's embeddable graph component is the closest thing to what HEBBS needs, but it is a library for developers to embed, not a standalone control panel. Nobody ships a complete, end-user-facing memory control panel with their product.

---

## Part 3: The Visualization Psychology

### Why "See Your Brain" Works

Three days after hearing information, people remember 10% of it. Add images and recall leaps to 65%. Visualizations tap into System 1 thinking (fast, instinctual), letting people grasp complex information without engaging slower analytical processing.

The "whoa moment" comes from seeing YOUR data reflected back:

- **Spotify Wrapped**: Personal listening data, presented as identity. Triggers "a constant tension between the need for belonging and the desire for individuality." Runs for weeks on social media every year.
- **GitHub Skyline**: Contribution graphs as 3D cityscapes. Developers print them, frame them, share them. Activity data becomes an identity artifact.
- **Obsidian Graph View**: "Aesthetic pleasure of watching your knowledge grow." Network representation "encourages thinking in terms of connections, helping make new links between different parts."

The pattern: personal data + beautiful visualization + interactive exploration = emotional attachment + social sharing + trust.

### The Memory Palace Effect

Virtual memory palaces provide "superior memory recall ability compared to desktop conditions." 3D spatial arrangements help users "understand content and form more memorable impressions with less effort." Spatial cognition research shows our ancestors used spatial and sensory visualization to handle information overload, "turning their minds into visual interfaces for accessing information."

For HEBBS: the control panel should create a spatial sense of "where" memories live. Not just a list. A place.

### From "Quantified Self" to "Qualitative Self"

Raw numbers fail: "numbers revealed patterns but often failed to provide meaning." The shift is from quantified (how many memories, how many accesses) to qualitative (what patterns emerged, what connections formed, what the brain learned). HEBBS's insight generation and composite scoring already produce qualitative data. The control panel surfaces it.

---

## Part 4: What Makes Software "First Install"

### The Pattern

Tools that become default share five traits:

1. **Foundational prerequisite.** Homebrew enables everything else. The tool is a building block for other tools. HEBBS becomes the memory layer that every agent on the machine reads from.

2. **Instant tangible productivity.** iTerm2, VS Code, Oh My Zsh. They reduce the distance between idea and outcome. HEBBS: point it at a folder, get searchable intelligence in seconds.

3. **Opinionated but extensible.** VS Code, Obsidian, Linear. Sweet spot between too basic and too complex. HEBBS: strong defaults (4 strategies, auto-scoring) with full tunability.

4. **Social infrastructure.** Developers share setups publicly (uses.tech, awesome-uses). Tool choice becomes identity. HEBBS: the brain visualization is inherently shareable. "Here is what my AI knows."

5. **One of the 6.** Stack Overflow 2025: 54% of developers use 6+ tools daily. Being one of those 6 requires being indispensable. HEBBS: if every agent on your machine uses it, it is indispensable by definition.

### The Evil Martians Rule (2026)

"AI-assisted iteration must be opt-in, reversible, and explainable." Developers want "explanations, controls, and reversibility more than yet another checkbox." This maps directly to the HEBBS control panel: every action visible, every action reversible (files are source of truth), every score explainable.

---

## Part 5: Control Panel Design Direction

### Design Principles

1. **Files are visible.** Every memory traces back to a file path and heading. Click to open. Edit to change. Delete to forget. The control panel is a lens over files, not a database browser.

2. **Scores are visible.** Composite score breakdown: relevance, recency, importance, reinforcement. Show the math. Show the weights. Let users tune them interactively and see results change in real-time.

3. **Progressive disclosure.** Level 1: dashboard overview (brain health, recent activity, top memories). Level 2: memory explorer (search, filter, browse). Level 3: parameter tuning (strategy weights, decay curves, engine config). Level 4: raw data (manifest, engine stats, embedding space).

4. **The brain grows visibly.** Every index, every recall, every insight shows the brain evolving. Not a static snapshot; a living system.

5. **Local-first, zero-config.** Ships with the binary. No server to set up. No cloud account. `hebbs panel` opens a local web UI served from the same binary.

6. **Shareable moments.** Export your brain graph as an image. Share your vault stats. The "Spotify Wrapped for your knowledge base" moment.

### Proposed Views

#### View 1: Dashboard (Home)

The first thing you see. Brain health at a glance.

```
+----------------------------------------------------------+
|  HEBBS Control Panel                     vault: ~/notes  |
+----------------------------------------------------------+
|                                                          |
|  BRAIN OVERVIEW                                          |
|  +-----------+  +-----------+  +-----------+             |
|  | 847       |  | 23        |  | 4         |             |
|  | memories  |  | insights  |  | files     |             |
|  |           |  |           |  | stale     |             |
|  +-----------+  +-----------+  +-----------+             |
|                                                          |
|  HEALTH                                                  |
|  [====================================----] 89% synced   |
|  [==============] 4 content-stale  [==] 1 orphaned       |
|                                                          |
|  RECENT ACTIVITY (live, watcher events)                  |
|  14:23  notes/standup-mar-14.md    +3 sections indexed   |
|  14:21  projects/hebbs-vault.md    2 sections revised    |
|  14:18  insights/vendor-pattern.md generated (0.82 conf) |
|                                                          |
|  TOP MEMORIES (by composite score)                       |
|  0.94  "Rust ownership model" > Core Concept             |
|  0.91  "HEBBS architecture overview"                     |
|  0.87  "Deploy to staging before prod" (importance: 0.9) |
|                                                          |
|  SCORING DEFAULTS                                        |
|  relevance: 0.50  recency: 0.20                         |
|  importance: 0.20  reinforcement: 0.10                   |
|  [Edit defaults]                                         |
|                                                          |
+----------------------------------------------------------+
```

**Why this works:** Instant orientation. How big is the brain? Is it healthy? What happened recently? What are the strongest memories? All without clicking anything.

#### View 2: Memory Explorer

Browse, search, filter all memories. The Obsidian-meets-LangSmith view.

```
+----------------------------------------------------------+
|  MEMORY EXPLORER                    [Search memories...] |
+----------------------------------------------------------+
|  FILTERS                                                 |
|  Strategy: [Similarity v]  State: [All v]                |
|  Importance: [0.0 ----*---- 1.0]                         |
|  File: [All files v]  Tags: [rust] [architecture]        |
+----------------------------------------------------------+
|                                                          |
|  Search: "deployment architecture"                       |
|                                                          |
|  1. [0.87] projects/hebbs-vault.md > Overview            |
|     importance: 0.80  recency: 0.95  reinforcement: 0.30 |
|     |=relevance====|==rec==|==imp==|=rnf=|               |
|     Tags: #architecture #vault                           |
|     Links: [[hebbs-core]], [[rocksdb-storage]]           |
|     Last accessed: 2 hours ago  Accesses: 12             |
|     [View file] [Open section] [Recall from here]        |
|                                                          |
|  2. [0.73] notes/deploy-guide.md > Staging Process       |
|     importance: 0.70  recency: 0.60  reinforcement: 0.15 |
|     |=relevance==|=rec=|==imp==|rf|                      |
|     ...                                                  |
|                                                          |
+----------------------------------------------------------+
```

**Why this works:** Every memory shows its full score breakdown as a visual bar. Users see exactly why result #1 beats result #2. Click "Recall from here" to try different strategies. The scoring is not hidden; it is the interface.

#### View 3: Recall Playground

Interactive query tuning. The "developer tools for your brain."

```
+----------------------------------------------------------+
|  RECALL PLAYGROUND                                       |
+----------------------------------------------------------+
|  Query: [deployment architecture                      ]  |
|                                                          |
|  STRATEGY          WEIGHTS (drag to adjust)              |
|  [x] Similarity    relevance:     [====*=====] 0.50     |
|  [x] Temporal      recency:       [==*=======] 0.20     |
|  [ ] Causal        importance:    [==*=======] 0.20     |
|  [ ] Analogical    reinforcement: [*=========] 0.10     |
|                                                          |
|  top_k: [5 v]  ef_search: [50 v]                        |
|                                                          |
|  [Run Recall]                                            |
|                                                          |
|  RESULTS                              Latency: 3.2ms    |
|  +-------------------------------------------------+    |
|  | #1  score: 0.8721                               |    |
|  | projects/hebbs-vault.md > Overview               |    |
|  | SCORE BREAKDOWN:                                 |    |
|  |   relevance  0.87 x 0.50 = 0.435               |    |
|  |   recency    0.95 x 0.20 = 0.190               |    |
|  |   importance  0.80 x 0.20 = 0.160               |    |
|  |   reinforcmnt 0.30 x 0.10 = 0.030               |    |
|  |                    composite = 0.815             |    |
|  | Content: "HEBBS vault operates as an invisible   |    |
|  |   cognitive layer over directories of markdown..." |    |
|  +-------------------------------------------------+    |
|                                                          |
|  Try: [Pure relevance] [Recency boost] [High importance] |
|       (preset weight profiles)                           |
+----------------------------------------------------------+
```

**Why this works:** This is the "convince them this is the solution" view. Users type a query, drag sliders, see results change in real-time. Every score component is visible. Preset buttons let beginners try different profiles instantly. The math is not hidden behind a flag; it is the interaction model.

#### View 4: Brain Graph

The "whoa" view. Force-directed graph of all memories with AI-discovered connections.

```
+----------------------------------------------------------+
|  BRAIN GRAPH                                             |
+----------------------------------------------------------+
|                                                          |
|        [rust-ownership]---[rust-patterns]                |
|              |                  |                         |
|        [borrow-checker]   [builder-pattern]              |
|              |                                           |
|     [hebbs-architecture]---[embedding-engine]            |
|              |                  |                         |
|        [rocksdb-storage]  [hnsw-index]                   |
|              |                                           |
|     [standup-mar-14]---[action-items]---[jasen-tasks]    |
|              |                                           |
|         [vendor-eval]                                    |
|              |                                           |
|     *[insight: vendor-pattern]*  (generated, 0.82 conf)  |
|                                                          |
|  LEGEND                                                  |
|  o  Memory section    *o* Insight    --- Wiki-link edge  |
|  ... Embedding similarity edge (>0.7)                    |
|  Size = importance    Color = recency (bright = recent)  |
|                                                          |
|  [Filter: files v] [Min similarity: 0.7] [Show insights] |
+----------------------------------------------------------+
```

**Why this works:** This is the Obsidian graph view, but smarter. Nodes are individual memory sections, not whole files. Edges include both explicit wiki-links AND AI-discovered embedding similarity. Insights glow differently. Node size encodes importance. Color encodes recency. The brain is alive.

Users share this. "Look at my knowledge graph." "Look how my meeting notes connect to my architecture decisions." This is the Spotify Wrapped moment.

#### View 5: Timeline

Temporal view of brain evolution. How the brain grew, what changed, what decayed.

```
+----------------------------------------------------------+
|  TIMELINE                                                |
+----------------------------------------------------------+
|                                                          |
|  Mar 14 |====*=========*=======*=====*====| 14 events   |
|          init  index(4)  recall(7) insight(1)            |
|                                                          |
|  Mar 13 |===*====*=*==*=======*============| 8 events    |
|          edit  edit edit index  recall(3)                 |
|                                                          |
|  Mar 12 |=*=================================| 1 event    |
|          init                                             |
|                                                          |
|  BRAIN GROWTH                                            |
|  memories:  [====>         ] 0 -> 847 over 3 days        |
|  insights:  [=>            ] 0 -> 23                     |
|  avg score: [======>       ] 0.45 -> 0.72                |
|                                                          |
|  DECAY MONITOR                                           |
|  12 memories below 0.10 decay score                      |
|  3 candidates for auto-forget                            |
|  [Review decay candidates]                               |
|                                                          |
+----------------------------------------------------------+
```

**Why this works:** Users see the brain growing. Not a static database; a living system that learns, strengthens, and naturally forgets. The decay monitor makes the cognitive science tangible: memories that are not accessed fade, just like in a real brain.

#### View 6: Engine Config

All tunable parameters with live defaults and explanations.

```
+----------------------------------------------------------+
|  ENGINE CONFIGURATION                                    |
+----------------------------------------------------------+
|                                                          |
|  SCORING WEIGHTS (used for composite score ranking)      |
|  w_relevance:      [====*=====] 0.50  semantic match     |
|  w_recency:        [==*=======] 0.20  how recent         |
|  w_importance:     [==*=======] 0.20  intrinsic value    |
|  w_reinforcement:  [*=========] 0.10  access frequency   |
|  [Save as default] [Reset to factory]                    |
|                                                          |
|  DECAY                                                   |
|  half_life_days:   [30]   auto_forget_threshold: [0.01]  |
|  reinforcement_cap: [100]                                |
|                                                          |
|  WATCHER                                                 |
|  phase1_debounce:  [500ms]   phase2_debounce: [3000ms]   |
|  burst_threshold:  [20 events] burst_debounce: [10000ms] |
|                                                          |
|  EMBEDDING                                               |
|  model: BGE-small-en-v1.5 (ONNX, 384-dim)              |
|  ef_search: [50]   ef_construction: [200]                |
|                                                          |
|  IGNORE PATTERNS                                         |
|  .hebbs/  .git/  node_modules/  .obsidian/              |
|  [Add pattern]                                           |
|                                                          |
|  [Apply] [Export config.toml]                            |
+----------------------------------------------------------+
```

**Why this works:** Power users see every knob. Sliders have labels that explain what they do in plain language. "Save as default" persists to config.toml. "Reset to factory" goes back to known-good. No guessing. No hidden parameters.

---

## Part 6: Technical Approach

### Architecture

```
hebbs-vault binary
  |
  +-- CLI commands (init, index, watch, recall, list, status)
  |
  +-- panel command (NEW)
       |
       +-- Embedded HTTP server (axum or warp, ~500 lines)
       |     GET /           -> serves SPA
       |     GET /api/status -> vault status JSON
       |     GET /api/memories -> memory list with scores
       |     GET /api/recall  -> run recall with params
       |     GET /api/graph   -> graph data (nodes + edges)
       |     GET /api/timeline -> event history
       |     GET /api/config  -> current config
       |     PUT /api/config  -> update config
       |
       +-- Static SPA (embedded in binary via include_dir!)
             HTML + JS + CSS, no build step needed
             Uses D3.js for graph visualization
             Uses vanilla JS or Preact (minimal deps)
```

### Why Embedded in the Binary

- **Zero setup.** `hebbs panel ~/notes` opens a browser. No npm install, no Docker, no separate process.
- **Cross-platform.** Same binary on macOS, Linux, Windows. The SPA is compiled into the binary.
- **Local-first by construction.** HTTP server binds to 127.0.0.1. No network exposure. No cloud dependency.
- **Ships with the tool.** Not a separate product. Not an optional add-on. Part of the binary.

### API Surface

The panel needs 7 endpoints:

| Endpoint | Method | Returns |
|---|---|---|
| `/api/status` | GET | Vault stats: files, sections, sync state, last activity |
| `/api/memories` | GET | Paginated memory list with scores, file paths, states |
| `/api/recall` | POST | Recall results with full score breakdown per result |
| `/api/graph` | GET | Nodes (memories) and edges (wiki-links + similarity) |
| `/api/timeline` | GET | Event history (index, recall, insight, edit events) |
| `/api/config` | GET/PUT | Read and write config.toml values |
| `/api/files/{path}` | GET | Read file content for a memory's source |

All data comes from the manifest + engine. No new storage layer.

---

## Part 7: What Makes This "The Default"

### The Adoption Flywheel

```
1. User installs HEBBS Vault (single binary, brew or curl)
2. Points it at their notes folder: hebbs init ~/notes && hebbs index ~/notes
3. Opens the control panel: hebbs panel ~/notes
4. Sees their brain for the first time (whoa moment)
5. Tries a recall, drags the weight sliders, sees scores change
6. Starts the watcher: hebbs watch ~/notes
7. Edits a file, sees the brain update in real-time
8. Shares a screenshot of their brain graph
9. Configures their agent (OpenClaw, Claude Code, Cursor) to use hebbs recall
10. Every agent on the machine now reads from the same brain
11. The brain becomes the user's persistent identity across all AI tools
```

### Why HEBBS Wins Each Trust Test

| Trust test | HEBBS answer |
|---|---|
| "Can I see what it knows?" | Yes. Every memory maps to a file + heading. Open the file. |
| "Can I understand why?" | Yes. Composite score breakdown visible per result. |
| "Can I control it?" | Yes. Edit the file. Delete the file. Tune the weights. |
| "Can I trust the architecture?" | Yes. Local-first. Delete .hebbs/ and rebuild. Files are yours. |
| "Can I leave?" | Yes. Your files are plain markdown. HEBBS is just an index. |
| "Can I see it evolving?" | Yes. Timeline view shows brain growth, decay, insights. |

### Cross-Platform Guarantee

HEBBS Vault must work identically on:

- **macOS** (Homebrew install, native FS events via kqueue)
- **Linux** (curl install, inotify watcher)
- **Windows** (curl install or scoop, ReadDirectoryChangesW watcher)

The `notify` crate already abstracts FS events cross-platform. The embedded HTTP server (axum) and SPA work on all three. The ONNX embedder runs on all three (CPU inference, no GPU required). The RocksDB storage works on all three.

Single binary. No runtime dependencies. No Docker. No Python. No Node. Download and run.

---

## Part 8: Competitive Moat Analysis

### What HEBBS Has That Nobody Else Does

| Capability | ChatGPT | Mem0 | Obsidian+AI | HEBBS |
|---|---|---|---|---|
| File-backed memories (edit to change) | No | No | Yes (manual) | Yes (auto-indexed) |
| Composite scoring with 4 signals | Unknown | Basic | No | Yes, tunable |
| Multiple recall strategies | No | Similarity only | Similarity only | 4 strategies, combinable |
| Insight generation as files | No | No | No | Yes |
| Interactive score tuning | No | No | No | **Yes (control panel)** |
| Visible provenance chains | No | Audit logs only | Manual links | File path + heading + score |
| Brain graph visualization | No | No | Links only (no AI) | Links + embedding similarity |
| Temporal evolution view | No | No | No | **Yes (timeline)** |
| Decay and reinforcement visible | No | No | No | **Yes (dashboard)** |
| Local-first, zero-config | No (cloud) | Self-host option | Yes | Yes |
| Cross-platform single binary | No | No | Electron | Yes (Rust) |
| Rebuildable index | No | No | N/A | **Yes (files are truth)** |

The control panel is not a nice-to-have. It is the product differentiator. It converts HEBBS from "a CLI that developers use" to "a brain that anyone can see and trust."

---

## Part 9: Phased Delivery

### Phase 1: Dashboard + Memory Explorer (MVP)

- `hebbs panel` command starts embedded HTTP server
- Dashboard view: brain stats, health, recent activity, scoring defaults
- Memory explorer: search, filter, browse with score breakdowns
- Serves from binary (no external deps)
- Target: functional in 1-2 weeks

### Phase 2: Recall Playground

- Interactive query tuning with drag sliders for weights
- Strategy selection checkboxes
- Live results with per-result score decomposition
- Preset weight profiles (pure relevance, recency boost, importance first)
- Target: +1 week after Phase 1

### Phase 3: Brain Graph

- Force-directed graph using D3.js
- Nodes = memory sections, edges = wiki-links + embedding similarity
- Node size = importance, color = recency
- Click to navigate, hover for preview
- Filter by file, tag, similarity threshold
- Target: +1-2 weeks after Phase 2

### Phase 4: Timeline + Decay Monitor

- Temporal view of brain evolution
- Event log (index, recall, insight, edit)
- Brain growth charts
- Decay candidates flagged for review
- Target: +1 week after Phase 3

### Phase 5: Config Editor + Export

- Interactive config.toml editing with live preview
- Brain graph export as PNG/SVG (shareable)
- Vault stats export (the "Spotify Wrapped" moment)
- Target: +1 week after Phase 4

---

## Part 10: Open Questions

1. **Technology for the SPA.** Vanilla JS + D3.js (minimal, no build step) vs. Preact (more structure, still tiny). Leaning vanilla for zero-dependency embedding.

2. **Real-time updates.** WebSocket from the watcher process to the panel, or polling? WebSocket is better UX (instant updates when files change) but adds complexity.

3. **Port selection.** Default 127.0.0.1:6382? Auto-detect available port? Open browser automatically on `hebbs panel`?

4. **Graph performance.** D3 force-directed graphs slow down above ~1000 nodes. For large vaults, need level-of-detail: cluster view at high zoom, section view when zoomed in. Or WebGL (e.g., force-graph library).

5. **Mobile/tablet.** Responsive design? Or desktop-only for v1? The vault is local, so mobile access requires network tunneling. Desktop-only for v1 is pragmatic.

6. **Electron/Tauri wrapper.** Eventually wrap the panel as a native app? Or stay browser-based? Browser-based is simpler and cross-platform by default.

---

## Sources

- [Simon Willison on ChatGPT Memory Dossier](https://simonwillison.net/2025/May/21/chatgpt-new-memory/)
- [Simon Willison on Claude Memory](https://simonwillison.net/2025/Sep/12/claude-memory/)
- [MIT Technology Review: What AI Remembers About You](https://www.technologyreview.com/2026/01/28/1131835/what-ai-remembers-about-you-is-privacys-next-frontier/)
- [Ink & Switch: Local-First Software](https://www.inkandswitch.com/essay/local-first/)
- [The Memory Wars (arxiv)](https://arxiv.org/html/2508.05867v1)
- [Evil Martians: 6 Things Dev Tools Must Have in 2026](https://evilmartians.com/chronicles/six-things-developer-tools-must-have-to-earn-trust-and-adoption)
- [Mem0 Dashboard Discussion #3599](https://github.com/mem0ai/mem0/discussions/3599)
- [Supermemory: Universal Memory MCP](https://supermemory.ai/)
- [MemR3: Explicit Evidence-Gap Memory](https://arxiv.org/html/2512.20237)
- [Obsidian Graph View Defense (Eleanor Konik)](https://www.eleanorkonik.com/p/its-not-just-a-pretty-gimmick-in-defense-of-obsidians-graph-view)
- [Smart Connections Visualizer](https://smartconnections.app/)
- [Spotify Wrapped Marketing Psychology (NoGood)](https://nogood.io/blog/spotify-wrapped-marketing-strategy/)
- [GitHub Skyline](https://github.com/github/gh-skyline)
- [Data Visualization Psychology (Toptal)](https://www.toptal.com/designers/data-visualization/data-visualization-psychology)
- [Neo4j Bloom](https://neo4j.com/product/bloom/)
- [Qdrant Web UI Visualization](https://qdrant.tech/documentation/web-ui/)
- [LangSmith Observability](https://www.langchain.com/langsmith/observability)
- [Mem0 raises $24M (TechCrunch)](https://techcrunch.com/2025/10/28/mem0-raises-24m-from-yc-peak-xv-and-basis-set-to-build-the-memory-layer-for-ai-apps/)
- [Anthropic Memory Import (Bloomberg)](https://www.bloomberg.com/news/articles/2026-03-03/anthropic-tries-to-win-users-from-chatgpt-with-memory-feature)
- [Taylor & Francis: Between Transparency and Trust](https://www.tandfonline.com/doi/full/10.1080/0144929X.2025.2533358)
- [From Quantified Self to Qualitative Self](https://medium.com/@ann_p/from-quantified-self-to-qualitative-self-ai-shifting-focus-in-personal-analytics-68209a851322)
- [Spatial Cognition and Memory Palaces (IxDF)](https://ixdf.org/literature/topics/spatial-cognition)
- [InfraNodus Obsidian Plugin](https://infranodus.com/obsidian-plugin)
- [HN: Claude Memory Discussion](https://news.ycombinator.com/item?id=45214908)
- [Zep Temporal Knowledge Graph (arxiv)](https://arxiv.org/abs/2501.13956)
- [OpenAI Forum: Memory Issues](https://community.openai.com/t/chatgpt-memory-issues-and-not-saving-or-referencing-memories/1308586)
- [OpenAI Forum: Memory Full No Warning](https://community.openai.com/t/chatgpt-plus-plan-memory-issue-no-warning-when-memory-is-full-leading-to-lost-entries/1111613)
- [Why OpenAI Won't Talk About ChatGPT's Silent Memory Crisis](https://www.allaboutai.com/ai-news/why-openai-wont-talk-about-chatgpt-silent-memory-crisis/)
- [Reddit User Feedback on LLM Chat Tools](https://cuckoo.network/blog/2025/04/15/reddit-user-feedback-llm-chat-tools-underserved-needs)
- [HN: Claude Memory Architecture](https://news.ycombinator.com/item?id=45684134)
- [Obsidian Forum: obsidian-graph-query plugin](https://forum.obsidian.md/t/obsidian-graph-query-let-your-ai-agent-query-your-vaults-knowledge-graph-bfs-shortest-path-bridges-hubs-orphans/111828)
- [Supermemory Memory Graph Component](https://supermemory.ai/docs/integrations/memory-graph)
- [Graphiti by Zep (GitHub)](https://github.com/getzep/graphiti)
- [Cognee AI Memory Tools Evaluation](https://www.cognee.ai/blog/deep-dives/ai-memory-tools-evaluation)
- [Contrasting Memory Philosophies: Claude vs ChatGPT](https://allarddewinter.net/blog/contrasting-memory-philosophies-claudes-explicit-tools-vs-chatgpts-automatic-profiles/)
- [Memory for AI Agents: A New Paradigm (The New Stack)](https://thenewstack.io/memory-for-ai-agents-a-new-paradigm-of-context-engineering/)
- [Mem0 Benchmark: 26% Higher Accuracy vs OpenAI](https://guptadeepak.com/the-ai-memory-wars-why-one-system-crushed-the-competition-and-its-not-openai/)
- [Context Rot: Why AI Gets Worse the Longer You Chat](https://www.producttalk.org/context-rot/)
- [Stack Overflow 2025: Developer Trust in AI Dropped to 33%](https://survey.stackoverflow.co/2025/)
- [Meta Acquires Limitless (TechCrunch)](https://techcrunch.com/2025/12/05/meta-acquires-ai-device-startup-limitless/)
- [Supermemory raises from Google Execs (TechCrunch)](https://techcrunch.com/2025/10/06/a-19-year-old-nabs-backing-from-google-execs-for-his-ai-memory-startup-supermemory/)
- [OpenAI Memory & Controls](https://openai.com/index/memory-and-new-controls-for-chatgpt/)
- [ChatGPT Memory Update 2025 (Capitaly)](https://www.capitaly.vc/blog/chatgpts-memory-update-2025-everything-you-need-to-know)
