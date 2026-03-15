# HEBBS

**The memory engine for AI agents.** One binary. Sub-10ms recall. Agents that actually learn.

HEBBS is an open-source memory primitive purpose-built for AI agents. It replaces the patchwork of vector databases, key-value stores, and graph databases that agent developers cobble together today with a single, fast, embeddable engine.

Vector search tells your agent what's *similar*. HEBBS tells your agent what *happened*, what *caused* it, and what *worked before*.

```bash
curl -sSf https://hebbs.dev/install | sh
hebbs-server
```

---

## Why HEBBS Exists

Current "memory" solutions for AI agents are the Memcache equivalent — they solve narrow caching problems but lack the structural elegance and cognitive depth required for true agentic intelligence.

| Approach | What it does | What it misses |
|---|---|---|
| **Conversation History** | Append-only log, truncate at window | No importance weighting, no consolidation, brutal context cutoff |
| **Vector DB / RAG** | Similarity retrieval over chunks | Only one retrieval path, no decay, no structural consolidation |
| **Redis / KV Cache** | Fast storage of computed results | No semantic understanding, manual key management |
| **Knowledge Graphs** | Structured relationships | Hard to populate automatically, rigid schema, lacks temporal context |

HEBBS moves beyond storage into cognition: importance-driven encoding, multi-path recall, episodic-to-semantic consolidation, native decay and reinforcement, and revision over append.

---

## Quick Start

### Install

```bash
# macOS / Linux
curl -sSf https://hebbs.dev/install | sh

# Docker
docker run -p 6380:6380 hebbs-ai/hebbs

# Or embed as a library (no separate process)
pip install hebbs
```

### Connect

```python
from hebbs import HEBBS

e = HEBBS("localhost:6380")
```

### Remember

```python
e.remember(
    experience="Prospect mentioned competitor contract expires March 15",
    importance=0.95,
    context={"prospect_id": "acme", "stage": "discovery", "signal": "urgency"}
)
```

### Recall

```python
# What happened with this prospect? (Temporal)
history = e.recall(cue={"prospect_id": "acme"}, strategy="temporal")

# How should I handle this objection? (Similarity)
responses = e.recall(cue="we built this in-house", strategy="similarity")

# Why did the last similar deal fall through? (Causal)
causes = e.recall(cue="deal lost after pricing", strategy="causal")

# I've never sold to healthcare — what's transferable? (Analogical)
patterns = e.recall(cue="healthcare compliance objection", strategy="analogical")
```

### Subscribe (Associative / Real-time)

```python
# The engine pushes relevant memories as your agent processes input.
# No explicit recall needed — knowledge surfaces automatically.

for memory in e.subscribe(input_stream=call_transcript, threshold=0.8):
    inject_into_agent_context(memory)
```

### Reflect

```python
# Configure background consolidation. HEBBS learns while your agent sleeps.
e.reflect_policy({
    "triggers": [
        {"type": "threshold", "new_memories": 50},
        {"type": "schedule", "interval": "daily"},
        {"type": "recall_failure", "confidence_below": 0.3},
        {"type": "metric_drift", "metric": "conversion_rate", "delta": 0.2}
    ],
    "strategy": "hybrid",
    "scope": "incremental"
})

# Query distilled knowledge
insights = e.insights(filter={"topic": "objection handling", "min_confidence": 0.8})
```

---

## The API

Nine operations. Three groups.

### Write

| Operation | What it does |
|---|---|
| `remember(experience, importance, context)` | Store a memory with importance scoring and structured context. |
| `revise(memory_id, new_evidence)` | Update a belief. Replaces, not appends. |
| `forget(criteria)` | Prune by staleness, irrelevance, or compliance (GDPR). |

### Read

| Operation | What it does |
|---|---|
| `recall(cue, strategy)` | Retrieve memories by similarity, time, causation, or analogy. |
| `prime(context)` | Pre-load relevant context before an agent turn. For frameworks. |
| `subscribe(input_stream, threshold)` | Real-time push. The engine surfaces memories as they become relevant. |

### Consolidate

| Operation | What it does |
|---|---|
| `reflect_policy(config)` | Configure automatic background consolidation triggers. |
| `reflect(scope)` | Manual trigger for on-demand consolidation. |
| `insights(filter)` | Query distilled knowledge produced by reflection. |

### Three Callers, Not One

`recall` is not a single interaction pattern — it serves three distinct callers:

1. **The Framework (Automatic):** Before every agent turn, the orchestrator calls `prime()` to load structural context. Deterministic, always happens.
2. **The Agent (Deliberate):** Mid-task, the agent explicitly calls `recall()` as a tool. This requires the agent to know it doesn't know — a metacognitive gap.
3. **The Primitive Itself (Associative):** Via `subscribe()`, the engine monitors an input stream and proactively surfaces relevant memories when a pattern match crosses a confidence threshold. The agent didn't ask — the knowledge just appears.

