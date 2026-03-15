# TASK-18: Query Audit Log

Parent: [TASK-16](./done/TASK-16-memory-palace-control-panel.md)
Related: [TASK-17](./TASK-17-one-brain-unified-engine.md) (unified engine), [ANALYSIS_TASK12_UX_CONTROLPANEL.md](./ANALYSIS_TASK12_UX_CONTROLPANEL.md) (trust research)

## Problem

When multiple agents (Claude Code, Cursor, custom sales agent) and the user all read from the same HEBBS brain, the user has no visibility into:

- What queries were asked
- When they were asked
- Who asked them (which agent, which API key, which human)
- What results were returned
- Which memories were surfaced and with what scores

The ANALYSIS 12 research found that 81% of Americans assume organizations will use their personal information in uncomfortable ways. If agents are silently reading your brain and you cannot see what they asked, you have no reason to trust the system. The query log is a trust feature, not a developer feature.

Today the engine tracks `access_count` and `last_accessed_at` per memory (for decay scoring), but nothing about the queries themselves.

---

## Goal

Ship a query audit log that records every recall operation with full context: who asked, what they asked, what was returned, and how long it took. Surface this in both the server API and the control panel.

---

## What Gets Logged

Every recall, prime, and subscribe operation produces an audit entry:

```rust
/// A single query audit log entry.
struct QueryLogEntry {
    /// Unique ID for this query event.
    id: u64,

    /// Microsecond timestamp (same clock as memory timestamps).
    timestamp_us: u64,

    /// Operation type.
    operation: QueryOperation,

    /// Caller identity, resolved from the API key used.
    caller: CallerIdentity,

    /// The query text submitted.
    query: String,

    /// Which recall strategies were active for this query.
    strategies: Vec<RecallStrategy>,

    /// Scoring weights used.
    weights: ScoringWeights,

    /// Number of results requested (top_k).
    top_k: u32,

    /// Number of results returned.
    result_count: u32,

    /// IDs of memories returned (ordered by rank).
    result_memory_ids: Vec<String>,

    /// Top result's composite score (for quick scanning).
    top_score: f32,

    /// Query latency in microseconds.
    latency_us: u64,

    /// Tenant ID (for multi-tenant deployments).
    tenant_id: String,

    /// Entity ID scope (if the query was scoped to an entity).
    entity_id: Option<String>,
}

enum QueryOperation {
    Recall,
    Prime,
    Subscribe,
}

struct CallerIdentity {
    /// API key name from KeyRecord (e.g., "cursor-dev", "claude-code", "sales-agent").
    key_name: String,

    /// API key hash prefix (first 8 hex chars) for identification without exposing the key.
    key_hash_prefix: String,

    /// Permission level of the caller.
    permissions: String,
}
```

### What Does NOT Get Logged

- Memory content (the log stores memory IDs, not the content itself; content is already in the engine)
- The raw API key (only the name and hash prefix)
- Write operations (remember, revise, forget) are out of scope for this task; they produce their own events through the watcher/manifest system

---

## Server: Engine Changes

### New Column Family: `QueryLog`

Add a `QueryLog` column family to RocksDB storage. Key format: `qlog:{timestamp_us}:{id}`. Value: bitcode-serialized `QueryLogEntry`.

This keeps query logs in the same embedded database (no new dependencies), queryable by time range via prefix iterator, and compactable independently.

```
ColumnFamilyName::QueryLog  // new
```

**Bounded storage:** Query log entries are bounded by a configurable retention policy:
- `query_log.max_entries`: default 10,000
- `query_log.max_age_days`: default 30
- Compaction runs on engine startup and periodically (every hour), deleting entries older than max_age or exceeding max_entries (oldest first)

### Recording: Middleware Layer

The query log is written in the server layer, not the engine core. The recall/prime/subscribe handlers already have access to the `TenantExtractor` (which resolves the API key). After the engine returns results, the handler appends a log entry.

This keeps the engine core free of logging concerns (Principle 1: hot path sanctity). The log write is a single RocksDB `put` after the response is computed, not on the critical read path.

```
Request arrives
  -> Auth middleware resolves KeyRecord (caller identity)
  -> Handler calls engine.recall(...)
  -> Engine returns results + latency
  -> Handler writes QueryLogEntry to QueryLog CF  (async, non-blocking)
  -> Handler returns response to caller
```

The log write is fire-and-forget on a background task. If it fails, the recall still succeeds. Query logging never degrades recall latency.

### New API Endpoints

