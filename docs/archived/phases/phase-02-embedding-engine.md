# Phase 2: Embedding Engine -- Architecture Blueprint

## Status: ✅ COMPLETE

All deliverables met. 130 tests passing across the workspace (53 in `hebbs-embed`, 30 unit in `hebbs-core`, 16 integration in `hebbs-core`, 13 property in `hebbs-core`, 18 in `hebbs-storage`). Criterion benchmarks established. Zero clippy warnings, zero `unsafe`, zero `unwrap()` on external paths.

---

## Intent

Phase 2 introduces the component that transforms text into the mathematical representation that makes similarity search, analogical recall, and associative memory possible. Without embeddings, HEBBS is a key-value store. With embeddings, it becomes a cognitive engine.

This phase produces one crate: `hebbs-embed`. By the end, every `remember()` call stores a dense vector embedding alongside the memory record. The embedding engine runs locally via ONNX Runtime with zero network dependencies, auto-detects hardware accelerators, and establishes the latency baseline that Phase 3 (HNSW index) and Phase 4 (similarity recall) will build on.

The decisions made here -- model selection, dimensionality, normalization strategy, trait boundary, tokenizer coupling -- are load-bearing for the entire recall pipeline. Changing the default embedding model after adoption means every stored vector becomes incompatible with new queries.

---

## Scope Boundaries

### What Phase 2 delivers

- `hebbs-embed` crate with `Embedder` trait and ONNX Runtime implementation
- Default model: BGE-small-en-v1.5 (384 dimensions), auto-downloaded on first use
- Tokenizer bundled with model (not separate)
- Single-text and batch embedding with L2 normalization
- Hardware accelerator auto-detection (CPU, CoreML/Neural Engine, CUDA, DirectML)
- Integration into `remember()`: every memory gets an embedding before persistence
- Pluggable provider interface for external embedders behind a feature flag
- Criterion benchmarks for embedding latency (single and batch)

### What Phase 2 explicitly does NOT deliver

- HNSW index or any similarity search (Phase 3)
- Any form of `recall()` (Phase 4)
- Async/background embedding pipeline (designed here, built when the async write pipeline is implemented)
- Model fine-tuning or training
- Multilingual model support (future, behind same trait)
- Embedding caching or deduplication

---

## Architectural Decisions

### 1. Crate Placement and Dependency Direction

```
hebbs-core  ──depends-on──>  hebbs-embed
hebbs-core  ──depends-on──>  hebbs-storage
hebbs-embed                  (standalone, no dependency on core or storage)
```

`hebbs-embed` is a standalone crate. It knows nothing about memories, RocksDB, or HEBBS concepts. It accepts text, returns float vectors. This isolation is critical:

- **Phase 9 (FFI):** Embedded mode links `hebbs-core` which pulls in `hebbs-embed`. The embed crate must not pull in server or network dependencies.
- **Phase 13 (Edge):** Edge devices may use a different model or accelerator. The embed crate is configured, not forked.
- **Testing:** `hebbs-core` tests can inject a mock embedder (a deterministic function that maps text to a fixed vector) to avoid loading the ONNX model in unit tests. ONNX model tests live in `hebbs-embed`'s own test suite.

`hebbs-core` depends on `hebbs-embed` via the `Embedder` trait, not a concrete type. The concrete `OnnxEmbedder` is injected at construction time. This is dependency inversion -- the core defines the interface, the embed crate provides the implementation.

### 2. Model Selection

The default model choice is one of the highest-leverage decisions in the project. It determines embedding quality, latency, memory footprint, and whether the system fits on edge devices.

**Candidates evaluated:**

| Model | Dimensions | ONNX Size | CPU Latency (single) | Quality (MTEB avg) | Verdict |
|-------|-----------|-----------|---------------------|-------------------|---------|
| BGE-small-en-v1.5 | 384 | ~33MB | ~3ms | 62.2 | **Selected as default** |
| all-MiniLM-L6-v2 | 384 | ~22MB | ~2ms | 58.8 | Faster but measurably lower quality |
| BGE-base-en-v1.5 | 768 | ~110MB | ~8ms | 64.2 | 2% quality gain costs 2.7x latency and 3.3x model size |
| nomic-embed-text-v1.5 | 768 | ~260MB | ~12ms | 65.1 | Too large for edge default |
| text-embedding-3-small (OpenAI) | 1536 | N/A (API) | ~50ms (network) | 62.3 | Network dependency, comparable quality to local BGE-small |

