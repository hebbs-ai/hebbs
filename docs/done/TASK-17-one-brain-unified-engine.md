# TASK-17: One Brain, Unified Engine

Parent: [TASK-12](./TASK-12-markdown-obsidian-cognitive-layer.md)

## Problem

HEBBS currently has two separate RocksDB instances: one inside `.hebbs/index/db/` (vault) and one in `~/.hebbs/data/` (server). Same engine code (`hebbs-core::Engine`), two storage locations, two brains. A user recalling memories gets different results depending on which CLI they use. File-backed memories and agent-stored memories are invisible to each other.

There are also two CLI binaries (`hebbs-cli` and `hebbs-vault`) with overlapping commands and different capabilities. A user should not have to choose.

## Goal

One brain. One RocksDB. One CLI. The engine knows about file-backed memories and agent-stored memories. Recall returns everything regardless of how it got there.

---

## Architecture: One Brain

```
~/.hebbs/                              ← ONE brain location (or per-vault .hebbs/)
  index/db/                            ← ONE RocksDB instance
  manifest.json                        ← tracks file-backed memories (source, path, heading, bytes)
  config.toml                          ← unified config

Sources that write to the brain:
  1. Vault watcher     (file changed → parse → embed → Engine::remember)
  2. Agent via CLI     (hebbs remember "..." → direct to RocksDB)
  3. Agent via REST    (POST /v1/memories → direct to RocksDB)
  4. Agent via MCP     (remember tool → direct to RocksDB)
  5. Agent via gRPC    (server wraps same Engine, optional for remote/team)

Sources that read from the brain:
  1. CLI recall        (hebbs recall "..." → Engine::recall → results from ALL sources)
  2. Panel             (hebbs panel → reads same Engine → shows everything)
  3. REST API          (GET /v1/recall → same Engine)
  4. MCP               (recall tool → same Engine)
```

### Key principle: two tiers of memory

**File-backed memories** are the user's actual content: notes, docs, meeting notes, code comments. The watcher indexes them. Delete `.hebbs/` and rebuild from files; nothing is lost. These are the user's content, and the index is just a lens over them.

**Agent-stored memories** are written directly to RocksDB via `remember` (CLI, REST, MCP, gRPC). They are not materialized as markdown files. One-liners like "prefers dark mode" or "uses Rust" do not need to become files cluttering the vault. Frameworks already ship their own memory files (Claude Code has `CLAUDE.md` + `memory/`, Cursor has `.cursorrules`, OpenClaw has `MEMORY.md`). HEBBS does not duplicate that.

Agent-stored memories are visible through `recall`, `list`, and the control panel. They are backed up and migrated via `hebbs export` / `hebbs import`. They are not rebuildable from files, because they were never files.

| | File-backed | Agent-stored |
|---|---|---|
| Storage | RocksDB (indexed from files) | RocksDB (direct) |
| Source of truth | Markdown files | RocksDB |
| Survives `.hebbs/` delete + rebuild | Yes | No (use export/import) |
| Visible in panel/recall | Yes | Yes |
| User edits how? | Edit the file | `hebbs forget` + `hebbs remember`, or via panel |
| Portability | Git sync the files, rebuild on new machine | `hebbs export` / `hebbs import` |

---

## One CLI

### Current state: two binaries

| Binary | Commands | Connection |
|---|---|---|
| `hebbs-cli` | remember, recall, revise, forget, prime, reflect, insights, inspect, export, subscribe, feed, status | gRPC to hebbs-server (requires running server) |
| `hebbs-vault` (hv) | init, index, watch, rebuild, status, recall, list | Embedded engine (no server needed) |

### Target state: one binary

```
hebbs                                  ← ONE binary
  init <path>                          ← initialize vault
  index <path>                         ← index all files
  watch <path>                         ← file watcher daemon
  rebuild <path>                       ← delete index, rebuild from files
  remember <text> [--importance] [--entity-id]  ← store memory (direct to RocksDB)
  recall <query> [--strategy] [--weights] [-k]  ← search ALL memories
  forget [--ids] [--entity-id] [--staleness]    ← remove memories
  prime [--entity-id] [--max-memories]           ← warm context
  reflect-prepare [--entity-id]                  ← two-step insight generation
  reflect-commit [--session-id] [--insights]     ← commit insights (writes .md files)
  insights [--entity-id] [--min-confidence]      ← query consolidated insights
  list [--sections] [--source]                   ← list all memories
  status                                         ← brain health
  panel                                          ← open memory palace
  export [--format jsonl|md]                     ← export brain
  import [--from chatgpt|claude|jsonl]           ← import memories
  serve [--grpc-port] [--http-port]              ← start server (optional, for remote/team)
```

### How the CLI finds the brain

