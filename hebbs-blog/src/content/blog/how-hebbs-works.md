---
title: "How HEBBS Works: From Markdown to Memory Palace"
description: "The complete technical walkthrough. Two-phase ingest pipeline, RocksDB storage schema with five column families, four recall strategies with documented complexity, and how an AI agent uses HEBBS end-to-end in a real conversation loop."
pubDate: 2026-03-16
tags: ["engineering", "architecture", "deep-dive", "agents"]
heroImage: "/images/how-hebbs-works.png"
ogImage: "/images/how-hebbs-works.png"
---

This post walks through the entire HEBBS system: how files become memories, how memories are stored and indexed, how four recall strategies work under the hood, and how an AI agent uses all of it in a real conversation loop.

## The Data Flow

```
.md files
  → Phase 1: Parse (cheap, fast)
  → Phase 2: Embed + Index (expensive, async)
  → RocksDB (5 column families)
  → 4 recall strategies
  → agent.recall()
```

Everything starts with markdown files.

## Phase 1: Parse and Manifest

When you run `hebbs index`, the first phase walks your directory for `.md` files. For each file it:

1. Computes a SHA-256 checksum
2. Skips if the checksum matches the manifest (idempotent reruns are free)
3. Parses markdown into sections based on `split_level` (default: `##` headings)
4. Extracts heading path, YAML frontmatter, wiki-links, tags, and byte offsets
5. Diffs against the manifest: marks sections as `ContentStale`, `Synced`, or `Orphaned`
6. Assigns ULIDs to new sections

The manifest is saved atomically: write to `.tmp`, then rename. If `kill -9` hits between any two instructions, no data is corrupted.

**Complexity**: O(F * S) where F = files and S = average sections per file. This phase is CPU-cheap because it does no embedding.

## Phase 2: Embed and Index

The second phase collects all `ContentStale` and `Orphaned` sections, then:

1. **Batch embeds** all content using a local ONNX model (BGE-small, 384 dimensions by default)
2. L2-normalizes all vectors
3. For new sections: `engine.remember()` writes to RocksDB
4. For modified sections: `engine.revise()` updates content while preserving history
5. For orphaned sections: `engine.delete()` removes the memory
6. Resolves wiki-links to `RELATED_TO` edges in the graph
7. Runs contradiction detection (optional): finds semantically similar memories, classifies whether they conflict

Embedding batches are capped at 256 sections per batch to prevent OOM. The ONNX runtime picks the best available backend automatically: CoreML on Apple Silicon (~1ms per embed), CUDA on NVIDIA GPUs (~2ms), CPU fallback (~3-5ms).

**Complexity**: O(N * D) for embedding, where N = sections and D = embedding dimensions.

## Storage: Five Column Families in RocksDB

HEBBS stores everything in a single embedded RocksDB instance with five column families. No external database. No network calls to a storage layer.

### 1. `default`: Memory Records

Key: `[0x01][memory_id 16B]`
Value: Bitcode-encoded `Memory` struct

Every memory carries: content, importance (0-1.0), context (structured metadata), entity_id, embedding vector reference, created_at, updated_at, last_accessed_at, access_count, decay_score, and kind (EPISODE, INSIGHT, or REVISION).

### 2. `temporal`: Entity-Time Index

Key: `[entity_id UTF-8][0xFF separator][timestamp_us big-endian u64]`
Value: memory_id (16 bytes)

This layout means a range scan on `entity_id + start_time .. entity_id + end_time` returns all memories for an entity in chronological order. O(log n + k) lookup.

### 3. `vectors`: Embedding Storage

Key: `[0x00][memory_id 16B]`
Value: f32 array (384 * 4 = 1,536 bytes per vector)

Vectors live in their own column family so the HNSW graph can load them without touching memory records.

### 4. `graph`: Directed, Typed Edges

