# Phase 7: Reflection Pipeline -- Architecture Blueprint

## Status: ✅ COMPLETE

---

## Intent

Phases 1 through 6 built a system that remembers, indexes, recalls, revises, forgets, decays, and streams. Every one of those operations works on individual memories -- raw episodes that an agent recorded at a point in time. Phase 7 introduces the operation that transforms HEBBS from a memory store into a learning engine.

`reflect` is the boundary between storage and cognition. Without it, HEBBS is a superior vector database. With it, HEBBS is a system that gets smarter over time -- the compound interest that no amount of RAG tuning can replicate. A voice sales agent that has handled 500 calls does not need to search through 500 transcripts to know that "pricing objections in Q4 respond best to ROI framing." That knowledge should be distilled, stored with lineage to its source episodes, and surfaced instantly on recall. Reflect is the pipeline that performs this distillation.

The decisions made here are load-bearing for five downstream concerns:

- **Phase 6 (Subscribe) upgrade:** Phase 6 uses a single centroid per subscription scope for coarse filtering. Reflect produces per-cluster centroids that replace this approximation, improving subscribe precision without changing the pipeline interface.
- **Phase 8 (gRPC Server):** `reflect_policy()`, `reflect()`, and `insights()` map to gRPC RPCs. The core reflect engine must produce output that the server can expose without transformation.
- **Phase 13 (Production Hardening):** At 10,000 tenants, reflect scheduling must avoid thundering-herd behavior. The trigger system designed here must support hash-based staggering without structural change.
- **Phase 17 (Edge Sync):** Edge devices run reflect locally with on-device LLMs. The LlmProvider trait must accommodate local inference (Ollama, llama.cpp) with the same interface as cloud providers (Anthropic, OpenAI). When contradicting evidence arrives via sync, lineage-aware re-reflection targets only affected insights.
- **ScalabilityArchitecture.md:** The "hardest unsolved problem" -- causal consistency of reflected insights across sync boundaries -- requires that every insight record its source memory IDs as graph edges. Phase 7 establishes this lineage invariant. Phase 17 builds invalidation logic on top of it.

This is the most architecturally consequential phase in the roadmap. Every phase before it builds infrastructure. Every phase after it depends on the intelligence that reflect produces.

---

## Scope Boundaries

### What Phase 7 delivers

- `hebbs-reflect` crate: a stateless pipeline that accepts a set of memories and produces candidate insights with lineage metadata
- Stage 1 (Clustering): pure-Rust clustering over memory embeddings within a bounded scope, with automatic cluster count determination
- Stage 2 (Proposal): LLM call that generates candidate insights from each cluster, with structured JSON output
- Stage 3 (Validation): LLM call that evaluates candidate insights against source memories and existing insights, with contradiction detection
- Stage 4 (Consolidation): store validated insights as `MemoryKind::Insight` memories with full index integration and `InsightFrom` graph edges to all source memories
- `LlmProvider` trait: blocking, minimal, mockable interface for LLM completion with structured output support
- Three provider implementations: Anthropic (Claude), OpenAI (GPT), Ollama (local models)
- `MockLlmProvider`: deterministic, no-network provider for testing
- `reflect(scope)` operation: manual trigger, runs the full four-stage pipeline, returns when complete
- `reflect_policy(config)` operation: configures background trigger conditions (threshold, schedule, recall-failure)
- Background reflect monitor: dedicated worker thread that evaluates trigger conditions and executes reflect when conditions are met
- `insights(filter)` operation: query insights by entity, topic similarity, confidence threshold, recency, or lineage
- Cluster centroid publication: per-cluster centroids stored for Phase 6 subscribe pipeline upgrade
- Insight invalidation tracking: when a source memory is revised, dependent insights are flagged for re-evaluation
- Configuration types for all reflect parameters with validated bounds
- Full test coverage: unit, property-based, integration (with MockLlmProvider), and Criterion benchmarks for clustering

### What Phase 7 explicitly does NOT deliver

- Async/tokio integration (Phase 8 -- Phase 7 uses blocking HTTP for LLM calls, consistent with Phase 5/6 threading model)
- gRPC transport for reflect operations (Phase 8 -- but the core pipeline must be transport-agnostic)
- Metric-drift trigger type (Phase 8 -- requires an external metric API that does not exist yet)
- Cross-device insight invalidation (Phase 17 -- Phase 7 tracks lineage locally; Phase 17 extends invalidation across sync boundaries)
- Production LLM rate limiting, token budget accounting, or cost tracking (Phase 13 -- Phase 7 uses simple retry with backoff)
- Streaming LLM responses (Phase 8 -- the background pipeline does not need streaming; it waits for complete responses)
- TOML configuration loading (Phase 8 -- reflect parameters are passed programmatically)
- Materialized causal summaries from ScalabilityArchitecture.md (Phase 12 optimization -- Phase 7 stores insights; causal chain pre-computation is a tuning concern)

These exclusions are deliberate. Phase 7 builds the intelligence pipeline. Phase 8 wraps it in a transport. Phase 13 hardens it for production. Phase 17 extends it across devices.

---

## Architectural Decisions

### 1. Crate Boundary: hebbs-reflect as a Stateless Transform

The reflect pipeline needs access to memories, embeddings, and the graph index. Two dependency structures are possible.

