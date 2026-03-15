# Blog: The Two-Vault Pattern

**Status:** To write
**Related:** TASK-17, TASK-16, SKILL.md rewrite

## What to communicate

HEBBS now supports a two-vault pattern for AI agents: a global vault (`~/.hebbs/`) for user identity (preferences, writing style, corrections) and per-project vaults (`.hebbs/`) for project-specific context (architecture decisions, conventions, deployment patterns).

### Key points

- **`--global` flag**: any command can target the global vault with `--global`. Without it, commands use the project vault discovered from the current directory.
- **Agent is the intelligence, HEBBS is the memory**: the engine doesn't know about "global vs project." It just opens whichever vault discovery resolves to. The agent (OpenClaw, Claude Code, Cursor) decides where to store and where to recall. The SKILL.md teaches this pattern.
- **Full isolation**: memories in the global vault are invisible to project vaults and vice versa. No federation, no aggregation, no leaking. Each vault is its own RocksDB.
- **Vault registry**: `hebbs init` registers each vault path in `~/.hebbs/vaults.json`. The control panel (`hebbs panel`) reads this registry and provides a dropdown to switch between vaults without restarting.
- **Session start pattern**: agents prime both vaults at conversation start. `hebbs prime --global` for user context, `hebbs prime` for project context. Two calls, merged by the agent.

### Why this matters

Every AI memory system today is either global-only (ChatGPT, Mem0) or workspace-only (file-based MEMORY.md). Neither handles the reality that users have both personal preferences that span all projects and project-specific context that shouldn't leak.

HEBBS solves this by keeping the engine simple (one vault, one RocksDB) and pushing routing intelligence to the agent. The result: user preferences follow you everywhere, project context stays scoped, and neither pollutes the other.

### Technical details worth covering

- The `--global` flag is a CLI convenience that resolves to `~/.hebbs/` directly, skipping project discovery
- No engine changes required. Same `resolve_vault_path` function, just a priority override
- The SKILL.md teaches agents a clear routing table: "Would this matter in a different project? If yes, store globally."
- The vault registry (`vaults.json`) enables the control panel to navigate across all vaults from a single UI

### Audience

Developers building with AI agents. AI framework builders. Users who have been frustrated by context pollution (work context leaking into personal, one project's conventions applied to another).