```
1. Check --endpoint flag or HEBBS_ENDPOINT env var
     → if set: REMOTE MODE (gRPC/REST client to server or cloud)
2. Check --vault <path> flag or HEBBS_VAULT env var
     → if set: LOCAL MODE (embedded engine at that path)
3. Walk up from current directory looking for .hebbs/
     → if found: LOCAL MODE (embedded engine)
4. Check ~/.hebbs/config.toml for a default endpoint or vault
5. Fall back to ~/.hebbs/ (global local brain)
6. If nothing found: "No brain found. Run: hebbs init <path>"
```

This means:
- In a project directory with `.hebbs/`, commands use that vault's embedded engine automatically
- With `HEBBS_ENDPOINT=https://api.hebbs.ai`, the same commands talk to HEBBS Cloud
- The user never changes their commands, only their config

### Dual-mode engine: local or remote

The CLI is a dual-mode client. Same commands, same output, different backend.

```
                    hebbs recall "query"
                           |
                    .hebbs/config.toml
                           |
              +------------+------------+
              |                         |
        mode = "local"           mode = "remote"
              |                         |
      Embedded Engine            gRPC/REST Client
      (RocksDB in .hebbs/)       (to server or cloud)
              |                         |
        Local results            Remote results
              |                         |
              +------------+------------+
                           |
                    Same output format
```

**Local mode** (default): embedded `hebbs-core::Engine` with RocksDB in `.hebbs/`. Zero network. Zero latency. Works offline. Free.

**Remote mode**: gRPC/REST client to a HEBBS endpoint. Three scenarios:

| Scenario | Endpoint | Who runs it |
|---|---|---|
| HEBBS Cloud | `https://api.hebbs.ai` | HEBBS (managed, synced, paid) |
| Self-hosted | `https://hebbs.internal.co:6380` | Customer's infra (enterprise) |
| Local server | `localhost:6380` | `hebbs serve` on the same machine (for multi-agent) |

**Config is the only difference:**

```toml
# Local mode (default, no config needed)
[engine]
mode = "local"

# HEBBS Cloud
[engine]
mode = "remote"
endpoint = "https://api.hebbs.ai"
api_key = "hb_..."

# Enterprise self-hosted
[engine]
mode = "remote"
endpoint = "https://hebbs.internal.company.com:6380"
api_key = "..."
```

### What changes between modes

| Feature | Local mode | Remote mode |
|---|---|---|
| `recall`, `remember`, `forget`, `prime`, `insights` | Works | Works (same commands) |
| `init`, `index`, `watch`, `rebuild` | Works (file operations) | Not applicable (no local files) |
| `panel` | Works (reads local engine) | Works (reads remote API) |
| `reflect-prepare`, `reflect-commit` | Works | Works |
| `list --sections` | Shows file paths + headings | Shows memory IDs + entities |
| `serve` | Starts server on local engine | Not applicable (already remote) |
| Offline | Yes | No |
| File-backed portability | Yes (git sync, rebuild) | No (cloud handles persistence) |

### Cloud product implications

The cloud product is just `hebbs-server` hosted by HEBBS. The user changes one line in config.toml and their CLI talks to the cloud instead of the local engine. Their agent skill doesn't change. Their MCP server doesn't change. Their control panel connects to the remote API.

For cloud users who also want local files (hybrid): the vault syncs file-backed memories to the cloud endpoint. Cloud stores both file-synced and agent-stored memories. This is a future feature but the architecture supports it because the API surface is identical.

### Server mode

`hebbs serve` starts gRPC + REST endpoints wrapping the SAME engine the CLI uses locally. Three use cases:
1. **Multi-agent on one machine**: multiple agents talk to one brain via gRPC
2. **Self-hosted enterprise**: team shares one brain on internal infra
3. **Development**: HEBBS Cloud runs the same server code at scale

---

## Migration Path

### From hebbs-server users

1. `hebbs export --from-server localhost:6380 --format jsonl` exports all server memories
2. `hebbs import --from jsonl export.jsonl` imports into the local vault's RocksDB
3. Server no longer needed (or run `hebbs serve` on the same vault)

### From hebbs-vault users

No migration needed. Vault already has the right architecture. All core commands (remember, forget, prime, reflect, insights) are implemented.

---

## Manifest Changes

The manifest tracks file-backed memories only. Agent-stored memories live in RocksDB and are not represented in the manifest (they have no source file).

Both tiers live in the same RocksDB index. Recall searches everything. The panel shows everything. `list --source file` or `list --source agent` filters if needed (source is a field on the memory record in RocksDB, not in the manifest).

---

## What Changes in Each Crate