| Model | Direction | Consequence |
|-------|-----------|-------------|
| A: hebbs-reflect depends on hebbs-core | Circular risk. hebbs-core already orchestrates everything. Adding reflect as a dependency of core, which itself depends on core, creates a cycle. |
| B: hebbs-reflect is independent; hebbs-core calls it | Clean separation. hebbs-reflect defines a pipeline: memories in, insights out. hebbs-core handles scoping (reading memories from storage), calling the pipeline, and consolidation (writing insights back). |

**Decision: Model B. hebbs-reflect is a stateless transform library.**

hebbs-reflect knows nothing about storage, indexes, or the Engine. It defines:

- `ReflectPipeline`: accepts a `ReflectInput` (a bounded set of memories with their embeddings, existing insights for dedup, and LLM provider configuration) and returns a `ReflectOutput` (validated insights with source lineage and cluster metadata).
- `LlmProvider` trait and implementations.
- Clustering logic.
- Prompt construction and response parsing.

hebbs-core orchestrates:

- Scoping: reading in-scope memories from storage based on entity, time range, or "new since last reflect."
- Calling `ReflectPipeline::run()` with the scoped memories.
- Consolidation: storing each validated insight as a Memory with `kind: Insight`, creating `InsightFrom` edges in the graph index, updating all three indexes atomically via WriteBatch.
- Publishing cluster centroids for the subscribe pipeline.

This separation means hebbs-reflect is testable in complete isolation -- no RocksDB, no HNSW, no storage trait. Feed it memories, get back insights. The consolidation logic in hebbs-core reuses the exact same WriteBatch + IndexManager path that `remember()` uses.

**Dependency direction:** `hebbs-core` depends on `hebbs-reflect`. `hebbs-reflect` depends on `hebbs-embed` (needs the `Embedder` trait to embed generated insight content). `hebbs-reflect` does NOT depend on `hebbs-core` or `hebbs-storage`.

### 2. The Four-Stage Pipeline: Stage-Gate Design

The pipeline processes memories through four sequential stages. Each stage is a gate: if the input does not meet the stage's quality bar, processing stops for that cluster.

| Stage | Input | Output | Can fail independently? | Cost |
|-------|-------|--------|------------------------|------|
| 1. Cluster | All in-scope memory embeddings | Groups of related memories + noise set | Yes (too few memories, no meaningful clusters) | CPU only, O(n * k * d) |
| 2. Propose | One cluster of memories | Candidate insights as structured JSON | Yes (LLM error, empty response, malformed JSON) | One LLM call per cluster |
| 3. Validate | Candidate insights + source memories + existing insights | Validated insights with verdicts and confidence | Yes (LLM error, all candidates rejected) | One LLM call per cluster |
| 4. Consolidate | Validated insights | Stored Insight memories with lineage edges | Yes (storage write failure) | Storage I/O |

**Why four stages, not two (cluster + LLM)?** Separating proposal from validation is a deliberate quality control mechanism:

- The proposer generates candidates optimistically. It may hallucinate, over-generalize, or miss contradictions with existing insights.
- The validator evaluates skeptically. It cross-references candidates against source memories, checks for contradictions with existing insights, and assigns calibrated confidence scores.
- Different models can serve different stages: a lightweight model (Phi-3, GPT-4o-mini, Claude Haiku) for proposal, a stronger model (Claude Sonnet, GPT-4o) for validation. This optimizes cost: proposal runs on every cluster, validation only on promising candidates.
- In testing, the mock provider returns different responses for propose vs validate prompts, enabling independent testing of each stage's logic.

**Why not a single monolithic LLM call?** A single prompt that says "here are 200 memories, find all insights" exceeds token limits, produces lower-quality output (the model tries to do too much), and is not testable in isolation. The stage-gate pattern bounds each LLM call's input and makes failures recoverable (a failed validation does not lose the clustering work).

**Partial failure semantics:** If Stage 2 fails for one cluster, other clusters continue. If Stage 3 rejects all candidates from a cluster, that cluster produces zero insights (not an error). If Stage 4 fails to write one insight, successfully written insights are not rolled back. The pipeline returns a `ReflectOutput` with per-cluster status (succeeded, failed with error, zero insights produced).

### 3. Clustering Algorithm Selection

The clustering stage must group related memories so the LLM can process coherent topical clusters. Key constraints:

- Memory set is bounded (Principle 4): max_memories_per_reflect is configurable, default 5,000, hard cap 10,000.
- Number of clusters is unknown in advance (an entity might have 2 topics or 50).
- Not every memory belongs to a cluster (some are truly unique observations).
- Embeddings are 384-dim, L2-normalized (unit vectors on the hypersphere).

| Algorithm | k required? | Noise handling | Time complexity | Memory | Quality for L2-normalized embeddings |
|-----------|------------|----------------|----------------|--------|--------------------------------------|
| K-Means (spherical) | Yes | No (forces all points into clusters) | O(n * k * d * iter) | O(n * d) | Good -- cosine distance is natural for unit vectors |
| K-Means + silhouette sweep | Auto | Partial (via min-cluster-size threshold) | O(n * k_max * k * d * iter) | O(n * d) | Good with proper k selection |
| DBSCAN | No | Yes (explicit noise category) | O(n²) naive, O(n log n) with spatial index | O(n²) naive | Good but epsilon parameter is hard to tune in high dimensions |
| Agglomerative (average-linkage) | No (cut at distance threshold) | Yes (isolated points) | O(n² log n) | O(n²) | Excellent -- produces hierarchical groupings |

**The O(n²) memory problem:** At n = 10,000 and 4 bytes per entry, the pairwise distance matrix consumes 400MB. On edge devices with 8GB RAM, this is 5% of total memory for a background operation. Acceptable for cloud, tight for edge.

