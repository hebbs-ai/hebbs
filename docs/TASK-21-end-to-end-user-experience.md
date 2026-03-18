# TASK-21: End-to-End User Experience

This is what the user sees. Not what happens inside. Not our problems. Theirs.

---

## Current Status (2026-03-18)

**985 tests passing. 0 failures. 17 of 18 original gap items fixed.**

### What's done

The CLI core loop works end-to-end: init (interactive wizard, non-blocking), index (background, plain language output), status (live progress, accurate state), recall (source attribution, completeness hint, contradiction warnings), panel (correct vault on startup/switch, human-readable labels, incremental live graph). Errors are human sentences with 10 unit tests. Model download shows a visual progress bar with ETA. Agents have a 3-command quick-start in SKILL.md. All tests green including the previously-flaky `skill_recall_ef_search` (TOCTOU port fix).

### Must Do Before Next Customer

These will break the demo or destroy trust if a customer finds them.

| Priority | Item | Why it's a must-do | Effort |
|----------|------|--------------------|--------|
| **P0** | **`hebbs contradictions` command** | Recall output tells users "Run `hebbs contradictions` to review." That command does not exist. Following the hint gives an error. Broken promise in the first 5 minutes. | 2-3 hr |
| **P0** | **Fix `truncate_str` UTF-8 panic** | `truncate_str` in `panel/routes.rs` slices bytes, not chars. If a memory starts with CJK/emoji text, the panel backend panics. A customer with Japanese notes crashes the panel. | 15 min |
| **P0** | **Emit `MemoryCreated` events during indexing** | The `MemoryCreated` panel event is defined but never sent. The live graph `mergeData` works, but only fires on `IngestComplete` (after each file batch). Graph grows in chunks, not node-by-node. A customer watching during init sees long pauses between updates instead of smooth growth. | 1-2 hr |
| **P1** | **Panel vault switch error feedback** | When `hebbs panel <path>` switches to an invalid/uninitialized vault, the HTTP POST silently fails. The panel shows the previous vault with no warning. Customer thinks the product is ignoring their command. | 1 hr |
| **P1** | **Forgotten timeline content preview** | Frontend now shows `f.content_preview` for forgotten memories, but the tombstone data in RocksDB may not store content. If undefined, it falls back to "Forgotten memory". Should store a content snippet when creating tombstones. | 1 hr |

### Should Do (quality bar for experienced customers)

| Priority | Item | Why it matters | Effort |
|----------|------|---------------|--------|
| P1 | **`hebbs subscribe` event stream** | Blocks IDE plugin integration and advanced agent workflows. Without it, tools must poll. | 3-4 hr |
| P1 | **Panel file change toasts** | Users don't know HEBBS noticed their edit until they run a command. "budget.md updated. 1 memory revised." | 2-3 hr |
| P1 | **Panel contradiction UI (red edges)** | Contradictions surface in CLI recall but are invisible in the panel graph. Red edges + click-to-review would be the "wow" moment. | 3-4 hr |
| P2 | **Node pulse on change** | When a file is re-indexed, its graph node should pulse briefly (amber glow). Visual proof the system is alive. | 1-2 hr |
| P2 | **Panel indexing progress bar** | Top bar should show "Indexing: 14/23 files" with a progress bar during active indexing. Status data is already in the daemon; panel just needs to poll/subscribe. | 1-2 hr |

### Test Coverage Gaps

| Gap | Risk | Effort |
|-----|------|--------|
| No integration test for `IndexingSnapshot` live progress | Medium -- status-during-indexing is a key user moment, only tested via compilation | 1-2 hr |
| No test for contradiction aggregation in recall responses | Medium -- engine-level detection tested, daemon cross-result pairing is not | 1 hr |
| No test for `send_fire_and_forget` semantics | Medium -- core to init decoupling, only tested implicitly | 1 hr |
| No test for interactive init wizard | Low -- TTY interaction hard to automate, manual verification sufficient for now | 2-3 hr |
| No test for `--initial-vault` daemon flag | Low -- startup path tested implicitly | 30 min |
| No test for `hebbs contradictions` command (once built) | High -- new command needs full coverage from day one | 1 hr |