Forward key: `[0xF0][source 16B][edge_type 1B][target 16B]`
Reverse key: `[0xF1][target 16B][edge_type 1B][source 16B]`
Value: `[confidence f32 LE][timestamp_us u64 LE]` (12 bytes)

Bidirectional encoding means "what did this memory cause?" and "what caused this memory?" are both O(log n + k) range scans. Six edge types: `CAUSED_BY`, `RELATED_TO`, `FOLLOWED_BY`, `REVISED_FROM`, `INSIGHT_FROM`, `CONTRADICTS`.

### 5. `meta`: System State

Schema version, sweep cursors, config, auto-forget candidates.

### Tuning

WAL enabled for durability. Bloom filters at 10 bits/key for O(1) negative lookups. 256 MB LRU block cache. LZ4 compression on all levels, Zstd on bottommost. Level compaction with 2 background threads and a 100 MB/s rate limit. No application-level locking: RocksDB handles concurrency internally with `DBWithThreadMode<MultiThreaded>` wrapped in `Arc`.

## Four Recall Strategies

Every other memory system gives you one question: "what looks similar?" HEBBS answers four.

### Similarity

Embed the cue, search the HNSW graph, rank by cosine distance.

The HNSW (Hierarchical Navigable Small World) graph is an in-memory structure protected by `RwLock`. Layer assignment follows the standard geometric distribution: `layer = floor(-ln(uniform) * ml)` where ml is approximately 0.7.

**Complexity**: O(embed) + O(log n * ef_search) + O(k * lookup)

### Temporal

Query the `temporal` column family by entity_id and time range. Returns memories in chronological or reverse-chronological order.

When filtering by entity_id, HEBBS applies a 4x oversample factor to ensure enough candidates survive filtering.

**Complexity**: O(log n + k)

### Causal

Find a seed memory (via embedding or direct ID), then traverse the graph along typed edges up to a configurable max_depth. Supports forward, backward, and bidirectional traversal.

This is the strategy that answers "why did this happen?" by walking the causal chain backward from an outcome to its root causes.

**Complexity**: O(embed or lookup) + O(branching_factor^max_depth) + O(k * lookup)

### Analogical

Wider HNSW search, then re-rank by a blend of embedding similarity and structural similarity. The `alpha` parameter controls the blend: 0.0 = pure embedding, 1.0 = pure structural.

This enables vector arithmetic (A:B::C:?) when A, B, C are provided. The strategy finds memories that are structurally similar even when the surface content differs.

**Complexity**: O(embed) + O(log n * 2 * ef_search) + O(candidates * structural_compare)

### Composite Scoring

All strategies feed results through a four-factor weighted score:

```
score = w_relevance * relevance
      + w_recency * recency_factor
      + w_importance * importance
      + w_reinforcement * reinforcement_factor
```

- **Relevance**: Strategy-specific signal (cosine distance, temporal position, graph depth, structural match)
- **Recency**: Exponential decay from `last_accessed_at`, max age 30 days
- **Importance**: Stored in the memory record at write time
- **Reinforcement**: `log2(1 + access_count)` capped at `reinforcement_cap` (default 100)

The agent controls all four weights on every call. "Show me the closest match" sets w_relevance=1.0 and everything else to 0. "What keeps coming up?" sets w_reinforcement=0.5. Same query, different weights, completely different results.

## Contradiction Detection

During ingest, each new section is checked against semantically similar existing memories. HEBBS supports two classification modes:

**Heuristic mode** (default, no external calls): detects negation asymmetry, antonym pairs (37 built-in pairs like reliable/unreliable, success/failure), numeric disagreement, and revision markers. Confidence capped at 0.75.

**LLM mode** (when a provider is configured): sends both statements to an LLM for entailment classification. Returns CONTRADICTION, REVISION, or NEUTRAL with full confidence range.

Confirmed contradictions create bidirectional `CONTRADICTS` edges in the graph and write human-readable markdown reports to the `contradictions/` directory with both sides, source files, and confidence scores.