**Decision: Spherical K-Means with silhouette-guided k selection and minimum cluster size threshold.**

Rationale:

- O(n * d) memory footprint -- at n = 10,000, d = 384: ~15MB. Negligible.
- Silhouette analysis over k in [2, k_max] where k_max = min(sqrt(n), 50) determines the optimal number of clusters automatically. For n = 5,000, this means evaluating ~70 values of k. Each evaluation is O(n * k * d * 10 iterations). Total: ~seconds on a single core. Acceptable for background.
- Clusters below `min_cluster_size` (default: 3) are discarded. Memories in discarded clusters are treated as noise and excluded from LLM processing. This approximates DBSCAN's noise handling without O(n²) memory.
- Cosine distance (equivalent to Euclidean on unit vectors) is the natural metric. Spherical K-Means normalizes centroids after each iteration to stay on the unit sphere.
- Deterministic given a fixed seed, enabling reproducible testing.

**Why not DBSCAN:** The epsilon parameter is notoriously difficult to tune in high-dimensional spaces. The "right" epsilon varies across entities and time scopes. K-Means with silhouette analysis is more robust to varying data characteristics. DBSCAN can be added as an alternative clustering backend in Phase 12 if clustering quality analysis shows K-Means is insufficient.

**Subsampling for silhouette analysis:** Computing the full silhouette score is O(n²). For k selection, we compute silhouette on a random subsample of min(n, 1000) points. This is O(1M) distance computations per k value -- milliseconds. The selected k is then used for full K-Means on all n points.

### 4. The LlmProvider Trait

Phase 7 introduces the first external network dependency in HEBBS. This is exclusively on the background path (Principle 1: no network calls on the hot path; Principle 5: background intelligence, foreground speed).

**Trait design principles:**

| Principle | Implication |
|-----------|-------------|
| Blocking, no tokio | Phase 7 follows the std::thread + crossbeam pattern from Phase 5/6. The reflect worker thread makes blocking HTTP calls. Phase 8 can bridge to async if needed. |
| Minimal surface | One method: `complete(request) -> Result<response>`. Not `chat`, not `stream`, not `embed`. One method. |
| Structured output | The request includes an optional response_format field (text or JSON). Providers that support native JSON mode use it; others rely on prompt-level instruction. |
| Mock-first | The `MockLlmProvider` is the primary testing provider. It returns deterministic, hardcoded responses based on prompt content patterns. All unit and integration tests run without API keys, network access, or running Ollama. |
| Configurable per stage | The reflect config specifies which provider + model to use for proposal vs validation independently. A lightweight model for proposal, a stronger model for validation. |

**Provider implementations:**

| Provider | Transport | JSON mode support | Phase 7 scope |
|----------|-----------|-------------------|---------------|
| Anthropic (Claude) | HTTPS REST API | System prompt instruction | Full implementation |
| OpenAI (GPT) | HTTPS REST API | `response_format: json_object` | Full implementation |
| Ollama | HTTP localhost | `format: "json"` | Full implementation (critical for edge/testing) |
| Mock | None | Returns hardcoded JSON | Full implementation (testing) |

**HTTP client choice:** A blocking HTTP client with no async runtime dependency. This keeps the "no tokio until Phase 8" invariant. The blocking client is sufficient because the reflect thread is dedicated and LLM calls are seconds-long I/O waits where thread blocking is expected behavior.

**Error handling:** LLM calls fail frequently (rate limits, timeouts, malformed responses, auth errors). The provider wraps all failures in a `LlmError` enum with structured context. The pipeline retries transient failures (rate limit, timeout) with configurable backoff (default: 3 retries, exponential backoff starting at 1 second). Permanent failures (auth, model not found) fail immediately. Malformed JSON responses trigger a single retry with a more explicit prompt; if the retry also fails, the cluster is marked as failed.

### 5. Prompt Architecture and Structured Output

The LLM stages require carefully structured prompts that produce parseable output. This is the most fragile part of the pipeline -- a prompt change can break all downstream parsing.

**Proposal prompt structure:**

The system message establishes the role and output format. The user message contains the cluster's memories serialized as structured text (not raw JSON dumps -- LLMs process natural text better than nested JSON). Each memory includes: content, importance, entity_id, created_at (human-readable), and its distance from the cluster centroid (indicating centrality).

The prompt instructs the model to:
1. Identify recurring patterns, consolidated knowledge, or actionable principles across the cluster.
2. Output each candidate insight with: content text, confidence (0.0-1.0), source_memory_ids (which cluster memories support it), and optional tags.
3. Produce zero candidates if no meaningful insight emerges (this is valid, not an error).

**Validation prompt structure:**

The system message establishes the evaluator role. The user message contains: candidate insights from Stage 2, the source memories for each candidate, and any existing insights for the same scope (to detect contradiction or duplication).

The prompt instructs the model to:
1. Evaluate each candidate for accuracy against source memories.
2. Check for contradiction with existing insights.
3. Assign a calibrated confidence score.
4. Return a verdict per candidate: accepted, rejected (with reason), merged_with_existing (with target insight ID), or revised (with corrected content).

**Token budget management:** Each cluster's prompt is bounded. The memory content in the prompt is truncated if total tokens would exceed a configurable limit (default: 4,000 tokens for proposal, 6,000 for validation). Truncation prioritizes memories closest to the cluster centroid (most representative). The token count is estimated using a conservative character-to-token ratio (4 chars ≈ 1 token for English text).