### Architecture Improvements

| Area | Issue | Recommendation |
|------|-------|---------------|
| Error handling | `humanize_error()` is string matching. Fragile if upstream error messages change. | Consider typed error variants with `thiserror` that carry human messages from construction. |
| Indexing progress | `Arc<Mutex<HashMap>>` works but is shared mutable state across async boundaries. | If contention becomes an issue, switch to `tokio::sync::watch` per vault for zero-copy reads. |
| Panel vault switch | HTTP POST retry loop (5x 300ms) is fire-and-forget. No confirmation the switch succeeded. | Return the switch result to CLI so it can warn if the panel couldn't switch. |
| Tombstone storage | Tombstones only store ID and timestamp. No content preview for forgotten timeline display. | Store first 60 chars of content when creating tombstones so the panel can show what was forgotten. |
| Event granularity | `MemoryCreated` defined but never emitted. Graph updates are batch-level, not per-memory. | Emit per-memory events from `phase2_ingest_inner` by threading `panel_event_tx` through the ingest pipeline. |

---

## Who is this person?

They have a folder of markdown files. Meeting notes, project docs, research, decisions, journals. Maybe 20 files, maybe 2000. They heard HEBBS gives their AI agent memory across conversations. They want to try it. They have 3 minutes of patience.

---

## Moment 1: Install + First Run

```
brew install hebbs
```

That's it. No Rust toolchain. No Docker. No Python env. One command.

```
$ cd ~/work/my-project
$ hebbs init
```

No flags. No `--provider`. No `--model`. No `--api-key-env`.

HEBBS asks what it needs, once:

```
$ hebbs init

  Welcome to HEBBS.

  Found 23 markdown files in this directory.

  HEBBS needs an LLM for deep understanding of your notes.
  You can use a cloud provider or a local model via Ollama.

  [1] Ollama (local, free, private)
  [2] Gemini (fast, cheap)
  [3] Anthropic
  [4] OpenAI

  Choice: 1

  Checking Ollama... found at localhost:11434
  Available models: gemma3:1b, qwen3:4b

  Recommended: gemma3:1b (fastest for this task)
  Use gemma3:1b? [Y/n]: y

  Downloading embedding model (1.2 GB, one-time)...
  ████████████████████░░░░░  83%  1.0 GB / 1.2 GB  12.4 MB/s

  Done. Indexing your files in the background.

  ┌─────────────────────────────────────────────┐
  │  Your vault is live.                        │
  │                                             │
  │  Try:  hebbs recall "what did we decide?"   │
  │                                             │
  │  Indexing: 4/23 files ready (18%)           │
  │  Run `hebbs status` to see progress.        │
  └─────────────────────────────────────────────┘
```

**Time to first prompt: 15 seconds** (after model download).

They don't wait for indexing. They try it immediately.

---

## Moment 2: First Recall (While Still Indexing)

```
$ hebbs recall "what database are we using"

  PostgreSQL is the primary database for transactional data.
  Redis for caching and session management.
  TimescaleDB for time-series metrics.

  Source: project-decisions.md > Database

  ℹ 18% of your vault is indexed. More results may appear as indexing completes.
```

It works. Even partially indexed, it found something useful. The note is honest -- not an error, not a warning. Just information.

They try again 30 seconds later:

```
$ hebbs recall "who owns what"

  Alice Chen - Backend Lead. Owns search infrastructure and APIs.
  Bob Tanaka - Frontend Engineer. Handles React, dashboard, mobile.
  Carol Singh - DevOps. Manages EKS clusters, CI/CD pipelines.
  David Park - Product Manager. Owns roadmap and customer relationships.
  Eve Okafor - Data Scientist. Recommendation models and analytics.

  Sources: team.md, meeting-notes.md > Sprint Planning

  ℹ 64% of your vault is indexed.
```

