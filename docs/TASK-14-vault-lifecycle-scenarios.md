# TASK-14: Vault Lifecycle Scenarios

Parent: [TASK-13](./TASK-13-file-first-markdown-sync.md)

## Purpose

TASK-13 covers four core lifecycle scenarios (first install, realtime editing, agent writing, bulk arrival). This task captures additional real-world scenarios that the vault architecture must handle. These should be thought through and resolved before or during implementation -- not discovered in production.

**This list is not exhaustive. Before implementing each TASK-13 milestone, actively think through what other lifecycle scenarios could break it. Add them here.**

---

## Scenario 5: User Edits an Engine-Generated Insight File

The engine wrote an insight file. The user disagrees, corrects it, or adds context.

- Edit = the watcher treats it like any file edit. Phase 1 re-parses, phase 2 re-embeds. The user's version becomes the truth.
- The engine must never overwrite a user-edited insight. The `hebbs-sources` frontmatter links back to the original source memories, but the content is now user-owned.
- How does reflect() know this insight already exists and shouldn't be re-proposed? Track insight lineage (source memory_ids -> insight file path) so reflect skips clusters that already produced a user-accepted or user-edited insight.

---

## Scenario 6: User Deletes an Engine-Generated Insight File

The user rejects the engine's insight by deleting the file.

- Watcher detects deletion, runs `forget()` on the insight's memory_id.
- The engine must not re-generate the same insight. Need a tombstone or suppression list: "these source memory combinations have been rejected." Store in `.hebbs/` (cognition plane), not in the vault.
- What if the source memories change significantly over time? At some point the suppression should expire. Configurable TTL on suppression entries.

---

## Scenario 7: File Rename / Move

User reorganizes their vault (e.g., moves `notes/meeting.md` to `archive/2026/meeting.md`).

- Treat as delete + create. No rename detection needed.
- Watcher sees file gone at old path: `forget()` all sections, remove from manifest.
- Watcher sees file appear at new path: parse, `remember()` all sections, add to manifest.
- Accepted losses (same as `hebbs rebuild`): decay scores, access counts, reinforcement signals reset. They rebuild naturally through usage.
- Graph edges from old sections are cleaned up by `forget()`. New edges are created from the new sections.
- Insight files with `hebbs-sources` pointing to the old path: cosmetic breakage only. The insight content remains valid and indexed. The engine doesn't rewrite insight frontmatter.
- Wiki-links in other files (`[[old-name]]`) that break are a user problem -- the engine doesn't rewrite user prose. If the filename didn't change (just the directory), Obsidian-style wiki-links (name-only, not path-based) still resolve fine.

---

## Scenario 8: Vault Reopened After Dormancy

User hasn't touched the vault in weeks. Files may have been edited outside the watcher (cloud sync, another machine, another app).

- On `hebbs watch` startup: walk all files, compare checksums against manifest. Any mismatches trigger phase 1 + phase 2. Essentially a differential re-index.
- Decay scores are stale. Run a decay update pass on startup.
- If many files changed (>threshold), treat as bulk arrival (adaptive debounce, batch processing).
- Should `hebbs watch` always do a startup scan? Yes. Cost is one directory walk + checksum comparisons. Cheap.

---

## Scenario 9: Git Branch Switch / Merge / Rebase

Many files change atomically. Some appear, some disappear, some revert to older versions.

- Watcher fires a burst of events (hundreds of creates, deletes, modifies simultaneously).
- Must handle as bulk arrival (adaptive debounce, batch processing).
- Reverted files (content goes back to an older version) should NOT be treated as "new." If the checksum matches a previously-indexed version, the old embedding may still be valid. Check manifest history? Or just re-embed (simpler, slightly wasteful).
- Branch-specific state: should `.hebbs/` be branch-aware? Probably not for v1. The index reflects whatever files are currently on disk. Switching branches re-indexes the diff. Accept the cost.

---

## Scenario 10: Dangling Wiki-Links (Forward References)

File A contains `[[not-yet-created]]`. The target file doesn't exist yet.

- Phase 1 extracts the wiki-link but can't resolve it to a memory_id.
- Store as a pending edge in the manifest or graph: `{source: memory_id_A, target_path: "not-yet-created", resolved: false}`.
- When a file matching that name is later created, resolve the pending edge retroactively. Phase 1 of the new file should check for pending edges pointing to it.
- Multiple files may link to the same not-yet-created target. All pending edges resolve at once when the target appears.

---

## Scenario 11: Non-Markdown Files in Vault

User has images, PDFs, CSVs, code files in the vault alongside markdown.

- Default: ignore non-`.md` files. The watcher filters by extension.
- Configurable include list in `config.toml` (e.g., `extensions = ["md", "txt"]`).
- Future consideration: images referenced in markdown (`![](image.png)`) could be tracked as edges even if the image itself isn't indexed.

---

## What Else?

This list is incomplete. Before implementing each TASK-13 milestone, ask:

- What happens if this operation is interrupted halfway?
- What happens if two of these scenarios overlap (e.g., user editing while agent is also writing)?
- What happens at scale (10,000 files, 100,000 sections)?
- What happens on a slow machine or slow disk?
- What happens if the embedding model changes (different dimensions, different model)?
- What happens if the user manually edits `.hebbs/manifest.json`?
- What happens with symlinks pointing outside the vault?
- What happens with very large files (50MB markdown export)?
- What happens with circular wiki-links (A -> B -> A)?
- What happens if the user runs two `hebbs watch` processes on the same vault?

Add scenarios here as they're discovered. Better to find them now than in production.

---

## Status

Living document. Update as new scenarios are identified during TASK-13 implementation.