## Decay and Reinforcement

Memories decay. This is by design. The decay formula:

```
decay_score = importance * 2^(-age / half_life) * (1 + log2(1 + access_count) / log2(1 + cap))
```

- **Time factor**: Exponential decay with a 30-day half-life. At 30 days, the time factor is 0.5.
- **Reinforcement factor**: Logarithmic scaling prevents runaway amplification. Capped at ~2.0 when access_count hits the cap (default 100).
- **Age computation**: Based on `last_accessed_at`, not `created_at`. Every recall resets the decay clock.

A background decay worker sweeps memories hourly in batches of 10,000. Memories below a threshold (default 0.01) become auto-forget candidates. The sweep cursor persists in the `meta` column family so it resumes cleanly after restart.

## Watch Mode: Two-Phase Debounce

`hebbs watch` monitors your directory using the `notify` crate for cross-platform filesystem events. When a file changes, it doesn't immediately re-embed. Instead:

```
FS event → burst detection → Phase 1 debounce (500ms) → Phase 2 debounce (2000ms)
                              parse into manifest         embed and index
```

Phase 1 is cheap (parse, checksum). Phase 2 is expensive (embed, index, contradiction check). The split prevents the embedding pipeline from being overwhelmed during rapid edits.

Burst detection: if more than 10 events arrive during Phase 1, the Phase 2 debounce extends to 5 seconds. This handles `git checkout` and bulk file operations gracefully.

## The Daemon

The daemon is a single long-lived process on `~/.hebbs/daemon.sock`. It loads the ONNX model once and keeps it warm in memory. Vault RocksDB instances open on demand and close after an idle timeout.

Protocol: length-prefixed JSON over Unix socket. 4-byte big-endian length prefix, then JSON payload. Max message size: 16 MiB.

The daemon auto-starts on first use, writes a PID file, cleans up stale sockets on startup, and shuts down after 5 minutes of inactivity. HTTP panel on port 6381 serves the Memory Palace web UI.

## Reflect: How Episodes Become Insights

Reflection consolidates raw episodes into higher-order insights. It runs in two phases:

**Prepare**: Collects memories (scoped to an entity or global), clusters their embeddings via K-means, and generates LLM prompts for each cluster. Returns cluster summaries and source memory IDs.

**Commit**: Takes the agent's proposed insights (or the server's own LLM-generated proposals), validates them, stores them as `Memory(kind=INSIGHT)`, and creates `INSIGHT_FROM` edges linking back to source memories.

The two-step design gives the agent control over insight reasoning. The agent sees the clusters, reads the source memories, and decides what patterns to extract. Session IDs expire after 10 minutes to prevent stale commits.

Configuration is bounded: max 500 memories per reflect, min 5 memories to trigger, max 50 clusters, 4000-token proposal budget, 6000-token validation budget.

## How an Agent Uses HEBBS End-to-End

Here's a real conversation turn from the reference sales agent (hebbs-demo):

### Session Start

```python
# Load context for this prospect
primed = await hebbs.prime(entity_id="acme_corp", similarity_cue="pricing discussion")
insights = await hebbs.insights(entity_id="acme_corp")
# Both go into the system prompt
```

`prime()` is a hybrid of temporal (most recent 20 memories) and similarity (most relevant memories to the cue). The agent gets a broad context snapshot in one call.

### Per-Turn Loop

