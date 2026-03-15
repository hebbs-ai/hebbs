# Phase Plan: Building HEBBS End-to-End

This document lays out the complete build sequence to go from zero to a production-usable HEBBS system. No timelines -- just the dependency-ordered work and the definition of done for each phase.

**Prerequisites:** Read [DocsSummary.md](DocsSummary.md) for the full documentation index, reading order, and cross-references between documents.

---

## Engineering Standard

**You are implementing a system that will be deployed in government-grade, top-level security environments.** Every line of code will be scrutinized by security auditors, performance engineers, and formal reviewers who will examine every allocation, every branch, every data flow, and every failure mode. This is not a startup prototype. This is infrastructure that critical systems will depend on.

**Code to the standard of the world's best systems engineer -- someone who is obsessive about latency, paranoid about correctness, and relentless about algorithmic efficiency.** Every function must justify its existence. Every data structure must be the optimal choice for its access pattern. Every allocation must be intentional.

### What this means in practice

**Latency obsession:**
- Know the cost of every operation you write. Not approximately -- exactly. Count the allocations, count the cache lines, know the branch prediction behavior.
- Choose algorithms by their worst-case complexity, not average-case. An O(n) scan hidden inside an O(log n) path makes the whole path O(n). Find it. Fix it.
- Profile before and after every change. Use Criterion benchmarks. Use `perf`, `flamegraph`, `dhat`. If you cannot prove a change is faster or neutral, it does not merge.
- Latency budgets in [GuidingPrinciples.md](GuidingPrinciples.md) are hard contracts, not guidelines. `recall` similarity at 10ms p99 means 10ms. Not 11ms. Not "usually 10ms." 10ms under load, under contention, under compaction, at 10M memories.

**Algorithm and data structure rigor:**
- Use the provably optimal data structure for each access pattern. B-tree for temporal range scans. HNSW for approximate nearest neighbor. Adjacency lists for bounded graph traversal. Do not default to `HashMap` or `Vec` without analyzing the access pattern.
- Every search, sort, and traversal must have documented time complexity in code comments. `O(log n)` lookup, `O(k * ef_search)` HNSW query, `O(d * branching_factor)` graph walk where `d` is bounded.
- Avoid quadratic or worse complexity anywhere in the codebase. If an operation touches N items, it must be O(N log N) or better. No exceptions.
- Batch operations must amortize overhead. Embedding 128 texts must not be 128x the cost of embedding 1 text.

**Security-grade implementation:**
- Every input is hostile. Validate at the boundary: length limits, UTF-8 validity, depth limits on nested structures, range checks on numeric parameters. Reject malformed input before it touches any engine internals.
- No `unsafe` blocks without a written safety invariant comment explaining exactly why the block is sound, what invariants the caller must uphold, and what happens if they are violated. Every `unsafe` block is a liability in a security audit.
- No `unwrap()` or `expect()` on paths reachable by external input. Use proper error handling (`Result` + `?`). Panics in production are audit failures.
- Tenant isolation is structural: separate column family prefixes, validated at the storage layer, not the application layer. A logic bug in query planning must not be able to cross tenant boundaries.
- Memory must be zeroized after use for sensitive data paths. Use `zeroize` crate for credentials, API keys, and any data subject to compliance requirements.
- `forget()` is real deletion: data is removed from all indexes, all column families, and WAL compaction is triggered past the deletion point. A forensic examiner must not be able to recover forgotten data after compaction completes.

**Correctness as a non-negotiable:**
- Crash safety at every point. If the process is killed between any two instructions, the database must recover to a consistent state. Test this with fault injection (kill -9 during writes, fsync failures, disk full scenarios).
- Atomic multi-index updates via RocksDB WriteBatch. If the temporal index has a record, the vector index and graph index must also have corresponding records. Partial states are corruption.
- Every edge case must have a test: empty database recall, forget non-existent memory, revise non-existent memory, recall with zero results, subscribe with no matching memories, reflect with insufficient memories to cluster, concurrent remember + forget on the same memory, WAL replay after crash mid-write.
- Property-based testing (using `proptest` or `quickcheck`) for serialization round-trips, index consistency invariants, and decay score monotonicity.

**Code quality:**
- Functions do one thing. If a function has the word "and" in its description, it is two functions.
- Public API surfaces are minimal. Expose the operation, not the mechanism. Internal types stay internal.
- Error messages are actionable. "Failed to recall" is useless. "Recall failed: HNSW index not initialized for tenant 'acme' -- run remember() first to populate the index" is actionable.
- No dead code, no commented-out code, no TODO comments in merged code. If something is not done, it is tracked in PhasePlan.md, not in a source comment.

---

## Enforcement

**Every phase is governed by [GuidingPrinciples.md](GuidingPrinciples.md).** The principles are not aspirational -- they are constraints. A phase is not done if its output violates any principle. In particular:

- **Latency budgets (Principle 1)** are enforced from Phase 4 onward with Criterion benchmarks. A phase that regresses p99 latency by > 10% is not done.
- **Atomic multi-index writes (Principle 11)** are required from Phase 3 onward. Partial index states are treated as corruption bugs.
- **Bounded resource usage (Principle 4)** is validated at every phase. Every data structure must have a calculable memory footprint.
- **Observability (Principle 9)** is built incrementally: each phase adds metrics for the operations it introduces.
- **Security audit readiness (Principle 12)** is continuous: no phase may introduce `unsafe` without justification, `unwrap()` on external paths, or unvalidated input handling.

---

## Phase 1: Storage Foundation ✅ COMPLETE

**Goal:** A Rust workspace that can persist and retrieve raw memory records from RocksDB.

**Status:** Complete. All deliverables met. See [phases/phase-01-storage-foundation.md](phases/phase-01-storage-foundation.md) for the full architecture blueprint and completed checklist.

### Work

1. ✅ Initialize the Cargo workspace with `hebbs-storage` and `hebbs-core` crates.
2. ✅ Define the core `Memory` data structure:
   - `memory_id` (ULID -- sortable, unique)
   - `content` (text)
   - `importance` (f32)
   - `context` (structured metadata, serde JSON — stored as pre-serialized JSON bytes in bitcode)
   - `created_at`, `updated_at`, `last_accessed_at` (timestamps, u64 microseconds)
   - `access_count` (u64)
   - `embedding` (Option<Vec<f32>>, None in Phase 1)
   - `decay_score` (f32)
   - `entity_id` (optional, for scoping)
   - `kind` (Episode/Insight/Revision enum)
   - `device_id` (optional, for edge sync)
   - `logical_clock` (u64, for conflict resolution)
3. ✅ Implement `hebbs-storage`:
   - RocksDB initialization with five column families: `default`, `temporal`, `vectors`, `graph`, `meta`.
   - Storage trait with put/get/delete/write_batch/prefix_iterator/range_iterator/compact.
   - RocksDB backend (production) and in-memory BTreeMap backend (testing).
   - Write path: serialize `Memory` and persist to the default column family.
   - Read path: get by `memory_id`.
   - Delete path: remove by `memory_id`.
   - Iterator: scan by prefix (entity_id filtering in Phase 1, temporal index in Phase 3).
4. ✅ Implement basic `remember()` in `hebbs-core`:
   - Accept content, importance, context, entity_id.
   - Validate all inputs before I/O.
   - Generate monotonic ULID, set timestamps.
   - Persist via `hebbs-storage`.
   - Return full `Memory` struct.

### Done When

- ✅ `cargo test` passes with round-trip write/read/delete of memory records (69 tests).
- ✅ Data survives process restart (RocksDB durability — integration test verified).
- ✅ Workspace compiles as a single `cargo build` (zero warnings).

### Benchmark Baseline (in-memory backend, release build)

| Operation | Latency |
|-----------|---------|
| `remember()` single (200B content) | 384 ns |
| `remember()` with structured context | 618 ns |
| `get()` point lookup | 114 ns |
| `get()` miss (non-existent key) | 102 ns |
| `remember()` batch × 10,000 | 3.04 ms (304 ns/op) |
| `serialize_memory` (bitcode) | 43 ns |
| `deserialize_memory` (bitcode) | 107 ns |
| `delete()` single | 84 ns |

---

## Phase 2: Embedding Engine ✅ COMPLETE

**Goal:** Every `remember()` call automatically generates a vector embedding stored alongside the memory.

**Status:** Complete. All deliverables met. See [phases/phase-02-embedding-engine.md](phases/phase-02-embedding-engine.md) for the full architecture blueprint.

### Work

1. ✅ Create the `hebbs-embed` crate.
2. ✅ Integrate ONNX Runtime (via `ort` crate v2.0.0-rc.11) with BGE-small-en-v1.5 model support (384 dimensions).
3. ✅ Define the `Embedder` trait:
   - `embed(text: &str) -> Result<Vec<f32>>`
   - `embed_batch(texts: &[&str]) -> Result<Vec<Vec<f32>>>`
   - `dimensions() -> usize`
   - All returned vectors are L2-normalized (unit length).
   - Trait requires `Send + Sync` for concurrent use.
