# Memory Palace: The HEBBS Control Panel

Status: to write

## Key Points

- **What it is**: A browser-based visualization of everything in your HEBBS brain. Force-directed graph of memories, edges, insights, and contradictions. No Electron. No npm install. Pure vanilla JS served from the daemon.

- **How to open**: `hebbs panel` opens your browser to `http://127.0.0.1:6381`. Served directly by the daemon process (no separate server).

- **Graph view**: Every memory is a node. Edges show relationships (RELATED_TO, INSIGHT_FROM, CONTRADICTS, REVISED_FROM). Node size scales with importance. Color encodes memory kind. Red dashed lines are contradictions.

- **Search**: Type a query, see scored results highlighted on the graph with amber glow. Results show relevance/recency/importance/reinforcement score breakdown. Latency badge shows how fast the recall was.

- **Weight sliders**: Drag sliders to adjust how much relevance, recency, importance, and reinforcement matter in search results. Presets for common scenarios (recent-first, high-importance, balanced).

- **Strategy toggles**: Switch between Similarity, Temporal, Causal, and Analogical recall strategies.

- **Filters**: Filter by state (synced/stale/orphaned), source file, importance range.

- **Timeline**: Sparkline showing memory growth over time. Scrubber to view the brain at any point in history.

- **Config editor**: Edit chunking, embedding, watcher, ignore patterns, and output settings directly from the panel. Changes reload live in the daemon.

- **Decay monitor**: Toggle to see which memories are decaying. Health detail panel shows stale files, orphaned sections, and decay candidates.

- **Export**: PNG export at 2400x1260 (2x DPI) for presentations and docs.

- **Vault switching**: Dropdown to switch between all registered vaults.

## Architecture

- Vanilla JS + Canvas 2D (no React, no build step, no node_modules)
- REST API endpoints under `/api/panel/`
- Served as static files embedded in the Rust binary
- WebSocket channel for live events (ingest completions, config reloads)
- Works offline (no CDN dependencies)
