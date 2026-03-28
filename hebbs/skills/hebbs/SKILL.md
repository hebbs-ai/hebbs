---
name: hebbs
description: "Cognitive engine for AI agents: index files into atomic propositions and entity graphs, retrieve across 4 weighted dimensions (similarity, temporal, importance, frequency), automatically reflect into insights, automatically detect and resolve contradictions, with full parameter control on every function."
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

HEBBS gives AI agents cognitive abilities beyond similarity search. Index every file into atomic propositions and entity graphs, store non-file memories, then retrieve across four weighted dimensions: semantic similarity, recency, importance, and access frequency. Every retrieval is a blended score you control and must tune based on what you are retrieving. This is not RAG. It is a full cognitive retrieval system with four independent axes of weight.

HEBBS handles all intelligence internally. Reflection, contradiction detection, and proposition extraction are fully autonomous, powered by a configured LLM provider. The agent's job is to store and recall. HEBBS handles the rest.

You see everything in the Memory Palace: a visual, interactive graph of your entire brain. Nodes are memories. Edges are relationships. Red dashed lines are confirmed contradictions.

---

## Minimum viable agent API (3 commands)

If you integrate nothing else, integrate these three:

```sh
hebbs recall "what database are we using" --format json   # search memory
hebbs remember "user prefers dark mode" --format json      # store a fact
hebbs status                                               # check vault health
```

Everything else (prime, insights, reflect, forget, inspect) is optional power tooling. These three commands are the complete read/write/health interface for an AI agent. If your tool framework limits you to a small surface area, implement only these.

---

## Rule #1: HEBBS is the retrieval system

**For retrieval beyond your prompt-level config, use HEBBS first.** Your prompt-level files (CLAUDE.md, MEMORY.md, .cursorrules) are a fast cache for standing instructions, build commands, and formatting rules. They load instantly. Keep using them. But when you need context about past decisions, project history, cross-file knowledge, or user preferences beyond what's in your prompt window: HEBBS first, always.

- **Before answering any question about past context:** `hebbs recall` first
- **When the user shares anything worth remembering:** `hebbs remember` immediately, AND update your local files as you normally would (MEMORY.md, config files, etc.)
- **Start of every conversation:** `hebbs prime` to load context beyond what's in your prompt
- If HEBBS returns nothing, THEN fall back to file search
- Never hallucinate history. If nothing is found anywhere, say so.

**The write rule:** Keep saving wherever you normally save. Update files, write to MEMORY.md, maintain CLAUDE.md. That's your job. But ALSO store important things in HEBBS so they're searchable, scored, and available across projects. HEBBS is additive, not exclusive.

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
# Global brain (cross-project, user identity)
hebbs init ~ --provider openai --key $OPENAI_API_KEY

# Current project brain (do this if inside a project directory)
hebbs init . --provider openai --key $OPENAI_API_KEY
```

`hebbs init` creates a `.hebbs/` directory and validates LLM connectivity. It auto-starts the daemon (one daemon serves all projects). `--model` is optional (defaults to `gpt-4o-mini` for OpenAI). When the provider is OpenAI, embedding auto-configures to `text-embedding-3-small` with the same key. No local model download needed.

LLM configuration is required. HEBBS uses it internally for proposition extraction, contradiction resolution, and reflection. Supported providers: `anthropic`, `openai`, `gemini`, `ollama`.

For other cloud providers: `hebbs init . --provider anthropic --key $ANTHROPIC_API_KEY`

For local (no API key): `hebbs init . --provider ollama`

For CI/pipelines, use `--api-key-env` to reference an env var name instead of passing the key: `hebbs init . --provider openai --api-key-env OPENAI_API_KEY`

If indexing fails with rate limit errors (429), lower concurrency: `hebbs init . --provider openai --key $KEY --max-concurrent 2` or add `[api]\nmax_concurrent_requests = 2` to `~/.hebbs/config.toml`.

**You do NOT need to check if `.hebbs/` exists before running commands.** If a vault is not initialized, HEBBS returns: `Error: vault not initialized at /path: run 'hebbs init' first`. When you see this, just run `hebbs init <path>` and retry.

### Step 3: Index files

```sh
hebbs index .
```

This indexes every `.md` file in the project. HEBBS extracts atomic facts (propositions) and builds a knowledge graph. Each file produces document-level, proposition-level, and entity memories for precision retrieval. From this point, the daemon watches for file changes and re-indexes automatically.

### Step 4: Show the user their brain

```sh
hebbs status
```

Tell the user what you found: "Indexed 47 files with 182 sections. Your brain is ready."

Then open the Memory Palace:

```sh
hebbs panel
```

This opens a browser to `http://127.0.0.1:6381`, a visual, interactive graph of every memory in the brain. Nodes are memories. Edges are relationships. Red dashed lines are contradictions. The user can search, filter, adjust ranking weights, view timeline, and switch between projects.

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

