# TASK-15: Hybrid Memory -- File-Backed and API-Stored Coexistence

**Status:** RESOLVED. Design decision made during [TASK-17](./TASK-17-one-brain-unified-engine.md). Option B (dual-track) selected. Implemented as the two-tier model in the unified engine.

---

## Context

TASK-12 introduces a file-first architecture where markdown files are the source of truth and `.hebbs/` is a rebuildable index. But HEBBS also serves agents that store memories directly via the API (e.g., user preferences from conversation, ephemeral context, tool outputs). These API-stored memories have no file backing. Both modes need to coexist cleanly in a single engine instance.

## Resolution

Decided during TASK-17 (one-brain-unified-engine) work. The two-tier model:

| Tier | Source | Rebuild behavior | Portability |
|------|--------|-------------------|-------------|
| File-backed | Markdown files in vault | Rebuildable from files. `hebbs rebuild` re-indexes everything. | Files are the source of truth. Copy files = copy memories. |
| Agent-stored | `hebbs remember` / API calls | Lives in RocksDB only. Lost on `hebbs rebuild`. | `hebbs export` / `hebbs import` for migration. |

### Key decisions

1. **"All writes become files" was rejected.** Agent memories do NOT auto-materialize as markdown files (Option C rejected). Reasons: write amplification, watcher loop complexity, ephemeral memories don't warrant files.

2. **Option B (dual-track) selected with simplification.** No separate storage location for API memories. Both tiers live in the same RocksDB inside `.hebbs/index/db`. The distinction is logical (provenance), not physical.

3. **Rebuild destroys agent-stored memories.** This is accepted. Users are told to `hebbs export` before rebuild if they want to preserve agent memories. The SKILL.md documents this.

4. **Two-vault pattern reduces the impact.** With global vault (`~/.hebbs/`) for user preferences and project vaults for project context, agent-stored memories are split across vaults. Rebuilding a project vault doesn't touch global agent memories (preferences, corrections, writing style).

5. **Insight sources for agent memories** reference `memory_id` when no file path exists. Less human-readable but accurate.

### What was NOT needed

- No `FileBackedMemory` / `ApiStoredMemory` provenance enum. The manifest already tracks which memories have file sources (they have manifest entries). Memories without manifest entries are implicitly API-stored.
- No separate `.hebbs-api-store/` directory. Single RocksDB is simpler.
- No per-deployment configuration. Both tiers always coexist.

---

## Original Design Options (for reference)

### Option A: Fully File-First -- REJECTED
Require all memories to be file-backed. Agents write preferences as markdown files.
Rejected: agents don't always have filesystem access (MCP, remote mode). Write amplification for ephemeral memories.

### Option B: Dual-Track with Provenance Tagging -- SELECTED (simplified)
Tag each memory with provenance, handle rebuild/query/reflect per track.
Selected with simplification: no physical separation, manifest presence = file-backed.

### Option C: Auto-Materialize API Memories as Files -- REJECTED
Every `remember()` call writes a markdown file automatically.
Rejected: write amplification, watcher loop complexity, latency on hot path.

## Dependencies

- [TASK-12](./TASK-12-markdown-obsidian-cognitive-layer.md) (architecture definition) -- DONE
- [TASK-13](./done/TASK-13-file-first-markdown-sync.md) / [PLAN-12](./plans/PLAN-12.md) Steps 1-9 (file-first implementation) -- DONE
- [TASK-17](./TASK-17-one-brain-unified-engine.md) (unified engine, where decision was made) -- DONE
