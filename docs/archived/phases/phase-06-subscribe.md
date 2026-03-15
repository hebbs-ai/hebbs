# Phase 6: Subscribe (Associative Real-Time Recall) -- Architecture Blueprint

## Status: ✅ COMPLETE

All deliverables met. 339 tests passing (128 unit in hebbs-core, 46 integration, 31 property-based, 53 embed, 64 index unit, 8 index integration, 18 storage). Criterion benchmarks established for pipeline match, bloom rejection, and notification fanout. Zero clippy warnings, zero fmt issues, zero `unsafe`, zero `unwrap()` on external paths.

---

## Intent

Phases 1 through 5 built a system that remembers, indexes, recalls, revises, forgets, and decays. Every one of those operations is request-response: the caller asks, the engine answers, the interaction ends. Phase 6 introduces the first long-lived, streaming operation -- and in doing so, it changes the fundamental relationship between HEBBS and its consumers.

`subscribe()` is not a polling loop over `recall()`. It is a continuous, bidirectional intelligence pipeline: the caller feeds a live stream of text (a voice transcript, a document being read, a sensor log being parsed), and the engine pushes relevant memories back as they become contextually appropriate. The latency budget is 8ms from input token to pushed memory (Principle 1). The caller never explicitly asks "what do I know about this?" -- the engine anticipates.

This is the operation that makes HEBBS indispensable for real-time agents. A voice sales agent mid-call does not have 500ms to construct a recall query, wait for results, and integrate them. It needs relevant memories pushed into its context window as the prospect speaks. Subscribe is the mechanism that delivers that.

The decisions made here are load-bearing for four downstream concerns:

- **Phase 7 (Reflect):** The reflect pipeline may subscribe to its own memory stream to detect when new evidence triggers re-evaluation of existing insights.
- **Phase 8 (gRPC Server):** Subscribe maps to a bidirectional streaming RPC. The core subscribe engine must produce output that the server can bridge to a gRPC stream without transformation.
- **Phase 13 (Sync):** Cross-device subscriptions -- one device subscribes to memories arriving from another -- reuse the same pipeline.
- **ScalabilityArchitecture.md:** At 10,000 concurrent subscriptions in cloud mode, the hierarchical filtering pipeline is the only thing that keeps latency bounded. The bloom → coarse → fine architecture must be correct in Phase 6; Phase 13 only tunes it.

---

## Scope Boundaries

### What Phase 6 delivers

- `subscribe()` operation: opens a long-lived subscription that accepts a stream of text chunks and pushes relevant memories back when confidence exceeds a threshold
- Hierarchical filtering pipeline with three stages: bloom filter pre-screening, coarse embedding match, fine HNSW search
- Text chunk accumulation: batching incoming tokens into embeddable-length text segments before processing
- Subscription lifecycle management: open, pause, resume, close, with bounded resource consumption per subscription
- Deduplication: within a subscription session, the same memory is pushed at most once (configurable reset)
- Backpressure: bounded output queue per subscription with configurable overflow policy
- Multiple concurrent subscriptions per engine instance
- Notification on new writes: active subscriptions are evaluated when `remember()` creates a new memory that falls within the subscription's scope
- Configuration types for subscription parameters (chunk size, confidence threshold, bloom filter capacity, output queue depth)
- Full test coverage: unit, property-based, integration, and Criterion benchmarks

### What Phase 6 explicitly does NOT deliver

