# TASK-15: Hybrid Memory -- File-Backed and API-Stored Coexistence

## Context

TASK-12 introduces a file-first architecture where markdown files are the source of truth and `.hebbs/` is a rebuildable index. But HEBBS also serves agents that store memories directly via the API (e.g., user preferences from conversation, ephemeral context, tool outputs). These API-stored memories have no file backing. Both modes need to coexist cleanly in a single engine instance.

## Problem

After TASK-12, a single engine could hold:
- 500 memories from indexed vault files (file-backed, rebuildable)
- 50 memories from agent conversations (API-stored, no file source)

Queries (`recall()`, `prime()`, `reflect()`) see all memories equally -- the engine doesn't distinguish provenance. This works today because everything is API-stored. But with the vault layer, several assumptions break.

### The Rebuild Problem

`hebbs rebuild` deletes `.hebbs/` and re-indexes from files. The engine's RocksDB lives inside `.hebbs/index/`. API-stored memories live in RocksDB. Therefore: **rebuild destroys API-stored memories**.

This violates the "delete `.hebbs/` and lose nothing" guarantee for API-stored memories, because they have no file source to rebuild from.

### The Query Problem

Step 6 of PLAN-12 (`query.rs`) intercepts recall/prime results and reads content from files via the manifest. Memories without manifest entries (API-stored) fall through to stored content. This works, but the behavior is implicit -- there's no explicit concept of "this memory is file-backed" vs "this memory is API-only."

### The Reflect Problem

`reflect()` consolidates clusters into insights. Insight output (Step 7) writes `.md` files with `hebbs-sources` pointing to file paths. If source memories are API-stored (no file path), the source reference is meaningless or broken.

## Design Options

### Option A: Fully File-First (recommended for simplicity)

Require all memories to be file-backed. Agents write preferences and context as markdown files in a designated directory (e.g., `vault/agent/`). The watcher picks them up and indexes them like any other file.

**Pros**: One model, rebuild always works, insight sources always resolve, simplest architecture.
**Cons**: Agents must write files instead of calling `remember()` directly. Adds I/O overhead for ephemeral memories. Some agent workflows (e.g., MCP tool calls) may not have filesystem access.

### Option B: Dual-Track with Provenance Tagging

Tag each memory with provenance: `FileBackedMemory` or `ApiStoredMemory`. Handle each track explicitly:

- **Rebuild**: only destroys file-backed memories (re-indexed from files). API-stored memories are preserved in a separate storage location outside `.hebbs/` (e.g., `vault/.hebbs-api-store/` or a configurable path).
- **Query**: `query.rs` checks provenance -- file-backed memories read from files, API-stored memories read from engine storage. No behavioral change, but the distinction is explicit.
- **Insight output**: `hebbs-sources` entries for API-stored memories reference `api://<memory_id>` instead of file paths. Less human-readable, but accurate.
- **Rebuild guarantee**: "delete `.hebbs/` and lose nothing *that has a file source*." API-stored memories have their own persistence guarantee via the separate store.

**Pros**: Both modes work naturally. Agents don't need filesystem access. Clean separation.
**Cons**: Two storage locations. Rebuild semantics are more nuanced. Insight source references are inconsistent.

### Option C: Auto-Materialize API Memories as Files

When an agent calls `remember()` directly (not via file ingest), the vault layer automatically writes a `.md` file for it:

```markdown
---
hebbs-kind: agent-memory
hebbs-agent: agent-name-or-id
hebbs-created: 2026-03-14T10:00:00Z
---

User prefers dark mode. Confirmed in conversation on 2026-03-14.
```

Written to `vault/agent-memories/` (configurable). The watcher then indexes it like any other file, completing the loop.

**Pros**: Everything becomes file-backed. Rebuild works. Human can inspect agent memories. Files are the single source of truth.
**Cons**: Write amplification (every `remember()` call writes a file). Watcher must detect and skip its own writes to avoid loops. Latency increase on `remember()` path.

## Open Questions

- Which option best serves the "engine is disposable, files are permanent" thesis?
- Should the choice be configurable per-deployment (vault mode vs API mode vs hybrid)?
- How do ephemeral/short-lived memories (e.g., "user is currently editing file X") fit? Writing a file for a 30-second memory seems wasteful.
- If Option C, how to handle the write loop (engine writes file, watcher sees file, watcher calls engine)?
- Should `remember()` in vault mode accept an optional `file_path` parameter to let callers choose?

## Dependencies

- TASK-12 (architecture definition)
- TASK-13 / PLAN-12 Steps 1-9 (file-first implementation)

## Status

Not started. Design decision needed before or during TASK-13 implementation.
