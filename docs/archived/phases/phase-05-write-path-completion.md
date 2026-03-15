# Phase 5: Write Path Completion (Revise, Forget, Decay) -- Architecture Blueprint

## Status: ✅ COMPLETE

**Completed:** 2026-02-28

---

## Intent

Phases 1 through 4 built a system that can remember, index, and recall. But a memory system that only accumulates is a filing cabinet, not a mind. Phase 5 introduces the three operations that make HEBBS cognitive: the ability to update beliefs (`revise`), intentionally purge knowledge (`forget`), and automatically attenuate stale signals (`decay`).

These three operations interact in non-obvious ways. Revise must preserve lineage so that downstream insights (Phase 7) can trace their provenance. Forget must be total -- not a soft delete -- because GDPR compliance demands it and because dangling references in any index are corruption. Decay must run continuously in the background without ever blocking the hot path, and it must be the first instance of a background task runtime that Phase 7 (reflect) and Phase 13 (sync) will reuse.

The decisions made here are load-bearing for everything that follows. Get the revision lineage model wrong, and Phase 7 insight invalidation is impossible. Get the forget cleanup wrong, and index corruption accumulates silently. Get the decay runtime wrong, and we rebuild it twice more.

---

## Scope Boundaries

### What Phase 5 delivers

- `revise()` operation: update a memory's content, re-embed, re-index all three indexes, preserve lineage via `RevisedFrom` graph edge
- `forget()` operation: remove memories matching caller-specified criteria from all five column families and all in-memory structures, with tombstone logging
- Decay engine: background task that periodically recalculates `decay_score` for all memories, using the reinforcement-weighted exponential decay formula
- Auto-forget pipeline: memories that decay below a configurable threshold are flagged as candidates (but not automatically deleted without policy approval)
- Background task runtime infrastructure: a reusable, interruptible, cursor-based background worker pattern
- Configuration types for decay half-life, sweep interval, batch size, and auto-forget threshold
- Full test coverage: unit, property-based, integration, and Criterion benchmarks

### What Phase 5 explicitly does NOT deliver

- Insight invalidation on revise/forget (Phase 7 -- but the lineage and tombstone infrastructure must support it)
- Automatic insight re-evaluation when source memories change (Phase 7)
- GDPR compliance reporting or audit log export (Phase 12 -- but tombstones capture the data Phase 12 needs)
- Sync conflict resolution for concurrent revise across devices (Phase 13 -- but the revision model must accommodate it)
- Configuration file loading or runtime reconfiguration (Phase 8 -- decay parameters are passed programmatically)
- Metrics emission for revise/forget/decay operations (added incrementally, but hook points are designed now)
- Compaction filter integration for physical data removal after forget (Phase 12 optimization)

These exclusions are deliberate. Phase 5 lays the mechanisms; later phases build the policies.

---

## Architectural Decisions

### 1. The Revision Model

This is the most consequential decision in Phase 5. Two competing models exist, and choosing wrong makes Phase 7 insight invalidation either trivial or impossible.

**Model A: New-ID Replacement.** Create a new memory with a new ULID. De-index the old. Link new to old with a `RevisedFrom` edge. The old memory becomes archival.

| Aspect | Consequence |
|--------|-------------|
| External references | Break. Any system holding the old memory_id now points to an archival record that won't appear in recall. |
| Graph edges TO this memory | Orphaned. Every edge from another memory to this one now points to the old version. Re-pointing them is O(incoming_edges) and requires scanning the reverse index. |
| API simplicity | Low. The caller must update their reference. |
| Lineage | Clean. Old version exists as a separate, complete record. |

**Model B: In-Place Update with Predecessor Snapshot.** Keep the same memory_id. Before updating, snapshot the current state to a new archival record. Update the primary record in place. Link primary to snapshot with a `RevisedFrom` edge.

| Aspect | Consequence |
|--------|-------------|
| External references | Preserved. The memory_id is stable. Graph edges, causal chains, and external systems see the updated content at the same address. |
| Graph edges TO this memory | Unaffected. They still point to the same ID, which now holds the revised content. |
| API simplicity | High. `revise(memory_id, evidence)` returns the same memory_id with updated content. Principle 10 (API Elegance). |
| Lineage | Complete. The snapshot holds the old state. The `RevisedFrom` edge from primary to snapshot is the lineage chain. |

**Decision: Model B (In-Place Update with Predecessor Snapshot).**

The API stability argument is decisive. When a sales agent has a memory "Client prefers email" and revises it to "Client prefers Slack", every causal chain and graph edge that references that memory should now see "Client prefers Slack" without graph surgery. Model B achieves this for free.

**How the snapshot works:**

Before any mutation, the engine copies the current state of the memory into a new record with a fresh ULID (the "predecessor snapshot"). This snapshot is stored in the `default` column family but is NOT inserted into HNSW, temporal, or graph indexes. It exists solely for lineage traversal.

The primary memory record is then updated in-place: new content, new embedding, bumped `updated_at`, `kind` set to `Revision`, `decay_score` reset to `importance`. All three indexes are updated atomically via WriteBatch (temporal and vector entries are replaced; graph edges are preserved).

A `RevisedFrom` edge is added from the primary memory_id to the snapshot's ID.

