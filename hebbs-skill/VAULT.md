# HEBBS Vault: File-First Memory for Agents

HEBBS Vault turns a folder of markdown files into a searchable memory store. No server needed. The agent reads and writes plain `.md` files; `hebbs-vault` handles parsing, embedding (BGE-small ONNX, local), and indexing into a local RocksDB store.

## When to use Vault vs Server

| Use case | Tool |
|---|---|
| Agent works on a local codebase/knowledge base in markdown | `hebbs-vault` (this doc) |
| Agent needs persistent cross-session memory for a user | `hebbs-cli` + `hebbs-server` (see SKILL.md) |
| Agent wants to combine both (vault for docs, server for user prefs) | Use both |

## Setup

```bash
# Build (one-time)
cd hebbs && cargo build -p hebbs-vault --release
alias hv='./target/release/hebbs-vault'

# Initialize a vault
hv init /path/to/vault

# Index all markdown files
hv index /path/to/vault

# Start file watcher (long-running, re-indexes on save)
hv watch /path/to/vault
```

First run downloads the BGE-small-en-v1.5 ONNX model (~128MB) and creates a `.hebbs/` directory inside the vault containing the index, manifest, and model.

## Commands

| Command | Purpose |
|---|---|
| `hv init <path>` | Create `.hebbs/` directory in vault |
| `hv index <path>` | Parse and embed all `.md` files |
| `hv watch <path>` | Long-running watcher, re-indexes on file changes |
| `hv rebuild <path>` | Delete index and re-index from scratch |
| `hv status <path>` | Show file/section counts and sync state |
| `hv list <path> [--sections]` | List indexed files and their sections |
| `hv recall <path> --query "..." [flags]` | Retrieve relevant sections |

## Recall: The Core Agent Interface

```bash
hv recall /path/to/vault \
  --query "retrieval-optimized cue" \
  --strategy similarity \
  --top-k 5
```

### The Critical Rule: Rewrite Before Recall

BGE-small is a retrieval embedding model, not a conversational model. Passing raw user questions produces mediocre results. The agent must rewrite every user question into a keyword-rich retrieval cue before calling recall.

**Measured quality difference:**

| Input type | Relevance score | Grade |
|---|---|---|
| Retrieval cue: `"Rust ownership borrowing memory safety"` | 0.87 | GOOD |
| Retrieval cue: `"HNSW vector index embedding search"` | 0.73 | GOOD |
| Conversational: `"what was discussed in the meeting?"` | 0.49 | WEAK |
| Conversational: `"what are the pending tasks?"` | 0.46 | WEAK |

The difference between 0.87 and 0.49 is the difference between a useful answer and noise.

---

## Use Cases: How an Agent Should Think

### Use Case 1: "What was discussed in the last meeting?"

**Wrong** (direct passthrough):
```bash
hv recall $VAULT --query "what was discussed in the last meeting" -k 5
# Result: 0.49 relevance, meeting section buried among unrelated content
```

**Right** (agent rewrites + uses temporal weighting):
```bash
# Step 1: Agent rewrites the conversational question into retrieval cues
# "last meeting" = recency matters, so boost recency weight
# "discussed" = the content is meeting notes, agendas, decisions

hv recall $VAULT \
  --query "meeting discussion agenda decisions action items" \
  --strategy similarity,temporal \
  --w-relevance 0.4 --w-recency 0.5 --w-importance 0.1 --w-reinforcement 0.0 \
  -k 5
```

**Why this works:**
- `similarity,temporal` combines semantic matching with recency ranking, so newer meetings surface first
- `--w-recency 0.5` heavily favors recent content ("last" meeting)
- The rewritten query uses vocabulary that matches note content: "discussion", "agenda", "decisions", "action items"
- Agent reads top 5 results and synthesizes across multiple sections (meetings are typically split into Discussion, Action Items, Attendees sections)

---

### Use Case 2: "What tasks are assigned to Jasen?"

**Wrong** (direct passthrough):
```bash
hv recall $VAULT --query "what tasks are assigned to jasen" -k 5
# Result: Finds "Attendees" section (contains "jasen" literally) but misses task assignments
```

