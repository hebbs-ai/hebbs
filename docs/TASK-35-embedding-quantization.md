# TASK-35: Embedding Quantization and Memory Efficiency

## Problem

HEBBS stores all embeddings as raw `Vec<f32>` in HNSW nodes (`hebbs-index/src/hnsw/node.rs`). No quantization, no compression, no tiered storage.

| Embedding model | Dimensions | Bytes per memory | At 100K memories | At 1M memories |
|-----------------|-----------|------------------|-------------------|----------------|
| OpenAI (default) | 1536 | 6,144 | ~600 MB | ~6 GB |
| OpenAI large | 3072 | 12,288 | ~1.2 GB | ~12 GB |
| Ollama/local | 768 | 3,072 | ~300 MB | ~3 GB |

This is all in-memory (HNSW graph). For enterprise deployments with large vaults, embedding memory is the scaling bottleneck.

## Research: TurboQuant (Google Research, 2025)

Source: https://research.google/blog/turboquant-redefining-ai-efficiency-with-extreme-compression/

### What TurboQuant does

Two-stage compression for vector data:

**Stage 1: PolarQuant**
- Randomly rotates vectors before quantizing. This spreads information evenly across dimensions, making simple per-component scalar quantization work much better.
- Converts from Cartesian to polar coordinates (radius + angles). Angle patterns are predictable and concentrated, so they compress well on a fixed grid.
- Eliminates expensive per-block normalization. Traditional scalar quantization stores min/max per block (1-2 extra bits per number). PolarQuant maps onto a fixed circular grid instead, requiring zero metadata overhead.

**Stage 2: QJL (Quantized Johnson-Lindenstrauss)**
- Allocates just 1 bit per component for error correction on top of Stage 1.
- Each component reduces to a sign bit (+1 or -1). Zero memory overhead.
- Specialized estimator balances high-precision queries against low-precision stored data.

### Key results

- 3-bit quantization with negligible quality loss (near theoretical lower bounds)
- 4-bit on H100: up to 8x performance over f32
- Zero accuracy loss on long-context benchmarks despite aggressive compression
- Data-oblivious: no dataset-specific tuning required
- Surpasses product quantization (PQ) and RabbiQ baselines on GloVe recall

### Why this matters for HEBBS specifically

1. **Data-oblivious is critical.** Vaults contain heterogeneous content (meeting notes, code, emails, documents). Methods requiring per-dataset calibration (product quantization, trained codebooks) would be fragile. TurboQuant works without tuning.

2. **Random rotation is nearly free.** One matrix multiply per vector at index time. No training, no codebook, no calibration data. Massive quality improvement for quantized search.

3. **Zero-overhead quantization constants.** At scale, the 1-2 extra bits per dimension for traditional scalar quant metadata adds up. PolarQuant's fixed grid eliminates this entirely.