4. ✅ Ship the default ONNX model as a downloadable asset (not compiled into binary). Auto-download on first run with SHA-256 checksum verification.
5. ✅ `external-embeddings` feature flag compiles. Pluggable embedder architecture in place via trait.
6. ✅ Wire `hebbs-core::remember()` to call the embedder before persisting. `Engine::new` now requires `Arc<dyn Embedder>`.

### Additional Deliverables

- ✅ `OnnxEmbedder`: Production embedding provider with ONNX Runtime, supporting CoreML/CUDA/DirectML execution providers via feature flags.
- ✅ `MockEmbedder`: Deterministic hash-based embedder for testing (no model files required).
- ✅ L2 normalization utilities (`l2_normalize`, `is_normalized`, `cosine_similarity`) with property-based tests.
- ✅ Model management: auto-download from Hugging Face with SHA-256 verification, local caching, offline mode.
- ✅ Batch inference with configurable max batch size (bounded at 256 per ONNX call, Principle 4).
- ✅ Mean pooling and CLS pooling strategies for token-to-sentence embedding aggregation.
- ✅ Thread-safe `OnnxEmbedder` via `parking_lot::Mutex` wrapping `ort::Session`.
- ✅ `EmbedError` integrated into `hebbs-core::HebbsError` via `#[from]`.

### Done When

- ✅ `remember("some text", ...)` stores a 384-dim f32 vector in the memory record.
- ✅ Embedding generation is < 10ms on CPU (MockEmbedder benchmarked; OnnxEmbedder awaiting model download for live benchmark).
- ✅ Pluggable embedder compiles behind `--features external-embeddings`.

### Test Coverage (130 tests total across workspace)

- 53 unit tests in `hebbs-embed` (normalize, mock, config, model, onnx pooling).
- 30 unit tests in `hebbs-core` (engine, keys, memory — updated for embedding integration).
- 16 integration tests in `hebbs-core` (including embedding persistence, restart survival, concurrent embedding, different-content-different-embedding).
- 13 property tests in `hebbs-core` (serialization round-trips, embedding invariants: always present, normalized, deterministic, survives storage round-trip).
- 18 unit tests in `hebbs-storage` (unchanged from Phase 1).
- Property-based tests for L2 normalization (idempotent, always unit, cosine bounds/symmetry) and MockEmbedder (normalized, deterministic, batch equivalence).

### Known Issues

- `ort` v2.0.0-rc.11 is a pre-release crate. Will upgrade to stable when released.
- `paste` crate (transitive dependency of `tokenizers`) flagged as unmaintained (RUSTSEC-2024-0436). No security impact — it is a proc-macro crate generating no runtime code.

---

## Phase 3: Index Layer ✅ COMPLETE

**Goal:** Three specialized indexes enabling the four recall strategies.

**Status:** Complete. All deliverables met. See [phases/phase-03-index-layer.md](phases/phase-03-index-layer.md) for the full architecture blueprint and completed checklist.

### Work

#### 3a: Temporal Index ✅
1. ✅ B-tree index on `(entity_id, timestamp)` stored in the `temporal` column family.
2. ✅ Range query: given an `entity_id`, return memories ordered by time (ascending/descending).
3. ✅ Time-windowed query: memories between `t1` and `t2`.

#### 3b: Vector Index (HNSW) ✅
1. ✅ Custom HNSW implementation in Rust (`hebbs-index` crate). No third-party HNSW dependencies.
2. ✅ HNSW graph structure persisted node-by-node in the `vectors` column family.
3. ✅ Insert: add embedding on `remember()` with atomic WriteBatch.
4. ✅ Query: given a query embedding, return top-K nearest neighbors. Recall@10 > 85% verified at 100–2K scale.
5. ✅ Delete: tombstone-based lazy deletion with configurable threshold cleanup.
6. ✅ Startup recovery: full HNSW rebuild from `vectors` CF via re-insertion algorithm.
7. ✅ Parameters: M=16, M_max=32, ef_construction=200, ef_search=100, ml=1/ln(16).

#### 3c: Graph Index ✅
1. ✅ Bidirectional adjacency lists in the `graph` column family.
2. ✅ Edge types: `caused_by` (0x01), `related_to` (0x02), `followed_by` (0x03), `revised_from` (0x04 reserved), `insight_from` (0x05 reserved).
3. ✅ Forward key: `[0xF0][source 16B][edge_type 1B][target 16B]`. Reverse key: `[0xF1][target_id 16B][edge_type 1B][source 16B]`.
4. ✅ Edge metadata: confidence (f32) + timestamp (u64) = 12 bytes per edge.
5. ✅ Insert: when `remember()` includes edges, create bidirectional entries atomically.
6. ✅ Traverse: BFS with bounded depth and max results, cycle detection, truncation flag.
7. ✅ Delete: scan + remove all forward/reverse edges touching the deleted memory.

#### 3d: Unified Index Manager ✅
1. ✅ Created `hebbs-index` crate with `IndexManager` coordinating all three indexes.
2. ✅ Two-phase commit pattern: `prepare_insert()` → `WriteBatch` → `commit_insert()` for atomicity.
3. ✅ On `remember()`: update default CF + temporal CF + vectors CF + graph CF in single WriteBatch.
4. ✅ On `delete()`: remove from all four CFs atomically, tombstone in-memory HNSW.
5. ✅ Thread safety: `RwLock<HnswGraph>` for reader-writer separation (parking_lot).

### Additional Deliverables

- ✅ `Engine::remember()` rewritten for Phase 3 pipeline: validate → embed → prepare_insert → WriteBatch → commit_insert.
- ✅ `Engine::delete()` rewritten: read memory → prepare_delete → WriteBatch → commit_delete (tombstone HNSW).
- ✅ `Engine::search_similar()`: HNSW top-K nearest neighbor search via IndexManager.
- ✅ `Engine::query_temporal()`: time-range queries via IndexManager.
- ✅ `Engine::traverse_graph()`: bounded BFS traversal via IndexManager.
- ✅ `Engine::list_by_entity()`: upgraded from O(n) full scan (Phase 1) to O(log n + k) temporal index lookup.
- ✅ `RememberInput` extended with `edges: Vec<RememberEdge>` for explicit graph edge creation.
- ✅ `HebbsError::Index` variant wrapping `IndexError` for downstream consumers.
- ✅ HNSW neighbor selection heuristic promoting graph connectivity diversity.
- ✅ HNSW tombstone cleanup with configurable threshold ratio (default 10%).
- ✅ Criterion benchmarks for HNSW insert, search, rebuild, temporal range query, and distance computation.

### Done When

- ✅ Temporal range query returns memories in correct order for a given entity.
- ✅ HNSW top-10 query returns semantically relevant results with > 85% recall@10 (verified at 100, 500, 1K, 2K scale).
- ✅ Graph traversal returns connected memories up to specified depth with cycle detection.
- ✅ All three indexes are updated in a single atomic write batch.
- ✅ All three indexes are cleaned up on delete in a single atomic write batch.
- ✅ HNSW in-memory graph is rebuilt from `vectors` CF on startup (verified via rebuild test).
- ✅ Concurrent search during insert produces correct results (integration test).
- ✅ Tombstone cleanup produces a valid, searchable graph (integration test).

### Test Coverage (205 tests total across workspace)

- 64 unit tests in `hebbs-index` (distance, params, node serialization, HNSW graph, temporal, graph, IndexManager).
- 8 integration tests in `hebbs-index` (full lifecycle, recall quality at multiple scales, graph edge lifecycle, temporal ordering, HNSW rebuild, concurrent access, multi-hop traversal, tombstone cleanup).
- 33 unit tests in `hebbs-core` (engine with Phase 3 pipeline, keys, memory — updated for multi-index writes).
- 16 integration tests in `hebbs-core` (including multi-index persistence, concurrent writes, restart survival).
- 13 property tests in `hebbs-core` (serialization, key ordering, embedding invariants).
- 53 unit tests in `hebbs-embed` (unchanged from Phase 2).
- 18 unit tests in `hebbs-storage` (unchanged from Phase 1).

### Known Issues

- `ort` v2.0.0-rc.11 remains a pre-release crate (carried from Phase 2). Will upgrade to stable when released.
- `paste` crate unmaintained advisory (RUSTSEC-2024-0436) — no security impact (carried from Phase 2).
- Recall@10 benchmark at 100K and 1M scale pending Criterion benchmark runs. Algorithmic correctness verified at smaller scales. Full-scale benchmarks will run during Phase 12 (Testing and Benchmark Suite).
- SIMD-optimized distance computation deferred to post-profiling optimization. Scalar implementation is correct and the data layout supports future SIMD alignment.

---

