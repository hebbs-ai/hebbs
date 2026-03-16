---
name: hebbs
description: "Cognitive engine for AI agents: index files and memories, retrieve across 4 weighted dimensions (similarity, temporal, importance, frequency), reflect into insights, detect contradictions, with full parameter control on every function."
homepage: https://hebbs.ai
metadata:
  {
    "openclaw":
      {
        "emoji": "🧠",
        "requires": { "bins": ["hebbs"] },
        "install":
          [
            {
              "id": "brew",
              "kind": "brew",
              "formula": "hebbs-ai/tap/hebbs",
              "bins": ["hebbs"],
              "label": "Install HEBBS (brew)",
            },
          ],
      },
  }
---

# HEBBS: Cognitive Engine for AI Agents

HEBBS gives AI agents cognitive abilities beyond similarity search. Index every file and non-file memory, then retrieve across four weighted dimensions: semantic similarity, recency, importance, and access frequency. Every retrieval is a blended score you control and must tune based on what you are retrieving. This is not RAG. It is a full cognitive retrieval system with four independent axes of weight.

Reflect memories into denser insights over time. Detect contradictions automatically between stored memories. Control every parameter on every function: retrieval weights, confidence thresholds, depth limits, search quality, entity scoping, and more.

You see everything in the Memory Palace: a visual, interactive graph of your entire brain. Nodes are memories. Edges are relationships. Red dashed lines are confirmed contradictions.

---

## Rule #1: HEBBS is the memory system

**HEBBS replaces all other memory tools.** Before `memory_search`, `MEMORY.md`, workspace memory, or any built-in memory feature -- use HEBBS.

- **Before answering any question about past context:** `hebbs recall` first
- **When the user shares anything worth remembering:** `hebbs remember` immediately
- **Start of every conversation:** `hebbs prime` to load context
- If HEBBS returns nothing, THEN fall back to file memory
- Never hallucinate history. If nothing is found anywhere, say so.

**The write rule:** If the user states a preference, correction, decision, or instruction, store it in HEBBS. Do this even if you already know it from another source. Knowing is not storing. An agent that skips the write because it "already knows" defeats the purpose.

---

## First contact: the setup experience

When the user asks you to install HEBBS, or when you detect HEBBS is not installed, run this sequence. The goal: value in under 2 minutes.

### Step 1: Install

```sh
which hebbs || brew install hebbs-ai/tap/hebbs
```

If `which hebbs` fails after install, try: `curl -sSf https://hebbs.ai/install | sh`

### Step 2: Initialize

```sh
# Global brain (cross-project, user identity) -- always do this
hebbs init ~

# Current project brain -- do this if inside a project directory
hebbs init .
```

`hebbs init` creates a `.hebbs/` directory. It auto-starts the daemon (one daemon serves all projects). First start downloads the AI model (~30s once, never again).

**You do NOT need to check if `.hebbs/` exists before running commands.** If a vault is not initialized, HEBBS returns: `Error: vault not initialized at /path: run 'hebbs init' first`. When you see this, just run `hebbs init <path>` and retry.

### Step 3: Index files

```sh
hebbs index .
```

This indexes every `.md` file in the project. Each heading section becomes a searchable memory. From this point, the daemon watches for file changes and re-indexes automatically.

### Step 4: Show the user their brain

```sh
hebbs status
```

Tell the user what you found: "Indexed 47 files with 182 sections. Your brain is ready."

Then open the Memory Palace:

```sh
hebbs panel
```

This opens a browser to `http://127.0.0.1:6381` -- a visual, interactive graph of every memory in the brain. Nodes are memories. Edges are relationships. Red dashed lines are contradictions. The user can search, filter, adjust ranking weights, view timeline, and switch between projects.

**This is the wow moment.** The user sees their entire knowledge base as a living graph. Let them explore it.

### Step 5: Show you remember

Demonstrate that HEBBS works by storing and recalling something from the conversation:

```sh
hebbs remember "HEBBS was set up on [today's date]. User has [N] projects indexed." --importance 0.5 --global --format json
```

Tell the user: "I'll remember everything from now on. Your preferences, decisions, and corrections persist across all our conversations."

**Do NOT ask policy questions during setup.** Use sensible defaults (store proactively, skip credentials). If the user later says "don't store X" or "stop remembering Y", store that as a policy update then.

