# TASK-08: Prime Similarity — Replace Brute-Force with Entity-Partitioned HNSW

**Status:** Deprioritized. Brute-force stopgap works for current scale. Two-vault pattern ([PLAN-daemon](./plans/PLAN-daemon.md)) keeps per-entity memory counts small enough that O(n) scan is acceptable. Revisit when per-entity counts reach thousands.

**Priority:** Below [PLAN-daemon](./plans/PLAN-daemon.md) (cold-start latency is a bigger problem than per-query scan latency) and [PLAN-12](./plans/PLAN-12.md) Features 2-3.

---

The current fix for prime's dead similarity component (brute-force cosine scan over entity memories) is correct but does not scale. This task tracks the long-term solution: entity-aware HNSW search that eliminates both the old post-filter fragility and the new O(n) linear scan.

## Current State (Post-Fix)

Prime's similarity phase was broken: it searched the global HNSW index and post-filtered by `entity_id` with `ENTITY_OVERSAMPLE=4`. When the target entity was a small fraction of total memories, the post-filter discarded most candidates, returning `similarity=0`.

The fix (`engine.rs`) replaced this with an entity-scoped brute-force scan:

1. Query temporal index with full time range (0..now) to get all entity memory IDs (capped at `PRIME_ENTITY_SCAN_LIMIT=500`).
2. Load each memory's embedding from storage.
3. Compute `cosine_similarity(cue_embedding, memory_embedding)` for each.
4. Sort by relevance, take top `similarity_limit`.

This works and all tests pass, but has known limitations.

## Problems with the Brute-Force Approach

### 1. Latency regression
- **Old:** One HNSW search O(log n * ef_search) + ~40 point lookups for post-filter. ~1-2ms.
- **New:** Up to 500 storage reads + 500 dot products (384-dim). ~25-50ms.
- Prime is called at conversation start (agent priming), so latency matters for perceived responsiveness.
- **Mitigated by daemon:** With [PLAN-daemon](./plans/PLAN-daemon.md), per-query latency is the only cost (no cold-start). 25-50ms brute-force is acceptable when you're not also paying 150-600ms for ONNX model load.

### 2. The 500 cap is lossy
- `PRIME_ENTITY_SCAN_LIMIT=500` queries the temporal index in `ReverseChronological` order, so it gets the 500 most recent entity memories.
- An older but highly relevant memory at position 501+ is invisible to similarity. The old HNSW approach, despite its filtering problem, at least searched the full vector space.
- The cap is necessary (Principle 4: Bounded Everything), but it introduces a recency bias in what should be a purely semantic ranking.
- **Mitigated by two-vault pattern:** With global + project vault separation, per-entity counts in any single vault are naturally smaller. A `user_prefs` entity in the global vault might have 20-50 memories, not thousands.

### 3. O(n) does not scale like O(log n)
- HNSW search is O(log n * ef_search). Brute-force is O(n * d).
- At 500 memories this is sub-millisecond. At the `MAX_PRIME_MEMORIES=200` output cap, the scan is fine.
- But if `PRIME_ENTITY_SCAN_LIMIT` needs to increase for entities with deep history, the linear scan becomes a bottleneck.
- **When this becomes urgent:** Enterprise use cases with many agents writing to the same vault, or long-lived project vaults accumulating thousands of memories per entity over months. Also relevant if PLAN-daemon's `--all` flag merges results across vaults, increasing the effective candidate set.

### 4. Error propagation change
- The old code silently swallowed embedding failures with `if let Ok(...)`, still returning temporal results.
- The new code uses `?`, propagating the error and failing the entire prime call.
- This is arguably better (no silent data loss), but changes partial-failure semantics. An agent calling prime during conversation start would get an error instead of degraded-but-partial results.

### 5. Dedup still zeros out similarity_count for small entities
- When all entity memories are recent and temporal captures them all, dedup removes every similarity result. `similarity_count` remains 0 even though the similarity phase is working.
- This is mathematically correct (no unique similarity contributions) but misleading to users/agents inspecting the counts.

## Proposed Long-Term Solution: Entity-Partitioned HNSW

### Option A: Per-Entity HNSW Graphs
Maintain a separate HNSW graph per entity (similar to how tenant graphs are already partitioned in `IndexManager.hnsw_graphs`).