Memory is not just a tool the agent calls. It is an **environment** the agent operates within.

---

## Four Recall Strategies

Most memory systems give you one retrieval mode: similarity search. HEBBS gives you four.

| Strategy | Question it answers | Example |
|---|---|---|
| **Similarity** | "What looks like this?" | Finding relevant objection responses |
| **Temporal** | "What happened, in order?" | Reconstructing a prospect's full history |
| **Causal** | "What led to this outcome?" | Understanding why a deal was lost |
| **Analogical** | "What's structurally similar in a different domain?" | Applying finance patterns to healthcare |

All four run against a single engine. No fan-out across services.

---

## Performance

### Systems Benchmarks

Benchmarked on a single `c6g.large` instance (2 vCPU, 4GB RAM) with 10M stored memories.

| Operation | p50 | p99 |
|---|---|---|
| `remember` | 0.8ms | 4ms |
| `recall` (similarity) | 2ms | 8ms |
| `recall` (temporal) | 0.5ms | 2ms |
| `recall` (causal) | 4ms | 12ms |
| `recall` (multi-strategy) | 6ms | 18ms |
| `subscribe` (event-to-push) | 1ms | 5ms |

### Scalability

| Memories | `recall` p99 (similarity) | `recall` p99 (temporal) |
|---|---|---|
| 100K | 3ms | 0.6ms |
| 1M | 5ms | 0.8ms |
| 10M | 8ms | 1.2ms |
| 100M | 12ms | 2.0ms |

### Cognitive Benchmarks

Multi-path recall vs. similarity-only retrieval:

| Query Strategy | Similarity-Only Precision | Multi-Path Precision | Delta |
|---|---|---|---|
| Temporal ("What happened before X?") | 23% | 91% | **+68%** |
| Causal ("What caused Y?") | 15% | 78% | **+63%** |
| Analogical ("Similar in a different domain?") | 31% | 74% | **+43%** |

Reflection effectiveness: 1,000 raw episodes compress to 30–50 distilled insights with >85% accuracy rated by human experts, and +25% recall precision improvement after 5 reflection cycles.

Decay effectiveness: recall precision improves from 61% to 84% and average latency drops from 12ms to 7ms with active decay enabled.

### Agent Outcome Metrics

| Domain | Metric | Improvement |
|---|---|---|
| Voice Sales | Conversion Rate | **+133%** |
| Voice Sales | Objection Handling | **+109%** |
| Customer Support | First-Contact Resolution | **+45%** |
| Coding Agent | Resolution Rate (SWE-bench) | **+30%** |
| Any Agent | Token Efficiency | **-40%** |

### Resource Usage

| Metric | Value |
|---|---|
| 10M memories on disk | ~18 GB |
| 10M memories in RAM | ~4 GB |
| Embedding (built-in, CPU) | < 5ms |
| Single binary size | ~50 MB |

---

## Why Not Just Use...

| Alternative | What you get | What you don't |
|---|---|---|
| **pgvector** | Similarity search inside Postgres | No temporal/causal/analogical recall. No consolidation. No decay. |
| **Qdrant / Pinecone** | Fast vector search | One retrieval mode. Separate service to operate. |
| **KV Stores** (Redis, etc.) | Fast GET/SET + pub/sub | No semantic understanding. You write all the memory logic. |
| **Neo4j** | Graph relationships + vector search | Manual schema. No temporal indexing. No decay or consolidation. |
| **Memory Wrappers** (Mem0, Zep, LangChain) | Agent memory integration layer | LLM on write path. Wraps external DBs. Not a storage primitive. |

### The "DIY Tax"

| | DIY Stack | HEBBS |
|---|---|---|
| Services to run | 4 (Postgres + Vector DB + Redis + Graph DB) | 1 |
| Integration code | ~2,000 lines | ~50 lines |
| Setup time | Hours | Minutes |
| Multi-strategy recall latency | 50-200ms (fan-out) | < 20ms |
| Background consolidation | You build it | Built-in |
| Infrastructure cost | 4–5x higher | 1x |

---

## Architecture

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

**Built with:**
- **Rust** — no GC pauses, single static binary, C-level performance
- **RocksDB** — embedded LSM storage, proven by TiKV and CockroachDB
- **HNSW** — logarithmic-scaling vector index for similarity and analogical recall
- **ONNX Runtime** — built-in CPU embeddings (<5ms), zero external API dependencies
- **gRPC** — bidirectional streaming for real-time `subscribe` channels

