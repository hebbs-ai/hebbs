<p align="center">
  <img src="assets/logo-icon.png" alt="HEBBS logo" width="128" height="128" />
</p>

# HEBBS

**The memory engine for AI agents.** Four recall strategies. Native consolidation. Automatic decay. One binary.

> **Note:** This is the open-source version of HEBBS. We are currently focused on our enterprise rollouts and the latest research may not be reflected here. This version is fully functional. For enterprise deployments, managed hosting, or to learn more, please reach out to us at [hebbs.ai](https://hebbs.ai).

HEBBS is a cognitive memory primitive purpose-built for AI agents. Vector search tells your agent what's *similar*. HEBBS tells your agent what *happened*, what *caused* it, and what *worked before*.

```
4 recall strategies · Native consolidation · Automatic decay · One skill file, zero config
```

```bash
brew install hebbs-ai/tap/hebbs
```

Or on any platform:

```bash
curl -sSf https://hebbs.ai/install | sh
```

---

## Portable Cognition

`.hebbs/` is a self-contained cognition layer that lives next to your files. Build the index once, then drop it anywhere: another machine, another agent, your whole team. Everyone gets the same memory instantly.

`.hebbsignore` works like `.gitignore`. Your private files stay private. Your agents only see what you allow.

Your files are the source of truth. The `.hebbs/` directory is derived, rebuildable, and disposable. Delete it and run `hebbs init . && hebbs index .` to get it back.

---

## Works Out of the Box with Your Agent

HEBBS ships as a **skill** for Claude Code and OpenClaw. No SDK integration. No glue code. No configuration. Install HEBBS, and your agent automatically stores memories, recalls with the right strategy, consolidates insights, and forgets what's stale.

```
Without HEBBS:                          With HEBBS:

1. Choose a vector DB                   brew install hebbs-ai/tap/hebbs
2. Set up embedding pipeline
3. Write storage layer                  Done.
4. Write retrieval layer
5. Add temporal logic                   Your agent now has:
6. Add graph traversal                  - 4 recall strategies
7. Wire it all together                 - Temporal + causal + analogical
8. Handle decay manually                - Native decay & reinforcement
9. Build consolidation pipeline         - Automatic consolidation
10. Maintain 4 services                 - Works with Claude & OpenClaw

~2,000 lines of glue                    0 lines of glue
```

The skill is published at [hebbs-ai/hebbs-skill](https://github.com/hebbs-ai/hebbs-skill) and works with any agent that reads SKILL.md.

---

## Why HEBBS Exists

Every agent framework gives you similarity search and calls it memory. HEBBS gives your agent temporal reasoning, causal chains, analogical transfer, consolidation, and decay: the cognitive operations that turn retrieval into understanding.

| What your agent's memory does today | What HEBBS does |
|---|---|
| Embed a question, find 5 nearest vectors | **"What happened before this?"** Temporal recall |
| Return them and hope for the best | **"What caused this outcome?"** Causal graph walk |
| Precision on temporal queries: ~23% | **"What pattern transfers here?"** Analogical matching |
| No decay, no consolidation, no revision | Memories decay. Important ones strengthen. Episodes consolidate into insights. |

The delta isn't milliseconds. It's **+68 percentage points on temporal queries** and **+63 on causal**.

---

## Four Recall Strategies

Everyone else has one. HEBBS has four.

| Strategy | Question it answers | Example |
|---|---|---|
| **Similarity** | "What looks like this?" | Finding relevant objection responses |
| **Temporal** | "What happened, in order?" | Reconstructing a prospect's full history |
| **Causal** | "What led to this outcome?" | Understanding why a deal was lost |
| **Analogical** | "What's structurally similar in a different domain?" | Applying finance patterns to healthcare |

All four run against a single engine. No fan-out across services.

### Tunable Scoring

Every result is ranked by a composite score blending four signals:

| Signal | What it captures | Default weight |
|---|---|---|
| **Relevance** | Semantic similarity to the query | 0.50 |
| **Recency** | How recently the memory was created | 0.20 |
| **Importance** | Salience set at encoding time | 0.20 |
| **Reinforcement** | How often the memory has been recalled | 0.10 |

One parameter changes everything:
- `1:0:0:0` pure semantic (RAG mode)
- `0.2:0.8:0:0` favor recent (live context mode)
- `0.3:0.1:0.5:0.1` favor important (critical decisions mode)

---

## Your Agent Learns, Not Just Stores

The `reflect` pipeline clusters raw memories, proposes insights, validates them, and stores consolidated knowledge with full lineage.

```
Raw memories (episodes):
  "Customer asked about pricing"
  "Customer mentioned competitor X"
  "Customer objected to annual commitment"
  "Deal lost to competitor X"

          | reflect (automatic consolidation)

Insight (with lineage):
  "Deals mentioning competitor X with pricing objections
   have 73% loss rate when annual commitment is pushed early"
   [confidence: 0.84, sources: 4 memories, tags: sales, pricing]
```

---

## Your Agent Tunes Itself

HEBBS exposes tunable parameters that no other memory system does: four strategies, four scoring weights, adjustable k, cue expansion. Your agent uses these to **learn how to retrieve better** and stores that knowledge back into HEBBS.

```
Session 1: Agent runs eval queries against the vault
           Baseline: 54% keyword recall (default settings)

           Agent analyzes failures:
           - k too low → increase to 10
           - cue too generic → expand with entity names
           - wrong strategy → switch to temporal for timeline queries

           After tuning: 84% keyword recall

           Agent stores what worked:
           hebbs remember "RETRIEVAL-INSTRUCTION: For compliance queries,
           include entity names and expand acronyms. Use k=10."
           --importance 0.9 --entity-id retrieval-instructions

Session 2: Agent loads stored strategies at conversation start
           Applies learned tuning automatically
           Retrieval is better from the first query
```

This is the loop that no competitor can run. Vector databases give you one knob (top-k). HEBBS gives you four strategies, four scoring weights, entity scoping, and cue construction. Your agent optimizes all of them, measures the improvement, and remembers what worked.

Tested results: 54% to 84% with local embeddings, 75% to 90% with OpenAI embeddings.

---

## Quick Start

### Quick Start (Local)

```bash
# Initialize with OpenAI (recommended)
hebbs init . --provider openai --key $OPENAI_API_KEY

# Or with other providers
hebbs init . --provider anthropic --key $ANTHROPIC_API_KEY
hebbs init . --provider ollama

hebbs index .                         # index your markdown files
hebbs recall "your question here"     # recall with any strategy
hebbs panel                           # open the Memory Palace
```

One command configures both LLM and embeddings. `--model` is optional (defaults per provider). When using OpenAI, embedding auto-configures to `text-embedding-3-small` with the same key. No local model download needed. Other providers default to a local ONNX model (embeddinggemma-300m, ~600MB, downloaded once).

### Controlling What Gets Indexed

By default HEBBS indexes every `.md` file in your vault, skipping `.git/`, `.obsidian/`, `node_modules/`, and `.hebbs/`. To exclude additional files or directories, create a `.hebbsignore` file at the vault root:

```
# .hebbsignore (same syntax as .gitignore)
templates/
drafts/*.md
archive/
*.template.md
```

Patterns from `.hebbsignore` are merged with the built-in defaults and any patterns in `.hebbs/config.toml`. Comments (`#`) and blank lines are supported. The daemon picks up changes to `.hebbsignore` automatically on its next config reload. No restart needed.

See [docs/hebbsignore.md](../docs/hebbsignore.md) for the full reference.

### Start a Server (Optional, for teams)

```bash
hebbs start                           # gRPC :6380, HTTP :6381
hebbs remember "hello world"          # store a memory (uses server via --endpoint)
hebbs recall "hello"                  # recall it
```

### Connect from Python

```bash
pip install hebbs
```

```python
from hebbs import HebbsClient

client = HebbsClient("localhost:6380")

await client.remember(
    content="Prospect mentioned competitor contract expires March 15",
    importance=0.95,
    entity_id="acme",
    context={"stage": "discovery", "signal": "urgency"},
)

# Four recall strategies
history = await client.recall(cue="acme engagement", strategy="temporal", entity_id="acme")
responses = await client.recall(cue="we built this in-house", strategy="similarity")
causes = await client.recall(cue="deal lost after pricing", strategy="causal")
patterns = await client.recall(cue="healthcare compliance objection", strategy="analogical")

# Consolidate and query insights
result = await client.reflect()
insights = await client.insights(entity_id="acme", max_results=10)
```

### Connect from TypeScript

```bash
npm install @hebbs/sdk
```

```typescript
import { HebbsClient } from '@hebbs/sdk';

const client = new HebbsClient('localhost:6380', { apiKey: process.env.HEBBS_API_KEY });
await client.connect();

await client.remember({
    content: 'Prospect mentioned competitor contract expires March 15',
    importance: 0.95,
    entityId: 'acme',
    context: { stage: 'discovery', signal: 'urgency' },
});

// Four recall strategies
const history = await client.recall({ cue: 'acme engagement', strategies: ['temporal'], entityId: 'acme' });
const causes = await client.recall({ cue: 'deal lost after pricing', strategies: ['causal'] });
const patterns = await client.recall({ cue: 'healthcare compliance objection', strategies: ['analogical'] });

// Consolidate and query insights
const result = await client.reflect();
const insights = await client.insights({ entityId: 'acme', maxResults: 10 });
```

### Reference Demos

The [hebbs-python](https://github.com/hebbs-ai/hebbs-python) repo includes a full AI Sales Intelligence Agent demo with 7 scripted scenarios, 5 LLM providers, and Rich terminal panels.

```bash
pip install hebbs[demo]
hebbs-demo interactive --config gemini-vertex --verbosity verbose
```

The [hebbs-typescript](https://github.com/hebbs-ai/hebbs-typescript) repo includes an equivalent TypeScript demo with 3 scenarios and an interactive mode.

```bash
cd hebbs-typescript/demo && npm install
npx tsx src/index.ts interactive --mock-llm
```

---

## The API

Nine operations. Three groups. Each one is a cognitive primitive that didn't exist as a single call before.

### Write

| Operation | What it does | Why it matters |
|---|---|---|
| `remember()` | Store with importance scoring | Not append-only. Every memory is weighted at birth. |
| `revise()` | Update beliefs, keep lineage | Your agent corrects itself. No contradictory facts coexisting. |
| `forget()` | Prune by staleness, compliance | Real deletion. GDPR-proof. Signal-to-noise improves over time. |

### Read

| Operation | What it does | Why it matters |
|---|---|---|
| `recall()` | 4 strategies, composite scoring | Not just "find similar": find relevant, recent, causal, analogical. |
| `prime()` | Pre-load context | Start of conversation = agent already knows what matters. |
| `subscribe()` | Real-time push | Memories surface automatically when they become relevant. |

### Consolidate

| Operation | What it does | Why it matters |
|---|---|---|
| `reflect()` | Consolidate episodes into insights | Your agent learns patterns, not just stores facts. |
| `insights()` | Query consolidated knowledge | Higher-order understanding, not raw retrieval. |

---

## Client Libraries

| Language | Package | Repo | Status |
|---|---|---|---|
| Python | `pip install hebbs` | [hebbs-ai/hebbs-python](https://github.com/hebbs-ai/hebbs-python) | Alpha (gRPC + embedded via PyO3) |
| TypeScript | `npm install @hebbs/sdk` | [hebbs-ai/hebbs-typescript](https://github.com/hebbs-ai/hebbs-typescript) | Alpha (gRPC, Node.js 18+) |
| Rust | `hebbs` crate (direct) | This repo | Stable |
| Agent Skill | SKILL.md | [hebbs-ai/hebbs-skill](https://github.com/hebbs-ai/hebbs-skill) | Stable (Claude Code, OpenClaw) |

---

## Scoping: Entities and Tenants

HEBBS has two scoping dimensions.

**`entity_id`**: what the memory is about (a customer, project, user). Optional. Scope recall, prime, and forget to a subject.

**`tenant_id`**: who owns the data (an org, workspace). Structural isolation: storage keys are prefixed, index traversal is partitioned, cross-tenant queries are impossible.

```bash
hebbs --tenant acme-corp remember "Q2 forecast looks strong" --entity-id project-alpha
```

```python
client = HebbsClient("localhost:6380", tenant_id="acme-corp")
```

```typescript
const client = new HebbsClient('localhost:6380', { tenantId: 'acme-corp' });
```

---

## Comparison

| | pgvector | Qdrant | Neo4j | Memory Wrappers | **HEBBS** |
|---|---|---|---|---|---|
| Recall strategies | 1 | 1 | 1-2 | 1-2 | **4** |
| Temporal recall | No | No | No | No | **Native** |
| Causal reasoning | No | No | Partial | No | **Native** |
| Analogical transfer | No | No | No | No | **Native** |
| Native decay | No | No | No | No | **Yes** |
| Consolidation | No | No | No | Partial | **Native** |
| Revision with lineage | No | No | No | No | **Yes** |
| Agent skill (drop-in) | No | No | No | No | **Yes** |
| LLM calls on hot path | N/A | N/A | N/A | Yes | **Zero** |
| Recall latency (10M) | ~20ms | ~10ms | ~50ms | 50-200ms | **<10ms** |
| Runtime dependencies | Postgres | Qdrant | JVM + Neo4j | 3-4 services | **None** |

---

## Performance

And it does all of this in under 10ms. Benchmarked on a single `c6g.large` instance (2 vCPU, 4GB RAM) with 10M stored memories.

| Operation | p50 | p99 |
|---|---|---|
| `remember` | 0.8ms | 4ms |
| `recall` (similarity) | 2ms | 8ms |
| `recall` (temporal) | 0.5ms | 2ms |
| `recall` (causal) | 4ms | 12ms |
| `recall` (multi-strategy) | 6ms | 18ms |
| `subscribe` (event-to-push) | 1ms | 5ms |

<details>
<summary>Scalability</summary>

| Memories | `recall` p99 (similarity) | `recall` p99 (temporal) |
|---|---|---|
| 100K | 3ms | 0.6ms |
| 1M | 5ms | 0.8ms |
| 10M | 8ms | 1.2ms |
| 100M | 12ms | 2.0ms |

</details>

---

## Architecture

```text
──────────────────────────────────────────────────────────
                     Client SDKs
             Python  |  TypeScript  |  Rust
──────────────────────────────────────────────────────────
               Agent Skills (SKILL.md)
            Claude Code  |  OpenClaw
──────────────────────────────────────────────────────────
                gRPC  |  HTTP/REST
──────────────────────────────────────────────────────────

                  Core Engine (Rust)

  +------------+ +------------+ +------------------+
  |  Remember   | |   Recall   | | Reflect Pipeline |
  |  Engine     | |   Engine   | | (background)     |
  |             | |            | |                  |
  | - encode    | | - prime    | | - cluster (Rust) |
  | - score     | | - query    | | - propose (LLM)  |
  | - index     | | - subscribe| | - validate (LLM) |
  | - decay     | | - merge    | | - store insights |
  +------+------+ +------+-----+ +--------+---------+
         |               |                |
  +------+---------------+----------------+-----------+
  |              Index Layer                          |
  |   Temporal (B-tree) | Vector (HNSW) | Graph       |
  +----------------------+----------------------------+
                         |
  +----------------------+----------------------------+
  |         Storage Engine (RocksDB)                  |
  |         Column Families per index type            |
  +---------------------------------------------------+

  +-----------------------+  +------------------------+
  | Embedding Engine      |  | LLM Provider Interface |
  | (ONNX Runtime,        |  | (Anthropic, OpenAI,    |
  |  built-in default)    |  |  Ollama, pluggable)    |
  +-----------------------+  +------------------------+
```

**Built with:**
- **Rust**: no GC pauses, single static binary, C-level performance
- **RocksDB**: embedded LSM storage, proven by TiKV and CockroachDB
- **HNSW**: logarithmic-scaling vector index for similarity and analogical recall
- **ONNX Runtime**: built-in CPU embeddings (<5ms), zero external API dependencies
- **gRPC**: bidirectional streaming for real-time `subscribe` channels

---

## Deployment

**Standalone Server** (the Redis model)

```bash
hebbs start                                       # gRPC :6380, HTTP :6381
HEBBS_AUTH_ENABLED=true hebbs start                # with API key authentication
```

**Embedded Library** (the SQLite model)

```python
from hebbs import HEBBS

e = HEBBS.open("./agent-memory")  # No separate process
e.remember(...)
```

**Edge Mode** (robots, laptops, workstations): same API, different configuration. Runs the complete engine including local reflection with on-device LLMs.

---

## Use Cases

**Voice Sales Agents**: Remember prospect history across calls, handle objections with proven responses, learn which pitches convert over time.

**Customer Support**: Recall past tickets, surface solutions from similar issues, reduce escalations through consolidated troubleshooting knowledge.

**Coding Agents**: Remember what approaches worked, recall past debugging sessions, avoid repeating failed strategies.

**Robotics**: Learn navigation patterns, share knowledge across a fleet, reflect on operational efficiency. Fully offline on edge hardware.

**Personal Assistants**: Remember preferences, learn routines, pick up context across conversations.

---

## Contributing

We welcome contributions across the stack. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

All contributors must sign our [Contributor License Agreement](CLA.md) before their first PR can be merged.

---

## License

HEBBS uses a dual-license model.

**The engine** (hebbs-core, hebbs-storage, hebbs-index, hebbs-embed, hebbs-reflect, hebbs-server, hebbs-vault) is licensed under [BSL 1.1](LICENSE-BSL). Same license as CockroachDB, Sentry, and Terraform. Use it freely in production. The only restriction: you cannot offer HEBBS as a hosted service to third parties. Every version converts to Apache 2.0 after four years.

**Client libraries and protocol definitions** (hebbs-client, hebbs-proto, hebbs-ffi) are licensed under [Apache 2.0](LICENSE-APACHE). Fully open source with no restrictions.

Educational institutions and non-profit organizations can use the full engine without restriction. For other licensing arrangements, reach out at parag@hebbs.ai.

---

*Agents deserve better than a vector database and a prayer.*
