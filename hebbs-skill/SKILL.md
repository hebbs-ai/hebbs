---
name: hebbs
description: "Cognitive memory engine: remember, recall, reflect, and forget knowledge across all projects with HEBBS."
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

# HEBBS: Your Brain

HEBBS gives you perfect memory. Every preference, decision, correction, and piece of context you share is stored, indexed, and recalled instantly across all your projects, all your sessions, forever.

It also indexes your files. Every markdown file in every project becomes searchable by meaning, not just keywords. Contradictions between your notes are detected automatically. Your knowledge consolidates into insights over time.

You see everything in the Memory Palace -- a visual, interactive graph of your entire brain.

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
| `--strategy` | `similarity` (default, topic search), `temporal` (recent activity, needs `--entity-id`), `causal` (trace chains), `analogical` (find patterns) |
| `--top-k <n>` | Max results (default 10) |
| `--global` | Search global brain only |
| `--all` | Search BOTH brains, merge by score. **Use this when unsure.** |
| `--entity-id <id>` | Scope to entity. Required for temporal. |
| `--weights <R:T:I:F>` | Scoring blend. Default `0.5:0.2:0.2:0.1`. Use `0.3:0.1:0.5:0.1` for preferences/decisions. Use `0.2:0.8:0:0` for recent-first. |
| `--format json` | **Always use.** |

Causal-specific: `--seed <id>`, `--max-depth <n>` (default 5), `--edge-types <comma-sep>`
Similarity-specific: `--ef-search <n>` (default 50, higher = better quality)
Analogical-specific: `--analogical-alpha <0-1>` (0 = structural, 1 = embedding)

### prime

```sh
hebbs prime <ENTITY_ID> --max-memories 20 --global --format json
```

| Flag | What it does |
|---|---|
| `--max-memories <n>` | Max memories to return |
| `--global` | Prime from global brain |
| `--all` | Prime from both brains |
| `--similarity-cue <text>` | Bias toward memories related to this topic |
| `--format json` | **Always use.** |

### insights

```sh
hebbs insights --max-results 10 --min-confidence 0.7 --global --format json
```

Insights are consolidated knowledge -- denser and more reliable than raw memories. Check these first.

| Flag | What it does |
|---|---|
| `--entity-id <id>` | Filter by entity |
| `--max-results <n>` | Max insights |
| `--min-confidence <0.0-1.0>` | Confidence threshold |
| `--global` | Query global brain |

### forget

```sh
hebbs forget --ids <ID>
hebbs forget --entity-id old_project --global
hebbs forget --decay-floor 0.1 --global
```

At least one filter required: `--ids`, `--entity-id`, `--staleness-us`, `--kind` (episode/insight/revision), `--decay-floor`, `--access-floor`.

### reflect (periodic, silent)

When an entity has 20+ memories, consolidate into insights. Do this silently -- don't announce it.

```sh
# Step 1: get clusters
RESULT=$(hebbs reflect-prepare --entity-id user_prefs --global --format json)
SESSION_ID=$(echo "$RESULT" | jq -r '.session_id')

# Step 2: read the clusters, reason about patterns, commit insights
hebbs reflect-commit --session-id "$SESSION_ID" --insights '[
  {"content": "...", "confidence": 0.9, "source_memory_ids": ["hex...", "hex..."], "tags": ["tag"]}
]' --global --format json
```

**Important:** `source_memory_ids` must be hex-encoded IDs from the cluster's `memory_ids` array, NOT the ULID from `memories[].memory_id`.

Requires 5+ memories. Sessions expire after 10 minutes.

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
- **Contradiction detection**: "uses JWT" in one note, "migrated to sessions" in another? HEBBS detects it, creates a CONTRADICTS edge, writes a contradiction file to `contradictions/`. Red dashed lines in the Memory Palace.
- **Vault discovery**: `hebbs init` on a new project? Daemon picks it up instantly. No restart.
- **Idle management**: daemon shuts down after 5 minutes of inactivity. Next command restarts it in ~1s.

---

## What the user should know

Tell the user these things (once, during setup or when relevant):

1. **Memory Palace**: "Run `hebbs panel` anytime to see your brain as an interactive graph. You can search, filter, see contradictions, and view your knowledge timeline."

2. **It's just files**: "Your `.hebbs/` directory is like `.git/` -- delete it and rebuild from your files anytime with `hebbs rebuild .`. Your files are the truth, the index is disposable."

3. **It works everywhere**: "I remember your preferences across all projects and conversations. Correct me once and I'll never make the same mistake again."

4. **Contradictions**: "If your notes contradict each other, I'll flag it. You'll see red lines in the Memory Palace connecting the conflicting memories."

5. **New machine**: "Clone your repos, run `hebbs init . && hebbs index .` in each, and your entire brain is back."

---

## Proactive behaviors

These are things you do WITHOUT the user asking:

1. **Remember corrections immediately.** User says "no, not like that" -- store it, importance 0.9.
2. **Recall before recommending.** About to suggest a library/pattern/approach? Check if the user has rejected it before.
3. **Remember project context.** User mentions "we use Kubernetes" in passing -- store it, importance 0.5.
4. **Recall at conversation start.** Prime both brains before the first response.
5. **Reflect when entities grow.** 20+ memories on `user_prefs`? Reflect silently. Don't announce it.
6. **Offer to init new projects.** Working in a directory without `.hebbs/`? "Want me to index this project for HEBBS?"
7. **Remember what worked.** Solution worked well? Store it: "Used X approach for Y problem, worked well."
8. **Remember what failed.** Solution caused issues? Store it: "X approach caused Y problem, avoid."

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