## Phase 4: Recall Engine ✅ COMPLETE

**Goal:** The `recall()` operation works across all four strategies, and `prime()` works for framework pre-loading.

**Status:** Complete. All deliverables met. See [phases/phase-04-recall-engine.md](phases/phase-04-recall-engine.md) for the full architecture blueprint with architectural decisions, risk register, and deliverables checklist.

### Work

1. ✅ Implement `recall()` in `hebbs-core` with a `strategy` parameter:
   - `Similarity`: embed the cue, query HNSW, return top-K ranked by embedding distance.
   - `Temporal`: extract entity_id, query temporal index, return chronologically ordered list.
   - `Causal`: find seed memory (by ID or by embedding closest match), traverse graph edges with bounded depth.
   - `Analogical`: embed the cue, query HNSW with wider search, re-rank by composite embedding + structural similarity.
2. ✅ Implement multi-strategy recall: run 2+ strategies in parallel (std::thread::scope), merge and deduplicate results by memory_id, rank by composite score (relevance + recency + importance + reinforcement).
3. ✅ Implement composite scoring: `composite = w_r * relevance + w_t * recency + w_i * importance + w_a * reinforcement`. Configurable weights with sensible defaults.
4. ✅ Implement `prime(entity_id, context)`:
   - Run temporal recall for recent entity history + similarity recall for relevant knowledge.
   - Merge, deduplicate, return chronological history followed by supplementary knowledge.
5. ✅ Update `last_accessed_at` and `access_count` on every recall hit via synchronous WriteBatch (reinforcement).
6. ✅ Define `RecallInput`, `RecallResult`, `PrimeInput`, `PrimeResult`, `PrimeOutput`, `RecallOutput`, `RecallStrategy`, `StrategyDetail`, `ScoringWeights`, and `AnalogicalWeights` types in `hebbs-core::recall`.
7. ✅ Handle partial failures in multi-strategy: return successful strategy results even when one strategy fails.

### Additional Deliverables

- ✅ `StrategyContext` parameter struct for clean strategy dispatch (eliminated clippy too-many-arguments warning).
- ✅ Cue embedding computed at most once per recall, shared across strategies via pre-dispatch embedding.
- ✅ Structural similarity computation for analogical recall: key overlap (Jaccard), value type matching, kind matching.
- ✅ Reinforcement is best-effort: WriteBatch failure logged but does not prevent results from being returned.
- ✅ `prime()` builds synthetic cue from entity_id + context values when no explicit similarity_cue provided.
- ✅ All bounds enforced: `top_k` capped at 1000, `max_depth` capped at 10, `max_memories` (prime) capped at 200.
- ✅ Empty index returns empty results for all strategies (no error, no panic).
- ✅ Temporal strategy without entity_id returns per-strategy error (not hard failure for multi-strategy).
- ✅ Causal strategy with missing seed returns per-strategy error with actionable message.

### Done When

- ✅ Each of the four strategies returns correct results against a test dataset of 1,000+ memories.
- ✅ Multi-strategy recall deduplicates and ranks correctly across all strategy combinations.
- ✅ Analogical recall re-ranks by structural similarity, producing different rankings than pure similarity.
- ✅ `prime()` returns temporal history + relevant knowledge, deduplicated.
- ✅ Reinforcement updates persist: `access_count` survives engine restart.
- ✅ Criterion benchmarks defined for all recall strategies at 1K and 10K scales (100K awaiting Phase 12 full-scale benchmark runs).

### Test Coverage (251 tests total across workspace)

- 66 unit tests in `hebbs-core` (engine with Phase 4 recall/prime, keys, memory).
- 25 integration tests in `hebbs-core` (including full recall lifecycle at 1K, multi-strategy consistency, reinforcement persistence, concurrent recall, prime round-trip, causal with edges, recall at 10K scale, empty database edge cases).
- 17 property tests in `hebbs-core` (serialization, key ordering, embedding invariants, recall deduplication, composite score bounds, reinforcement monotonicity, multi-strategy subset-of-union).
- 53 unit tests in `hebbs-embed` (unchanged from Phase 2).
- 64 unit tests in `hebbs-index` (unchanged from Phase 3).
- 8 integration tests in `hebbs-index` (unchanged from Phase 3).
- 18 unit tests in `hebbs-storage` (unchanged from Phase 1).

### Known Issues

- `ort` v2.0.0-rc.11 remains a pre-release crate (carried from Phase 2). Will upgrade to stable when released.
- `paste` crate unmaintained advisory (RUSTSEC-2024-0436) — no security impact (carried from Phase 2).
- Recall latency benchmarks at 100K and 1M scale pending full Criterion benchmark runs (Phase 12). Algorithmic correctness verified at 1K and 10K scales. MockEmbedder is used for benchmarks; OnnxEmbedder will add ~3ms embedding latency in production.
- SIMD-optimized distance computation deferred to post-profiling optimization (carried from Phase 3).

---

## Phase 5: Write Path Completion (Revise, Forget, Decay) ✅ COMPLETE

**Goal:** Complete the write-side API -- `revise()`, `forget()`, and automatic decay.

**Status:** Complete. All deliverables met. See [phases/phase-05-write-path-completion.md](phases/phase-05-write-path-completion.md) for the full architecture blueprint and completed checklist.

### Work

1. ✅ **`revise(memory_id, ReviseInput)`:**
   - Load existing memory, validate inputs.
   - Create predecessor snapshot (fresh ULID, stored in default CF, NOT indexed in HNSW/temporal/graph).
   - Update content, re-embed via `Embedder`, update all three indexes atomically via `IndexManager::prepare_update` + WriteBatch.
   - Add `RevisedFrom` edge from primary memory to snapshot for lineage tracking.
   - Bump `updated_at`, increment `logical_clock`, reset `decay_score` to new importance, set `kind = Revision`.
   - Supports content, importance, context (merge/replace modes), entity_id, and additive edge updates.
   - In-place update preserves memory_id: all external references and graph edges remain valid (Model B).

2. ✅ **`forget(ForgetCriteria)`:**
   - Accept criteria: explicit IDs, entity scope, staleness threshold, access count floor, memory kind, decay score floor, or any AND-combination.
   - Remove matching memories from all five CFs (default, temporal, vectors, graph forward + reverse) and in-memory HNSW atomically via WriteBatch.
   - Cascade to predecessor snapshots (configurable via `ForgetConfig::cascade_snapshots`).
   - Create `Tombstone` records in meta CF with content hash (SHA-256), criteria description, and cascade count.
   - Process in bounded batches (configurable `max_batch_size`, default 1000) with truncation flag.
   - Trigger async compaction on affected CFs (configurable via `ForgetConfig::trigger_compaction`).
   - Non-existent IDs are no-ops (no error). Empty criteria rejected with `InvalidInput`.

3. ✅ **Decay engine (background task):**
   - Dedicated OS thread via `std::thread::Builder` + `crossbeam-channel` for control signals (Resume, Pause, Shutdown, Reconfigure).
   - Configurable half-life (default 30 days), sweep interval (default 1 hour), batch size (default 10,000).
   - Corrected decay formula: `decay_score = importance × 2^(−age / half_life) × (1 + log₂(1 + min(access_count, cap)) / log₂(1 + cap))`.
   - Reinforcement multiplier range [1.0, 2.0]: zero accesses = 1.0 (not 0.0), maximum accesses = 2.0.
   - Cursor-based sweep with persistence to meta CF, wrap-around, epsilon threshold for write avoidance.
   - Auto-forget candidates written to meta CF, not deleted directly (separation of identification and execution).
   - `auto_forget()` method reads candidates and executes via standard `forget()` path.
   - Tombstone garbage collection via `gc_tombstones()` removes tombstones older than configurable TTL (default 90 days).

4. ✅ **Background worker infrastructure:**
   - Generic pattern: thread + channel + cursor + bounded batch -- reusable for Phase 7 (reflect) and Phase 13 (sync).
   - Engine lifecycle: `start_decay()`, `pause_decay()`, `resume_decay()`, `stop_decay()`, `reconfigure_decay()`.
   - `DecayHandle` with `Drop` implementation for clean shutdown.

5. ✅ **Index layer extension:**
   - `IndexManager::prepare_update()`: generates atomic `BatchOperation`s for revise (temporal re-key, vector re-index, graph edge creation).
   - Two-phase commit: prepare → WriteBatch → commit for HNSW consistency.

### Additional Deliverables