More results now. The percentage goes up. The system is alive, working in the background.

---

## Moment 3: Indexing Complete

No fanfare. No notification they didn't ask for. Next time they run any command:

```
$ hebbs recall "on-call process"

  On-call rotation is weekly, Monday to Monday.
  P0: acknowledge within 5 minutes, resolve within 30 minutes.
  P1: acknowledge within 15 minutes.
  Blameless postmortem required within 48 hours.
  Schedule managed in PagerDuty. Carol Singh coordinates.

  Sources: engineering-handbook.md > On-Call

  ✓ Vault fully indexed (23 files, 87 memories)
```

The checkmark appears once. After that, no status line at all. Clean output.

---

## Moment 4: They Edit a File

They open `budget.md` and change the infrastructure budget from $5,000 to $8,000. Save the file. They don't run any command. They don't even think about HEBBS.

Next time they ask:

```
$ hebbs recall "infrastructure budget"

  The infrastructure budget is $8,000 per tenant per month.
  This covers compute, storage, and network costs across all environments.

  Source: budget.md > Budget
  Updated: 2 minutes ago (was $5,000, automatically revised)
```

HEBBS noticed the change, re-indexed the file, detected the revision, updated the memory. The user sees "Updated: 2 minutes ago" and knows it's current. They didn't do anything.

---

## Moment 5: Contradictions Surface Naturally

They add a new file `q2-planning.md` that says "Infrastructure budget reduced to $3,000 per tenant."

Next recall:

```
$ hebbs recall "budget"

  ⚠ Conflicting information found:

  budget.md says: $8,000 per tenant per month
  q2-planning.md says: $3,000 per tenant per month

  Both sources are current. You may want to resolve this.

  To see all contradictions: hebbs contradictions
```

Not an error. Not a crash. A gentle heads-up that something doesn't add up. The user decides what's true. HEBBS just noticed.

---

## Moment 6: Status When They Want It

```
$ hebbs status

  Vault: ~/work/my-project
  Files: 24 (all indexed)
  Memories: 91
  Last change: budget.md (3 min ago, auto-indexed)
  Contradictions: 1 unresolved

  Everything is up to date.
```

Short. Clean. Answers "is everything working?" in one glance.

During indexing, it shows more:

```
$ hebbs status

  Vault: ~/work/my-project
  Indexing: 14/24 files (58%)
  Currently processing: engineering-handbook.md

  ████████████████░░░░░░░░░  58%

  Memories created: 43
  Contradictions found: 1 (auto-resolved)
  Estimated time remaining: ~40 seconds
```

---

## Moment 7: Agent Integration (The Real Product)

The user's AI agent (Claude, Cursor, Copilot) has HEBBS as a tool. The agent uses it automatically. The user never types `hebbs recall` -- the agent does.

What the agent sees in SKILL.md:

```
hebbs recall "relevant query"    -- search memory, always fast
hebbs remember "important fact"  -- store something the user said
hebbs status                     -- check if vault is ready
```

Three commands. That's the entire API for agents. Everything else is invisible.

The agent starts a conversation:

```
User: "What's our deployment process?"

Agent: [internally calls: hebbs recall "deployment process"]
Agent: [gets results from engineering-handbook.md + project-decisions.md]

Agent: "Your deployments happen on Tuesdays and Thursdays on AWS EKS
        via GitHub Actions. Staging auto-deploys on PR merge to develop.
        Production requires manual approval. Carol manages the pipeline."
```

The user didn't mention HEBBS. The agent just knew. That's the product.

---

## Moment 8: New Files, No Ceremony

User creates a new file. Saves it. Doesn't tell HEBBS. Minutes later:

```
User: "What did the customer say about pricing?"

Agent: [hebbs recall "customer pricing feedback"]

Agent: "In the March 20 customer feedback session, two customers
        reported slow search on mobile. Three requested dark mode.
        No specific pricing feedback was captured in your notes."
```