| Crate | Change | Status |
|---|---|---|
| **hebbs-vault** | Add `remember`, `forget`, `prime`, `reflect-prepare`, `reflect-commit`, `insights`, `panel`, `export`, `import`, `serve` commands. Rename binary to `hebbs`. Add dual-mode engine resolution (local embedded or remote gRPC client). | Done (except `panel`, `import`, `serve`). Binary is `hebbs`. Dual-mode resolution implemented. `remember`, `forget`, `prime`, `reflect-prepare`, `reflect-commit`, `insights`, `export` all work in both local and remote modes. |
| **hebbs-server** | Becomes a library function (`serve`) called from the unified binary. Same code powers `hebbs serve`, self-hosted enterprise, and HEBBS Cloud. | Not started. Server is still a separate binary. |
| **hebbs-cli** | Deprecated for end users. All commands move to the unified binary. Kept as `hebbs-client` crate (the gRPC/REST client library used by the unified binary in remote mode). | Done. All commands moved to unified binary. `hebbs-cli` library used by vault CLI in remote mode. |
| **hebbs-core** | No change. Engine is already unified. | Done. No change needed. |
| **hebbs-storage** | No change. Single RocksDB backend. | Done. No change needed. |
| **hebbs-client** | Stays. Used by the unified binary when `mode = "remote"`. Same client that powers hebbs-cli today. | Done. Used by vault CLI for remote mode. |
| **hebbs-skill** | Rewrite SKILL.md to teach agents the unified CLI. One doc. Agent does not need to know if the brain is local or remote. | Not started. |

---

## Skill Unification

One skill doc. One decision tree:

```
Agent needs to recall?
  → hebbs recall "query" [--strategy] [--weights]

Agent needs to store a memory?
  → hebbs remember "content" [--importance] [--entity-id]
  (stores directly to RocksDB, no file created)

Agent needs context at conversation start?
  → hebbs prime [--entity-id]

Agent needs insights?
  → hebbs insights [--entity-id]

User wants to see the brain?
  → hebbs panel
```

No mention of "server mode" unless the user explicitly asks about team/remote scenarios.

---

## Phased Delivery

### Phase 1: Unified CLI commands on vault -- DONE

- ~~Add `remember` command (stores directly to RocksDB)~~ Done
- ~~Add `forget` command (deletes file or removes section)~~ Done (supports filtering by staleness, access floor, kind, decay floor)
- ~~Add `prime` command (blends recent + relevant)~~ Done (temporal + similarity scoring, both modes)
- ~~Rename binary from `hebbs-vault` to `hebbs`~~ Done
- ~~Brain discovery logic (walk up for .hebbs/, fall back to ~/.hebbs/)~~ Done (full priority chain: explicit arg > --vault flag > HEBBS_VAULT env > walk up > ~/.hebbs/)
- ~~Dual-mode engine resolution (local embedded or remote gRPC client)~~ Done (--endpoint flag or HEBBS_ENDPOINT env var triggers remote mode)
- ~~Export command~~ Done (JSONL format with entity_id scope and limit)

**Design decision:** Agent-stored memories (via `remember`) go directly to RocksDB, not as markdown files. No file materialization needed. Portable via `hebbs export` / `hebbs import`.

### Phase 2: Reflect and insights -- DONE

- ~~Add `reflect-prepare` and `reflect-commit` (agent-driven, writes insight .md files)~~ Done (both modes)
- ~~Add `insights` query command~~ Done (filters: entity_id, min_confidence, max_results)

### Phase 3: Panel (TASK-16) -- NOT STARTED

- Memory palace control panel
- Depends on unified engine (this task, now largely complete)

### Phase 4: Server as library -- NOT STARTED

- `hebbs serve` command wrapping the same engine
- Deprecate standalone hebbs-server binary
- Deprecate hebbs-cli binary (or keep as alias)

Note: hebbs-server still exists as a separate binary. Not yet embedded as a subcommand of the unified CLI.

### Phase 5: MCP + integrations -- NOT STARTED

- MCP server wrapping vault engine
- Import from ChatGPT/Claude/other tools

### Remaining work

- `panel` command (TASK-16)
- `import` command (ChatGPT/Claude/JSONL)
- `serve` command (embed server as library)
- MCP server
- SKILL.md rewrite for unified CLI

---

## Success Criteria

1. One `hebbs` binary. `brew install hebbs` gives you everything. -- **DONE**
2. `hebbs recall` returns results from files AND agent-stored memories. -- **DONE** (both stored in same RocksDB)
3. `hebbs remember` stores a memory that appears in recall, list, and panel. -- **DONE** (direct to RocksDB)
4. Deleting `.hebbs/` and running `hebbs index` rebuilds file-backed memories from files. -- **DONE**
5. Agent-stored memories are portable via `hebbs export` / `hebbs import`. -- **PARTIAL** (export works, import not yet implemented)
6. A user on a new machine with synced files runs `hebbs init && hebbs index` and has file-backed memories back. Agent-stored memories restored via `hebbs import`. -- **PARTIAL** (import not yet implemented)

---

## Status

Phase 1 and Phase 2 complete. Unified CLI binary (`hebbs`) ships with all core commands (remember, forget, prime, recall, reflect-prepare, reflect-commit, insights, export) working in both local and remote modes. Brain discovery and dual-mode resolution implemented.

Remaining: Panel (Phase 3), server-as-library (Phase 4), MCP + import (Phase 5).
