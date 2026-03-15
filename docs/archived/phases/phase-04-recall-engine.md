# Phase 4: Recall Engine -- Architecture Blueprint

## Status: ✅ COMPLETE

---

## Intent

Phase 4 is the phase where HEBBS delivers its core value proposition. Before this phase, memories go in and come back by ID or by raw index query. After this phase, a caller says "I need this kind of knowledge" and the engine finds it — by meaning, by time, by causation, or by structural analogy. This is the difference between a database and a cognitive engine.

This phase does not create a new crate. All recall logic lives in `hebbs-core`, orchestrating the three indexes from `hebbs-index` and the embedding engine from `hebbs-embed`. The crate boundary is deliberate: `hebbs-core` is where cognitive strategy lives. Index queries are mechanical; recall strategy is judgment. The engine decides what to ask each index, how to combine answers, and how to rank the final result.

By the end, two new public operations exist: `recall()` and `prime()`. Four recall strategies work independently. Multi-strategy recall runs strategies in parallel, merges results, and produces a single ranked list. Every recall hit reinforces the accessed memories, strengthening them against future decay. And `prime()` gives frameworks a single call that loads the right context before an agent's turn.

The decisions made here — strategy selection semantics, scoring formula, merge algorithm, reinforcement model, the boundary between "find" and "rank" — are load-bearing for Phase 5 (decay scoring interacts with access_count), Phase 6 (subscribe reuses the similarity search path), Phase 7 (reflect queries memories through recall internally), and Phase 8 (gRPC exposes recall with strategy as a protobuf enum).

---

## Scope Boundaries

### What Phase 4 delivers

- `recall()` operation with four strategy variants: Similarity, Temporal, Causal, Analogical
- Multi-strategy recall: run 2+ strategies in parallel, merge, deduplicate, rank by composite score
- `prime()` operation: framework-oriented context loading combining temporal recency and similarity relevance
- Composite scoring function combining relevance, recency, importance, and reinforcement signal
- Reinforcement on recall: every returned memory gets `last_accessed_at` and `access_count` updated
- `RecallInput` and `RecallResult` types defining the public API contract
- Criterion benchmarks for each strategy at 100K+ memories and multi-strategy merge overhead
- Property-based tests for score monotonicity, deduplication correctness, and strategy independence

### What Phase 4 explicitly does NOT deliver

- `revise()` (Phase 5). The recall engine reads memories but does not modify their content.
- `forget()` with criteria-based bulk deletion (Phase 5). Recall returns results; it does not prune.
- Decay scoring recalculation on read (Phase 5). Phase 4 stores `decay_score` on write and uses the stored value for ranking. Recalculating decay on every recall would add unbounded computation to the read path.
- `subscribe()` streaming (Phase 6). Subscribe is continuous associative recall — it reuses the similarity search path but has fundamentally different lifetime and push semantics.
- Bloom filter pre-screening (Phase 6). The hierarchical filtering pipeline belongs to subscribe, not to single-shot recall.
- Any networking, gRPC, or configuration file concerns (Phase 8).
- Async execution. Phase 4 is synchronous. The `Engine` does not own a tokio runtime. Multi-strategy parallelism uses `std::thread::scope` for bounded thread-per-strategy execution without runtime overhead.

---

## Architectural Decisions

### 1. The `recall()` Operation — API Shape

`recall()` is the second most important operation in HEBBS after `remember()`. Its interface defines how every agent, framework, and SDK interacts with the read path. Getting the API right is more important than getting the implementation fast — a bad API requires every downstream consumer to change.

**Input contract:**

- `cue`: required, the query signal. A text string that is embedded for similarity/analogical strategies or parsed for entity_id extraction in temporal/causal strategies. Max length: same as `content` (64KB).
- `strategy`: required, one or more strategies to execute. Explicit — the caller decides the retrieval mode, not the engine. This is Principle 3 (cognition, not storage): the caller must express intent, not just query.
- `top_k`: optional, defaults to 10. Maximum number of results to return per strategy before merge. Bounded at 1000 (Principle 4).
- `entity_id`: optional. Required for temporal strategy, optional hint for others. When provided, scopes temporal and causal queries. When absent for temporal strategy, the engine attempts to extract entity_id from the cue's context or returns an error.
- `time_range`: optional. Constrains temporal strategy to a time window `(start_us, end_us)`. Defaults to full range.
- `edge_types`: optional. Constrains causal strategy to specific edge types. Defaults to all edge types.
- `max_depth`: optional. Bounds causal graph traversal depth. Defaults to 5. Hard maximum 10 (Principle 4, ScalabilityArchitecture.md).
- `ef_search`: optional. Overrides the default HNSW ef_search parameter for this query. Allows callers to trade latency for recall quality on a per-query basis.