- ✅ `ReviseInput` with `ContextMode` (Merge/Replace), `is_noop()` validation, `new_content()` constructor.
- ✅ `ForgetCriteria` with `by_ids()`, `by_entity()`, `is_empty()`, `is_id_only()` helpers.
- ✅ `ForgetOutput` with `forgotten_count`, `cascade_count`, `truncated`, `tombstone_count`.
- ✅ `ForgetConfig` with `max_batch_size`, `tombstone_ttl_us`, `cascade_snapshots`, `trigger_compaction`.
- ✅ `DecayConfig` with `half_life_us`, `sweep_interval_us`, `batch_size`, `max_batches_per_sweep`, `auto_forget_threshold`, `epsilon`, `reinforcement_cap`, `enabled` -- all with validated bounds.
- ✅ `Tombstone` struct with bitcode serialization, content SHA-256 hash, tombstone key encoding for range-scan by time.
- ✅ `compute_decay_score()` pure function with corrected formula (documented deviation from original PhasePlan formula).
- ✅ Full lifecycle integration test: remember → recall → revise → recall (updated) → forget → recall (empty).
- ✅ Predecessor snapshot not in similarity search (verified by unit and integration tests).
- ✅ Revision chain with multiple snapshots (verified for 2+ revisions).
- ✅ Concurrent revise across threads with no corruption (integration test: 5 threads × 20 memories).

### Done When

- ✅ `revise()` updates content, embedding, and all indexes atomically; old version traceable via `RevisedFrom` graph edge.
- ✅ `forget()` cleanly removes from all indexes with no dangling references; tombstones created for audit.
- ✅ Decay scores decrease over time and increase on access (reinforcement); zero access_count produces non-zero score.
- ✅ Integration tests cover the full lifecycle: remember → recall → revise → recall (updated) → forget → recall (empty).
- ✅ Criterion benchmarks established for revise, forget, decay computation, and tombstone serialization at 1K and 10K scales.

### Test Coverage (304 tests total across workspace)

- 97 unit tests in `hebbs-core` (engine with Phase 5 revise/forget/decay, keys, memory -- 31 new Phase 5 tests).
- 37 integration tests in `hebbs-core` (including revise roundtrip, revise persistence across restart, re-embedding, predecessor snapshots, forget by ID/entity, tombstone creation/GC, cascade deletion, similarity cleanup, concurrent revise, full lifecycle -- 12 new Phase 5 tests).
- 27 property tests in `hebbs-core` (serialization, key ordering, embedding invariants, decay monotonicity properties, tombstone roundtrip, revise/forget correctness -- 10 new Phase 5 tests).
- 53 unit tests in `hebbs-embed` (unchanged from Phase 2).
- 64 unit tests in `hebbs-index` (unchanged from Phase 3).
- 8 integration tests in `hebbs-index` (unchanged from Phase 3).
- 18 unit tests in `hebbs-storage` (unchanged from Phase 1).

### Dependencies Added

- `crossbeam-channel = "0.5"` -- channel-based control signals for background decay worker thread.
- `sha2` (workspace dependency) -- content hashing for tombstone audit records.

### Known Issues

- `ort` v2.0.0-rc.11 remains a pre-release crate (carried from Phase 2). Will upgrade to stable when released.
- `paste` crate unmaintained advisory (RUSTSEC-2024-0436) -- no security impact (carried from Phase 2).
- SIMD-optimized distance computation deferred to post-profiling optimization (carried from Phase 3).
- Decay formula in the Work section above uses the corrected formula. The original PhasePlan formula (`log₂(1 + access_count)`) produces 0 for `access_count = 0`, making freshly remembered memories immediate auto-forget candidates. The corrected formula uses a normalized reinforcement multiplier in [1.0, 2.0]. See `phase-05-write-path-completion.md` Section 6 for full analysis.
- Decay sweep cursor recovery across restarts is implemented but not integration-tested with a kill-mid-sweep scenario. Cursor persistence verified via unit tests. Full crash-recovery testing deferred to Phase 12.
- Snapshot retention is unlimited. A memory revised 1000 times produces 1000 snapshots. Configurable retention limit deferred to Phase 12.

---

## Phase 6: Subscribe (Associative Real-Time Recall) ✅ COMPLETE

**Goal:** The `subscribe()` operation streams relevant memories in real-time as an agent processes input.

**Status:** Complete. All deliverables met. See [phases/phase-06-subscribe.md](phases/phase-06-subscribe.md) for the full architecture blueprint with architectural decisions, hierarchical filtering pipeline design, and deliverables checklist.

### Work

1. ✅ **`subscribe(SubscribeConfig) -> SubscriptionHandle`:**
   - Continuous query model: text chunks flow in, relevant `(memory, confidence)` pairs stream out.
   - `SubscribeConfig` with validated bounds: `entity_id`, `kind_filter`, `confidence_threshold` (0.0–1.0), `max_results` (1–1000), `chunk_min_tokens` (1–512), `chunk_max_tokens` (1–4096), `chunk_max_wait_us`, `bloom_fpr` (0.001–0.5), `coarse_threshold` (0.0–1.0), `output_buffer_size` (1–10000).
   - `SubscriptionHandle` with `feed(text)`, `try_recv()`, `recv_timeout()`, `pause()`, `resume()`, `close()`, `stats()`, `reset_dedup()`.

2. ✅ **Hierarchical filtering pipeline (3-stage):**
   - **Stage 1 (Bloom Filter):** Keyword pre-screening — accumulated chunk keywords checked against scope bloom filter. Rejects chunks with zero keyword overlap in microseconds.
   - **Stage 2 (Coarse Centroid Match):** Single centroid vector (running mean of scope embeddings). Chunk embedding compared via inner product. Rejects if similarity < `coarse_threshold`. Sub-millisecond.
   - **Stage 3 (Fine HNSW Search):** Full approximate nearest-neighbor search via `IndexManager::search_vector()`. Returns top candidates ranked by `relevance = (1.0 - distance).max(0.0)`. Filters by `confidence_threshold`, entity scope, kind filter, time range, and per-session deduplication.

3. ✅ **Text chunk accumulation:**
   - Time-bounded tumbling window: accumulates tokens until `chunk_min_tokens` reached, then flushes on `chunk_max_tokens` or `chunk_max_wait_us` timeout (with jitter to stagger concurrent subscriptions).
   - Content length capped at 100KB safety limit.

4. ✅ **Subscription lifecycle management:**
   - `SubscriptionRegistry` enforces max concurrent subscriptions (bounded at 100).
   - Thread-per-subscription model: dedicated OS thread per subscription via `std::thread::Builder`, communicating through `crossbeam-channel`.
   - Signals: `TextChunk`, `Pause`, `Resume`, `Close`, `ResetDedup`.
   - `SubscriptionHandle::drop()` triggers deregistration; `Engine::drop()` calls `shutdown_all()` to join all worker threads.

5. ✅ **Per-session deduplication:**
   - `HashSet<[u8; 16]>` tracks pushed memory IDs within a subscription session.
   - `reset_dedup()` clears the set for re-evaluation of previously pushed memories.

6. ✅ **Backpressure:**
   - Bounded output buffer (`VecDeque<SubscribePush>`) with `parking_lot::Mutex` + `Condvar`.
   - Drop-oldest overflow policy: when buffer is full, oldest push is evicted (Principle 4: Bounded Everything).

7. ✅ **New-write notification:**
   - `Engine::remember()` calls `SubscriptionRegistry::notify_new_write(memory_id)` non-blocking (`try_send`) after successful commit (Principle 1: Hot Path Sanctity).
   - Worker receives notification, fetches memory, applies scope checks, compares against last context embedding, pushes if relevant and above threshold.
   - Dead notification channels cleaned lazily on broadcast.

8. ✅ **Concurrency model:**
   - `Arc<IndexManager>` shared between Engine and all subscription worker threads, leveraging `parking_lot::RwLock` for reader-writer separation.
   - No locks on the hot path beyond the RwLock read guard for HNSW search.
   - Periodic bloom filter and centroid rebuild (every 1000 chunks) to incorporate new writes.

9. ✅ **Engine integration:**
   - `Engine::subscribe(config)` public API. `Engine::active_subscriptions()` for monitoring.
   - `IndexManager` wrapped in `Arc` (changed from direct ownership) for safe concurrent access.
   - `SubscriptionRegistry` field added to `Engine` struct.

### Additional Deliverables

- ✅ `BloomFilter`: Double-hashing with configurable false positive rate, `insert()`, `contains()`, `contains_any()`.
- ✅ `TextAccumulator`: Token-counting, time-bounded flushing, content length safety cap.
- ✅ `OutputBuffer`: Thread-safe bounded queue with condition variable signaling and drop-oldest eviction.
- ✅ `CentroidState`: Incremental sum/count tracking with lazy L2 normalization for centroid computation.
- ✅ `SubscriptionStats`: Tracks `chunks_processed`, `memories_pushed`, `bloom_rejections`, `coarse_rejections`.
- ✅ `SubscribePush`: Contains `memory_id`, `content`, `confidence`, `kind`, `entity_id`, `created_at`.
- ✅ Helper functions: `extract_keywords` (lowercase, split on non-alphanumeric, stop-word filtering), `inner_product`, `l2_normalize`.
- ✅ `build_scope_data`: Efficiently constructs initial bloom filter and centroid from scoped memories with `MAX_SCOPE_SCAN` bound (10,000).