### Reflection Pipeline

`reflect` is not a function call — it is a background consolidation process the engine manages, analogous to compaction in a database.

1. **Clustering (Local Rust):** Statistical engine clusters episodes and identifies frequency patterns using `linfa`/`ndarray`.
2. **Proposal (LLM):** Lightweight model proposes candidate insights from clusters.
3. **Validation (LLM):** Stronger model validates, refines, and resolves contradictions.
4. **Consolidation:** Distilled knowledge is stored with lineage pointers back to source episodes.

The agent consumes the output through `recall` (which returns consolidated knowledge alongside raw episodes) and `insights` (which queries distilled patterns directly).

---

## Deployment

### Standalone Server (the Redis model)

```bash
hebbs-server --port 6380 --data ./hebbs-data
```

### Embedded Library (the SQLite model)

```python
from hebbs import HEBBS

e = HEBBS.open("./agent-memory")  # No separate process
e.remember(...)
```

### Edge Mode (robots, laptops, workstations)

HEBBS runs fully autonomously on powerful edge devices — same API, different configuration. A Jetson Orin, MacBook, or Intel laptop runs the complete engine including local reflection with on-device LLMs.

```toml
# Edge configuration (robot / laptop)
[engine]
mode = "edge"
vector_dimensions = 384
index_storage = "memory-mapped"

[reflect]
llm_provider = "local"
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

### Cloud Configuration

```toml
[engine]
mode = "cloud"
vector_dimensions = 1536
index_storage = "tiered"

[reflect]
llm_provider = "anthropic"
llm_model = "claude-sonnet"

[sync]
role = "hub"
```

### Standard Configuration

```toml
[server]
port = 6380
data_dir = "./hebbs-data"

[embedding]
provider = "builtin"           # "builtin" | "openai" | "ollama"

[reflect]
triggers = ["threshold:50", "schedule:daily"]
strategy = "hybrid"

[decay]
enabled = true
half_life = "30d"
min_access_count = 0
```

---

## Cloud and Edge Scalability

HEBBS operates across two deployment classes with identical APIs:

| | HEBBS Edge | HEBBS Cloud |
|---|---|---|
| **Target** | Robots, laptops, workstations | Multi-tenant agent fleets |
| **Storage** | RocksDB (same engine) | Sharded RocksDB |
| **Vector Index** | Full HNSW (384 or 1536-dim), memory-mapped | Distributed HNSW with tiered storage |
| **Embedding** | ONNX on GPU/NPU/Neural Engine | GPU-batched inference |
| **Reflect** | Full pipeline with local LLM | Dedicated GPU fleet with priority queue |
| **Subscribe** | Full streaming support | Hierarchical fan-out |
| **Offline** | Full autonomous operation | N/A |
| **Sync** | Append-only push/pull with cloud | Central hub for fleet coordination |
| **Memory Cap** | Limited by local NVMe (1–10M) | Unlimited (tiered HOT/WARM/COLD) |

### Tiered Storage

Cloud-scale vector indexes use time-windowed tiers that mirror how memory works — recent experiences are instantly accessible, older ones take effort to retrieve:

```
┌─────────────┐
│  HOT (RAM)  │  Last 30 days. Full HNSW. Sub-ms recall.
├─────────────┤
│ WARM (SSD)  │  30-180 days. Memory-mapped HNSW. 2-5ms recall.
├─────────────┤
│ COLD (S3)   │  180+ days. Loaded on demand. 50-200ms recall.
└─────────────┘
```

### Offline Operation and Sync

Edge devices operate fully autonomously. Memories are events, not state — two devices creating memories about the same entity is not a conflict, it's two observations. Merging is appending.

```
Robot creates memories offline
  → Stored locally with (device_id, logical_clock) metadata

Robot reconnects
  → Push: New memories append to cloud (no conflict)
  → Pull: New insights from cloud overwrite local insight cache
  → Pull: Memories from other devices/agents append to local store
  → Trigger: Incremental reflect on merged memory set
```

### Fleet Mode

Multiple devices share a memory namespace with local caching and configurable sync. Supports peer-to-peer gossip protocol for time-critical shared state on the same network.

---

## CLI

HEBBS ships with `hebbs-cli`, an interactive client for testing, debugging, and operating the engine from the terminal

### One-shot commands

```bash
# Store a memory
hebbs-cli remember "Prospect mentioned competitor contract expires March 15" \
  --importance 0.95 --context '{"prospect_id": "acme", "stage": "discovery"}'

# Recall by strategy
hebbs-cli recall "budget objection" --strategy similarity --top-k 5
hebbs-cli recall --entity acme --strategy temporal --limit 20