**Output contract:**

- `results`: a `Vec<RecallResult>` ranked by composite score, descending. Deduplicated by `memory_id` even when multiple strategies find the same memory.
- `strategy_details`: per-result metadata indicating which strategy found the memory and the strategy-specific match signal (distance for similarity, depth for causal, rank position for temporal).
- `truncated`: a flag per strategy indicating whether the result set was limited by `top_k` before merge.

**Why strategy is explicit, not automatic:** The temptation is to build an "auto" mode that guesses the best strategy from the cue. This is wrong for Phase 4. Auto-selection requires understanding the cue's intent — is it a temporal question ("what happened last week?"), a causal question ("what caused this?"), or a similarity question ("what's related to this?"). Intent detection is an LLM task. Putting an LLM on the read path violates Principle 1 (no network calls on hot path) and Principle 5 (expensive computation runs in background). The caller — the agent or framework — knows what kind of knowledge it needs. The engine provides the retrieval mechanism, not the retrieval judgment. An "auto" mode may be added in a later phase as a convenience that runs a lightweight classifier, but Phase 4 delivers the explicit primitives.

### 2. The Four Recall Strategies

Each strategy is an independent retrieval path with its own latency profile, index dependency, and relevance signal. They share a common output format but have different internal mechanics.

#### 2a. Similarity Recall

**What it answers:** "What memories are semantically close to this cue?"

**Pipeline:**

1. Embed the cue text using the engine's `Embedder`. Cost: ~3ms.
2. Query the HNSW index via `IndexManager::search_vector(embedding, top_k, ef_search)`. Cost: ~5ms at 1M.
3. For each `(memory_id, distance)` result, fetch the full `Memory` from the default CF. Cost: ~100µs per point lookup.
4. Compute relevance score: `relevance = 1.0 - distance`. For L2-normalized vectors with inner product distance, this maps to [0.0, 1.0] where 1.0 is identical.
5. Return `Vec<(Memory, relevance, StrategyDetail::Similarity { distance })>`.

**Complexity:** O(embed) + O(log n * ef_search) + O(k * point_lookup).

**Latency budget:** 10ms p99. embed (3ms) + HNSW search (5ms) + deserialize k=10 (2ms).

This is the baseline strategy. It is the one every vector database provides. HEBBS must be at least as good here before the other strategies add value.

#### 2b. Temporal Recall

**What it answers:** "What happened with this entity, in chronological order?"

**Pipeline:**

1. Extract `entity_id` from the explicit parameter or from the cue. If neither is available, return an error — temporal recall requires an entity scope.
2. Query the temporal index via `IndexManager::query_temporal(entity_id, start_us, end_us, order, top_k)`. Cost: ~2ms at 1M.
3. For each `(memory_id, timestamp)` result, fetch the full `Memory` from the default CF.
4. Relevance score: derived from rank position. The most recent memory (or earliest, depending on order) gets the highest relevance, decaying linearly across the result set. `relevance = 1.0 - (rank / top_k)`.
5. Return `Vec<(Memory, relevance, StrategyDetail::Temporal { timestamp, rank })>`.

**Complexity:** O(log n + k).

**Latency budget:** 5ms p99. B-tree range scan (2ms) + deserialize k=10 (3ms).

**Why rank-based relevance instead of time-based:** Time-based relevance (newer = more relevant) is one heuristic but not always correct. A memory from 6 months ago might be the most important if the entity has been dormant. Rank-based relevance says "the temporal index already sorted these; the position in the sorted result is the relevance signal." The composite scoring function (Decision 3) then combines this with importance and recency to produce the final ranking.

#### 2c. Causal Recall

**What it answers:** "What memories are causally connected to this one?"

**Pipeline:**

1. Identify the seed memory. Two modes:
   - If the cue is a memory_id (detected by format: 32 hex characters), use it directly as the seed. No embedding needed.
   - If the cue is text, embed it, run a similarity search with k=1 to find the closest memory, and use that as the seed. This is the "what caused something like this?" mode.
2. Traverse the graph index via `IndexManager::traverse(seed_id, edge_types, max_depth, max_results)`. Cost: ~10ms at depth 5.
3. For each `TraversalEntry { memory_id, depth, edge_type }`, fetch the full `Memory` from the default CF.
4. Relevance score: inversely proportional to graph depth. `relevance = 1.0 - (depth / max_depth)`. Direct connections are most relevant; distant connections fade.
5. Return `Vec<(Memory, relevance, StrategyDetail::Causal { depth, edge_type, seed_id })>`.

