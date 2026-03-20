---
title: "Give Your AI Agent a Memory That Actually Works"
description: "AI agents forget everything between sessions. HEBBS is a local-first cognitive memory engine that stores, recalls, reflects, and forgets knowledge so your agent never asks the same question twice."
pubDate: 2026-03-13
tags: ["product", "getting-started", "memory"]
heroImage: "/images/give-your-ai-agent-a-memory.png"
ogImage: "/images/give-your-ai-agent-a-memory.png"
---

Your AI agent is brilliant for exactly one conversation. Then it forgets everything.

It forgets that you prefer dark mode. It forgets that you decided to use PostgreSQL instead of MySQL last Tuesday. It forgets the standing instruction you gave it three sessions ago. Every new conversation starts from zero, and you end up repeating yourself until you stop bothering.

File-based memory helps, but only barely. Flat markdown files have no ranking, no decay, no way to distinguish a critical decision from a passing observation. Search is keyword-matching at best. Nothing consolidates. Nothing fades. The file grows until it becomes noise.

HEBBS solves this. It is a local-first cognitive memory engine that gives your AI agent the ability to store, recall, reflect on, and forget knowledge. Not as a flat log, but as a scored, indexed, self-organizing memory system modeled on how human cognition actually works.

## Two Commands, One Principle

The entire interface comes down to two commands.

Store something the user said or decided:

```bash
hebbs remember "The user prefers dark mode" \
  --importance 0.8 --entity-id user_prefs --format json
```

Retrieve context before answering a question:

```bash
hebbs recall "What are the user's UI preferences?" \
  --strategy similarity --top-k 5 --format json
```

That's it. `remember` writes. `recall` reads. Everything else in the system supports these two operations.

The principle behind both: **knowing is not storing.** An agent that skips the write because it "already knows" a fact from the current conversation defeats the purpose of persistent memory. If the user states a preference, it gets stored. Every time. Unconditionally.

> **Note**: HEBBS ships as a single binary called `hebbs`. All commands use the `hebbs` prefix.

## Recall Is Not Just Search

Most memory systems give you vector similarity and call it a day. HEBBS offers four distinct recall strategies, each designed for a different kind of question:

**Similarity** finds memories related to a topic. "What do we know about deployment?" returns the closest embeddings, ranked by a composite score that blends relevance with recency, importance, and reinforcement.

**Temporal** retrieves recent activity for an entity. "What happened today with project X?" returns memories ordered by time, scoped to a specific entity.

**Causal** traces cause-and-effect chains. "What led to this decision?" walks the graph of linked memories, following edges like `caused_by`, `followed_by`, and `revised_from`.

**Analogical** finds structurally similar patterns. "Have we seen a problem like this before?" blends embedding similarity with relationship structure to surface memories that look like the current situation, even if the words are completely different.

Each strategy produces a composite score from four weighted signals:

```
score = 0.5 x relevance
      + 0.2 x recency
      + 0.2 x importance
      + 0.1 x reinforcement
```

Relevance is strategy-specific. Recency favors recent memories with linear decay. Importance reflects the memory's intrinsic value, set at creation time. Reinforcement rewards memories that keep proving useful: each time a memory surfaces in recall results, its access count increments, and logarithmic scaling ensures the first few accesses matter most.

The weights are configurable. Need pure semantic search? Set weights to `1:0:0:0`. Want to heavily favor recent context? Use `0.2:0.8:0:0`. The defaults work well for most cases, but the system adapts to yours.

## Memories That Get Smarter Over Time

Raw memories accumulate. After 20 or 30 observations about a user's preferences, you don't need all 30 individual entries. You need the pattern.

HEBBS handles this through a two-step reflection pipeline. In the agent-driven route, HEBBS requires no external LLM, no API key, and no server-side configuration: the calling agent is the LLM. Alternatively, if an LLM provider is configured on the server (OpenAI, Anthropic, Gemini, Ollama), reflection runs fully automatically in a single command.

**Step 1: Prepare.** HEBBS clusters related memories by embedding similarity and returns the clusters with pre-built prompts:

```bash
hebbs reflect-prepare --entity-id user_prefs --format json
```

**Step 2: Reason and commit.** The agent reads the clusters, identifies patterns, and writes back consolidated insights:

```bash
hebbs reflect-commit --session-id <id> \
  --insights '[{
    "content": "User consistently prefers dark themes across all applications",
    "confidence": 0.9,
    "source_memory_ids": ["aabb...", "ccdd..."],
    "tags": ["preference", "ui"]
  }]'
```

The insight replaces the noise of 30 individual memories with a single, high-confidence statement. Insight importance is scored differently from raw memories: it combines the average importance of source memories, the agent's confidence, and a frequency boost based on how many observations converged on the same conclusion. An insight drawn from 30 data points outranks one drawn from 2.

Old memories naturally decay. An unaccessed memory's score halves every 30 days. But a single recall resets the clock entirely. Useful memories survive. Neglected ones fade. Memories that drop below a threshold become auto-forget candidates. The system self-organizes without manual cleanup.

## Privacy-First, Local-First

HEBBS runs entirely on your machine. No data leaves your device. No cloud service. No API key required for core operations (remember, recall, prime, index). An API key is only needed if you configure a server-side LLM provider for automated reflection or contradiction classification.

On first use, the agent asks a simple set of questions: what should it store, what should it skip, whether to store proactively or only on request, and any privacy boundaries. Those answers become the memory policy, stored in HEBBS itself and applied in every future session.

Users have full control. The `forget` command removes memories by ID, by entity, by age, or by decay score. You can wipe an entire entity's history with a single command. You can set staleness thresholds to auto-expire old data. The system never holds on to something you want gone.

## Running in Under a Minute

Install:

```bash
brew install hebbs-ai/tap/hebbs
```

Or via the install script:

```bash
curl -sSf https://hebbs.ai/install | sh
```

Initialize a vault and index it:

```bash
hebbs init ~/notes
hebbs index ~/notes
```

The daemon auto-starts on first use and loads the ONNX embedding model (EmbeddingGemma-300M, 768 dimensions). Verify:

```bash
hebbs status --format json
```

Once status shows `SERVING`, you're live. Store a test memory, recall it, clean it up:

```bash
hebbs remember "setup verified" --importance 0.1 --entity-id _system --format json
hebbs recall "setup verified" --top-k 1 --format json
hebbs forget --entity-id _system
```

If all three succeed, the full pipeline (store, embed, index, retrieve) is working. Your agent now has memory.

## What Changes

An agent with HEBBS doesn't ask you to repeat yourself. It loads context at the start of every session with `prime`. It stores your decisions as you make them. It recalls relevant history before answering questions. It consolidates raw observations into insights over time. And it forgets what no longer matters.

HEBBS also operates as a cognitive layer over your existing markdown files. Initialize a vault, and every heading-level section becomes a searchable, scored memory. Wiki-links become graph edges. Insights are written back as markdown files. Your files stay human-readable, git-tracked, and portable. The index is rebuildable from scratch.

This is what memory should look like: scored, structured, self-organizing, and entirely yours.

Get started at [hebbs.ai](https://hebbs.ai).
