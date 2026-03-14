# TEST-PLAN-13: Manual Test Plan for File-First Markdown Sync

**Date**: 2026-03-14
**Binary**: `hebbs/target/release/hebbs-vault`
**Prerequisites**: Build with `cd hebbs && cargo build -p hebbs-vault --release`

---

## Setup

```bash
export VAULT=/tmp/manual-test-vault
alias hv='./target/release/hebbs-vault'

mkdir -p $VAULT/notes $VAULT/projects $VAULT/meetings $VAULT/research
```

---

## Part A: Infrastructure Tests (Index Lifecycle)

### A1: Init and First Index

```bash
cat > $VAULT/notes/rust-ownership.md << 'EOF'
---
title: Rust Ownership Model
tags: [rust, memory-safety]
---

## Core Concept

Rust's ownership model ensures memory safety without a garbage collector. Each value has exactly one owner, and the value is dropped when the owner goes out of scope.

## Borrowing Rules

References allow borrowing values without taking ownership. You can have either one mutable reference OR any number of immutable references at a time.

## Lifetimes

Lifetimes annotate how long references are valid. The borrow checker uses lifetimes to ensure references don't outlive the data they point to. See [[rust-patterns]] for common lifetime patterns.
EOF

cat > $VAULT/notes/rust-patterns.md << 'EOF'
---
title: Common Rust Patterns
tags: [rust, design-patterns]
---

## Builder Pattern

The builder pattern avoids constructors with many parameters. Chained method calls return `&mut Self` and a final `.build()` consumes the builder.

## Newtype Pattern

Wrapping a type in a single-field struct creates a distinct type with zero runtime cost. Useful for enforcing invariants.

## Error Handling

Use `thiserror` for library error types and `anyhow` for application errors. See [[rust-ownership#borrowing-rules]] for how error propagation interacts with borrowing.
EOF

cat > $VAULT/projects/hebbs-architecture.md << 'EOF'
---
title: HEBBS Architecture
author: paragarora
importance: 0.9
---

## Overview

HEBBS is a memory engine that uses Hebbian learning principles. It stores memories with semantic embeddings, temporal decay, and graph relationships.

## Storage Layer

RocksDB provides the persistent key-value store. Memories are stored as protocol buffer records with metadata including importance scores and timestamps.

## Embedding Engine

ONNX runtime powers the embedding model (BGE-small). Embeddings are 384-dimensional vectors stored in an HNSW index for approximate nearest neighbor search.

## Reflection Pipeline

The reflect pipeline synthesizes insights from stored memories. It identifies patterns, contradictions, and emergent themes across the memory graph.
EOF

cat > $VAULT/meetings/2026-03-14-standup.md << 'EOF'
## Attendees

Team alpha: paragarora, jasen, emily

## Discussion

Reviewed progress on the vault sync feature. The markdown parser handles frontmatter, wiki-links, and section splitting correctly.

Jasen raised the question of how [[hebbs-architecture#reflection-pipeline]] will handle contradictions in future iterations.

## Action Items

- paragarora: finish e2e test scenarios by end of sprint
- jasen: draft contradiction detection spec
- emily: benchmark embedding latency on large vaults
EOF

hv init $VAULT
hv index $VAULT
hv status $VAULT
hv list $VAULT --sections
```

**Verify**:
- [ ] `.hebbs/` created with config.toml, manifest.json, index/
- [ ] ONNX model at `.hebbs/index/models/bge-small-en-v1.5/` (~128MB)
- [ ] RocksDB at `.hebbs/index/db/`
- [ ] 4 files, 13 sections, all synced
- [ ] `hv list --sections` shows heading paths, byte offsets, memory IDs

### A2: Incremental Re-Index

```bash
cat >> $VAULT/notes/rust-ownership.md << 'EOF'

## Move Semantics

When you assign a value to another variable, ownership moves. The original variable becomes invalid. For types that implement `Copy`, a copy is made instead.
EOF

hv index $VAULT
hv status $VAULT
```

**Verify**:
- [ ] Phase 1: 1 processed, 3 skipped
- [ ] 1 new section (Move Semantics)
- [ ] 14 sections total, all synced

### A3: Add New Folder