**Revision chain reconstruction:** If a memory is revised multiple times, each revision creates a new snapshot. The primary memory accumulates multiple outgoing `RevisedFrom` edges: one per historical version. The revision history is reconstructed by following all `RevisedFrom` edges from the primary and sorting the resulting snapshots by `created_at` ascending. The oldest snapshot is the original version; the newest is the most recent predecessor.

**Why snapshots are not indexed:** A snapshot is a frozen point-in-time record. It must not appear in similarity recall, temporal queries, or graph traversal (except when explicitly followed via lineage edges). Keeping it out of HNSW and temporal indexes is sufficient. Direct `get()` by ID still works (it reads from `default` CF), which is exactly what lineage traversal needs.

### 2. Index Update Atomicity for Revise

Revise touches all three indexes: the old HNSW entry must be replaced with a new embedding, the old temporal entry must be replaced if `entity_id` changed or if the memory is re-dated, and the graph index gains a `RevisedFrom` edge.

The existing two-phase pattern from `remember()` applies: `prepare` generates batch operations, `commit` updates in-memory structures after the WriteBatch succeeds.

**The revise pipeline:**

```
load memory → validate inputs → embed new content
  → create predecessor snapshot (new ULID, copy old fields)
  → prepare_delete for old index entries (temporal, vector CF keys)
  → prepare_insert for new index entries (temporal, vector CF keys, RevisedFrom edge)
  → build WriteBatch:
      [1] Put snapshot into default CF
      [2] Put updated memory into default CF (same key, new value)
      [3] Delete old temporal entry
      [4] Put new temporal entry
      [5] Delete old vector CF entry
      [6] Put new vector CF entry
      [7] Put RevisedFrom edge (forward + reverse)
  → execute WriteBatch atomically
  → commit_delete on in-memory HNSW (tombstone old point)
  → commit_insert on in-memory HNSW (add new point, same memory_id)
```

