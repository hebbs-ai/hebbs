# Blog: HEBBS Now Detects Contradictions in Your Knowledge Base

**Status:** To write
**Related:** TASK-12 Feature 2, PLAN-contradiction, TASK-20 Item 10

## What to communicate

HEBBS now automatically detects when memories in your vault contradict each other. Works out of the box with zero config (heuristic mode). Add an LLM API key for higher-quality detection. Contradictions appear as red edges in the Memory Palace panel.

### Key points

- **Zero-config contradiction detection**: enabled by default on every `hebbs index` and `hebbs watch` run. No LLM needed. Heuristic mode catches obvious contradictions using negation asymmetry, antonym pairs, and numeric disagreement.
- **LLM mode auto-activates**: if your vault config has a reflect API key and model, contradiction detection automatically upgrades to LLM-powered entailment classification. No separate config needed. Remove the key, it falls back silently.
- **Incremental, not N^2**: each new memory is checked against its top-K nearest neighbors (default K=10) using the existing HNSW index. Cost per new memory: O(log n) search + O(K) classification. A 10,000-memory vault adds ~10ms per ingest in heuristic mode.
- **Bidirectional CONTRADICTS edges**: when a contradiction is detected, edges are created in both directions. Both memories know about the conflict.
- **Never auto-resolves**: contradictions are flagged, never silently resolved. The user or agent decides: dismiss, mark as revision, or delete one.
- **Revision detection**: the classifier distinguishes "I used to think X, now I think Y" (revision) from genuine contradiction. Temporal markers like "previously", "updated", "used to" bias toward revision, not contradiction.
- **Visible in Memory Palace**: CONTRADICTS edges render as red dashed lines in the graph view. Immediately visible which areas of your knowledge base are in tension.
- **Contradiction files in your vault**: each detected contradiction writes a `.md` file to `contradictions/` with both memory contents, source paths, confidence, and classification method. Human-readable, deletable, git-trackable. The watcher ignores this directory so contradiction files are never re-ingested.

### Why this matters

No Obsidian plugin, note-taking tool, or AI memory system does this today. Knowledge bases accumulate contradictions silently over time. A January note says "vendor X is reliable." An August note says "vendor X missed three deadlines." No one catches this until it causes a bad decision.

For AI agents, contradictions in memory are actively dangerous. An agent that retrieves contradictory context will produce incoherent output. HEBBS catches this at ingest time so agents always work with consistent knowledge.

### How it works (technical)

1. New memory is ingested via `engine.remember()`
2. HNSW search finds the top-K semantically similar existing memories (O(log n))
3. Each candidate pair is classified: CONTRADICTION, REVISION, or NEUTRAL
4. Contradictions above the confidence threshold become bidirectional `CONTRADICTS` edges in the graph
5. A `.md` file is written to `contradictions/` with both contents, sources, and confidence
6. Panel renders these as red dashed lines

**Heuristic mode signals:**
- Negation asymmetry: one memory asserts, the other negates ("delivered on time" vs "failed to deliver")
- Antonym pairs: reliable/unreliable, success/failure, increase/decrease, etc. (25 pairs)
- Numeric disagreement: same context, different numbers ("3 errors" vs "150 errors")
- Confidence capped at 0.75 to reflect reduced accuracy vs LLM

**LLM mode:**
- Structured entailment prompt with JSON output
- Understands nuance, temporal context, soft contradictions
- Confidence up to 1.0
- Uses existing reflect LLM provider (no new API integration)

### Config

```toml
[contradiction]
enabled = true           # default: true
candidates_k = 10        # neighbors to check per memory
min_similarity = 0.7     # skip dissimilar pairs
min_confidence = 0.7     # threshold to create edge

[output]
contradiction_dir = "contradictions/"   # where .md files go
```

LLM mode activates when `[reflect_llm]` section has `provider` and `model`.

### Audience

- Obsidian/PKM users who want knowledge consistency checking
- AI agent builders who need contradiction-safe retrieval
- Teams using HEBBS as shared knowledge infrastructure
- Anyone who has been burned by stale/conflicting information in their notes

### Messaging angles

1. **"Your notes disagree with each other."** Most note-taking tools treat every note as truth. HEBBS treats them as claims and checks consistency.
2. **"Works offline, scales with LLM."** Heuristic mode needs nothing. LLM mode is one config line away. Same API, same edges, same panel view.
3. **"Incremental, not batch."** Catches contradictions as they're created, not in a weekly scan.
4. **"Flags, never fixes."** Contradiction resolution is a human/agent decision, not an automated one. HEBBS surfaces the conflict. You resolve it.
