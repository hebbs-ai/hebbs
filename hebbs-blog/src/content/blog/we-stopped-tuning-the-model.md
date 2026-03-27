---
title: "We Stopped Tuning the Model. We Tuned the Memory."
description: "Everyone's chasing bigger models and longer context. We tried a different lever: tuning how the agent retrieves its own memories. 75% recall jumped to 94%. Same LLM. Same data. Just better wiring."
pubDate: 2026-03-27
tags: ["engineering", "agents", "memory", "tuning"]
heroImage: "/images/we-stopped-tuning-the-model.png"
ogImage: "/images/we-stopped-tuning-the-model.png"
---

There's a lever nobody is pulling.

The entire AI industry is focused on making models smarter. Longer context windows, better fine-tuning, more RLHF, bigger parameter counts. And it works. Models keep getting better. But for agents that need to recall their own stored knowledge, the model was never the bottleneck.

The bottleneck is how the agent accesses its memory.

We proved it. Same LLM, same stored memories, same corpus. The only variable we changed was how retrieval was configured: which strategy the agent picked, how results were scored, what weights were applied to each dimension. Keyword recall went from 75% to 94%. No model upgrade. No additional data. No fine-tuning. Just better wiring.

## The Experiment

We took 52 legal documents (contracts, board minutes, vendor assessments, compliance policies, risk registers, incident reports) and indexed them into HEBBS. 949 memories across dozens of entities.

Then we wrote 20 evaluation queries. Not synthetic benchmarks. Real questions a legal team would actually ask: "What are the data retention obligations across all vendor contracts?" "Which board resolutions from Q4 contradict the current compliance policy?" "What changed in the risk register after the last audit?"

Each query had 3-5 expected keywords. Strict matching. No partial credit. If the retrieved results didn't contain the specific facts, it failed.

With default retrieval settings (similarity search, k=5, default weights), the system hit 75% keyword recall. That's decent. Better than most memory systems out of the box.

But 75% means one in four facts is missing. In legal, that's the fact that costs you.

## What We Tuned (and What We Didn't)

We didn't touch the model. We didn't change the embeddings. We didn't add more data. We changed four things about how the agent retrieves:

**1. Strategy selection.** Not every query is a semantic similarity search. "What changed after the audit?" is a temporal query. "Which vendors have contradicting terms?" is a cross-entity comparison. The right strategy depends on what you're asking. Default settings use similarity for everything. That's like using a hammer for every tool.

**2. Scoring weights.** HEBBS scores every memory across four dimensions: semantic similarity, recency, importance, and access frequency. The default blend is balanced. But a compliance query should weight importance heavily. A "what happened recently" query should weight recency. The optimal blend changes with every question.

![Default retrieval uses one fixed configuration for every query. Tuned retrieval adapts the weights per query type.](/images/retrieval-faders-default-vs-tuned.png)

**3. Query construction.** "SOC2 policy" retrieves vaguely related results. "SOC 2 Type II audit findings access controls Cloudvault" retrieves exactly what you need. Expanding acronyms, including entity names, and adding specifics to the query made the single biggest difference.

**4. Result depth.** Default k=5 returns five results. For simple factual lookups, that's enough. For cross-entity comparisons, the supporting evidence lives in results 6 through 10. Adjusting k per query type closed the remaining gaps.

That's it. Four knobs. No model changes.

## 75% to 94%

After tuning those four parameters across the 20 evaluation queries, keyword recall jumped from 75% to 94%.

The breakdown of what each fix contributed:

- **Query construction** (expanding cues, adding entity names): biggest single lever. Turned vague queries into precise ones.
- **Result depth** (k=5 to k=10 for complex queries): second biggest. Supporting facts were there, just below the default cutoff.
- **Strategy selection** (temporal, analogical instead of pure similarity): fixed the queries where similarity was fundamentally wrong.
- **Weight tuning** (recency-heavy for timeline queries, importance-heavy for compliance): refinement that closed the last gaps.

## The Part That Matters

Here is where it gets interesting. The agent did all of this itself.

We didn't hand-tune 20 queries. We built a tuning skill that lets the agent run its own eval loop. It profiles the domain, generates evaluation queries from the actual corpus, runs baselines, classifies why each failure happened, applies fixes, and re-scores.

Then it stores what it learned. Not as code. As memories.

```
RETRIEVAL-INSTRUCTION: For compliance queries, always expand
acronyms and include the vendor name in the cue. Use k=10 minimum.
```

```
RETRIEVAL-INSTRUCTION: For timeline queries, use temporal strategy
with recency weights 0.3:0.5:0.2:0. Always scope to entity-id.
```

```
MASTER-RULE: Default k=10 for all non-trivial queries. Only use
k=5 for simple factual lookups with unique entity names.
```

These retrieval instructions get recalled before every query in every future conversation. The agent loads its own playbook, then applies it. No human intervention.

After 2-3 tuning sessions, the agent compresses its individual strategies into master rules and exports them to a markdown file that loads into context before the first tool call. The memory system learns how to be used, and that learning persists.

## Why Nobody Else Can Do This

Every other memory system in the agent space is a black box. Memories go in through an API. Results come back through the same API. You don't control the strategy. You don't control the scoring. You don't control the weights. You can't measure recall precision. You can't run evals. You can't tune.

If the retrieval is bad, your only option is to hope the next version of their service is better.

HEBBS exposes every knob. The agent sees the parameters, measures the results, and optimizes the configuration for its specific domain. This is only possible when the infrastructure is designed for agent control from the ground up. You cannot bolt tuning onto a black box.

## The Compounding Effect

This is not a one-time optimization. The tuning loop runs continuously.

![The tuning flywheel: Use, Measure, Tune, Compile, Load. Every conversation, it gets better.](/images/tuning-loop-flywheel.png)

Every conversation generates signal: which queries worked, which missed, what the agent had to fall back on. That signal feeds the next tuning cycle. Strategies that work get reinforced (HEBBS uses Hebbian reinforcement, memories that get recalled grow stronger). Strategies that stop working decay naturally.

After a month, the agent's retrieval policy is shaped by hundreds of real interactions with its specific corpus and its specific users. No two agents end up with the same policy, because no two agents operate in the same context.

This is what we mean by "memory that wires itself." The agent doesn't just accumulate facts. It accumulates retrieval wisdom. It learns not just what to remember, but how to remember it.

## Try It

```bash
brew install hebbs-ai/tap/hebbs
hebbs init .
hebbs index .
```

Index your files. Run the tune skill. Watch the numbers climb.

The bottleneck was never the brain. It was the recall.