**Prompt versioning:** Prompts are not hardcoded strings. They are constructed by a `PromptBuilder` that accepts the structured input and produces the formatted prompt. This makes prompts testable (unit tests verify prompt structure for known inputs) and versionable (the builder can be swapped for future improvements without changing the pipeline).

### 6. Insight Data Model, Lineage, and the insights() Operation

**Insights are memories.** This is the most elegant consequence of Phase 1's data model design. The `Memory` struct already has `kind: MemoryKind` with an `Insight` variant reserved since Phase 1. An insight is stored exactly like an episode:

| Field | Value for Insight |
|-------|-------------------|
| `memory_id` | Fresh ULID (new identity) |
| `content` | The insight text (produced by the LLM) |
| `importance` | Derived: weighted mean of source memory importance × LLM confidence score |
| `embedding` | Generated by the embedder from the insight content (same 384-dim space as episodes) |
| `entity_id` | Inherited from the reflect scope (if entity-scoped) |
| `kind` | `MemoryKind::Insight` |
| `context` | Metadata: `reflect_run_id`, `cluster_id`, `source_count`, `llm_model`, `confidence` |
| `created_at` | Timestamp of the reflect run |
| `access_count` | 0 (starts fresh; reinforced via recall like any memory) |
| `decay_score` | Set to `importance` (same as any new memory) |

Because insights are memories, they are automatically:

- Indexed in all three indexes (temporal, vector, graph) via the standard `remember()` pipeline.
- Recallable via similarity, temporal, and causal strategies.
- Subject to decay (low-value insights fade, high-value ones persist).
- Reinforceable (frequently recalled insights strengthen).
- Revisable (a human or future reflect run can revise an insight).
- Forgettable (GDPR compliance applies to insights too).

**Lineage edges:** For each source memory that contributed to an insight, an `InsightFrom` edge (type 0x05, reserved in Phase 3) is created in the graph index:

| Edge | Direction | Purpose |
|------|-----------|---------|
| Forward: `insight_id → source_memory_id` | Insight to source | "This insight was derived from these memories" -- powers `insights()` lineage queries |
| Reverse: `source_memory_id → insight_id` | Source to insight | "These insights depend on this memory" -- powers invalidation when a source is revised |

Both directions use the existing bidirectional graph index from Phase 3. The edge metadata carries the reflect_run_id and the confidence score at the time of creation.

**The insights() operation** is a filtered recall with convenience semantics:

| Filter parameter | Implementation |
|------------------|---------------|
| Topic (text query) | Similarity recall with `kind = Insight` post-filter |
| Entity | Temporal recall with entity_id scope and `kind = Insight` post-filter |
| Confidence threshold | Post-filter on the confidence value stored in insight context |
| Recency | Temporal recall with time range and `kind = Insight` post-filter |
| Lineage (insights from specific memories) | Graph traversal: reverse `InsightFrom` edges from the specified memory IDs |

This is not a separate query engine -- it composes existing recall strategies with kind filtering. The `insights()` method on Engine is syntactic sugar over `recall()` with appropriate defaults.

### 7. reflect_policy() and Background Trigger Monitor

**Policy model:** A reflect policy defines WHEN to run reflect and WHAT scope to process. Multiple policies can coexist (e.g., per-entity policies with different thresholds, plus a global catch-all).

| Trigger type | Condition | Phase 7 scope |
|-------------|-----------|---------------|
| Threshold | N new memories since last reflect for this scope | Yes |
| Schedule | Wall-clock interval since last reflect for this scope | Yes |
| Recall failure | Recall returned results below confidence threshold | Yes (bounded signal channel from recall path) |
| Metric drift | External metric changed by more than delta | No (requires Phase 8 external API) |

**Policy storage:** Serialized in the `meta` column family with key format `reflect_policy:{policy_id}`. Policies include: scope definition (entity_id or global), trigger conditions, stage configuration (which LLM provider/model per stage), and scheduling metadata (last_reflect_timestamp, memories_since_last_reflect counter).

**Background monitor:** A dedicated worker thread (same pattern as Phase 5 decay worker):

- Runs a periodic check loop (configurable interval, default 60 seconds).
- On each tick: evaluates all active policies. For each policy, checks if any trigger condition is met.
- If a trigger fires: executes the reflect pipeline for that policy's scope. The execution is synchronous on the worker thread (reflect is background, latency is not a concern).
- Control signals via crossbeam-channel: Pause, Resume, Shutdown, Reconfigure (same pattern as decay worker).
- The worker thread is started via `Engine::start_reflect()` and stopped via `Engine::stop_reflect()`, mirroring the decay worker lifecycle.

**Recall-failure trigger:** When `recall()` returns results where the best match has confidence below a configurable threshold (default: 0.3), it sends a lightweight signal (entity_id + timestamp, ~40 bytes) to a bounded channel monitored by the reflect worker. The channel is bounded (capacity 100) and uses try_send (non-blocking, Principle 1). If the channel is full, the signal is dropped -- the next recall failure will re-signal. The reflect worker aggregates these signals: if an entity accumulates N failure signals (configurable, default 5) within a time window, reflect is triggered for that entity scope.

### 8. Scope and Incremental Processing

**The scope problem:** Reflect must process a bounded, relevant subset of memories. Processing all memories from the beginning of time is wasteful (Principle 4) and produces lower-quality clusters (mixing decades of context dilutes signal).

**Scope definition:**

