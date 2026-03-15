# Scalability Architecture: HEBBS Across Cloud and Edge

## The Scale Spectrum

HEBBS must operate across two deployment classes — not a gradient from weak to strong, but two distinct **operating environments** with different constraints:

```
   Powerful Edge (Robot / Laptop)              Multi-Tenant Cloud
   ──────────────────────────────              ──────────────────
   8-32GB RAM, GPU/NPU, NVMe SSD              Distributed fleet
   Single agent, single owner                 Thousands of agents, multi-tenant
   Intermittent or no connectivity             Always online
   Must operate autonomously                   Central coordination
   Local reflection capability                 Dedicated reflection infrastructure
   100K - 10M memories                         10B+ memories across tenants
```

The API is identical across both. `hebbs.remember()` and `hebbs.recall()` work the same whether the agent is running on a warehouse robot or a 1000-agent cloud fleet. The internals adapt.

---

## Cloud-Scale Bottlenecks

### 1. HNSW Vector Index Growth

**The Problem:** HNSW indexes live in memory. At 100M+ memories with 1536-dim float32 vectors, the index consumes ~600GB RAM. No single machine holds this.

**The Solution: Tiered Storage + Tenant Sharding**

- **Tenant-level partitioning.** Each tenant gets its own index. A tenant with 1M memories has a ~60MB index — easily fits on a single node. Horizontal scaling is just adding nodes and distributing tenants.
- **Time-windowed tiers within a tenant:**

```
┌─────────────┐
│  HOT (RAM)  │  Last 30 days. Full HNSW. Sub-ms recall.
├─────────────┤
│ WARM (SSD)  │  30-180 days. Memory-mapped HNSW. 2-5ms recall.
├─────────────┤
│ COLD (S3)   │  180+ days. Loaded on demand. 50-200ms recall.
└─────────────┘
```

- **Product Quantization (PQ).** Compress vectors from 6KB (float32, 1536-dim) to ~128 bytes. 48x reduction with <3% recall quality loss. Makes warm-tier SSD indexes viable.

This mirrors how memory works: recent experiences are instantly accessible, older ones take effort to retrieve.

### 2. Write Throughput at Scale

**The Problem:** At 1M+ `remember` calls/sec across thousands of agents, every write triggers embedding generation + three index updates (temporal, vector, graph) + durability fsync. RocksDB compaction becomes the bottleneck.

**The Solution: Decoupled Write Pipeline**

```
remember()
  → Write-Ahead Log (immediate ack, <1ms)
  → Background pipeline: embed → index temporal → index vector → index graph
```

- **Acknowledge immediately** after WAL persistence. The caller gets sub-millisecond response. Indexing is asynchronous.
- **Batch embedding.** Instead of one-at-a-time, batch 64-128 memories through ONNX/GPU together. 10-50x more efficient.
- **Separate write and read paths.** Writers append to WAL. Background workers build indexes. Readers query indexes. No write-read contention.

### 3. Reflect Pipeline Contention

**The Problem:** 10,000 tenants all trigger `reflect` at midnight UTC. Each cycle processes hundreds of memories through clustering and LLM inference. The inference layer saturates.

**The Solution: Queued Reflection with Staggered Scheduling**

- **Hash-based staggering.** Spread daily triggers across a 6-hour window using tenant ID hash. No thundering herd.
- **Priority queue.** Enterprise tier gets dedicated GPU capacity. Pro tier uses shared pool. Free tier is best-effort.
- **Incremental only.** Each reflection run processes only memories created since the last run. Bounds per-tenant cost regardless of total memory count.

### 4. Subscribe Fan-Out

**The Problem:** 10,000 agents with active `subscribe` streams. Every input token must be pattern-matched against the agent's memory store in real-time.

**The Solution: Hierarchical Filtering**

```
Input arrives
  → Stage 1: Bloom filter (microseconds) — relevant to ANY memory?
  → Stage 2: Coarse embedding match (sub-ms) — which clusters are candidates?
  → Stage 3: Fine HNSW search (few ms) — exact top-K matches
  → Push if confidence > threshold
```

The bloom filter eliminates 90%+ of inputs before any expensive computation. For the rest, coarse-to-fine keeps latency bounded.

### 5. Causal Graph Depth

**The Problem:** Causal chains can grow deep. Traversing hundreds of nodes in the graph index is expensive at scale.