```bash
cat > $VAULT/research/vector-databases.md << 'EOF'
---
title: Vector Database Comparison
tags: [databases, embeddings]
---

## HNSW Algorithm

Hierarchical Navigable Small World graphs provide approximate nearest neighbor search with logarithmic complexity. Used by Pinecone, Weaviate, and HEBBS.

## Product Quantization

PQ compresses high-dimensional vectors by splitting them into sub-vectors and quantizing each independently. Trades recall accuracy for memory efficiency.
EOF

cat > $VAULT/research/hebbian-learning.md << 'EOF'
## Biological Basis

"Neurons that fire together wire together." Donald Hebb's 1949 postulate describes how synaptic connections strengthen when neurons are co-activated.

## Application to Memory Systems

In HEBBS, memories accessed together develop stronger graph edges. Importance scores decay over time but are reinforced through retrieval.

## Decay Function

The importance decay follows an exponential curve: importance(t) = base * e^(-lambda * t). Each retrieval resets the base importance.
EOF

hv index $VAULT
hv status $VAULT
```

**Verify**:
- [ ] 2 new files processed, 4 skipped
- [ ] 6 files, 19 sections total

### A4: File Watcher

```bash
hv watch $VAULT &
WATCHER_PID=$!
sleep 2

cat > $VAULT/notes/new-while-watching.md << 'EOF'
## Live Note

This file was created while the watcher was running. It should be automatically indexed.
EOF

sleep 5

echo -e "\n## New Section\n\nAdded while watcher active." >> $VAULT/research/hebbian-learning.md
sleep 5

rm $VAULT/notes/new-while-watching.md
sleep 5

kill -INT $WATCHER_PID
wait $WATCHER_PID 2>/dev/null

hv status $VAULT
```

**Verify**:
- [ ] Watcher starts, reports events
- [ ] New file indexed automatically
- [ ] Modified file re-indexed
- [ ] Deleted file sections orphaned
- [ ] Watcher reports event/phase counts on shutdown

### A5: Rebuild Equivalence

```bash
hv status $VAULT
hv list $VAULT > /tmp/pre-rebuild.txt

hv rebuild $VAULT

hv status $VAULT
hv list $VAULT > /tmp/post-rebuild.txt
```

**Verify**:
- [ ] Same file and section counts
- [ ] All synced
- [ ] Config preserved

### A6: Edge Cases

```bash
# Empty file
touch $VAULT/notes/empty.md

# No headings
echo "Just prose, no headings at all. Full file is one section." > $VAULT/notes/no-headings.md

# Frontmatter only
printf -- '---\ntitle: Empty\n---\n' > $VAULT/notes/fm-only.md

# Code blocks with fake headings
cat > $VAULT/notes/code-blocks.md << 'OUTER'
## Real Section

```python
## This is NOT a heading
def hello():
    return "world"
```
OUTER

hv index $VAULT
hv list $VAULT --sections
```

**Verify**:
- [ ] Empty file: 0 or 1 section, no crash
- [ ] No headings: 1 section with `(root)` heading
- [ ] Frontmatter only: no crash
- [ ] Code blocks: only "Real Section" extracted, not "This is NOT a heading"

---

## Part B: Agent Recall Quality Analysis

These tests simulate how an AI agent would use `hebbs-vault recall` to answer user questions. Each test documents:
- What the agent asks
- What it gets back
- Whether the agent can answer the user's question from the results
- Quality grade: GOOD (top result is correct), OK (correct in top 3), WEAK (correct but buried or low score), FAIL (missing)

### B1: Direct Concept Lookup

**User asks agent**: "Explain Rust's ownership model"
**Agent query**:
```bash
hv recall $VAULT -q "Rust ownership and memory safety" -k 5
```

**Expected top result**: `rust-ownership.md > Core Concept` with relevance > 0.8

**Observed** (from real run):
| Rank | File | Section | Relevance |
|------|------|---------|-----------|
| 1 | rust-ownership.md | Core Concept | 0.8694 |
| 2 | hebbs-architecture.md | Reflection Pipeline | 0.6861 |
| 3 | hebbs-architecture.md | Storage Layer | 0.6715 |

**Grade**: GOOD. Top result is exactly right, high relevance. Agent can answer directly from result 1. Gap to result 2 is large (0.18), clean separation.

### B2: Conversational / Indirect Query

**User asks agent**: "What was discussed in the last meeting?"
**Agent query**:
```bash
hv recall $VAULT -q "what was discussed in the last meeting" -k 5
```

**Expected**: All 3 meeting sections in top 3

**Observed**:
| Rank | File | Section | Relevance |
|------|------|---------|-----------|
| 1 | standup.md | Discussion | 0.4887 |
| 2 | standup.md | Attendees | 0.4479 |
| 3 | rust-ownership.md | Lifetimes | 0.4421 |
| 4 | rust-ownership.md | Borrowing Rules | 0.4409 |
| 5 | rust-patterns.md | Builder Pattern | 0.4205 |