**Complexity:** O(embed or point_lookup) + O(branching_factor^max_depth) + O(k * point_lookup).

**Latency budget:** 15ms p99. seed lookup (3ms) + bounded graph walk (10ms) + deserialize (2ms).

**Why the seed can be text or ID:** In practice, agents rarely know the exact memory_id of a causal chain's origin. They know the situation: "what led to the deal falling through?" The engine embeds this, finds the closest memory ("deal lost: Acme Corp Q3"), and walks backward through `caused_by` edges. This bridges the gap between human intent and graph structure.

#### 2d. Analogical Recall

**What it answers:** "What memories have a similar structure, even in a different domain?"

This is the hardest strategy and the most differentiated. Similarity recall finds content-similar memories. Analogical recall finds structurally-similar memories — same process stage, same entity pattern, same relational shape — even when the content is different.

**Pipeline:**

1. Embed the cue text. Cost: ~3ms.
2. Query HNSW with an expanded candidate set: `ef_search` is doubled (or explicitly overridden). This casts a wider net because the final ranking uses a different signal than pure embedding distance. Cost: ~7ms at 1M.
3. For each candidate memory, compute two scores:
   - `embedding_similarity`: `1.0 - distance` (same as similarity recall).
   - `structural_similarity`: a normalized score based on shared context structure between the cue and the candidate.
4. Final analogical relevance: `relevance = α * embedding_similarity + (1 - α) * structural_similarity`, where `α` is configurable (default 0.5).
5. Re-rank candidates by the analogical relevance score.
6. Return top_k after re-ranking.

**Structural similarity computation:**

Structural similarity measures overlap in metadata shape, not content. Given the cue's context and a candidate memory's context, the score is computed from:

- **Key overlap:** Fraction of context keys shared between the cue and candidate. If both have `stage`, `outcome`, `entity_type` keys, that is high overlap.
- **Value type match:** For shared keys, whether the values are the same type (string-string, number-number). Type match without value match indicates structural analogy ("same kind of situation, different specifics").
- **Entity pattern match:** Whether the `entity_id` patterns are similar in structure (same prefix format, same namespace) but different in identity.
- **Kind match:** Whether both memories are the same `MemoryKind`.

The score is a weighted sum of these signals, normalized to [0.0, 1.0]. The weights are configurable but the defaults are: key_overlap (0.4), value_type_match (0.3), kind_match (0.2), entity_pattern (0.1).

**Why this is not just "similarity with a filter":** Filtering by context keys before HNSW search would miss memories that are structurally analogous but have slightly different context schemas. The two-phase approach (wide HNSW search → structural re-rank) finds memories that are both semantically adjacent and structurally analogous. The wider HNSW search catches candidates that pure similarity would rank lower but structural analogy makes highly relevant.

**Complexity:** O(embed) + O(log n * 2 * ef_search) + O(candidates * structural_compare) + O(k * point_lookup).

**Latency budget:** 10ms p99. This is the same as similarity because the wider HNSW search is offset by the fact that structural comparison is pure in-memory computation on deserialized context bytes — no additional I/O.

### 3. Composite Scoring and Result Ranking

When a strategy returns raw results, each result carries a strategy-specific relevance score. But the final ranking presented to the caller must account for more than relevance. A highly relevant but ancient memory with zero access count should rank below a somewhat-relevant, recent, well-reinforced memory.

**The composite score formula:**

```
composite = w_relevance * relevance
          + w_recency  * recency_signal
          + w_importance * importance
          + w_reinforcement * reinforcement_signal
```

Where:

| Component | Derivation | Range |
|-----------|-----------|-------|
| `relevance` | Strategy-specific (see Decision 2) | [0.0, 1.0] |
| `recency_signal` | `1.0 - (now_us - created_at) / max_age_us`, clamped to [0, 1]. `max_age_us` is configurable (default: 30 days). | [0.0, 1.0] |
| `importance` | The memory's stored `importance` field | [0.0, 1.0] |
| `reinforcement_signal` | `min(1.0, log2(1.0 + access_count) / log2(1.0 + reinforcement_cap))`. Logarithmic to prevent popular memories from dominating. `reinforcement_cap` is configurable (default: 100). | [0.0, 1.0] |

**Default weights:**

| Weight | Default | Rationale |
|--------|---------|-----------|
| `w_relevance` | 0.5 | Relevance is the primary signal — the strategy found this memory for a reason |
| `w_recency` | 0.2 | Temporal freshness matters for most agent workloads |
| `w_importance` | 0.2 | The caller's importance scoring at write time carries weight |
| `w_reinforcement` | 0.1 | Reinforcement prevents useful memories from decaying, but should not dominate |