| Scope type | Selection criteria | Typical use |
|-----------|-------------------|-------------|
| Entity-scoped, incremental | All memories for entity_id created since last reflect for this entity | Per-prospect reflection after each batch of calls |
| Entity-scoped, full | All memories for entity_id within a time window | Re-reflect on a specific prospect (manual) |
| Global, incremental | All memories created since last global reflect | Periodic cross-entity pattern discovery |
| Global, full | All memories within a time window | Manual full re-reflection (rare, expensive) |

**Incremental is the default.** Each reflect run records a cursor (timestamp of the newest memory processed) in the `meta` column family. The next incremental run starts from that cursor. This bounds per-run cost regardless of total memory count (Principle 4, ScalabilityArchitecture.md).

**Minimum memory threshold:** Clustering requires a minimum number of data points to be meaningful. If the scope contains fewer than `min_memories_for_reflect` (default: 10), the reflect run is skipped (not an error). This prevents LLM calls on trivially small inputs.

**Maximum memory bound:** If the scope contains more than `max_memories_per_reflect` (default: 5,000), the most recent N memories are selected. Older memories within scope are deferred to a subsequent reflect run. The cursor advances only to the oldest processed memory, ensuring deferred memories are picked up next time.

**Existing insight dedup:** Before the proposal stage, existing insights for the same scope are loaded and passed to the LLM. This prevents the pipeline from re-deriving insights that already exist. The validator also checks for duplication and can return a `merged_with_existing` verdict.

### 9. Integration with Phase 6 Subscribe Pipeline

Phase 6 established two extension points that Phase 7 fulfills:

**Extension point 1: Per-cluster centroids for coarse filtering.**

Phase 6's coarse stage compares the input chunk's embedding against a single scope centroid (mean of all in-scope memory embeddings). This is a gross approximation that only catches extreme outliers. Phase 7's clustering produces semantically meaningful cluster centroids.

After each reflect run, the cluster centroids (k vectors of dimension 384) are stored in a lightweight structure accessible to subscription workers. When a subscription's periodic refresh fires (bloom + centroid rebuild), it checks for available cluster centroids for its scope. If available, the coarse stage compares against ALL k centroids instead of one, and passes if the input is similar to ANY centroid. This provides topic-level filtering: input about "pricing" matches the pricing cluster centroid, input about "weather" matches none.

The Phase 6 pipeline architecture already supports this without structural change -- the coarse stage was designed to compare against 1 or K centroids through the same interface.

**Extension point 2: Insight-derived keywords for bloom filter.**

Phase 6's bloom filter contains keywords extracted from raw memory content. After reflect produces insights, the insight keywords (which are distilled, higher-signal terms) can be added to the bloom filter. This improves bloom filter precision: insight keywords like "ROI-based pricing objection" are more discriminating than raw episode keywords like "mentioned" or "pricing."

This is an optional enhancement. The bloom filter rebuild path already supports arbitrary keyword sources. Phase 7 adds insight keywords to the keyword extraction during bloom rebuild if insights exist for the scope.

### 10. Insight Invalidation and Re-Reflection

**The lineage invariant:** Every insight records its source memory IDs as `InsightFrom` graph edges. This invariant, established in Phase 7, is the foundation for the "hardest unsolved problem" in ScalabilityArchitecture.md.

**When does invalidation trigger?**

| Event | Detection mechanism | Action |
|-------|-------------------|--------|
| Source memory revised (`revise()`) | After revise completes, reverse graph lookup on the revised memory_id finds all `InsightFrom` edges pointing to it | Mark dependent insights as "stale" in their context metadata |
| Source memory forgotten (`forget()`) | After forget completes, same reverse graph lookup | Mark dependent insights as "stale" (the insight may still be valid from remaining sources, or may need re-evaluation) |
| New memories contradict an insight | Detected during the next reflect run's validation stage (the validator compares new candidates against existing insights) | Existing insight is either confirmed, revised, or invalidated |

**Stale insight handling:** A "stale" flag in the insight's context metadata does not remove the insight from indexes or recall results. Stale insights are still recallable but carry a signal that their confidence may have degraded. The next reflect run for the affected scope re-evaluates stale insights by including them in the validation prompt alongside new candidates. The validator either confirms them (removes stale flag), revises them (updates content, resets flag), or invalidates them (the insight is marked for forget or its confidence is set to zero, causing decay to handle it).

**Why not immediate re-reflection on every revise?** Re-reflection is expensive (LLM calls). A single revised memory affecting 3 insights does not justify 3 LLM calls immediately. Batching is more efficient: the stale flag accumulates, and the next scheduled reflect run processes all stale insights together. For urgent invalidation (e.g., a critical fact changed), the caller can invoke `reflect()` manually.

### 11. Concurrency and Resource Isolation

Reflect is on the background side of the Principle 5 wall. It must never compete with the hot path.

| Resource | Hot path usage | Reflect usage | Contention mitigation |
|----------|---------------|---------------|----------------------|
| Embedder (ONNX session) | `remember()` embeds content, `subscribe()` embeds chunks | Stage 4 embeds insight content | Insight embedding happens during consolidation, one embed call per insight. At ~5 insights per reflect run, this is ~15ms total. The embedder mutex serializes this with hot path calls, adding < 5ms worst-case wait. |
| HNSW index (RwLock) | `recall()` and `subscribe()` read | Stage 4 consolidation writes (via `remember()` path) | Write lock is held for microseconds during HNSW commit. Same contention as `remember()` -- no new concern. |
| Storage (RocksDB) | All operations | Stage 4 writes insights + edges via WriteBatch | RocksDB serializes writes internally. The WriteBatch for 5 insights + lineage edges is a single atomic write, same as `remember()`. |
| CPU | HNSW search, bloom filter checks | Clustering (CPU-intensive) | Clustering runs on the dedicated reflect thread. It does not share a thread pool with hot path operations. At n = 5,000, d = 384, clustering completes in 1-3 seconds -- a burst, not sustained contention. |
| Network | None (Principle 1) | LLM API calls (seconds per call) | The reflect thread blocks on network I/O. No hot path thread ever blocks on network. Complete isolation. |