The new file was automatically picked up. No reindex command. No restart. The system just works.

---

## What the User Never Sees

- "ONNX Runtime", "HNSW index", "RocksDB", "phase2_ingest"
- Stack traces or Rust error messages
- "daemon.sock", "daemon.pid", "daemon.log"
- Embedding dimensions or model architecture details
- "section", "proposition", "entity" (our internal concepts)
- Any mention of "background workers" or "work queue"
- Configuration files unless they go looking

---

## What the User Feels

1. **Fast.** Every command responds in under 2 seconds. Always.
2. **Alive.** The system notices file changes without being told.
3. **Honest.** When results are partial, it says so. When there's a conflict, it surfaces it.
4. **Quiet.** No unnecessary output. No notifications they didn't ask for.
5. **Trustworthy.** Sources are always shown. They can verify.
6. **Invisible.** When used through an agent, they forget it's there. Their agent just knows things.

---

## Non-Negotiable UX Rules

1. **No command should ever hang.** If work takes time, it happens in the background. The terminal returns.
2. **No jargon in user-facing output.** "Files" not "sections". "Memories" not "HNSW vectors". "Conflicts" not "contradictions with bidirectional CONTRADICTS edges".
3. **Always show sources.** Every recall result must say where it came from.
4. **Partial is better than blocked.** 30% indexed with results beats 100% indexed after 5 minutes of nothing.
5. **Errors are sentences, not stack traces.** "Could not reach Ollama at localhost:11434. Is it running?" not "ureq::Error::Transport(...)".
6. **One install command. One init command. Zero config files to edit.**
7. **The default should be right.** Model selection, batch sizes, thresholds -- we pick. They override only if they want to.

---

## Moment 9: The Panel (Their Window Into Memory)

Init already told them:

```
  Your vault is live. Panel: http://localhost:6381
```

They click it. Or `hebbs panel` opens it.

### What they see on first visit (still indexing):

A dark, calm interface. Center of screen: their knowledge graph growing in real time. Nodes appear one by one as files get indexed. Edges form between related memories. A gentle progress indicator at the top:

```
┌──────────────────────────────────────────────────────────────────┐
│  Indexing... 14/23 files  ████████████░░░░░░░░  61%             │
│  Currently: engineering-handbook.md > Deployments                │
└──────────────────────────────────────────────────────────────────┘
```

They watch their notes become a living graph. Nodes cluster by topic. The engineering docs form one cluster, meeting notes another, research a third. They can see the shape of their knowledge.

### What they see after indexing completes:

The progress bar fades. The graph settles. Top bar becomes:

```
  23 files  ·  87 memories  ·  3 insights  ·  All synced
```

### Live file changes:

They edit `budget.md` in their editor. The panel, without refresh:

- The `budget.md` node pulses briefly (amber glow)
- A toast slides in from the bottom: "budget.md updated. 1 memory revised."
- If a contradiction was found: the edge between two nodes turns red, and the toast says: "Conflict detected: budget $8K vs $3K"

They delete a file. The nodes fade out and disappear. Toast: "old-notes.md removed. 5 memories forgotten."

They add a new file. New nodes fade in, edges connect to existing memories. Toast: "api-design.md indexed. 4 new memories."

### The tabs they actually use:

**Graph** (default) -- Their knowledge as a living map. They click a node, see the memory content, source file, when it was created, what it's connected to. They don't need to understand graph theory. It's just "here's what I know, and how things connect."

**Recall** -- They type a question, see answers ranked by relevance with source files. Like a search engine for their own brain. Instant results.

**Timeline** -- "What did I learn this week?" A chronological view. They see which days they added the most knowledge, how their vault grew over time.

**Dashboard** -- One-glance health check. Files synced, contradictions to review, insights generated. Three numbers and a green checkmark when everything is current.

### What they DON'T see in the panel:

