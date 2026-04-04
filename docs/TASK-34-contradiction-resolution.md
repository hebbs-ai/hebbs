# TASK-34: Contradiction Resolution Post-Processor

## Problem

Contradiction detection today is write-only metadata. The pipeline stops at labeling:

1. During ingest, `check_contradictions()` classifies pairs via LLM and writes `CONTRADICTS` or `REVISED_FROM` graph edges.
2. The `contradiction_writer` outputs markdown files to `contradictions/`.
3. **Nothing consumes these edges.** Recall (`hebbs-core/src/recall.rs`) has zero awareness of contradiction edges. Both conflicting memories are returned equally in search results with no filtering, deranking, or annotation.
4. The Phase 2 prepare/commit REST API exists but is unused in enterprise (LLM is always present during ingest, so everything goes through the immediate LLM path).

The system detects contradictions but never resolves them. Users accumulate conflicting memories with no automated cleanup.

## Goal

Build a post-processor that consumes contradiction graph edges and resolves them: updating source files for file-backed memories, or updating/soft-deleting pure memories.

## Key Findings (from research)

### What exists today

| Component | File | What it does |
|-----------|------|--------------|
| Contradiction detector | `hebbs-core/src/contradict.rs` (921 lines) | HNSW top-K search, LLM or heuristic classification |
| LLM classifier | `hebbs-llm/src/contradiction.rs` (138 lines) | Structured prompt: contradiction / revision / dismiss |
| Contradiction writer | `hebbs-vault/src/contradiction_writer.rs` (496 lines) | Writes markdown files to `contradictions/` |
| Graph edges | `CONTRADICTS` (bidirectional), `REVISED_FROM` (B supersedes A) | Stored in Graph CF with confidence + timestamp |
| Engine API | `check_contradictions()`, `contradiction_prepare()`, `contradiction_commit()` | Detection + unused review flow |
| Enterprise surface | `hebbs-platform/src/routes/config.ts`, `hebbs-dashboard/src/app/settings/page.tsx` | Enabled toggle only |

### Memory timestamps available

Every `Memory` struct carries:
- `created_at`: microseconds since epoch, set on initial write
- `updated_at`: starts equal to `created_at`, updated by `revise()`
- `last_accessed_at`: updated on every `recall()` hit

For `REVISED_FROM` edges, staleness is already encoded (A is the older version). For `CONTRADICTS` edges, `created_at` / `updated_at` can serve as a heuristic before falling back to LLM judgment.

### Two memory lineage types

- **File-backed memories**: Ingested from vault files. Source of truth is the file. Resolution requires editing the file, then re-ingest propagates the fix.
- **Pure memories**: Created via `remember` API, no file source. Resolution means updating or soft-deleting the stale memory directly in storage.

## Design

### Resolution pipeline

```
Graph CF scan for CONTRADICTS / REVISED_FROM edges
        |
        v
  [1. Load both memories]
        |
        v
  [2. Determine which is stale]
        |  REVISED_FROM: A is stale (already encoded)
        |  CONTRADICTS: use timestamps as heuristic,
        |               fall back to LLM judgment
        v
  [3. Check lineage]
        |
        +-- File-backed? --> [4a. Edit source file, re-ingest]
        |
        +-- Pure memory? --> [4b. Soft-delete or revise stale memory]
        |
        v
  [5. Clean up edge + contradiction .md file]
```

### Step 1: Recall-time contradiction awareness

Before full resolution, recall should at minimum surface contradiction metadata:

- When a recalled memory has `CONTRADICTS` or `REVISED_FROM` edges, include that signal in the recall response.
- Option A (minimal): add a `contradicted: bool` flag to recall results.
- Option B (richer): return the contradicting memory ID + edge type + confidence so callers can make informed decisions.
- For `REVISED_FROM`, consider deranking the superseded memory (memory A) in recall scoring.

**Files to modify:** `hebbs-core/src/recall.rs`, proto definitions, REST response types, SDK types.

### Step 2: REVISED_FROM auto-resolution

`REVISED_FROM` is the easy case since staleness is explicit:

- Memory A (the old version) should be soft-deleted or marked as superseded.
- If file-backed: flag the source file section for update (or auto-update if content mapping is precise enough).
- If pure memory: mark as superseded in storage (new field or status enum), exclude from default recall.
- Clean up the `REVISED_FROM` edge and any `contradictions/*.md` file.

### Step 3: CONTRADICTS resolution

Requires judgment to determine which memory is correct:

- **Timestamp heuristic**: newer `created_at` / `updated_at` wins by default.
- **LLM arbitration**: for close calls or when timestamps are similar, ask the LLM which memory reflects current truth given the full context.
- **Human review fallback**: surface unresolvable contradictions in the dashboard for manual resolution.

### Step 4: Enterprise dashboard integration

- Surface pending contradictions in dashboard (not just the toggle).
- Allow manual resolution: pick winner, edit content, dismiss false positive.
- Show resolution history / audit trail.

## Open Questions

1. Should soft-deleted memories be fully removed or kept as tombstones for audit?
2. For file-backed resolution, should the system auto-edit vault files or only flag them for human edit?
3. Should recall derank or fully exclude superseded memories?
4. How to handle contradictions where both memories are partially correct (merge rather than pick a winner)?
5. Should the contradiction markdown files in `contradictions/` be cleaned up on resolution or kept as a log?

## Scope

- **Phase A**: Recall-time awareness (Step 1) + REVISED_FROM auto-resolution (Step 2)
- **Phase B**: CONTRADICTS resolution with LLM arbitration (Step 3)
- **Phase C**: Dashboard integration for manual review (Step 4)
