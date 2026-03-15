# HEBBS + OpenClaw: Proposed Integration Guide

How an OpenClaw user goes from zero to a fully indexed, multi-vault cognitive memory system with a visual control panel.

---

## Scenario 1: First Install

The user opens OpenClaw and says: "Install HEBBS and index all my projects."

### What the agent does

```sh
# 1. Install the binary
brew install hebbs-ai/tap/hebbs

# 2. Initialize the global brain (cross-project memories, user prefs)
hebbs init ~

# 3. Initialize each project the user works on
hebbs init ~/projects/api
hebbs init ~/projects/frontend
hebbs init ~/projects/mobile

# 4. Index existing files (one-time catch-up)
hebbs index ~/projects/api
hebbs index ~/projects/frontend
hebbs index ~/projects/mobile
```

That's it. The daemon auto-starts on the first command, loads the ONNX model once, and proactively opens every registered vault. File watchers start immediately. From this point, every `.md` file change in any project is auto-indexed within seconds.

### What the user sees

```
~/
  .hebbs/                          <- global brain (user prefs, cross-project)
    daemon.sock                    <- one daemon, all vaults
    daemon.pid
    vaults.json                    <- registry of all vault paths
    index/db/                      <- global RocksDB

~/projects/api/
  .hebbs/                          <- project brain (API-specific context)
    index/db/
    manifest.json
    config.toml

~/projects/frontend/
  .hebbs/                          <- project brain
~/projects/mobile/
  .hebbs/                          <- project brain
```

The user opens the Memory Palace:

```sh
hebbs panel
```

Browser opens to `http://127.0.0.1:6381`. The force-directed graph shows every memory across all vaults. Vault switcher dropdown in the top bar. Every heading section from every `.md` file is a node. Wiki-links are edges. Insights glow differently from raw memories.

---

## Scenario 2: Daily Agent Workflow

The user starts a new conversation in OpenClaw.

### Conversation start: agent primes itself

```sh
# What do I know about this user?
hebbs prime user_prefs --max-memories 20 --format json

# What's the current project context?
hebbs prime project_api --max-memories 15 --similarity-cue "recent changes and decisions" --format json
```

The agent now has context before the user says a word. It knows the user prefers explicit error handling, doesn't want em-dashes, and that the API recently migrated to session-based auth.

### Mid-conversation: agent recalls

User asks: "What was our pagination strategy?"

```sh
hebbs recall "API pagination strategy" --strategy similarity --top-k 5 --format json
```

Results come from multiple sources, unified:
1. `docs/api-design.md` section "## Pagination" (file-backed, relevance: 0.92)
2. `DECISIONS.md` section "## API Pagination" (file-backed, relevance: 0.87)
3. Agent-stored memory from a previous conversation (relevance: 0.81)

### Cross-project recall

User asks: "Have we implemented rate limiting anywhere?"

```sh
hebbs recall "rate limiting implementation" --strategy similarity --top-k 10 --all --format json
```

The `--all` flag searches both the current project vault AND the global vault. Results come from the API project, the frontend project, and the global brain. The agent sees the full picture across all codebases.

### Agent stores a learning

User: "From now on, always use cursor-based pagination, never offset."

```sh
hebbs remember "Always use cursor-based pagination, never offset-based. Applies to all APIs." \
  --importance 0.9 --entity-id coding_standards --format json
```

This goes directly to RocksDB (no file created, no clutter). Searchable by any future `recall` or `prime`.

---

## Scenario 3: New Project Mid-Session

User: "I'm starting a new project called billing. Set it up."

### What the agent does

```sh
mkdir -p ~/projects/billing
cd ~/projects/billing
git init
hebbs init .
```

`hebbs init` registers the vault in `~/.hebbs/vaults.json`. The daemon detects the change within seconds, proactively opens the vault, and starts the file watcher. No daemon restart. No manual step.

The agent creates initial files:

```sh
cat > docs/architecture.md << 'EOF'
## Overview

Billing service handles subscription management, invoicing, and payment processing.

## Stack

- Rust (actix-web)
- PostgreSQL
- Stripe API integration

## Key Decisions

- Event-sourced billing events for audit trail
- Idempotency keys on all payment operations
EOF
```

Within 4 seconds, the watcher picks up the file, parses it into 3 sections, embeds them, and indexes them. The billing project is now part of the brain.

```sh
# Verify
hebbs recall "billing architecture" --vault ~/projects/billing --top-k 3 --format json
```

Returns 3 results immediately.