### Done When

- ✅ Subscribe stream receives relevant memories when matching input arrives, with hierarchical filtering pipeline rejecting non-matching input at the earliest possible stage.
- ✅ Bloom filter rejects keyword-disjoint chunks without embedding computation.
- ✅ Multiple concurrent subscriptions work without interference (integration test: 3 simultaneous subscriptions).
- ✅ New-write notification delivers freshly remembered memories to active subscriptions.
- ✅ Pause/resume lifecycle works correctly (integration test verified).
- ✅ Per-session deduplication prevents repeated pushes of the same memory.
- ✅ Backpressure drops oldest entries when output buffer is full (integration test verified).
- ✅ `SubscriptionHandle::drop()` and `Engine::drop()` cleanly shut down worker threads.
- ✅ Criterion benchmarks defined for pipeline match, bloom rejection, and notification fanout.

### Test Coverage (339 tests total across workspace)

- 128 unit tests in `hebbs-core` (engine with Phase 6 subscribe, subscribe module internals — 31 new Phase 6 tests including bloom filter, text accumulator, output buffer, centroid, registry, keyword extraction, config validation, inner product, L2 normalize).
- 46 integration tests in `hebbs-core` (including subscribe basic lifecycle, entity isolation, deduplication, close, pause/resume, multiple concurrent, new-write notification, stats tracking, backpressure — 9 new Phase 6 tests).
- 31 property tests in `hebbs-core` (including subscribe confidence above threshold, no duplicate memory IDs, output bounded, bloom no false negatives — 4 new Phase 6 tests).
- 53 unit tests in `hebbs-embed` (unchanged from Phase 2).
- 64 unit tests in `hebbs-index` (unchanged from Phase 3).
- 8 integration tests in `hebbs-index` (unchanged from Phase 3).
- 18 unit tests in `hebbs-storage` (unchanged from Phase 1).

### Dependencies Added

- `parking_lot = "0.12"` — `Mutex` and `Condvar` for output buffer thread-safe signaling (workspace dependency, already present for `IndexManager`).

### Known Issues

- `ort` v2.0.0-rc.11 remains a pre-release crate (carried from Phase 2). Will upgrade to stable when released.
- `paste` crate unmaintained advisory (RUSTSEC-2024-0436) — no security impact (carried from Phase 2).
- SIMD-optimized distance computation deferred to post-profiling optimization (carried from Phase 3).
- Thread-per-subscription model is adequate for Phase 6 scale (≤100 concurrent subscriptions). Thread pool optimization deferred to Phase 13 (Production Hardening) if profiling shows thread creation overhead is significant.
- Cluster centroids (Phase 7) will replace the single-centroid coarse filter with per-cluster centroids for improved filtering precision.
- gRPC transport for subscribe streams deferred to Phase 8 (gRPC Server). Phase 6 provides the in-process Rust API.
- Async/tokio integration deferred to Phase 8. Phase 6 uses `std::thread` + `crossbeam-channel` for isolation and predictability.

---

## Phase 7: Reflection Pipeline ✅ COMPLETE

**Goal:** The `reflect_policy()`, `reflect()`, and `insights()` operations work end-to-end.

**Status:** Complete. All deliverables met. See [phases/phase-07-reflection-pipeline.md](phases/phase-07-reflection-pipeline.md) for the full architecture blueprint.

**Implementation summary:**
- `hebbs-reflect` crate: stateless four-stage pipeline (cluster → propose → validate → consolidate), spherical K-means with silhouette-guided k selection, `LlmProvider` trait with Mock/Anthropic/OpenAI/Ollama providers, `PromptBuilder` for proposal and validation prompts, 24 unit tests.
- `hebbs-core` integration: `Engine::reflect()`, `Engine::insights()`, background reflect monitor with `start_reflect()`/`stop_reflect()`, insight invalidation on `revise()` and `forget()` via `InsightFrom` graph edges, cursor-based incremental processing.
- 14 integration tests, 5 property-based tests (proptest), 5 Criterion benchmarks.
- 393 total workspace tests, zero regressions.

### Work

1. Create the `hebbs-reflect` crate.
2. **Stage 1 -- Clustering (pure Rust):**
   - K-means or DBSCAN clustering over memory embeddings within a scope.
   - Use `linfa` or `ndarray` for statistical computation.
   - Output: clusters of related memories with frequency and pattern signals.
3. **Stage 2 -- Proposal (LLM call):**
   - Define the `LlmProvider` trait: `complete(prompt: &str) -> String`.
   - Implement providers: Anthropic, OpenAI, Ollama (local).
   - Feed cluster summaries to a lightweight model. Output: candidate insights as structured JSON.
4. **Stage 3 -- Validation (LLM call):**
   - Feed candidate insights + source memories to a stronger model.
   - Output: validated insights with confidence scores, contradiction flags.
5. **Stage 4 -- Consolidation:**
   - Store validated insights as a new memory type (`MemoryKind::Insight`).
   - Create lineage edges from insight to all source memories in the graph index.
   - Index the insight in all three indexes (it is recallable like any memory).
6. **`reflect_policy(config)`:**
   - Accept trigger configuration: threshold (N new memories), schedule (interval), recall failure (confidence below X), metric drift.
   - Store policy in RocksDB.
   - Background tokio task monitors triggers and runs reflect when conditions are met.
7. **`reflect(scope)`:**
   - Manual trigger. Runs the full pipeline synchronously (returns when done).
   - Scope: entity, time range, or "all new since last reflect".
8. **`insights(filter)`:**
   - Query insights by topic, entity, confidence threshold, recency.
   - Internally: recall over `MemoryKind::Insight` memories.

### Done When

- 100 test memories cluster into 3-5 coherent groups.
- LLM proposal generates structured insights from clusters.
- Insights are stored with lineage edges and are recallable via `recall()` and `insights()`.
- `reflect_policy` triggers reflect automatically when threshold is reached.
- Full pipeline runs end-to-end with at least one LLM provider (Ollama for local testing).

---

## Phase 8: Protobuf and gRPC Server ✅ COMPLETE

**Goal:** HEBBS is accessible over the network as a standalone server binary.

**Status:** Complete. All deliverables met. See [phases/phase-08-grpc-server.md](phases/phase-08-grpc-server.md) for the full architecture blueprint.

### Work

1. Create `hebbs-proto` crate:
   - Define `.proto` files for all 9 operations.
   - `MemoryService`: `Remember`, `Recall`, `Revise`, `Forget`, `Prime`.
   - `SubscribeService`: `Subscribe` (server-streaming RPC).
   - `ReflectService`: `SetPolicy`, `Reflect`, `GetInsights`.
   - Generate Rust code with `tonic-build`.
2. Create `hebbs-server` crate:
   - Standalone binary with CLI argument parsing (clap).
   - Load configuration from TOML file + environment variables + CLI flags.
   - Start gRPC server (tonic) on configurable port (default 6380).
   - Start HTTP/REST server (axum) on configurable port (default 6381) -- maps to the same service handlers.
   - Health check endpoint.
   - Graceful shutdown (SIGTERM handling).
3. Telemetry:
   - Structured logging (tracing crate).
   - Prometheus metrics endpoint: operation latencies (histogram), memory count (gauge), index sizes (gauge), reflect runs (counter).
4. Configuration:
   - `hebbs.toml` with sections: `[server]`, `[storage]`, `[embedding]`, `[reflect]`, `[decay]`, `[sync]`.
   - Environment variable overrides (`HEBBS_SERVER_PORT`, etc.).

### Done When

- ✅ `hebbs-server` starts from a single binary, accepts gRPC and HTTP connections.
- ✅ All 9 operations work over gRPC (25 integration tests pass).
- ✅ REST endpoints work for all operations: remember, get, recall, prime, revise, forget, insights (15 integration tests, verified with `curl`).
- ✅ Prometheus metrics are exported at `/v1/metrics` with operation histograms, memory count gauge, error counters.
- ✅ Configuration loads from TOML file, env vars, and CLI flags with correct precedence.
- ✅ 52 total tests pass (12 unit + 25 gRPC integration + 15 REST integration).
- ✅ Health endpoints: `/v1/health/live`, `/v1/health/ready`, gRPC `HealthService`.
- ✅ Graceful shutdown on SIGTERM/SIGINT.
- ✅ `hebbs-server version`, `config-check`, `config-dump` subcommands.
- ✅ Zero clippy warnings, zero compilation warnings.

### Deferred to later phases

- `SetPolicy` RPC → Phase 13 (reflect policy via TOML for now).
- REST Subscribe via SSE → Phase 13.
- Criterion benchmarks → Phase 12.
- Property-based tests → Phase 12.

---

## Phase 9: CLI Client

**Goal:** An interactive CLI client (`hebbs-cli`) for testing, debugging, and operating HEBBS