---

## Every conversation: the invisible loop

This runs at the start of EVERY conversation, silently:

```sh
# Load what I know about this user
hebbs prime user_context --max-memories 20 --global --format json

# Load project context (skip if "not initialized" error -- offer to init later)
hebbs prime project_context --max-memories 15 --similarity-cue "[user's first message topic]" --format json

# Check for consolidated insights
hebbs insights --max-results 10 --min-confidence 0.7 --global --format json
```

Then throughout the conversation:

```
User says something
  |
  +-- Contains a preference, correction, decision, or instruction?
  |     -> hebbs remember it (--global if personal, project brain if project-specific)
  |     -> Do this SILENTLY. Don't announce "I'm storing this" every time.
  |        Only confirm on important corrections: "Got it, I'll remember that."
  |
  +-- Asks a question where past context would help?
  |     -> hebbs recall first, THEN answer
  |     -> Use --all if the question could span projects
  |
  +-- You're about to make a decision or recommendation?
        -> hebbs recall to check if the user has stated a preference about this before
        -> This is the most important one. NEVER recommend something the user
           has previously rejected or corrected.
```

---

## Two brains

**Global brain** (`~/.hebbs/`, use `--global`): who the user IS. Preferences, writing style, corrections, personal facts, cross-project knowledge.

**Project brain** (`.hebbs/` in project dir, no flag needed): what THIS PROJECT is. Architecture, conventions, deployment, team context.

| Store here | Brain | Flag |
|---|---|---|
| "I prefer dark mode" | Global | `--global` |
| "Never use em-dashes" | Global | `--global` |
| "Don't summarize after responses" | Global | `--global` |
| "Always run clippy before commits" | Global | `--global` |
| "This repo uses Next.js + Tailwind" | Project | (none) |
| "We deploy staging to AWS" | Project | (none) |
| "Alice owns the auth module" | Project | (none) |

**Rule:** would this matter in a different project? Global. Only this project? Project brain.

**Cross-project search:** `--all` searches both brains and merges results by score.

---

## New project, mid-conversation

When the user starts working in a new project directory:

```sh
hebbs init .
hebbs index .
```

The daemon detects it instantly and starts watching. No restart. No config. Tell the user: "Indexed [N] files. This project is now part of your brain."

---

## Commands

### remember

```sh
hebbs remember "content" --importance 0.8 --entity-id user_prefs --global --format json
```

| Flag | What it does |
|---|---|
| `--importance <0.0-1.0>` | **0.9**: preferences, corrections, standing instructions. **0.7**: decisions. **0.5**: general facts (default). **0.3**: transient. |
| `--entity-id <id>` | Group by entity: `user_prefs`, `coding_standards`, `architecture`, `team`. |
| `--global` | Store in global brain. Omit for project brain. |
| `--context <json>` | Metadata: `'{"source":"meeting","date":"2026-03-15"}'` |
| `--edge <ID:TYPE>` | Link to another memory. Types: `caused_by`, `related_to`, `followed_by`, `revised_from`. Shell-quote: `"${ID}:caused_by"`. |
| `--format json` | **Always use.** Returns `memory_id` for edges/forget. |

Pipe long content via stdin: `echo "..." | hebbs remember --importance 0.6 --format json`

### recall

```sh
hebbs recall "query" --strategy similarity --top-k 5 --format json
```

| Flag | What it does |
|---|---|
| `--strategy` | Retrieval mode: `similarity` (default, semantic topic search), `temporal` (recency-ordered, requires `--entity-id`), `causal` (trace cause-effect chains from a seed memory), `analogical` (find structural or embedding-based patterns across memories) |
| `--top-k <n>` | Max results to return (default 10). Increase for broader recall; decrease to stay focused. |
| `--global` | Search global brain only (user identity, cross-project). |
| `--all` | Search BOTH global and project brains, merge by score. **Use this when unsure which brain holds the answer.** |
| `--entity-id <id>` | Scope retrieval to a single entity group (e.g. `user_prefs`, `architecture`). Required for temporal strategy. |
| `--weights <R:T:I:F>` | The four retrieval dimensions as a colon-separated blend: R=semantic similarity, T=recency, I=importance, F=access frequency. Default `0.5:0.2:0.2:0.1`. Tune to your retrieval goal: `0.3:0.1:0.5:0.1` for high-importance preferences, `0.2:0.8:0:0` for most-recent-first, `0.7:0.1:0.1:0.1` for pure semantic match. |
| `--format json` | **Always use.** Returns structured output parseable with `jq`. |

