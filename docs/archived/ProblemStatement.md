# Problem Statement: The HEBBS Memory Primitive

## The Context: Pre-AI vs. AI Paradigms

In the pre-AI era, developers built applications using robust primitives like PostgreSQL (relational data), Redis (predictable latency, rich data structures), and Kafka (asynchronous messaging). These primitives provided clean, composable APIs that allowed frameworks to flourish.

Currently, in the AI agent space, we are in the "framework-first" phase (LangChain, CrewAI, AutoGen). These frameworks are attempting to stitch together primitives that weren't designed for agentic workflows. Vector databases were bolted onto PostgreSQL, memory is hacked together with Redis, and tool calling is essentially RPC with extra steps.

## The Core Problem: Memory as Storage vs. Memory as Cognition

Current "memory" solutions for AI agents are the "Memcache" equivalent — they solve narrow use cases (like caching conversation history) but lack the structural elegance and cognitive depth required for true agentic intelligence.

### Limitations of Current Approaches

| Approach | What it does | What it misses |
|---|---|---|
| **Conversation History** | Append-only log, truncate at window | No importance weighting, no consolidation, brutal context cutoff. |
| **Vector DB / RAG** | Similarity retrieval over chunks | Only one retrieval path, no decay, no structural consolidation. |
| **Redis / KV Cache** | Fast storage of computed results | No semantic understanding, manual key management. |
| **Knowledge Graphs** | Structured relationships | Hard to populate automatically, rigid schema, lacks temporal context. |

## The Requirements for an Agentic Memory Primitive

A true memory primitive for agents must move beyond simple storage and incorporate cognitive patterns:

1.  **Importance-Driven Encoding:** Not every interaction is worth remembering. The primitive should understand which patterns or resolutions matter at write-time.
2.  **Multi-Path Recall:** Similarity search is only one mode. Agents need:
    *   **Temporal Recall:** "What did I try last time this failed?"
    *   **Causal Recall:** "What sequence of events led to this state?"
    *   **Analogical Recall:** "Have I seen a structurally similar problem before?"
3.  **Episodic-to-Semantic Consolidation:** The ability to compress individual experiences into general knowledge or refined procedures over time.
4.  **Native Decay and Reinforcement:** Frequently accessed memories should strengthen; stale or irrelevant ones should fade, preventing retrieval degradation.
5.  **Revision over Append:** When an agent learns a fact is wrong, it needs to *update* its internal model, not just add a contradictory entry.

## The Proposed "Redis-like" API

The winner in this space will be the one with the most elegant abstraction that developers reach for instinctively:

```javascript
// Write path
remember(experience, importance, context) → memory_id
revise(memory_id, new_evidence) → updated memory
forget(criteria) → prune by staleness, irrelevance, or contradiction

// Read path
recall(cue, strategy: similarity|temporal|causal|analogical) → memories[]
prime(context) → relevant_memories[]           // framework pre-loads context before an agent turn
subscribe(input_stream, threshold) → stream<memories>  // primitive pushes when it detects relevance

// Consolidation (background process, not a hot-path call)
reflect_policy(config) → policy_id
reflect(scope) → manual trigger for on-demand consolidation
insights(filter) → query consolidated knowledge
```

### A Note on `recall`: Three Callers, Not One

`recall` is not a single interaction pattern — it serves three distinct callers:

1. **The Framework (Automatic):** Before every agent turn, the orchestrator calls `prime()` to load structural context (e.g., prospect history before a sales call). Deterministic, always happens.
2. **The Agent (Deliberate):** Mid-task, the agent decides it needs information and explicitly calls `recall()` as a tool. This requires the agent to know it doesn't know — a metacognitive gap.
3. **The Primitive Itself (Associative):** Via `subscribe()`, the primitive monitors an input stream and proactively surfaces relevant memories when a pattern match crosses a confidence threshold. The agent didn't ask — the knowledge just appears. This is how human associative memory works.

Memory is not just a tool the agent calls. It is an **environment** the agent operates within.

### A Note on `reflect`: A Policy, Not a Function Call

`reflect` is the most misunderstood operation. It is not something the agent calls in a hot path — it is a **background consolidation process** the primitive manages, analogous to compaction in a database.

**Why it can't be a simple function call:** Consolidating hundreds of episodes into distilled knowledge is computationally expensive, requires cross-memory pattern extraction, and produces outputs consumed later through `recall` and `insights` — not immediately.

**Triggers should be configurable as a policy:**

```javascript
reflect_policy({
  triggers: [
    { type: "threshold", new_memories: 50 },           // after N new memories
    { type: "schedule", interval: "daily" },            // time-based
    { type: "recall_failure", confidence_below: 0.3 },  // demand-driven: reflect when recall can't find good answers
    { type: "metric_drift", metric: "conversion_rate", delta: 0.2 }  // outcome-driven: reflect when performance shifts
  ],
  strategy: "hybrid",      // statistical clustering + LLM validation
  scope: "incremental"     // don't reprocess all memories, only new since last run
})
```

**The execution is a pipeline, not a single model call:**

1. Statistical engine clusters episodes and identifies frequency patterns.
2. Lightweight model proposes candidate insights from clusters.
3. Stronger model validates, refines, and resolves contradictions.
4. Consolidated knowledge is stored with lineage back to source episodes.

The agent consumes the *output* of reflection through `recall` (which now returns consolidated knowledge alongside raw episodes) and `insights` (which queries distilled patterns directly).

## Conclusion

The next decade of the AI stack will be defined by whoever builds the "Postgres/Redis/Kafka" equivalents purpose-built for agent cognition. Memory is the HEBBS primitive—it is the foundation upon which planning, tool execution, and multi-agent communication will be built.