```
User: "What CRM do you recommend?"

1. RECALL
   hebbs.recall(cue="CRM recommendation", strategies=["similarity", "temporal"],
                entity_id="acme_corp", top_k=10,
                scoring_weights={w_relevance: 0.4, w_recency: 0.3,
                                 w_importance: 0.2, w_reinforcement: 0.1})
   → [0.92 "Acme uses Salesforce", 0.87 "Considering Hubspot migration"]

2. SUBSCRIBE (real-time surfacing)
   sub.feed("What CRM do you recommend?")
   → Engine embeds the text, searches, pushes matches above threshold
   → [0.85 "CRM budget is $50K annually"]

3. BUILD CONTEXT
   System prompt + primed memories + recalled memories
   + real-time surfaced + institutional insights + conversation history

4. LLM RESPONSE
   → "Based on your current Salesforce setup and $50K budget..."

5. EXTRACT AND REMEMBER
   LLM analyzes both user message and agent response, extracts:
   → {content: "Prospect asking about CRM alternatives", importance: 0.7,
      context: {stage: "discovery", topic: "tech_stack"}}
   → hebbs.remember() for each extracted memory

6. LATENCIES
   RECALL 4.1ms, SUBSCRIBE 0.8ms, REMEMBER 3.2ms
```

The agent decides everything: which strategies to use, what weights to apply, which entity to scope to, how many results to fetch. HEBBS executes.

### The SKILL.md Contract

The agent learns to use HEBBS through a `SKILL.md` file dropped into its skill directory. The skill teaches:

- **When to remember**: User shares a preference, corrects something, makes a decision, or reveals new information
- **When to recall**: Before answering any question that requires context
- **When to prime**: At conversation start, to load entity context
- **When to reflect**: After accumulating enough memories to extract patterns
- **When to forget**: GDPR requests, stale data cleanup

The skill also teaches the "two brains" architecture: a global brain at `~/.hebbs/` for cross-project knowledge (user preferences, writing style, corrections) and a project brain at `.hebbs/` for project-specific context (architecture, conventions, deployment).

### Subscribe: Real-Time Memory Surfacing

The most powerful pattern is subscribe. The agent opens a streaming connection at session start:

```python
sub = await hebbs.subscribe(entity_id="acme_corp", confidence_threshold=0.5)
```

On every user turn, the agent feeds the raw text:

```python
await sub.feed(user_message)
```

The engine embeds the text, runs similarity search, and pushes any match above the confidence threshold back to the agent. The agent includes these in context as "real-time surfaced" memories, separate from explicit recall results.

This means the agent surfaces relevant memories without explicitly deciding to search for them. The prospect mentions "SOC2 compliance" in passing, and a memory from three weeks ago about their compliance blocking issue appears automatically.

### Reflect: Institutional Learning

After enough conversations, the agent runs reflect:

```python
# Agent-driven two-step
prep = await hebbs.reflect_prepare(entity_id="acme_corp")
for cluster in prep.clusters:
    # Agent reads cluster.memories[] and cluster.proposal_system_prompt
    # Agent reasons about patterns across the cluster
    insights.append(ProducedInsightInput(
        content="Acme prioritizes compliance over cost in vendor decisions",
        confidence=0.9,
        source_memory_ids=[...],
        tags=["priority", "compliance"]
    ))
result = await hebbs.reflect_commit(prep.session_id, insights)
```

The insights become `INSIGHT` memories with `INSIGHT_FROM` edges linking back to their source episodes. Future recall calls return these insights alongside raw episodes. A new sales rep talking to Acme for the first time gets institutional knowledge from day one.

## The Full Picture

```
Markdown files ──→ Phase 1 (parse) ──→ Phase 2 (embed) ──→ RocksDB
                                                              │
         ┌────────────────────────────────────────────────────┘
         │
         ├── HNSW graph (similarity)
         ├── Temporal CF (time-ordered per entity)
         ├── Graph CF (causal chains, contradictions)
         └── Associative index (analogical patterns)
                          │
                    4 recall strategies
                          │
              ┌───────────┴───────────┐
              │                       │
         agent.recall()          agent.subscribe()
              │                       │
         explicit search       real-time surfacing
              │                       │
              └───────────┬───────────┘
                          │
                     LLM context
                          │
                   agent response
                          │
                  agent.remember()
                          │
                     back to RocksDB
```

One binary. No external dependencies. Every parameter exposed to the agent. That's HEBBS.