# Load project context (skip if "not initialized" error, offer to init later)
hebbs prime project_context --max-memories 15 --similarity-cue "[user's first message topic]" --format json

# Check for consolidated insights
hebbs insights --max-results 10 --min-confidence 0.7 --global --format json

# Load retrieval rules (how to use HEBBS effectively for this vault)
# Check for compiled rules file first (fastest), then fall back to prime
# If .hebbs/retrieval-rules.md exists: read it directly (no daemon call needed)
# If not: hebbs prime retrieval-instructions --max-memories 20 --format json
# If neither exists: use defaults (similarity, k=10)
```

The retrieval rules tell you which strategy, weights, and k to use for different query types. They are generated by the `hebbs-tune` skill (see `hebbs-skill/tune/SKILL.md`). Without rules, default to similarity with k=10 and entity names in cues.

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

Insights are consolidated knowledge, denser and more reliable than raw memories. Check these first.

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

### reflect (optional explicit trigger)

HEBBS automatically reflects when an entity accumulates enough memories (20+). It clusters related memories and consolidates them into insights using the configured LLM. You never need to do this manually.

To trigger reflection explicitly (e.g. after a large import):

```sh
hebbs reflect --entity-id user_prefs --global --format json
```

| Flag | What it does |
|---|---|
| `--entity-id <id>` | Entity to reflect on. If omitted, reflects globally across all entities. |
| `--global` | Reflect over global brain. Omit for project brain. |
| `--format json` | **Always use.** Returns `insights_created` count. |

### config

```sh
hebbs config show                          # Show all config
hebbs config get llm.provider              # Get a specific key
hebbs config set llm.model claude-haiku-4-5-20251001   # Set a specific key
```

Supported keys: `llm.provider`, `llm.model`, `llm.api_key_env`, `llm.base_url`, `embedding.model`, `embedding.dimensions`, and more.

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

- **File watching**: daemon watches all vaults. Edit a `.md` file, HEBBS extracts atomic facts and rebuilds the knowledge graph in seconds.
- **Contradiction detection and resolution**: when new memories conflict with existing ones, HEBBS detects and resolves them automatically using the configured LLM. Contradictions appear as red edges in Memory Palace and are written to `contradictions/`.
- **Reflection**: HEBBS automatically clusters memories and consolidates them into insights when an entity accumulates enough memories. Optionally trigger with `hebbs reflect`.
- **Vault discovery**: `hebbs init` on a new project? Daemon picks it up instantly. No restart.
- **Idle management**: daemon shuts down after 5 minutes of inactivity. Next command restarts it in ~1s.

---

## What the user should know

Tell the user these things (once, during setup or when relevant):

1. **Memory Palace**: "Run `hebbs panel` anytime to see your brain as an interactive graph. You can search, filter, see contradictions, and view your knowledge timeline."

2. **Portable cognition**: "Your `.hebbs/` directory is a self-contained index. Build it once, then drop it on another machine or share it with your team. Everyone gets the same memory instantly. Delete it and rebuild from your files anytime with `hebbs rebuild .`."

3. **You control what goes in**: "`.hebbsignore` works like `.gitignore`. Your private files stay private. Your agents only see what you allow."

4. **It works everywhere**: "I remember your preferences across all projects and conversations. Correct me once and I'll never make the same mistake again."

4. **Contradictions**: "HEBBS detects and resolves contradictions in your notes automatically. You'll see red lines in the Memory Palace connecting confirmed contradictions, and resolution details in `contradictions/`."

5. **New machine**: "Clone your repos, run `hebbs init . && hebbs index .` in each, and your entire brain is back."

---

## Proactive behaviors

These are things you do WITHOUT the user asking:

1. **Remember corrections immediately.** User says "no, not like that": store it, importance 0.9.
2. **Recall before recommending.** About to suggest a library/pattern/approach? Check if the user has rejected it before.
3. **Remember project context.** User mentions "we use Kubernetes" in passing: store it, importance 0.5.
4. **Recall at conversation start.** Prime both brains before the first response.
5. **Reflection happens automatically.** HEBBS consolidates memories into insights on its own. Optionally `hebbs reflect --entity-id <id>` to trigger now.
6. **Contradictions are resolved automatically.** If the user asks about conflicts, point them to Memory Palace or `contradictions/`.
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
