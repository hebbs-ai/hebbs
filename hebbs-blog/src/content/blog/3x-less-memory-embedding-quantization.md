---
title: "3x Less Memory. Same Recall Quality."
description: "HEBBS now quantizes HNSW embeddings to int8 with random rotation and two-tier reranking. At 1M memories with 1536-dim embeddings, that is the difference between 6 GB and 2 GB of RAM."
pubDate: 2026-04-01
tags: ["engineering", "performance", "embeddings", "release"]
heroImage: "/images/3x-less-memory-embedding-quantization.png"
ogImage: "/images/3x-less-memory-embedding-quantization.png"
---

Enterprise vaults grow. A hundred thousand memories fit comfortably in RAM. A million start to hurt. At 1536 dimensions (the OpenAI default), each f32 embedding vector eats 6 KB. Multiply by a million and your HNSW graph alone consumes 6 GB of memory before you count the graph structure, metadata, or anything else your server is doing.

This was the scaling bottleneck. Not disk, not CPU, not network. Memory.

Today's release fixes it. HEBBS now stores HNSW vectors as int8 instead of f32, cutting per-vector memory from 4 bytes per dimension to 1 byte. The math works out to roughly 3x memory reduction in practice (the graph structure and per-vector metadata take up some fixed space). At 1M memories, that is 6 GB down to approximately 2 GB.

The quality impact? Less than 1% reduction in recall. Here is how we kept it that low.

## Int8 Scalar Quantization

The core change is simple. Every f32 embedding vector stored in the HNSW graph is quantized to int8 using affine mapping. For each vector, we compute a `scale` and `zero_point` that map the vector's value range into the [-128, 127] integer range:

```
quantized[i] = round((original[i] - zero_point) / scale)
```

Two f32 values (scale and zero_point) are stored alongside each quantized vector. At 1536 dimensions, that is 8 bytes of overhead against 1536 bytes saved. Negligible.

The dequantization path is the inverse:

```
approximate[i] = quantized[i] * scale + zero_point
```

Distance computations during HNSW traversal use the quantized int8 vectors directly. This is where the memory savings come from: the HNSW graph, which must live entirely in memory for fast search, now holds 1-byte values instead of 4-byte values at every dimension of every vector.

## Random Rotation Before Quantization

Naive scalar quantization has a well-known weakness: if most of the information in an embedding is concentrated in a few dimensions (and it usually is), quantizing all dimensions to the same grid wastes precision on the important dimensions while over-allocating precision to near-zero dimensions.

The fix is to rotate vectors before quantizing. HEBBS applies a randomized Walsh-Hadamard transform that spreads energy evenly across all dimensions. After rotation, every dimension carries roughly the same amount of information, so uniform scalar quantization loses minimal fidelity.

The implementation uses a butterfly algorithm with O(d log d) complexity, which is significantly faster than a naive O(d^2) matrix multiply. Vectors are padded to the next power of 2 before the transform (so 1536-dim vectors are padded to 2048), and the rotation matrix is fully deterministic from a seed stored per index. The same seed means the same rotation on every machine, so indexes are portable.

Both stored vectors and incoming query vectors go through the same rotation before quantization and distance computation.

## Two-Tier Reranking

Quantized distance computations are approximate. During HNSW traversal, the greedy search might occasionally take a wrong turn because int8 distances are slightly different from f32 distances. At the margins, a few true top-K neighbors could be missed.

To prevent this from affecting final results, HEBBS now uses a two-tier search architecture:

```
Query
  |
  v
HNSW search (int8 distances, fetches 2x candidates)
  |
  v
Load original f32 vectors from RocksDB
  |
  v
Recompute exact distances (f32)
  |
  v
Final top-K results (exact ranking)
```

The HNSW search oversamples by 2x: if you request 10 results, it fetches 20 candidates using fast int8 distances. Then it loads the full-precision f32 vectors for those 20 candidates from RocksDB (on disk) and recomputes exact distances. The final ranking uses exact f32 scores.

This means quantization error only affects candidate selection, not final scores. A candidate that would have been ranked #11 with f32 distances might end up at #15 with int8, but since we are pulling 20 candidates for a top-10 request, it still makes the cut. The exact reranking then puts it back in the right position.

The RocksDB reads add a small amount of latency (disk I/O for 20 vectors), but these are point lookups by memory ID, which RocksDB handles efficiently from its block cache. In practice, the reranking overhead is well within the latency budget.

## What This Means for You

If you are running HEBBS in production, the upgrade is automatic. Quantization is enabled by default for new indexes. Existing indexes are quantized on the next full reindex (run `hebbs index --rebuild` to trigger it immediately).

The practical impact:

- **Smaller servers.** A deployment that previously needed 8 GB of RAM for 1M memories now fits in 3 GB. That is a meaningful cost reduction on cloud instances.
- **Higher density.** Same server, more memories. If you were running near your memory limit, you just got 3x more headroom.
- **Negligible quality change.** Our quality gates passed with less than 1% reduction in recall. The two-tier reranking ensures final scores are computed from full-precision vectors. For most workloads, the results are identical.

No configuration changes required. No new flags to set. The quantization parameters (int8, random rotation seed, reranking factor) are managed automatically per index.

## What's Next

This release covers int8 quantization (Phase A and B of our compression roadmap). Next on the roadmap: 4-bit quantization, which would push memory reduction to approximately 8x. That requires more careful quality benchmarking, so we are taking our time with it. Expect it in a future release once the quality gates are as clean as they are for int8.

For now, enjoy the extra headroom. Your servers will thank you.
