---
title: "Reflect and Contradict: How HEBBS Consolidates Knowledge Without an LLM"
description: "Two pipelines that make memory smarter over time. Reflection turns episodes into insights. Contradiction detection catches conflicting beliefs. Both work with or without an LLM."
pubDate: 2026-03-16
tags: ["engineering", "deep-dive", "agents", "reflection", "contradictions"]
heroImage: "/images/reflect-and-contradict.png"
ogImage: "/images/reflect-and-contradict.png"
---

Raw memories accumulate. After 40 conversations about a customer's deployment preferences, your agent has 40 individual observations. Some agree. Some conflict. None of them tell you what the pattern is.

Two pipelines solve this. Reflection consolidates raw episodes into higher-order insights. Contradiction detection catches when two memories assert opposing facts. Both run inside HEBBS with zero external dependencies. Both offer two routes: a fully automatic LLM-powered path, and an agent-driven path where the calling agent is the LLM.

## Reflection: Episodes Become Insights

An agent that stores 30 memories about a user's UI preferences doesn't need all 30 at recall time. It needs: "User consistently prefers dark mode, monospace fonts, and minimal chrome." That's an insight.

### The LLM Route

If you have an LLM provider configured on the server (OpenAI, Anthropic, Gemini, Ollama), reflection is a single command:

```bash
hebbs reflect --entity-id user_prefs
```

The server handles everything:

```
Memories → K-means clustering → LLM proposal → LLM validation → Insight storage
```

1. Collects up to 500 episodes (scoped by entity or global)
2. Clusters their embeddings via K-means (min 5 memories, max 50 clusters)
3. For each cluster, generates an LLM prompt with all source memories
4. The LLM proposes an insight
5. A second LLM call validates the insight against the source memories
6. Validated insights are stored as `Memory(kind=INSIGHT)` with `INSIGHT_FROM` edges back to source episodes

This is the simplest path. One command, fully automated, results in seconds. But it requires a server-side LLM, which means API keys, network calls, and cost.

### The Agent Route

HEBBS has no LLM. The daemon is a single binary that runs locally with zero external dependencies. When there's no server-side LLM configured, or when the agent wants full control over reasoning, the agent becomes the LLM.

Two steps. Prepare, then commit.

**Step 1: Prepare.** HEBBS clusters the memories and builds prompts, but calls no LLM.

```bash
hebbs reflect-prepare --entity-id user_prefs --format json
```

The response contains everything the agent needs to reason:

```json
{
  "session_id": "01JQXYZ...",
  "memories_processed": 47,
  "clusters": [
    {
      "cluster_id": 0,
      "member_count": 8,
      "proposal_system_prompt": "You are analyzing a cluster of related memories...",
      "proposal_user_prompt": "Here are the memories:\n1. User prefers dark mode\n2. ...",
      "memory_ids": ["aabb1122...", "ccdd3344..."],
      "memories": [
        {
          "memory_id": "aabb1122...",
          "content": "User prefers dark mode in all applications",
          "importance": 0.8,
          "created_at": 1720000000000000
        }
      ]
    }
  ]
}
```

The `proposal_system_prompt` and `proposal_user_prompt` are ready to send to any LLM. But the agent doesn't have to use them. It can read `memories[]` directly and reason however it wants.

**Step 2: Commit.** The agent writes back its insights.

```bash
hebbs reflect-commit --session-id "01JQXYZ..." \
  --insights '[{
    "content": "User consistently prefers dark mode across all applications",
    "confidence": 0.9,
    "source_memory_ids": ["aabb1122...", "ccdd3344..."],
    "tags": ["preference", "ui"]
  }]'
```

HEBBS stores each insight as a `Memory(kind=INSIGHT)`, creates `INSIGHT_FROM` edges to the source episodes, and the insight is immediately available in future recall and prime results.

Sessions expire after 10 minutes. Source memory IDs must come from the prepare output.

### What the Agent Gets

In code, the pattern looks like this:

```python
# Agent-driven reflect
prep = await hebbs.reflect_prepare(entity_id="acme_corp")

insights = []
for cluster in prep.clusters:
    # The agent reads the source memories
    # The agent reasons about patterns
    # The agent decides what insight to extract
    insights.append(ProducedInsightInput(
        content="Acme prioritizes compliance over cost in vendor decisions",
        confidence=0.9,
        source_memory_ids=cluster.memory_ids[:3],
        tags=["priority", "compliance"]
    ))

result = await hebbs.reflect_commit(prep.session_id, insights)
# result.insights_created == 1
```

A new agent talking to Acme for the first time gets institutional knowledge from day one, because `prime()` and `recall()` return insights alongside raw episodes.

### Insight Scoring

Insights aren't just summaries. They carry weight. An insight's importance combines:

- Average importance of source memories
- The agent's stated confidence
- Frequency boost: how many observations converged on the same conclusion

An insight drawn from 30 data points outranks one drawn from 2. This means high-quality, well-sourced insights naturally dominate recall results over individual episodes.

## Contradiction Detection: Catching Conflicting Beliefs

Your agent stores "Budget is $5,000 per tenant" in January. In March, someone says "Budget is $2,000 per tenant." Both memories exist. Both are valid recalls. The agent has no idea they conflict.

HEBBS catches this with a two-phase contradiction pipeline.

### Phase 1: Heuristic Detection (Automatic)

During every ingest (indexing, watch, or remember), HEBBS runs a lightweight heuristic classifier against semantically similar existing memories:

```
New memory → Find top-K nearest neighbors → Similarity filter → Heuristic classify → Store pending
```

The heuristic classifier uses three signals:

**Negation asymmetry.** One statement uses negation words ("not", "didn't", "never", "failed") and the other doesn't. Asymmetric negation between semantically similar statements is a strong contradiction signal.

