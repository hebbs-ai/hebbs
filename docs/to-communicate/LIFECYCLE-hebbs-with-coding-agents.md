# HEBBS Lifecycle: Coding Agents with Persistent Memory

How HEBBS works end-to-end when a developer uses a coding agent (Claude Code, OpenClaw, Cursor, Windsurf, or any tool-calling LLM) with a file vault and persistent memory.

---

## The Setup

A developer installs HEBBS and initializes a vault in their project directory:

```sh
brew install hebbs-ai/tap/hebbs
cd ~/projects/my-app
hebbs init .
```

This creates a `.hebbs/` directory (the cognition plane) alongside their existing files (the content plane). The developer adds `.hebbs/` to `.gitignore` because it is fully rebuildable from files.

They start the watcher:

```sh
hebbs watch .
```

The watcher indexes every `.md` file in the project. Each heading section becomes a memory. Wiki-links become graph edges. Tags become memory kinds. The developer's existing notes, ADRs, meeting logs, and design docs are now searchable by semantic meaning, not just keywords.

To exclude files from indexing (templates, drafts, generated docs), the developer creates a `.hebbsignore` file at the vault root -- same syntax as `.gitignore`:

```
# .hebbsignore
templates/
drafts/*.md
api-reference/
```

See [hebbsignore.md](../hebbsignore.md) for the full reference.

---

## Day 1: The Agent Arrives

The developer starts a conversation with their coding agent (Claude Code, OpenClaw, etc.). The agent has the HEBBS skill installed. Before doing anything, the agent primes itself:

```sh
hebbs prime --entity-id user_prefs --max-memories 20
```

On day one this returns nothing. The agent has no memories of this user yet.

### The agent learns preferences

The developer asks the agent to refactor a module. During the conversation:

- Developer: "Don't use em-dashes in comments."
- Developer: "I prefer explicit error handling over unwrap()."
- Developer: "Always run clippy before committing."

The agent stores each of these:

```sh
hebbs remember "Developer does not want em-dashes in comments or documentation" \
  --importance 0.9 --entity-id writing_prefs

hebbs remember "Developer prefers explicit error handling (match/if-let) over unwrap()" \
  --importance 0.9 --entity-id coding_prefs

hebbs remember "Always run clippy before committing code" \
  --importance 0.8 --entity-id workflow_prefs
```

Each `remember` call writes a markdown file to the vault:

```
my-app/memories/2026-03-14T10-30-00-no-em-dashes.md
---
hebbs-kind: memory
hebbs-importance: 0.9
hebbs-entity-id: writing_prefs
hebbs-created: 2026-03-14T10:30:00Z
hebbs-source: agent
---

Developer does not want em-dashes in comments or documentation.
```

The watcher picks up the file, embeds it, and indexes it. The memory is now in the brain, backed by a file, visible in git, portable across machines.

### The agent also indexes project context

The developer's project already has files the agent benefits from:

```
my-app/
  docs/
    architecture.md          <- 8 sections, 8 memories
    api-design.md            <- 5 sections, 5 memories
  DECISIONS.md               <- 3 sections, 3 memories
  memories/                  <- agent-written files
    2026-03-14T10-30-00-no-em-dashes.md
    2026-03-14T10-31-00-explicit-errors.md
    2026-03-14T10-32-00-clippy-before-commit.md
  .hebbs/                    <- cognition plane (gitignored)
    index/db/                <- RocksDB
    manifest.json            <- file-to-memory mapping
    config.toml              <- vault config
```

All of these live in one index. One brain. One `recall` searches everything.

---

## Day 5: The Agent Remembers

A new conversation starts. Different session, maybe a different day. The agent primes itself:

```sh
hebbs prime --entity-id coding_prefs --max-memories 10
```

It gets back: "Developer prefers explicit error handling over unwrap()" and "Always run clippy before committing." The agent now writes code with `match` instead of `unwrap()` and runs clippy before suggesting commits. The developer never repeats themselves.

### Recall across sources

The developer asks: "What did we decide about the API pagination strategy?"

The agent queries:

```sh
hebbs recall "API pagination strategy" --strategy similarity --top-k 5
```

Results come back from two sources, unified in one ranked list:

1. `docs/api-design.md` section "## Pagination" (source: file, relevance: 0.92)
2. `DECISIONS.md` section "## API Pagination" (source: file, relevance: 0.87)
3. `memories/2026-03-16-cursor-pagination.md` (source: agent, relevance: 0.81)

The agent sees the full picture: the original design doc, the decision record, and a preference the agent stored from a previous conversation. All from one query. The developer does not know or care which memories came from files they wrote and which came from agent conversations.

### Filtering when needed

Sometimes the agent wants only its own stored memories (not project docs):

```sh
hebbs recall "error handling preferences" --source agent --top-k 5
```

Or only project documentation:

```sh
hebbs recall "authentication flow" --source file --top-k 10
```

But the default is always combined. The filter is for edge cases, not the normal path.

---

## Day 14: The Agent Reflects

After two weeks, the brain has 200+ memories: project docs, agent-stored preferences, meeting notes, decision records. The agent triggers reflection:

```sh
hebbs reflect-prepare --entity-id coding_prefs
```

The engine clusters related memories and returns prepared data. The agent examines the clusters, reasons about patterns, and commits insights:

```sh
hebbs reflect-commit --session-id <id> \
  --insights '[{"title":"Error Handling Standard","body":"Developer consistently prefers explicit match/if-let over unwrap() and expect(). All error paths should return typed errors, never panic.","confidence":0.95,"source_memory_ids":["01JAB...","01JAC..."]}]'
```

The insight is written as a markdown file:

```
my-app/memories/insights/2026-03-28-error-handling-standard.md
---
hebbs-kind: insight
hebbs-confidence: 0.95
hebbs-entity-id: coding_prefs
hebbs-sources: 01JAB..., 01JAC...
---

Developer consistently prefers explicit match/if-let over unwrap() and expect().
All error paths should return typed errors, never panic.
```

The watcher indexes it. Future `recall` queries now return this consolidated insight alongside raw memories. The agent's understanding sharpens over time.

---

## The Daily Loop

Here is what happens every day without the developer thinking about it:

```
Developer writes code, docs, notes
       |
       v
Watcher detects file changes
       |
       v
Parser splits into heading sections
       |
       v
Embedder generates vectors (local ONNX, no API call)
       |
       v
Engine indexes: HNSW + temporal + graph
       |
       v
Agent primes at conversation start
       |
       v
Agent recalls mid-conversation when it needs context
       |
       v
Agent stores new learnings via remember (writes .md files)
       |
       v
Watcher indexes those files too
       |
       v
Periodically: agent reflects, consolidates, writes insight files
       |
       v
The brain gets smarter. The developer never repeats themselves.
```

---

## Multi-Agent Scenario

The developer uses multiple agents: Claude Code for backend work, a separate agent for frontend, and a CI agent that runs on every PR.

### Option A: Shared local vault

All agents read and write to the same `.hebbs/` vault. The watcher is running. Each agent's `remember` calls write files. Each agent's `recall` calls search the full brain. The backend agent's knowledge is visible to the frontend agent. The CI agent can check "what are the coding standards?" before reviewing a PR.

### Option B: Shared via server

For teams or remote agents without filesystem access:

```sh
hebbs serve --grpc-port 6380 --http-port 6381
```

Agents connect via gRPC or REST. The config changes one line:

```toml
[engine]
mode = "remote"
endpoint = "localhost:6380"
```

The same `hebbs recall`, `hebbs remember`, `hebbs prime` commands work identically. The agent skill does not change. Only the transport changes.

### Option C: Cloud

For a team sharing one brain across machines:

```toml
[engine]
mode = "remote"
endpoint = "https://api.hebbs.ai"
api_key = "hb_..."
```

Same commands. Same agent skill. Different backend.

---

## New Machine, Full Brain

The developer gets a new laptop. Their project files sync via git (or Dropbox, iCloud, whatever). The memories directory syncs too because it is just markdown files.

```sh
git clone git@github.com:user/my-app.git
cd my-app
hebbs init .
hebbs index .
```

The engine rebuilds the entire cognition plane from files. Every memory, every preference, every insight is back. The agent primes itself and picks up exactly where it left off.

Delete `.hebbs/` at any time. Run `hebbs index .` and it comes back. The files are the database. The engine is disposable.

---

## What the Agent Sees

From the agent's perspective, HEBBS is five operations:

| Need | Command | When |
|------|---------|------|
| Load context | `hebbs prime` | Start of every conversation |
| Find something | `hebbs recall "query"` | Mid-conversation, when the agent needs information |
| Store something | `hebbs remember "text"` | When the developer says something worth keeping |
| Get insights | `hebbs insights` | When the agent needs consolidated patterns |
| Forget something | `hebbs forget --ids <id>` | When the developer corrects or retracts |

The agent does not think about files vs API memories. It does not think about local vs remote. It does not think about embedding models or RocksDB. It calls five commands and gets smarter over time.

---

## What the Developer Sees

The developer sees markdown files they can read, edit, delete, and version control. They see a `.hebbs/` directory they can ignore (or delete and rebuild). They see an agent that remembers their preferences, understands their project context, and never asks the same question twice.

```
my-app/
  src/                       <- their code
  docs/                      <- their docs (indexed as memories)
  memories/                  <- agent-written memories (also just files)
    insights/                <- consolidated knowledge (also just files)
  .hebbs/                    <- the brain (rebuildable, gitignored)
```

If they want to see what the agent knows:

```sh
hebbs list                              # everything
hebbs list --source agent               # only agent-stored memories
hebbs list --source file                # only file-backed memories
hebbs recall "what are my preferences"  # semantic search across all sources
hebbs status                            # brain health
```

If they want to correct something, they edit or delete the markdown file. The watcher updates the brain. No API calls, no dashboards, no admin panels required.

---

## Lifecycle Summary

```
Install         brew install hebbs-ai/tap/hebbs
Initialize      hebbs init .
Index           hebbs index .        (or hebbs watch . for real-time)
Agent primes    hebbs prime          (at conversation start)
Agent recalls   hebbs recall "..."   (mid-conversation)
Agent stores    hebbs remember "..." (writes .md file, watcher indexes)
Agent reflects  hebbs reflect-prepare + reflect-commit (periodic)
New machine     git clone + hebbs init + hebbs index (full brain restored)
Reset           rm -rf .hebbs/ + hebbs index . (clean rebuild from files)
```

One brain. One binary. Files are the truth. The engine is disposable. The agent gets smarter every day.