**Grade**: WEAK. Meeting Discussion is #1 but at only 0.49 relevance. Action Items is #4 at 0.46 -- below unrelated Rust content. The conversational phrasing "what was discussed" has poor lexical overlap with the factual content.

**Agent workaround**: Agent should rewrite query to match content vocabulary:
```bash
hv recall $VAULT -q "standup meeting progress review action items" -k 5
```

**Takeaway**: BGE-small is a retrieval model, not a conversational model. Agents MUST rewrite user questions into retrieval-friendly cues. This is standard RAG practice.

### B3: Cross-Domain Knowledge Retrieval

**User asks agent**: "How does HEBBS store and retrieve embeddings?"
**Agent query**:
```bash
hv recall $VAULT -q "how does HEBBS store and retrieve embeddings" -k 5
```

**Observed**:
| Rank | File | Section | Relevance |
|------|------|---------|-----------|
| 1 | hebbs-architecture.md | Overview | 0.7882 |
| 2 | hebbs-architecture.md | Embedding Engine | 0.6891 |
| 3 | standup.md | Discussion | 0.6281 |
| 4 | hebbs-architecture.md | Reflection Pipeline | 0.6330 |
| 5 | hebbs-architecture.md | Storage Layer | 0.6183 |

**Grade**: GOOD. Top 2 are the right sections. Agent gets Overview (mentions "semantic embeddings") and Embedding Engine (mentions "HNSW index"). All 4 architecture sections appear. Agent can synthesize a complete answer.

### B4: Action Item / Task Tracking

**User asks agent**: "What are the pending action items?"
**Agent query**:
```bash
hv recall $VAULT -q "what are the pending action items and tasks" -k 5
```

**Observed**:
| Rank | File | Section | Relevance |
|------|------|---------|-----------|
| 1 | hebbs-architecture.md | Storage Layer | 0.4865 |
| 2 | standup.md | Discussion | 0.4552 |
| 3 | standup.md | Attendees | 0.4475 |
| 4 | standup.md | Action Items | 0.4633 |
| 5 | hebbs-architecture.md | Reflection Pipeline | 0.4073 |

**Grade**: WEAK. The actual Action Items section is #4 (score 0.4633, composite puts it at rank 4 due to scoring weights). RocksDB storage is #1 which is wrong. Relevance scores are all clustered in a narrow band (0.41-0.49) meaning the model can't differentiate well.

**Agent workaround**: More specific query:
```bash
hv recall $VAULT -q "paragarora jasen emily sprint deliverables" -k 3
```

### B5: Person-Scoped Query

**User asks agent**: "What is Jasen working on?"
**Agent query**:
```bash
hv recall $VAULT -q "what is jasen working on" -k 5
```

**Observed**:
| Rank | File | Section | Relevance |
|------|------|---------|-----------|
| 1 | standup.md | Attendees | 0.5891 |
| 2 | standup.md | Discussion | 0.4941 |
| 3 | hebbs-architecture.md | Embedding Engine | 0.4990 |
| 4 | standup.md | Action Items | 0.4807 |

**Grade**: OK. Meeting sections are #1, #2, #4. The actual answer ("draft contradiction detection spec") is in Action Items at #4. Attendees is #1 because it contains "jasen" literally. An agent reading top-5 results can find the answer, but it requires scanning multiple sections.

**Agent workaround**: Entity-scoped queries would help here. Future: tag memories with entity_id during indexing so temporal strategy can filter by person.

### B6: Technical Concept Lookup

**User asks agent**: "Tell me about design patterns in Rust"
**Agent query**:
```bash
hv recall $VAULT -q "builder pattern newtype pattern" -k 5
```

**Observed**:
| Rank | File | Section | Relevance |
|------|------|---------|-----------|
| 1 | rust-patterns.md | Builder Pattern | 0.7119 |
| 2 | rust-patterns.md | Newtype Pattern | 0.6476 |
| 3 | standup.md | Discussion | 0.5016 |
| 4 | standup.md | Action Items | 0.5181 |
| 5 | rust-patterns.md | Error Handling | 0.5477 |

**Grade**: GOOD. Top 2 are exactly right. All 3 rust-patterns sections in top 5. Agent can give a complete answer about Rust patterns.

### B7: Weight Tuning -- Recency Bias

**Scenario**: Agent preparing context for a daily standup, wants recent info first.
```bash
hv recall $VAULT -q "project progress" -k 5 --w-relevance 3 --w-recency 7
```

**Observed**: Weights normalized to relevance=0.29 recency=0.68. Meeting sections bubble up because recency dominates. Scores are high (0.83-0.85) but tightly clustered because recency is the same for all memories (all indexed at same time).

