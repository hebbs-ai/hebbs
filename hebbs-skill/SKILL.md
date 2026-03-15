---
name: hebbs
description: Cognitive memory engine - remember, recall, reflect, and forget knowledge with HEBBS.
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

# HEBBS - Cognitive Memory Engine

HEBBS is a local-first memory engine. It stores, indexes, and retrieves knowledge using multiple recall strategies and can consolidate raw memories into higher-order insights through reflection.

## Two brains, one user

HEBBS uses two brains. You (the agent) decide which to query and where to store.

**Global brain** (`~/.hebbs/`): the user as a person. Preferences, writing style, communication style, cross-project knowledge, personal facts. Persists across all projects. Access with `--global`.

**Project brain** (`.hebbs/` in the project directory): project-specific context. Architecture decisions, conventions, deployment patterns, team context. Discovered automatically when you run commands inside a project directory.

```
~/.hebbs/                    <- global brain (user identity)
~/projects/foo/.hebbs/       <- project brain (foo-specific)
~/projects/bar/.hebbs/       <- project brain (bar-specific)
```

### Where to store

| Memory type | Brain | Example |
|---|---|---|
| User preference | Global | "prefers dark mode", "writes in AP style" |
| Writing/communication style | Global | "never use em-dashes", "keep responses terse" |
| Personal fact | Global | "senior Rust engineer", "based in SF" |
| Correction to your behavior | Global | "don't summarize after every response" |
| Project convention | Project | "this repo uses Next.js + Tailwind" |
| Architecture decision | Project | "chose PostgreSQL over MongoDB for X reason" |
| Deployment/infra context | Project | "staging is on AWS, prod on GCP" |
| Team member context | Project | "Alice owns the auth module" |

When unsure, ask: "Would this matter in a different project?" If yes, store globally. If no, store in the project brain.

### Where to recall

| Situation | What to query |
|---|---|
| Start of conversation | Global prime (user context) + project prime (project context) |
| User asks about preferences | Global recall |
| User asks about project specifics | Project recall |
| User asks a general question | Both: project recall first, then global if insufficient |

## Your two essential commands

**Store something the user said or decided:**
```
hebbs remember "The user prefers dark mode" --importance 0.8 --entity-id user_prefs --global --format json
```

**Retrieve context before answering a question:**
```
hebbs recall "What are the user's UI preferences?" --strategy similarity --top-k 5 --global --format json
```

These two commands are your primary interface to HEBBS. Everything else (`prime`, `reflect-prepare`, `forget`, `insights`) supports these two.

## Trigger

Use HEBBS **before any other memory source** (file-based memory, memory_search, MEMORY.md, workspace memory files) when the user:

- Asks what they previously said, decided, or preferred
- Asks a question that prior context would help answer
- Corrects you or says "I told you before..."
- Shares a preference, decision, instruction, or fact
- Starts a new session (prime both brains immediately)
- Asks for patterns or insights across past conversations

**Write-path rule:** If the user states a preference, correction, or standing instruction, always `remember` it in HEBBS. Do this even if the same fact already exists in file memory, workspace memory, or your conversation context. HEBBS is the source of truth. Knowing a fact is not the same as storing it.

If another memory tool exists (e.g. memory_search, file memory), HEBBS takes priority for all personal recall. Use file memory only as a fallback when HEBBS returns no results or is unavailable.

## Priority and fallback chain

When answering from memory, follow this order:

1. **HEBBS insights** (`hebbs insights`) for consolidated, high-confidence knowledge
2. **HEBBS recall** (`hebbs recall`) for raw memories matching the query
3. **File memory** (memory_search, MEMORY.md, memory/*.md) only if HEBBS returns nothing or is unreachable
4. If all sources are empty, say so. Do not guess or hallucinate history.

When storing new information:

1. Write to HEBBS immediately (`hebbs remember`) with the correct brain (global or project)
2. Optionally mirror critical facts to file memory as a backup

## Capability tiers

| Tier | What works | Requirements |
|---|---|---|
| **Basic memory** | `remember`, `recall`, `forget`, `prime` | `hebbs init` on the target directory |
| **File-backed memory** | Vault watcher indexes markdown files automatically | `hebbs watch` running |
| **Semantic recall** | Similarity, temporal, causal, analogical strategies | ONNX embedder included in binary |
| **Reflection / insights** | `reflect-prepare` + `reflect-commit`, `insights` | Agent acts as the LLM (no API key needed) |

All tiers work out of the box with a single binary. No server, no external LLM, no API key required.

## First-run setup

### Phase 1: Install binary

Check if the binary exists:
```
which hebbs
```

If missing, install:
```
brew install hebbs-ai/tap/hebbs
```

Or on any platform (Linux, macOS):
```
curl -sSf https://hebbs.ai/install | sh
```

### Phase 2: Initialize brains

Initialize the global brain (if it does not exist):
```
hebbs init ~/.hebbs
```

Initialize the project brain (if inside a project and `.hebbs/` does not exist):
```
hebbs init .
```

### Phase 3: Index files (project brain)

If the project has markdown files to index:
```
hebbs index .
```

### Phase 4: Verify

```
hebbs remember "HEBBS setup verified" --importance 0.1 --entity-id _system --global --format json
hebbs recall "setup verified" --top-k 1 --global --format json
```

If recall returns the memory, the full pipeline is working. Clean up:
```
hebbs forget --entity-id _system --global
```

## Policy bootstrap

On the first substantive interaction with a new user, check whether a memory policy exists:

```
hebbs recall "memory policy" --entity-id _policy --top-k 1 --global --format json
```

If results are returned, load the policy and apply it. Do not re-ask.

If no results are returned, and the user's message is substantive (not a smoke test or "hello"), ask for a brief memory policy:

> HEBBS is your memory system. Before I start using it, I'd like to understand your preferences. This takes about 30 seconds and I'll only ask once.
>
> 1. **What should I store?** (e.g., preferences, decisions, project context, corrections, everything)
> 2. **What should I NOT store?** (e.g., personal info, credentials, temporary thoughts, nothing off-limits)
> 3. **Should I store proactively** when you mention something, or **only when you explicitly ask** me to remember?
> 4. **Any privacy boundaries?** (e.g., no names of other people, no financial info)
>
> If you'd rather skip this, I'll use sensible defaults.

Store each answer under entity `_policy` with importance 0.95 in the **global** brain:

```
hebbs remember "Store policy: [user's answer]" --importance 0.95 --entity-id _policy --global --format json
hebbs remember "Exclude policy: [user's answer]" --importance 0.95 --entity-id _policy --global --format json
hebbs remember "Storage mode: [proactive|explicit-only]" --importance 0.95 --entity-id _policy --global --format json
hebbs remember "Privacy policy: [user's answer]" --importance 0.95 --entity-id _policy --global --format json
```

If the user skips setup, store the defaults:

```
hebbs remember "Memory policy: defaults active - store preferences and decisions proactively, skip sensitive personal info and credentials" --importance 0.95 --entity-id _policy --global --format json
```

**Default policy** (when user skips):

| Setting | Default |
|---|---|
| What to store | Preferences, decisions, corrections, project context |
| What not to store | Credentials, API keys, sensitive personal info |
| Storage mode | Proactive |
| Privacy | No credentials or secrets |

## Operations

| Situation | Operation | Command |
|---|---|---|
| User shares a personal preference | Store globally | `hebbs remember --global` |
| User shares a project convention | Store in project | `hebbs remember` |
| User asks about past context | Retrieve | `hebbs recall` (project) + `hebbs recall --global` |
| User corrects your behavior | Store globally (importance 0.9) | `hebbs remember --global` |
| Start of a new conversation | Load both contexts | `hebbs prime --global` + `hebbs prime` |
| Want consolidated patterns | Get distilled knowledge | `hebbs insights` |
| 20+ raw memories accumulated | Consolidate into insights | `hebbs reflect-prepare` + `reflect-commit` |
| Outdated or wrong memories | Remove them | `hebbs forget` |

## Commands

### Remember - store a memory

```
hebbs remember "The user prefers dark mode in all applications" --importance 0.8 --entity-id user_prefs --global --format json
```

> **Always use `--format json` when you need the memory ID** (e.g. for `--edge` on a subsequent `remember`). Extract the ID with: `jq -r '.memory_id'`
>
> **Warning:** Capture the memory ID from `--format json` output **before** referencing it in `--edge`. Do not parse IDs from human-format output.

Flags:
- `--importance <0.0-1.0>` - how important this memory is (default 0.5). Use 0.8+ for user preferences, decisions, corrections. Use 0.3 for transient observations.
- `--entity-id <id>` - group memories by entity (e.g. `user_prefs`, `project_alpha`, a person's name). Omit for unscoped context.
- `--global` - store in the global brain (~/.hebbs/) instead of the project brain.
- `--context <json>` - arbitrary metadata as JSON object (e.g. `'{"source":"email","topic":"billing"}'`).
- `--edge <TARGET_ID:EDGE_TYPE[:CONFIDENCE]>` - link to another memory (repeatable). Types: `caused_by`, `related_to`, `followed_by`, `revised_from`, `insight_from`. **Shell quoting:** use `"${MEM_ID}:edge_type"` to avoid zsh variable modifier expansion.

### Recall - retrieve relevant memories

```
hebbs recall "What does the user prefer for UI themes?" --strategy similarity --top-k 5 --global --format json
```

Four strategies:

| Strategy | When to use | Example |
|---|---|---|
| `similarity` | Find memories related to a topic | "What do we know about deployment?" |
| `temporal` | Get recent activity for an entity | "What happened today with project X?" |
| `causal` | Trace cause-effect chains from a memory | "What led to this decision?" |
| `analogical` | Find structurally similar patterns | "Have we seen a problem like this before?" |

**Core flags:**
- `--strategy <similarity|temporal|causal|analogical>` - recall strategy (default: similarity).
- `--top-k <n>` - max results (default 10).
- `--entity-id <id>` - scope to entity (required for temporal).
- `--global` - recall from the global brain instead of the project brain.
- `--format json` - machine-readable output.

**Scoring weights** - control how results are ranked. The composite score blends four signals: `relevance x recency x importance x reinforcement`. Default weights are `0.5:0.2:0.2:0.1`.
- `--weights <R:T:I:F>` - four colon-separated floats.
- `1:0:0:0` - pure semantic similarity.
- `0.2:0.8:0:0` - heavily favor recent memories.
- `0.3:0.1:0.5:0.1` - prioritize high-importance memories.

**Strategy-specific flags:**

| Flag | Strategy | Default | Description |
|---|---|---|---|
| `--ef-search <n>` | similarity | 50 | HNSW search quality. Higher = more accurate, slower. |
| `--time-range <START:END>` | temporal | unbounded | Microsecond timestamps. Omit for newest-first up to top_k. |
| `--seed <hex_id>` | causal | auto-detect | Starting memory for graph traversal. |
| `--max-depth <n>` | causal | 5 (max 10) | Maximum hops from seed memory. |
| `--edge-types <types>` | causal | all | Comma-separated: `caused_by,followed_by,related_to,revised_from,insight_from`. |
| `--analogical-alpha <0-1>` | analogical | 0.5 | 0.0 = pure structural similarity, 1.0 = pure embedding similarity. |

### Reflect (two-step, agent-driven)

You (the agent) are the LLM. HEBBS does the clustering and prompt construction; you read the clusters, reason about them, and commit insights. No server-side LLM needed.

**Step 1: Prepare**

```
hebbs reflect-prepare --entity-id user_prefs --format json
```

Returns JSON with:
- `session_id` - pass this to step 2
- `clusters` - groups of related memories, each with:
  - `memories` - full memory content for this cluster
  - `proposal_system_prompt` + `proposal_user_prompt` - prompts for generating insight candidates
  - `memory_ids` - source memory IDs (hex-encoded)

**Step 2: Reason and commit**

```
hebbs reflect-commit --session-id <id> --insights '[{"content":"Users consistently prefer dark themes","confidence":0.9,"source_memory_ids":["aabb...","ccdd..."],"tags":["preference","ui"]}]'
```

Each insight needs:
- `content` - the consolidated insight text
- `confidence` - 0.0 to 1.0
- `source_memory_ids` - hex-encoded IDs. **Use the `memory_ids` array from the cluster**, not `memories[].memory_id` (which is a ULID and will be rejected).
- `tags` - categorical labels

Reflection requires at least 5 memories for an entity. Sessions expire after 10 minutes.

### Insights - retrieve consolidated knowledge

```
hebbs insights --entity-id user_prefs --max-results 10 --min-confidence 0.7 --format json
```

Flags:
- `--entity-id <id>` - filter by entity.
- `--max-results <n>` - maximum insights to return.
- `--min-confidence <0.0-1.0>` - only return insights above this confidence threshold.

Check insights before recalling raw memories; they represent distilled, validated knowledge.

### Forget - remove memories

```
hebbs forget --ids <hex_id1> --ids <hex_id2>
hebbs forget --entity-id old_project
hebbs forget --staleness-us 2592000000000  # older than 30 days
hebbs forget --kind insight --decay-floor 0.1  # remove low-value decayed insights
```

Flags (at least one filter required):
- `--ids <id>` - specific memory IDs (repeatable).
- `--entity-id <id>` - scope to entity.
- `--global` - forget from the global brain.
- `--staleness-us <microseconds>` - remove memories older than this.
- `--kind <episode|insight|revision>` - filter by memory kind.
- `--decay-floor <0.0-1.0>` - remove memories with decay score below this.
- `--access-floor <n>` - remove memories with access count below this.

### Prime - warm context at session start

```
hebbs prime user_prefs --max-memories 20 --global
hebbs prime project_context --max-memories 20 --similarity-cue "current task topic"
```

Flags:
- `--max-memories <n>` - maximum memories to return.
- `--global` - prime from the global brain.
- `--similarity-cue <text>` - bias selection toward memories related to this text.
- `--recency-us <microseconds>` - only include memories within this time window.
- `--context <json>` - additional context as JSON.

Returns a blend of recent + relevant memories for an entity.

## Decision guide

1. **Start of conversation**: Prime both brains. `hebbs prime <entity> --global` for user context, then `hebbs prime` in the project for project context. Check for memory policy (`_policy` entity in global brain).
2. **Before answering any question about past context**: `hebbs recall` with the question as cue. Query both brains if the question could span personal and project knowledge.
3. **User shares a fact, preference, or decision**: `hebbs remember` immediately. Route to the correct brain (global for personal, project for project-specific).
4. **User corrects something**: `hebbs remember` the correction with importance 0.9 in the global brain. Old conflicting memories will naturally decay.
5. **User states a standing instruction** (e.g., "always do X", "never do Y"): `hebbs remember` with importance 0.9. Global brain if it applies everywhere, project brain if project-specific.
6. **After 20+ new memories on an entity**: `hebbs reflect-prepare` + `reflect-commit` to consolidate into insights.
7. **Periodic maintenance**: `hebbs insights` to review, `hebbs forget` to clean stale data.

## Output format

Always use `--format json` when parsing output programmatically. Human format is for display only.

## Brain discovery

When you omit `--global`, HEBBS finds the brain automatically:

1. `--vault <path>` flag or `HEBBS_VAULT` env var (explicit)
2. Walk up from current directory looking for `.hebbs/`
3. Fall back to `~/.hebbs/` (global brain)
4. `--endpoint` or `HEBBS_ENDPOINT` enables remote mode (gRPC client to server)

When you use `--global`, HEBBS goes straight to `~/.hebbs/`.