**Thread model:** One dedicated reflect worker thread, same pattern as Phase 5 decay worker. The thread sleeps between reflect runs (checking triggers on each wake). During a reflect run, it is CPU-active during clustering and I/O-blocked during LLM calls. At no point does it hold locks that hot path threads need.

### 12. Configuration Surface

| Parameter | Type | Default | Bounds | Rationale |
|-----------|------|---------|--------|-----------|
| `max_memories_per_reflect` | usize | 5,000 | [10, 10,000] | Bounds clustering cost and LLM prompt size. Edge devices use lower values. |
| `min_memories_for_reflect` | usize | 10 | [3, 1,000] | Below this count, clustering is not meaningful. Skip the run. |
| `min_cluster_size` | usize | 3 | [2, 100] | Clusters smaller than this are discarded as noise. |
| `max_clusters` | usize | 50 | [2, 200] | Upper bound on k for silhouette sweep. Caps the number of LLM calls per reflect run. |
| `clustering_seed` | u64 | 42 | any u64 | Fixed seed for deterministic clustering in tests. |
| `proposal_provider` | LlmProviderConfig | Ollama default | -- | Which LLM provider and model for the proposal stage. |
| `validation_provider` | LlmProviderConfig | Ollama default | -- | Which LLM provider and model for the validation stage. Can be a stronger model. |
| `proposal_max_tokens` | usize | 4,000 | [500, 32,000] | Token budget for the proposal prompt (truncates memory content if exceeded). |
| `validation_max_tokens` | usize | 6,000 | [500, 32,000] | Token budget for the validation prompt. |
| `llm_timeout_secs` | u64 | 60 | [5, 600] | Per-call timeout for LLM requests. |
| `llm_max_retries` | usize | 3 | [0, 10] | Retry count for transient LLM failures. |
| `llm_retry_backoff_ms` | u64 | 1,000 | [100, 60,000] | Initial backoff for retry (exponential). |
| `trigger_check_interval_us` | u64 | 60,000,000 (60s) | [1,000,000, 3,600,000,000] | How often the background monitor checks trigger conditions. |
| `threshold_trigger_count` | usize | 50 | [5, 10,000] | Number of new memories that triggers an automatic reflect run. |
| `schedule_trigger_interval_us` | u64 | 86,400,000,000 (24h) | [3,600,000,000, 604,800,000,000] | Time interval between scheduled reflect runs. |
| `recall_failure_threshold` | f32 | 0.30 | [0.0, 1.0] | Best-match confidence below this signals a recall failure to the reflect monitor. |
| `recall_failure_count` | usize | 5 | [1, 100] | Number of recall failure signals before triggering reflect for the affected entity. |
| `insight_importance_weight` | f32 | 0.7 | [0.0, 1.0] | Weight of source memory importance in computing insight importance. Remainder is LLM confidence weight. |
| `stale_insight_revalidation` | bool | true | -- | Whether stale insights are re-evaluated on the next reflect run. |
| `publish_cluster_centroids` | bool | true | -- | Whether to store cluster centroids for subscribe pipeline consumption. |
| `enabled` | bool | true | -- | Master switch for the background reflect monitor. |

### 13. Testing Strategy

**Layer 1: Unit tests (in hebbs-reflect, no storage dependency)**

Clustering:
- Verify that known-structure embeddings (3 well-separated clusters) produce 3 clusters.
- Verify silhouette-guided k selection picks the correct k for synthetic data with known structure.
- Verify min_cluster_size filtering: clusters below threshold are discarded.
- Verify determinism: same input + same seed = same clusters.
- Verify empty input and below-minimum input are handled gracefully (no panic, no LLM call).

Prompt construction:
- Verify proposal prompt includes all memory content, importance, and centroid distance.
- Verify validation prompt includes candidates, source memories, and existing insights.
- Verify token budget truncation: memories beyond the token limit are excluded, starting with the least central.
- Verify prompt builder output is stable (no random ordering that would break snapshot tests).

LLM response parsing:
- Verify valid JSON responses parse correctly into candidate insights.
- Verify malformed JSON triggers a retry.
- Verify empty candidate lists are handled (no insights from this cluster, not an error).
- Verify all verdict types parse correctly: accepted, rejected, merged_with_existing, revised.

Pipeline integration (with MockLlmProvider):
- Feed 50 memories with 3 natural clusters. Verify pipeline produces 1-3 insights per cluster.
- Verify each insight's source_memory_ids are a subset of the cluster's memory IDs.
- Verify insights with rejected verdicts are not in the output.
- Verify partial failure: one cluster's LLM call fails, other clusters still produce insights.

**Layer 2: Property-based tests**

- For any set of L2-normalized embeddings and any valid config, clustering produces clusters where every cluster has >= min_cluster_size members.
- For any ReflectOutput, every insight's source_memory_ids list is non-empty (no orphan insights).
- For any ReflectOutput, every source_memory_id in an insight's lineage exists in the original input memory set (no hallucinated IDs).
- The number of clusters never exceeds max_clusters.
- Token budget is never exceeded in constructed prompts.