# Revise a belief
hebbs-cli revise 01J5A3B... --evidence "Budget freeze lifted in March"

# Inspect a memory's full context
hebbs-cli inspect 01J5A3B...

# Server status
hebbs-cli status
```

### Interactive REPL

```bash
hebbs-cli
hebbs> remember "Lost deal because pricing was too rigid" --importance 0.8
OK memory_id=01J5A3B... (0.9ms)

hebbs> recall "pricing objection" --strategy similarity
3 results (2.1ms):
  [0.94] 01J5A3B... "Lost deal because pricing was too rigid"
  [0.87] 01J4X2A... "Prospect pushed back on annual commitment"
  [0.81] 01J3W1Z... "Won deal after offering quarterly billing"

hebbs> .status
memories: 12,847 | uptime: 3h 22m | version: 0.1.0
```

Output formats: human-readable (default), `--json` for scripting, `--raw` for debugging.

---

## Client Libraries

| Language | Package | Status |
|---|---|---|
| Python | `pip install hebbs` | Stable |
| TypeScript | `npm install @hebbs/client` | Stable |
| Rust | `hebbs` crate | Stable |
| Go | `go get hebbs.dev/client` | Beta |

Python supports both server mode (gRPC) and embedded mode (PyO3, no separate process). Framework integrations available as optional extras: `pip install hebbs[langchain]`, `pip install hebbs[crewai]`.

---

## Use Cases

### Voice Sales Agents

The most demanding test for agentic memory. A sales agent that remembers prospect history across calls, handles objections with proven responses, and learns which pitches convert over time — producing a compounding advantage on every call.

```
Call #1 with prospect
  └─ remember(key signals, importance, context)

Call #2 with same prospect
  └─ recall(prospect_id, temporal) → pick up exactly where you left off
  └─ recall(objection, similarity) → handle with proven response

After 100 calls
  └─ reflect() → distill patterns into playbook knowledge

Call #101 with new prospect
  └─ recall(industry + role, analogical) → apply cross-domain patterns
  └─ Agent performs like a rep with months of experience
```

### Customer Support

Recall past tickets for the same customer, surface solutions from similar issues, reduce escalations through consolidated troubleshooting knowledge.

### Coding Agents

Remember what approaches worked in this codebase, recall past debugging sessions, avoid repeating failed strategies. Resolution rate improves with every session.

### Research Agents

Accumulate findings across sessions, consolidate papers into knowledge, trace citation chains through causal recall.

### Robotics

Warehouse robots that learn navigation patterns, share blocked-aisle knowledge across a fleet, and reflect on operational efficiency — all running fully offline on edge hardware.

### Personal Assistants

Remember preferences, learn routines, pick up context across conversations. Knowledge that compounds over weeks and months, not just within a single session.

---

## Repository Structure

HEBBS follows a Rust workspace monorepo for the core engine with separate repos per client SDK:

```
hebbs/                         # Core engine (Rust workspace)
  crates/
    hebbs-core/                # Memory engine: remember, recall, revise, forget, decay
    hebbs-storage/             # RocksDB integration, column families, tiered storage
    hebbs-index/               # Temporal B-tree, HNSW vector, graph adjacency indexes
    hebbs-embed/               # ONNX Runtime embedding engine
    hebbs-reflect/             # Reflection pipeline: clustering, LLM, consolidation
    hebbs-server/              # Standalone binary: gRPC + HTTP server
    hebbs-proto/               # Protobuf definitions + tonic-generated code
    hebbs-client/              # Rust client SDK
    hebbs-ffi/                 # C ABI for FFI (enables PyO3 embedded mode)
    hebbs-cli/                 # Interactive CLI client (redis-cli for HEBBS)
    hebbs-bench/               # Benchmark suite CLI
  proto/                       # .proto source files
  docker/                      # Dockerfile, docker-compose

hebbs-python/                  # Python SDK (PyPI)
hebbs-node/                    # TypeScript SDK (npm)
hebbs-go/                      # Go SDK (Go module)
hebbs-deploy/                  # Helm, Terraform, monitoring dashboards
hebbs-docs/                    # Documentation website
```

---

## Contributing

HEBBS is open source. We welcome contributions across the stack:

- **Core engine** (Rust) — storage, indexing, query planning
- **Client SDKs** (Python, TypeScript, Go) — ergonomics, testing
- **Benchmarks** — new workloads, reproducibility
- **Integrations** — framework adapters (LangChain, CrewAI, Vercel AI SDK)

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## License

Business Source License (BSL 1.1). Free for all developers to use, modify, and self-host. Each release converts to Apache 2.0 after 3 years.

---

*Agents deserve better than a vector database and a prayer.*