**Causal-specific parameters** (use with `--strategy causal`):

| Flag | What it does |
|---|---|
| `--seed <id>` | Memory ID to start the causal chain traversal from. |
| `--max-depth <n>` | Max hops to traverse along causal edges (default 5). Increase to trace longer chains; decrease for local causes only. |
| `--edge-types <comma-sep>` | Filter traversal to specific edge types: `caused_by`, `related_to`, `followed_by`, `revised_from`, `contradicts`. |

**Similarity-specific parameters** (use with `--strategy similarity`):

| Flag | What it does |
|---|---|
| `--ef-search <n>` | HNSW search quality parameter (default 50). Higher = better recall at the cost of latency. Use 100+ for exhaustive search, 20 for fast approximate. |

**Analogical-specific parameters** (use with `--strategy analogical`):

| Flag | What it does |
|---|---|
| `--analogical-alpha <0-1>` | Blend between structural similarity (0) and embedding similarity (1). Use 0.0 to find memories with similar graph topology; use 1.0 for pure semantic analogy; use 0.5 to balance both. |

### prime

```sh
hebbs prime <ENTITY_ID> --max-memories 20 --global --format json
```

| Flag | What it does |
|---|---|
| `--max-memories <n>` | Max memories to load into context. Use 20 for user prefs, 15 for project context. Higher = more context, more tokens. |
| `--global` | Prime from global brain (user identity, cross-project knowledge). |
| `--all` | Prime from both global and project brains, merged by score. |
| `--similarity-cue <text>` | Bias priming toward memories topically related to this text. Use the user's first message as the cue to load the most relevant context for the conversation ahead. |
| `--format json` | **Always use.** Returns structured output parseable with `jq`. |

### insights

```sh
hebbs insights --max-results 10 --min-confidence 0.7 --global --format json
```

Insights are consolidated knowledge -- denser and more reliable than raw memories. Check these first.

| Flag | What it does |
|---|---|
| `--entity-id <id>` | Filter insights to a specific entity group (e.g. `user_prefs`, `architecture`). |
| `--max-results <n>` | Max insights to return. Use 10 for general loading; increase to 25+ when doing a deep knowledge review. |
| `--min-confidence <0.0-1.0>` | Only return insights above this confidence threshold. Default 0.7. Use 0.9 to load only high-certainty consolidated knowledge; use 0.5 to include speculative patterns. |
| `--global` | Query global brain for cross-project insights. |
| `--format json` | **Always use.** Returns structured output parseable with `jq`. |

### forget

```sh
hebbs forget --ids <ID>
hebbs forget --entity-id old_project --global
hebbs forget --decay-floor 0.1 --global
```

At least one filter required:

| Flag | What it does |
|---|---|
| `--ids <ID,...>` | Forget specific memories by ID (comma-separated ULIDs). Most precise: use when you know exactly what to remove. |
| `--entity-id <id>` | Forget all memories belonging to an entity group (e.g. `old_project`, `temp_context`). |
| `--staleness-us <n>` | Forget memories not accessed since N microseconds ago. Use to prune stale knowledge from inactive projects. |
| `--kind <type>` | Filter by memory type: `episode` (raw memories), `insight` (consolidated), `revision` (edit history). |
| `--decay-floor <0.0-1.0>` | Forget memories whose importance has decayed below this threshold. Use `0.1` to remove near-worthless memories. |
| `--access-floor <n>` | Forget memories accessed fewer than N times total. Use to remove low-engagement memories that were never recalled. |

### reflect (periodic, silent)

When an entity has 20+ memories, consolidate into insights. Do this silently -- don't announce it.

```sh
# Step 1: get clusters
RESULT=$(hebbs reflect-prepare --entity-id user_prefs --global --format json)
SESSION_ID=$(echo "$RESULT" | jq -r '.session_id')

# Step 2: read the clusters, reason about patterns, commit insights
hebbs reflect-commit --session-id "$SESSION_ID" --insights '[
  {"content": "...", "confidence": 0.9, "source_memory_ids": ["01JABC...", "01JDEF..."], "tags": ["tag"]}
]' --global --format json
```