This is a single atomic WriteBatch containing 7+ operations. If the process crashes mid-pipeline (after prepare but before execute), no state changes. If it crashes after the WriteBatch but before HNSW commit, the next startup detects the inconsistency (vector CF has the new entry but HNSW doesn't) and rebuilds the HNSW layer from the vector CF. This crash-recovery path must be tested.

**IndexManager needs a `prepare_update` method.** Rather than exposing the prepare_delete + prepare_insert internals to the Engine, IndexManager should provide a single `prepare_update` that accepts the old memory's metadata and the new memory's metadata, and returns the combined batch operations. This encapsulates the index-layer knowledge of what needs to change.

### 3. The `revise()` Input Contract

The revision input must be carefully designed. Not every field of a memory is revisable, and partial updates must be explicit.

**Revisable fields and their semantics:**

| Field | Revisable? | Semantics |
|-------|-----------|-----------|
| `content` | Yes | New content replaces old. Triggers re-embedding and HNSW re-index. |
| `importance` | Yes | Updated importance score. Resets `decay_score` to the new importance. |
| `context` | Yes (merge or replace) | Caller can merge new keys into existing context or fully replace it. |
| `entity_id` | Yes | Changing entity_id triggers temporal index re-key. Rare but valid (e.g., reassigning a memory to a different entity). |
| `edges` | Yes (additive) | New edges are added. Existing edges are not removed. Removing edges is a separate concern. |
| `memory_id` | No | Immutable. The identity of the memory. |
| `created_at` | No | Immutable. The original creation timestamp. |
| `kind` | No (system-managed) | Set to `Revision` automatically. |
| `device_id` | No | Immutable. Origin device. |
| `logical_clock` | No (system-managed) | Incremented automatically on revise. |

**Context merge semantics:** The caller provides a context map. A `context_mode` field on the input controls behavior:

| Mode | Behavior |
|------|----------|
| Merge (default) | New keys are added. Existing keys are overwritten. Keys not in the update are preserved. |
| Replace | The entire context is replaced with the provided map. |

This distinction matters because an agent might want to add a tag without losing existing metadata.

**Validation rules:**

- `memory_id` must be a valid 16-byte ULID of an existing, non-snapshot memory
- `content` (if provided) must pass the same validation as `remember()`: valid UTF-8, max 64KB
- `importance` (if provided) must be in [0.0, 1.0]
- At least one revisable field must be provided (no-op revisions are rejected)
- Revising a predecessor snapshot directly is invalid (snapshots are immutable lineage records)

### 4. Forget Criteria Engine

`forget()` accepts a criteria specification, not just a list of IDs. This makes it a query-then-delete operation with significant design implications.

**Supported criteria:**

| Criterion | What it matches | Index used | Complexity |
|-----------|----------------|-----------|------------|
| Explicit IDs | Specific memories by their 16-byte ULID | Direct point lookup in default CF | O(k) where k = number of IDs |
| Entity scope | All memories belonging to a given entity_id | Temporal CF prefix scan | O(log n + k) |
| Staleness threshold | Memories whose `last_accessed_at` is older than a given microsecond timestamp | Requires full scan or temporal index heuristic | O(n) worst case |
| Access count floor | Memories with `access_count` below a threshold | Requires full scan | O(n) worst case |
| Memory kind | Memories of a specific `MemoryKind` | Requires full scan | O(n) worst case |
| Decay score floor | Memories with `decay_score` below a threshold | Requires full scan | O(n) worst case |
| Combined | Logical AND of any of the above | Most selective criterion scanned first, others used as filters | O(narrowest_scan) |

**The O(n) problem:** Criteria like staleness, access count, kind, and decay score have no dedicated index. Scanning every record in the default CF for 10M memories is expensive (seconds, not milliseconds). This is acceptable because criteria-based forget is a maintenance operation, not a hot-path operation.

**Optimization: entity scope as a filter accelerant.** When `entity_id` is combined with any other criterion, the temporal index narrows the candidate set to O(log n + k_entity) before the other criteria apply as in-memory filters on deserialized candidates. The Engine should detect this optimization automatically.

**Optimization: decay-score-based forget is a special case.** The decay engine already iterates all memories periodically. It can maintain a "low-score candidates" set in the meta CF. When `forget()` is called with a decay score criterion, it reads from this pre-computed set instead of scanning.

**Batch processing:** Forget must process candidates in bounded batches (configurable, default 1000). Each batch is one WriteBatch transaction. This prevents a single forget call from holding resources for minutes on a large dataset. The return value includes the total count of forgotten memories and whether the operation was truncated (more candidates remain).

### 5. Forget Execution and Multi-Index Cleanup

When a memory is selected for forget, it must be completely removed from every storage location. A forgotten memory that still appears in a similarity search is data corruption.

**Cleanup checklist per memory:**

| Storage location | What to delete | How |
|-----------------|---------------|-----|
| Default CF | The memory record (key: memory_id) | BatchOperation::Delete |
| Temporal CF | The temporal index entry (key: entity_id + 0xFF + timestamp) | BatchOperation::Delete (requires entity_id and created_at from memory) |
| Vectors CF | The serialized HNSW node data (key: memory_id) | BatchOperation::Delete |
| Graph CF (forward) | All outgoing edges: source=this memory | Prefix scan on memory_id, delete each |
| Graph CF (reverse) | All incoming edges: target=this memory | Reverse prefix scan, delete each |
| In-memory HNSW | The vector point | commit_delete (tombstone, then eventual cleanup) |

**Edge cascade policy:** When memory A is forgotten:

- All edges FROM A (outgoing) are deleted. These are A's claims about relationships.
- All edges TO A (incoming) are also deleted. Other memories' claims about A become dangling; since A no longer exists, the edges are meaningless.
- **Exception:** `RevisedFrom` edges. If A has outgoing `RevisedFrom` edges pointing to predecessor snapshots, those snapshots are also deleted (they exist solely for A's lineage and have no independent value). This is a cascading delete.
- **Exception:** If a predecessor snapshot is itself the target of an `InsightFrom` edge (Phase 7), that edge must be preserved for insight invalidation. Phase 5 does not implement this logic yet, but the forget infrastructure must not blindly cascade into records that have non-lineage edges. The safe rule: cascade delete snapshots that have ONLY `RevisedFrom` incoming edges and NO other edge types.

**Tombstone creation:** After successful deletion, a tombstone record is written to the `meta` CF. The tombstone captures:

| Field | Why |
|-------|-----|
| Forgotten memory_id(s) | To answer "was this ID ever in the system?" |
| Entity_id (if any) | To scope GDPR proof-of-deletion reports |
| Forget timestamp | When the deletion occurred |
| Criteria used | To distinguish intentional forget from auto-forget from GDPR request |
| Count of cascade-deleted snapshots | Audit trail completeness |
| Content hash (NOT content) | Allows detecting if the same content is re-remembered without retaining the content itself |

Tombstones are keyed in the meta CF with a prefix (`tombstone:` + timestamp + memory_id) so they can be range-scanned by time.

**Post-forget compaction:** After a forget batch completes, the engine should trigger an asynchronous compaction on the affected column families. This is not a correctness requirement (the data is already logically deleted via the WriteBatch) but a physical cleanup requirement: until RocksDB compacts, the deleted data remains on disk in SST files. For GDPR compliance, compaction must eventually run. Phase 5 triggers it; Phase 12 adds guarantees about compaction timing.

### 6. The Decay Score Formula

The decay formula is the mathematical heart of HEBBS's cognitive model. It must be:
- **Monotonically decreasing with time** (memories fade)
- **Monotonically increasing with access count** (reinforcement strengthens)
- **Sensitive to importance** (high-importance memories decay slower in practice because they start higher)
- **Bounded** (output must be in a predictable range for comparison)
- **Cheap to compute** (called once per memory per sweep, for millions of memories)

**The formula:**

```
decay_score = importance × 2^(−age / half_life) × log₂(1 + min(access_count, reinforcement_cap))
```

**Component analysis:**

| Component | Range | Behavior | Cost |
|-----------|-------|----------|------|
| `importance` | [0.0, 1.0] | Scales the entire score. A memory with importance 0.0 always has decay_score 0.0. | 1 multiply |
| `2^(−age / half_life)` | (0.0, 1.0] | Exponential decay. At age = half_life, this equals 0.5. At age = 2 × half_life, this equals 0.25. Approaches but never reaches 0.0. | 1 division + 1 exp2 |
| `log₂(1 + min(access_count, cap))` | [0.0, ~6.66] (cap=100) | Reinforcement amplifier. Diminishing returns: going from 0 to 10 accesses matters more than going from 90 to 100. The cap prevents runaway amplification from bot-like access patterns. | 1 min + 1 add + 1 log2 |

**Why `log₂` and not `ln` or `log₁₀`:** `log₂` keeps the amplifier in a human-intuitive range. At 100 accesses, `log₂(101) ≈ 6.66`. With `importance = 1.0` and `age = 0`, the maximum decay_score is ~6.66. This provides a meaningful spread for ranking without requiring normalization. If `ln` were used, the range would be ~4.62 (less spread). If `log₁₀`, ~2.0 (too compressed).

**Score range for ranking purposes:** The composite recall score from Phase 4 normalizes relevance, recency, importance, and reinforcement into weighted [0, 1] components. The decay_score is a separate signal used by the decay engine for pruning decisions, NOT used directly in recall ranking (recall uses the composite scorer). This avoids conflating two different scoring regimes.

**Age computation:** `age = now_us - last_accessed_at` (NOT `now_us - created_at`). A memory that was accessed yesterday is "young" regardless of when it was created. This matches the cognitive model: reinforcement resets the decay clock. If a memory has never been accessed, `last_accessed_at` equals `created_at`, and it decays from its creation time.

**Half-life semantics:**

| Deployment | Recommended half_life | Rationale |
|-----------|----------------------|-----------|
| Cloud (SaaS) | 30 days (2,592,000,000,000 µs) | Long retention, agents operate over months-long contexts |
| Edge (robot) | 14 days | Tighter working set, environment changes frequently |
| Edge (laptop dev agent) | 90 days | Developer context evolves slowly |
| Custom | Any positive u64 microsecond value | Per-use-case tuning |

### 7. Background Task Runtime

The decay engine is the first background task in HEBBS. Its runtime design must be reusable because Phase 7 (reflect pipeline) and Phase 13 (sync worker) will follow the same pattern.

**Requirements for the background runtime:**

| Requirement | Why |
|-------------|-----|
| Non-blocking to the hot path | Principle 1 (Hot Path Sanctity), Principle 5 (Background Intelligence). Decay must never hold a lock that recall, remember, or revise contend for. |
| Interruptible | A sweep in progress must be pausable within one batch boundary. The engine must be able to shut down cleanly without waiting for a full sweep to complete. |
| Cursor-based | A sweep that processes 10M memories in one pass is unbounded (Principle 4). Instead, process in batches with a cursor that persists across sweep intervals. |
| Configurable interval | The time between sweep starts. Default: 1 hour. Tunable from seconds to days. |
| Configurable batch size | The number of memories processed per sweep iteration. Default: 10,000. |
| Observable | Emits metrics: sweep duration, memories processed, scores updated, auto-forget candidates found. |
| Crash-safe | If the process crashes mid-sweep, the next startup resumes from the last persisted cursor. No work is lost, no work is double-counted. |

**Runtime model: dedicated OS thread, not tokio task.**

The decay sweep reads sequentially through the default CF and writes `decay_score` updates via WriteBatch. This is CPU-bound (score computation) interspersed with sequential I/O (RocksDB reads and writes). Tokio tasks are designed for I/O-bound async work, and a long-running CPU-bound scan would starve the tokio executor if not carefully managed with `spawn_blocking`.

Using a dedicated OS thread (spawned via `std::thread::Builder`) is simpler, more predictable, and avoids polluting the async runtime. The thread communicates with the engine via:

- A `crossbeam::channel` for control signals (pause, resume, shutdown, reconfigure)
- Shared read access to the storage backend via `Arc<dyn StorageBackend>` (RocksDB is internally thread-safe)
- Writes via `write_batch()` which is also thread-safe

This is the same concurrency model as the engine itself: shared `Arc` references, no application-level locks on the data path.

**Lifecycle:**

```
Engine::new()
  └─ spawns DecayWorker thread (paused by default)

Engine::start_decay(config)
  └─ sends Resume signal to worker

Engine::pause_decay()
  └─ sends Pause signal (worker finishes current batch, then idles)

Engine::stop_decay()
  └─ sends Shutdown signal (worker finishes current batch, then exits)

Engine::reconfigure_decay(new_config)
  └─ sends Reconfigure signal (applied before next batch)

Drop for Engine
  └─ sends Shutdown signal, joins thread
```

**Why not tokio:** Phase 8 (server) will introduce a tokio runtime for gRPC/HTTP. The decay worker must not depend on that runtime's existence. In embedded mode (Phase 9), there may be no tokio runtime at all. A dedicated OS thread works in all deployment modes.

**Reusability contract:** The background worker pattern (thread + channel + cursor + batch) is extracted into a generic `BackgroundWorker` abstraction in `hebbs-core`. Phase 7's reflect worker and Phase 13's sync worker instantiate the same abstraction with different work functions.

### 8. Decay Sweep Mechanics

**The sweep algorithm:**

```
initialize:
  cursor ← load from meta CF ("decay_sweep_cursor") or START_OF_DEFAULT_CF
  config ← current decay configuration

loop:
  wait for interval OR resume signal
  
  for batch_number in 0..max_batches_per_sweep:
    if shutdown or pause signal received:
      persist cursor to meta CF
      break
    
    read batch_size memories starting from cursor (prefix_iterator or range_iterator on default CF)
    
    if batch is empty:
      cursor ← START_OF_DEFAULT_CF (wrap around)
      persist cursor to meta CF
      break
    
    for each memory in batch:
      new_score ← compute_decay_score(memory, config)
      if |new_score - memory.decay_score| > epsilon:
        add to update_batch: (memory_id, updated memory with new decay_score)
      if new_score < auto_forget_threshold:
        add to candidates_batch: (memory_id, new_score, entity_id)
    
    if update_batch is non-empty:
      execute WriteBatch to default CF (update memory records with new decay_scores)
    
    if candidates_batch is non-empty:
      write candidate IDs to meta CF under "auto_forget_candidates:" prefix
    
    cursor ← last processed memory_id + 1
    persist cursor to meta CF
```

**Key design choices in this algorithm:**

**Cursor persistence in meta CF.** The cursor is a 16-byte memory_id (ULID). Storing it in the meta CF means the sweep survives process restarts. The cost is one extra write per batch, which is negligible compared to the batch updates.

**Wrap-around.** When the cursor reaches the end of the default CF, it wraps to the beginning. This means the sweep continuously cycles through all memories. A memory created after the cursor passes will be picked up on the next cycle. Staleness between sweeps is bounded by: `(total_memories / batch_size) × sweep_interval`.

**Epsilon threshold.** Only write a decay_score update if the new score differs from the stored score by more than a small epsilon (e.g., 0.001). This avoids generating write amplification for scores that haven't meaningfully changed. At 10M memories with a 30-day half-life, most scores change negligibly between hourly sweeps.

**Auto-forget candidates are written, not acted on.** The decay engine identifies candidates but does not call `forget()` directly. This separation is deliberate:

- It allows a policy layer (Phase 7 or operator configuration) to decide what to do with candidates
- It prevents the background task from making irreversible decisions
- It allows the operator to review candidates before they're purged
- The auto-forget candidate list in meta CF is the input to a separate `auto_forget()` method that the operator (or a policy) invokes

### 9. Auto-Forget Pipeline

Auto-forget connects the decay engine's candidate identification to actual memory removal.

**The pipeline has three stages:**

**Stage 1: Candidate Identification (Background, done by decay sweep).** The decay engine writes candidate memory_ids and their scores to the meta CF under a `auto_forget_candidates:` key prefix. This is append-only and batched.

**Stage 2: Policy Evaluation (On-demand or scheduled).** A policy decides which candidates to actually forget. Phase 5 provides a default policy: "forget all candidates below the threshold." Phase 7 may provide more sophisticated policies (e.g., "don't forget if this memory is a source for an active insight"). The policy interface is a trait so that future phases can plug in without modifying Phase 5 code.

**Stage 3: Execution (Synchronous, bounded).** The engine calls `forget()` with the policy-approved candidate IDs. This reuses the same forget execution path as a manual `forget()` call. Batch size is bounded.

**Why three stages instead of one:** The temptation is to have the decay sweep call `forget()` directly when it finds a sub-threshold memory. This creates three problems:

1. The background thread would need to hold locks or coordinate with the hot path for multi-index deletes.
2. There is no opportunity for policy review -- what if the memory is a source for an important insight?
3. The sweep's latency becomes unpredictable (a forget involves multiple index scans).

Separating identification from execution keeps the sweep fast and the deletion controlled.

### 10. Configuration Surface

Phase 5 introduces the first runtime-configurable subsystem. The configuration design must be consistent with what Phase 8 (server configuration) will expect.

**Decay configuration parameters:**

| Parameter | Type | Default | Bounds | Rationale |
|-----------|------|---------|--------|-----------|
| `half_life_us` | u64 | 2,592,000,000,000 (30 days) | > 0 | The exponential decay half-life. Larger = slower decay. |
| `sweep_interval_us` | u64 | 3,600,000,000 (1 hour) | ≥ 1,000,000 (1 second) | Time between sweep starts. |
| `batch_size` | usize | 10,000 | [100, 1,000,000] | Memories processed per batch within a sweep. |
| `max_batches_per_sweep` | usize | 100 | [1, 10,000] | Caps total work per sweep to `batch_size × max_batches`. |
| `auto_forget_threshold` | f32 | 0.01 | [0.0, 1.0] | Decay score below which a memory becomes a forget candidate. |
| `epsilon` | f32 | 0.001 | > 0.0 | Minimum score change to trigger a write. |
| `enabled` | bool | true | -- | Master switch. When false, the worker thread idles. |

**Forget configuration parameters:**

| Parameter | Type | Default | Bounds | Rationale |
|-----------|------|---------|--------|-----------|
| `max_batch_size` | usize | 1,000 | [1, 100,000] | Maximum memories deleted in a single `forget()` call. If criteria match more, the operation returns a truncation flag. |
| `tombstone_ttl_us` | u64 | 7,776,000,000,000 (90 days) | > 0 | How long tombstones are retained in meta CF. Tombstones older than this are garbage-collected. |
| `cascade_snapshots` | bool | true | -- | Whether forget cascades to revision predecessor snapshots. Default true. |
| `trigger_compaction` | bool | true | -- | Whether forget triggers async compaction on affected CFs. |

**Configuration is a struct, not a file.** In Phase 5, configuration is passed programmatically to the engine constructor or to `start_decay()`. Phase 8 will add TOML file loading and map it onto these structs. The structs are the source of truth; the file format is a convenience layer.

### 11. Concurrency Model

Phase 5 introduces the first concurrent writer beyond the main thread: the decay sweep writes updated `decay_score` values to the same default CF that `remember()`, `revise()`, and `recall()` (reinforcement) also write to.

**Why this is safe without application-level locks:**

RocksDB's write path serializes all writes through the WAL. Two concurrent `write_batch()` calls are serialized internally -- one completes, then the other. Neither sees a half-written state. The decay sweep's WriteBatch (batch of `decay_score` updates) and a concurrent `revise()` WriteBatch (memory update + index changes) are independent transactions. The only risk is a **lost update**: the decay sweep reads a memory, computes a new score, and writes it back. If `revise()` updates the same memory between the read and write, the decay sweep's write overwrites the revised version's `decay_score`.

**Mitigation: decay only writes the `decay_score` field.** The decay sweep should use a read-modify-write pattern that only touches `decay_score`, not the entire memory. However, since the memory is a single serialized blob in the default CF (not individual columns), any write replaces the entire record. This means the decay sweep must re-read the memory and apply the score to the latest version before writing.

**Practical approach:** The decay sweep's write is a stale-score correction, not a critical mutation. If a concurrent `revise()` resets the `decay_score` to `importance` (which it does), the decay sweep's slightly-stale write will be corrected on the next sweep cycle. The worst case is a memory's decay_score is off by one sweep interval's worth of decay -- negligible.

**For the hot path (recall reinforcement):** The `reinforce_memories()` method from Phase 4 already updates `last_accessed_at` and `access_count` via WriteBatch. This is a hot-path write. The decay sweep's writes are background. These two writers may interleave, but since RocksDB serializes them, no corruption occurs. The reinforcement write is authoritative for `access_count` and `last_accessed_at`; the decay sweep is authoritative for `decay_score`. They don't conflict on the same fields in any semantically important way.

**For revise() and forget():** These are caller-initiated operations on specific memories. If the decay sweep happens to be processing the same memory simultaneously, the same "last writer wins, corrected on next sweep" logic applies. `forget()` deleting a memory that the decay sweep is about to write a score for will result in a harmless write to a deleted key (RocksDB treats it as a new put, which is then garbage on the next read -- but since the memory is deleted from all indexes, it will never be read. The next sweep cycle skips it because it no longer exists in the cursor scan).

### 12. Testing Strategy

**Layer 1: Unit tests (in-crate, in-memory backend)**

- `revise()` basic: remember a memory, revise content, verify returned memory has same ID but new content, `updated_at` is bumped, `kind` is `Revision`, `decay_score` is reset
- `revise()` embedding: verify the embedding changes when content changes (mock embedder returns different vectors for different inputs)
- `revise()` context merge: verify merge mode preserves existing keys and adds new ones; replace mode replaces entirely
- `revise()` validation: reject empty content, reject oversized content, reject importance out of range, reject non-existent memory_id, reject revise-on-snapshot
- `revise()` lineage: after revision, follow `RevisedFrom` edge and verify snapshot has old content, old embedding, original timestamps
- `revise()` chain: revise twice, verify two `RevisedFrom` edges, both snapshots have correct historical content
- `revise()` index update: after revision, similarity search returns the revised memory (with new embedding), temporal query returns it, old embedding does not match
- `forget()` by explicit ID: remember, forget by ID, verify `get()` returns NotFound, similarity search returns empty, temporal scan returns empty, all graph edges removed
- `forget()` by entity: remember 5 memories for entity A and 3 for entity B, forget entity A, verify A's are gone, B's are intact
- `forget()` by staleness: remember 5 memories with old timestamps, forget by threshold, verify only stale ones removed
- `forget()` combined criteria: entity + staleness, verify AND semantics
- `forget()` tombstone: after forget, verify tombstone exists in meta CF with correct fields
- `forget()` cascade: revise a memory (creating snapshot), then forget it, verify snapshot is also removed
- `forget()` batch limit: set max_batch_size to 3, forget 10 memories, verify first call removes 3, returns truncated flag
- `forget()` non-existent ID: forgetting a non-existent ID is a no-op (not an error)
- `forget()` empty criteria: reject with InvalidInput
- `compute_decay_score()`: verify formula with known inputs -- zero age returns `importance × log₂(1 + access_count)`, age = half_life returns half of that, access_count = 0 returns `importance × 2^(-age/half_life) × log₂(1) = 0` (!!-- this is important: a never-accessed memory has score 0 regardless of age)
- Wait, `log₂(1 + 0) = log₂(1) = 0`. That means a memory with zero access_count has decay_score = 0 always. That can't be right for a freshly remembered memory.

**Formula correction needed:** The formula `importance × 2^(-age/half_life) × log₂(1 + access_count)` produces 0 when `access_count = 0`. A just-remembered memory would have score 0, which means it would be an immediate auto-forget candidate. This is wrong.

**Fix: use `log₂(2 + access_count)` or add a base of 1:** `importance × 2^(-age/half_life) × (1 + log₂(1 + access_count) / log₂(1 + reinforcement_cap))`. This normalizes the reinforcement component to [1.0, ~2.0] range, where 1.0 means no reinforcement and ~2.0 means maximum reinforcement. A never-accessed memory has a reinforcement multiplier of 1.0, not 0.0.

This is a critical design correction. The Phase 5 implementation must use the corrected formula and document why it differs from the PhasePlan.md formula.

- `compute_decay_score()` corrected: zero access_count returns `importance × 2^(-age/half_life) × 1.0 = importance × 2^(-age/half_life)`
- Decay sweep: mock 1000 memories with various ages, run sweep, verify all scores updated correctly
- Decay sweep cursor: run sweep, stop midway, restart, verify it resumes from correct position
- Auto-forget candidates: configure low threshold, run sweep, verify candidates written to meta CF

**Layer 2: Property-based tests (proptest)**

- Decay score monotonicity: for any two memories with identical fields except `last_accessed_at`, the one with the more recent access has a higher or equal score
- Decay score reinforcement: for any two memories with identical fields except `access_count`, the one with the higher count has a higher or equal score
- Decay score importance scaling: for any two memories with identical fields except `importance`, the one with higher importance has a higher or equal score
- Revise round-trip: for any valid memory and valid revision input, `revise()` followed by `get()` returns the revised content
- Forget completeness: for any set of memories, after `forget(all_ids)`, every `get()` returns NotFound AND every similarity search returns empty AND every temporal scan returns empty

**Layer 3: Integration tests (RocksDB backend, real disk)**

- Full lifecycle: remember → recall (found) → revise → recall (found with new content, old content not found via similarity) → forget → recall (empty)
- Revision lineage at scale: remember 100 memories, revise each 5 times, verify 100 primary memories exist with correct content AND 500 snapshots exist AND graph has 500 `RevisedFrom` edges
- Forget at scale: remember 10,000 memories, forget all by entity, verify zero memories remain in all indexes
- Decay sweep over 10K memories: populate 10K memories with varying ages and access counts, run one sweep, verify all scores match formula
- Decay sweep cursor recovery: populate 5K memories, run sweep for 2 batches (batch_size=1000), kill the engine, restart, verify cursor is at position 2000 and next sweep continues from there
- Concurrent revise + decay: spawn 10 threads each revising random memories while decay sweep runs, verify no panics, no corruption, all memories have valid states after completion
- Forget + recall race: spawn threads calling forget and recall on overlapping sets, verify no panics, recall never returns a forgotten memory (eventual consistency: a recall that started before the forget WriteBatch may return the memory, but any recall started after the WriteBatch must not)

**Layer 4: Criterion benchmarks**

- `revise()` single: measure p50/p99 for revising one memory (includes embedding, index update, snapshot creation)
- `revise()` batch of 100: measure throughput
- `forget()` by single ID: measure p50/p99
- `forget()` by entity (100 memories): measure total time
- `forget()` by entity (1,000 memories): measure total time and per-memory amortized cost
- `compute_decay_score()` for 10K memories: measure pure computation cost (no I/O)
- Decay sweep batch of 10K: measure wall-clock time for one batch (read + compute + write)
- Forget with cascade (memory revised 5 times): measure overhead of snapshot cleanup vs simple forget

---

## Risk Register

| Risk | Impact | Probability | Mitigation |
|------|--------|------------|------------|
| Concurrent revise() on same memory_id causes lost update (one revision's snapshot captures stale state) | Medium -- one historical version is slightly wrong | Low (requires two callers revising the exact same memory within the same millisecond) | Document last-writer-wins semantics. Phase 12 can add optimistic concurrency control (CAS on updated_at) if needed. The snapshot captures whatever state existed at read time, which is a valid historical version. |
| Decay sweep write overwrites a concurrent revise's decay_score reset | Low -- score is off by one sweep cycle | High (this will happen frequently under load) | Acceptable. Revise resets decay_score. The next sweep corrects it. The maximum error is one sweep interval's worth of decay. |
| Forget with criteria scan takes minutes on large datasets | Medium -- blocks the calling thread | Medium (O(n) scans on 10M memories are real) | Bounded batch processing with truncation flag. Document that criteria-based forget is a maintenance operation. Encourage explicit-ID forget for hot-path use. |
| Tombstone accumulation in meta CF grows unbounded | Low -- meta CF bloats | High (every forget creates tombstones) | Tombstone TTL garbage collection. A periodic (low-priority) background pass deletes tombstones older than `tombstone_ttl_us`. Included in Phase 5 scope. |
| HNSW tombstone accumulation from revise + forget degrades search quality | Medium -- search becomes slower as tombstone ratio increases | Medium (each revise creates one HNSW tombstone) | IndexManager already tracks tombstone count and provides `hnsw_needs_cleanup()` + `hnsw_cleanup()`. Phase 5 should trigger HNSW cleanup after bulk forget operations or when tombstone ratio exceeds a threshold. |
| Decay formula correction breaks expected behavior documented in PhasePlan.md | Low -- documentation inconsistency | Certain (the PhasePlan formula produces 0 for access_count=0) | Update PhasePlan.md with corrected formula. Document the rationale in code comments. |
| Predecessor snapshots consume storage without bound for frequently-revised memories | Medium -- a memory revised 1000 times has 1000 snapshots | Low (most memories are revised 0-5 times) | Configurable snapshot retention limit (e.g., keep last 10 snapshots, prune older ones). Default: unlimited for Phase 5. Phase 12 can tighten. |
| Crash between WriteBatch commit and HNSW commit during revise leaves HNSW inconsistent | High -- similarity search returns wrong results | Very low (process crash in a microsecond window) | Same recovery path as remember(): on startup, reconcile HNSW state from vector CF contents. This startup reconciliation is not new to Phase 5 but must be tested for the revise case specifically. |

---

## Deliverables Checklist

Phase 5 is done when ALL of the following are true:

- [x] `revise()` updates content, re-embeds, updates all three indexes atomically via WriteBatch
- [x] `revise()` creates predecessor snapshot in default CF with `RevisedFrom` edge
- [x] `revise()` supports content, importance, context (merge/replace), entity_id, and edge updates
- [x] `revise()` validates all inputs and rejects no-op revisions
- [x] `revise()` increments `logical_clock`, bumps `updated_at`, resets `decay_score`, sets `kind = Revision`
- [x] Predecessor snapshots are retrievable by ID but do not appear in similarity, temporal, or graph index queries
- [x] Multiple revisions produce a correct lineage chain (sorted by snapshot `created_at`)
- [x] `forget()` accepts criteria: explicit IDs, entity scope, staleness, access count, memory kind, decay score, or any AND-combination
- [x] `forget()` removes matching memories from all five CFs and in-memory HNSW
- [x] `forget()` cascades to predecessor snapshots (configurable)
- [x] `forget()` cleans up all graph edges (incoming and outgoing) for each forgotten memory
- [x] `forget()` creates tombstone records in meta CF
- [x] `forget()` processes in bounded batches with truncation flag
- [x] `forget()` of non-existent IDs is a no-op (no error)
- [x] Decay score formula correctly handles zero access_count (produces non-zero for non-zero importance)
- [x] Decay engine runs on a dedicated OS thread with channel-based control (pause, resume, shutdown, reconfigure)
- [x] Decay sweep is cursor-based, persists cursor to meta CF, wraps around
- [x] Decay sweep uses epsilon threshold to avoid unnecessary writes
- [x] Decay sweep identifies auto-forget candidates and writes them to meta CF
- [x] Auto-forget pipeline: candidate identification → policy evaluation → execution, with a default policy
- [x] Background worker abstraction is generic enough for Phase 7 (reflect) and Phase 13 (sync) reuse
- [x] All configuration parameters have documented defaults and bounds
- [x] `IndexManager` exposes a `prepare_update` method (or equivalent) for atomic re-indexing
- [x] Tombstone garbage collection runs periodically, removes tombstones older than TTL
- [x] No `unwrap()` or `expect()` on any path reachable by external input
- [x] No `unsafe` blocks
- [x] All unit tests pass (in-memory backend)
- [x] All property-based tests pass
- [x] All integration tests pass (RocksDB backend)
- [x] Criterion benchmarks established for revise, forget, decay sweep, and compute_decay_score
- [x] `cargo clippy` passes with zero warnings
- [x] `cargo fmt --check` passes
- [x] `cargo audit` passes
- [x] PhasePlan.md updated with Phase 5 completion marker and known issues

---

## Interfaces Published to Future Phases

Phase 5 creates contracts that later phases depend on. These interfaces are stable after Phase 5 and should not change without a documented migration plan.

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| `revise()` input contract and return type | Phase 7 (reflect revises insights), Phase 8 (server handler), Phase 10 (Python SDK), Phase 13 (sync conflict resolution) | Additive only (new optional fields, never remove or rename) |
| `forget()` criteria specification and return type | Phase 7 (insight invalidation triggers), Phase 8, Phase 10, Phase 12 (GDPR compliance), Phase 13 (authoritative forget in sync) | Additive only (new criteria types can be added) |
| Tombstone format in meta CF | Phase 12 (GDPR audit trail and proof-of-deletion reporting) | Append-only (new fields can be added to tombstone records) |
| `RevisedFrom` edge semantics and lineage chain structure | Phase 7 (insight re-evaluation when source is revised), Phase 13 (sync merge for revised memories) | Immutable (the meaning of a `RevisedFrom` edge cannot change) |
| Predecessor snapshot storage model (in default CF, not indexed) | Phase 7 (may need to read old versions during insight invalidation) | Immutable |
| Decay score formula and parameter semantics | Phase 7 (reflect policy may use decay score as a trigger condition), Phase 8 (exposed via configuration), Phase 12 (tuning for production) | Formula changes require a migration sweep |
| Background worker abstraction (thread + channel + cursor) | Phase 7 (reflect pipeline), Phase 13 (sync worker) | The trait/struct interface is additive only |
| `DecayConfig` and `ForgetConfig` structs | Phase 8 (TOML config file maps onto these), Phase 12 (production tuning) | Additive only |
| Auto-forget candidate format in meta CF | Phase 7 (policy layer reads candidates and applies reflect-aware rules), Phase 12 | Additive only |
| `IndexManager::prepare_update` method | Internal to `hebbs-index` consumers, primarily `hebbs-core` | Signature stable after Phase 5 |