**Why these weights are configurable, not hardcoded:** Different workloads have different ranking needs. A voice sales agent wants high recency weight (yesterday's call matters more than last month's). A coding agent wants high relevance weight (the correct API docs matter regardless of when they were indexed). A research agent wants high importance weight (landmark findings matter regardless of recency). The defaults are sensible for the common case. Phase 8 exposes these as configuration parameters.

**Why not use `decay_score` directly?** The `decay_score` field combines importance, age, and access_count into a single number. Using it directly would collapse three independent ranking signals into one pre-computed value that cannot be reweighted per query. The composite scoring function keeps the signals separate so that per-query weight overrides are possible. Phase 5's decay engine recalculates `decay_score` for the background sweep, but the recall ranking uses the decomposed signals.

### 4. Multi-Strategy Recall — Parallel Execution and Merge

Multi-strategy recall is what makes HEBBS categorically different from any vector database. No other system offers "run temporal + similarity + causal in parallel, merge, deduplicate, rank by composite score" as a single operation.

**Execution model:**

When the caller specifies multiple strategies, the engine runs them in parallel using `std::thread::scope`. Each strategy executes on its own thread with a shared `&self` reference to the engine. This is safe because:

- HNSW search acquires a shared read lock (parking_lot `RwLock`). Multiple readers proceed concurrently.
- Temporal and graph queries go directly to RocksDB, which is internally thread-safe for concurrent reads.
- The embedder (`Arc<dyn Embedder>`) is `Send + Sync` by trait bound.

The engine embeds the cue once and shares the resulting vector across strategies that need it (similarity, analogical, causal-with-text-seed). Strategies that do not need the embedding (temporal, causal-with-id-seed) skip it.

**Merge algorithm:**

1. Collect all `(Memory, relevance, strategy_detail)` tuples from all strategies.
2. Group by `memory_id`. If the same memory was found by multiple strategies, keep the highest relevance score and record all strategies that found it (multi-strategy provenance).
3. Compute the composite score for each unique memory (Decision 3).
4. Sort by composite score, descending.
5. Truncate to the caller's `top_k`.
6. Return the final ranked list.

**Deduplication is by memory_id, not by content.** Two different memories with identical content are two results. The same memory found by two strategies is one result with a composite score boosted by appearing in multiple strategy results (this is implicit — the highest relevance is kept, and a memory with high relevance in both temporal and similarity will naturally score higher in the composite).

**Why parallel, not sequential with early termination:** Sequential execution would allow the engine to skip a strategy if the first one already produced high-confidence results ("I found 10 memories with > 0.95 relevance from similarity, skip causal"). This sounds clever but violates Principle 3 (cognition, not storage). The caller asked for multi-strategy for a reason — they want diverse retrieval paths. Skipping a strategy changes the result set semantics. If the caller wants single-strategy, they specify one strategy.

**Merge cost:** The merge itself is pure in-memory computation: deduplication via HashSet on memory_id, scoring via arithmetic, sorting via unstable sort. For k=10 results per strategy across 4 strategies, merge operates on ≤40 items. Cost: microseconds. Negligible.

### 5. Reinforcement — Access Count and Timestamp Updates

Every memory returned by `recall()` receives a reinforcement signal: `last_accessed_at` is updated to now, and `access_count` is incremented. This is the mechanism by which frequently-useful memories resist decay (Phase 5).

**Implementation: synchronous WriteBatch after result construction.**

After the recall results are scored, ranked, and ready to return, the engine constructs a WriteBatch containing a `Put` for each returned memory with updated `last_accessed_at` and `access_count` fields. The batch is executed before returning results.

**Why synchronous, not background:**

- A background reinforcement task would require a channel or queue (tokio, crossbeam). Phase 4 does not have a background task runtime. Adding one for reinforcement alone is premature.
- The WriteBatch cost is bounded: k memories × (16-byte key + ~1.7KB serialized memory) ≈ 17KB for k=10. RocksDB writes this in under 1ms including WAL.
- The total latency budget allows it. Similarity recall has 10ms p99; the HNSW search takes ~5ms and embed takes ~3ms, leaving ~2ms for deserialization + reinforcement.

**What is updated:**

