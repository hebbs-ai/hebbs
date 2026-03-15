# Technology Stack: Building the "Redis for Agent Memory" (HEBBS)

## Design Principles

For a primitive to become an industry standard, every architectural choice must serve three core principles:
1.  **Extreme Performance:** Hot-path operations (`recall`, `subscribe`) must have sub-10ms latency to support real-time agents (voice, robotics).
2.  **Simplicity of Deployment:** No complex distributed systems setup. Ships as a single static binary with zero external database dependencies for the core engine.
3.  **Predictable Resource Usage:** No garbage collection pauses (critical for real-time streams) and tunable memory/storage footprint.

---

## The Core Stack

### 1. Language: Rust
Every successful modern infrastructure primitive (Qdrant, Meilisearch, TiKV, Turso) has converged on Rust. 
- **Performance:** C-level speed with zero-cost abstractions.
- **Safety:** Memory safety without a garbage collector (eliminating "stop the world" pauses).
- **Concurrency:** Fearless concurrency for simultaneous read/write/subscribe operations.
- **Portability:** Compiles to a single static binary; perfect for embedded and standalone modes.

### 2. Storage Engine: RocksDB (Embedded)
Rather than gluing together separate databases, we build on a foundational storage engine:
- **LSM-tree Architecture:** High write throughput (optimized for `remember` calls).
- **Embedded:** No separate database process; ships inside the core binary.
- **Tunable Compaction:** Native support for background data maintenance (the foundation for the `reflect` policy).
- **Mature Ecosystem:** Proven by TiKV, CockroachDB, and Yugabyte.

### 3. Index Layer: Three Engines, One Store
We implement three specialized index structures on top of the RocksDB key-space using column families:

| Index | Recall Strategy | Implementation |
|---|---|---|
| **Temporal Index** | Temporal | B-tree on `(entity_id, timestamp)` key prefix. |
| **Vector Index** | Similarity, Analogical | HNSW (Hierarchical Navigable Small Worlds) stored natively. |
| **Graph Index** | Causal, Relational | Adjacency lists for causal chain and entity relationship traversal. |

### 4. Embedding Engine: Built-in ONNX Runtime
To eliminate network round-trips and provide a "zero-config" experience, the primitive generates embeddings locally.
- **Built-in Default:** Ships with a small, high-quality model (e.g., BGE-small-en) running via ONNX.
- **Latency:** <5ms embedding generation on CPU.
- **Pluggable:** Optional support for external providers (OpenAI, Cohere) or local GPU models (Ollama).

### 5. Network Protocol: gRPC + HTTP/REST
- **gRPC (Primary):** Strongly typed via Protobuf, low overhead, and native support for bidirectional streaming (required for `subscribe()` associative recall).
- **HTTP/REST (Secondary):** For debugging, quick integration, and standard web compatibility.

---

## Reflection Pipeline (Hybrid Architecture)

`reflect` is the one operation that intelligence beyond what local code can provide. It uses a tiered approach:

1.  **Clustering (Local):** Pure-Rust spherical K-means with silhouette-guided k selection in `hebbs-reflect`. No external ML dependencies -- uses cosine distance on L2-normalized embeddings with K-means++ initialization.
2.  **Proposal (External LLM):** Blocking HTTP call via `ureq` to produce candidate insights from each cluster. `LlmProvider` trait with Mock/Anthropic/OpenAI/Ollama implementations.
3.  **Validation (External LLM):** Second LLM call validates candidates against source memories and existing insights. Detects contradictions, duplication, and factual errors.
4.  **Consolidation:** The engine writes distilled insights back as `MemoryKind::Insight` with `InsightFrom` graph edges linking to source episodes (lineage invariant).

---

## Deployment Modes

- **Standalone Server:** Single binary (the Redis model). `curl | sh` and run.
- **Embedded Library:** Linked directly into Rust, Python (via PyO3), or Go applications (the SQLite model).
- **Edge Mode:** Optimized for low-resource environments with local ONNX embedding.

---

## Architecture Diagram

```text
┌──────────────────────────────────────────────────────┐
│                    Client SDKs                       │
│            Python  │  TypeScript  │  Rust            │
├──────────────────────────────────────────────────────┤
│               gRPC  │  HTTP/REST                     │
├──────────────────────────────────────────────────────┤
│                                                      │
│                 Core Engine (Rust)                    │
│                                                      │
│  ┌────────────┐ ┌────────────┐ ┌──────────────────┐ │
│  │  Remember   │ │   Recall   │ │ Reflect Pipeline │ │
│  │  Engine     │ │   Engine   │ │ (background)     │ │
│  │            │ │            │ │                  │ │
│  │ • encode   │ │ • prime    │ │ • cluster (Rust) │ │
│  │ • score    │ │ • query    │ │ • propose (LLM)  │ │
│  │ • index    │ │ • subscribe│ │ • validate (LLM) │ │
│  │ • decay    │ │ • merge    │ │ • store insights │ │
│  └─────┬──────┘ └─────┬──────┘ └────────┬─────────┘ │
│        │              │                 │            │
│  ┌─────┴──────────────┴─────────────────┴─────────┐ │
│  │              Index Layer                        │ │
│  │   Temporal (B-tree) │ Vector (HNSW) │ Graph    │ │
│  └──────────────────────┬──────────────────────────┘ │
│                         │                            │
│  ┌──────────────────────┴──────────────────────────┐ │
│  │         Storage Engine (RocksDB)                 │ │
│  │         Column Families per index type           │ │
│  └─────────────────────────────────────────────────┘ │
│                                                      │
│  ┌─────────────────────┐  ┌────────────────────────┐ │
│  │ Embedding Engine    │  │ LLM Provider Interface │ │
│  │ (ONNX Runtime,      │  │ (Anthropic, OpenAI,    │ │
│  │  built-in default)  │  │  Ollama — pluggable)   │ │
│  └─────────────────────┘  └────────────────────────┘ │
└──────────────────────────────────────────────────────┘
```

---

## Summary of Tech Choices

| Component | Choice | Rationale |
|---|---|---|
| **Language** | **Rust** | Performance, safety, single-binary, no GC. |
| **Storage** | **RocksDB** | High write throughput, embedded, proven stability. |
| **Protocol** | **gRPC** | Bidirectional streaming for real-time associative recall. |
| **Embedding**| **ONNX** | Sub-5ms local embedding, zero network dependency. |
| **ML Engine** | **Hybrid** | Statistical clustering in Rust + Pluggable LLM for insights. |