**Pros:**
- O(log n_entity * ef_search) search, where n_entity << n_total.
- No post-filtering, no brute-force scan.
- Exact same search quality as global HNSW but scoped to entity.

**Cons:**
- Memory overhead: one graph per entity. Entities with 1-2 memories get a full HNSW structure.
- Insert cost: each `remember()` must insert into both global and entity-specific graphs.
- Graph lifecycle: need eviction/compaction for dormant entities (LRU similar to tenant graph eviction).

### Option B: HNSW with Pre-Filter Labels
Add entity_id as a label/tag on HNSW nodes. Modify the search to accept a filter predicate that prunes during graph traversal (not after).

**Pros:**
- Single graph, no memory overhead per entity.
- O(log n * ef_search) with filter applied during traversal — no wasted candidates.
- Standard approach in production vector databases (Pinecone, Qdrant, Weaviate all do this).

**Cons:**
- Requires modifying the HNSW implementation (`hebbs-index/src/hnsw/`) to support filtered search.
- Filter during traversal can reduce recall quality if the entity's memories are clustered in a sparse region of the graph.
- More complex implementation than Option A.

### Option C: Hybrid — Brute-Force Below Threshold, HNSW Above
Keep the current brute-force for entities with < N memories (where N ~ 200-500), switch to entity-partitioned HNSW for larger entities.

**Pros:**
- Brute-force is actually faster than HNSW for small n (no graph overhead).
- Only builds per-entity HNSW graphs for high-volume entities that need it.
- Simplest incremental path from current state.

**Cons:**
- Two code paths to maintain.
- Threshold tuning needed.

## Recommendation

**Option B (HNSW with pre-filter labels)** is the right long-term answer. It's the industry-standard approach, keeps a single graph, and scales to arbitrary entity counts without per-entity overhead. The current brute-force fix is acceptable as a stopgap while Option B is implemented.

**Updated priority (2026-03-14):** The two-vault pattern and daemon architecture ([PLAN-daemon](./plans/PLAN-daemon.md)) reduce the urgency of this optimization. Per-entity memory counts stay naturally small when vaults are scoped to individual projects and user identity is in the global vault. The brute-force scan at 20-100 memories per entity is sub-millisecond. Prioritize PLAN-daemon (eliminates 150-600ms cold-start per command) and PLAN-12 Features 2-3 (contradiction detection, token-budgeted prime) before this task.

**Trigger to reprioritize:** When benchmarks or production telemetry show prime latency > 10ms at p99 due to entity scan, or when `--all` multi-vault merge pushes candidate sets above 500.

**Last reviewed (2026-03-19, commit 452bf15):** All five issues confirmed still present. Brute-force scan active in `engine.rs:1046-1084`, no filtered HNSW implemented, post-filter fragility in `execute_similarity()` unchanged, 500 cap in place. No action needed at current scale.

## Scope

### Files Affected
- `hebbs-index/src/hnsw/graph.rs` -- Add filtered search method
- `hebbs-index/src/hnsw/node.rs` -- Add entity label storage on nodes
- `hebbs-index/src/manager.rs` -- Add `search_vector_filtered()` API
- `hebbs-core/src/engine.rs` -- Replace brute-force prime similarity with filtered HNSW call
- `hebbs-core/src/engine.rs` -- Also fix `execute_similarity()` in recall (same post-filter fragility)

### Interaction with Other Work
- **PLAN-daemon:** The daemon holds the engine warm, so the HNSW graph stays in memory. This makes Option B even more effective since graph traversal hits hot pages.
- **PLAN-12 Feature 3 (token-budgeted prime):** Token budgeting reduces the number of results returned but not the number of candidates scanned. This task reduces candidate scan cost, Feature 3 reduces output cost. They are complementary.
- **`--all` multi-vault merge (PLAN-daemon Milestone 5):** Merging results across vaults runs separate searches per vault. This task optimizes each individual vault's search; the merge is additive.

## Acceptance Criteria

1. `prime(entityId="initech", similarityCue="enterprise evaluation")` returns `similarity_count > 0` when entity has memories semantically related to the cue, even when those memories overlap with temporal results.
2. Prime similarity latency at p99 <= 5ms for entities with up to 10K memories (benchmark required).
3. No regression in recall similarity accuracy (existing recall tests pass).
4. Criterion benchmark comparing brute-force vs filtered HNSW at 1K, 10K, 100K total memories with varying entity fractions.
