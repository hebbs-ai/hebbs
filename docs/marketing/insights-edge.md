# The Two Structural Edges: Agent-Controlled Retrieval and Autotunable Evals

## Edge 1: Agent-Controlled Retrieval

### The insight

Every other memory layer makes the retrieval decisions for the agent. The agent calls `memory.search("query")` and gets back whatever the system thinks is relevant. The agent is a passive consumer.

Hebbs flips this. The agent decides:
- Which strategy to use (similarity, temporal, causal, analogical)
- How to weight the four scoring dimensions (relevance, recency, importance, frequency)
- How deep to traverse causal chains
- Which edge types to follow
- How to blend structural vs. embedding similarity in analogical queries

This means the agent is the cognitive actor. The memory engine is the instrument it plays.

### Why this matters now

Baking opinion into the memory layer is actively losing edge. As agents get smarter (better tool use, better reasoning about when to use which tool), the value shifts from "smart defaults" to "full control." A smart agent with a dumb-but-controllable engine will outperform a dumb agent with a smart-but-opaque engine.

The industry is converging on agents that reason about *how* to retrieve, not just *what* to retrieve:
- "What changed since Q3?" requires temporal strategy with high recency weight
- "What caused this incident?" requires causal edge traversal
- "Has the user said anything about this before?" requires high importance and frequency weights
- "What's analogous to this pattern?" requires analogical strategy with structural blending

Mem0 and Supermemory can't do any of this. They have one mode: similarity search. The agent gets whatever the system thinks is "most similar." That's it.

### The pitch line

"Other memory tools treat your agent like a user typing into a search bar. Hebbs treats your agent like a systems engineer with full parameter control over a cognitive retrieval engine."

### The enabler

This edge only works if agents actually use the parameters well. The SKILL.md is the moat here. It teaches agents:
- When to use which strategy
- What weight profiles work for which retrieval goals
- Concrete examples: `0.3:0.1:0.5:0.1` for high-importance preferences, `0.2:0.8:0:0` for most-recent-first

Without a well-written skill file, agents default to `hebbs recall "query"` every time and you lose the advantage. The skill file is as important as the engine.

---

## Edge 2: Autotunable with Evals

### The insight

Memory retrieval quality is measurable but nobody is measuring it. The optimal retrieval configuration for a legal compliance agent is completely different from a sales agent or a coding assistant. Today, everyone is guessing at their parameters or accepting defaults they can't change.

Because Hebbs exposes every parameter, enterprises can:

1. Generate thousands of synthetic eval scenarios for their specific domain
   - Each eval: a query paired with the known correct memories that should be returned
   - Scenarios simulate real usage patterns: multi-hop questions, temporal queries, contradiction detection, cross-entity synthesis

2. Sweep the parameter space systematically
   - Strategy selection per query type
   - Weight ratio optimization (relevance vs. recency vs. importance vs. frequency)
   - top-k tuning
   - ef-search quality parameter
   - Analogical alpha blending
   - Edge type filtering for causal queries

3. Measure precision and recall per configuration against ground truth
   - Precision: are the returned memories actually relevant?
   - Recall: are all the relevant memories being returned?
   - Per-strategy breakdowns
   - Per-query-type breakdowns

4. Lock in the optimal config for their use case
   - Different configs for different query types within the same application
   - Reproducible results: same weights, same results, every time
   - Regression testing in CI

### Why competitors can't do this

You can't autotune what you can't control.

| Dimension | Mem0 / Supermemory | Hebbs |
|---|---|---|
| Retrieval parameters | None exposed. Black box. | 4 strategies, 4 weight dimensions, edge traversal depth, ef-search, analogical alpha |
| Eval capability | Can only test input/output. Can't vary retrieval behavior. | Sweep every parameter per query type |
| Tuning workflow | Hope the defaults work. File a feature request if they don't. | Autotune to your domain with measurable results |
| Reproducibility | Opaque ranking that may change across versions | Deterministic: same weights, same results |
| CI integration | Not possible | Run eval suite as part of deployment pipeline |

### The product opportunity

A `hebbs eval` command:

```bash
hebbs eval --suite legal-compliance.json --sweep weights --output results.csv
```

The eval suite is a JSON file of (query, expected_memory_ids, query_type) triples. The command:
- Runs each query against the indexed vault
- Sweeps the parameter space (or uses a specified config)
- Outputs precision/recall/F1 per configuration
- Recommends optimal weights per query type

This is the kind of artifact a VP of Engineering signs off on. Not "our memory is better" but "here's the eval report proving our retrieval hits 94% precision on your domain after autotuning."

### The enterprise workflow

```
1. Ingest domain documents (contracts, policies, call notes)
2. Generate eval suite (manually or with LLM assistance)
3. Run: hebbs eval --suite evals.json --sweep weights
4. Review results, pick optimal config per query type
5. Deploy agent with tuned configs
6. Add evals to CI: hebbs eval --suite evals.json --config tuned.json --threshold 0.90
7. Any retrieval regression blocks deployment
```

This positions Hebbs not as "better memory" but as memory infrastructure that enterprises can engineer against. Enterprise teams don't want magic. They want measurable retrieval quality, reproducible results, tunable parameters, and eval suites in CI.

### The pitch line

"Mem0 and Supermemory are products you use. Hebbs is infrastructure you tune."

---

## How These Two Edges Compound

Edge 1 (agent-controlled retrieval) gives agents the knobs. Edge 2 (autotunable evals) tells you which settings to put the knobs on.

Together they create a flywheel:
1. Agent uses parameterized retrieval in production
2. Log the queries, strategies, and weights the agent chose
3. Generate evals from production query patterns
4. Autotune parameters against those evals
5. Feed tuned configs back to the agent (or let the agent learn which configs work)
6. Agent gets smarter at choosing parameters over time

No competitor can enter this loop because they don't expose the parameters in step 1.

This is the real moat. Not the engine alone, not the evals alone, but the closed loop between agent cognition and measurable retrieval optimization.