| Endpoint | Method | Returns |
|---|---|---|
| `/v1/queries` | GET | Paginated query log entries, newest first |
| `/v1/queries/stats` | GET | Aggregate stats: queries per caller, per hour, top queries |
| `/v1/queries/:id` | GET | Single query log entry with full detail |

**`/v1/queries` query parameters:**
- `limit` (default 50, max 500)
- `offset` (default 0)
- `caller` (filter by key name, e.g., `?caller=cursor-dev`)
- `operation` (filter by type: `recall`, `prime`, `subscribe`)
- `since` / `until` (time range, microsecond timestamps or ISO 8601)
- `query_contains` (substring match on query text)
- `min_latency` (filter slow queries, microseconds)

**`/v1/queries/stats` response:**

```json
{
  "total_queries": 1847,
  "period": { "since": "2026-03-01T00:00:00Z", "until": "2026-03-14T23:59:59Z" },
  "by_caller": [
    { "key_name": "cursor-dev", "count": 823, "avg_latency_us": 3200 },
    { "key_name": "claude-code", "count": 512, "avg_latency_us": 2800 },
    { "key_name": "panel", "count": 341, "avg_latency_us": 4100 },
    { "key_name": "sales-agent", "count": 171, "avg_latency_us": 3500 }
  ],
  "by_hour": [
    { "hour": "2026-03-14T14:00:00Z", "count": 47 },
    { "hour": "2026-03-14T13:00:00Z", "count": 32 }
  ],
  "top_queries": [
    { "query": "deployment architecture", "count": 12, "last_seen": "2026-03-14T14:23:00Z" },
    { "query": "rust ownership", "count": 8, "last_seen": "2026-03-14T14:21:00Z" }
  ],
  "avg_latency_us": 3200,
  "p99_latency_us": 8400
}
```

### Auth for Query Log Endpoints

Query log endpoints require `PERM_ADMIN` permission. Agents with read-only keys cannot see the query log. Only the brain owner (admin key) or the panel can access it.

### Panel Internal Key

When `hebbs panel` starts the embedded HTTP server, it creates (or reuses) a special internal API key:

```
key_name: "hebbs-panel"
permissions: PERM_READ | PERM_ADMIN
```

This key is never exposed to the user. Panel queries appear in the log as `caller: "hebbs-panel"`, clearly distinguishable from external agents.

---

## Panel: Query Log View

The query log surfaces in the control panel as a new interaction on the existing single-surface design.

### Access Point

Clock icon in the header bar, next to the gear icon. Opens a slide-out panel (same pattern as the config editor and memory explorer).

### Query Log Panel

```
+------------------------------------------------------------------+
|  QUERY LOG                                    [Filter v] [Export] |
+------------------------------------------------------------------+
|                                                                   |
|  CALLERS (last 24h)                                               |
|  [cursor-dev: 823] [claude-code: 512] [panel: 341] [sales: 171]  |
|  Click a caller to filter. Active: [All]                          |
|                                                                   |
+------------------------------------------------------------------+
|  14:23  cursor-dev      "deployment architecture"                 |
|         5 results  top: 0.87  3.2ms                               |
|         [View results]                                            |
|                                                                   |
|  14:21  claude-code     "rust ownership patterns"                 |
|         3 results  top: 0.91  2.8ms                               |
|         [View results]                                            |
|                                                                   |
|  14:18  hebbs-panel     "meeting notes march"                     |
|         8 results  top: 0.73  4.1ms                               |
|         [View results]                                            |
|                                                                   |
|  14:02  sales-agent     "vendor evaluation criteria"              |
|         4 results  top: 0.82  3.5ms                               |
|         [View results]                                            |
|                                                                   |
|  ...                                                              |
+------------------------------------------------------------------+
```

### Interactions

**1. Caller filter chips.** Top of the panel shows all callers as chips with query counts. Click a chip to filter the log to that caller only. This answers: "what has Cursor been asking about?"

**2. Click "View results."** Expands the entry to show:
- Full list of returned memory IDs (clickable, highlights the node on the graph)
- Per-result score breakdown
- Strategies and weights used for this query
- Entity scope (if any)

**3. Click a returned memory ID.** The graph pans to that node and opens the memory side panel. The user can trace: this agent asked this query, got this memory, and here is that memory's content and scoring. Full provenance from query to result.

**4. Graph overlay mode.** When viewing a specific query's results, the graph highlights those result nodes in amber (same pattern as the search overlay). Non-result nodes fade. The user sees exactly which part of their brain this agent accessed.

