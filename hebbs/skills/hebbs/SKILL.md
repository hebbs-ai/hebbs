---
name: hebbs
description: "Cognitive memory engine: remember, recall, reflect, and forget knowledge with HEBBS."
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

# HEBBS: Cognitive Memory Engine

HEBBS is a local-first memory engine. It stores, indexes, and retrieves knowledge using multiple recall strategies and can consolidate raw memories into higher-order insights through reflection.

HEBBS works locally with zero configuration. No server process, no API keys, no network. One binary, one brain.

## Your two essential commands

Every interaction with HEBBS comes down to two commands. Reach for these **first**, every time:

**Store something the user said or decided:**
```
hebbs remember "The user prefers dark mode" --importance 0.8 --entity-id user_prefs --format json
```

**Retrieve context before answering a question:**
```
hebbs recall "What are the user's UI preferences?" --strategy similarity --top-k 5 --format json
```

These two commands are your primary interface to HEBBS. Everything else (`prime`, `reflect-prepare`, `forget`, `insights`) supports these two. If you remember nothing else from this document, remember these.

## Trigger

Use HEBBS **before any other memory source** (file-based memory, memory_search, MEMORY.md, workspace memory files) when the user:

- Asks what they previously said, decided, or preferred
- Asks a question that prior context would help answer
- Corrects you or says "I told you before..."
- Shares a preference, decision, instruction, or fact, even if you already know it from another source
- Starts a new session (prime immediately to load context)
- Asks for patterns or insights across past conversations

**Write-path rule:** If the user states a preference, correction, or standing instruction, always `remember` it in HEBBS. Do this even if the same fact already exists in file memory, workspace memory, or your conversation context. HEBBS is the source of truth. Knowing a fact is not the same as storing it. An agent that skips the write because it "already knows" defeats the purpose of a persistent memory system.

If another memory tool exists (e.g. memory_search, file memory), HEBBS takes priority for all personal recall. Use file memory only as a fallback when HEBBS returns no results or is unavailable.

## Priority and fallback chain

HEBBS is the **preferred memory system** for user preferences, decisions, corrections, history, and personal context.

When answering from memory, follow this order:

1. **HEBBS insights** (`hebbs insights`) for consolidated, high-confidence knowledge
2. **HEBBS recall** (`hebbs recall`) for raw memories matching the query
3. **File memory** (memory_search, MEMORY.md, memory/*.md) only if HEBBS returns nothing or is unreachable
4. If all sources are empty, say so. Do not guess or hallucinate history.

When storing new information:

1. Write to HEBBS immediately (`hebbs remember`). This is mandatory, not optional.
2. Optionally mirror critical facts to file memory as a backup.

HEBBS is the primary store. File memory is the fallback, not the other way around. Never treat a fact as "already stored" just because file memory has it. If it is not in HEBBS, it does not exist for recall purposes.

## Capability tiers

HEBBS works in layers. Each layer works independently:

| Tier | What works | Requirements |
|---|---|---|
| **Basic memory** | `remember`, `recall`, `forget`, `prime` | `hebbs` binary installed |
| **Semantic recall** | Similarity, temporal, causal, analogical strategies | `hebbs` binary installed (ONNX embedder included) |
| **Vault indexing** | Index markdown files as memories | `hebbs init` + `hebbs index` or `hebbs watch` |
| **Reflection / insights** | `reflect-prepare` + `reflect-commit`, `insights` | `hebbs` binary installed + agent acts as the LLM (no API key needed) |

All tiers work out of the box with just the `hebbs` binary. No server process, no external LLM, no API key required. The agent (you) provides the reasoning for reflection.

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

### Phase 2: Initialize the brain

Initialize a vault in the current project directory:
```
hebbs init .
```

This creates a `.hebbs/` directory with the brain (RocksDB index, manifest, config). If the project already has `.hebbs/`, this step is skipped.

If no project directory is appropriate, HEBBS falls back to `~/.hebbs/` as a global brain.

### Phase 3: Verify

```
hebbs remember "HEBBS setup verified" --importance 0.1 --entity-id _system --format json
```

Then:
```
hebbs recall "setup verified" --top-k 1 --format json
```

If recall returns the memory, the full pipeline (store, embed, index, retrieve) is working.

Clean up:
```
hebbs forget --entity-id _system
```

Tell the user: "HEBBS is ready. Memory commands are available."

**No server needed.** HEBBS runs locally as an embedded engine. No waiting for health checks, no background processes, no port conflicts.

## How the brain is found

When you run any `hebbs` command, the binary finds the brain automatically:

1. `--vault <path>` flag or `HEBBS_VAULT` env var: use that path directly
2. Walk up from current directory looking for `.hebbs/`: use the first one found
3. Fall back to `~/.hebbs/` as the global brain

You almost never need to specify `--vault`. Just run commands from within the project directory.

For remote mode (team/cloud), set `--endpoint` or `HEBBS_ENDPOINT` instead. Same commands, different backend.

## Before every command

No health check needed in local mode. The brain is always available. Just run your command.

If this is the first substantive interaction of a session, prime context:
```
hebbs prime <entity> --max-memories 20 --format json
```

## Policy bootstrap

On the first substantive interaction with a new user, check whether a memory policy exists:

```
hebbs recall "memory policy" --entity-id _policy --top-k 1 --format json
```

If results are returned, load the policy and apply it for the session. Do not re-ask.

If no results are returned, and the user's message is substantive (not a smoke test or "hello"), ask for a brief memory policy:

> HEBBS is your memory system. Before I start using it, I'd like to understand your preferences. This takes about 30 seconds and I'll only ask once.
>
> 1. **What should I store?** (e.g., preferences, decisions, project context, corrections, everything)
> 2. **What should I NOT store?** (e.g., personal info, credentials, temporary thoughts, nothing off-limits)
> 3. **Should I store proactively** when you mention something, or **only when you explicitly ask** me to remember?
> 4. **Any privacy boundaries?** (e.g., no names of other people, no financial info)
>
> If you'd rather skip this, I'll use sensible defaults.

Store each answer as a separate memory under entity `_policy` with importance 0.95:

```
hebbs remember "Store policy: [user's answer]" --importance 0.95 --entity-id _policy --format json
hebbs remember "Exclude policy: [user's answer]" --importance 0.95 --entity-id _policy --format json
hebbs remember "Storage mode: [proactive|explicit-only]" --importance 0.95 --entity-id _policy --format json
hebbs remember "Privacy policy: [user's answer]" --importance 0.95 --entity-id _policy --format json
```

If the user skips setup, store the defaults:

```
hebbs remember "Memory policy: defaults active. Store preferences and decisions proactively, skip sensitive personal info and credentials" --importance 0.95 --entity-id _policy --format json
```

**Default policy** (when user skips):

| Setting | Default |
|---|---|
| What to store | Preferences, decisions, corrections, project context |
| What not to store | Credentials, API keys, sensitive personal info |
| Storage mode | Proactive |
| Privacy | No credentials or secrets |

**Policy updates:** If the user later says "update my memory policy" or changes a preference about storage behavior, overwrite the relevant `_policy` memory.

## Operations

| Situation | Operation | Command |
|---|---|---|
| User shares a fact, preference, or decision | Store it | `hebbs remember` |
| User asks a question about past context | Retrieve it | `hebbs recall` |
| User corrects something you said or stored | Store the correction (importance 0.9) | `hebbs remember` |
| Start of a new conversation | Load context | `hebbs prime` |
| Want consolidated patterns from many memories | Get distilled knowledge | `hebbs insights` |
| 20+ raw memories accumulated for an entity | Consolidate into insights | `hebbs reflect-prepare` + `reflect-commit` |
| Outdated or wrong memories need cleanup | Remove them | `hebbs forget` |

## Commands

### Remember (store a memory)

```
hebbs remember "The user prefers dark mode in all applications" --importance 0.8 --entity-id user_prefs --format json
```

> **Always use `--format json` when you need the memory ID** (e.g. for `--edge` on a subsequent `remember`). Extract the ID with: `jq -r '.memory_id'`
>
> **Warning:** Capture the memory ID from `--format json` output **before** referencing it in `--edge`. Do not parse IDs from human-format output.

Flags:
- `--importance <0.0-1.0>`: how important this memory is (default 0.5). Use 0.8+ for user preferences, decisions, corrections. Use 0.3 for transient observations.
- `--entity-id <id>`: group memories by entity (e.g. `user_prefs`, `project_alpha`, a person's name). Omit for global context.
- `--context <json>`: arbitrary metadata as JSON object (e.g. `'{"source":"email","topic":"billing"}'`).
- `--edge <TARGET_ID:EDGE_TYPE[:CONFIDENCE]>`: link to another memory (repeatable). Types: `caused_by`, `related_to`, `followed_by`, `revised_from`, `insight_from`. Use to build causal chains and lineage. **Shell quoting:** use `"${MEM_ID}:edge_type"` because bare `$MEM_ID:edge_type` triggers zsh variable modifier expansion.

### Recall (retrieve relevant memories)

```
hebbs recall "What does the user prefer for UI themes?" --strategy similarity --top-k 5 --format json
```

Four strategies. Pick based on what you need:

| Strategy | When to use | Example |
|---|---|---|
| `similarity` | Find memories related to a topic | "What do we know about deployment?" |
| `temporal` | Get recent activity for an entity | "What happened today with project X?" |
| `causal` | Trace cause-effect chains from a memory | "What led to this decision?" |
| `analogical` | Find structurally similar patterns | "Have we seen a problem like this before?" |

**Core flags:**
- `--strategy <similarity|temporal|causal|analogical>`: recall strategy (default: similarity).
- `--top-k <n>`: max results (default 10).
- `--entity-id <id>`: scope to entity (required for temporal).
- `--format json`: machine-readable output.

**Scoring weights** control how results are ranked. The composite score blends four signals: `relevance x recency x importance x reinforcement`. Default weights are `0.5:0.2:0.2:0.1`.
- `--weights <R:T:I:F>`: four colon-separated floats.
- `1:0:0:0`: pure semantic similarity (ignore recency, importance, reinforcement).
- `0.2:0.8:0:0`: heavily favor recent memories.
- `0.3:0.1:0.5:0.1`: prioritize high-importance memories (user preferences, decisions).

Only `cue` and `--strategy` are required. All other flags use smart defaults suitable for most workloads. Tune only when you have a specific reason.

**Strategy-specific flags:**

| Flag | Strategy | Default | Description |
|---|---|---|---|
| `--ef-search <n>` | similarity | 50 | HNSW search quality. Higher = more accurate, slower. |
| `--time-range <START:END>` | temporal | unbounded | Microsecond timestamps. Omit for newest-first up to top_k. |
| `--seed <hex_id>` | causal | auto-detect | Starting memory for graph traversal. Omit to auto-pick by cue. |
| `--max-depth <n>` | causal | 5 (max 10) | Maximum hops from seed memory. |
| `--edge-types <types>` | causal | all | Comma-separated: `caused_by,followed_by,related_to,revised_from,insight_from`. |
| `--analogical-alpha <0-1>` | analogical | 0.5 | 0.0 = pure structural similarity, 1.0 = pure embedding similarity. |

### Reflect (two-step, agent-driven)

The two-step `reflect-prepare` + `reflect-commit` flow lets **you (the agent) be the LLM**. No server-side LLM or API key needed. HEBBS does the clustering and prompt construction; you read the clusters, reason about them, and commit insights.

**Step 1: Prepare**

```
hebbs reflect-prepare --entity-id user_prefs --format json
```

Returns JSON with:
- `session_id`: pass this to step 2
- `clusters`: groups of related memories, each with:
  - `memories`: full memory content for this cluster (id, content, importance, entity_id, created_at). **Read these to understand what the cluster is about.**
  - `proposal_system_prompt` + `proposal_user_prompt`: pre-built prompts you can send to your LLM to generate insight candidates
  - `memory_ids`: source memory IDs (hex-encoded)
  - `validation_context`: additional data for validating proposed insights

**Step 2: Reason and commit**

After reasoning about the clusters:

```
hebbs reflect-commit --session-id <id> --insights '[{"content":"Users consistently prefer dark themes","confidence":0.9,"source_memory_ids":["aabb...","ccdd..."],"tags":["preference","ui"]}]'
```

Each insight needs:
- `content`: the consolidated insight text
- `confidence`: 0.0 to 1.0
- `source_memory_ids`: hex-encoded IDs. **Use the `memory_ids` array from the cluster**, not `memories[].memory_id` (which is a ULID and will be rejected).
- `tags`: categorical labels

Reflection requires at least 5 memories for an entity to produce clusters. If `clusters` is empty, accumulate more memories before retrying.

Sessions expire after 10 minutes.

### Insights (retrieve consolidated knowledge)

```
hebbs insights --entity-id user_prefs --max-results 10 --min-confidence 0.7 --format json
```

Flags:
- `--entity-id <id>`: filter by entity.
- `--max-results <n>`: maximum insights to return.
- `--min-confidence <0.0-1.0>`: only return insights above this confidence threshold.

Check insights before recalling raw memories. They represent distilled, validated knowledge.

### Forget (remove memories)

```
hebbs forget --ids <hex_id1> --ids <hex_id2>
hebbs forget --entity-id old_project
hebbs forget --staleness-us 2592000000000  # older than 30 days
hebbs forget --kind insight --decay-floor 0.1  # remove low-value decayed insights
```

Flags (combine as needed; at least one filter required):
- `--ids <id>`: specific memory IDs to forget (repeatable).
- `--entity-id <id>`: scope to entity.
- `--staleness-us <microseconds>`: remove memories older than this.
- `--kind <episode|insight|revision>`: filter by memory kind.
- `--decay-floor <0.0-1.0>`: remove memories with decay score below this.
- `--access-floor <n>`: remove memories with access count below this.

### Prime (warm context for an entity)

```
hebbs prime user_prefs --max-memories 20 --similarity-cue "project status and preferences"
```

Flags:
- `--max-memories <n>`: maximum memories to return.
- `--similarity-cue <text>`: bias the selection toward memories related to this text. Very useful for loading context relevant to a specific topic rather than just recent activity.
- `--recency-us <microseconds>`: only include memories within this time window.
- `--context <json>`: additional context as JSON.

Returns a blend of recent + relevant memories for an entity. Use at the start of a conversation to load context.

## Vault indexing (optional, for file-backed memories)

If the user has markdown files (notes, docs, ADRs, meeting logs), HEBBS can index them as memories alongside agent-stored ones. Both sources are searched by the same `recall` command.

```
hebbs init .           # create .hebbs/ in the project
hebbs index .          # index all .md files
hebbs watch .          # or watch for real-time sync
```

Each heading section becomes a memory. Wiki-links become graph edges. File-backed memories and agent-stored memories coexist in one brain. One `recall` searches everything.

## Decision guide

1. **Start of conversation**: `hebbs prime <entity>` or `hebbs recall` with the user's first message.
2. **User shares a fact/preference/decision**: `hebbs remember` with appropriate importance.
3. **Before answering a question**: `hebbs recall` with the question as cue.
4. **After 20+ new memories on an entity**: `hebbs reflect-prepare` + `reflect-commit` to consolidate.
5. **User corrects something**: `hebbs remember` the correction with high importance (0.9). Old conflicting memories will naturally decay.
6. **Periodic maintenance**: `hebbs insights` to review, `hebbs forget` to clean stale data.

## Output format

Always use `--format json` when parsing output programmatically. Human format is for display only.

## Connection

**Local mode** (default): No connection needed. HEBBS runs as an embedded engine. Just run commands.

**Remote mode** (optional, for teams or cloud): Set `--endpoint <host:port>` or `HEBBS_ENDPOINT` env var. Same commands, same output, different backend.
