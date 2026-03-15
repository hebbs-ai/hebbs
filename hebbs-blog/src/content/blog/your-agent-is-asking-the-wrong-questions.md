---
title: "Your Agent Is Asking the Wrong Questions"
description: "Raw conversational queries score 0.49 relevance against a retrieval model. Rewritten cues score 0.87. The difference is the agent's intelligence layer: query rewriting, strategy selection, and weight tuning."
pubDate: 2026-03-14
tags: ["engineering", "vault", "recall", "agents"]
heroImage: "/images/your-agent-is-asking-the-wrong-questions.png"
ogImage: "/images/your-agent-is-asking-the-wrong-questions.png"
---

Your agent has a vault full of indexed markdown. It can recall any section by meaning. But when a user asks "what was discussed in the last meeting?", the agent passes that string directly to the retrieval model and gets back noise.

The retrieval model is not broken. The agent is asking the wrong question.

## The Problem: Conversational Queries Hit Retrieval Models

BGE-small-en-v1.5 is a retrieval embedding model. It was trained on query-document pairs, not conversations. When you give it a well-formed retrieval cue, it performs excellently. When you give it a conversational question, it struggles.

We measured this across a real vault with 13 indexed sections:

| Query | Relevance | Grade |
|---|---|---|
| `"Rust ownership borrowing memory safety"` | **0.87** | GOOD |
| `"HNSW vector index embedding search"` | **0.73** | GOOD |
| `"Rust design patterns builder typestate"` | **0.71** | GOOD |
| `"what was discussed in the meeting?"` | **0.49** | WEAK |
| `"what are the pending tasks?"` | **0.46** | WEAK |

The gap between 0.87 and 0.49 is the difference between a useful answer and a coin flip. The top three queries use vocabulary that matches the document content. The bottom two use conversational phrasing that has low lexical overlap with the factual text in the files.

This is not a limitation of HEBBS. It is a fundamental property of dense retrieval models. BERT-family encoders compress text into fixed-dimension vectors that capture semantic meaning, but "what was discussed" and "standup meeting decisions on API versioning and deployment timeline" live in very different parts of the embedding space.

## The Fix: The Agent is the Intelligence Layer

The retrieval model is the back-end. The agent is the front-end. The agent's job is to translate user intent into retrieval-optimized input. Three transformations matter:

1. **Query rewriting**: Convert conversational questions into keyword-rich cues
2. **Strategy selection**: Pick the right recall strategy for the intent
3. **Weight tuning**: Shift the scoring weights to match what the user actually cares about

### 1. Query Rewriting

The agent should never pass a raw user question to recall. Instead, decompose the question and rewrite it.

**User says:** "What was discussed in the last meeting?"

**Agent thinks:**
- "discussed" = the content is meeting notes, agendas, decisions
- "last meeting" = recency matters
- The retrieval cue should use vocabulary that matches note content

**Agent queries:**

```bash
hv recall $VAULT \
  --query "meeting discussion agenda decisions action items" \
  -k 5
```

The rewritten cue uses words that actually appear in meeting notes. "discussion", "agenda", "decisions", "action items" all have high lexical overlap with the kind of content people write in meeting files.

**User says:** "What tasks are assigned to Jasen?"

**Agent thinks:**
- This is entity-scoped (Jasen) plus topic-scoped (tasks)
- A single query will find content mentioning "Jasen" literally but may miss the actual task list
- Better to do two targeted recalls and cross-reference

**Agent queries:**

```bash
# Recall 1: Find content about Jasen
hv recall $VAULT \
  --query "jasen responsibilities deliverables owner assigned" \
  -k 5

# Recall 2: Find task lists
hv recall $VAULT \
  --query "action items tasks TODO deliverables due deadline" \
  -k 5
```

Then the agent reads both result sets and cross-references: which tasks mention Jasen? Which Jasen-related content describes task assignments?

The agent is the join layer. The retrieval model cannot do entity-scoped filtering because it embeds meaning, not named entities. Two targeted recalls give the agent what it needs to reason about.

### 2. Strategy Selection

HEBBS supports four recall strategies. Each one answers a different kind of question:

| User intent | Strategy | Why |
|---|---|---|
| "What do we know about X?" | `similarity` | Pure semantic match |
| "What happened recently?" | `temporal` | Rank by timestamp |
| "What led to this decision?" | `causal` | Trace cause-effect edges |
| "Have we seen this before?" | `analogical` | Find structural parallels |

The default is similarity, and for most lookups it is correct. But when the user asks about recency ("what changed since yesterday?"), the agent should switch to temporal. When the user asks about causation ("why did we pick RocksDB?"), the agent should first find the decision with similarity, then trace its causal chain.

Strategies can be combined:

```bash
hv recall $VAULT \
  --query "meeting discussion decisions" \
  --strategy similarity,temporal \
  -k 5
```

This returns results scored by both semantic relevance and recency. For "last meeting" queries, this is the right combination.

### 3. Weight Tuning

The composite score blends four signals: relevance, recency, importance, and reinforcement. Default weights are `0.5:0.2:0.2:0.1`. But different questions call for different blends.

**Factual lookup** ("What is our deployment architecture?"):

```bash
--w-relevance 1.0 --w-recency 0.0 --w-importance 0.0 --w-reinforcement 0.0
```

Pure semantic match. The answer does not depend on when it was written or how important it is. In our tests, this produced a clean 0.23 gap between the top result and the second, making the right answer unambiguous.

**Recent activity** ("What changed since the last standup?"):

```bash
--w-relevance 0.2 --w-recency 0.7 --w-importance 0.1 --w-reinforcement 0.0
```

Recency dominates. The cue still matters for filtering (we want changes, not random old content), but the ranking should favor whatever was modified most recently.

**Important decisions** ("What are our key architectural decisions?"):

```bash
--w-relevance 0.4 --w-importance 0.5 --w-recency 0.1 --w-reinforcement 0.0
```

Importance-weighted. Architecture decisions should be tagged with high importance in the frontmatter or by the agent during storage. Boosting the importance weight surfaces decisions over casual notes.

**Meeting prep** (breadth required):

The agent should run multiple queries with different weight profiles:

```bash
# Step 1: Get factual overview (pure relevance)
hv recall $VAULT --query "HEBBS architecture components" \
  --w-relevance 1.0 -k 5

# Step 2: Get recent changes (recency-heavy)
hv recall $VAULT --query "architecture changes decisions migration" \
  --w-relevance 0.3 --w-recency 0.6 --w-importance 0.1 -k 5

# Step 3: Get open risks (importance-heavy)
hv recall $VAULT --query "risks concerns blockers technical debt" \
  --w-relevance 0.4 --w-importance 0.5 --w-recency 0.1 -k 3
```

Three recalls, three weight profiles, one synthesized briefing. The agent reads all results and composes a meeting prep document covering facts, recent changes, and open risks.

Weights are automatically normalized to sum to 1.0, so you can use any ratio. `--w-relevance 4 --w-recency 1` is equivalent to `--w-relevance 0.8 --w-recency 0.2`.

## The Full Pattern

Every recall should follow this flow:

```
User question
     |
     v
Classify intent
     |  What kind of question is this?
     |  Factual? Temporal? Entity-scoped? Causal?
     v
Rewrite into cues
     |  1-3 keyword-rich retrieval cues
     |  No conversational phrasing
     v
Select strategy + weights
     |  Match strategy to intent
     |  Shift weights to match what matters
     v
Run recall (1-3 calls)
     |
     v
Read top-N results
     |  Cross-reference if multi-query
     |  Cite file paths for provenance
     v
Synthesize answer
```

The agent is doing the work that a conversational search engine would do internally, but explicitly, with full control over strategy and ranking.

## What We Learned from Testing

We ran 8 structured tests against a real vault with BGE-small ONNX embeddings. Key findings:

**Direct concept lookups work excellently.** Queries with technical vocabulary matching the content consistently score 0.71 to 0.87 relevance. No tricks needed. Just expand the query with related terms.

**Conversational queries need rewriting.** Without rewriting, meeting and task queries score 0.46 to 0.49. The retrieval model cannot bridge conversational phrasing to factual content. This is the single most impactful optimization an agent can make.

**Multi-query beats single-query for complex questions.** Entity-scoped queries ("what is Jasen working on?"), meeting prep, and decision tracing all benefit from decomposing into 2-3 targeted recalls with different strategies or weights.

**Weight tuning provides clean separation.** Setting `--w-relevance 1.0` on factual lookups produced a 0.23 gap between the correct result and the next candidate. Default weights produced a 0.18 gap. Tuning narrows the noise band.

**Top-N matters.** Meeting notes are split into sections (Discussion, Action Items, Attendees). The answer is often spread across 3-5 results. An agent that reads only the top result misses context.

**Small corpus inflates scores.** With fewer than 50 sections, even irrelevant results score 0.40+ because the embedding space is sparse. Larger vaults (100+ sections) show much better separation between relevant and irrelevant content. Do not interpret 0.40 as "somewhat relevant" in a small vault.

## The Takeaway

The retrieval model is a precision instrument. It returns exactly what you ask for. If you ask poorly, you get poor results. If you ask well, you get excellent results.

The agent's job is to ask well. Rewrite the question. Pick the right strategy. Tune the weights. Read multiple results. Cite the sources. The vault handles the embedding, indexing, and retrieval. The agent handles the intelligence.

That is the split. That is how it should work.