**5. Filter bar.** Dropdown with:
- Time range (last hour, last 24h, last 7d, custom)
- Operation type (recall, prime, subscribe)
- Min latency (find slow queries)
- Query text search (substring match)

**6. Export.** Download query log as JSON or CSV for external analysis.

### Query Activity on the Graph

Optional toggle: "Show query heatmap" on the main graph. Nodes that have been recalled frequently glow warmer (more orange/red). Nodes that have never been recalled stay cool (blue/gray). This shows which parts of the brain agents actually use vs. which parts sit dormant.

This is distinct from the reinforcement signal (which is access count over all time). The query heatmap can be filtered to a time window or a specific caller: "show me what Cursor accessed this week."

---

## Vault Mode (hebbs-vault)

For vault mode (local file-backed, no API keys), the caller identity comes from the process that invoked the CLI or connected to the panel:

- `hebbs recall` from terminal: caller = `"cli"`
- `hebbs panel` search: caller = `"hebbs-panel"`
- MCP server queries: caller = `"mcp:{client_name}"` (MCP protocol includes client identification)
- Direct gRPC/REST with API key: caller = key name (same as server mode)

If no API key is provided (vault mode allows keyless local access), the caller defaults to `"local"`. The log still captures the query, results, and latency.

---

## Why This Matters

### Trust

From ANALYSIS 12 research: Simon Willison praised Claude's memory because "you can see exactly when and how it is accessing previous context." The query log extends this principle to all agents. The user sees not just what the AI knows, but who is reading it and what they are looking for.

### Security

If an agent starts making unexpected queries (e.g., a sales agent suddenly querying personal health notes), the query log surfaces this immediately. The user can revoke that agent's API key from the panel.

### Debugging

When an agent gives a bad answer, the user can check: "what did it actually recall?" The query log shows the exact memories surfaced, with scores, so the user can diagnose whether the problem was bad retrieval (wrong memories surfaced) or bad generation (right memories, wrong answer).

### Behavioral Insight

Over time, the query log reveals patterns: which agents are most active, what topics get queried most, which parts of the brain are useful vs. dormant. This is the "Spotify Wrapped for your brain's readers."

---

## Phased Delivery

### Phase 1: Engine storage + server endpoints

- Add `QueryLog` column family to storage
- `QueryLogEntry` struct with bitcode serialization
- Async log write in recall/prime/subscribe handlers
- `/v1/queries` and `/v1/queries/:id` endpoints
- Retention policy (max_entries, max_age_days) with periodic compaction
- Auth: `PERM_ADMIN` required
- Target: alongside TASK-16 Phase 2

### Phase 2: Panel query log view

- Clock icon in header, slide-out query log panel
- Caller filter chips with counts
- Expandable entries with result details
- Click result memory ID to highlight on graph
- Graph overlay: highlight a query's result nodes in amber
- Target: alongside TASK-16 Phase 2 or Phase 3

### Phase 3: Stats + heatmap + export

- `/v1/queries/stats` endpoint (aggregate stats by caller, by hour, top queries)
- Caller stats summary in panel header
- Query heatmap toggle on graph (frequently recalled nodes glow warmer)
- Heatmap filterable by caller and time window
- Export as JSON/CSV
- Target: alongside TASK-16 Phase 4

---

## Config

New section in `config.toml`:

```toml
[query_log]
enabled = true           # toggle logging on/off
max_entries = 10000      # bounded storage
max_age_days = 30        # retention policy
log_query_text = true    # opt-out of storing query text (privacy option)
log_result_ids = true    # opt-out of storing which memories were returned
```

The `log_query_text = false` option stores everything except the actual query string. For users who want the audit trail (who queried, when, latency) without revealing what was asked. Defense in depth for privacy-conscious deployments.

---

## Success Criteria

1. User opens the query log and sees every recall made in the last 24 hours with caller, query, result count, and latency
2. User clicks a caller chip and sees only that agent's queries
3. User clicks "View results" and sees exactly which memories were returned with scores
4. User clicks a result memory ID and the graph highlights that node
5. User toggles query heatmap and sees which parts of their brain agents actually use
6. User filters by time range and sees query patterns over days
7. User revokes an agent's API key after seeing unexpected query patterns
8. Query logging adds zero measurable latency to recall operations (async write)
9. Query log stays bounded (never exceeds max_entries or max_age_days)

---

## Status

Not started. Depends on:
- TASK-16 Phase 1 (panel infrastructure)
- TASK-17 (unified engine with server endpoints)