**Right** (agent decomposes into multiple targeted recalls):
```bash
# Step 1: Find content mentioning Jasen
hv recall $VAULT \
  --query "jasen responsibilities deliverables owner assigned" \
  --strategy similarity \
  --w-relevance 0.7 --w-importance 0.3 --w-recency 0.0 --w-reinforcement 0.0 \
  -k 5

# Step 2: Find action items and task lists
hv recall $VAULT \
  --query "action items tasks TODO deliverables due deadline" \
  --strategy similarity \
  --w-relevance 0.5 --w-importance 0.3 --w-recency 0.2 --w-reinforcement 0.0 \
  -k 5

# Step 3: Agent cross-references results from both recalls
# to find tasks that are both in task lists AND mention Jasen
```

**Why this works:**
- Single-query recall cannot do entity-scoped filtering well because BGE-small embeds semantic meaning, not named entities
- Two targeted recalls give the agent both the "Jasen context" and the "task context" to cross-reference
- Importance weight is boosted because tasks/assignments are typically high-importance content
- Agent acts as the join layer, connecting person to tasks using its reasoning

---

### Use Case 3: "Prepare me for a meeting about the HEBBS architecture"

**Right** (agent runs a multi-query strategy):
```bash
# Step 1: Get architecture overview
hv recall $VAULT \
  --query "HEBBS architecture system design components overview" \
  --strategy similarity \
  --w-relevance 1.0 --w-recency 0.0 --w-importance 0.0 --w-reinforcement 0.0 \
  -k 5

# Step 2: Get recent changes and decisions
hv recall $VAULT \
  --query "architecture decisions changes migration refactor" \
  --strategy similarity,temporal \
  --w-relevance 0.3 --w-recency 0.6 --w-importance 0.1 --w-reinforcement 0.0 \
  -k 5

# Step 3: Get open questions and risks
hv recall $VAULT \
  --query "open questions risks concerns blockers technical debt" \
  --strategy similarity \
  --w-relevance 0.5 --w-importance 0.4 --w-recency 0.1 --w-reinforcement 0.0 \
  -k 3
```

**Why this works:**
- Meeting prep requires breadth: architecture facts, recent changes, and open issues
- Step 1 uses pure relevance (no recency/importance bias) for factual lookup
- Step 2 shifts to 60% recency because "recent changes" is the whole point
- Step 3 boosts importance because risks and blockers tend to be high-importance items
- Agent synthesizes a briefing document from all three result sets

---

### Use Case 4: "What do we know about Rust patterns?"

**Right** (simple, direct lookup):
```bash
hv recall $VAULT \
  --query "Rust design patterns builder typestate error handling" \
  --strategy similarity \
  -k 5
```

**Why this is easy:** The query vocabulary directly matches the content vocabulary. No rewriting tricks needed. Direct concept lookups score 0.71-0.87 relevance. Just expand the query with related technical terms.

---

### Use Case 5: "Has anything changed since yesterday?"

**Right** (temporal-dominant strategy):
```bash
hv recall $VAULT \
  --query "recent changes updates modifications" \
  --strategy temporal \
  --w-recency 0.8 --w-relevance 0.1 --w-importance 0.1 --w-reinforcement 0.0 \
  -k 10
```

**Why this works:**
- `temporal` strategy ranks by timestamp, not semantic similarity
- 80% recency weight means the most recently modified sections surface first
- Higher `top_k` (10) because you want a broad view of what changed
- The cue still matters for tie-breaking, but recency dominates

---

### Use Case 6: Agent learns from file edits in real-time

**Setup** (run watcher as background process):
```bash
hv watch $VAULT &
```

**Scenario:** User edits `meetings/standup-2026-03-14.md` and adds new action items. The watcher detects the change, re-parses the file (phase 1), then re-embeds changed sections (phase 2). The agent's next recall automatically includes the updated content.

**Agent workflow:**
```bash
# After user says "I just updated the meeting notes"
# Wait 2-3 seconds for watcher debounce to complete
sleep 3

# Then recall normally; results reflect the latest file content
hv recall $VAULT \
  --query "standup meeting action items decisions" \
  --strategy similarity,temporal \
  --w-relevance 0.4 --w-recency 0.5 --w-importance 0.1 --w-reinforcement 0.0 \
  -k 5
```