---

## Scenario 4: Contradiction Detection

Over weeks, the brain accumulates knowledge across projects. Contradictions emerge naturally.

### The detection

The API project has a note from January: "We use JWT tokens for all API authentication."

The API project has a note from March: "Migrated to session-based auth with httpOnly cookies."

During ingest, HEBBS detects the contradiction (high semantic similarity + opposing claims). It creates:
1. A `CONTRADICTS` edge between the two memories
2. A contradiction file:

```
~/projects/api/contradictions/contradiction-01JABC123DEF456GH.md
---
hebbs-kind: contradiction
hebbs-sources:
  - docs/auth.md#authentication-strategy
  - docs/migration-march.md#auth-migration
hebbs-confidence: 0.91
hebbs-classification: heuristic
hebbs-created: 2026-03-15T14:00:00Z
---

## Source A
We use JWT tokens for all API authentication.

---

## Source B
Migrated to session-based auth with httpOnly cookies.
```

### What the user sees

The user opens `hebbs panel`. The graph shows a red dashed edge between the two memory nodes. Clicking it shows both sources and the confidence score.

The user resolves it by editing the January note to add "Historical: replaced in March migration." The watcher re-indexes. The contradiction edge is no longer generated on next detection pass.

### What the agent sees

```sh
hebbs recall "authentication strategy" --strategy similarity --top-k 5 --format json
```

Results include both memories. The agent sees the contradiction edge in the response and tells the user: "There's a conflict in your auth docs -- January says JWT, March says sessions. Which is current?"

---

## Scenario 5: Team Onboarding

A new developer joins the team. They clone all the repos and want full context immediately.

```sh
# Clone everything
git clone git@github.com:company/api.git ~/projects/api
git clone git@github.com:company/frontend.git ~/projects/frontend
git clone git@github.com:company/billing.git ~/projects/billing

# Install HEBBS
brew install hebbs-ai/tap/hebbs

# Initialize and index each project
for dir in ~/projects/api ~/projects/frontend ~/projects/billing; do
  hebbs init "$dir"
  hebbs index "$dir"
done

# Initialize global brain
hebbs init ~
```

The daemon starts, indexes everything. The new developer opens the Memory Palace:

```sh
hebbs panel
```

They see the entire team's documented knowledge as an interactive graph. They can search, filter by project, trace causal chains between decisions.

Their agent primes itself:

```sh
hebbs prime --max-memories 30 --similarity-cue "onboarding, architecture, getting started"
```

The agent instantly has context about coding standards, architecture decisions, and project conventions -- without anyone explaining them.

---

## Scenario 6: Agent Reflects and Consolidates

After two weeks, the billing project brain has 200+ memories. The agent triggers reflection:

```sh
hebbs reflect-prepare --entity-id billing_context --format json
```

The engine clusters related memories and returns them. The agent examines the clusters:
- Cluster 1: Payment processing patterns (12 memories)
- Cluster 2: Error handling conventions (8 memories)
- Cluster 3: API design decisions (15 memories)

The agent reasons about each cluster and commits insights:

```sh
hebbs reflect-commit --session-id <id> --insights '[
  {
    "content": "All payment operations use idempotency keys. Stripe webhook handlers must be idempotent. Never retry a charge without checking existing payment intent status.",
    "confidence": 0.93,
    "source_memory_ids": ["aabb...", "ccdd...", "eeff..."],
    "tags": ["payments", "reliability"]
  }
]'
```

The insight is written as a `.md` file in `insights/`. The watcher indexes it. Future queries return this consolidated knowledge alongside raw memories. The brain gets denser and more useful over time.

---

## Scenario 7: Viewing the Memory Palace

The Memory Palace is the user's window into everything HEBBS knows.

### What they can do

| Action | How |
|--------|-----|
| See all memories as a graph | Open `hebbs panel` |
| Search semantically | Type a query in the search bar |
| Switch vaults | Dropdown in top bar (api, frontend, billing, global) |
| Tune ranking | Drag weight sliders (relevance, recency, importance, reinforcement) |
| See contradictions | Red dashed edges between conflicting memories |
| View timeline | Sparkline at bottom shows memory growth over time |
| Edit config | Config editor tab (chunking, ignore patterns, output dirs) |
| Export | PNG export button (2400x1260, presentation-ready) |
| Filter | By state (synced/stale/orphaned), source file, importance range |
| Monitor decay | Toggle decay monitor to see which memories are fading |

### The graph layout