**Layer 3: Integration tests (RocksDB backend, MockLlmProvider)**

- Full lifecycle: remember 100 memories for entity A, run reflect, verify insights are stored with `kind: Insight`, verify `InsightFrom` edges exist in graph index, verify insights are recallable via `recall()` and `insights()`.
- Incremental reflect: remember 50 memories, reflect, remember 50 more, reflect again. Second run processes only the new 50. No duplicate insights.
- Insight invalidation: reflect produces insight X from memories [A, B, C]. Revise memory B. Verify insight X is flagged stale. Run reflect again. Verify insight X is re-evaluated.
- reflect_policy threshold trigger: set threshold to 10, remember 9 memories (no reflect triggered), remember 1 more (reflect triggered). Verify insight production.
- insights() query: reflect, then query insights by entity, by topic similarity, by confidence threshold. Verify correct filtering.
- Concurrent reflect + remember: one thread runs reflect while another thread writes 100 new memories. Verify no panics, no corruption.
- Reflect with zero in-scope memories: verify graceful no-op (no LLM calls, no errors).
- Reflect with all memories below min_memories_for_reflect: verify skip.

**Layer 4: Criterion benchmarks**

- Clustering latency: 1K, 5K, 10K memories at 384 dimensions. Target: < 5 seconds at 10K.
- Silhouette k-selection latency: subsample of 1K, k range [2, 50]. Target: < 1 second.
- Full pipeline latency (with MockLlmProvider, zero network): 1K memories, 5 clusters. Target: < 2 seconds (dominated by clustering + embedding of insights).
- Consolidation write latency: 10 insights with lineage edges. Target: < 50ms (same order as 10 `remember()` calls).

---

## Risk Register

| Risk | Impact | Probability | Mitigation |
|------|--------|------------|------------|
| LLM produces malformed JSON despite structured output instruction | Medium -- pipeline fails for affected cluster | Medium (model-dependent) | Retry with explicit format reinforcement in prompt. Parse with lenient JSON parser that handles common LLM formatting errors (trailing commas, unquoted keys). Fall back to regex extraction of key fields. |
| LLM hallucinates source_memory_ids that do not exist in the cluster | High -- broken lineage edges, phantom graph relationships | Medium | Validate all source_memory_ids against the actual cluster membership before consolidation. Any ID not in the cluster is silently dropped from the lineage. Log a warning for monitoring. |
| Clustering produces degenerate results (one giant cluster or all singletons) | Medium -- LLM receives too many memories (giant cluster) or too few (singletons discarded) | Low (silhouette analysis prevents this for well-separated data) | Cap maximum cluster size (default: 500 memories). If a cluster exceeds this, split it by re-running K-Means with k=2 on the oversized cluster. Singletons below min_cluster_size are expected noise. |
| LLM rate limits cause cascading reflect failures across tenants | High in cloud -- all tenants' reflect runs fail simultaneously | Medium at scale | Per-tenant LLM call budget. Exponential backoff with jitter. Phase 13 adds hash-based staggering of reflect schedules across tenants. Phase 7 implements retry + backoff per call. |
| Insight importance formula produces scores that are systematically too high or too low | Medium -- insights dominate or disappear from recall results | Medium | The formula is configurable (insight_importance_weight). Monitor the distribution of insight importance vs episode importance in integration tests. Adjust the weight based on empirical observation. |
| Re-reflection on stale insights enters a loop (insight X invalidates Y, Y invalidates X) | Medium -- infinite reflect cycles consuming LLM budget | Low (requires circular contradiction, rare in practice) | Bound re-reflection: an insight can be re-evaluated at most N times (default: 3) per reflect run. After that, it is accepted with a "low-confidence" flag. The `reflect_run_id` in context metadata enables cycle detection. |
| Embedder contention during consolidation delays hot path | Low -- insight embedding is a single call per insight | Low (consolidation produces ~5 insights per run) | Same mitigation as Phase 6: embedding is behind a Mutex, but the reflect thread contributes < 15ms of contention per run. Negligible at 1 reflect run per hour. |
| Recall-failure signal channel floods the reflect monitor under sustained low-quality recall | Low -- reflect runs too frequently, consuming LLM budget | Medium | The signal channel is bounded (capacity 100, try_send). The reflect monitor aggregates signals within a time window and requires N failures (default: 5) before triggering. After a reflect run, the failure counter resets. A cooldown period (configurable, default: 10 minutes) prevents re-triggering immediately. |

---

## Deliverables Checklist

Phase 7 is done when ALL of the following are true:

- [ ] `hebbs-reflect` crate exists with `ReflectPipeline::run(input) -> Result<ReflectOutput>` stateless API
- [ ] Stage 1: Spherical K-Means clustering with silhouette-guided k selection. Produces coherent clusters for known-structure test data.
- [ ] Stage 2: Proposal stage constructs prompt from cluster memories, calls LlmProvider, parses structured JSON response into candidate insights
- [ ] Stage 3: Validation stage evaluates candidates against source memories and existing insights, produces verdicts (accepted/rejected/merged/revised)
- [ ] Stage 4: Consolidation stores validated insights as `MemoryKind::Insight` memories via the standard `remember()` pipeline path
- [ ] `InsightFrom` graph edges created for every (insight, source_memory) pair, bidirectional (forward + reverse)
- [ ] `LlmProvider` trait defined with `complete(request) -> Result<response>` and structured output support
- [ ] Anthropic provider implementation (Claude API, blocking HTTP)
- [ ] OpenAI provider implementation (GPT API, blocking HTTP)
- [ ] Ollama provider implementation (local HTTP, blocking)
- [ ] `MockLlmProvider` implementation: deterministic, no-network, returns valid structured JSON based on input patterns
- [ ] `Engine::reflect(scope)` executes the full four-stage pipeline synchronously and returns `ReflectOutput`
- [ ] `Engine::insights(filter)` returns insights filtered by entity, topic, confidence, recency, or lineage
- [ ] `Engine::reflect_policy(config)` stores a trigger policy in meta CF
- [ ] Background reflect monitor: dedicated worker thread, periodic trigger evaluation, control signals (Pause/Resume/Shutdown/Reconfigure)
- [ ] Threshold trigger: fires after N new memories since last reflect for the scope
- [ ] Schedule trigger: fires after configurable wall-clock interval since last reflect
- [ ] Recall-failure trigger: bounded signal channel from recall path, aggregation with configurable threshold
- [ ] Incremental processing: cursor-based, only processes memories newer than last reflect cursor
- [ ] Minimum memory threshold: reflect runs with fewer than `min_memories_for_reflect` memories are skipped
- [ ] Maximum memory bound: scopes exceeding `max_memories_per_reflect` are truncated to most recent
- [ ] Existing insight dedup: existing insights for the scope are passed to the validation stage
- [ ] Insight invalidation: `revise()` and `forget()` flag dependent insights as stale via reverse graph lookup
- [ ] Stale insights are re-evaluated on the next reflect run for the affected scope
- [ ] Cluster centroids published for Phase 6 subscribe pipeline (stored in a structure accessible to subscription workers)
- [ ] All source_memory_ids in insight lineage are validated against cluster membership before consolidation
- [ ] `ReflectConfig` has documented defaults and validated bounds for all parameters
- [ ] Partial failure: LLM failure for one cluster does not prevent other clusters from producing insights
- [ ] Token budget enforcement: prompts do not exceed configured token limits
- [ ] LLM retry with configurable backoff for transient failures (rate limit, timeout)
- [ ] No `unwrap()` or `expect()` on any path reachable by external input
- [ ] No `unsafe` blocks
- [ ] All unit tests pass (hebbs-reflect crate, MockLlmProvider)
- [ ] All property-based tests pass
- [ ] All integration tests pass (RocksDB backend, MockLlmProvider)
- [ ] Criterion benchmarks established: clustering < 5s at 10K, full pipeline (mock LLM) < 2s at 1K, consolidation < 50ms for 10 insights
- [ ] `cargo clippy` passes with zero warnings
- [ ] `cargo fmt --check` passes
- [ ] `cargo audit` passes
- [ ] PhasePlan.md updated with Phase 7 completion marker and known issues
- [ ] DocsSummary.md updated with Phase 7 entry

---

## Interfaces Published to Future Phases

Phase 7 creates contracts that later phases depend on. These interfaces are stable after Phase 7 and should not change without a documented migration plan.

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| `LlmProvider` trait (`complete(request) -> Result<response>`) | Phase 8 (server bridges provider config to TOML), Phase 17 (edge uses Ollama provider for local reflect) | Additive only. New optional request fields allowed. Existing fields never change semantics. |
| `ReflectPipeline::run()` input/output types | Phase 8 (gRPC handler maps proto to ReflectInput), Phase 17 (sync triggers re-reflect with merged memories) | Additive only. New optional fields in ReflectInput and ReflectOutput allowed. |
| `Engine::reflect(scope)` and `Engine::insights(filter)` public API | Phase 8 (gRPC RPC handlers), Phase 9 (CLI commands), Phase 10 (Rust SDK), Phase 11 (Python SDK) | Additive only. New optional parameters allowed. Existing parameters never change behavior. |
| `Engine::reflect_policy(config)` public API | Phase 8 (gRPC RPC handler, TOML config mapping), Phase 13 (hash-based staggering extension) | Additive only. New trigger types allowed. Existing trigger types never change semantics. |
| `MemoryKind::Insight` storage and indexing conventions | All phases that query memories (4, 6, 8, 10, 11, 15, 16, 17) | Immutable. Insights are memories. Any code that handles memories handles insights. |
| `InsightFrom` graph edge type (0x05) and bidirectional index | Phase 17 (cross-device invalidation traverses InsightFrom edges), Phase 9 (CLI inspect shows lineage) | Immutable. Edge type and direction are fixed. |
| Cluster centroid storage format | Phase 6 (subscribe coarse stage reads centroids), Phase 12 (benchmark suite measures centroid quality) | Additive. The storage structure can gain metadata. The centroid vectors and cluster IDs are immutable per reflect run. |
| Insight stale flag in context metadata | Phase 17 (sync-triggered invalidation sets the same flag), Phase 9 (CLI shows stale status) | The flag key and semantics are stable. |
| Recall-failure signal channel interface | Phase 8 (gRPC recall handler may extend the signal with request metadata) | Additive. The signal payload can gain fields. Existing fields (entity_id, timestamp) are immutable. |
| Background reflect worker lifecycle (`start_reflect`, `stop_reflect`, `pause_reflect`, `resume_reflect`) | Phase 8 (server startup/shutdown orchestrates worker lifecycle), Phase 13 (production hardening tunes scheduling) | Additive only. New lifecycle methods allowed. Existing methods never change behavior. |
