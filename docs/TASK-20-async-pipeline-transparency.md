# TASK-20: Async Pipeline with Full Transparency

## Problem

Today `hebbs init` blocks for 5-10 minutes on 8 files. Users see a frozen terminal. No visibility into what's happening. If they Ctrl-C, state is corrupted. When new files are added later, the watcher races with explicit commands. There is no way to know if the system is "ready" or still processing.

This is not how production memory systems work. Mem0's `add()` returns immediately. Graphiti processes episodes in background workers. The user is never blocked.

## Design: Non-Blocking Everything

### Principle

Every user-facing command returns in <2 seconds. All heavy work (embedding, LLM extraction, contradiction detection, reflection) happens in background async workers. The user always knows what's happening and whether results are complete.

---

### 1. Init Returns Instantly

```
$ hebbs init . --provider ollama --model gemma3:1b
Initialized vault at .
Embedding model: embeddinggemma-300m (cached)
LLM provider: ollama/gemma3:1b (validated)
Daemon started. Indexing 8 files in background...

Vault is live. Run `hebbs status` to track indexing progress.
Run `hebbs recall` anytime -- results improve as indexing completes.
```

**What changes:**
- `hebbs init` creates config, validates LLM, starts daemon, fires async Index command, returns immediately
- No waiting for daemon to finish indexing
- Daemon indexes in background via the existing worker infrastructure

### 2. Background Work Queue

Single ordered work queue per vault. All heavy operations are queue items:

```
WorkItem::Index { file_path }        -- embed + store sections
WorkItem::Extract { file_path }      -- LLM proposition/entity extraction
WorkItem::Contradict { memory_id }   -- LLM contradiction check for one memory
WorkItem::Reflect { entity_id }      -- LLM reflection/consolidation
WorkItem::Forget { memory_id }       -- decay-triggered cleanup
```

**Queue properties:**
- Persistent (survives daemon restart) -- backed by RocksDB column family
- Ordered by priority: Index > Extract > Contradict > Reflect > Forget
- Deduplication: re-indexing a file cancels pending work for that file
- Concurrency: configurable worker count (default: 2 for LLM, 4 for embed-only)
- Backpressure: queue depth visible in status

**Worker architecture:**
```
                    +------------------+
                    |   File Watcher   |
                    +--------+---------+
                             |
                             v
                    +------------------+
  hebbs index ----> |   Work Queue     | <---- hebbs remember
  hebbs remember -> |   (RocksDB CF)   | <---- file watcher
                    +--------+---------+
                             |
              +--------------+--------------+
              |              |              |
         +----v----+   +----v----+   +-----v-----+
         | Embed   |   | LLM     |   | LLM       |
         | Worker  |   | Worker  |   | Worker    |
         | (fast)  |   | (slow)  |   | (slow)    |
         +---------+   +---------+   +-----------+
              |              |              |
              v              v              v
         +----------------------------------------+
         |           Engine (RocksDB + HNSW)      |
         +----------------------------------------+
```

### 3. Status Command: Full Transparency

```
$ hebbs status
Vault: /Users/me/project
State: indexing (67% complete)

Files:     8 total, 6 indexed, 2 pending
Sections:  33 total, 22 embedded, 11 queued
Queue:     11 items (3 embed, 5 contradict, 3 extract)
Workers:   2/2 active (LLM: gemini-3-flash-preview via Ollama)

Recent activity:
  [2s ago]  Stored engineering-handbook.md > Security (3 propositions)
  [5s ago]  Stored engineering-handbook.md > Deployments
  [8s ago]  Contradiction found: budget $5K vs $2K (auto-resolved)
  [12s ago] Stored meeting-notes.md > Sprint Planning

Errors: 0
```

**What this requires:**
- Queue depth counters per work type
- Per-file indexing state in manifest: `pending | indexing | indexed | error`
- Activity log (ring buffer, last 50 events) stored in memory, queryable via status
- Percentage = (indexed sections / total sections)

### 4. Recall Works Immediately (Partial Results)

```
$ hebbs recall "database" --format json
{
  "results": [...],
  "completeness": 0.67,
  "note": "33% of vault still indexing. Results may be incomplete."
}
```

**What changes:**
- Recall always works, searches whatever is indexed so far
- Response includes `completeness` field (indexed_sections / total_sections)
- Human format shows a note when completeness < 1.0
- When completeness = 1.0, no note shown

### 5. Event Stream for UIs and Agents