**The Solution: Pre-computed Summaries + Bounded Traversal**

- **Bounded depth.** Default max of 10 hops. Most causal queries resolve within 3-5.
- **Materialized causal summaries.** During `reflect`, pre-compute and cache common chains. "No ROI → pricing objection → deal lost" becomes a single indexed insight, not a live graph traversal.
- **Entity-scoped subgraphs.** Each prospect/project gets its own causal partition. Cross-entity queries are rare and tolerate higher latency.

---

## Edge Bottlenecks (Robots, Laptops, Workstations)

These are **powerful machines** — the constraint isn't compute, it's **autonomy and connectivity.** A robot on a factory floor has a Jetson Orin (16GB RAM, GPU). A developer's laptop has 32GB RAM and an M-series chip. HEBBS must run fully independently on these devices, with optional cloud sync.

### 1. Local Reflection Without Cloud LLM Access

**The Problem:** The reflection pipeline's stages 2-3 (insight proposal and validation) require LLM inference. On a robot in a warehouse or a laptop on a plane, there's no API to call.

**The Solution: On-Device LLMs + Tiered Reflection**

In 2026, powerful edge devices run capable local models. A Jetson Orin runs Phi-3 or Gemma-2B. A MacBook runs Llama-3-8B via `llama.cpp` or MLX.

```
On-device reflection pipeline:
  Stage 1: Statistical clustering (pure Rust, always available)
  Stage 2: Local LLM proposes insights (Phi-3 / Gemma-2B / Llama-3-8B)
  Stage 3: Local LLM validates and refines

When cloud is available:
  Stage 4 (optional): Cloud LLM re-validates with stronger model
  → Upgraded insights sync back to device
```

The device is **fully autonomous.** It can reflect, consolidate, and learn entirely offline. Cloud connectivity upgrades the quality of reflection but is never required.

This is the key architectural difference from the "phone" model. A phone can't reflect locally. A robot or laptop can.

### 2. Embedding Generation Latency

**The Problem:** During a live interaction (robot processing speech, laptop agent handling a task), embedding generation adds latency to `remember` and `recall`.

**The Solution: Hardware Acceleration is Already There**

| Device | Accelerator | Embedding Latency (384-dim model) |
|---|---|---|
| MacBook M3 | Neural Engine | ~1ms |
| Jetson Orin | GPU (CUDA) | ~2ms |
| Intel Laptop | NPU (Meteor Lake+) | ~3ms |
| Any device | CPU fallback | ~8ms |

These are fast enough for real-time operation. No architectural workaround needed — just hardware-aware ONNX Runtime configuration. HEBBS auto-detects available accelerators at startup.

For higher-quality 1536-dim models, latency increases to 5-15ms on edge hardware — still acceptable for most use cases.

### 3. Index Size on Single Machine

**The Problem:** A robot accumulates memories over months/years of operation. At 10M memories with full-dimension vectors, the HNSW index is ~60GB. That may exceed available RAM on a 16GB device.

**The Solution: Adaptive Index Strategy**

- **Use 384-dim vectors on edge by default.** 10M memories = ~3.8GB index. Fits comfortably.
- **Memory-mapped HNSW.** The index lives on NVMe SSD and is memory-mapped. The OS pages hot portions into RAM automatically. Recall latency increases from 2ms to 5-8ms but the index can be arbitrarily large.
- **Active decay.** Edge devices run decay more aggressively. A robot doesn't need 3-year-old memories about a shelf layout that has been reorganized 50 times. Keep the working set sharp.
- **Cloud archival.** When connected, push cold memories to cloud storage. The device keeps a compact, high-quality working set. Full history is accessible via cloud recall when needed.

### 4. Offline Operation & Sync (The Hard Problem)

**The Problem:** A robot creates 500 memories during an 8-hour shift with no connectivity. Meanwhile, the cloud (or another robot) has also created memories about the same environment. When the robot reconnects, these must merge coherently.

**The Solution: Append-Only Sync with Cloud-Authoritative Insights**

The core insight: **memories are events, not state.** Two devices creating memories about the same entity isn't a conflict — they are two observations. Merging is appending.

```
Robot creates memories offline
  → Stored locally with (device_id, logical_clock) metadata

Robot reconnects
  → Push: New memories append to cloud (no conflict possible)
  → Pull: New insights from cloud overwrite local insight cache
  → Pull: Memories from other devices/agents append to local store
  → Trigger: Incremental reflect on merged memory set
```