**Status:** Complete. See [phases/phase-09-cli-client.md](phases/phase-09-cli-client.md) for the full architecture blueprint.

### Rationale

The CLI is the foundational debugging tool for every subsequent phase. Before building SDKs in any language, developers need a fast, zero-ceremony way to exercise every operation, inspect engine state, and validate behavior from the terminal. Every operation is testable before a single line of SDK code exists.

### Work

1. **`hebbs-cli` crate (separate binary):**
   - Uses the tonic-generated gRPC client from `hebbs-proto` directly (no dependency on `hebbs-client` SDK or engine crates).
   - CLI argument parsing via `clap` with subcommands for all 9 operations.
   - Connection configuration: `--endpoint` (default `localhost:6380`), `--timeout`, `--http-port`.
   - Unified command dispatch: both one-shot and REPL modes parse into the same Command enum via clap and execute through the same handler.

2. **One-shot command mode:**
   - Every operation as a subcommand: `remember`, `recall`, `revise`, `forget`, `prime`, `subscribe`, `feed`, `reflect`, `insights`.
   - Output formats: human-readable table (default), `--json` for scripting/piping, `--raw` for raw protobuf debug.
   - Meaningful exit codes: 0 (success), 2 (usage error), 3 (connection error), 4 (not found), 5 (server error).
   - Pipe-friendly stdin integration for content ingestion.

3. **Interactive REPL mode:**
   - `hebbs-cli` with no subcommand enters a REPL 
   - Persistent connection, command history (via `rustyline`), tab completion for subcommands and flags.
   - Prompt displays connection state and endpoint.
   - Dot-commands for session management: `.help`, `.quit`, `.connect`, `.disconnect`, `.status`, `.clear`, `.history`, `.time`.
   - Subscribe mode takes over the terminal (like `redis-cli SUBSCRIBE`), `.stop` or Ctrl-C exits.

4. **Diagnostic commands (beyond the 9 API operations):**
   - `status` -- server health, version, uptime, memory count.
   - `get <id>` -- full detail view of a single memory with all metadata.
   - `inspect <id>` -- composite view: memory details + graph edges + nearest vector neighbors (client-side RPC composition).
   - `export` -- bounded JSONL dump for debugging (default limit 1000, max 10000).
   - `metrics` -- fetch and display Prometheus metrics from the HTTP port in human-readable form.

5. **Developer experience:**
   - Colorized output with importance-based highlighting, tty detection, `--color` override.
   - Elapsed time shown for every operation.
   - Verbose mode (`-v`, `-vv`) for gRPC tracing and request/response dumps.
   - Memory IDs displayed as 26-character ULID strings (canonical human-readable format).

### Done When

- `hebbs-cli remember` + `hebbs-cli recall` round-trip works against a running `hebbs-server`.
- All 9 operations exercisable from the CLI in one-shot mode.
- REPL mode starts, maintains connection, supports command history and tab completion.
- Diagnostic commands (`status`, `get`, `inspect`) return useful output.
- `--json` output is valid, parseable JSON for all commands.
- Integration tests: a shell script exercises the full lifecycle (remember → recall → revise → recall → forget → recall empty) via `hebbs-cli` commands.

---

## Phase 10: Rust Client SDK and FFI ✅ COMPLETE

**Goal:** A Rust client crate and a C-ABI FFI layer for embedding HEBBS in other languages.

**Status:** Complete. See [phases/phase-10-rust-client-ffi.md](phases/phase-10-rust-client-ffi.md) for the full architecture blueprint.

### Work

1. ✅ **`hebbs-client` crate:**
   - Async wrapper over gRPC client stubs from `hebbs-proto`.
   - Ergonomic domain types (`Ulid` IDs, Rust enums, `HashMap` context).
   - Builder pattern: `HebbsClient::builder().endpoint("localhost:6380").build().await`.
   - All 9 operations: remember, get, recall, prime, revise, forget, subscribe, reflect, insights.
   - Health check.
   - Exponential backoff retry policy for idempotent operations (get, recall, prime, insights, health).
   - Structured error taxonomy (`ClientError`) mapping gRPC status codes.
   - Subscribe stream via `SubscribeHandle` with async `Stream`.
   - `tracing` instrumentation on all operations.
2. ✅ **`hebbs-ffi` crate:**
   - C ABI functions: `hebbs_open`, `hebbs_remember`, `hebbs_recall`, `hebbs_revise`, `hebbs_forget`, `hebbs_prime`, `hebbs_reflect`, `hebbs_insights`, `hebbs_get`, `hebbs_close`, `hebbs_count`.
   - Embedded mode: links `hebbs-core` directly, no network.
   - Opaque handle pattern (`Arc<Engine>` behind `*mut c_void`) for thread-safe concurrent access.
   - JSON-based data exchange for structured inputs/outputs.
   - Thread-local error reporting via `hebbs_last_error()`.
   - Callback-based subscribe with polling fallback.
   - C header file (`include/hebbs.h`).
   - 66 unit tests + 10 integration tests + 25 property tests.

### Done When

- ✅ Rust client exercises all 9 operations + health via gRPC.
- ✅ FFI functions callable from C with C header file.
- ✅ Embedded mode (FFI, no server) passes full lifecycle integration tests.
- ✅ Concurrent FFI access from multiple threads verified.
- ✅ All workspace tests pass (679 tests, 0 failures).

---

## Phase 11: Python SDK ✅

**Goal:** `pip install hebbs` gives Python developers the full HEBBS API.

**Status: COMPLETE** — Embedded mode fully implemented with all 9 operations, 66 tests passing.

### What Was Built

Location: `crates/hebbs-python/` (maturin mixed Rust+Python layout within the monorepo).

1. **`hebbs-python` Rust crate (PyO3 native extension):**
   - `NativeEngine` wraps `hebbs-core::Engine` directly (no FFI indirection).
   - All 9 operations: remember, get, recall, revise, forget, prime, subscribe, reflect, insights.
   - GIL released during all Rust-side computation.
   - Full error mapping: `HebbsError` variants → Python exceptions.
   - `NativeSubscription` for real-time memory matching.

2. **`hebbs` Python package:**
   - `HEBBS.open("./path")` for embedded mode.
   - `Memory`, `RecallOutput`, `ForgetOutput` etc. as `@dataclass` types.
   - `MemoryKind`, `RecallStrategy`, `EdgeType`, `ContextMode` as `enum.Enum`.
   - Hierarchical exception hierarchy (`HebbsError` → 10 subclasses).
   - `SubscribeStream` with context manager and iterator protocol.
   - PEP 561 `py.typed` marker for type checker support.

3. **Framework integrations (optional extras):**
   - `hebbs[langchain]` — `HebbsMemory(BaseMemory)` adapter.
   - `hebbs[crewai]` — `HebbsShortTermMemory` and `HebbsLongTermMemory`.

4. **Packaging:**
   - `pyproject.toml` with maturin build backend.
   - `maturin develop --release` produces platform-specific wheel.
   - Pure-Python server mode stub ready for future activation.

### Deferred

- **Server mode (gRPC):** Stub in place. Requires `grpcio` and `hebbs-proto` bindings. Planned for Phase 18 SDK expansion.
- **Async API (`hebbs.aio`):** Stub in place. Activated with server mode.
- **PyPI publishing and CI wheels:** Planned for Phase 12 CI pipeline.

### Done When

- ✅ `pip install hebbs` works on macOS (arm64) — verified via `maturin develop`.
- ⏳ Server mode: all 9 operations work against a running `hebbs-server` — deferred.
- ✅ Embedded mode: `HEBBS.open(path)` works without a server — 66 tests passing.
- ✅ LangChain integration adapter implemented.
- ✅ CrewAI integration adapter implemented.

---

## Phase 12: Testing and Benchmark Suite — COMPLETE

**Goal:** Confidence that HEBBS is correct and fast.

### What Was Built

1. **System integration tests** (`crates/hebbs-core/tests/system_tests.rs`) — 19 tests across 6 categories:
   - **Category A (Full Lifecycle):** remember → get → recall (all strategies) → revise → reflect → insights → forget; lifecycle with active subscription
   - **Category B (Concurrency):** 20-thread mixed workload (remember + recall + revise + forget) for 3 seconds with zero corruption
   - **Category C (Crash Recovery):** Engine restart persistence — 100 memories survive close/reopen
   - **Category D (Scale Validation):** 1K and 10K recall validation across all strategies (`#[ignore]` for CI)
   - **Category E (Edge Cases):** Empty DB recall, nonexistent forget/revise, empty content rejection, 64KB content, top_k boundaries, nonexistent prime/reflect, multi-entity scoping
   - **Category F (Data Integrity):** Complex nested context roundtrip, merge/replace context modes, causal edges + graph recall, forget cascade verification