```
$ hebbs subscribe
{"event":"file_indexed","file":"meeting-notes.md","sections":5,"ts":"..."}
{"event":"memory_stored","id":"01J...","file":"meeting-notes.md","section":"Sprint Planning","ts":"..."}
{"event":"contradiction_found","memory_a":"01J...","memory_b":"01K...","verdict":"contradiction","ts":"..."}
{"event":"contradiction_resolved","edge_id":"...","ts":"..."}
{"event":"reflection_complete","entity":"user_prefs","insights_created":2,"ts":"..."}
{"event":"indexing_complete","files":8,"sections":33,"memories":33,"ts":"..."}
{"event":"file_changed","file":"budget.md","action":"modified","ts":"..."}
{"event":"file_deleted","file":"old-notes.md","sections_forgotten":3,"ts":"..."}
```

**Two consumption modes:**
1. `hebbs subscribe` -- CLI streams events as NDJSON to stdout (for agents/scripts)
2. Daemon protocol `Subscribe` command -- WebSocket-like persistent connection (for Memory Palace panel)

**Event types:**
- `queue_item_started` / `queue_item_completed` / `queue_item_failed`
- `file_indexed` / `file_changed` / `file_deleted`
- `memory_stored` / `memory_revised` / `memory_forgotten`
- `contradiction_found` / `contradiction_resolved`
- `reflection_complete`
- `indexing_complete` (all files done)
- `vault_ready` (all queued work done, vault fully operational)

### 6. File Change Handling (Add/Edit/Delete)

File watcher already exists. What changes is the response path:

**File added:**
```
[watcher] new file: api-design.md
[watcher] queued: 4 sections for embedding
[worker]  Stored api-design.md > Authentication (2 propositions)
[worker]  Stored api-design.md > Rate Limiting
[worker]  Contradiction check: api-design.md > Rate Limiting vs engineering-handbook.md > Tooling
[worker]  No contradiction found.
[event]   file_indexed: api-design.md (4 sections, 2 propositions)
```

**File edited:**
```
[watcher] modified: budget.md (checksum changed)
[watcher] queued: 1 section revised, 0 new, 0 orphaned
[worker]  Revised budget.md > Budget
[worker]  Contradiction check: budget $2K vs existing $5K
[worker]  Contradiction resolved: budget updated from $5K to $2K (REVISED_FROM edge)
[event]   file_changed: budget.md (1 revised, 1 contradiction resolved)
```

**File deleted:**
```
[watcher] deleted: old-notes.md
[watcher] queued: 5 sections for forget
[worker]  Forgotten old-notes.md > Section 1
[worker]  Forgotten old-notes.md > Section 2 ...
[event]   file_deleted: old-notes.md (5 memories forgotten)
```

All of this happens in background. User never waits. Status/subscribe show everything.

---

## Implementation Plan

### Phase 1: Non-blocking init (quick win)
- `hebbs init` sends Index command as fire-and-forget, returns immediately
- Add `completeness` field to recall response
- Status shows indexed vs total counts

### Phase 2: Work queue
- New RocksDB column family for persistent queue
- Worker pool (configurable concurrency)
- Queue depth in status output
- Deduplication on file re-index

### Phase 3: Event stream
- In-memory event ring buffer (last 100 events)
- `hebbs subscribe` CLI command (NDJSON stdout)
- Daemon protocol `Subscribe` command for persistent connections
- Status shows recent activity from event buffer

### Phase 4: Memory Palace integration
- Panel subscribes to event stream via WebSocket
- Live progress bar for indexing
- Real-time contradiction/reflection notifications
- File change indicators

---

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Queue persistence | RocksDB CF | Survives daemon restart, no new dependency |
| Worker concurrency | 2 LLM + 4 embed | LLM is the bottleneck, embed is fast |
| Event storage | In-memory ring buffer | Events are ephemeral, subscribers get live stream |
| Partial recall | Always allowed | Better UX than blocking, user sees completeness % |
| Init behavior | Fire-and-forget | <2s init, user unblocked immediately |

## What This Fixes

| Problem today | After this task |
|--------------|----------------|
| Init blocks 5-10 min | Init returns in <2s |
| No visibility during indexing | Per-file progress in status + event stream |
| Watcher races with explicit index | Single work queue, deduplication |
| Ctrl-C corrupts state | Persistent queue, crash-safe resume |
| No way to know if results are complete | `completeness` field on recall |
| No streaming events for UIs | `hebbs subscribe` + daemon Subscribe |
| File changes silent | Full add/edit/delete event trail |