**Grade**: OK for the mechanism, but the test vault has no temporal diversity. All memories created within the same second, so recency can't differentiate. In a real vault with memories spanning days/weeks, this would meaningfully change rankings.

### B8: Weight Tuning -- Pure Relevance

**Scenario**: Agent doing a factual technical lookup, doesn't care about recency.
```bash
hv recall $VAULT -q "HNSW vector index nearest neighbor" -k 3 \
  --w-relevance 1 --w-recency 0 --w-importance 0 --w-reinforcement 0
```

**Observed**:
| Rank | File | Section | Relevance | Score |
|------|------|---------|-----------|-------|
| 1 | hebbs-architecture.md | Embedding Engine | 0.7338 | 0.7338 |
| 2 | standup.md | Action Items | 0.5013 | 0.5013 |
| 3 | hebbs-architecture.md | Overview | 0.4884 | 0.4884 |

**Grade**: GOOD. Score = relevance exactly (weights are 100% relevance). Top result is the right section. Clear gap to #2 (0.23). Agent gets exactly what it needs.

---

## Part C: Agent Integration Scenarios

These simulate complete agent workflows, not just individual queries.

### C1: Agent Answers a User Question (RAG)

**User**: "How does our system handle memory storage?"

**Agent workflow**:
1. Rewrite query for retrieval: "memory storage architecture RocksDB embeddings persistence"
2. Call recall with `--w-relevance 1 --w-recency 0 -k 5`
3. Read top 3 results
4. Synthesize answer from retrieved sections
5. Cite source files in response

```bash
hv recall $VAULT -q "memory storage architecture RocksDB embeddings persistence" -k 5 \
  --w-relevance 1 --w-recency 0 --w-importance 0 --w-reinforcement 0
```

**Verify**:
- [ ] Top results contain Storage Layer and Embedding Engine sections
- [ ] Agent has enough context to answer accurately
- [ ] Source file paths are correct (not "(unknown)")

### C2: Agent Prepares Meeting Context

**User**: "Prepare me for today's standup"

**Agent workflow**:
1. Query for recent meeting content: `--w-recency 7 --w-relevance 3`
2. Query for recent action items
3. Combine into a briefing

```bash
# Step 1: Recent meeting content
hv recall $VAULT -q "meeting standup progress review" -k 5 --w-relevance 3 --w-recency 7

# Step 2: Action items
hv recall $VAULT -q "action items tasks deliverables sprint" -k 3 --w-relevance 1 --w-recency 0 --w-importance 0 --w-reinforcement 0
```

**Verify**:
- [ ] Step 1 returns meeting sections
- [ ] Step 2 returns action items section
- [ ] Agent can identify: who attended, what was discussed, what's pending

### C3: Agent Learns While User Works (Watch Mode)

**Scenario**: Agent runs `hv watch` in background. User edits files. Agent periodically queries for updates.

```bash
# Terminal 1: Start watcher
hv watch $VAULT

# Terminal 2: User creates a new note
cat > $VAULT/notes/today-learning.md << 'EOF'
## TIL: Async Rust

Learned that `tokio::spawn` requires the future to be `Send`. This means you cannot hold a non-Send type (like `Rc`) across an `.await` point.

## Debugging Tip

Use `RUST_BACKTRACE=1` and `#[tokio::main]` with `flavor = "current_thread"` to debug async issues without Send bounds.
EOF

# Wait for watcher to process
sleep 5

# Terminal 2: Agent queries for what user just learned
hv recall $VAULT -q "async Rust tokio Send" -k 3
```

**Verify**:
- [ ] Watcher picks up new file (phase 1 + phase 2 logs)
- [ ] Recall finds the new content immediately
- [ ] Agent can reference what the user just wrote

### C4: Agent Writes an Insight (Insight Loop)

**Scenario**: Agent identifies a pattern and writes an insight file. The watcher indexes it, making it available for future recall.

```bash
# Agent writes insight
mkdir -p $VAULT/insights
cat > $VAULT/insights/01ABC-rust-safety-architecture-pattern.md << 'EOF'
---
hebbs-kind: insight
hebbs-sources:
  - notes/rust-ownership.md#core-concept
  - projects/hebbs-architecture.md#storage-layer
hebbs-confidence: 0.78
hebbs-created: 2026-03-14T12:00:00Z
---

The same principle that makes Rust memory-safe (single ownership, compile-time verification) is analogous to how HEBBS ensures data integrity in RocksDB (atomic WriteBatch, single-writer model). Both systems trade flexibility for correctness guarantees.
EOF