2. **`hebbs-bench` CLI** (`crates/hebbs-bench/`) — Standalone benchmark binary with 4 subcommands:
   - `latency` — p50/p95/p99/p999/min/max/mean for 8 hot-path operations (remember, get, recall_similarity, recall_temporal, prime, revise, forget_single, count)
   - `scalability` — Recall latency at 3-5 scale points (1K → 10K/100K/1M depending on tier)
   - `resources` — Disk and RSS measurement at scale (macOS `task_info` / Linux `/proc/self/statm`)
   - `all` — Runs all categories in sequence
   - Tiers: `quick` (~1 min), `standard` (~15 min), `full` (~2 hrs)
   - `--output <path.json>` produces machine-readable JSON report with system metadata
   - `--baseline <path.json>` compares against previous run, exits non-zero on >10% p99 regression
   - `--seed` for deterministic reproducibility
   - Committed baseline at `benches/baseline.json`

3. **CI pipeline** (`.github/workflows/`):
   - `ci.yml` — Every PR: fmt check, clippy (`-D warnings`), workspace tests, Python SDK tests (3.9/3.11/3.13 matrix), benchmark regression gate, cargo audit
   - `nightly.yml` — Daily: full tests including `#[ignore]`, standard-tier benchmarks, Python benchmarks
   - `release.yml` — On tag push: full benchmarks, cross-platform binary builds (linux-x86_64, linux-aarch64, macos-arm64), Python wheel builds via `maturin-action`, sdist, PyPI publishing

4. **Clippy cleanup** — Fixed all pre-existing clippy warnings across the workspace (needless_range_loop in hebbs-reflect, redundant closures in hebbs-core, unnecessary casts in hebbs-client/hebbs-bench, derived Default impls in hebbs-client, thread_local const init in hebbs-ffi)

### Benchmark Results (Quick Tier, Apple Silicon)

| Operation | p50 µs | p99 µs | Target p99 |
|---|---|---|---|
| remember | 200 | 350 | 5,000 (14x margin) |
| get | <1 | 1 | — |
| recall_similarity | 107 | 141 | 10,000 (70x margin) |
| recall_temporal | 286 | 334 | 5,000 (15x margin) |
| prime | 439 | 539 | — |
| revise | 209 | 346 | — |
| count | 1,631 | 1,766 | — |

Resource efficiency: ~1,022 bytes per memory on disk (target <2,048)

### Test Results

- **696 Rust tests passed, 0 failed** across 12 crates
- **66 Python SDK tests passed** (all strategies, types, concurrency, context roundtrip)
- **19 system integration tests passed** (17 in CI, 2 scale tests with `#[ignore]`)
- Zero clippy warnings with `-D warnings`
- `cargo fmt --check` passes

### Deferred to Phase 13

- Cognitive benchmark labeled dataset (requires curated ground-truth annotations; the benchmark harness is built, dataset is deferred)
- Golden serialization files (schema versioning; relevant when breaking changes occur)
- `hebbs-server` Criterion benchmarks (gRPC/REST round-trip)
- 72-hour soak test execution
- Actual PyPI publishing (workflow built, publishing gated on release decision)

---

## Phase 13: Hardening for Production

**Goal:** The system is safe to run with real data in production.

### Work

1. **Authentication and authorization:**
   - Token-based auth for gRPC and HTTP (API key in header).
   - Optional mTLS for gRPC.
2. **Multi-tenancy:**
   - Tenant ID scoping: all operations require a tenant context.
   - Separate RocksDB column family prefixes per tenant.
   - Memory and query isolation enforced at the storage layer.
3. **Rate limiting:**
   - Per-tenant rate limits on all operations (configurable).
   - Backpressure on `subscribe` streams.
4. **Data integrity:**
   - WAL-based crash recovery (RocksDB provides this, but verify with chaos tests).
   - Checksum verification on read.
5. **72-hour soak test:**
   - Continuous 100K ops/sec mixed workload.
   - Monitor latency drift, memory leaks, disk growth.
6. **Security audit checklist:**
   - Input validation on all API fields (max content length, valid UTF-8, context depth limits).
   - No SQL/command injection vectors (not applicable with RocksDB, but validate).
   - Dependency audit (`cargo audit`).

### Done When

- Auth-protected server rejects unauthenticated requests.
- Two tenants sharing the same server cannot access each other's memories.
- 72-hour soak test completes with stable latency (< 5% p99 drift).
- `cargo audit` shows no known vulnerabilities.

---

## Phase 14: Reference Application and LLM Integration ✅ COMPLETE

**Goal:** Build a real application on HEBBS to validate end-to-end use cases, exercise all LLM providers, and produce a reference implementation for prospective customers.

**Status:** Complete. See [phases/phase-14-reference-app-llm-integration.md](phases/phase-14-reference-app-llm-integration.md) for the full architecture blueprint and completed checklist.

### What Was Built

1. **`hebbs-demo` Python package (`hebbs-demo/`):**
   - CLI-based AI Sales Intelligence Agent using the `hebbs` Python SDK (embedded mode).
   - Interactive mode (human as prospect) and scripted mode (7 automated multi-turn scenarios).
   - Unified LLM client supporting OpenAI, Anthropic, and Ollama with configurable per-role providers.
   - Memory extraction via LLM: conversations → structured `remember()` calls with context metadata.
   - Rich-based observability display: collapsible panels showing HEBBS operations, latency, memory scores.
   - TOML-based configuration with 7 pre-built profiles (openai, anthropic, local, demo-quality, cost-optimized, embedding-specific).

2. **Seven scripted scenarios (75 assertions total, all passing):**
   - **A: Discovery Call** — 10-turn conversation, `remember()` + `recall(Similarity)`, 7 assertions.
   - **B: Objection Handling** — Two sessions with pricing objection, `recall(Analogical)` + `prime()`, 6 assertions.
   - **C: Multi-Session** — 5 sessions over simulated 2 weeks, `recall(Temporal)`, 11 assertions.
   - **D: Institutional Learning** — 50 call summaries → `reflect()` → new session with insights, 7 assertions.
   - **E: Real-Time Surfacing** — `subscribe()` active during conversation, 7 assertions.
   - **F: GDPR Forget** — 20 memories → `forget()` by entity → verify complete removal, 7 assertions.
   - **G: Multi-Entity Isolation** — 3 entities interleaved, 30 assertions (zero cross-contamination).

3. **LLM provider test harness:**
   - `hebbs-demo compare-llm`: runs scenarios across OpenAI, Anthropic, Ollama (mock mode for CI).
   - `hebbs-demo compare-embeddings`: runs with different embedding configs side by side.
   - Rich table output with p50/p99 latency, memory count, error count, estimated cost.

4. **API ergonomics audit:**
   - `ERGONOMICS_REPORT.md` with 10 categorized findings (2 high, 4 medium, 4 low severity).
   - Key findings: no custom embedder in embedded mode, no LLM provider config in embedded mode, missing batch and list APIs.

### Test Coverage

- **51 tests, 0 failures** (42 unit tests + 9 integration/scenario tests)
- Unit tests: config (7), display (8), LLM client (5), memory extraction (14), prompt templates (3), scenario registration (2)
- Scenario tests: all 7 scenarios pass with mock LLM (no API keys needed for CI)

### Done When

- ✅ `hebbs-demo` installs and runs with `pip install -e .`
- ✅ All 7 scenarios pass (75 assertions, 0 failures)
- ✅ All 9 HEBBS operations exercised across the scenario suite
- ✅ All 4 recall strategies exercised and validated
- ✅ `subscribe()` surfaces memories in real-time (Scenario E)
- ✅ `reflect()` runs end-to-end with 50 calls (Scenario D)
- ✅ Provider comparison reports produced via CLI commands
- ✅ MockEmbedder available as fallback for CI (OnnxEmbedder configurable)
- ✅ API ergonomics report written with 10 actionable findings

---

## Phase 15: Deployment and Operations ✅ COMPLETE

**Goal:** HEBBS is deployable to production cloud environments as a container image with Kubernetes-native infrastructure.

**Status:** Complete. See [phases/phase-15-deployment-and-operations.md](phases/phase-15-deployment-and-operations.md) for the full architecture blueprint.

**Distribution philosophy:** HEBBS follows the Redis model. The product ships as a server image with a `/metrics` endpoint. Monitoring integration (Prometheus scraping, Grafana dashboards, alerting) is documentation and tutorials delivered in Phase 16, not bundled product. Dashboard JSON and alerting rules are published as standalone importable files.

### Work

1. **Docker:**
   - Multi-stage Dockerfile (build in Rust image, run in distroless/cc-debian12).
   - Non-root execution (UID 65532), `STOPSIGNAL SIGTERM`, `HEALTHCHECK` instruction.
   - Multi-platform build (linux/amd64, linux/arm64) via `docker buildx`.
   - Target image size: under 80MB. ONNX model not baked in.