| Field | Update |
|-------|--------|
| `last_accessed_at` | Set to current timestamp (microseconds) |
| `access_count` | Incremented by 1 |
| `updated_at` | NOT changed (this field tracks content changes from `revise()`, not access) |
| `decay_score` | NOT recalculated (deferred to Phase 5's background decay sweep) |

**Index consistency during reinforcement:** The reinforcement WriteBatch only updates the default CF (memory records). The temporal index key includes `created_at` (not `last_accessed_at`), so no temporal index update is needed. The vectors CF stores the embedding (unchanged). The graph CF stores edges (unchanged). Reinforcement is a default-CF-only operation and requires no cross-index atomic write.

**Edge case: concurrent recall on the same memory.** Two concurrent `recall()` calls both return memory M and both attempt to increment `access_count`. Because RocksDB serializes writes internally, one Write will complete before the other. The second write will use a stale `access_count` (it read the value before the first write committed). The count will be incremented by 1 instead of 2. This is acceptable. Reinforcement is a statistical signal, not a financial transaction. Losing one increment out of thousands is inconsequential. Fixing this would require read-modify-write with a lock or a RocksDB merge operator, both of which add complexity for negligible benefit.

### 6. The `prime()` Operation

`prime()` is the framework-integration point. Before every agent turn, the orchestration framework calls `prime()` with the current context. The engine returns a pre-loaded set of relevant memories that the framework injects into the agent's prompt.

**How `prime()` differs from `recall()`:**

| Aspect | `recall()` | `prime()` |
|--------|-----------|-----------|
| Caller | Agent (deliberate) or framework | Framework (automatic, every turn) |
| Strategy | Explicit, caller-specified | Implicit: always temporal + similarity |
| Cue | Single text query | Structured context (entity, stage, recent input) |
| Focus | Precision (find the best matches) | Coverage (load a coherent context window) |
| Output | Ranked results | Deduplicated, chronologically-ordered context set |

**Input contract:**

- `entity_id`: required. Prime always scopes to an entity — the agent is working on something specific.
- `context`: optional structured map. Provides additional context keys for the similarity component (e.g., `stage`, `topic`, `recent_input`).
- `max_memories`: optional, defaults to 20. The maximum number of memories to return. Bounded at 200.
- `recency_window`: optional, defaults to 7 days. How far back the temporal component looks.
- `similarity_cue`: optional. A text cue for the similarity component. If not provided, the engine constructs a cue from the context values.

**Pipeline:**

1. **Temporal component:** Query the temporal index for the entity's most recent memories within `recency_window`. Retrieve up to `max_memories / 2` results in reverse chronological order. This ensures the agent has recent history.
2. **Similarity component:** If `similarity_cue` is provided, embed it and search HNSW with `top_k = max_memories / 2`. If not, concatenate context values into a synthetic cue and embed that. This surfaces relevant knowledge that may not be recent.
3. **Merge:** Deduplicate by `memory_id`. Apply composite scoring. Sort by composite score.
4. **Reinforcement:** Same as `recall()` — update `last_accessed_at` and `access_count` for all returned memories.
5. **Return:** The merged, scored, deduplicated set.

**Why `prime()` always uses temporal + similarity:** These are the two strategies most relevant for framework pre-loading. Temporal provides history ("what happened recently with this entity"). Similarity provides knowledge ("what do I know that's relevant to the current topic"). Causal and analogical are deliberate strategies that the agent invokes mid-task, not strategies that frameworks use for automatic context loading.

**Why the output is ordered differently:** `recall()` returns results ranked by composite score (most relevant first). `prime()` returns results in a hybrid order: temporal results in chronological order interleaved with similarity results by relevance. This produces a context window that reads naturally: "here's what happened recently, and here's what's relevant." The exact interleaving algorithm is: first all temporal results in chronological order, then all non-duplicate similarity results by relevance. This gives the agent a narrative followed by supplementary knowledge.

### 7. Cue Embedding — The Shared Computation

Similarity, Analogical, and Causal (text-seed mode) all require embedding the cue text. This is the dominant cost on the read path (~3ms). The engine must embed the cue exactly once, even when multiple strategies need it.

**Implementation:** Before dispatching strategies to parallel threads, the engine checks which strategies require an embedding. If any do, embed the cue once on the main thread. Pass the resulting `Vec<f32>` as a shared reference to all threads that need it.

This saves 3ms per additional strategy that needs the embedding. For multi-strategy recall with [Similarity, Analogical, Causal], the embedding cost is 3ms total, not 9ms.

**Why embed before spawning threads, not inside each thread:** Embedding requires `&self.embedder`, which is behind an `Arc`. Sharing it across threads is safe, but the ONNX session inside the embedder is behind a `Mutex`. Two threads calling `embed()` simultaneously would serialize on the mutex, offering no parallelism and adding lock contention overhead. Embedding once on the main thread and sharing the result is both simpler and faster.

### 8. Error Semantics on the Recall Path

Recall can fail in several ways. The error taxonomy established in Phase 1 covers all cases, but the recall-specific semantics need definition.

| Failure | Error type | Recovery |
|---------|-----------|----------|
| Empty cue | `InvalidInput` | Reject before any computation |
| Temporal strategy without entity_id | `InvalidInput` | Caller must provide entity_id for temporal |
| Causal strategy with invalid seed (memory not found) | `MemoryNotFound` | Clear error: "causal recall seed memory not found" |
| Embedding failure | `Embedding` | Propagated from embedder. Caller retries or falls back. |
| HNSW search with empty index | Returns empty results, not an error | No memories yet — this is valid |
| Graph traversal from unconnected seed | Returns the seed only, not an error | Memory exists but has no edges |
| Reinforcement write failure | Logged, results still returned | Reinforcement is best-effort. Do not fail the recall because the access count could not be updated. |
| One strategy fails in multi-strategy | Return results from successful strategies, include error details for the failed one | Partial results are better than no results |

**The partial-failure decision for multi-strategy is deliberate.** If a caller requests [Similarity, Temporal, Causal] and the causal seed is not found, similarity and temporal still return useful results. Failing the entire recall because one strategy could not complete would be hostile API design. The response includes both the successful results and the per-strategy error information.

### 9. Memory Footprint Analysis

Phase 4 adds no new persistent data structures. Recall is a read-path operation that queries existing indexes and reads existing memories. The only write is reinforcement (updating existing records in the default CF, same size).

**Transient allocation during recall:**

| Component | Size | Lifecycle |
|-----------|------|-----------|
| Embedded cue vector | 384 * 4 = 1,536 bytes | Allocated once per recall, freed on return |
| HNSW search candidates buffer | ~ef_search * 20 bytes ≈ 2KB | Internal to HNSW search, freed on return |
| Deserialized Memory records (k=10) | ~1.7KB * 10 = 17KB | Allocated per result, owned by caller |
| Strategy result buffers | ~2KB per strategy | Per-thread, freed on join |
| Merge dedup set | ~16 * k bytes | Transient, freed on return |

**Total transient per recall:** ~25KB for k=10 with single strategy, ~45KB for 4-strategy multi-recall. Negligible. No concern for memory pressure.

**No new in-memory caches or indexes.** Phase 4 does not introduce any resident memory structures. All query state is transient. The existing HNSW in-memory graph (from Phase 3) is the dominant RAM consumer and is unchanged.

### 10. Testing Strategy

**Layer 1: Unit tests (in `hebbs-core`)**

Recall mechanics:
- Each strategy returns correct results for a known dataset (pre-populated memories with known embeddings, entities, and edges).
- Similarity strategy returns memories ranked by embedding distance.
- Temporal strategy returns memories in correct chronological/reverse-chronological order.
- Causal strategy traverses edges to correct depth, respects edge type filter.
- Analogical strategy re-ranks candidates by structural similarity, not just embedding distance.
- Multi-strategy deduplicates correctly: same memory from two strategies appears once in results.
- Composite scoring produces expected rankings for controlled inputs (known relevance, recency, importance, access_count).
- Empty index returns empty results (no error, no panic).
- Temporal strategy without entity_id returns `InvalidInput` error.
- Causal strategy with missing seed returns `MemoryNotFound` error.
- Multi-strategy with one failing strategy returns partial results.

Prime mechanics:
- `prime()` returns temporal + similarity results, deduplicated.
- `prime()` respects `recency_window` — memories older than the window are excluded from the temporal component.
- `prime()` constructs a synthetic cue from context values when `similarity_cue` is not provided.

Reinforcement:
- After `recall()`, returned memories have updated `last_accessed_at` and `access_count`.
- Reinforcement is idempotent: two sequential recalls on the same memory increment `access_count` by 1 each.
- Reinforcement failure does not prevent results from being returned.

Scoring:
- Composite score is monotonically increasing with relevance (for fixed recency, importance, reinforcement).
- Composite score is monotonically increasing with recency (for fixed relevance, importance, reinforcement).
- Weight overrides change ranking as expected (zero weight on recency produces ranking independent of age).

**Layer 2: Property-based tests**

- For any set of memories, recall with `top_k = N` (where N >= total memories) returns all memories (no dropped results).
- For any multi-strategy recall, the result set is a subset of the union of individual strategy results (merge only removes duplicates, never drops unique results).
- Composite score is always in [0.0, sum_of_weights] (bounded output from bounded inputs).
- Reinforcement: `access_count` after `n` recalls is ≥ `n` (concurrent races may undershoot but never overshoot).
- Deduplication: for any result set, no two results share the same `memory_id`.

**Layer 3: Integration tests (with RocksDB)**

- Full lifecycle: `remember` 1,000 memories with entities, embeddings, and edges. Run each strategy. Verify results are correct and complete.
- Recall at scale: insert 10K memories, run similarity recall, verify results are semantically reasonable (the top result for a query embedding is the memory with the closest embedding, verified by brute force).
- Multi-strategy consistency: run the same recall as single-strategy and multi-strategy. Verify that multi-strategy results contain all single-strategy results (for the same `top_k`).
- Reinforcement persistence: recall a memory, restart the engine, verify `access_count` survived.
- Concurrent recall: 10 threads running recall simultaneously against the same engine. No panics, no corrupted results.
- Prime round-trip: `remember` 50 memories for an entity over simulated time, `prime(entity)` returns recent history + relevant knowledge.
- Edge case: recall on empty database returns empty results for all strategies.
- Edge case: causal recall from a memory with no edges returns only the seed.

**Layer 4: Criterion benchmarks**

- `recall(Similarity)` at 10K, 100K, 1M memories. Measure p50/p99. Target: < 10ms p99 at 1M.
- `recall(Temporal)` at 10K, 100K, 1M memories. Measure p50/p99. Target: < 5ms p99 at 1M.
- `recall(Causal)` at depth 3, 5 at 100K edges. Measure p50/p99. Target: < 15ms p99.
- `recall(Multi: [Similarity, Temporal])` at 100K memories. Measure p50/p99. Target: < 20ms p99.
- `recall(Analogical)` at 100K memories. Measure p50/p99. Target: < 10ms p99.
- `prime()` at 100K memories. Measure p50/p99. Target: < 20ms p99.
- Reinforcement overhead: measure recall with and without reinforcement. Delta should be < 2ms.
- Multi-strategy merge overhead (in isolation): merge 40 results from 4 strategies. Target: < 100µs.
- Composite scoring (in isolation): score 100 memories. Target: < 50µs.

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| Analogical recall structural similarity is too naive — context key overlap does not capture true structural analogy | Medium — analogical strategy returns results no better than similarity | Design the structural similarity as a pluggable scoring function. Phase 4 ships the key-overlap heuristic. Phase 7's reflect pipeline can learn better structural features and feed them back to analogical scoring. If the heuristic is useless, it degrades gracefully to similarity (α=1.0). |
| Composite scoring weights are wrong for common workloads — users get poor default rankings | Medium — users override weights on every call, defeating the purpose of defaults | Benchmark the defaults against the voice sales agent use case (UseCaseAnalysis.md). Validate that the default weights produce intuitive rankings for temporal questions, similarity questions, and mixed questions. Adjust defaults based on empirical evidence, not intuition. |
| Reinforcement WriteBatch adds latency that blows the recall p99 budget | Medium — recall latency exceeds contract | Benchmark reinforcement overhead in isolation. If it exceeds 2ms, consider: (a) writing reinforcement asynchronously via a spawned thread that fires and forgets, (b) batching reinforcement updates across multiple recalls (write every N recalls), (c) deferring reinforcement to Phase 5's background task infrastructure. |
| `std::thread::scope` per multi-strategy recall has high thread creation overhead | Low — thread creation is ~10µs on modern OS, negligible vs 10ms HNSW search | Benchmark the thread spawn/join overhead. If it exceeds 500µs, consider a thread pool (rayon) or accept that multi-strategy adds a small constant overhead. At 4 threads for 4 strategies, the expected overhead is ~40µs. |
| Cue embedding failure blocks all strategies that need it, even strategies that do not | Low — temporal strategy does not need embedding | The implementation embeds before dispatching. If embedding fails and the requested strategies include temporal (which does not need embedding), the temporal results are still useful. Handle this by attempting embedding, and if it fails, only execute strategies that do not require it. Return partial results with an error for the embedding-dependent strategies. |
| `prime()` synthetic cue from context values produces poor embeddings | Medium — similarity component of prime returns irrelevant results | The synthetic cue is a concatenation of context values. If the context is sparse (e.g., only `entity_id`), the cue is low-quality. Mitigation: require `similarity_cue` to be provided if high-quality similarity results are needed. When constructing a synthetic cue, include the entity_id, recent content snippets from temporal results, and context values. Test synthetic cue quality against a labeled dataset. |
| Partial failure in multi-strategy recall confuses callers who expect all-or-nothing semantics | Low — documented behavior | Document clearly in the API that multi-strategy recall is partial-success. Return structured per-strategy status in the response. SDKs (Phase 11, 15, 16) can wrap this in a convenience method that raises on any failure if the caller prefers strict semantics. |

---

## Deliverables Checklist

Phase 4 is done when ALL of the following are true:

- [x] `recall()` accepts a cue text and strategy enum, returns ranked `Vec<RecallResult>`
- [x] Similarity strategy: embeds cue, queries HNSW, returns memories ranked by embedding distance
- [x] Temporal strategy: queries temporal index by entity_id and time range, returns chronologically ordered memories
- [x] Causal strategy: finds seed memory (by ID or by embedding closest match), traverses graph, returns connected memories ranked by depth
- [x] Analogical strategy: embeds cue, queries HNSW with wider search, re-ranks by structural similarity, returns memories ranked by composite embedding + structural score
- [x] Multi-strategy recall: runs 2+ strategies in parallel, merges, deduplicates by memory_id, ranks by composite score
- [x] Composite scoring combines relevance, recency, importance, and reinforcement signal with configurable weights
- [x] `prime()` accepts entity_id and context, runs temporal + similarity, returns merged context set
- [x] Reinforcement: every memory returned by recall/prime has `last_accessed_at` and `access_count` updated via synchronous WriteBatch
- [x] Reinforcement failure does not prevent results from being returned
- [x] Multi-strategy partial failure returns results from successful strategies with per-strategy error details
- [x] Cue is embedded at most once per recall, shared across strategies
- [x] Empty index returns empty results for all strategies (no error, no panic)
- [x] Temporal strategy without entity_id returns `InvalidInput` error
- [x] Causal strategy with missing seed returns `MemoryNotFound` error
- [x] `top_k` is bounded at 1000; `max_depth` is bounded at 10
- [x] Property tests pass for deduplication, score bounds, and reinforcement monotonicity
- [x] Integration tests pass for full recall lifecycle at 1K+ memories, concurrent recall, prime round-trip
- [ ] `recall(Similarity)` < 10ms p99 at 100K memories (Criterion benchmark) — benchmarks defined, 100K scale run deferred to Phase 12
- [ ] `recall(Temporal)` < 5ms p99 at 100K memories (Criterion benchmark) — benchmarks defined, 100K scale run deferred to Phase 12
- [ ] `recall(Causal)` < 15ms p99 at 100K edges (Criterion benchmark) — benchmarks defined, 100K scale run deferred to Phase 12
- [ ] `recall(Multi: [Similarity, Temporal])` < 20ms p99 at 100K memories (Criterion benchmark) — benchmarks defined, 100K scale run deferred to Phase 12
- [ ] `prime()` < 20ms p99 at 100K memories (Criterion benchmark) — benchmarks defined, 100K scale run deferred to Phase 12
- [x] No `unwrap()` or `expect()` on any path reachable by external input
- [x] No `unsafe` blocks
- [x] `cargo clippy` passes with zero warnings
- [x] `cargo fmt --check` passes

---

## Interfaces Published to Future Phases

Phase 4 creates contracts that later phases depend on. These interfaces are stable after Phase 4 and should not change without a documented migration plan.

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| `recall()` input contract (cue, strategy, top_k, entity_id, time_range, max_depth, ef_search) | 5, 6, 7, 8, 10, 11, 15, 16 | Additive only. New optional parameters allowed. Existing parameters never change semantics. |
| `RecallResult` struct (memory, score, strategy_detail) | 8 (gRPC response mapping), 10 (Rust SDK), 11 (Python SDK) | Additive only. New fields allowed. Existing fields immutable. |
| Recall strategy enum (Similarity, Temporal, Causal, Analogical) | 8 (protobuf enum), 10, 11, 15, 16 | Append-only. New variants may be added (never removed or renumbered). |
| `prime()` input contract (entity_id, context, max_memories, recency_window, similarity_cue) | 8, 10, 11 | Additive only. |
| Composite scoring weights interface | 8 (server config), 10 (client builder) | Stable. Defaults may change between major versions with documentation. |
| Reinforcement semantics (`last_accessed_at` + `access_count` on every recall hit) | 5 (decay formula depends on access_count), 7 (reflect uses access_count to identify frequently-recalled memories) | Immutable. Phase 5's decay formula `importance * 2^(-age/half_life) * log(1 + access_count)` depends on this reinforcement path existing and being reliable. |
| Partial-failure semantics for multi-strategy recall | 8 (gRPC per-strategy status), 10, 11 | Immutable. Downstream consumers rely on partial results being available when one strategy fails. |
