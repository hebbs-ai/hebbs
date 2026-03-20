---
title: "The Memory Palace Comes to the Homepage"
description: "HEBBS now has an interactive force-directed memory graph on the homepage, a new agent-control section showing every tunable parameter, and a sharper identity: memory that wires itself."
pubDate: 2026-03-16
tags: ["website", "memory-palace", "agents", "update"]
heroImage: "/images/memory-palace-comes-to-the-homepage.png"
ogImage: "/images/memory-palace-comes-to-the-homepage.png"
---

The HEBBS homepage got three significant changes today. Each one addresses the same gap: visitors could read about what HEBBS does, but they couldn't *see* it or *feel* why it's different.

## Interactive Memory Palace Demo

The Memory Palace is HEBBS's visualization of how memories cluster, connect, and contradict each other in real time. Until this update, the homepage only described it in a text card. Now there's a live, interactive demo directly on the page.

> **Update**: The in-app Memory Palace panel has since evolved into a bioluminescent brain visualization with organic tendrils, spreading activation, and ambient drift. The homepage demo retains the force-directed graph style described below.

The demo renders 42 pre-baked nodes organized in three clusters:

- **Sales Interactions** (15 episodes): calls, demos, security reviews, contract discussions
- **Product Knowledge** (12 episodes): docs, benchmarks, battlecards, compliance
- **Deal Insights** (8 insights + 7 episodes): patterns the engine has extracted from the episodes above

Connecting them: 55 edges. Solid amber for causal chains ("this call led to that meeting"). Dashed gray for similarity ("these two topics are semantically close"). Dashed red for contradictions ("these two memories disagree").

### What you can do with it

- **Hover** any node to see its content and type badge (EPISODE or INSIGHT)
- **Scroll** to zoom toward your cursor
- **Drag** to pan the graph
- **Touch** works too: single-finger pan, two-finger pinch-zoom

The graph uses the same force simulation as the real Memory Palace panel: repulsion between all nodes, spring attraction along edges, gravity toward center, velocity damping. The rendering is identical: cluster convex hulls, edge styles by type, hexagons for insights, circles for episodes, glow on hover.

### Performance details

An IntersectionObserver watches the section. When it scrolls out of view, the requestAnimationFrame loop pauses. When it scrolls back in, the loop resumes. No CPU cycles wasted rendering a graph nobody can see.

Entry animation staggers by cluster: Sales nodes fade in first, then Product Knowledge, then Deal Insights, over roughly two seconds. This gives the visitor a sense of structure forming before they interact.

## Agent-First Control Section

HEBBS is designed to give AI agents granular control over every recall parameter. Strategy, scoring weights, entity scope, memory kinds, time ranges, result limits. The agent reasons about what it needs and tells HEBBS exactly how to retrieve it.

The homepage now has a section that makes this explicit. It shows the anatomy of a single `recall()` call with every parameter annotated:

```python
results = e.recall(
    cue             = "why did the Acme deal fail"
    strategy        = "causal"
    entity_id       = "acme"
    scoring_weights = {"w_relevance": 0.3, "w_recency": 0.1,
                       "w_importance": 0.5, "w_reinforcement": 0.1}
    memory_kinds    = ["episode", "insight"]
    top_k           = 10
)
```

Above the code: the agent's reasoning. "User asked about a deal that failed. I need the causal chain, scoped to this entity, weighted toward importance."

Below it: four scenario cards showing how the same agent makes different parameter choices for different situations. A failed deal analysis uses causal strategy with importance weighting. A follow-up call prep uses temporal strategy with recency weighting. A pricing objection uses analogical strategy searching across all entities. A quick fact lookup uses similarity with pure relevance scoring.

At the bottom: a reference table of all eight exposed parameters, noting they're available across CLI, Python SDK, TypeScript SDK, gRPC, and REST.

## New Identity

The hero tagline changed from "The memory engine for AI agents" to something sharper:

> **Memory that wires itself.**
>
> Temporal. Causal. Analogical. Similarity. Memory palace your agent controls.

"Memory that wires itself" references Hebbian learning, the neuroscience principle HEBBS is named after: connections that get used get stronger. The subtitle lists the four recall strategies as bare words, then names the palace and the agent's role in one line.

## Other Changes

- **Install command**: `curl -sSf https://hebbs.ai/install | sh` is now the primary install in the hero, with `brew install` as a secondary option below
- **Footer**: Added hebbs.ai link
- **Removed WeightClass section**: The "Your files become memories, your memories become understanding" interstitial was redundant with the Vault section below it. Removing it tightens the flow from Hero directly into the problem statement

## What's Next

The homepage now shows the value proposition instead of just describing it. The Memory Palace demo lets visitors see clustering, causal chains, and contradictions without installing anything. The agent-control section proves that HEBBS gives agents full autonomy over how they remember. And the new identity ties it all back to the Hebbian learning principle at the core.
