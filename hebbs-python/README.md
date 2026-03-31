# HEBBS Python SDK

Python client for the [HEBBS](https://hebbs.ai) cognitive memory engine. Provides an async gRPC client with a Pythonic interface -- no protobuf in the public API.

HEBBS gives your AI agents real memory: sub-10ms writes, sub-10ms recalls, four recall strategies, entity-scoped multitenancy, and GDPR-compliant erasure. This SDK wraps the gRPC API in idiomatic Python so you can integrate HEBBS in minutes.

## Quick Start

### 1. Install the HEBBS Server

```bash
curl -sSf https://hebbs.ai/install | sh
```

This installs `hebbs-server`, `hebbs-cli`, and `hebbs-bench` to `~/.hebbs/bin/`. The server runs BGE-small-en-v1.5 (ONNX) for embeddings internally -- no external embedding API needed.

### 2. Start the Server

```bash
hebbs-server
```

The server listens on `localhost:6380` (gRPC) and `localhost:6381` (REST) by default. On first start it prints a bootstrap API key -- save it.

### 3. Install the Python SDK

```bash
pip install hebbs
```

To include the demo app and all LLM providers:

```bash
pip install "hebbs[demo]"
```

### 4. Use the SDK

```python
import asyncio
from hebbs import HebbsClient

async def main():
    # api_key falls back to HEBBS_API_KEY env var if not passed
    async with HebbsClient("localhost:6380", api_key="hb_...") as h:
        # Store a memory (entity-scoped)
        mem = await h.remember(
            content="Acme Corp uses Salesforce CRM and has 200 engineers",
            importance=0.8,
            context={"company": "Acme Corp", "topic": "tech_stack"},
            entity_id="acme_corp",
        )

        # Recall by semantic similarity
        results = await h.recall(
            cue="What CRM does Acme use?",
            strategies=["similarity"],
            top_k=5,
            entity_id="acme_corp",
        )
        for r in results.results:
            print(f"  [{r.score:.2f}] {r.memory.content}")

        # Prime a session (load relevant context for an entity)
        prime = await h.prime(entity_id="acme_corp", max_memories=50)
        print(f"Primed {len(prime.results)} memories")

        # Generate insights from memory clusters (uses LLM server-side)
        reflect = await h.reflect(entity_id="acme_corp")
        print(f"Created {reflect.insights_created} insights")

        # GDPR-compliant cryptographic erasure
        forget = await h.forget(entity_id="acme_corp")
        print(f"Forgot {forget.forgotten_count} memories")

asyncio.run(main())
```

## SDK Reference

### HebbsClient

| Method | Description |
|--------|-------------|
| `remember(content, importance, context, entity_id, edges)` | Store a memory |
| `get(memory_id)` | Retrieve a memory by ID (bytes) |
| `recall(cue, strategies, top_k, entity_id, scoring_weights, cue_context)` | Multi-strategy recall |
| `prime(entity_id, max_memories, similarity_cue, scoring_weights)` | Session warm-up |
| `revise(memory_id, content, importance, context, entity_id)` | Update a memory |
| `forget(entity_id, memory_ids)` | GDPR-compliant erasure (by entity or by IDs) |
| `set_policy(max_snapshots_per_memory, auto_forget_threshold, decay_half_life_days)` | Configure tenant policies |
| `subscribe(entity_id, confidence_threshold)` | Real-time memory surfacing |
| `reflect(entity_id)` | Generate insights from clusters (LLM server-side) |
| `insights(entity_id, max_results)` | Retrieve accumulated insights |
| `health()` | Server health check |
| `count()` | Total memory count |

### Recall Strategies

- **similarity** -- semantic vector search (HNSW, entity-filtered)
- **temporal** -- time-ordered retrieval
- **causal** -- cause-and-effect graph traversal
- **analogical** -- cross-domain pattern matching (blends embedding + structural similarity)

Pass strategy names as strings for basic usage. For advanced tuning, pass `RecallStrategyConfig` objects. You can mix both in the same call:

```python
results = await h.recall(
    cue="What happened with Acme?",
    strategies=["temporal", RecallStrategyConfig("similarity", top_k=3, ef_search=200)],
    entity_id="acme_corp",
)
```

### RecallStrategyConfig

Per-strategy tuning parameters for advanced recall. Most users should just pass strategy names as strings. Use this when you need fine-grained control.

| Field | Type | Default | Used By | Description |
|-------|------|---------|---------|-------------|
| `strategy` | `str` | *(required)* | all | Strategy name: `"similarity"`, `"temporal"`, `"causal"`, `"analogical"` |
| `entity_id` | `str \| None` | `None` | all | Override entity scope for this strategy |
| `top_k` | `int \| None` | `None` | all | Per-strategy result limit (separate from the top-level `top_k`) |
| `ef_search` | `int \| None` | `50` | similarity | HNSW candidate count. Higher = more accurate, slower. |
| `time_range` | `tuple[int, int] \| None` | `None` (unbounded) | temporal | `(start_us, end_us)` microsecond timestamps. When omitted, returns all memories newest-first. |
| `seed_memory_id` | `bytes \| None` | `None` (auto) | causal | Starting node for graph traversal. When omitted, the engine picks the best seed. |
| `max_depth` | `int \| None` | `5` (max 10) | causal | Maximum hops in graph traversal. |
| `edge_types` | `list[EdgeType] \| None` | `None` (all) | causal | Restrict traversal to specific edge types. |
| `analogical_alpha` | `float \| None` | `0.5` | analogical | Blend weight: `0.0` = pure structural, `1.0` = pure embedding similarity. |

**Causal recall** -- trace cause-and-effect chains from a seed memory:

```python
from hebbs import RecallStrategyConfig, EdgeType

results = await h.recall(
    cue="What led to the pricing pushback?",
    strategies=[
        RecallStrategyConfig(
            "causal",
            seed_memory_id=mem.id,
            max_depth=3,
            edge_types=[EdgeType.CAUSED_BY, EdgeType.FOLLOWED_BY],
        )
    ],
)
```

**Analogical recall** -- find structurally similar patterns across entities:

```python
results = await h.recall(
    cue="enterprise CRM evaluation",
    strategies=[RecallStrategyConfig("analogical", analogical_alpha=0.7)],
    cue_context={"industry": "technology", "stage": "evaluation"},
    top_k=5,
)
```

### Scoring Weights

Recall results are ranked by a composite score blending relevance, recency, importance, and reinforcement. Pass `scoring_weights` to tune the blend:

```python
from hebbs import ScoringWeights

# Pure semantic match
results = await h.recall(
    cue="competitor pricing",
    scoring_weights=ScoringWeights(w_relevance=1.0, w_recency=0.0, w_importance=0.0, w_reinforcement=0.0),
)

# Recency-biased -- "what just happened?"
results = await h.recall(
    cue="latest updates",
    scoring_weights=ScoringWeights(w_relevance=0.2, w_recency=0.8, w_importance=0.0, w_reinforcement=0.0),
)

# Also works as a plain dict
results = await h.recall(
    cue="latest updates",
    scoring_weights={"w_relevance": 0.2, "w_recency": 0.8, "w_importance": 0.0, "w_reinforcement": 0.0},
)
```

Omit `scoring_weights` for the default blend (relevance 0.5, recency 0.2, importance 0.2, reinforcement 0.1).

### Authentication

The server generates a bootstrap API key on first start and prints it to stderr. Pass it to the client:

```python
async with HebbsClient("localhost:6380", api_key="hb_...") as h:
    ...
```

Or set the `HEBBS_API_KEY` environment variable and omit `api_key` -- the SDK picks it up automatically. To explicitly connect without auth, pass `api_key=""`.

## Entity Isolation (Multitenancy)

All HEBBS operations are scoped by `entity_id`. Memories stored under one entity are never returned when querying a different entity -- this applies to all four recall strategies, prime, reflect, and insights. No configuration needed; isolation is structural.

```python
await h.remember(content="Uses Salesforce", entity_id="acme_corp")
await h.remember(content="Uses HubSpot", entity_id="techflow_inc")

results = await h.recall(cue="What CRM?", entity_id="acme_corp")
# Only returns "Uses Salesforce" -- techflow_inc data is invisible
```

## Demo App

The demo ships an AI sales agent ("Atlas") that uses HEBBS for memory-augmented conversations. It shows every HEBBS operation in real-time panels: remember latency, recall scores, prime context, and session metrics.

### Configure an LLM

```bash
# Pick one:
export GEMINI_API_KEY="your-key"        # Gemini (default)
export OPENAI_API_KEY="your-key"        # OpenAI
export ANTHROPIC_API_KEY="your-key"     # Anthropic
# Or use Ollama / mock (no keys needed)
```

### Run Interactive Chat

```bash
hebbs-demo interactive
```

Or specify a config:

```bash
hebbs-demo interactive --config gemini-vertex  # Gemini via Vertex AI
hebbs-demo interactive --config gemini         # Gemini via API key
hebbs-demo interactive --config openai         # GPT-4o
hebbs-demo interactive --config local          # Ollama (no API key)
hebbs-demo interactive --mock-llm              # Mock LLM (no API key)
```

Switch entities mid-session to demonstrate multitenancy:

```bash
hebbs-demo interactive --entity-id acme_corp
# In-session: /session techflow_inc
```

### Run Scenarios

```bash
hebbs-demo scenarios --all                # Run all 7 scenarios
hebbs-demo scenarios --run discovery_call # Run a specific one
```

Available scenarios: `discovery_call`, `objection_handling`, `multi_session`, `reflect_learning`, `subscribe_realtime`, `forget_gdpr`, `multi_entity`.

### LLM Providers

| Provider | Config | Env Variable |
|----------|--------|--------------|
| Gemini (Vertex AI) | `gemini-vertex.toml` | `GOOGLE_APPLICATION_CREDENTIALS`, `GOOGLE_CLOUD_PROJECT` |
| Gemini (API key) | `gemini.toml` | `GEMINI_API_KEY` |
| OpenAI | `openai.toml` | `OPENAI_API_KEY` |
| Anthropic | (custom toml) | `ANTHROPIC_API_KEY` |
| Ollama | `local.toml` | (none -- Ollama must be running) |

## Requirements

- Python >= 3.10
- A running HEBBS server (gRPC on port 6380)

## Contributing

Contributions are welcome. By submitting a pull request, you agree to the [Contributor License Agreement](CLA.md).

## License

Copyright 2025 Parag Arora. Apache 2.0 -- see [LICENSE](LICENSE).