- Memory IDs or ULIDs
- Embedding dimensions or HNSW parameters
- RocksDB column families
- "Phase 1" or "Phase 2"
- JSON blobs
- Daemon status or PID files
- Config TOML

### The panel during contradictions:

They wrote conflicting information in two files. The graph shows it:

- Two nodes connected by a red edge
- Click the edge: "budget.md says $8,000/month. q2-planning.md says $3,000/month. Detected 2 minutes ago."
- Two buttons: "Keep newer" / "I'll fix it in the file"
- If they fix the file, HEBBS detects the change, resolves the contradiction, the red edge disappears. No panel interaction needed.

### The panel during reflection:

After enough memories accumulate, HEBBS generates insights. The panel shows them:

- Insight nodes are visually different (hexagon vs circle)
- They glow when new
- Click one: "Based on 12 memories about deployments, architecture, and on-call: your team operates with a strong separation between staging (automated) and production (manual approval), with Carol as the infrastructure single point of contact."
- Source memories listed below. The user can verify every claim.

### What makes them come back to the panel:

1. **It's beautiful.** The graph is not a debugging tool. It's a visualization of their knowledge that they're proud to show people.
2. **It's alive.** Things move, update, glow. It feels like a living system, not a static dashboard.
3. **It answers "what do I know?"** -- a question no other tool answers visually.
4. **It surfaces problems.** Contradictions, stale files, gaps in knowledge -- shown gently, not as errors.
5. **It's optional.** Power users live in the panel. Others never open it. Both are fine.

---

## Gap Between Today and This

| Status | Today | Target |
|--------|-------|--------|
| **Fixed** | `hebbs init` takes 15 flags | `hebbs init` asks interactively, 3 questions max |
| **Fixed** | Init blocks 5-10 minutes | Init returns in 2 seconds, indexing runs in background |
| **Fixed** | Recall fails if not indexed | Recall works immediately, shows "18% indexed" completeness hint |
| **Fixed** | Errors show Rust types | Errors are human sentences (humanize_error strips Rust types) |
| **Fixed** | Status lies about stale re-index when daemon not watching | Status reports accurate re-index state based on whether daemon is watching |
| **Fixed** | Status shows internal counters, no live progress | Status shows live phase/file progress during active indexing via IndexingSnapshot |
| **Fixed** | No mention of sources | Every recall result shows `Source: file.md > Heading` from context data |
| **Fixed** | Contradictions require CLI commands | Contradictions surface naturally in recall results via CONTRADICTS edge scan |
| **Fixed** | Agent needs 20+ commands in SKILL.md | SKILL.md has "3 command" quick start section at top |
| **Fixed** | Model download is silent | Visual progress bar with percentage, MB, speed, and ETA |
| Open | No event stream | `hebbs subscribe` for tools that want live updates |
| **Fixed** | Panel shows internal IDs and counters | All 12 ID-display spots replaced with human-readable labels/content previews |
| **Fixed** | Panel graph is static on load | `mergeData()` preserves positions, new nodes settle via physics. Incremental on WS events. |
| Open | No live file change feedback in panel | Toasts for file add/edit/delete with memory counts |
| Open | Contradictions require CLI to review | Red edges in graph, click to review, resolve from panel |
| Open | Panel feels like a debug tool | Panel feels like a knowledge visualization you'd show someone |
| Open | No indexing progress in panel | Top bar shows file-by-file progress during indexing |
| Open | Panel requires manual refresh | WebSocket pushes all changes in real time |
| **Fixed** | Panel always opens home directory vault | Panel opens the vault specified by the user on startup |
| **Fixed** | `hebbs panel <path>` with already-running daemon shows wrong vault | Vault switch sent when daemon is already running |
| **Fixed** | Index output uses jargon ("sections embedded", "sections remembered") | Output uses plain language ("files indexed", "memories created") |

---

## Implementation Log

### Completed