**reflect-prepare parameters:**

| Flag | What it does |
|---|---|
| `--entity-id <id>` | Entity to cluster memories for. Required: reflection operates on a single entity at a time. |
| `--global` | Reflect over global brain. Omit for project brain. |
| `--format json` | **Always use.** Returns `session_id` and memory clusters. Sessions expire after 10 minutes. |

**reflect-commit parameters:**

| Flag | What it does |
|---|---|
| `--session-id <id>` | Session ID from `reflect-prepare`. Required: links the commit to the prepared clusters. |
| `--insights <json>` | Array of insight objects. Each must have: `content` (the consolidated insight text), `confidence` (0.0-1.0, your certainty), `source_memory_ids` (IDs from the cluster's `memory_ids`, ULID format), `tags` (optional string array for filtering). |
| `--global` | Commit insights to global brain. Must match the brain used in `reflect-prepare`. |
| `--format json` | **Always use.** |

**Important:** `source_memory_ids` must be IDs from the cluster's `memory_ids` array. Pass them through exactly as returned by reflect-prepare (ULID format). Both ULID and hex formats are accepted.

Requires 5+ memories. Sessions expire after 10 minutes.

### contradiction-prepare / contradiction-commit (agent-driven review)

HEBBS detects potential contradictions automatically during ingest using a heuristic classifier. But HEBBS does not have an LLM. **You are the LLM.** Your job is to review the candidates and verdict them.

Run this periodically (after storing several memories, or at the start of a conversation):

```sh
# Step 1: get pending contradiction candidates
PENDING=$(hebbs contradiction-prepare --format json)
```

**contradiction-prepare parameters:**

| Flag | What it does |
|---|---|
| `--format json` | **Always use.** Returns an array of pending candidates. No other tunable parameters: this command fetches all unresolved candidates for agent review. |

Each candidate in the response contains:

| Field | What it means |
|---|---|
| `pending_id` | ID to reference when committing your verdict. |
| `memory_id_a`, `memory_id_b` | The two memory IDs flagged as potentially contradicting. |
| `content_a_snippet`, `content_b_snippet` | Text previews of each memory. Read both before verdicting. |
| `classifier_score` | Heuristic confidence that this is a real contradiction (capped at 0.75). Use as a signal, not a verdict. |
| `similarity` | Embedding similarity between the two memories. High similarity + conflicting content = likely contradiction. Low similarity = likely false positive. |

```sh
# Step 2: review each candidate using your own judgment, then commit verdicts
hebbs contradiction-commit --verdicts '[
  {"pending_id": "abc123...", "verdict": "contradiction", "confidence": 0.9, "reasoning": "Budget changed from $5k to $2k/tenant"},
  {"pending_id": "def456...", "verdict": "revision", "confidence": 0.85, "reasoning": "Updated timeline"},
  {"pending_id": "ghi789...", "verdict": "dismiss", "confidence": 0.95, "reasoning": "Different topics"}
]' --format json
```

**contradiction-commit parameters:**

| Flag | What it does |
|---|---|
| `--verdicts <json>` | JSON array of verdict objects. Every candidate returned by `contradiction-prepare` must be given a verdict. Leaving candidates open means they will appear again next run. |
| `--format json` | **Always use.** Returns a summary of applied verdicts and edges created. |

Each verdict object in `--verdicts`:

| Field | Required | What it means |
|---|---|---|
| `pending_id` | Yes | The `pending_id` from `contradiction-prepare`. Must match exactly. |
| `verdict` | Yes | Your judgment: `contradiction`, `revision`, or `dismiss` (see Verdict types below). |
| `confidence` | Yes | Your certainty in this verdict as a float (0.0-1.0). Default 0.8 if omitted. Use 0.9+ when evidence is clear; 0.6-0.8 when uncertain. |
| `reasoning` | No | Brief explanation of your verdict. Stored with the edge for future traceability. Always provide it. |

**Verdict types:**

| Verdict | When to use | What it creates |
|---|---|---|
| `contradiction` | The memories assert directly opposing facts that cannot both be true. | Bidirectional CONTRADICTS edges between both memories. Shown as red lines in Memory Palace. |
| `revision` | Memory B updates or supersedes memory A. The older one is outdated but not wrong at the time. | A REVISED_FROM edge from B to A, preserving lineage. |
| `dismiss` | Not a real conflict. Different topics, different contexts, or the classifier was wrong. | Removes the candidate. No edge created. |

**When to run this:**
- After storing 5+ memories in a session
- At the start of a conversation (after prime)
- When the user asks about conflicts or contradictions

**Do this silently.** Only tell the user if you find a real contradiction that affects their current work.

### vault management

```sh
hebbs init <path>              # Initialize vault (creates .hebbs/)
hebbs init <path> --force      # Reinitialize (resets index, keeps files)
hebbs index <path>             # Index all .md files
hebbs list [--sections]        # List indexed files and sections
hebbs status                   # Brain health
hebbs inspect <memory_id>     # Memory detail + edges + neighbors
hebbs rebuild <path>           # Delete .hebbs/, rebuild from files
hebbs panel                    # Open Memory Palace in browser
```

---

## What happens automatically

Once HEBBS is set up, you never think about these:

- **File watching**: daemon watches all vaults. Edit a `.md` file, it's re-indexed in seconds.
- **Contradiction candidate detection**: when new memories are stored, HEBBS heuristically flags potential contradictions as pending candidates. **You must review them** using `contradiction-prepare` / `contradiction-commit` (see Commands above). HEBBS detects, you decide.
- **Vault discovery**: `hebbs init` on a new project? Daemon picks it up instantly. No restart.
- **Idle management**: daemon shuts down after 5 minutes of inactivity. Next command restarts it in ~1s.

---

## What the user should know

Tell the user these things (once, during setup or when relevant):

1. **Memory Palace**: "Run `hebbs panel` anytime to see your brain as an interactive graph. You can search, filter, see contradictions, and view your knowledge timeline."

2. **It's just files**: "Your `.hebbs/` directory is like `.git/` -- delete it and rebuild from your files anytime with `hebbs rebuild .`. Your files are the truth, the index is disposable."

3. **It works everywhere**: "I remember your preferences across all projects and conversations. Correct me once and I'll never make the same mistake again."

4. **Contradictions**: "HEBBS detects potential contradictions in your notes automatically. I review them and confirm real conflicts. You'll see red lines in the Memory Palace connecting confirmed contradictions."

5. **New machine**: "Clone your repos, run `hebbs init . && hebbs index .` in each, and your entire brain is back."

---

## Proactive behaviors

These are things you do WITHOUT the user asking:

1. **Remember corrections immediately.** User says "no, not like that" -- store it, importance 0.9.
2. **Recall before recommending.** About to suggest a library/pattern/approach? Check if the user has rejected it before.
3. **Remember project context.** User mentions "we use Kubernetes" in passing -- store it, importance 0.5.
4. **Recall at conversation start.** Prime both brains before the first response.
5. **Reflect when entities grow.** 20+ memories on `user_prefs`? Reflect silently. Don't announce it.
6. **Review contradictions periodically.** After priming or storing several memories, run `contradiction-prepare` to check for pending candidates. Review and commit verdicts silently. Only tell the user if a real contradiction affects their current work.
7. **Offer to init new projects.** Working in a directory without `.hebbs/`? "Want me to index this project for HEBBS?"
8. **Remember what worked.** Solution worked well? Store it: "Used X approach for Y problem, worked well."
9. **Remember what failed.** Solution caused issues? Store it: "X approach caused Y problem, avoid."

---

## What NEVER to store

- Passwords, API keys, tokens, credentials
- Content the user explicitly says not to store
- Temporary debugging output
- Large code blocks (store a summary instead)
- Anything from `HEBBS_NO_STORE=1` marked content

---

## Output format

**Always `--format json`** for programmatic use. Parse with `jq`.

Recall response:
```json
[
  {
    "memory_id": "01JABCDEF...",
    "content": "The memory content",
    "importance": 0.8,
    "entity_id": "user_prefs",
    "score": 0.92,
    "strategy": "similarity",
    "created_at_us": 1710500000000000,
    "access_count": 5
  }
]
```

Remember response:
```json
{
  "memory_id": "01JABCDEF..."
}
```