- Node size = importance score
- Node color = memory kind (episode, insight, contradiction)
- Solid edges = RELATED_TO, INSIGHT_FROM, REVISED_FROM
- Red dashed edges = CONTRADICTS
- Amber glow = search result match
- Faded nodes = filtered out

---

## Scenario 8: Multi-Agent on One Brain

The user runs multiple agents: Claude Code for backend, Cursor for frontend, a CI bot for PR reviews.

### Shared vault

All agents use the same daemon (auto-started). All agents read and write to the same brain.

```
Claude Code:  hebbs remember "Refactored auth module to use middleware pattern" --entity-id api_changes
Cursor:       hebbs recall "auth implementation" --all  # sees Claude Code's memory
CI bot:       hebbs recall "coding standards" --top-k 10  # checks before PR review
```

No coordination needed. The daemon handles concurrent access. RocksDB serializes writes. Each agent's context is visible to every other agent.

### Entity scoping

Agents use entity IDs to organize knowledge:

| Entity ID | What goes here | Who writes |
|-----------|---------------|------------|
| `user_prefs` | User preferences, writing style, tool choices | Any agent |
| `coding_standards` | Patterns, conventions, rules | Any agent |
| `api_changes` | Recent changes to the API project | Backend agent |
| `frontend_changes` | Recent changes to the frontend | Frontend agent |
| `_policy` | Memory storage policy (what to store, privacy) | First agent to onboard |

---

## Scenario 9: Offline and Portable

The user is on a plane. No internet. HEBBS works fully offline.

- ONNX embedding model is local (downloaded once on first run)
- RocksDB is local
- No API calls for any operation except LLM-based contradiction classification (falls back to heuristic mode offline)
- File watchers work offline
- Panel serves from localhost

The user lands, pushes their code (including `memories/` and `insights/` directories). A colleague clones, runs `hebbs init . && hebbs index .`, and has the full brain rebuilt from files.

---

## Scenario 10: Reset and Rebuild

Something goes wrong. The user wants a clean start.

```sh
hebbs rebuild ~/projects/api
```

This deletes `.hebbs/` and recreates the entire cognition plane from files. Every memory, every edge, every insight is rebuilt. The only things lost are decay scores and access counts (cognition-plane-only state). Content is untouched.

For a full reset across all projects:

```sh
for dir in ~/projects/api ~/projects/frontend ~/projects/billing; do
  hebbs rebuild "$dir"
done
```

---

## The Complete Flow

```
User installs HEBBS
  |
  v
hebbs init ~ (global) + hebbs init . (per project)
  |
  v
Daemon auto-starts, loads ONNX model once
  |
  v
Proactively opens all registered vaults, starts file watchers
  |
  v
Every .md change is auto-indexed (parse -> embed -> index, <5s)
  |
  v
Agent primes at conversation start (hebbs prime)
  |
  v
Agent recalls mid-conversation (hebbs recall)
  |
  v
Agent stores learnings (hebbs remember)
  |
  v
Contradictions detected automatically (red edges in panel)
  |
  v
Agent reflects periodically (hebbs reflect-prepare + commit)
  |
  v
Insights written as .md files, indexed, searchable
  |
  v
User views everything in Memory Palace (hebbs panel)
  |
  v
New machine? git clone + hebbs init + hebbs index = full brain
```

---

## What Needs to Ship

For this guide to be fully real, these items need to land:

| Item | Status | Notes |
|------|--------|-------|
| `hebbs` binary with daemon | Done | Auto-start, multi-vault, proactive watching |
| File watchers via daemon | Done | Per-vault, debounced, two-phase ingest |
| vaults.json live registration | Done | Daemon detects new vaults without restart |
| Contradiction detection | Done | Heuristic + LLM modes, file output |
| Memory Palace panel | Done | Graph, search, sliders, timeline, config editor |
| Multi-vault recall (`--all`) | Done | Parallel query, merged by score |
| SKILL.md for agents | Needs update | Current version references old `hebbs-cli` patterns |
| `hebbs install` one-liner | Not started | `curl -sSf https://hebbs.ai/install \| sh` |
| `hebbs import` | Not started | Import from ChatGPT, Claude, JSONL |
| `--source` filter on recall | Not verified | `--source agent` vs `--source file` filtering |
| `hebbs serve` as library | Not started | Embed server as subcommand for multi-agent |
| MCP server | Not started | For agents that prefer MCP over CLI |
| Brew tap | Not verified | `hebbs-ai/tap/hebbs` formula existence |