---

### Use Case 7: "Why did we decide to use RocksDB?"

**Right** (causal strategy to trace decision chains):
```bash
# Step 1: Find the decision
hv recall $VAULT \
  --query "RocksDB storage decision rationale comparison" \
  --strategy similarity \
  --w-relevance 0.8 --w-importance 0.2 --w-recency 0.0 --w-reinforcement 0.0 \
  -k 3

# Step 2: If the result has a memory ID, trace the causal chain
hv recall $VAULT \
  --query "RocksDB storage" \
  --strategy causal \
  --seed-id <MEMORY_ID_FROM_STEP_1> \
  --causal-direction backward \
  --max-depth 3 \
  -k 5
```

**Why this works:**
- Step 1 finds the decision itself using pure relevance
- Step 2 traces backward through causal edges to find what led to the decision
- `--causal-direction backward` follows "caused_by" edges to find antecedents
- This requires memories to have been linked with `caused_by` edges during creation (either by the agent or during structured note-taking)

---

## Strategy Selection Guide

The agent should choose strategy and weights based on what the user is actually asking:

| User intent | Strategy | Weight profile | Why |
|---|---|---|---|
| Factual lookup ("what is X?") | `similarity` | `--w-relevance 1.0` (others 0) | Pure semantic match, no time/importance bias |
| Recent activity ("what happened lately?") | `temporal` or `similarity,temporal` | `--w-recency 0.6-0.8` | Recency is the signal |
| Important items ("key decisions", "critical bugs") | `similarity` | `--w-importance 0.5 --w-relevance 0.4` | Importance-tagged content surfaces first |
| Person/entity scoped | `similarity` (multi-query) | `--w-relevance 0.7` | Two recalls: one for person, one for topic, then cross-reference |
| Meeting prep (breadth) | `similarity,temporal` (multi-query) | Varies per sub-query | Multiple targeted recalls covering facts, recent changes, open issues |
| Decision history ("why did we...") | `similarity` then `causal` | `--w-relevance 0.8` | Find the decision, then trace its causal chain |
| "Anything like this before?" | `analogical` | default | Structural + embedding similarity to find patterns |

## Weight Tuning Reference

Weights are normalized to sum to 1.0 automatically. You can pass any ratio:

```bash
# These are equivalent:
--w-relevance 0.8 --w-recency 0.2
--w-relevance 4.0 --w-recency 1.0
```

| Weight | What it controls | When to boost |
|---|---|---|
| `w_relevance` | Semantic similarity to query | Factual lookups, concept search |
| `w_recency` | How recently the content was modified | "Latest", "recent", "since yesterday" |
| `w_importance` | Section importance score (from frontmatter or content signals) | Decisions, preferences, critical items |
| `w_reinforcement` | How often a section has been accessed/recalled | Frequently referenced content |

## Quality Characteristics

Based on testing with real BGE-small-en-v1.5 embeddings:

- **Direct concept queries**: 0.71-0.87 relevance (excellent)
- **Technical term queries**: 0.73 relevance (good)
- **Conversational queries**: 0.46-0.49 relevance (weak without rewriting)
- **Small corpus effect**: With fewer than ~50 sections, even irrelevant results score 0.40+. Larger vaults show better separation.
- **Multi-section content**: Meeting notes, long documents get split into sections by heading. Agent should read top-N results and synthesize, not rely on a single result.

## Agent Integration Pattern

```
User question
    |
    v
Agent: classify intent (factual? temporal? entity-scoped? decision trace?)
    |
    v
Agent: rewrite question into 1-3 retrieval cues (keyword-rich, no conversational phrasing)
    |
    v
Agent: select strategy + weights per cue (see Strategy Selection Guide)
    |
    v
hv recall (1-3 calls)
    |
    v
Agent: read top-N results, cross-reference if multi-query, cite file paths
    |
    v
Agent: synthesize answer for user
```

The agent is the intelligence layer. The vault is the retrieval layer. The agent should never pass raw user input to recall; it should always decompose, rewrite, and select the right strategy first.