**Where conflicts CAN arise:**

- `revise()` — both device and cloud revised the same memory. Resolution: **higher-importance evidence wins.** Ties broken by logical clock.
- `forget()` — cloud deleted a memory the device still references. Resolution: **forget is authoritative.** If cloud says forget, device forgets.
- `reflect` insights — cloud produced an insight, then device adds memories that invalidate it. Resolution: **re-reflect incrementally.** The cloud tracks which memories fed which insights (lineage). New memories trigger targeted re-evaluation.

### 5. Multi-Device Coherence (Fleet of Robots)

**The Problem:** A fleet of 50 warehouse robots all operating in the same environment. Each has its own HEBBS instance. Robot #12 learns that Aisle 7 is blocked. How does Robot #23 know?

**The Solution: Shared Memory Namespace with Local Caching**

```
Each robot has:
  ├── Local namespace (private memories, device-specific)
  └── Shared namespace (fleet-wide knowledge, synced)

When Robot #12 remembers "Aisle 7 blocked":
  → Stored in shared namespace
  → Synced to cloud hub on next connection
  → Cloud pushes to all other robots on their next sync

Sync frequency: configurable (every 30s when connected, batch when reconnecting)
```

The shared namespace is eventually consistent. For time-critical shared state (like "aisle blocked"), robots can use a lightweight gossip protocol for direct peer-to-peer sync when on the same network, bypassing the cloud entirely.

---

## The Two-Tier Architecture

| | HEBBS Edge | HEBBS Cloud |
|---|---|---|
| **Target** | Robots, laptops, workstations | Multi-tenant agent fleets |
| **Storage** | RocksDB (same engine) | Sharded RocksDB |
| **Vector Index** | Full HNSW (384 or 1536-dim), memory-mapped | Distributed HNSW with tiered storage |
| **Embedding** | ONNX on GPU/NPU/Neural Engine | GPU-batched inference |
| **Reflect** | Full pipeline with local LLM (Phi-3 / Llama-3-8B) | Dedicated GPU fleet with priority queue |
| **Subscribe** | Full streaming support | Hierarchical fan-out |
| **Offline** | Full autonomous operation | N/A |
| **Sync** | Append-only push/pull with cloud | Central hub for fleet coordination |
| **Memory Cap** | Limited by local NVMe (practically 1-10M) | Unlimited (tiered) |
| **Binary Size** | ~50MB (same binary, edge config) | Cluster deployment |

**Same binary. Same API. Different configuration.**

```toml
# Edge configuration (robot)
[engine]
mode = "edge"
vector_dimensions = 384
index_storage = "memory-mapped"

[reflect]
llm_provider = "local"          # Uses on-device Phi-3 / Llama
llm_model = "phi-3-mini"

[sync]
enabled = true
hub = "wss://fleet.example.com/hebbs"
interval = "30s"
namespace = "warehouse-fleet-01"

[decay]
enabled = true
half_life = "14d"
max_memories = 5_000_000
```

```toml
# Cloud configuration
[engine]
mode = "cloud"
vector_dimensions = 1536
index_storage = "tiered"

[reflect]
llm_provider = "anthropic"
llm_model = "claude-sonnet"

[sync]
role = "hub"                     # Accepts connections from edge devices
```

---

## The Single Hardest Unsolved Problem

**Causal consistency of reflected insights across sync boundaries.**

1. Cloud reflects on memories A, B, C and produces Insight X.
2. Robot (offline) creates memories D, E, F that contradict memory B.
3. Robot reconnects and syncs D, E, F to cloud.
4. Insight X is now potentially invalid — but which part? Does it need full re-reflection or targeted revision?

This requires **lineage tracking** in the reflection pipeline: every insight must record which source memories contributed to it, so that when new evidence arrives, the system knows exactly which insights to re-evaluate.

```
Insight X:
  sources: [memory_A, memory_B, memory_C]
  confidence: 0.87
  
New memory D contradicts memory_B
  → Insight X flagged for re-evaluation
  → Incremental re-reflect on {A, B, C, D} only
  → Insight X either strengthened, revised, or invalidated
```

No existing system does this. Getting it right is the moat that makes HEBBS irreplaceable once adopted.