4. **Polar coordinates separate magnitude from direction.** This is conceptually aligned with how cosine similarity works (direction matters, magnitude doesn't). Quantizing in polar space preserves what matters for similarity search.

## Current HEBBS architecture (relevant files)

| Component | File | Current state |
|-----------|------|---------------|
| HNSW node | `hebbs-index/src/hnsw/node.rs` | `vector: Vec<f32>`, no compression |
| Distance computation | `hebbs-index/src/hnsw/distance.rs` | f32 cosine/euclidean |
| Graph traversal | `hebbs-index/src/hnsw/graph.rs` | Loads full f32 vectors for every distance calc |
| Memory struct | `hebbs-core/src/memory.rs` | `embedding: Option<Vec<f32>>` |
| Index manager | `hebbs-index/src/manager.rs` | No tiered storage |
| Storage backend | `hebbs-storage/` | RocksDB, stores serialized f32 vectors |

## Quality Impact Analysis

### Will quantization decrease recall quality?

**Yes, slightly at HNSW navigation. No, at final ranking, if two-tier reranking is used.**

#### Where precision loss occurs

The only affected code path is HNSW graph traversal (`hebbs-index/src/hnsw/distance.rs`). With int8, each of the ~2,500 distance comparisons per query (ef_search=100, avg degree ~24) loses precision. This can cause greedy search to take slightly wrong turns, missing some true top-K neighbors.

#### Why the impact is smaller than expected

**1. Composite scoring is a safety net.** Final ranking uses four signals (`engine.rs:compute_composite_score`):

```
composite = 0.5 * relevance + 0.2 * recency + 0.2 * importance + 0.1 * reinforcement
```

Embedding similarity is only 50% of the final score. Even if quantization slightly reorders HNSW candidates, recency, importance, and access count stabilize the final ranking.

**2. Two-tier reranking eliminates final-score loss.** The proposed approach uses int8 only for HNSW candidate selection. Final relevance scores are recomputed from f32 vectors loaded from RocksDB. The int8 error only affects which candidates enter the reranking pool, not their final scores.

**3. ef_search provides an oversampling buffer.** Default ef_search=100 for a typical top_k=10 query means 10x oversampling. Even if quantization causes a few true neighbors to be missed during traversal, the 10x buffer absorbs most misses.

**4. Entity recall already oversamples 4x.** Entity-scoped queries use `ENTITY_OVERSAMPLE = 4` (`engine.rs` line 71), providing additional buffer.

**5. Industry baselines are strong.** int8 scalar quantization typically achieves 97-99% recall@10 vs f32 baseline across standard benchmarks. HEBBS target is >85% recall@10.

#### Compensating levers

| Lever | How it helps | Cost |
|-------|-------------|------|
| Increase ef_search (100 -> 150) | Explores more candidates, recovers missed neighbors | ~50% latency increase |
| Random rotation (Phase B) | Reduces quantization error, fewer wrong turns | One matrix multiply per query |
| Two-tier f32 rerank | Exact final scores, only candidate selection is approximate | RocksDB reads for top-K |

### Benchmark gap (prerequisite)

The current benchmark suite (`benches/baseline.json`) only tracks **latency** (p50/p99/p999), not **recall@k quality**. There is no existing recall@k measurement to serve as a baseline.

**Phase A must begin by adding recall@k quality benchmarks:**
1. Generate a synthetic dataset with known ground truth (brute-force f32 top-K)
2. Measure recall@1, @5, @10, @50 for the current f32 HNSW implementation
3. Establish the quality baseline before any quantization changes
4. Re-measure after quantization to prove quality stays within 2% threshold

Without this, any claim about quality preservation is unverifiable.

## Proposed approach

### Phase A: Scalar quantization (int8) with two-tier storage

**Impact: 4x memory reduction. Medium effort.**

The lowest-risk, highest-impact change:

1. **Quantized vectors for HNSW traversal.** Store int8 (or uint8) quantized vectors in the HNSW graph nodes. Each distance comparison during search uses the quantized vectors.
2. **Full-precision vectors for reranking.** Keep f32 vectors in RocksDB (on disk). After HNSW returns top-K candidates, load f32 vectors and recompute exact distances for final ranking.
3. **Per-vector scale/offset.** Store two f32 values per vector (scale + zero-point) for dequantization. At 1536 dims, this is 8 bytes overhead vs 1536 bytes saved = negligible.

```
Query (f32) ──> quantize ──> HNSW search (int8, ~4x faster)
                                  |
                              top-K candidates
                                  |
                              load f32 from disk ──> exact rerank ──> final results
```

Memory per node: 1536 bytes (int8) + 8 bytes (scale/offset) = ~1.5 KB vs 6 KB today.

### Phase B: Random rotation preprocessing (PolarQuant-inspired)

**Impact: better quantization quality. Low effort.**

Before quantizing to int8:
1. Generate a random orthogonal rotation matrix R (fixed per index, stored once).
2. Multiply every vector by R before quantization.
3. This spreads concentrated embedding dimensions evenly, reducing quantization error.

The rotation matrix is generated once at index creation (deterministic from a seed) and applied to both stored vectors and queries at search time. Cost: one matrix multiply per vector, O(d^2) but d is fixed and small (1536).

### Phase C: 4-bit quantization

**Impact: 8x memory reduction. High effort, needs careful benchmarking.**

After Phase A proves the two-tier architecture:
1. Reduce HNSW traversal vectors to 4-bit (two values per byte).
2. Apply PolarQuant-style polar coordinate transform for better 4-bit fidelity.
3. Add 1-bit QJL error correction layer if recall degrades.

Memory per node: ~768 bytes (4-bit) + overhead = ~800 bytes vs 6 KB today.

This needs Criterion benchmarks proving recall@10 stays within 2% of f32 baseline before merging.

### Phase D: SIMD-optimized quantized distance

**Impact: faster search. Medium effort.**

int8 and int4 distance computations can leverage:
- AVX-512 VNNI (x86): native int8 dot product
- NEON (ARM): int8 multiply-accumulate
- Potential 8-16x speedup over f32 distance on supported hardware

## Benchmarking requirements

Per AGENTS.md, every hot-path change needs Criterion benchmarks. Required:

1. **Recall@10 at 10K, 100K, 1M memories** (f32 baseline vs quantized)
2. **Search latency p50/p99** (f32 vs int8 vs int4)
3. **Memory footprint** (RSS measurement at 10K, 100K, 1M)
4. **Index build time** (with and without rotation preprocessing)
5. **Quality by content type** (ensure heterogeneous vault content doesn't degrade unevenly)

Regression threshold: recall@10 must stay within 2% of f32 baseline. Latency p99 must not increase.

## Open questions

1. Should quantization be configurable per vault, or always-on with a sensible default?
2. For Phase C (4-bit), is polar coordinate transform worth the complexity vs just using int8 with rotation?
3. Should we implement our own quantized HNSW or integrate an existing library (e.g., `quantized-vectors` crate)?
4. How does quantization interact with the associative index (`hebbs-index/src/associative.rs`)?
5. Disk-backed f32 with mmap vs explicit load-from-RocksDB for reranking: which fits HEBBS's crash-safety model better?

## Priority

High. This directly affects enterprise scaling limits and deployment cost. Phase A alone (int8 + two-tier) would let HEBBS handle 4x more memories in the same RAM, which is the difference between "works on a laptop" and "needs a beefy server" for mid-size vaults.