2. **Helm chart:**
   - StatefulSet with PersistentVolumeClaim per pod for RocksDB data.
   - Configurable StorageClass, resource limits, replica count.
   - ConfigMap for `hebbs.toml`, Secret for API keys and auth.
   - Startup, liveness, and readiness probes mapped to Phase 8 health endpoints.
   - ServiceMonitor for Prometheus Operator integration.
   - Service and Ingress definitions with annotation presets for NGINX and ALB.
   - Init container for ONNX model download.
3. **Terraform module:**
   - AWS: EKS + EBS CSI + ALB Controller + Helm release.
   - GCP: placeholder structure (stretch goal).
4. **Monitoring artifacts (standalone files, not a running stack):**
   - Grafana dashboard JSON (5 rows: health, latency, capacity, cognitive, security).
   - Prometheus alerting rules (8 rules with `runbook_url` annotations).
5. **Runbook:**
   - Backup and restore procedure (RocksDB checkpoint-based).
   - Scaling guide (vertical only -- horizontal requires Phase 17 sync).
   - Upgrade procedure for single and multi-replica deployments.
   - Incident response for common failure modes.
6. **CI/CD:**
   - Container image build and push to `ghcr.io` on `v*` tag push.
   - Container smoke test in CI (start → health → remember → recall → stop).
7. **Configuration:**
   - Production-ready annotated `hebbs.toml` example.

### Done When

- `docker run hebbs-ai/hebbs` starts a working server that responds to health checks.
- Helm chart deploys to a Kubernetes cluster and serves traffic.
- Grafana dashboard JSON is importable and covers all key metrics.
- At least one Terraform module (AWS) provisions the full stack.
- Runbook covers backup, restore, scaling, and incident response.
- Container image is built and pushed on release tags in CI.

---

## Phase 16: Documentation Site

**Goal:** Comprehensive documentation for users, contributors, and operators.

### Work

1. Set up `hebbs-docs` repo with a static site generator (Docusaurus or Astro).
2. **Getting started:** Install, connect, first `remember`/`recall` in < 5 minutes.
3. **Concepts:** Memory model, four recall strategies, reflection, decay, importance scoring.
4. **API reference:** Auto-generated from `.proto` files + hand-written guides per operation.
5. **SDK guides:** Python, TypeScript, Go -- each with idiomatic examples.
6. **Deployment guide:** Standalone, embedded, Docker, Kubernetes, edge mode.
7. **Architecture deep-dive:** For contributors -- crate structure, index internals, reflection pipeline.
8. **Benchmarks page:** Reproducible results with `hebbs-bench` instructions.
9. **Monitoring tutorials (from Phase 15 -- follows the Redis model):**
   - "Monitor HEBBS with Prometheus and Grafana" -- step-by-step tutorial with Docker Compose example showing how to set up Prometheus scraping of `/v1/metrics` and import the Phase 15 Grafana dashboard JSON.
   - Dashboard walkthrough -- documentation explaining each panel row, what to look for, what the numbers mean.
   - Alerting rules guide -- how to import the Phase 15 alerting rules into Prometheus, how to wire them to PagerDuty/Opsgenie, how to customize thresholds.
   - Docker Compose companion file (HEBBS + Prometheus + Grafana) published alongside the tutorial as a documentation artifact, not a product artifact.

### Done When

- Documentation site is live and covers all sections above.
- A new developer can go from zero to a working integration in < 15 minutes following the docs.
- Monitoring tutorial enables an operator to set up Prometheus + Grafana for HEBBS in < 15 minutes following the guide.

---

## Phase 17: Edge Mode and Sync

**Goal:** HEBBS runs autonomously on edge devices and syncs with a cloud hub.

### Work

1. **Edge configuration profile:**
   - 384-dim vectors by default (smaller index footprint).
   - Memory-mapped HNSW (index on NVMe, OS pages hot portions into RAM).
   - Aggressive decay defaults (`half_life = 14d`, `max_memories = 5_000_000`).
2. **Local reflection:**
   - `LlmProvider` implementation for local models via `llama.cpp` / MLX / Ollama.
   - Full reflect pipeline works offline.
3. **Sync protocol:**
   - Append-only push: edge pushes new memories to cloud hub (no conflict on memories -- they are events).
   - Pull insights: cloud-generated insights overwrite local insight cache.
   - Pull memories: memories from other devices/agents append to local store.
   - `revise` conflict: higher-importance evidence wins; ties broken by logical clock.
   - `forget` conflict: forget is authoritative (cloud wins).
   - Metadata per memory: `(device_id, logical_clock)` for ordering.
4. **Shared memory namespace (fleet mode):**
   - Local namespace (private) + shared namespace (synced).
   - Configurable sync interval.
   - Gossip protocol option for peer-to-peer sync on same network.
5. **Lineage-aware re-reflection:**
   - Every insight records its source memory IDs.
   - When new memories contradict a source, the insight is flagged for re-evaluation.
   - Incremental re-reflect on the affected source set only.

### Done When

- HEBBS starts in edge mode with 384-dim embeddings and memory-mapped index.
- Reflect works fully offline using a local LLM.
- Two HEBBS instances sync memories bidirectionally via a hub.
- Conflicting `revise` and `forget` operations resolve deterministically.
- An insight is correctly flagged and re-evaluated when contradicting evidence arrives via sync.

---

## Phase 18: TypeScript SDK

**Goal:** `npm install @hebbs/client` gives TypeScript developers the full API.

### Work

1. Set up `hebbs-node` repo.
2. gRPC client via `@grpc/grpc-js`, generated from `.proto` files.
3. REST fallback client for environments where gRPC is unavailable (edge runtimes, browsers).
4. Full TypeScript type definitions for all operations, memories, and insights.
5. Publish to npm.

### Done When

- All 9 operations work from TypeScript against a running server.
- Types are correct and complete.
- Published to npm, installable in a fresh project.

---

## Phase 19: Go SDK

**Goal:** `go get hebbs.dev/client` gives Go developers the full API.

### Work

1. Set up `hebbs-go` repo.
2. Generate Go gRPC client from `.proto` files using `protoc-gen-go-grpc`.
3. Idiomatic Go: context passing, functional options pattern, error wrapping.
4. Publish as a Go module.

### Done When

- All 9 operations work from Go against a running server.
- Module is importable via `go get`.

---

## Dependency Graph

```mermaid
graph TD
    P1[Phase 1: Storage Foundation] --> P2[Phase 2: Embedding Engine]
    P2 --> P3[Phase 3: Index Layer]
    P3 --> P4[Phase 4: Recall Engine]
    P3 --> P5[Phase 5: Revise / Forget / Decay]
    P4 --> P6[Phase 6: Subscribe]
    P4 --> P7[Phase 7: Reflection Pipeline]
    P5 --> P7
    P4 --> P8[Phase 8: gRPC Server]
    P5 --> P8
    P6 --> P8
    P7 --> P8
    P8 --> P9[Phase 9: CLI Client]
    P8 --> P10[Phase 10: Rust Client and FFI]
    P10 --> P11[Phase 11: Python SDK]
    P8 --> P12[Phase 12: Testing and Benchmarks]
    P8 --> P13[Phase 13: Production Hardening]
    P13 --> P14[Phase 14: Reference App and LLM Integration]
    P11 --> P14
    P13 --> P15[Phase 15: Deployment and Ops]
    P14 --> P16[Phase 16: Documentation Site]
    P15 --> P16
    P16 --> P17[Phase 17: Edge Mode and Sync]
    P10 --> P18[Phase 18: TypeScript SDK]
    P10 --> P19[Phase 19: Go SDK]
```

---

## What Each Phase Produces

| Phase | Deliverable | Usable As |
|-------|------------|-----------|
| 1 | RocksDB-backed memory store | Rust library (write/read only) |
| 2 | Auto-embedding on write | Rust library (write with vectors) |
| 3 | Three indexes operational | Rust library (indexed storage) |
| 4 | Four recall strategies + prime | Rust library (full read path) |
| 5 | Revise, forget, decay | Rust library (full write path) |
| 6 | Subscribe streaming | Rust library (real-time) |
| 7 | Reflect pipeline | Rust library (learning engine) |
| 8 | gRPC + HTTP server | Standalone server binary |
| 9 | CLI client (REPL + one-shot) | Testing, debugging, operations tool |
| 10 | Rust client + FFI | Client library + embeddable engine |
| 11 | Python SDK | PyPI package |
| 12 | Test suite + benchmarks | CI/CD + public benchmarks |
| 13 | Auth, multi-tenancy, hardening | Production-grade server |
| 14 | Reference app + LLM integration | Customer demo + validation tool |
| 15 | Docker, Helm, Terraform | Cloud deployment |
| 16 | Documentation site | Public docs |
| 17 | Edge mode + sync | Autonomous edge deployment |
| 18 | TypeScript SDK | npm package |
| 19 | Go SDK | Go module |