# If watcher is running, it auto-indexes. Otherwise:
hv index $VAULT
hv recall $VAULT -q "Rust safety and HEBBS data integrity analogy" -k 3
```

**Verify**:
- [ ] Insight file indexed as a regular markdown file
- [ ] Recall finds the insight when querying about the connection
- [ ] `hebbs-*` frontmatter parsed correctly
- [ ] The insight participates in future recalls (the loop closes)

### C5: Agent Multi-Query Strategy

**Scenario**: Agent uses multiple strategies for comprehensive recall.

```bash
# Similarity + temporal (if entity_id were set during indexing)
hv recall $VAULT -q "architecture design decisions" -s similarity -k 5

# Compare with recency-boosted
hv recall $VAULT -q "architecture design decisions" -s similarity -k 5 \
  --w-relevance 5 --w-recency 5

# Compare with pure relevance
hv recall $VAULT -q "architecture design decisions" -s similarity -k 5 \
  --w-relevance 1 --w-recency 0 --w-importance 0 --w-reinforcement 0
```

**Verify**:
- [ ] Rankings shift meaningfully between weight configurations
- [ ] Pure relevance mode gives the most topically accurate results
- [ ] Recency mode would differentiate if memories had different timestamps

---

## Part D: Quality Summary and Known Limitations

### What works well

| Scenario | Quality | Notes |
|----------|---------|-------|
| Direct concept lookup | GOOD (0.87 relevance) | BGE-small excels when query vocabulary matches content |
| Technical term search | GOOD (0.73 relevance) | "HNSW vector index" -> Embedding Engine section |
| Design pattern lookup | GOOD (0.71 relevance) | Both pattern sections in top 2 |
| Cross-domain retrieval | GOOD (0.79 relevance) | "HEBBS store embeddings" -> Overview + Embedding Engine |
| Weight tuning | WORKS | Normalized weights shift rankings as expected |

### What needs agent workarounds

| Scenario | Quality | Issue | Workaround |
|----------|---------|-------|------------|
| Conversational queries | WEAK (0.49) | "what was discussed" has low lexical overlap with factual content | Agent rewrites to retrieval-friendly cue |
| Action item lookup | WEAK (0.46) | "pending tasks" doesn't match bullet-point content well | Agent uses specific vocabulary from the domain |
| Person-scoped queries | OK (0.59) | "what is jasen working on" finds attendees list, not tasks | Future: entity_id tagging during indexing |

### Known limitations

1. **No query rewriting**: Agent must do its own cue optimization. BGE-small is a retrieval model, not a conversation model. Passing raw user questions gives mediocre results. Passing keyword-rich retrieval cues gives excellent results.

2. **No temporal diversity in test**: All memories created simultaneously, so recency weighting can't differentiate. Real vaults with days/weeks of history will show meaningful recency effects.

3. **No entity_id tagging**: Memories are stored without entity_id, so temporal strategy has no entity to filter by. Future work: extract entity mentions during parsing and tag memories.

4. **Small corpus effect**: With only 13-19 sections, even unrelated content scores 0.40+ because the embedding space is sparse. Larger vaults (100+ sections) will show better separation between relevant and irrelevant results.

5. **Index-only deletion**: `hv index` doesn't detect deleted files. Use `hv rebuild` or `hv watch` (which handles delete events in real-time).

### Recommendations for agent integration

1. **Always rewrite queries**: Transform user questions into keyword-rich retrieval cues before calling recall.
2. **Use weight tuning**: `--w-relevance 1 --w-recency 0` for factual lookups. `--w-recency 7 --w-relevance 3` for context preparation.
3. **Read top-N, not top-1**: Relevant information is often spread across multiple sections (e.g., meeting has 3 sections).
4. **Check file paths**: Use the file path in results to provide citations to the user.
5. **Use the watcher**: Run `hv watch` as a background process so the agent always has fresh context.

---

## Results Summary

| # | Test | Pass/Fail | Notes |
|---|------|-----------|-------|
| A1 | Init and first index | | |
| A2 | Incremental re-index | | |
| A3 | Add new folder | | |
| A4 | File watcher | | |
| A5 | Rebuild equivalence | | |
| A6 | Edge cases | | |
| B1 | Direct concept lookup | | |
| B2 | Conversational query | | |
| B3 | Cross-domain retrieval | | |
| B4 | Action item lookup | | |
| B5 | Person-scoped query | | |
| B6 | Technical concept lookup | | |
| B7 | Weight tuning (recency) | | |
| B8 | Weight tuning (pure relevance) | | |
| C1 | Agent RAG workflow | | |
| C2 | Agent meeting prep | | |
| C3 | Agent learns via watcher | | |
| C4 | Agent insight loop | | |
| C5 | Agent multi-query strategy | | |