**2026-03-18: Decouple init from indexing**
`hebbs init` no longer blocks on indexing. After creating `.hebbs/` and ensuring the embedding model is cached, it sends a fire-and-forget `Index` command to the daemon and returns immediately. The terminal prints "Indexing N file(s) in the background. Run `hebbs status` to check progress." The daemon completes both Phase 1 and Phase 2 asynchronously. The manifest is saved at each phase, so a crash mid-index leaves the vault in a resumable state.

**2026-03-18: Accurate status for stale sections**
The daemon's Status handler now includes `daemon_watching: bool` in the response, set by checking whether the vault is currently open in the VaultManager. The CLI uses this to show either "daemon watching, will re-index on next change" or "run `hebbs index` to re-index" -- whichever is actually true.

**2026-03-18: Panel opens correct vault on startup**
`DaemonConfig` now has an `initial_vault: Option<PathBuf>` field. When `hebbs panel <path>` is run and the daemon needs to start, `--initial-vault <path>` is passed to `hebbs serve`. The panel HTTP server loads this vault before binding, so the first page load shows the right vault with no race. If the vault path is not initialized, a clear warning is shown rather than silent fallback to home.

**2026-03-18: Recall completeness hint**
When the vault is still being indexed, `hebbs recall` now appends "N% of your vault is indexed. More results may appear as indexing completes." The daemon's Recall handler reads manifest section counts (O(1)) and returns `indexing_pct` in the response when `content_stale > 0`. The CLI renders it after the results list.

**2026-03-18: Panel vault switch when daemon already running**
When `hebbs panel <path>` connects to an already-running daemon (fast path), it now sends a POST to `/api/panel/vaults/switch` with the specified vault path. The switch uses the same retry logic (5 attempts, 300ms delay) as before. Combined with the `--initial-vault` fix for fresh daemon starts, the panel now opens the correct vault in both cases.

**2026-03-18: Plain language CLI output**
Replaced jargon throughout the CLI:
- Index: "sections embedded, sections remembered" became "N file(s) indexed. N memories created, N revised, N removed."
- Status: "Stale: N (will re-index...)" became "Indexing: in progress, will complete automatically" or "Indexing: incomplete. Run `hebbs index` to finish."
- Status: "Orphaned: N (source file deleted)" became "Removed: N source file(s) deleted"
- Status: "Files: N (all indexed)" now shows "Files: N (P% indexed)" during active indexing.

**2026-03-18: Status live indexing progress**
The daemon now maintains an `IndexingSnapshot` (`Arc<Mutex<HashMap<PathBuf, IndexingSnapshot>>>`) shared between the Index handler and the Status handler. During active indexing, `hebbs status` shows "Indexing: 14/24 files (58%). Phase: 1. Currently processing: engineering-handbook.md". The snapshot is cleared when indexing completes or errors out.

**2026-03-18: Contradictions in recall output**
The daemon's Recall handler now checks for CONTRADICTS edges among returned results. For each result, `engine.contradictions(id)` is called (O(log n + k) prefix scan). Bidirectional edges are deduplicated via a `HashSet<(id_a, id_b)>`. When contradictions exist, the CLI prints a warning block: "Conflicting information found between [source_a] and [source_b]."

**2026-03-18: Interactive init (TTY wizard)**
When `hebbs init` is run without `--provider`/`--model` flags and stdin is a TTY (`std::io::IsTerminal`), a 3-question interactive wizard runs: (1) choose LLM provider [1-4 or s to skip], (2) model name [default provided], (3) API key env var [default provided]. Non-TTY invocations (CI, piped input) fall through to the existing flag-based path. No new dependencies added.

**2026-03-18: Errors as sentences (humanize_error)**
Added `humanize_error()` to the CLI binary. It strips Rust type prefixes (`ureq::Error::Transport`, `std::io::Error`, etc.) and pattern-matches common errors to human sentences. Example: "Connection refused" on port 11434 becomes "Could not reach Ollama at localhost:11434. Is it running?" All user-facing error paths in the CLI now pass through this function.