**Why BGE-small-en-v1.5:**

- **384 dimensions is the edge sweet spot.** At 10M memories: 384 dims * 4 bytes * 10M = 15.4GB of raw vectors. With HNSW overhead (~1.3x), that is ~20GB -- fits on a 32GB laptop with room for the OS and application. At 768 dims, the same count requires ~40GB, which does not fit.
- **3ms CPU latency.** This fits within the `remember()` p99 budget of 5ms (3ms embed + 1ms WAL + 1ms overhead). Going to 8ms for BGE-base would blow the budget.
- **33MB model file.** Downloads in seconds. Does not bloat the binary (shipped separately).
- **Quality is sufficient.** 62.2 MTEB average is competitive with OpenAI's text-embedding-3-small at 62.3. The quality delta between 62 and 65 is meaningful for academic benchmarks but negligible for the memory recall use case where the recall pipeline also uses temporal, causal, and analogical strategies. Similarity is one of four paths, not the only path.

**Upgrading is a configuration change, not a code change.** Users who want higher quality can configure `embedding.model = "bge-base-en-v1.5"` or `embedding.provider = "openai"`. The trait boundary makes this transparent to the rest of the system.

**Dimensionality is not hardcoded.** The embedder reports its dimensionality via `fn dimensions(&self) -> usize`. Phase 3 uses this to configure the HNSW index. Changing the model changes the dimensions, which requires rebuilding the HNSW index. This is documented as a breaking operation (similar to changing a database schema).

### 3. The Embedder Trait

The trait is the contract between `hebbs-core` and any embedding implementation. It must be minimal, correct, and future-proof.

**Trait surface:**

- `fn embed(&self, text: &str) -> Result<Vec<f32>>` -- embed a single text, return a normalized vector
- `fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>` -- embed multiple texts in one call, return normalized vectors
- `fn dimensions(&self) -> usize` -- return the dimensionality of vectors this embedder produces

Three methods. Not four, not five. This is the complete interface.

**Design decisions embedded in the trait:**

- **Normalization is the embedder's responsibility.** Every vector returned by `embed()` and `embed_batch()` is L2-normalized. Callers never normalize. This is critical because Phase 3's HNSW index uses inner product distance (which equals cosine similarity when vectors are L2-normalized). If any code path returns an unnormalized vector, similarity search produces wrong results. The invariant is enforced inside the trait implementation, not at call sites.
- **The trait is synchronous.** Embedding is CPU/GPU-bound, not I/O-bound. Making the trait `async` would add unnecessary overhead (future polling, task scheduling) for zero benefit. The ONNX Runtime session runs on the calling thread (or its own internal thread pool for batches). If a future external provider (OpenAI API) needs async, the `PluggableEmbedder` implementation handles the async internally and blocks to return a synchronous result. The core engine does not need to know.
- **Errors are opaque at the trait level.** `Result<Vec<f32>>` uses the project error taxonomy (`EmbeddingError` variant under `Internal`). The trait does not expose ONNX-specific errors, tokenizer errors, or model loading errors. These are wrapped with context.
- **The trait is `Send + Sync`.** Multiple threads can call `embed()` concurrently on the same embedder instance. This is required by Phase 8 (gRPC handler spawns concurrent tasks) and Phase 6 (`subscribe()` pipeline embeds input chunks in parallel with `recall` queries).

### 4. ONNX Runtime Integration

ONNX Runtime is the inference engine. The integration must be correct, performant, and hardware-aware.

**Session lifecycle:**

- Model loading and ONNX session creation happen once at embedder construction time. Session creation is expensive (100-500ms): model parsing, graph optimization, memory allocation. This cost is paid once at startup, never on the hot path.
- The `Session` object is stored inside the embedder and reused for every inference call. ONNX Runtime sessions are thread-safe for concurrent inference.
- Sessions are not cloned or recreated. One session per embedder instance. One embedder instance per HEBBS engine.

**Execution providers (hardware acceleration):**

ONNX Runtime supports multiple execution providers (EPs). The embedder auto-detects and selects the best available:

| Priority | Execution Provider | Platform | Expected Latency (384-dim) |
|----------|-------------------|----------|---------------------------|
| 1 | CoreML | macOS (Apple Silicon) | ~1ms |
| 2 | CUDA | Linux/Windows (NVIDIA GPU) | ~2ms |
| 3 | DirectML | Windows (any GPU) | ~3ms |
| 4 | CPU | All platforms | ~3-5ms |

**Auto-detection strategy:** Attempt providers in priority order. If a provider fails to initialize (no GPU, no driver), fall back to the next. Log which provider was selected at startup. This is a one-time cost during embedder construction.

**Why not always use CPU?** On Apple Silicon, CoreML uses the Neural Engine which is 3-5x faster than CPU for small models. On NVIDIA hardware, CUDA batching is dramatically faster for `embed_batch`. The auto-detection is free (try/catch during init) and the performance gain is significant.

### 5. Tokenizer Strategy

The model and tokenizer are an inseparable pair. A tokenizer trained for BGE-small-en produces different token IDs than one trained for MiniLM. Using the wrong tokenizer produces garbage embeddings that silently degrade recall quality.

**Decision: bundle tokenizer.json with the model file.**

Both the ONNX model file and the `tokenizer.json` are downloaded together as a pair. They are versioned together. The embedder loads both from the same directory. There is no configuration to "mix and match" a tokenizer with a different model.