**Antonym detection.** 37 built-in antonym pairs: reliable/unreliable, success/failure, safe/unsafe, fast/slow, increase/decrease, and more. Each detected pair increases confidence.

**Numeric disagreement.** Both statements contain numbers in a similar context but the numbers differ significantly. "$5,000 per tenant" vs "$2,000 per tenant" triggers this.

Heuristic confidence is capped at 0.75. A heuristic alone cannot be certain. It can only flag candidates.

Candidates that pass both `min_similarity` (default: 0.5) and `min_confidence` (default: 0.35) thresholds are stored as `PendingContradiction` records. No edges are created. No reports are written. The candidates wait for review.

### Phase 2: Agent Review (On-Demand)

HEBBS has no LLM. It cannot determine whether a flagged pair is a real contradiction, a revision, or a false positive. The calling agent is the LLM.

**Step 1: Prepare.** Retrieve all pending candidates.

```bash
hebbs contradiction-prepare --format json
```

```json
[
  {
    "pending_id": "abc123...",
    "memory_id_a": "aabb1122...",
    "memory_id_b": "ccdd3344...",
    "content_a_snippet": "Budget is $5,000 per tenant",
    "content_b_snippet": "Budget is $2,000 per tenant",
    "classifier_score": 0.65,
    "classifier_method": "heuristic",
    "similarity": 0.82
  }
]
```

Each candidate includes both content snippets, the classifier's confidence score, which classifier flagged it, and the cosine similarity between the two memories. The agent has everything it needs to make a judgment.

**Step 2: Commit.** The agent reviews each candidate and submits a verdict.

```bash
hebbs contradiction-commit --verdicts '[
  {"pending_id": "abc123...", "verdict": "contradiction", "confidence": 0.9,
   "reasoning": "Budget changed from $5k to $2k per tenant"},
  {"pending_id": "def456...", "verdict": "revision", "confidence": 0.85,
   "reasoning": "Updated deployment timeline"},
  {"pending_id": "ghi789...", "verdict": "dismiss", "confidence": 0.95,
   "reasoning": "Different topics, not a real conflict"}
]'
```

Three verdict types:

| Verdict | What happens |
|---|---|
| `contradiction` | Creates bidirectional `CONTRADICTS` edges. Both memories are flagged. Red lines appear in Memory Palace. |
| `revision` | Creates a `REVISED_FROM` edge. Memory B supersedes Memory A. The newer information takes precedence in recall. |
| `dismiss` | Removes the candidate. No edges created. The heuristic was wrong. |

### The LLM Route for Contradictions

If an LLM provider is configured on the server, HEBBS can also run contradiction classification through the LLM during ingest. The LLM receives both statements and returns a structured classification (CONTRADICTION, REVISION, or NEUTRAL) with full confidence range up to 1.0.

This path skips the pending stage entirely: the LLM's verdict is applied immediately during ingest, creating edges in real-time.

The heuristic + agent path exists for the common case: running HEBBS locally with no LLM server, where the calling agent (Claude, GPT, etc.) provides the reasoning at its own pace.

### In Code

```python
# Agent reviews contradictions after priming
pending = await hebbs.contradiction_prepare()

verdicts = []
for p in pending:
    # Agent reads both snippets
    # Agent decides: contradiction, revision, or dismiss
    verdicts.append(ContradictionVerdictInput(
        pending_id=p.pending_id,
        verdict="contradiction",
        confidence=0.9,
        reasoning=f"'{p.content_a_snippet}' directly conflicts with '{p.content_b_snippet}'"
    ))

if verdicts:
    result = await hebbs.contradiction_commit(verdicts)
    # result.contradictions_confirmed, result.revisions_created, result.dismissed
```

### When to Run It

The SKILL.md teaches agents to review contradictions:

- After priming at conversation start (silently)
- After storing 5+ memories in a session
- When the user asks about conflicts
- Silently. Only tell the user if a real contradiction affects their current work.

## Two Routes, Same Graph

Both pipelines share a critical property: they produce graph edges.

Reflection creates `INSIGHT_FROM` edges linking insights to source episodes. Contradiction detection creates `CONTRADICTS` edges (or `REVISED_FROM` for revisions). Both edge types are first-class citizens in the memory graph.

This means causal recall can traverse them. `hebbs recall --strategy causal --edge-types contradicts` walks the contradiction graph. `hebbs recall --strategy causal --edge-types insight_from` traces an insight back to its source evidence.

The Memory Palace visualizes all of it. Insights appear as consolidated nodes. Contradictions appear as red dashed lines. The agent and the user see the same picture.

```
Episodes ──→ Reflect ──→ Insights
    │                       │
    │                   INSIGHT_FROM edges
    │
    ├── Contradict ──→ CONTRADICTS edges (bidirectional)
    │
    └── Contradict ──→ REVISED_FROM edges (directional)
```

## Why the Agent Route Matters

The agent-driven path isn't a fallback. It's the primary design.

HEBBS is a single binary with no external dependencies. It runs on laptops, CI runners, air-gapped networks, and embedded devices. No API keys. No network calls. No cost per operation.

When an agent like Claude or GPT uses HEBBS, the agent already has an LLM: itself. The prepare/commit pattern lets the agent use its own reasoning capabilities to generate insights and review contradictions. The agent sees the raw data, applies its own judgment, and writes back structured results.

This also means the agent controls quality. A Claude Opus agent reviewing contradictions will produce different (possibly better) verdicts than a server-side GPT-4o-mini call. The agent can apply conversation context, user preferences, and domain knowledge that a generic server-side LLM prompt cannot.

Both routes produce the same output: typed edges in a scored memory graph. The quality of the reasoning is the only difference. Pick the route that matches your deployment.
