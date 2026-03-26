# HEBBS: The Memory Engine for AI Agents

---

## The Problem

AI agents forget everything between sessions. Teams building persistent agents are forced to hack around this: stuffing context windows, re-prompting with summaries, or bolting on vector databases that retrieve text by similarity instead of understanding.

The result: agents that parrot back recent messages instead of reasoning over what they know. They can't notice patterns, detect contradictions, or build understanding over time. Your agent doesn't have memory. It has search.

---

## What HEBBS Does

HEBBS is a memory engine, not a memory store. It gives your agent the ability to learn, reason, and build understanding across sessions, automatically.

**Structured Knowledge, Not Vector Slop**
Every memory is decomposed into propositions with typed relationships. Your agent doesn't search for "similar text." It traverses a knowledge graph. Ask "what does this customer care about?" and get a synthesized answer drawn from 50 interactions, not a list of 5 chunks ranked by cosine distance.

**Background Reasoning**
A reflection pipeline runs asynchronously: scoring memories by relevance, recency, and access frequency. It detects contradictions ("user praised the product in January, filed 3 complaints in February") and surfaces insights the agent never explicitly asked for. Memory gets smarter without the agent doing extra work.

**Contradiction Detection**
When new information conflicts with existing knowledge, HEBBS flags it. Your agent can ask the user for clarification instead of silently overwriting history. This is the difference between a chatbot and an employee.

**Recall Strategies, Not Just Search**
Multiple retrieval modes: semantic similarity, temporal recency, graph traversal, and scoped filtering by entity, kind, or time range. Your agent picks the right recall strategy for the question, not a one-size-fits-all nearest-neighbor lookup.

---

## Why HEBBS Wins

| | Vector DB + wrapper | Hosted memory API | **HEBBS** |
|---|---|---|---|
| Storage model | Flat embeddings | Key-value + embeddings | Knowledge graph with typed edges |
| Retrieval | Cosine similarity | Similarity + keyword | Semantic, temporal, graph traversal |
| Reasoning | None | Basic summarization | Background reflection, contradiction detection, insight generation |
| Latency | 50-200ms (API) | 100-500ms (API) | **< 10ms (local)** |
| Infrastructure | Managed DB + embedding service | Hosted SaaS | **Single binary, zero dependencies** |
| Data residency | Their cloud | Their cloud | **Your machine, your data** |
| Offline support | No | No | **Yes** |

---

## How It Works

```
Your Agent ──► HEBBS daemon (local) ──► Structured memory store
                    │
                    ├── recall: query memories by semantic, temporal, or graph traversal
                    ├── store: ingest new memories, auto-decompose into propositions
                    └── reflect: background pipeline scores, consolidates, detects contradictions
```

Integration is three lines of code:

```python
from hebbs import Hebbs

hb = Hebbs()
hb.store("Customer prefers async communication over calls")
memories = hb.recall("How does this customer like to communicate?")
```

SDKs available in Python, TypeScript, and Rust. gRPC and REST APIs for everything else.

---

## Installation

```bash
brew install hebbs/tap/hebbs
hebbs init my-agent
```

One binary. No Postgres, no Redis, no vector DB, no embedding service. Runs embedded in your process or as a local daemon. Works offline.

---

## The Demo Moment

1. **Session 1:** User casually mentions they prefer email over Slack.
2. **Session 2:** User says "just ping me on Slack anytime."
3. **HEBBS flags the contradiction.** The agent asks: "You previously mentioned preferring email. Has that changed, or does it depend on context?"

No other memory layer does this. It's the difference between remembering and understanding.

---

## Who It's For

Teams building AI agents that need to work like employees, not chatbots. Sales agents, support agents, coding assistants, research agents: any agent that talks to the same users or works on the same problems across multiple sessions.

If your agent needs to remember context, notice patterns, and connect dots across conversations, it needs an engine, not a database.

---

**Website:** hebbs.dev
**License:** BSL 1.1 (free for non-production, commercial license available)
**GitHub:** github.com/hebbs-ai/hebbs
