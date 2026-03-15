# TASK-13: File-First Markdown Sync

Parent: [TASK-12](./TASK-12-markdown-obsidian-cognitive-layer.md) (Feature 1)

## Goal

Build the content plane / cognition plane architecture where markdown files are the source of truth and `.hebbs/` is a rebuildable index. No contradiction detection, no token budgeting -- just the foundational loop: **files -> index -> queries -> new files (insights) -> index.**

---

## Lifecycle Scenarios

Four scenarios drive the design. Every architectural decision must handle all four cleanly.

| Scenario | What happens | Volume | Speed |
|----------|-------------|--------|-------|
| **First install** | User has an existing vault (100-10,000 .md files). Runs `hebbs init` + `hebbs index`. | Bulk | One-time |
| **Realtime editing** | User has a file open, saving every 1-5 seconds (autosave or manual). | Single file, repeated | Continuous |
| **Agent writing** | AI agent creates/modifies 10-50 files in rapid succession (seconds). | Burst | Fast |
| **Bulk arrival** | User drops files from a zip, git pull, cloud sync, or copy-paste. | Bulk | One-time |

---

## Two-Phase Processing Model

Processing is NOT atomic. Parsing is microseconds. Embedding is milliseconds-to-seconds. They must be decoupled.

**Phase 1 -- Parse (cheap, immediate)**:
- Parse markdown into sections
- Update manifest with new checksums, byte offsets, heading paths
- Resolve wiki-links to file paths (not yet to memory_ids if target isn't indexed)
- Runs on every file change, within the debounce window
- Cost: microseconds per file

**Phase 2 -- Embed (expensive, batched)**:
- Collect all sections marked as needing embedding
- Batch embed (one embedding call for N sections, not N calls for N sections)
- Update HNSW index, graph edges, temporal index
- Runs on a timer (e.g., every 2-5 seconds) or when no file changes for N seconds
- Cost: milliseconds-to-seconds per batch

Between phases, a section is **content-stale**: the manifest has current byte offsets (so queries return fresh file content), but the embedding is from the old content (so ranking might be slightly off). This is acceptable. The user sees correct content; the ranking self-corrects when phase 2 runs.

### Section States

```
                  file created/modified
                         |
                         v
                    [ PENDING ]
                         |
                    phase 1 (parse)
                         |
                         v
                 [ CONTENT-STALE ]
                         |
                    phase 2 (embed)
                         |
                         v
                     [ SYNCED ] <--- checksum matches, embedding current
                         |
                    file modified
                         |
                         v
                 [ CONTENT-STALE ] ---> (cycle continues)

                    file deleted
                         |
                         v
                   [ ORPHANED ]
                         |
                    cleanup (forget)
                         |
                         v
                    (removed)
```

### Per-Scenario Behavior

**First install**: `hebbs index` runs phase 1 for all files (fast, seconds), then phase 2 in batches (slower, parallelized). Progress reporting: `indexing: 145/500 files parsed, 80/500 embedded`. Manifest is written incrementally (not at the end), so a crash at file 300 resumes from 300, not from 0.

**Realtime editing**: phase 1 runs on every save (after debounce). Phase 2 is deferred until the file stops changing for N seconds (e.g., 3s). If the user saves 10 times in 20 seconds, phase 1 runs ~10 times (cheap), phase 2 runs once at the end (expensive, but only for the final content).

**Agent writing**: watcher collects all file events during the debounce window. Phase 1 runs for all changed files as a batch. Phase 2 collects all new sections across all files and embeds them in one batch call. 20 files = 1 parse batch + 1 embed batch, not 20 independent pipelines.

**Bulk arrival**: same as agent writing but larger. The watcher detects the burst (many events in short window) and extends the debounce window automatically (adaptive debounce). Phase 2 processes in configurable batch sizes (e.g., 50 sections per embed call) to avoid memory pressure.

---

## Milestone 1: Markdown Parser

Parse a single `.md` file into memory-ready chunks.

**Input**: file path
**Output**: list of sections, each with:
- content (the prose under the heading)
- heading path (e.g., `## Design > ### API` becomes `["Design", "API"]`)
- frontmatter metadata (parsed YAML)
- wiki-links extracted (`[[target]]` and `[[target|alias]]`)
- tags extracted (`#tag`)
- byte offsets (start, end) for each section in the file

**Decisions**:
- Default chunking: split on `##` headings. Files with no headings = one chunk (the whole file).
- Frontmatter applies to all sections in the file (inherited metadata).
- Wiki-links within a section belong to that section's edges.

**Testable**: parse a directory of .md files, assert correct section count, content, metadata, links.

---

## Milestone 2: Manifest and Cognition Plane Layout

The `.hebbs/` directory structure and the manifest that maps files to indexed state.

**Structure**:
```
.hebbs/
  manifest.json       <- file -> memory_id mapping, checksums, section offsets
  config.toml         <- chunking strategy, embedding model, watched dirs
  index/
    hnsw.bin           <- vector index (later)
    temporal.bin       <- time-sorted index (later)
    graph.bin          <- edges (later)
```

**manifest.json schema**:
```json
{
  "version": 1,
  "files": {
    "notes/meeting-jan.md": {
      "checksum": "sha256:abc...",
      "last_parsed": "2026-03-13T20:00:00Z",
      "last_embedded": "2026-03-13T20:00:02Z",
      "sections": [
        {
          "memory_id": "01JABCDEF...",
          "heading_path": ["Vendor Evaluation"],
          "byte_start": 142,
          "byte_end": 890,
          "state": "synced",
          "content_checksum": "sha256:def..."
        }
      ]
    }
  }
}
```

**Section states in manifest**: `pending` (new, not yet parsed -- transient), `content-stale` (parsed, byte offsets current, embedding outdated), `synced` (embedding matches content). The `content_checksum` field tracks per-section content so phase 2 knows exactly which sections need re-embedding.

**Key property**: if a file's checksum matches, skip entirely. If it doesn't, re-parse (phase 1) and mark changed sections as `content-stale` for phase 2. Manifest is written incrementally after each file, not at the end of a batch.

**`hebbs init <vault-path>`**: initializes `.hebbs/` in the vault root. Creates manifest, config, index directory. Analogous to `git init`.

**Testable**: init a vault, verify `.hebbs/` structure, verify manifest updates on re-init.

---

## Milestone 3: Ingest Pipeline (two-phase)

### Phase 1: Parse and Manifest Update (cheap, immediate)

For each changed file:
1. Parse into sections (Milestone 1)
2. Diff new sections against manifest (by heading path)
   - Unchanged sections (same `content_checksum`): skip
   - Modified sections: update byte offsets, mark `content-stale`
   - New sections: assign memory_id, mark `content-stale`
   - Removed sections: mark `orphaned`
3. Update manifest incrementally (write after each file, not at end of batch)

### Phase 2: Embed and Index (expensive, batched)

Collect all `content-stale` sections across all files, then:
1. Read content from files at byte offsets
2. Batch embed (one call for N sections)
3. For new sections: call `remember()` with content, metadata, heading path
4. For modified sections: call `revise()` with same memory_id
5. For orphaned sections: call `forget()`
6. Resolve wiki-links to memory_ids, create RELATED_TO graph edges
7. Store tags as memory kind or context metadata
8. Mark sections as `synced` in manifest

### File-level operations

**File deletion**: mark all sections as `orphaned`, phase 2 calls `forget()`, remove from manifest.

**File rename/move**: treated as delete + create. No rename detection. Old sections are forgotten, new sections are remembered. Accepted loss: decay scores and access counts reset (same as `hebbs rebuild`). Keeps the watcher and manifest simple.

### CLI commands

**`hebbs index <vault-path>`**: full re-index. Phase 1 for all files (fast), then phase 2 in configurable batch sizes. Progress reporting: `parsed: 500/500, embedded: 145/500`. Idempotent (skips files with matching checksums). Crash-safe (manifest written incrementally).

**Testable**: ingest a vault, verify memories exist via `recall()`. Modify a file, re-ingest, verify revisions. Delete a file, verify `forget()`. Kill mid-index, restart, verify it resumes from where it left off.

---

## Milestone 4: File Watcher Daemon

Real-time bridge between content plane and cognition plane.

**Behavior**:
- Watch vault directory recursively for `.md` file events (create, modify, delete, rename)
- Ignore `.hebbs/` directory, `.git/`, `node_modules/`, `.obsidian/`, and configurable patterns
- Two debounce timers:
  - **Phase 1 debounce** (short, ~500ms): after last file event, run phase 1 (parse + manifest) for all changed files. Cheap enough to run frequently.
  - **Phase 2 debounce** (longer, ~3s of no changes): run phase 2 (embed + index) for all `content-stale` sections. Batched. Avoids re-embedding while user is still typing.
- **Adaptive debounce**: if more than N file events arrive in the phase 1 window (burst detection), extend the phase 2 debounce to let the burst complete before embedding.
- Phase 2 runs in a background thread. Queries continue to work during embedding (content-stale sections return fresh file content with slightly stale ranking).

**`hebbs watch <vault-path>`**: starts the daemon. Foreground process (daemonization is a later concern). Logs phase 1/phase 2 activity.

**Testable**:
- Start watcher, create a file, verify phase 1 runs within 500ms and phase 2 within 3s.
- Rapid-save a file 10 times in 5 seconds, verify phase 1 runs multiple times but phase 2 runs once at the end.
- Drop 50 files at once, verify batch processing (not 50 individual embed calls).
- Verify queries return correct content during the content-stale window between phase 1 and phase 2.

---

## Milestone 5: Query from Files

Recall and prime read content from files, not from stored memory content.

**Change**: when a recall/prime result is returned, the content field is populated by reading the file at the byte offsets stored in the manifest, not from the memory record's stored content.

**Why**: the file may have been edited since last embedding. The embedding might be slightly stale, but the content the user sees is always the current file content.

**Fallback**: if the file doesn't exist or offsets are invalid (file was modified but not yet re-indexed), fall back to stored content and flag as stale.

**Testable**: recall a section, modify the file (without re-indexing), recall again, verify content reflects the file edit.

---

## Milestone 6: Insight Output as Files

When `reflect()` produces insights, write them as `.md` files into the vault.

**Output directory**: configurable (default `insights/`), created automatically.

**File format**:
```markdown
---
hebbs-kind: insight
hebbs-sources:
  - notes/meeting-jan.md#vendor-evaluation
  - notes/q3-review.md#vendor-performance
hebbs-confidence: 0.82
hebbs-created: 2026-03-13T20:00:00Z
---

The initial positive vendor assessment was based on a single
project. By Q3, three missed deadlines revealed a pattern.
```

**Filename**: slugified from first ~50 chars of insight content, with ULID prefix for uniqueness (e.g., `01JABC-vendor-assessment-pattern.md`).

**The loop**: watcher picks up the new insight file, indexes it like any other file. Insight participates in future recalls, primes, and reflects. User can edit or delete the insight file.

**Testable**: trigger reflect on a vault with enough memories, verify insight .md files appear, verify they get indexed by the watcher.

---

## Milestone 7: Rebuild Guarantee

`hebbs rebuild <vault-path>`: delete `.hebbs/` and re-create from scratch.

This is the acid test of the architecture. If rebuild produces a functionally equivalent index (same memories, same edges, same recall results), the files are truly the source of truth.

**What is preserved**: all content, all metadata from frontmatter, all wiki-link edges, all tags, all insight files (they're just .md files in the vault).

**What is lost on rebuild**: decay scores, access counts, reinforcement signals. These are cognition-plane-only state. Acceptable loss -- they rebuild naturally over time through usage.

**Testable**: index a vault, run some queries (to build up access counts), rebuild, verify recall results are equivalent (content and edges match, scores may differ).

---

## Out of Scope (for TASK-13)

- Contradiction detection (TASK-12 Feature 2, future task)
- Token-budgeted retrieval (TASK-12 Feature 3, future task)
- Obsidian plugin / UI integration
- Daemonization (systemd, launchd) -- `hebbs watch` runs in foreground
- Multi-vault support
- Encryption or access control on `.hebbs/`

---

## Open Decisions (resolve before implementation)

1. **Where does this code live?** New crate `hebbs-vault` (separate from `hebbs-cli`).
2. **Embedded by default, server mode optional.** `hebbs-vault` ships with `hebbs-core` directly -- same engine that `hebbs-server` wraps, minus the network layer. All functionality (remember, recall, prime, reflect, graph, HNSW, decay) works in-process. The vault daemon talks to an `Engine` trait with two backends:
   - **Embedded backend** (default): uses `hebbs-core` directly. Zero setup, offline-first, sub-millisecond queries. Single-user vaults.
   - **Remote backend** (opt-in via config): uses `hebbs-client` to talk to a running `hebbs-server` over gRPC. For multi-user/team scenarios where vaults are scoped to different users on a shared server.

   The vault daemon doesn't know or care which backend it's using. Build embedded first, add remote backend later.

   ```
   hebbs-server  = hebbs-core + gRPC/HTTP layer + multi-tenant management
   hebbs-vault   = hebbs-core + file watcher + markdown parser + Engine trait
   ```
3. **Frontmatter schema**: all `hebbs-*` prefixed fields are engine-reserved. User fields (anything without the prefix) pass through as context metadata. Reserved fields:
   - `hebbs-kind` -- type of engine-generated file: `insight`, `contradiction`, `summary`
   - `hebbs-sources` -- list of source file paths (with optional `#section` anchors) that produced this file
   - `hebbs-confidence` -- engine's confidence score (0.0-1.0)
   - `hebbs-created` -- ISO 8601 timestamp of when the engine generated the file
4. **File watcher library**: `notify` crate (Rust, cross-platform) is the obvious choice. Confirm it handles recursive watching and rename detection.

## Status

**Implemented.** See [PLAN-13](./plans/PLAN-13.md) for implementation details.

### What was built

- **`hebbs-vault` crate** (`hebbs/crates/hebbs-vault/`) with 8 modules: parser, manifest, config, ingest, watcher, query, insight_writer, error
- **CLI binary** (`hebbs-vault`) with 5 commands: `init`, `index`, `watch`, `rebuild`, `status`
- **45 unit tests** passing across all modules
- **E2E test suite** (`e2e-scenario-tests/`) with 10 scenario scripts, fixture generator, shared assertion/metrics libs

### How to build

```bash
cd hebbs
cargo build -p hebbs-vault --release
```

### How to use

```bash
# Initialize a vault
./target/release/hebbs-vault init --vault /path/to/your/notes

# Index all markdown files (two-phase: parse then embed)
./target/release/hebbs-vault index --vault /path/to/your/notes

# Watch for changes in real-time
./target/release/hebbs-vault watch --vault /path/to/your/notes

# Rebuild index from scratch
./target/release/hebbs-vault rebuild --vault /path/to/your/notes

# Check vault status
./target/release/hebbs-vault status --vault /path/to/your/notes
```

### How to test

```bash
# Unit tests
cargo test -p hebbs-vault

# E2E scenario tests (requires built binary + python3 + jq)
cd ../e2e-scenario-tests
./run_all.sh --binary ../hebbs/target/release/hebbs-vault
./run_all.sh --scenario 01    # run single scenario
./run_all.sh --verbose        # verbose output
```

### Open decisions resolved

1. **Where does this code live?** New crate `hebbs-vault` (separate binary, not in `hebbs-cli`).
2. **Embedded by default.** Uses `hebbs-core::engine::Engine` directly. No gRPC.
3. **Frontmatter schema:** `hebbs-*` prefixed fields are reserved. User fields pass through as context metadata.
4. **File watcher library:** `notify` v6 crate with recursive watching.