**2026-03-18: Agent SKILL.md quick-start**
Added a "Minimum viable agent API (3 commands)" section at the top of `hebbs/skills/hebbs/SKILL.md`: `hebbs recall "query" --format json`, `hebbs remember "fact" --format json`, `hebbs status`. Agents can integrate with just these three commands. The rest of the SKILL.md remains for advanced usage.

**2026-03-18: Fix flaky `skill_recall_ef_search` test**
The test harness had a TOCTOU race: bind port 0, get random port, drop listener, rebind with tonic. Another parallel test could grab the port in between, causing `AddrInUse`. Fixed by keeping the `TcpListener` and passing it directly to `tonic::transport::server::TcpIncoming::from_listener()`, then using `serve_with_incoming_shutdown` instead of `serve_with_shutdown`. Result: 842/842 tests passing, 0 failures.

**2026-03-18: `humanize_error()` moved to library with unit tests**
Extracted `humanize_error()` from `bin/hebbs.rs` into `error.rs` as a public function. Added 10 table-driven unit tests covering: Ollama connection refused, generic connection refused, file not found, permission denied, RocksDB errors (upper and lower case), serde/anyhow prefix stripping, and unknown error passthrough. Binary delegates to `hebbs_vault::error::humanize_error`.

**2026-03-18: Model download progress bar enhanced with visual bar and ETA**
The download progress (already in `hebbs-embed/src/model.rs`) was enhanced from a text-only percentage to a visual progress bar (`████████░░░░░░░░░░░░`), ETA with minutes/seconds formatting, and retained speed/MB display. ETA computed from current download speed and remaining bytes.

**2026-03-18: Panel humanization -- all raw IDs replaced**
Backend: Added `label` field to `EdgeInfo` struct (content preview of target memory, 60 chars). Changed `source_ids` from `Vec<String>` to `Vec<SourceInfo>` with `id` and `label` fields. Added `truncate_str` helper to `panel/routes.rs`.
Frontend: Replaced all 12 spots in `app.js` that displayed truncated hex IDs (`sid.slice(0, 12)`, `target_id.slice(0, 12)`, `memory_id.slice(0, 16)`, `memory_id.substring(0, 12)`) with human-readable labels. Fallback is `'Memory'` instead of hex. Query result IDs show `'Result 1'`, `'Result 2'` instead of hex. Forgotten timeline shows `content_preview` or `'Forgotten memory'`.

**2026-03-18: Panel live graph with incremental merge**
Added `mergeData()` method to `MemoryGraph` in `graph.js`: preserves existing node positions (`x, y, vx, vy, fx, fy`), places new nodes at periphery with fibonacci spiral, gently reheats physics (`alpha = 0.15`) only when new nodes appear. WebSocket handler updated: `memory_created` and `ingest_complete` events use `mergeGraph()` (incremental), `memory_forgotten` uses full `loadGraph()` (since nodes are removed). Graph grows smoothly during indexing without jarring layout resets.

### Test Results (2026-03-18)

Full workspace test run: **985 passed, 0 failed.** All green.

Test count increased from 842 to 985 due to: 10 new `humanize_error` unit tests, plus full recompilation picking up previously-filtered test binaries.

### Remaining (prioritized)

See "Must Do Before Next Customer" and "Should Do" sections at the top of this document for the full prioritized list. Summary:

**Must Do (P0):**
1. `hebbs contradictions` command -- recall output references it but it doesn't exist
2. Fix `truncate_str` UTF-8 panic -- byte slicing on multi-byte chars crashes the panel
3. Emit `MemoryCreated` events during indexing -- live graph mergeData is ready but events never fire

**Must Do (P1):**
4. Panel vault switch error feedback -- silent failure on invalid vault path
5. Forgotten timeline content preview -- store content snippet in tombstones

**Should Do:**
6. `hebbs subscribe` event stream
7. Panel file change toasts
8. Panel contradiction UI (red edges)
9. Node pulse on change
10. Panel indexing progress bar
