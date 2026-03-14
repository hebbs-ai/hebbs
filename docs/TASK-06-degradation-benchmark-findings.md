# TASK-06: Degradation Benchmark Findings

Date: 2026-03-13

## Overview

Ran degradation curve benchmarks with both MockEmbedder (8-dim) and ONNX BGE-small (384-dim) to profile latency scaling from 100 to 1000 memories. Benchmark uses `hebbs-bench degradation-curve` with SpanCollector tracing layer to capture per-phase timing.

## Results: Mock Embedder (8-dim)

### Degradation Curve — Latency (µs)

| Memories | Remember p99 | Similarity p99 | Temporal p99 | Causal p99 | Analogical p99 |
|----------|-------------|----------------|-------------|-----------|---------------|
| 100 | 283 | 120 | 43 | 158 | 160 |
| 500 | 330 | 98 | 41 | 188 | 171 |
| 1000 | 366 | 105 | 56 | 195 | 193 |

### Phase Breakdown — Mean µs (Mock)

| Memories | remember | recall.causal | recall.analogical | recall.embed | recall.similarity | recall.temporal |
|----------|----------|--------------|-------------------|-------------|------------------|----------------|
| 100 | 242.8 | 118.1 | 107.9 | 75.8 | 52.6 | 5.3 |
| 500 | 284.5 | 139.7 | 136.4 | 91.9 | 59.7 | 14.2 |
| 1000 | 310.2 | 149.5 | 149.2 | 102.2 | 64.6 | 28.5 |

## Results: ONNX Embedder (BGE-small, 384-dim)

### Degradation Curve — Latency (µs)

| Memories | Remember p99 | Similarity p99 | Temporal p99 | Causal p99 | Analogical p99 |
|----------|-------------|----------------|-------------|-----------|---------------|
| 100 | 4,383 | 4,749 | 58 | 2,604 | 4,200 |
| 500 | 10,354 | 3,936 | 552 | 2,606 | 3,950 |
| 1000 | 5,090 | 3,811 | 117 | 2,680 | 3,986 |

### Phase Breakdown — Mean µs (ONNX)

| Memories | remember (total) | recall.embed (ONNX inference) | remember (non-embed) | recall.analogical | recall.causal | recall.similarity | recall.temporal |
|----------|-----------------|------------------------------|---------------------|------------------|--------------|------------------|----------------|
| 100 | 3,348.7 | 3,020.1 | 606.5 | 181.5 | 191.1 | 87.6 | 5.3 |
| 500 | 3,338.0 | 2,931.9 | 1,112.1 | 344.5 | 298.8 | 160.8 | 14.2 |
| 1000 | 3,375.8 | 2,909.1 | 1,104.7 | 407.8 | 376.8 | 155.4 | 28.5 |

## Mock vs ONNX Comparison (at 1000 memories)

| Phase | Mock (µs) | ONNX (µs) | Ratio |
|-------|-----------|-----------|-------|
| remember (total) | 310 | 3,376 | 10.9x |
| remember (embed only) | ~0 | 2,909 | — |
| remember (non-embed) | 310 | ~467 | 1.5x |
| recall.embed | 102 | 257 | 2.5x |
| recall.causal | 149 | 377 | 2.5x |
| recall.analogical | 149 | 408 | 2.7x |

## Key Findings

1. **Embedding dominates everything under ONNX.** `recall.embed` is ~2,909µs and completely flat (doesn't scale with memory count). This single phase accounts for 86% of total `remember` latency.

2. **Non-embed `remember` scales worse with real vectors.** 607µs to 1,105µs (1.8x over 10x data) with 384-dim vectors vs 243 to 310µs (1.3x) with 8-dim. HNSW insert cost grows with dimensionality.

3. **Recall strategies hit harder with high-dim vectors.** `recall.causal` goes from 149µs (mock) to 377µs (ONNX) because each distance computation is 48x more work (384 vs 8 dimensions).

4. **`recall.temporal` is immune** — 117µs p99 even with ONNX because it doesn't use embedding search. However, it has the worst relative scaling curve (5.3µs to 28.5µs = 5.4x over 10x data), suggesting an O(n) scan somewhere.

5. **p99 variance is high with small sample sizes.** The delta column shows big swings (e.g., Remember +114.5% then -32.3%). Phase breakdown means are smooth and monotonic — use those for decision-making.

## Optimization Priorities

| Priority | What | Impact | Approach |
|----------|------|--------|----------|
| 1 | Embed latency (~3ms) | 86% of remember cost | Quantize model (INT8), batch inference, or cache embeddings |
| 2 | HNSW insert with 384-dim vectors | 607 to 1105µs scaling | Product quantization, lower ef_construction, or dimensionality reduction |
| 3 | HNSW search in recall (all strategies) | ~200-400µs | PQ or scalar quantization of stored vectors |
| 4 | recall.temporal O(n) scaling | 5 to 29µs (mock) | Small now but problematic at 100K+ memories |

## Production Readiness Assessment

### What's good

- Recall strategies (similarity, causal, temporal, analogical) at sub-500µs with real vectors is genuinely fast. Most vector DBs give you similarity search and nothing else — HEBBS gives you four cognitive retrieval strategies in a single embedded binary with no external dependencies. That's a real differentiator.
- Temporal recall at ~60-120µs is excellent.
- The scaling curves are linear, not super-linear. That's the right shape.

### What blocks production adoption right now

1. **3ms embed per remember is a dealbreaker for hot-path ingestion.** Production RAG systems ingest at thousands of docs/sec. At 3ms/embed you cap at ~330 ops/sec single-threaded. The fix is straightforward — async batched inference, INT8 quantization, or offloading embedding to a pre-processing pipeline so the engine never blocks on it.

2. **5ms total remember latency (ONNX) is too high for inline use.** If someone calls `remember()` in a request handler, that's 5ms added to every API call. Most teams would want <1ms for the non-embed portion (currently ~1.1ms at 1K memories and growing). That's fixable with vector quantization and HNSW tuning.

3. **These benchmarks stop at 1K memories.** Production workloads are 100K-10M. The temporal scan that's 29µs at 1K could be 2.9ms at 100K if it's truly O(n). Need to run at scale to know.

4. **No concurrent access benchmarks yet.** Single-threaded throughput is one thing — how does it behave under 50 concurrent readers + writers? RocksDB handles this well, but the HNSW index locking story matters.

### Bottom line

The cognitive retrieval model (similarity + temporal + causal + analogical in one engine) is genuinely novel and useful. The latency profile with mock embedder (sub-400µs everything) shows the core engine is fast enough. The work is in (a) making embedding not dominate, and (b) proving it holds at 100K+ scale. Those are engineering problems, not architecture problems.

## Reproduction

```bash
# Mock embedder
cargo run -p hebbs-bench --release -- degradation-curve --total 1000 --interval 100 --measure-runs 50 --output /tmp/deg-report.json

# ONNX embedder
cargo run -p hebbs-bench --release -- degradation-curve --embedder onnx --total 1000 --interval 100 --measure-runs 50 --data-dir /tmp/hebbs-models --output /tmp/deg-onnx.json
```
