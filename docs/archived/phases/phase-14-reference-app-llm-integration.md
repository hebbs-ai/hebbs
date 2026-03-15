# Phase 14: Reference Application and LLM Integration -- Architecture Blueprint

## Status: COMPLETE

---

## Intent

Phases 1 through 13 built the engine, the server, the SDKs, and hardened the system for production. Every operation works. Every test passes. Benchmarks prove the latency budgets hold. But no one has used HEBBS to build something real yet. Phase 14 exists because a memory engine that has never been exercised by a real application is an untested hypothesis.

The gap is not correctness -- Phase 12 proved that. The gap is *fitness for purpose*. Does the API surface feel right when building a multi-session agent? Does `subscribe()` actually improve response quality in a live conversation? Does `reflect()` produce insights that a real LLM can use to give better answers? Does `prime()` load the right context, or does it return noise? Does the Python SDK's ergonomics hold up across a 500-line application? These questions cannot be answered by unit tests. They require a real application exercising every operation in a realistic workflow.

Phase 14 serves four audiences:

- **The HEBBS team:** Validates that the API surface is complete and ergonomic. Surfaces usability gaps, missing convenience methods, and error messages that are unhelpful in practice. Every API rough edge found here is one fewer support ticket from adopters.
- **Prospective customers (YC companies building AI agents):** A working reference app demonstrates HEBBS's unique capabilities -- compound learning via `reflect()`, real-time memory surfacing via `subscribe()`, multi-strategy recall, and graph-based causal traversal -- in a scenario they care about: AI agents that get measurably smarter over time.
- **The LLM integration story:** Phase 7 built the `LlmProvider` trait and four implementations (Mock, Anthropic, OpenAI, Ollama). But the only consumer is the reflect pipeline. Phase 14 exercises LLMs in two additional roles: conversational generation (the agent's responses) and intelligent memory extraction (deciding what to remember from a conversation). This validates prompt robustness across providers and models, measures latency and cost, and produces a provider compatibility matrix.
- **The documentation site (Phase 16):** Every cookbook, tutorial, and "getting started" guide needs working code. Phase 14 produces the canonical reference implementation that all documentation references.

The choice of use case -- an AI Sales Intelligence Agent -- is deliberate. Sales AI is the largest cluster of funded YC companies in the current batch (Leaping AI, Ergo, and dozens more). Every one of them needs an agent that learns from past interactions. The use case exercises every HEBBS operation in a realistic flow and showcases the three capabilities that no competitor (including Mem0) can replicate: `reflect()` for compound learning, `subscribe()` for real-time memory surfacing, and multi-strategy recall for contextual intelligence.

---

## Scope Boundaries

### What Phase 14 delivers

- `hebbs-demo` Python package: a CLI-based AI Sales Intelligence Agent built on the `hebbs` Python SDK (embedded mode)
- Multi-session conversation workflow exercising all 9 HEBBS operations in realistic patterns
- LLM provider test harness: identical scenarios run across OpenAI (default), Anthropic, and optionally Ollama, producing latency/quality/cost comparison reports
- Scripted scenario test suite: deterministic multi-turn conversations validating specific HEBBS behaviors (memory persistence, insight generation, cross-session recall, real-time surfacing, forgetting)
- Provider compatibility matrix: documented results for which models work best for each role (conversation, reflection proposal, reflection validation, memory extraction)
- End-to-end validation with real embeddings (OnnxEmbedder BGE-small and OpenAI text-embedding-3) -- the first time the full stack runs with real semantic similarity
- Embedding provider comparison: ONNX (local, free) vs OpenAI (API, paid) with recall quality, latency, and cost metrics
- API ergonomics report: a written list of friction points, missing convenience methods, and suggested improvements discovered during app development

### What Phase 14 explicitly does NOT deliver

- A production-ready SaaS application (this is a reference app and validation tool, not a product)
- A web UI or Streamlit frontend (CLI-only; a web frontend is a separate concern for Phase 16 documentation or a standalone project)
- New HEBBS engine features or API changes (if Phase 14 discovers gaps, they are documented as recommendations, not implemented in this phase)
- Performance optimization based on findings (document, don't fix -- fixes belong in targeted follow-up work)
- Deployment of the reference app (it runs locally; cloud deployment is Phase 15)
- Voice AI integration (microphone input, TTS output) -- text-based CLI only

---

## Architectural Decisions

### 1. Use Case Selection: AI Sales Intelligence Agent

Three candidate use cases were evaluated:

| Use Case | Operations Exercised | Differentiator Showcase | Market Relevance |
|----------|---------------------|------------------------|------------------|
| Personal Knowledge Assistant | remember, recall, reflect, forget | reflect, decay | Medium (crowded, many competitors) |
| Customer Support Agent | remember, recall, prime, subscribe | subscribe, prime | High (large market, but commoditized) |
| **Sales Intelligence Agent** | **all 9** | **reflect, subscribe, causal recall, analogical recall, prime, graph edges** | **Very high (largest YC category, clear ROI)** |

**Decision: Sales Intelligence Agent.** It is the only use case that naturally exercises all 9 operations, all 4 recall strategies, graph edges (deal networks, referral chains), subscribe (real-time coaching during calls), and reflect (institutional knowledge from call patterns). It also directly maps to the pain point of the largest cluster of funded AI startups.

### 2. Architecture: Python CLI on Embedded Mode

The reference app runs as a Python CLI using `HEBBS.open("./data")` (embedded mode). No server process, no gRPC, no Docker.

**Why embedded, not server mode:**
- Eliminates deployment complexity for anyone trying the demo
- Exercises the Python SDK's embedded path end-to-end (the most common adoption pattern for early users)
- Real OnnxEmbedder runs in-process (no network latency to an embedding service)
- LLM calls go directly from Python to provider APIs (no HEBBS server intermediary)

**Why Python, not Rust:**
- Python is the lingua franca of AI/ML teams -- the target audience for HEBBS
- The Python SDK (Phase 11) is the primary adoption surface; this validates its ergonomics
- LLM provider SDKs (openai, anthropic) are best supported in Python
- Lower barrier to entry for contributors and evaluators

**Why CLI, not web app:**
- No frontend framework dependency
- Terminal output is debuggable, scriptable, and pipeable
- Scripted scenarios can run headlessly in CI
- Streamlit or web frontend can be layered on later without changing the core logic

### 3. Conversation Architecture: Simulated Multi-Session Sales Flow

The app does not require a live human to operate. It supports two modes:

**Interactive mode:** A human types messages as a sales prospect. The AI sales agent responds using an LLM with HEBBS-powered context. Memories are created, recalled, and surfaced in real time.

**Scripted mode:** Pre-written conversation scripts simulate multi-session sales workflows. Each script defines:
- A sequence of prospect messages across multiple sessions
- Expected HEBBS behaviors at each step (what should be recalled, what insights should exist, what subscribe should surface)
- Assertions that validate HEBBS is working correctly

Scripted mode is the primary validation tool. Interactive mode is the demo for prospective customers.

### 4. LLM Integration: Three Roles Beyond Reflect

Phase 7's `LlmProvider` trait handles reflect (proposal + validation). Phase 14 introduces two additional LLM roles that use standard Python LLM client libraries (not the Rust `LlmProvider` trait, since the app is Python-side):

| Role | What It Does | Provider Used | Latency Sensitivity |
|------|-------------|---------------|-------------------|
| **Conversation** | Generate the sales agent's responses, given recalled context | Any (configurable) | High (user-facing) |
| **Memory Extraction** | Decide what facts from a conversation turn to remember, with what importance and context metadata | Any (configurable) | Low (background) |
| **Reflect Proposal** | Generate candidate insights from memory clusters | Via HEBBS engine (Phase 7 pipeline) | Low (batch) |
| **Reflect Validation** | Validate candidate insights against source memories | Via HEBBS engine (Phase 7 pipeline) | Low (batch) |

The conversation and memory extraction roles run in Python using the `openai` and `anthropic` client libraries directly. The reflect roles run through HEBBS's built-in reflect pipeline (which calls the `LlmProvider` trait implementations in Rust).

### 5. Memory Extraction Strategy: Structured Context from Conversations

Every conversation turn produces structured memories, not raw transcript dumps. The memory extraction LLM call takes a conversation turn and outputs:

```json
{
  "memories": [
    {
      "content": "Acme Corp's main concern is compliance with SOC 2 requirements",
      "importance": 0.8,
      "context": {
        "company": "Acme Corp",
        "topic": "compliance",
        "stage": "discovery",
        "sentiment": "concerned"
      },
      "edges": [
        {"target": "previous_memory_id", "edge_type": "followed_by"}
      ]
    }
  ],
  "skip_reason": null
}
```

This structured extraction is what makes HEBBS's multi-strategy recall work well. Raw text dumps would make similarity search the only useful strategy. Structured context enables temporal queries ("last 5 interactions with Acme Corp" via entity_id), causal traversal (following `followed_by` and `caused_by` edges), and analogical recall (structural similarity on context keys).

### 6. Observability Display: Showing HEBBS Behind the Scenes

The demo's primary differentiation tool is not the agent's responses -- it is the **visible HEBBS activity panel** that shows evaluators exactly what the memory engine is doing on every turn. Without this, the demo is indistinguishable from good prompt engineering. With it, the evaluator sees infrastructure working.

**Design: Collapsible Rich Panels with Title Bar + Toggle**

Every HEBBS operation renders a Rich panel in the terminal. Each panel has a **title bar that is always visible** (one-line summary with operation name, count, and latency) and a **detail section that is collapsible** via a `+`/`-` toggle controlled by the `--detail` flag or an interactive keypress in REPL mode.

**Collapsed view (default in interactive mode -- clean, scannable):**

```
[+] REMEMBER   1 memory stored (importance: 0.8, entity: acme_corp)              3.2ms
[+] RECALL     5 memories retrieved (strategy: Similarity+Temporal)               4.1ms
[+] SUBSCRIBE  1 memory surfaced (confidence: 0.84)                               0.8ms
                                                                          total:  8.1ms

Agent: Based on what I've seen with similar fintech companies, SOC 2
       compliance is usually the top concern at this stage...
```

**Expanded view (toggle `+` → `-`, or use `--detail` flag):**

```
[-] REMEMBER   1 memory stored (importance: 0.8, entity: acme_corp)              3.2ms
    │ content:  "Acme Corp concerned about SOC 2 compliance"
    │ context:  company=Acme Corp, topic=compliance, stage=discovery, sentiment=concerned
    │ edge:     followed_by → mem_01JBX7KM...
    │ embed:    384-dim BGE-small (2.9ms)

[-] RECALL     5 memories retrieved (strategy: Similarity+Temporal)               4.1ms
    │ query:    "compliance concerns in fintech"
    │ 0.91  "Beta Inc had identical SOC 2 blockers"                    [Episode]
    │ 0.87  "Gamma Corp resolved compliance via Vanta partnership"     [Episode]
    │ 0.83  "Fintech prospects prioritize compliance over cost"        [Insight]
    │ 0.79  "Last call with Acme: discussed pricing timeline"          [Episode]
    │ 0.71  "Delta Corp compliance timeline was 6 months"              [Episode]

[-] SUBSCRIBE  1 memory surfaced (confidence: 0.84)                               0.8ms
    │ "Beta Inc resolved SOC 2 by partnering with Vanta"
    │ surfaced because: keyword overlap (SOC, compliance) + embedding similarity

                                                                          total:  8.1ms

Agent: Based on what I've seen with similar fintech companies...
```

**Session initialization (prime) display:**

```
[-] PRIME      14 memories loaded for entity acme_corp                            6.2ms
    │ temporal:      8 memories (recent history)
    │ insights:      4 memories (from reflect)
    │ supplementary: 2 memories (similar entities)

[+] INSIGHTS   3 active insights for this scope
    │ "Fintech prospects prioritize compliance over cost"              (0.91)
    │ "Discovery calls convert 2x when leading with ROI"              (0.87)
    │ "Q4 budget cycles require 3-week follow-up cadence"             (0.82)
    │ source: reflected from 50 calls across 12 clusters
```

**Reflect pipeline display (batch operation):**

```
[-] REFLECT    47 memories → 5 clusters → 8 candidates → 5 insights             14.2s
    │ clustering:    47 memories → 5 clusters (142ms, spherical k-means)
    │ proposing:     5 clusters → 8 candidate insights (LLM: claude-sonnet-4)
    │ validating:    8 candidates → 5 accepted, 3 rejected (LLM: claude-sonnet-4)
    │ consolidating: 5 insights stored with InsightFrom edges
    │
    │ New insights:
    │   ✅ "Fintech prospects prioritize compliance over cost"         (0.91, 12 sources)
    │   ✅ "Discovery calls convert 2x when leading with ROI"         (0.87, 8 sources)
    │   ✅ "Pricing objections respond to ROI framing in Q4"          (0.85, 6 sources)
    │   ✅ "Healthcare prospects need HIPAA before feature demo"      (0.83, 9 sources)
    │   ✅ "Multi-threaded deals close 3x faster with exec sponsor"   (0.81, 5 sources)
    │   ❌ "Enterprise deals close faster in Q4" (rejected: insufficient evidence)
    │   ❌ "Cold outreach converts better on Tuesdays" (rejected: contradicted by data)
    │   ❌ "Startups prefer monthly billing" (rejected: low confidence 0.42)
```

**Forget display:**

```
[-] FORGET     20 memories removed for entity acme_corp                          12.1ms
    │ memories:   20 deleted from all indexes
    │ edges:      34 graph edges removed
    │ cascaded:   3 predecessor snapshots deleted
    │ tombstones: 20 created (SHA-256 hashed)
    │ compaction: triggered on 4 column families
```

**"No activity" indicator (equally important):**

```
[·] HEBBS      no memories recalled, no subscriptions fired                       1.2ms

Agent: Could you tell me more about your team size?
```

This makes the moments when HEBBS *does* fire visually distinct by contrast.

**Implementation: `DisplayManager` component**

A `DisplayManager` class in `hebbs_demo/display.py` wraps every HEBBS SDK call and captures:
- Operation name and parameters
- Return values (memory count, recall results, subscribe pushes, reflect output)
- Wall-clock latency via `time.perf_counter()`
- Cumulative per-turn latency

It renders using `rich.panel.Panel`, `rich.table.Table`, and `rich.text.Text` with color coding:
- **Insights** highlighted in cyan (distinct from episodes)
- **Subscribe pushes** highlighted in yellow (attention-grabbing)
- **Rejected reflect candidates** in dim/strikethrough
- **Latency** in green if < 10ms, yellow if 10-100ms, red if > 100ms
- **Memory kind** badges: `[Episode]`, `[Insight]`, `[Revision]`

**Verbosity levels (CLI flag):**

| Flag | Title Bar | Details | Use Case |
|------|-----------|---------|----------|
| `--quiet` | Hidden | Hidden | Scripted tests, CI runs |
| `--normal` (default) | Visible | Collapsed (`+`) | Interactive demos, clean output |
| `--verbose` | Visible | Expanded (`-`) | Deep demos, debugging, development |
| `--verbose --show-embeddings` | Visible | Expanded + raw vectors | Engineering deep-dives |

In interactive REPL mode, the user can toggle individual panels between `+` and `-` by pressing `d` (detail toggle) after each turn.

### 7. LLM Provider Configuration and Testing

The app supports configurable LLM providers via environment variables and a config file:

```toml
[llm]
conversation_provider = "openai"       # "openai", "anthropic", or "ollama"
conversation_model = "gpt-4o"          # best quality for conversation
extraction_provider = "openai"
extraction_model = "gpt-4o-mini"       # structured JSON output, cost-effective

[llm.openai]
api_key_env = "OPENAI_API_KEY"         # reads from $OPENAI_API_KEY
model = "gpt-4o"                       # default model for OpenAI provider

[llm.anthropic]
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-20250514"

[llm.ollama]
base_url = "http://localhost:11434"
model = "llama3.2"

[embedding]
provider = "openai"                    # "onnx" (local), "openai", or "ollama"

[embedding.onnx]
model = "bge-small-en-v1.5"           # 384-dim, local, free, offline
# model = "bge-large-en-v1.5"         # alt: 1024-dim, local, better quality

[embedding.openai]
api_key_env = "OPENAI_API_KEY"
model = "text-embedding-3-small"       # 1536-dim, ~62% MTEB, $0.02/1M tokens
# model = "text-embedding-3-large"     # 3072-dim, ~64% MTEB, $0.13/1M tokens

[embedding.ollama]
base_url = "http://localhost:11434"
model = "nomic-embed-text"             # 768-dim, local, good quality

[hebbs]
data_dir = "./hebbs-data"

[hebbs.reflect]
provider = "openai"                    # uses OpenAI for reflect proposal + validation
model = "gpt-4o"
threshold = 20                         # reflect after every 20 new memories
```

All LLM and embedding calls default to OpenAI -- a single `OPENAI_API_KEY` environment variable is all that's needed to run the full demo. No Ollama, no local model downloads required.

Switching embedding providers is a one-line config change (`provider = "openai"` instead of `provider = "onnx"`). The demo app wraps the `Embedder` trait to route to the configured provider:

- **`onnx` (default):** Uses the built-in `OnnxEmbedder` directly. Zero cost, offline, ~3ms latency. Best for development, CI, edge deployment demos, and the "data sovereignty" story.
- **`openai`:** Implements the `Embedder` trait by calling OpenAI's embedding API. Higher quality at large scale, but adds ~100-300ms network latency per call and API cost. Best for quality-focused demos or head-to-head comparisons.
- **`ollama`:** Uses Ollama's `/api/embeddings` endpoint with models like `nomic-embed-text`. Local, free, better quality than BGE-small. Good middle ground.

**Important constraint:** Switching embedding providers mid-database is not supported. Different providers produce different vector dimensions and different vector spaces. When you change `[embedding].provider`, you must start with a fresh `data_dir`. The demo app validates this on startup and refuses to open a database created with a different embedder.

The test harness has two comparison modes:

**`hebbs-demo compare-llm`** -- Runs the same scenario across all configured LLM providers (fixed embedding):

| Metric | Measured Per Provider |
|--------|---------------------|
| Response latency (p50, p99) | Per-turn conversation response time |
| Extraction quality | Structured output parse success rate, field completeness |
| Insight quality | Reflect insight relevance score (human evaluation rubric) |
| Token usage | Input/output tokens per operation type |
| Cost | Estimated USD per 100-conversation-turn session |
| Prompt robustness | Identical scenario, different providers -- same behavioral outcomes? |

**`hebbs-demo compare-embeddings`** -- Runs the same scenario across all configured embedding providers (fixed LLM). This is the "HEBBS with a free local model vs Mem0 with expensive OpenAI embeddings" demo:

| Metric | Measured Per Embedding Provider |
|--------|-------------------------------|
| Recall precision@10 | Top-10 similarity recall relevance (human-judged or proxy score) |
| Recall latency (p50, p99) | Time for `recall(Similarity)` including embedding the query |
| Subscribe hit rate | Fraction of conversation turns where subscribe surfaces a relevant memory |
| Reflect cluster quality | Silhouette score of clusters produced by the reflect pipeline |
| Embedding latency | Per-call embedding time (local ONNX vs API round-trip) |
| Index size on disk | RocksDB vectors CF size at 1K memories |
| Total cost | Embedding API cost for the full scenario run |

Each embedding provider gets a fresh database (mandatory -- different vector spaces). The comparison report shows side-by-side results and highlights where HEBBS's multi-strategy recall compensates for a simpler embedding model. The expected narrative: "HEBBS with free BGE-small-384d achieves comparable or better recall quality to similarity-only search with OpenAI-3-large-3072d, because temporal, causal, and analogical strategies contribute recall hits that embedding quality alone cannot."

Pre-built config profiles for quick switching:

```bash
# OpenAI everything -- single API key, no local setup (default)
hebbs-demo interactive --config configs/openai.toml

# Best quality for demos (GPT-4o conversation + OpenAI embeddings)
hebbs-demo interactive --config configs/demo-quality.toml

# Local-only, free, offline (requires Ollama installed)
hebbs-demo interactive --config configs/local.toml

# Cost-optimized (GPT-4o-mini LLM + local ONNX embeddings, no embedding API cost)
hebbs-demo interactive --config configs/cost-optimized.toml

# Embedding comparison run
hebbs-demo compare-embeddings --configs configs/openai-embed.toml,configs/onnx-embed.toml
```

---

## Work

### 1. `hebbs-demo` Package Structure

```
hebbs-demo/
  pyproject.toml              # Dependencies: hebbs, openai, anthropic, click, rich
  README.md                   # Setup instructions, usage guide
  hebbs_demo/
    __init__.py
    cli.py                    # Click-based CLI: interactive, scripted, bench, compare
    agent.py                  # SalesAgent class: conversation loop with HEBBS integration
    memory_manager.py         # Memory extraction, structured remember(), context building
    llm_client.py             # Unified LLM interface wrapping openai/anthropic/ollama
    config.py                 # TOML config loading, env var resolution
    display.py                # DisplayManager: collapsible Rich panels for HEBBS activity
    scenarios/
      __init__.py
      base.py                 # Scenario base class with assertion framework
      discovery_call.py       # Scenario: initial discovery call, prospect qualification
      objection_handling.py   # Scenario: pricing objection → resolution → close
      multi_session.py        # Scenario: 5 sessions over 2 weeks, cross-session recall
      reflect_learning.py     # Scenario: 50 calls → reflect → new call uses insights
      subscribe_realtime.py   # Scenario: subscribe active during call, real-time surfacing
      forget_gdpr.py          # Scenario: customer requests data deletion
      multi_entity.py         # Scenario: multiple prospects, entity isolation
    reports/
      __init__.py
      comparison.py           # LLM provider comparison report generator
      ergonomics.py           # API friction point collector
  tests/
    test_scenarios.py         # pytest suite running all scenarios (OpenAI default, mock fallback)
    test_memory_extraction.py # Unit tests for extraction prompt/parse
    test_llm_client.py        # Unit tests for LLM client abstraction
  configs/
    openai.toml               # Default: OpenAI for LLM + embeddings (single API key)
    demo-quality.toml         # Best quality: GPT-4o conversation + OpenAI embeddings
    local.toml                # Local-only: Ollama LLM + ONNX BGE-small (requires Ollama)
    cost-optimized.toml       # Budget: GPT-4o-mini LLM + local ONNX embeddings
    openai-embed.toml         # OpenAI embeddings only (for compare-embeddings)
    onnx-embed.toml           # Local ONNX embeddings only (for compare-embeddings)
    anthropic.toml            # Anthropic LLM config template
```

### 2. Scripted Scenario Suite

Seven scenarios, each validating distinct HEBBS capabilities:

**Scenario A: Discovery Call**
- Single-session conversation (10 turns)
- Validates: `remember()` with structured context, `recall(Similarity)` mid-conversation, memory extraction quality
- Assertions: memories created with correct context metadata, recall returns relevant prior context

**Scenario B: Objection Handling**
- Two sessions: first call encounters pricing objection, second call with different prospect has similar objection
- Validates: `recall(Analogical)` finds structurally similar past situations, `prime()` loads relevant context before the second call
- Assertions: second call's context includes the first call's resolution, analogical recall ranks the match above random memories

**Scenario C: Multi-Session Relationship**
- Five sessions with the same prospect over simulated 2-week period
- Validates: `recall(Temporal)` returns correct chronological history, `prime()` correctly initializes each session, graph edges form a `followed_by` chain, decay scores reflect access patterns
- Assertions: temporal recall returns sessions in order, prime includes recent history + relevant insights, graph traversal follows the session chain

**Scenario D: Institutional Learning (Reflect)**
- Ingest 50 simulated call transcripts across 10 companies in the same industry (fintech)
- Run `reflect()` with a real LLM
- Start a new call with an 11th fintech company
- Validates: `reflect()` produces coherent insights from call patterns, insights are recallable via `recall(Similarity)` and `insights()`, the new call benefits from institutional knowledge
- Assertions: at least 3 insights generated from 50 calls, insights have `InsightFrom` graph edges to source memories, recall for the new call includes at least one insight

**Scenario E: Real-Time Surfacing (Subscribe)**
- Start a conversation with `subscribe()` active
- Feed conversation text through the subscription as the call progresses
- Validates: relevant memories surface in real-time as topics arise, new memories created during the call are picked up by the subscription (new-write notification)
- Assertions: at least one memory surfaces during a 10-turn conversation, surfaced memories are relevant (confidence > 0.5), subscribe stats show chunks processed and memories pushed

**Scenario F: GDPR Forget**
- Remember 20 memories about a prospect, recall to verify they exist, then `forget()` by entity
- Validates: `forget()` removes all memories and graph edges, subsequent recall returns empty, tombstones are created
- Assertions: recall returns zero results after forget, graph traversal returns zero results, forget output reports correct count

**Scenario G: Multi-Entity Isolation**
- Interleave conversations with 3 different prospects
- Validates: entity scoping isolates memories correctly, `prime()` for entity A does not include entity B's memories, `subscribe()` scoped to entity A does not surface entity B's memories
- Assertions: no cross-entity contamination in recall, prime, or subscribe results

### 3. LLM Provider Test Harness

A CLI command (`hebbs-demo compare`) that runs Scenario D (the most LLM-intensive scenario) across all configured providers and produces a structured comparison report.

For each provider:
1. Initialize a fresh HEBBS database
2. Run the same 50-call ingestion with the same simulated transcripts
3. Measure extraction quality (parse success rate, field completeness)
4. Run reflect and measure insight quality (count, confidence scores, coherence)
5. Run a new-call scenario and measure context quality (relevance of recalled memories and insights)
6. Record latency, token usage, and estimated cost per turn

Output: JSON report + human-readable Rich table to stdout.

### 4. End-to-End Validation with Real Embeddings

All scenarios run with the real OnnxEmbedder (BGE-small-en-v1.5, 384 dimensions) by default. This is the first time the full HEBBS stack runs with real semantic similarity outside of the `hebbs-embed` crate's own tests.

What this validates:
- OnnxEmbedder model auto-download works on first run
- Real embeddings produce meaningful similarity rankings (not just hash-based MockEmbedder similarity)
- HNSW recall quality with real embeddings meets the >85% recall@10 target
- Subscribe's bloom filter and centroid stages work with real embedding distributions
- Reflect clustering produces meaningful clusters from real embeddings (not random hash vectors)

### 5. API Ergonomics Audit

During development of `hebbs-demo`, systematically document every friction point encountered with the `hebbs` Python SDK. Categories:

| Category | Examples |
|----------|---------|
| Missing convenience methods | No `hebbs.recall_similar(text, top_k)` shorthand; must construct full RecallInput |
| Awkward type conversions | Converting between Python dicts and HEBBS context types |
| Unhelpful error messages | Error says "InvalidInput" but doesn't say which field |
| Missing features | No way to query "all memories for entity X" without recall |
| Performance surprises | First `remember()` takes 3s (model download) with no progress indicator |
| Documentation gaps | Which fields are optional? What are sensible defaults? |

Output: `ERGONOMICS_REPORT.md` checked into the `hebbs-demo/` directory, organized by category with severity ratings and suggested fixes.

---

## LLM Model Recommendations

Based on the roles defined above, the following models are recommended for each role:

### Conversation Generation

| Provider | Model | Rationale |
|----------|-------|-----------|
| Ollama | llama3.2 (3B) | Fast local inference, good for development and demos |
| Ollama | mistral (7B) | Better quality for complex responses, still local |
| Anthropic | claude-sonnet-4-20250514 | Production quality, strong at role-playing and structured output |
| OpenAI | gpt-4o-mini | Cost-effective, fast, good quality for conversation |

### Memory Extraction (Structured Output)

| Provider | Model | Rationale |
|----------|-------|-----------|
| Ollama | qwen2.5 (7B) | Strong at structured JSON output, local |
| Anthropic | claude-haiku-3 | Cheapest Anthropic model, sufficient for extraction |
| OpenAI | gpt-4o-mini | Good at following JSON schemas, low cost |

### Reflection (Proposal + Validation)

| Provider | Model | Rationale |
|----------|-------|-----------|
| Ollama | llama3.2 (3B) | Adequate for proposal, local/free |
| Anthropic | claude-sonnet-4-20250514 | Best for validation (needs reasoning about contradictions) |
| OpenAI | gpt-4o | Strong reasoning for validation stage |

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| OnnxEmbedder model download fails in CI or on first run | High -- demo doesn't start | Pre-download model in setup script. Fallback to MockEmbedder with clear warning. Document the 33MB download requirement. |
| OpenAI API key not available or rate-limited | Medium -- demo doesn't start | Support `--mock-llm` flag that uses canned responses for all LLM roles. Document the single env var requirement (`OPENAI_API_KEY`). Fall back to Ollama or mock if key is missing with a clear error message. |
| LLM structured output parsing fails (malformed JSON) | Medium -- memory extraction breaks | Implement retry with simplified prompt on parse failure. Log raw LLM output for debugging. Fall back to raw-text memory if structured extraction fails 3 times. |
| Real embeddings produce different recall quality than MockEmbedder tests expect | Medium -- scenario assertions may need tuning | Use similarity thresholds, not exact match assertions. Accept "at least N relevant results" rather than "exactly these results." Run scenarios with both MockEmbedder and OnnxEmbedder during development. |
| LLM API rate limits during provider comparison runs | Low -- comparison is slow, not broken | Implement per-provider rate limiting with configurable delay. Retry with exponential backoff on 429. Run comparison in "patient" mode with 1s delay between calls. |
| Python SDK ergonomics issues discovered are too numerous to fix before Phase 14 completion | Expected -- this is the point | Document all issues in ERGONOMICS_REPORT.md. Prioritize by severity. Fix critical blockers (crashes, data loss) immediately. Defer UX improvements to a follow-up task. Phase 14's job is to find the issues, not necessarily fix all of them. |
| Reflect with real LLMs produces low-quality or irrelevant insights | Medium -- diminishes the demo's impact | Iterate on prompt engineering within Phase 14 scope. Test with multiple models to find the best quality/cost trade-off. If no provider produces good insights, document the prompt improvement as future work and use curated examples for demos. |

---

## Done When

### Reference Application

- [x] `hebbs-demo` installs via `pip install -e .` with all dependencies
- [x] Interactive mode: human can converse with the AI sales agent, memories persist across sessions
- [x] Scripted mode: all 7 scenarios pass with mock LLM (OpenAI configurable via API key)
- [x] All 9 HEBBS operations exercised across the scenario suite
- [x] All 4 recall strategies exercised and validated (similarity, temporal, causal, analogical)
- [x] Graph edges created during conversations and traversable via `recall(Causal)`
- [x] `subscribe()` surfaces relevant memories in real-time during Scenario E
- [x] `reflect()` runs end-to-end in Scenario D (50 call ingestion + reflect pipeline)
- [x] `forget()` cleanly removes all data for an entity in Scenario F (20 memories, verified empty)
- [x] Multi-entity isolation verified in Scenario G (3 entities, 30 assertions, zero contamination)
- [x] Embedding provider switchable via one-line config change
- [x] README documents embedding provider selection, cost comparison, and switching procedure

### Observability Display

- [x] Every HEBBS operation renders a collapsible Rich panel with always-visible title bar (operation, count, latency)
- [x] `+`/`-` toggle: collapsed shows one-line summary, expanded shows full details (memories, scores, edges, context)
- [x] `--quiet`, `--normal` (default), `--verbose` flags control default expansion level
- [x] Per-turn cumulative latency displayed
- [x] Insights visually distinct from episodes (color-coded, `[Insight]` badge)
- [x] Subscribe pushes highlighted with confidence score
- [x] Reflect pipeline shows memories processed, clusters found, insights created
- [x] "No activity" indicator on turns where HEBBS returns no results
- [x] Prime display at session start shows temporal/insight/supplementary memory breakdown
- [x] Forget display shows memory count, cascade count, tombstone creation

### LLM Integration

- [x] Conversation generation works with OpenAI, Anthropic, and Ollama providers (configurable)
- [x] Memory extraction produces valid structured JSON output (with retry logic and parse validation)
- [x] Reflect pipeline runs end-to-end (mock provider in embedded mode; real provider via server mode)
- [x] `hebbs-demo compare-llm` produces an LLM provider comparison report (Rich table)
- [x] `hebbs-demo compare-embeddings` produces an embedding provider comparison report
- [x] Provider compatibility matrix documented via config profiles and comparison commands
- [x] Prompt templates checked in and versioned for each LLM role (`prompts.py`)

### Validation

- [x] `pytest tests/` passes all 51 tests (42 unit + 9 scenario/integration)
- [x] Scenario tests runnable in CI with mock LLM fallback (no API keys needed)
- [x] API ergonomics report (`ERGONOMICS_REPORT.md`) written with 10 categorized findings
- [x] README documents setup, usage, and all CLI commands
- [x] Config profiles provided: `openai.toml`, `local.toml`, `demo-quality.toml`, `cost-optimized.toml`, `anthropic.toml`, `openai-embed.toml`, `onnx-embed.toml`
- [x] Switching between embedding providers is a one-line config change

---

## Interfaces Published to Future Phases

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| Scenario suite framework (`scenarios/base.py`) | 16 (Documentation -- scenarios become tutorial code) | Stable: scenarios are reusable examples |
| LLM provider comparison report format | 16 (Documentation -- published as benchmark data) | Schema-versioned JSON |
| API ergonomics report | Python SDK improvements (backlog) | One-time document, not a stable interface |
| `hebbs-demo` as a reference implementation | 16 (Documentation -- code samples extracted from here) | Living code, updated as APIs evolve |
| Prompt templates for memory extraction | Future agent integrations | Versioned, provider-specific |
| `DisplayManager` observability pattern | 16 (Documentation -- demo screenshots and recordings), future web UI | Reusable: wrap HEBBS calls, emit structured activity events |

---

## Relationship to Mem0

Phase 14's reference app is positioned against Mem0's cookbook ecosystem. The key differentiators that the reference app must clearly demonstrate:

| Mem0 Cookbook Pattern | HEBBS Equivalent | What HEBBS Adds |
|---------------------|-----------------|-----------------|
| `memory.add(text)` → `memory.search(query)` | `hebbs.remember(content, context)` → `hebbs.recall(strategy)` | Four recall strategies, not just similarity. Structured context enables temporal, causal, and analogical recall. |
| Manual `memory.update()` for contradictions | `hebbs.revise()` with automatic lineage tracking | Revision history preserved via graph edges. Old versions are traceable. |
| `expiration_date` for time-bound facts | `decay` with biological reinforcement model | Frequently-accessed memories stay strong. Unused memories fade naturally. No manual TTL management. |
| No learning from patterns | `hebbs.reflect()` → institutional insights | The agent gets measurably smarter over time. 500 calls become distilled playbook entries. |
| No real-time surfacing | `hebbs.subscribe()` → memories surface during conversation | Context appears as the conversation unfolds, not just when explicitly queried. |
| `memory.search()` returns flat list | `hebbs.recall(Causal)` → graph traversal | Follow deal networks, referral chains, and cause-effect relationships. |
| Cloud-hosted SaaS | Embedded mode, data stays local | No cloud dependency. Sub-millisecond latency. Data sovereignty. |

The reference app should make these differentiators viscerally obvious through the scripted scenarios. A prospective customer watching Scenario D (reflect learning) or Scenario E (subscribe real-time) should immediately understand what HEBBS does that a "store and search" memory layer cannot.