**Tokenizer library:** Use the `tokenizers` crate (HuggingFace's Rust tokenizer library). It is fast (microseconds per tokenization), supports all major tokenizer types (BPE, WordPiece, Unigram), and loads from the standard `tokenizer.json` format.

**Text preprocessing pipeline:**

1. Accept raw text (UTF-8 string)
2. Truncate to model's max sequence length (512 tokens for BGE-small-en). Do not error -- silently truncate. Memories longer than 512 tokens lose trailing content in the embedding, but the full text is stored in the Memory record for exact retrieval. This is a tradeoff: embedding quality degrades for very long texts, but the system does not reject them.
3. Tokenize to token IDs + attention mask
4. Pad batch to uniform length (for batch inference)
5. Run ONNX inference
6. Mean pooling over token embeddings (excluding padding tokens)
7. L2 normalization

**Why mean pooling over [CLS] token:** BGE-small-en is trained with mean pooling. Using [CLS] token output with a mean-pooling model reduces quality by ~5% on MTEB benchmarks. The pooling strategy must match the model's training. This is hardcoded per model, not configurable.

### 6. Model Distribution

The ONNX model is not compiled into the binary. It is too large (33MB) and would make every release download wasteful for users who use external embedders.

**Distribution strategy:**

1. On first `remember()` call, if no model is present at the configured model path, download it.
2. Download from a CDN URL (configurable, default: HEBBS project hosting).
3. Verify SHA-256 checksum after download. Reject tampered files. This is a security requirement -- a compromised model could produce adversarial embeddings that degrade recall.
4. Store in the data directory alongside RocksDB files: `{data_dir}/models/bge-small-en-v1.5/model.onnx` and `tokenizer.json`.
5. Subsequent starts find the model on disk and skip the download.

**Offline/air-gapped deployment:** Users can pre-place the model files in the expected directory. No download attempt is made if the files exist and checksums match. This is critical for edge devices (factory robots) and government deployments where outbound network access is prohibited.

**Model directory structure:**

```
{data_dir}/models/
  bge-small-en-v1.5/
    model.onnx          (33MB, SHA-256 verified)
    tokenizer.json       (~700KB)
    config.json          (metadata: dimensions, max_seq_length, pooling_strategy)
```

The `config.json` is a HEBBS-defined file (not HuggingFace format) that captures the model's properties. The embedder reads it to configure tokenization and pooling without hardcoding model-specific logic.

### 7. Batch Embedding Amortization

Batch embedding is not a convenience feature. It is a performance requirement for Phase 5 (background write pipeline) and Phase 7 (reflect clustering).

**Why batching matters:**

- ONNX Runtime has per-inference overhead: input tensor allocation, kernel launch, output copy. For a single 384-dim embedding, this overhead is ~0.5ms on CPU.
- Batching amortizes the overhead: 64 texts in one call costs ~8ms total (~0.125ms/text) vs 64 sequential calls at ~3ms each (~192ms total). That is a 24x throughput improvement.
- GPU execution providers benefit even more: CUDA kernel launches are expensive per call but parallelize across batch items. Batch-64 on CUDA is often only 2-3x slower than batch-1, making it ~20-30x more efficient per item.

**Batch size limits:**

- Maximum batch size: 256 texts per call. This bounds memory usage: 256 texts * 512 tokens * 4 bytes (int32 token IDs) * 2 (token IDs + attention mask) = ~1MB of input tensors. Bounded (Principle 4).
- If a caller passes more than 256 texts, `embed_batch` splits internally into chunks of 256, processes sequentially, and concatenates results. The caller does not need to manage chunking.

**Padding strategy:**

- Within a batch, all inputs are padded to the length of the longest input in that batch (not to max_seq_length). This avoids wasting computation on 512-token padding when the longest input is 50 tokens.
- Attention mask correctly marks padding tokens so mean pooling excludes them.

### 8. Integration with `remember()`

Phase 2 changes the `remember()` pipeline from Phase 1. This is the first mutation of a Phase 1 interface and must be handled carefully.

**Phase 1 `remember()` pipeline:**

```
validate -> generate ULID -> construct Memory(embedding=None) -> serialize -> write to default CF -> return ID
```

**Phase 2 `remember()` pipeline:**

```
validate -> generate ULID -> embed(content) -> construct Memory(embedding=Some(vector)) -> serialize -> write to default CF -> return ID
```

**What changed:** One new step (`embed(content)`) inserted between validation and construction. The embedding is computed before the Memory struct is built so it can be stored inline.

**Latency impact:**

- Phase 1 `remember()`: ~400ns (in-memory backend)
- Phase 2 `remember()`: ~3-5ms (dominated by embedding, in-memory backend)

This is a 10,000x increase. It is expected and budgeted. The `remember()` latency target is 5ms p99 (from GuidingPrinciples.md), and embedding is the dominant cost. The WAL write (~1ms on real disk) is additive but still within budget.

**The future async path (designed now, built later):**

The ultimate architecture (Phase 5+) decouples embedding from the acknowledge path:

```
validate -> generate ULID -> construct Memory(embedding=None) -> write to WAL -> return ID (< 1ms)
                                                                    |
                                                                    └──> background: embed -> update Memory -> write indexes
```

Phase 2 does not implement this decoupled pipeline. It embeds synchronously. But the design must not preclude the async path:

- The `Embedder` trait is separate from `remember()`. The core engine holds an `Arc<dyn Embedder>` that can be called from any thread (hot path or background worker).
- The `Memory` struct already has `embedding: Option<Vec<f32>>`. The async path writes `None` initially and updates to `Some(vector)` later. This update is a single-key put in the default CF -- no schema change needed.
- The background worker pattern (tokio task pool consuming from a channel) is described here for the implementer's awareness but not built in Phase 2.

### 9. Memory Management

ONNX Runtime allocates significant memory for the inference session. This must be predictable and bounded (Principle 4).

**Memory breakdown for BGE-small-en-v1.5:**

| Component | Size | Lifecycle |
|-----------|------|-----------|
| ONNX model weights (in memory) | ~33MB | Loaded at startup, never freed until shutdown |
| ONNX session overhead (graph, metadata) | ~5MB | Same lifecycle as weights |
| Tokenizer vocabulary | ~2MB | Loaded at startup |
| Input tensor buffer (per inference) | ~4KB single, ~256KB batch-256 | Allocated per call, freed after call |
| Output tensor buffer (per inference) | ~1.5KB single, ~384KB batch-256 | Same as input |

**Total steady-state:** ~40MB for the embedding engine. This is fixed regardless of memory count. It does not scale with N.

**The embedding engine's contribution to the RAM formula:** Phase 1 defined `RAM ≈ N * (metadata_bytes + D * 4 + hnsw_overhead)`. Phase 2 adds a fixed ~40MB constant: `RAM ≈ 40MB + N * (metadata_bytes + D * 4 + hnsw_overhead)`.

**Embedding vectors in the Memory record:**

Each memory's embedding adds `384 * 4 = 1,536 bytes` to the serialized record. At 10M memories, this is ~15.4GB of additional disk. This is the dominant storage cost -- the rest of the Memory struct is ~200 bytes. The total per-memory on-disk cost goes from ~200 bytes (Phase 1) to ~1,736 bytes (Phase 2).

This is within the < 2KB per memory target from BenchmarksAndProofs.md.

### 10. Pluggable Provider Architecture

External embedding providers (OpenAI, Cohere, Ollama) are behind a feature flag: `--features external-embeddings`. They are not in the default build to avoid pulling in HTTP client dependencies (`reqwest`, `tokio`, TLS) for users who only need the built-in model.

**Provider design:**

Each external provider implements the same `Embedder` trait. From `hebbs-core`'s perspective, a local ONNX model and OpenAI's API are interchangeable.

**Key differences from the local implementation:**

| Concern | Local (ONNX) | External (API) |
|---------|-------------|---------------|
| Latency | ~3ms | 50-200ms (network) |
| Availability | Always | Requires network |
| Cost | Zero | Per-token pricing |
| Thread safety | Native | Rate limiting needed |
| Batch size | Limited by RAM | Limited by API |

**External providers are NOT for the hot path.** They violate Principle 1 (no network calls on hot path). They exist for:

- Users who want higher-quality embeddings and accept the latency
- The background re-embedding pipeline (re-embed existing memories with a better model)
- Testing against a specific model (match embeddings with a production API)

**Rate limiting:** External provider implementations must include built-in rate limiting (token bucket). API providers have per-minute/per-second quotas. Exceeding them causes 429 errors that cascade into `remember()` failures. The rate limiter smooths burst traffic.

### 11. Testing Strategy

**Layer 1: Unit tests (in `hebbs-embed`)**

- Tokenizer loads correctly from `tokenizer.json`.
- Tokenization produces expected token IDs for known inputs.
- Token truncation at max_seq_length works correctly (long input is truncated, not errored).
- Batch padding pads to max length within batch, not global max.
- L2 normalization produces unit-length vectors (verify ||v|| = 1.0 within floating-point tolerance).
- `dimensions()` returns the correct value for the loaded model.
- `embed()` and `embed_batch()` produce identical vectors for the same input (determinism test).
- `embed_batch([single_text])` produces the same vector as `embed(single_text)` (batch-single equivalence).

**Layer 2: Mock embedder for `hebbs-core` tests**

- A `MockEmbedder` that returns a deterministic vector based on a hash of the input text. This lets `hebbs-core` test `remember()` with embeddings without loading the ONNX model.
- The mock embedder returns L2-normalized vectors (same invariant as the real embedder).
- `hebbs-core` tests verify: `remember()` stores a non-None embedding, `get()` returns the memory with its embedding intact, the embedding dimensions match the mock's configured dimensions.

**Layer 3: Integration tests (requires ONNX model)**

- Full pipeline: `remember("some text")` produces a memory with a 384-dim embedding that is L2-normalized.
- Semantic coherence: `embed("dog")` is closer to `embed("puppy")` than to `embed("airplane")` by cosine similarity. This is a smoke test, not a full quality benchmark, but it catches catastrophic model loading errors.
- Model auto-download: start with an empty model directory, call `remember()`, verify the model is downloaded and subsequent calls do not re-download.
- Checksum verification: corrupt the model file, attempt to load, verify the error is caught and reported (not a silent quality degradation).
- Concurrent embedding: 10 threads calling `embed()` simultaneously produce correct, deterministic results.

**Layer 4: Criterion benchmarks**

- `embed()` single text (short, 10 words): measure p50/p99 latency
- `embed()` single text (long, 200 words): measure p50/p99 latency
- `embed_batch()` batch of 16 texts: measure total and per-item latency
- `embed_batch()` batch of 64 texts: measure total and per-item latency
- `embed_batch()` batch of 256 texts: measure total and per-item latency
- `remember()` end-to-end with embedding (in-memory storage backend): measure p50/p99 latency
- Tokenizer throughput: texts tokenized per second

The embedding latency must be < 5ms p99 for a single 200-word text on CPU. This is the contract from BenchmarksAndProofs.md.

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| ONNX Runtime `ort` crate has breaking API changes | Medium -- blocks compilation | Pin exact `ort` version. Monitor releases. The `ort` crate has had major API changes between v1 and v2. Pin the major version and audit upgrade impact. |
| BGE-small-en-v1.5 model quality is insufficient for real-world recall | High -- users see poor recall | Quality is validated in Phase 4 (recall) with real workloads. If insufficient, swap to BGE-base (768-dim) via config. The trait boundary makes this a 1-line change for users. |
| CoreML execution provider causes segfaults on certain macOS versions | Medium -- crashes on Apple Silicon | Test on macOS 13, 14, 15. If unstable, disable CoreML EP and fall back to CPU. CPU at 3-5ms is still within budget. |
| Model download fails in CI/CD environments with no internet | Medium -- tests fail | CI caches the model file. Integration tests that require the model are behind a feature flag or use the mock embedder. |
| Embedding latency exceeds 5ms budget on low-end CPUs | High -- violates latency contract | Benchmark on target hardware (c6g.large, Jetson Orin). If budget is blown, evaluate: (a) smaller model (MiniLM at 2ms), (b) quantized ONNX model (INT8, ~40% faster), (c) move embedding to background pipeline earlier than planned. |
| Tokenizer panics on malformed UTF-8 or adversarial input | High -- security audit failure | Phase 1 already validates UTF-8 on `remember()` input. The embedder receives only validated text. Add fuzz testing with `cargo-fuzz` targeting the tokenizer input path. |
| Embedding vectors are not deterministic across platforms (x86 vs ARM) | Medium -- different platforms produce different HNSW neighbors | Floating-point determinism across architectures is not guaranteed by ONNX Runtime. Accept within epsilon tolerance (< 1e-5 per dimension). Similarity rankings should be stable even if exact vectors differ. Test with cross-platform round-trip comparison. |

---

## Deliverables Checklist

Phase 2 is done when ALL of the following are true:

- [x] `hebbs-embed` crate compiles independently (no dependency on core or storage)
- [x] `Embedder` trait exposes `embed()`, `embed_batch()`, `dimensions()` -- all `Send + Sync`
- [x] `OnnxEmbedder` implementation loads BGE-small-en-v1.5 from disk
- [x] Model auto-download with SHA-256 verification works on first run
- [x] Offline mode works (pre-placed model files, no download attempt)
- [x] Hardware acceleration auto-detection selects the best available EP
- [x] All returned vectors are L2-normalized (unit length within floating-point tolerance)
- [x] `embed()` and `embed_batch()` produce identical vectors for the same input
- [x] Batch-single equivalence: `embed_batch([x])` == `embed(x)`
- [x] Text truncation at 512 tokens (no error, silent truncate)
- [x] Batch size bounded at 256 (larger batches chunked internally)
- [x] `remember()` in `hebbs-core` stores a non-None embedding
- [x] `MockEmbedder` available for core tests (deterministic, normalized, no ONNX dependency)
- [x] Pluggable external provider compiles behind `--features external-embeddings`
- [x] Semantic smoke test passes (dog closer to puppy than airplane)
- [x] Concurrent embedding test (10 threads) produces correct results
- [x] `embed()` single text (200 words) < 5ms p99 on CPU
- [x] `embed_batch()` 64 texts < 12ms total on CPU
- [x] `remember()` end-to-end with embedding < 5ms p99 (in-memory storage backend)
- [x] No `unwrap()` or `expect()` on paths reachable by external input
- [x] No `unsafe` blocks (ONNX Runtime FFI is inside the `ort` crate, not ours)
- [x] `cargo audit` passes
- [x] `cargo clippy` passes with zero warnings

---

## Interfaces Published to Future Phases

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| `Embedder` trait (embed, embed_batch, dimensions) | 3, 4, 6, 7 | Immutable after Phase 2 (additive only -- new methods allowed, never change existing signatures) |
| L2 normalization invariant | 3, 4, 6 | Immutable. Every vector from any Embedder implementation is L2-normalized. Phase 3 HNSW uses inner product distance relying on this. |
| Default dimensionality (384) | 3, 13 | Configurable, but default is stable. Changing it requires HNSW index rebuild. |
| `MockEmbedder` for testing | 3, 4, 5, 6, 7 | Stable test utility. Must satisfy same invariants as real embedder. |
| Model directory layout (`{data_dir}/models/{model_name}/`) | 8 (server config), 13 (edge) | Stable path convention. Config references this layout. |
| `remember()` now populates `Memory.embedding` | 3, 4 | From Phase 2 onward, all new memories have embeddings. Phase 3 can assume embeddings exist for indexing. Pre-Phase-2 memories (if any exist from development) have `None` embeddings and are skipped by the vector index. |