- gRPC bidirectional streaming transport (Phase 8 -- but the core subscribe engine must be transport-agnostic and bridgeable to gRPC without transformation)
- Async/tokio integration (Phase 8 -- Phase 6 uses synchronous channels, same as Phase 5's decay worker)
- Cluster centroids for the coarse filtering stage (Phase 7's reflect pipeline produces clusters -- Phase 6 uses a simpler centroid approximation computed from the subscription scope)
- Cross-device subscription (Phase 13 -- but the subscription state model must accommodate remote memory arrival)
- Configuration file loading (Phase 8 -- subscription parameters are passed programmatically)
- Metrics emission for subscribe latency and push rates (hook points are designed now; wiring happens incrementally)
- Subscribe-triggered recall reinforcement (subscribe pushes are informational; they do not update `access_count` or `last_accessed_at` -- the caller invokes `recall()` explicitly if they want reinforcement)

These exclusions are deliberate. Phase 6 builds the continuous query engine. Phase 8 wraps it in a transport. Phase 7 improves its filtering quality. Phase 13 extends its scope across devices.

---

## Architectural Decisions

### 1. The Subscribe Model: Continuous Query, Not Pub/Sub

This is the most consequential decision in Phase 6. Two fundamentally different models exist.

**Model A: Event-Driven Pub/Sub.** The subscription registers a filter (entity scope, memory kind). When `remember()` writes a new memory that matches, it is pushed to the subscriber. The subscriber receives new writes, not search results.

| Aspect | Consequence |
|--------|-------------|
| Latency | Excellent -- no embedding or search needed on push. The matching is a simple filter check on the write path. |
| Utility | Limited. The subscriber only sees new memories. If the agent is mid-conversation and the relevant memory was written an hour ago, it will never be pushed. |
| Write path impact | Every `remember()` must check all active subscriptions. At 1,000 subscriptions, this adds O(1000) filter checks to the hot path. |
| Real-time recall | Not supported. The agent cannot say "here's what I'm hearing right now, tell me what's relevant." |

**Model B: Continuous Query (Bidirectional Stream).** The subscriber sends a stream of text chunks. For each chunk (or accumulated batch), the engine embeds it, searches for relevant memories, and pushes matches that exceed a confidence threshold. The subscriber also receives notifications when new memories are written that match the subscription scope.

| Aspect | Consequence |
|--------|-------------|
| Latency | 8ms budget. Requires hierarchical filtering to stay bounded. |
| Utility | High. The agent feeds its live input and receives relevant memories regardless of when those memories were created. |
| Write path impact | New-write notification is a secondary path, not the primary mechanism. The write path fans out to active subscriptions asynchronously (not on the `remember()` hot path). |
| Real-time recall | The core capability. This is what the PhasePlan describes. |

**Decision: Model B (Continuous Query) with new-write notification as a secondary channel.**

The continuous query model matches the voice sales use case exactly: the agent feeds the live transcript, and HEBBS pushes "this prospect mentioned budget constraints last quarter" as the prospect speaks. Model A cannot do this. Model B can do everything Model A does (by processing new memories through the pipeline when they arrive) plus continuous recall over live input.

The new-write notification is implemented as an internal fan-out: when `remember()` completes, it sends the new memory's ID to all active subscriptions whose scope matches. The subscription pipeline then evaluates the new memory against the subscriber's current accumulated context. This keeps `remember()` non-blocking (the fan-out goes through a bounded channel) while giving subscriptions awareness of freshly written memories.

### 2. Text Chunk Accumulation

Live transcript tokens arrive one word at a time. Embedding a single word is meaningless -- the embedding model (384-dim BGE-small) needs at least a sentence-length input to produce a semantically useful vector. The subscribe engine must accumulate tokens into chunks before processing.

**The accumulation problem has three dimensions:**

| Dimension | Options | Tradeoff |
|-----------|---------|----------|
| Trigger | Fixed token count vs sentence boundary vs time deadline | Fixed count is predictable but may split mid-thought. Sentence detection adds latency waiting for punctuation. Time deadline caps latency but may embed fragments. |
| Window | Tumbling (non-overlapping chunks) vs sliding (overlapping) | Tumbling is simple and cheap. Sliding captures cross-boundary context but doubles embedding cost. |
| Minimum length | Strict minimum before any processing vs best-effort | Strict minimum prevents embedding nonsense. Best-effort avoids buffering stalls on slow input. |

**Decision: Time-bounded tumbling window with minimum token threshold.**

The accumulator buffers incoming text. It flushes and triggers the pipeline when ANY of the following conditions are met:

1. The buffer reaches `chunk_min_tokens` tokens (default: 15). This ensures the embedding has enough semantic content.
2. A time deadline of `chunk_max_wait_us` (default: 500ms) elapses since the first token in the current buffer. This caps latency for slow-arriving input.
3. The caller explicitly flushes (e.g., end of utterance signal).

If condition 2 fires but the buffer has fewer than 3 tokens, the flush is suppressed (to avoid embedding single words). The deadline resets.

**Why tumbling, not sliding:** A sliding window with 50% overlap doubles the embedding cost (every token is embedded twice). At 8ms p99, there is no budget for redundant embeddings. The tumbling window loses some cross-boundary context, but the confidence threshold compensates -- a memory that is marginally relevant to the end of one chunk and the start of the next will likely be caught by one of the two chunks.

**Why 15 tokens default:** BGE-small's embedding quality degrades below ~10 tokens. 15 tokens is approximately one spoken sentence at average speaking pace (150 words/minute ≈ 2.5 words/second ≈ 6 seconds for 15 tokens). This balances embedding quality against latency.

### 3. The Hierarchical Filtering Pipeline

The filtering pipeline is the performance heart of subscribe. It must reduce 10M+ memories to 0-5 relevant results within 8ms. Brute-force HNSW search at every chunk would exceed budget at scale. The solution is progressive filtering where each stage is cheaper than the next and eliminates most candidates before the expensive final stage.

**Stage 1: Bloom Filter (target: < 100µs)**

A counting bloom filter constructed from keywords extracted from memories within the subscription scope. The filter answers: "does this input chunk contain ANY keyword that appears in ANY scoped memory?"

| Design parameter | Choice | Rationale |
|------------------|--------|-----------|
| What goes in the filter | Lowercased, stemmed content words from scoped memories. Stop words excluded. | Maximizes signal density. Stop words would cause every input to match. |
| Capacity | Proportional to scoped memory count. Default: 10 bits per keyword, targeting 1% false positive rate. | Standard bloom filter sizing. |
| Update policy | Rebuilt when subscription scope changes or when new memories arrive within scope. Rebuilds are asynchronous and atomic (swap on completion). | Avoids stale filters that miss new memories while keeping the check path lock-free. |
| Rejection semantics | If the bloom filter returns false, skip stages 2 and 3 entirely. The input chunk is irrelevant. | This is the 90%+ elimination that makes subscribe viable at scale. |

**Why bloom and not a hash set:** At 100K keywords, a hash set consumes ~6MB. A bloom filter at 10 bits/key consumes ~125KB. Per subscription, this matters when 1,000 subscriptions are active simultaneously.

**Stage 2: Coarse Embedding Match (target: < 1ms)**

A single inner-product comparison between the input chunk's embedding and a scope centroid vector. The centroid is the mean of all memory embeddings within the subscription scope. If the similarity is below a coarse threshold (default: 0.15), skip Stage 3.

| Design parameter | Choice | Rationale |
|------------------|--------|-----------|
| Centroid computation | Mean of all in-scope memory embeddings, updated incrementally (running mean) when new memories arrive. | O(1) update, O(1) storage. A single 384-dim vector per subscription. |
| Threshold | Default 0.15 (very permissive). | The coarse stage is a gross outlier filter, not a precision instrument. It rejects input about "weather" when all memories are about "enterprise sales." Phase 7 can replace this with cluster centroids for multi-topic scopes. |
| Comparison cost | One 384-dim inner product = 384 multiplies + 383 adds. | ~1µs on modern hardware. Negligible. |

**Why a single centroid and not multiple:** Phase 7's reflect pipeline produces memory clusters. Before Phase 7 is implemented, there are no clusters. The single centroid is a Phase 6 approximation that Phase 7 replaces with per-cluster centroids. The pipeline architecture supports both without structural change -- the coarse stage compares against 1 or K centroids through the same interface.

**Stage 3: Fine HNSW Search (target: < 5ms)**

Full HNSW similarity search using the input chunk's embedding. This is the existing `IndexManager::search_vector` path from Phase 3, with the subscription's confidence threshold applied as a post-filter.

| Design parameter | Choice | Rationale |
|------------------|--------|-----------|
| ef_search | Default 50 (lower than recall's default 100). | Subscribe is latency-critical and runs frequently. Trading recall quality for speed is acceptable because the same memory will likely match on a subsequent chunk. |
| top_k | Default 5 per chunk. | Subscribe pushes are supplementary context, not comprehensive search results. Fewer results, higher confidence. |
| Confidence threshold | Default 0.60. Only memories with `1.0 - distance > threshold` are pushed. | The threshold is the quality gate. Higher = fewer, more relevant pushes. Lower = more pushes, more noise. Configurable per subscription. |
| Entity scope filter | If the subscription specifies an entity, post-filter HNSW results to only include memories with matching entity_id. | HNSW is not entity-aware. Scoping happens after search. At k=5, this is negligible cost. |

**End-to-end pipeline latency breakdown:**

| Stage | Time | Cumulative |
|-------|------|------------|
| Bloom filter check | < 100µs | 100µs |
| Embed input chunk | ~3ms | 3.1ms |
| Coarse centroid comparison | < 10µs | 3.1ms |
| HNSW search (ef=50, k=5) | ~3ms | 6.1ms |
| Deserialize + threshold filter | ~0.5ms | 6.6ms |
| Dedup + push to output channel | < 100µs | 6.7ms |
| **Total** | | **< 7ms** |

This is within the 8ms p99 budget with 1.3ms of headroom.

**Critical observation: embedding is the dominant cost.** The embed call (~3ms) is almost half the budget. If the bloom filter or coarse stage eliminates the chunk, embedding is skipped entirely. This is why the bloom filter exists -- it saves 3ms of embedding cost for 90%+ of input chunks.

**Pipeline ordering: bloom BEFORE embed.** The bloom filter operates on raw text (keyword matching), not embeddings. It runs before the embedding call. If the bloom filter rejects the chunk, neither embedding nor any subsequent stage executes. This ordering is non-negotiable for the latency budget.

### 4. Channel and Threading Model

Phase 6 must work without tokio. Phase 8 bridges the synchronous channel API to async gRPC streams. The threading model follows the same pattern as Phase 5's decay worker: dedicated OS thread + crossbeam-channel for communication.

**Per-subscription architecture:**

Each subscription runs on a dedicated worker thread. Communication is via two crossbeam channels:

| Channel | Direction | Bounded? | Capacity |
|---------|-----------|----------|----------|
| Input (text chunks) | Caller → subscription worker | Yes | 1,000 chunks |
| Output (memory pushes) | Subscription worker → caller | Yes | 100 pushes |

The caller interacts via a `SubscriptionHandle` returned by `Engine::subscribe()`. The handle exposes:

- `feed(text)` -- send text to the input channel. Non-blocking. Returns `Err` if the input channel is full (backpressure signal to the caller).
- `try_recv()` -- non-blocking poll for pushed memories from the output channel.
- `recv_timeout(duration)` -- blocking receive with timeout.
- `close()` -- signals the worker to shut down. The worker finishes its current pipeline cycle and exits.
- `pause()` / `resume()` -- temporarily suspends processing without closing.
- `reset_dedup()` -- clears the deduplication set (useful when the conversation topic shifts).
- `stats()` -- returns subscription statistics (chunks processed, pushes sent, bloom rejections, chunks dropped).

**Why one thread per subscription, not a thread pool:**

| Model | Pros | Cons |
|-------|------|------|
| Thread-per-subscription | Simple lifecycle. No task scheduling. Each subscription is isolated -- one cannot starve another. Clean shutdown per subscription. | Thread creation overhead (~50µs). At 1,000 subscriptions, 1,000 OS threads. |
| Shared thread pool (rayon/custom) | Fewer OS threads. Better for very large subscription counts. | Scheduling overhead. One slow subscription can block others. Complex cancellation. Harder to reason about backpressure per subscription. |

**Decision: Thread-per-subscription for Phase 6. Phase 13 can introduce a pool if 1,000+ concurrent subscriptions are needed.**

At the target scale for Phase 6 (tens to low hundreds of subscriptions per engine), dedicated threads are simpler, more predictable, and easier to reason about. Each thread consumes ~8KB stack + the subscription state (~200KB for bloom filter + centroid + dedup set). At 100 subscriptions, total overhead is ~20MB. Manageable.

The thread pool optimization is a Phase 13 concern. The `SubscriptionHandle` abstraction hides the threading model, so the caller's code does not change.

**Thread lifecycle:**

The subscription worker thread runs a loop:

1. Wait for input on the input channel (blocking receive with timeout).
2. Accumulate text into the chunk buffer.
3. When the chunk buffer triggers (per Decision 2), run the hierarchical pipeline.
4. Push results to the output channel. If the output channel is full, apply overflow policy (drop oldest).
5. Check for control signals (pause, resume, close, reset_dedup, new-write notification).
6. Repeat.

The timeout on the blocking receive serves double duty: it is the `chunk_max_wait_us` deadline from Decision 2. If no input arrives within the deadline and the buffer has sufficient tokens, the buffer flushes.

### 5. Subscription State and Scope

Each active subscription maintains state that is bounded, predictable, and scoped.

**Scope parameters (set at subscription creation, immutable for the subscription's lifetime):**

| Parameter | Type | Default | Rationale |
|-----------|------|---------|-----------|
| `entity_id` | `Option<String>` | None (global scope) | Narrows the subscription to memories for a specific entity. Affects bloom filter construction, centroid computation, and HNSW post-filtering. |
| `memory_kinds` | `Vec<MemoryKind>` | All kinds | Restricts to specific memory types (e.g., only Episodes, not Insights). |
| `confidence_threshold` | `f32` | 0.60 | Minimum similarity score to push a memory. Range [0.0, 1.0]. |
| `time_scope_us` | `Option<u64>` | None (all time) | Only consider memories created within this many microseconds from now. Limits the effective memory set for the bloom filter and HNSW search. |

**Per-subscription mutable state:**

| State | Size | Purpose | Bounded? |
|-------|------|---------|----------|
| Bloom filter | ~125KB at 100K keywords | Stage 1 keyword pre-filter | Yes (proportional to scoped memory count, bounded by memory count) |
| Scope centroid | 384 × 4 = 1,536 bytes | Stage 2 coarse embedding match | Yes (fixed size) |
| Dedup set | 16 bytes × pushed count | Tracks which memory IDs have been pushed in this session | Yes (bounded by total memory count, but expected to be small -- tens to hundreds) |
| Chunk accumulation buffer | Variable, capped at 64KB | Buffers incoming text tokens | Yes (max 64KB, same as memory content limit) |
| Statistics counters | ~64 bytes | Chunks processed, pushes sent, bloom rejections, drops | Yes (fixed size) |

**Total per-subscription memory footprint:** ~130KB typical (dominated by bloom filter). At 100 concurrent subscriptions: ~13MB. At 1,000: ~130MB. Both are manageable.

### 6. Deduplication Strategy

A 30-minute voice call about "pricing" will trigger the HNSW search dozens of times with semantically similar chunks. Without deduplication, the same memory about "client's budget constraint" would be pushed repeatedly.

**Primary deduplication: per-session memory ID set.**

When a memory is pushed to the output channel, its 16-byte memory ID is added to a `HashSet`. Before any push, the engine checks this set. If the memory ID is present, the push is suppressed.

**Dedup reset:**

The caller can reset the dedup set explicitly via `SubscriptionHandle::reset_dedup()`. This is useful when the conversation topic shifts dramatically (e.g., from pricing to technical requirements). After reset, previously-pushed memories can be pushed again if they match the new context.

**Why not time-based dedup (suppress re-push within N seconds):** Time-based dedup is unpredictable. A memory pushed 30 seconds ago about "budget" is still relevant 31 seconds later if the conversation is still about budget. Session-scoped dedup with explicit reset gives the caller precise control.

**Why not content-based dedup (suppress memories with similar content):** Two different memories might have similar content but represent different observations (e.g., two separate calls where the client mentioned budget). Both are independently valuable. Dedup is by identity (memory_id), not by content similarity.

### 7. Backpressure and Bounded Output

Principle 4 mandates bounded fan-out. If the subscriber cannot consume pushes fast enough, the output channel fills up. The engine must handle this without unbounded buffering.

**Overflow policy: drop oldest.**

When the output channel (capacity: 100 pushes by default) is full and a new push arrives, the oldest un-consumed push is dropped. This is ring-buffer semantics.

| Policy | Behavior | Tradeoff |
|--------|----------|----------|
| Block (wait for consumer) | Producer stalls until consumer reads | Violates Principle 1 -- the pipeline thread blocks, stalling the subscription |
| Drop newest (discard incoming) | New pushes are lost; consumer sees stale data | Consumer gets outdated results |
| Drop oldest (evict head) | Consumer always gets the most recent pushes | Consumer may miss transiently-relevant memories, but current context is preserved |

**Decision: Drop oldest.** For a real-time agent, the most recently relevant memories are always more valuable than older pushes sitting unconsumed in a queue. If the consumer is falling behind, it should see current relevance, not historical queue contents.

The subscription statistics counter tracks the number of dropped pushes. The caller can monitor this to detect persistent backpressure and either increase the output channel capacity or optimize consumption speed.

### 8. New-Write Notification Path

When `remember()` creates a new memory, active subscriptions whose scope includes the new memory should evaluate it for potential push. This is the secondary notification channel from Decision 1.

**The notification must not block `remember()`'s hot path (Principle 1, 5ms budget).**

**Design: broadcast channel with bounded fan-out.**

The engine maintains a single broadcast channel. When `remember()` completes, it sends the new memory's ID (16 bytes) to the broadcast channel. Each subscription worker receives from its own end of the broadcast.

| Design parameter | Choice | Rationale |
|------------------|--------|-----------|
| Channel type | `crossbeam-channel` broadcast (or a simple fan-out: one sender, cloned receivers) | Synchronous, no tokio dependency. |
| Bounded? | Yes, capacity 1,000 per receiver | If a subscription is slow, it drops notification of old writes rather than blocking `remember()`. |
| What is sent | Memory ID only (16 bytes), not the full memory | Keeps the channel lightweight. The subscription worker fetches the full memory from storage only if the scope check passes. |
| Scope check | The subscription worker checks if the new memory's entity_id and kind match its scope before fetching and evaluating | Cheap filter to avoid unnecessary storage reads. |

**Processing order:** New-write notifications are interleaved with chunk processing. When the subscription worker's input channel receive times out (the chunk deadline), it also drains the notification channel. New memories that pass scope check are evaluated against the subscriber's current accumulated context using the same pipeline (bloom check on context, embed, coarse, fine). If the new memory's content is highly similar to recent context, it is pushed.

**Why evaluate against accumulated context, not just scope match:** A scope match only means the memory is in the right entity or kind. It does not mean it is relevant to the current conversation. A subscriber for entity "acme" processing a conversation about "pricing" should not be interrupted with a push about an "acme" memory related to "shipping logistics" that was just created by a different agent. Evaluating against the accumulated context preserves relevance.

### 9. Interaction with revise() and forget()

Active subscriptions must handle mutations to the memory set during their lifetime.

**forget():** When a memory is forgotten, it could be in a subscription's dedup set (already pushed) or a subscription's output queue (not yet consumed). Two cases:

- **Already pushed and in dedup set:** The dedup set entry becomes stale (points to a deleted memory). This is harmless -- the memory will never match again, and the stale entry is cleaned up on dedup reset or subscription close.
- **In output queue, not yet consumed:** The consumer may receive a push for a memory that no longer exists. When the consumer tries to use it, `get()` returns NotFound. This is documented behavior, not a bug. The subscription provides a best-effort stream, not a transactional guarantee.

**revise():** When a memory is revised, its content and embedding change. Active subscriptions with bloom filters and centroids built from the old content are now slightly stale. This is acceptable:

- The bloom filter may have false negatives for the revised content's new keywords until the next rebuild.
- The centroid may shift slightly. One revised memory out of thousands has negligible impact on the mean.
- Periodic bloom filter and centroid refresh (on a configurable interval, default: every 60 seconds or every 100 new writes, whichever comes first) corrects staleness.

**No real-time subscription invalidation on revise/forget.** The cost of real-time invalidation (locking dedup sets, draining output queues, recomputing bloom filters) is disproportionate to the benefit. Subscribe is a best-effort real-time stream. Eventual consistency within the refresh interval is sufficient.

### 10. Concurrency Model

Phase 6 introduces the first concurrent reader structure beyond `recall()`: multiple subscription worker threads reading from the same storage and HNSW index concurrently with each other and with the main engine operations.

**Why this is safe:**

- HNSW reads use the existing `RwLock<HnswGraph>` read lock (parking_lot). Multiple subscription workers and multiple `recall()` calls can read simultaneously. The only write lock holder is `remember()` during HNSW commit, which is a microsecond-scale operation.
- RocksDB reads are internally thread-safe. Multiple concurrent `get()` and `prefix_iterator()` calls do not block each other.
- The embedder is behind `Arc<dyn Embedder>` with a `Mutex` internally (in the ONNX session). Subscription workers calling `embed()` will serialize on this mutex. At 100 subscriptions all embedding simultaneously, this becomes a bottleneck.

**Embedding contention mitigation:**

This is the primary concurrency risk in Phase 6. The ONNX embedding session is single-threaded (one inference at a time). If 20 subscriptions all flush their chunk buffers in the same millisecond, they queue on the embedder mutex.

| Mitigation | Effect | Cost |
|------------|--------|------|
| Stagger chunk deadlines | Each subscription's `chunk_max_wait_us` is jittered by ±10% using its subscription ID as seed | Zero runtime cost, reduces temporal clustering |
| Batch embedding queue | A dedicated embedding worker thread accepts requests from all subscriptions, batches them, and calls `embed_batch()` | Amortizes ONNX overhead across subscriptions. Adds ~1ms of batching latency but improves throughput by 5-10x at high subscription counts |
| Accept serialization | At low subscription counts (< 20), mutex contention adds < 1ms average wait time | No complexity cost |

**Decision: Stagger chunk deadlines (always) + batch embedding queue (when subscription count exceeds a configurable threshold, default 10).**

For the common case (1-10 subscriptions), serialized embedding access is adequate. The staggering prevents worst-case clustering. For cloud-scale workloads (10+ subscriptions), the batch embedding queue activates. This is a runtime configuration, not a code path fork -- the batch queue with a single consumer and batch size 1 behaves identically to direct access.

### 11. Configuration Surface

Phase 6 introduces the subscribe configuration, following the same pattern as Phase 5's `DecayConfig` and `ForgetConfig`.

**Subscribe configuration parameters:**

| Parameter | Type | Default | Bounds | Rationale |
|-----------|------|---------|--------|-----------|
| `chunk_min_tokens` | usize | 15 | [3, 500] | Minimum tokens before a chunk is processed. Lower = more embedding calls, higher = more latency. |
| `chunk_max_wait_us` | u64 | 500,000 (500ms) | [10,000, 10,000,000] | Maximum time to buffer tokens before forced flush. |
| `confidence_threshold` | f32 | 0.60 | [0.0, 1.0] | Minimum similarity score to push a memory. |
| `hnsw_ef_search` | usize | 50 | [10, 500] | HNSW ef_search for subscribe queries. Lower than recall's default for latency. |
| `hnsw_top_k` | usize | 5 | [1, 100] | Maximum HNSW results per chunk. |
| `bloom_fp_rate` | f64 | 0.01 | (0.0, 0.5) | Target false positive rate for the bloom filter. |
| `coarse_threshold` | f32 | 0.15 | [0.0, 1.0] | Minimum centroid similarity to proceed to Stage 3. |
| `output_queue_depth` | usize | 100 | [10, 10,000] | Maximum pending pushes in the output channel. |
| `input_queue_depth` | usize | 1,000 | [100, 100,000] | Maximum pending text chunks in the input channel. |
| `bloom_refresh_interval_us` | u64 | 60,000,000 (60s) | [1,000,000, 3,600,000,000] | How often the bloom filter and centroid are rebuilt. |
| `bloom_refresh_write_count` | usize | 100 | [1, 10,000] | Number of new scoped writes that trigger a bloom/centroid rebuild. |
| `max_subscriptions` | usize | 100 | [1, 10,000] | Maximum concurrent subscriptions per engine. Principle 4. |
| `embed_batch_threshold` | usize | 10 | [1, 1,000] | Subscription count above which the batch embedding queue activates. |

### 12. Testing Strategy

**Layer 1: Unit tests (in `hebbs-core`, in-memory backend)**

Pipeline mechanics:
- Bloom filter: insert keywords, check presence, verify false positive rate is within target, verify rebuild correctness after memory addition.
- Coarse centroid: compute centroid for a set of embeddings, verify inner product comparison gives expected accept/reject decisions.
- Text accumulator: feed tokens one at a time, verify flush triggers at minimum token count, at time deadline, and on explicit flush. Verify sub-minimum buffers are not flushed on timeout alone.
- Full pipeline: feed a text chunk through bloom → coarse → fine, verify correct memory is pushed for a matching chunk and no push occurs for an irrelevant chunk.

Subscribe lifecycle:
- Open subscription, feed text, receive push, close subscription. Verify worker thread exits cleanly.
- Pause and resume: feed text while paused, verify no pushes. Resume, verify pipeline resumes.
- Multiple subscriptions on the same engine operate independently.
- Dedup: push a memory, feed the same text again, verify no re-push. Reset dedup, feed again, verify re-push.

Backpressure:
- Fill the output queue, feed more matching text, verify oldest pushes are dropped and newest are retained.
- Fill the input queue, verify `feed()` returns an error.

Validation:
- Empty confidence threshold (0.0) pushes all HNSW results.
- Maximum confidence threshold (1.0) pushes nothing (no memory is a perfect match for arbitrary text).
- Entity scope correctly filters: memories from entity B are never pushed to a subscription scoped to entity A.

New-write notification:
- Open a subscription, `remember()` a memory within scope, verify the subscription evaluates and potentially pushes the new memory.
- `remember()` a memory outside scope, verify no push.

**Layer 2: Property-based tests**

- For any subscription scope and any sequence of text chunks, every pushed memory has a similarity score >= the confidence threshold.
- For any subscription session, no memory_id appears more than once in the output (until dedup reset).
- The bloom filter's false negative rate is zero: if a keyword exists in the scoped memories, the bloom filter always returns true.
- Output queue depth never exceeds the configured capacity (backpressure bound).
- Chunk accumulator never produces a chunk exceeding 64KB.

**Layer 3: Integration tests (RocksDB backend)**

- Full lifecycle: remember 100 memories for entity A, subscribe to entity A, feed a transcript that mentions topics from 5 of those memories, verify those 5 memories are pushed (and no others).
- Concurrent subscriptions: 5 subscriptions on different entities, each fed different text, verify isolation (no cross-entity leakage).
- New-write during subscription: open subscription, feed text, then remember a new highly-relevant memory, verify it is pushed within the next pipeline cycle.
- Forget during subscription: push a memory, then forget it, verify the subscription does not break (consumer receives the push; subsequent lookups return NotFound as documented).
- Revise during subscription: revise a memory that was previously pushed, verify the revised version can be pushed on the next dedup reset (dedup is by ID, so the revised memory at the same ID is still suppressed until reset).
- Subscribe at scale: remember 10,000 memories, subscribe, feed 100 text chunks, verify latency is within 8ms p99 and that relevant memories surface.
- Concurrent subscribe + remember: one thread subscribes and processes a transcript while another thread writes 1,000 new memories. Verify no panics, no corruption, and that new writes within scope are eventually evaluated.
- Subscription close under load: close a subscription while the pipeline is mid-processing. Verify clean shutdown (no panics, thread exits, channels drained).

**Layer 4: Criterion benchmarks**

- Bloom filter check latency: 100K keywords, 1K checks. Target: < 100µs per check.
- Coarse centroid comparison: 384-dim inner product. Target: < 10µs.
- Full pipeline single chunk: bloom → embed → coarse → HNSW → dedup → push. Target: < 8ms p99.
- Pipeline with bloom rejection: feed irrelevant text to a subscription with 10K scoped memories. Target: < 200µs (bloom rejects, no embedding).
- New-write notification fan-out: `remember()` with 10, 50, 100 active subscriptions. Measure `remember()` latency overhead. Target: < 100µs additional at 100 subscriptions.
- Bloom filter rebuild: 10K memories, full rebuild. Target: < 50ms.
- Centroid recomputation: 10K embeddings. Target: < 10ms.
- Subscription throughput: feed 1,000 text chunks at 100 chunks/second to a subscription with 10K scoped memories. Measure pushes/second and p99 pipeline latency.

---

## Risk Register

| Risk | Impact | Probability | Mitigation |
|------|--------|------------|------------|
| Embedder mutex contention at 20+ concurrent subscriptions causes latency spikes | High -- subscribe p99 exceeds 8ms budget | Medium (depends on workload synchronization) | Batch embedding queue (Decision 10) activates above configurable threshold. Staggered chunk deadlines reduce temporal clustering. Benchmark at 50 subscriptions to validate. |
| Bloom filter false positive rate higher than expected (poor keyword extraction) | Low -- more chunks pass to embedding stage, increasing latency | Medium | Configurable fp_rate. Keyword extraction uses aggressive stop-word removal. Integration test validates false positive rate against a controlled dataset. |
| Single centroid is meaningless for multi-topic memory scopes | Medium -- coarse stage passes everything, offering no filtering benefit | High (any entity with diverse memories) | Acceptable for Phase 6. Phase 7 replaces single centroid with per-cluster centroids. The coarse stage threshold is very permissive (0.15) so it only catches gross outliers. The bloom filter is the primary eliminator. |
| Thread-per-subscription does not scale beyond ~1,000 OS threads | Medium -- limits cloud deployment concurrency | Low for Phase 6 (designed for tens to hundreds) | Document the limit. Phase 13 can introduce a thread pool if needed. The `SubscriptionHandle` abstraction hides the threading model. |
| New-write notification broadcast overwhelms subscription workers during bulk `remember()` | Medium -- notification channels fill up, subscriptions miss new writes | Low (bounded channels with drop-oldest semantics) | Bounded notification channel per subscription (capacity 1,000). Dropped notifications are acceptable -- the memory will be discovered via normal pipeline processing if relevant. Subscription stats track notification drops. |
| Chunk accumulation time deadline (500ms) is too high for real-time voice applications | Medium -- agent receives relevant memories 500ms late | Low (configurable) | Default is tuned for voice (~6 seconds of speech per chunk). Callers needing lower latency reduce `chunk_max_wait_us`. At 100ms deadline, the embedding budget is still met but chunks are shorter (3-5 tokens), reducing embedding quality. Document the tradeoff. |
| `remember()` hot path overhead from broadcast notification channel | High -- violates 5ms `remember()` budget if notification send blocks | Very low (crossbeam send is non-blocking for bounded channels) | The broadcast send is a bounded-channel try_send. If full, the notification is dropped (not blocking). Benchmark overhead at 100 subscriptions. Target: < 50µs additional. |
| Subscription state (bloom filter + centroid) becomes stale between refreshes | Low -- missed pushes for very new memories or revised content | Medium | Configurable refresh interval and write-count trigger. Default 60s / 100 writes ensures staleness is bounded. New-write notification path provides real-time awareness for individual memories independent of bloom/centroid staleness. |

---

## Deliverables Checklist

Phase 6 is done when ALL of the following are true:

- [x] `Engine::subscribe(SubscribeConfig)` returns a `SubscriptionHandle` with `feed()`, `try_recv()`, `recv_timeout()`, `close()`, `pause()`, `resume()`, `reset_dedup()`, `stats()`
- [x] Text chunk accumulation: tokens buffer until `chunk_min_tokens` is reached, `chunk_max_wait_us` elapses, or explicit flush
- [x] Stage 1 (Bloom filter): rejects input chunks with no keyword overlap against scoped memories. False positive rate within configured target.
- [x] Stage 2 (Coarse centroid match): rejects input chunks with gross semantic mismatch against scope centroid. Single 384-dim inner product comparison.
- [x] Stage 3 (Fine HNSW search): embeds chunk, searches HNSW, applies confidence threshold, entity scope filter, and deduplication
- [x] Pipeline ordering: bloom check runs BEFORE embedding call. Rejected chunks skip embedding entirely.
- [x] Per-session deduplication: same memory_id is pushed at most once per session (until explicit `reset_dedup()`)
- [x] Backpressure: output channel is bounded with drop-oldest overflow policy. Input channel is bounded with error return on overflow.
- [x] New-write notification: `remember()` sends memory ID to broadcast channel. Subscription workers evaluate new memories against current accumulated context.
- [x] New-write notification is non-blocking to `remember()` (try_send, drop on full)
- [x] Bloom filter and centroid are refreshed periodically (configurable interval and write-count trigger)
- [x] Multiple concurrent subscriptions operate independently with no cross-subscription interference
- [x] Subscription worker runs on a dedicated OS thread with crossbeam-channel communication
- [x] `SubscriptionHandle::close()` cleanly shuts down the worker thread (no dangling threads, no resource leaks)
- [x] `Engine::drop()` shuts down all active subscriptions
- [x] `SubscribeConfig` has documented defaults and validated bounds for all parameters
- [x] `max_subscriptions` limit is enforced (Principle 4: bounded everything)
- [x] Subscription statistics track: chunks processed, chunks rejected by bloom, chunks rejected by coarse, pushes sent, pushes dropped (backpressure), notification drops
- [x] `forget()` of a memory that was pushed does not crash the subscription (documented eventual-consistency behavior)
- [x] `revise()` of a scoped memory is picked up on next bloom/centroid refresh
- [x] No `unwrap()` or `expect()` on any path reachable by external input
- [x] No `unsafe` blocks
- [x] All unit tests pass (in-memory backend)
- [x] All property-based tests pass
- [x] All integration tests pass (RocksDB backend)
- [x] Criterion benchmarks established: full pipeline < 8ms p99, bloom rejection < 200µs, bloom rebuild < 50ms, remember() overhead < 100µs at 100 subscriptions
- [x] `cargo clippy` passes with zero warnings
- [x] `cargo fmt --check` passes
- [x] `cargo audit` passes
- [x] PhasePlan.md updated with Phase 6 completion marker and known issues
- [x] DocsSummary.md updated with Phase 6 entry

---

## Interfaces Published to Future Phases

Phase 6 creates contracts that later phases depend on. These interfaces are stable after Phase 6 and should not change without a documented migration plan.

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| `subscribe()` input contract (`SubscribeConfig` with entity scope, confidence threshold, chunk parameters) | Phase 8 (gRPC streaming RPC handler maps config to protobuf), Phase 10 (Rust SDK), Phase 11 (Python SDK) | Additive only. New optional config fields allowed. Existing fields never change semantics. |
| `SubscriptionHandle` API (`feed`, `try_recv`, `close`, `pause`, `resume`, `reset_dedup`, `stats`) | Phase 8 (bridges to gRPC stream lifecycle), Phase 10, Phase 11 | Additive only. New methods allowed. Existing methods never change behavior. |
| `SubscribePush` output type (memory, confidence score, push timestamp) | Phase 8 (protobuf response mapping), Phase 10, Phase 11, Phase 18, Phase 19 | Additive only. New fields allowed. Existing fields immutable. |
| Bloom filter refresh mechanism (interval + write-count trigger) | Phase 7 (reflect may replace bloom content with insight-derived keywords), Phase 13 (remote writes trigger refresh) | The refresh trigger interface is stable. What populates the bloom filter can change. |
| Coarse stage centroid interface (single centroid vs multi-centroid) | Phase 7 (replaces single centroid with per-cluster centroids from reflect) | The pipeline stage interface is stable. The centroid source is pluggable. |
| New-write broadcast channel (memory ID notification from `remember()`) | Phase 7 (reflect may subscribe to new writes for insight trigger detection), Phase 13 (remote memories broadcast to local subscriptions) | Additive. The broadcast payload may gain fields. Existing field (memory_id) is immutable. |
| `SubscribeConfig` struct | Phase 8 (TOML config maps onto this), Phase 12 (production tuning) | Additive only. |
| Subscription statistics counters | Phase 8 (Prometheus metrics export), Phase 9 (CLI diagnostics) | Additive only. New counters allowed. Existing counters never change semantics. |
