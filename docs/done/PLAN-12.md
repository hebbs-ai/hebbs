# PLAN-12: Markdown/Obsidian Cognitive Layer

Parent: [TASK-12](../TASK-12-markdown-obsidian-cognitive-layer.md)
Detailed spec: [TASK-13](../done/TASK-13-file-first-markdown-sync.md) (Feature 1)

---

## Status

| Feature | Status | Notes |
|---------|--------|-------|
| 1. File-First Markdown Sync | **DONE** | TASK-13 complete. All 9 steps shipped. Crate at `hebbs-vault/`. E2e tests passing (scenarios 1-13). |
| 2. Full-Corpus Contradiction Detection | **DONE** | All 7 steps complete. See [PLAN-contradiction](./PLAN-contradiction.md). Heuristic + LLM classifiers, pipeline, file output, engine API, panel viz. 44 tests. |
| 3. Token-Budgeted prime() | **DEPRIORITIZED** | Agents handle truncation themselves. Revisit when tight context windows or Obsidian sidebar ships. |

---

## Scope

Three features, implemented in order:

1. **File-First Markdown Sync** (TASK-13) -- DONE. Files are truth, `.hebbs/` is rebuildable index.
2. **Full-Corpus Contradiction Detection** -- DONE. CONTRADICTS edges, entailment pipeline, file output.
3. **Token-Budgeted prime()** -- DEPRIORITIZED. `max_tokens` parameter, greedy packing.

---

## Feature 1: File-First Markdown Sync -- DONE

### New Crate: `hebbs-vault` -- DONE

Lives at `hebbs/crates/hebbs-vault/`. Depends on `hebbs-core`, `hebbs-embed`, `hebbs-index`, `hebbs-storage`. Does NOT depend on `hebbs-server` or `hebbs-client` (embedded-first).

```
hebbs-vault/
  src/
    lib.rs              -- public API: init, index, watch, rebuild
    parser.rs           -- markdown parsing (frontmatter, headings, wiki-links, tags)
    manifest.rs         -- .hebbs/manifest.json read/write, section state machine
    ingest.rs           -- two-phase pipeline (parse then embed)
    watcher.rs          -- file watcher daemon (notify crate)
    query.rs            -- file-backed recall/prime (read content from files)
    insight_writer.rs   -- write reflect output as .md files
    config.rs           -- .hebbs/config.toml schema and defaults
  Cargo.toml
```

**Engine trait**: `hebbs-vault` talks to `hebbs-core::Engine` directly (embedded backend). A `RemoteBackend` via `hebbs-client` is out of scope for now but the trait boundary makes it easy to add later.

---

### Step 1: Markdown Parser (`parser.rs`) -- DONE

**Goal**: Parse a single `.md` file into structured sections.

**Input**: file path (or raw bytes for testing)

**Output**: `ParsedFile` struct:
```rust
pub struct ParsedFile {
    pub frontmatter: Option<FrontmatterMap>,   // HashMap<String, serde_yaml::Value>
    pub sections: Vec<ParsedSection>,
}

pub struct ParsedSection {
    pub heading_path: Vec<String>,             // e.g., ["Design", "API"]
    pub heading_level: u8,                     // 0 = preamble (before first heading)
    pub content: String,                       // prose under the heading
    pub byte_start: usize,
    pub byte_end: usize,
    pub wiki_links: Vec<WikiLink>,             // [[target]] and [[target|alias]]
    pub tags: Vec<String>,                     // #tag
}

pub struct WikiLink {
    pub target: String,                        // file path or note name
    pub alias: Option<String>,                 // display text
    pub section: Option<String>,               // #section anchor
}
```

**Parsing rules**:
- Split on `##` headings (configurable: any heading level as split point)
- Files with no headings produce one section (the whole file body)
- Frontmatter is YAML between `---` delimiters at file start
- `hebbs-*` frontmatter fields are engine-reserved; everything else is user context metadata
- Wiki-links: regex `\[\[([^\]|]+)(?:\|([^\]]+))?\]\]`, extract target + optional alias + optional `#section`
- Tags: regex `(?:^|\s)#([a-zA-Z0-9_/-]+)` (avoid matching inside code blocks)
- Byte offsets are from file start (for later content reads without re-parsing)

**Dependencies**: `serde_yaml` for frontmatter, no external markdown AST library (heading-level splitting is simpler than full parsing; avoids heavy deps per Principle 2)

**Tests**:
- File with frontmatter + multiple headings: correct section count, content, byte offsets
- File with no headings: single section
- File with nested headings (`##`, `###`): correct heading_path nesting
- Wiki-links: `[[note]]`, `[[note|alias]]`, `[[note#section]]`
- Tags: `#tag`, `#nested/tag`, tag inside code block (should be ignored)
- Edge cases: empty file, frontmatter-only file, heading with no body

---

### Step 2: Manifest and Cognition Plane Layout (`manifest.rs`, `config.rs`) -- DONE

**Goal**: `.hebbs/` directory structure, manifest schema, init/read/write.

**`.hebbs/` layout**:
```
.hebbs/
  manifest.json       -- file-to-section mapping, checksums, states
  config.toml         -- chunking strategy, embedding model, ignore patterns
  index/              -- (populated by hebbs-core indexes in later steps)
```

**Manifest schema** (Rust structs, serde JSON):
```rust
pub struct Manifest {
    pub version: u32,                          // schema version (1)
    pub files: HashMap<String, FileEntry>,     // relative path -> entry
}

pub struct FileEntry {
    pub checksum: String,                      // "sha256:<hex>"
    pub last_parsed: DateTime<Utc>,
    pub last_embedded: Option<DateTime<Utc>>,
    pub sections: Vec<SectionEntry>,
}

pub struct SectionEntry {
    pub memory_id: String,                     // ULID
    pub heading_path: Vec<String>,
    pub byte_start: usize,
    pub byte_end: usize,
    pub state: SectionState,
    pub content_checksum: String,              // per-section content hash
}

pub enum SectionState {
    ContentStale,   // parsed, byte offsets current, embedding outdated
    Synced,         // embedding matches content
    Orphaned,       // section removed from file, pending forget()
}
```

**Config schema** (`config.toml`):
```toml
[chunking]
split_on = "##"            # heading level to split on
min_section_length = 50    # chars; sections shorter than this merge with parent

[embedding]
model = "bge-small-en-v1.5"
dimensions = 384

[watch]
ignore_patterns = [".hebbs/", ".git/", ".obsidian/", "node_modules/"]
phase1_debounce_ms = 500
phase2_debounce_ms = 3000

[output]
insight_dir = "insights/"
```

**CLI: `hebbs init <vault-path>`**:
1. Validate vault path exists
2. Create `.hebbs/` directory (error if already exists, unless `--force`)
3. Write default `config.toml`
4. Write empty `manifest.json` (`{"version": 1, "files": {}}`)
5. Add `.hebbs/` to vault's `.gitignore` if git repo detected

**Tests**:
- `hebbs init` creates correct directory structure
- Manifest round-trip: write then read, verify identical
- Config parsing: defaults, overrides, invalid values
- Double init without `--force` errors

---

### Step 3: Ingest Pipeline, Phase 1 (`ingest.rs`) -- DONE

**Goal**: Parse changed files, update manifest. Cheap, runs on every file change.

**Algorithm** for a single file:
1. Compute file checksum (SHA-256). If matches manifest entry, skip entirely.
2. Parse file (Step 1 parser)
3. Diff new sections against manifest sections (match by heading_path):
   - **Same heading_path, same content_checksum**: unchanged, keep existing memory_id, keep state
   - **Same heading_path, different content_checksum**: modified, keep memory_id, update byte offsets, mark `ContentStale`
   - **New heading_path**: assign new memory_id (ULID), mark `ContentStale`
   - **Missing heading_path**: mark existing section `Orphaned`
4. Update file checksum, `last_parsed` timestamp
5. Write manifest (incremental: write after each file for crash safety)

**Batch mode**: accept `Vec<PathBuf>`, process each file, write manifest once per file (not once at end).

**Tests**:
- New file: all sections get new memory_ids, all `ContentStale`
- Unmodified file: skipped entirely (checksum match)
- Modified file (content change in one section): only that section `ContentStale`, others unchanged
- Modified file (heading renamed): old section `Orphaned`, new section created
- Deleted file: all sections `Orphaned`
- Crash safety: kill mid-batch, re-run, verify consistent state

---

### Step 4: Ingest Pipeline, Phase 2 (`ingest.rs`) -- DONE

**Goal**: Embed content-stale sections, update indexes via `hebbs-core` engine.

**Algorithm**:
1. Collect all `ContentStale` sections across all files from manifest
2. For each, read content from file at byte offsets (NOT from stored memory content)
3. Batch embed via `hebbs-embed` (one call for N sections, respecting batch size limits)
4. For each section:
   - **New** (no existing memory in engine): `engine.remember()` with content, importance from frontmatter (default 0.5), context = frontmatter metadata + heading_path
   - **Modified** (existing memory_id): `engine.revise()` with new content + embedding
   - **Orphaned**: `engine.forget()` by memory_id
5. Resolve wiki-links: for each wiki-link in a section, look up target file/section in manifest, create `RELATED_TO` edge between memory_ids
6. Mark all processed sections as `Synced` in manifest
7. Write manifest

**Frontmatter-to-context mapping**:
- `hebbs-kind` in frontmatter maps to `MemoryKind` (insight, etc.)
- `hebbs-sources` maps to `INSIGHT_FROM` edges
- All other frontmatter keys become context metadata JSON

**Batch sizing**: configurable (default 50 sections per embed call). Prevents memory pressure on large vaults.

**Tests**:
- Ingest a vault, verify memories exist via `engine.recall()`
- Modify a file section, run phase 2, verify `revise()` updated the memory
- Delete a file, run phase 2, verify `forget()` removed memories
- Wiki-link between two files creates `RELATED_TO` edge
- Frontmatter metadata appears in recall results as context

---

### Step 5: File Watcher Daemon (`watcher.rs`) -- DONE

**Goal**: Real-time bridge between content plane and cognition plane.

**Dependencies**: `notify` crate (cross-platform file system events, recursive watching)

**Architecture**:
```
                   notify events
  filesystem -----> [event queue] ----> debounce logic
                                            |
                                   phase 1 timer (500ms)
                                            |
                                            v
                                    parse + manifest
                                            |
                                   phase 2 timer (3s idle)
                                            |
                                            v
                                    embed + index (background tokio task)
```

**Event handling**:
- `Create` / `Modify`: add file path to phase 1 pending set
- `Remove`: mark all file's sections as `Orphaned` in manifest, add to phase 2 pending
- `Rename`: treat as `Remove` old path + `Create` new path (no rename tracking, per TASK-13 decision)
- Filter: only `.md` files, ignore patterns from config

**Debounce logic**:
- Phase 1 debounce (default 500ms): after last event, run phase 1 for all pending files. Reset timer on new events.
- Phase 2 debounce (default 3s): after last phase 1 completion with no new events, run phase 2. Reset on new events.
- Adaptive burst detection: if >20 events arrive within a phase 1 window, extend phase 2 debounce to 10s (let the burst settle).

**Threading model**:
- Watcher thread: receives `notify` events, manages debounce timers, runs phase 1 (cheap, sync)
- Background tokio task: phase 2 (async, embedding + engine calls)
- Phase 2 is cancellable: if new phase 1 runs while phase 2 is in progress, phase 2 finishes its current batch but skips remaining sections (they'll be re-collected on next phase 2 run with updated content)

**CLI: `hebbs watch <vault-path>`**:
1. Verify `.hebbs/` exists (error if not initialized)
2. Load manifest and config
3. Run initial phase 1 + phase 2 for any files changed since last run (checksum diff)
4. Start file watcher
5. Foreground process, logs activity, Ctrl-C for graceful shutdown

**Tests**:
- Create file, verify phase 1 within 500ms, phase 2 within 3s
- Rapid-save 10 times in 5s: phase 1 runs ~10 times, phase 2 runs once
- Drop 50 files: batch processing (not 50 individual embed calls)
- Shutdown: verify manifest is saved, no partial state

---

### Step 6: Query from Files (`query.rs`) -- DONE

**Goal**: Recall and prime return content read from files, not from stored memory records.

**Change to recall/prime flow**:
1. Engine returns ranked results as normal (memory_ids, scores, strategy details)
2. For each result, look up file path + byte offsets in manifest
3. Read content from file at byte offsets
4. Replace memory record's content field with file content
5. If file missing or byte offsets invalid (modified but not re-indexed), fall back to stored content and add `stale: true` flag

**Implementation**: wrapper around `engine.recall()` / `engine.prime()` that intercepts results and populates content from files. The engine itself is unchanged.

**Why this matters**: user edits a file, the next recall returns the edited content immediately, even before phase 2 re-embeds it. Ranking might be slightly off (embedding is stale), but content is always fresh.

**Tests**:
- Recall a section, modify file (without re-indexing), recall again: content reflects edit
- Delete a file (without re-indexing), recall: falls back to stored content with stale flag
- Performance: file reads add <1ms per result (sequential reads, OS page cache)

---

### Step 7: Insight Output as Files (`insight_writer.rs`) -- DONE

**Goal**: When `reflect()` produces insights, write them as `.md` files into the vault.

**Output format**:
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

**Filename**: `{ulid_prefix}-{slugified-first-50-chars}.md`
Example: `01JABC-vendor-assessment-pattern.md`

**Source path resolution**: for each source memory_id in the insight, look up the manifest to find `file_path#heading_path` and write that as the `hebbs-sources` entry. Human-readable, clickable in Obsidian.

**The loop**:
1. `reflect()` produces insight (normal engine flow)
2. Insight writer intercepts the insight (via subscribe event or post-reflect hook)
3. Writes `.md` file to configured output directory (default `insights/`)
4. File watcher picks up the new file
5. Phase 1 parses it, phase 2 embeds it
6. Insight now participates in future recalls, primes, and reflects

**Edge case**: insight writer must NOT trigger infinite reflect loops. The config should have an option to exclude `insights/` directory from reflect input, or the reflect pipeline should skip memories with `hebbs-kind: insight` as source material for new insights (configurable).

**Tests**:
- Trigger reflect, verify `.md` file created with correct frontmatter
- Verify watcher indexes the insight file
- Verify insight appears in recall results
- Verify no infinite reflect loop

---

### Step 8: Rebuild Guarantee -- DONE

**Goal**: `hebbs rebuild <vault-path>` deletes `.hebbs/` and recreates from scratch.

**Algorithm**:
1. Delete `.hebbs/` entirely
2. Run `hebbs init <vault-path>`
3. Run `hebbs index <vault-path>` (full phase 1 + phase 2)

**What is preserved**: all content, frontmatter metadata, wiki-link edges, tags, insight files (they're just `.md` files).

**What is lost**: decay scores, access counts, reinforcement signals (cognition-plane-only state). Acceptable per TASK-13 spec.

**Equivalence test**: recall results after rebuild should return the same content and edges (scores may differ due to lost access counts).

**Tests**:
- Index a vault, run queries, rebuild, verify recall returns same content
- Verify insight files survive rebuild (they're in the vault, not `.hebbs/`)
- Verify edge consistency post-rebuild (wiki-links re-created)

---

### Step 9: CLI Integration -- DONE

Add commands to `hebbs-cli`:

| Command | Description |
|---------|-------------|
| `hebbs init <vault-path>` | Initialize `.hebbs/` in vault |
| `hebbs index <vault-path>` | Full re-index (phase 1 + phase 2) |
| `hebbs watch <vault-path>` | Start file watcher daemon (foreground) |
| `hebbs rebuild <vault-path>` | Delete `.hebbs/` and re-index from scratch |
| `hebbs status <vault-path>` | Show manifest stats (files, sections, states) |

All commands validate that vault-path exists and (except `init`) that `.hebbs/` is initialized.

---

## Feature 2: Full-Corpus Contradiction Detection -- DONE

Depends on: Feature 1 (file-backed memories, RELATED_TO edges, insight output) -- DONE

### Step 10: CONTRADICTS Edge Type -- PENDING

Add `Contradicts = 0x06` to `EdgeType` enum in `hebbs-index/src/graph.rs`. The graph layer is edge-type-agnostic internally, so this is a one-line addition plus proto update.

**Proto change**: add `CONTRADICTS = 6` to `EdgeType` enum in `hebbs.proto`.

**Cross-component propagation**: proto -> server -> CLI -> Rust client -> Python SDK -> TypeScript SDK -> FFI.

### Step 11: Contradiction Detection Pipeline -- PENDING

**New module** in `hebbs-reflect` (or new crate `hebbs-contradict` if complexity warrants separation).

**Pipeline** (runs as background worker, same scheduling model as reflect):

1. **Trigger**: new or modified memory (via subscribe event or post-ingest hook)
2. **Candidate selection** (cheap): query HNSW for top-K similar memories to the new memory (K=20, configurable). O(log n) per query. This is the "cheap first pass" from the spec.
3. **Temporal filter**: exclude memories from same file/section (self-contradiction is revision, not contradiction). Exclude memories with `REVISED_FROM` edges to each other (explicit evolution).
4. **Entailment classification** (expensive): for each candidate pair, call LLM with both memory contents and ask for entailment classification:
   - `ENTAILS`: consistent
   - `CONTRADICTS`: opposing claims about same subject
   - `NEUTRAL`: unrelated or tangential
   - `EVOLVES`: temporal evolution ("used to think X, now think Y")
5. **Edge creation**: for `CONTRADICTS` pairs, create bidirectional `CONTRADICTS` edges with confidence score
6. **Output** (configurable):
   - **Quiet mode** (default): edges only, surfaced via queries
   - **Explicit mode**: also write contradiction note as `.md` file (same pattern as insight files):

```markdown
---
hebbs-kind: contradiction
hebbs-sources:
  - notes/meeting-jan.md#vendor-evaluation
  - notes/q3-review.md#vendor-performance
hebbs-confidence: 0.91
hebbs-created: 2026-03-14T10:00:00Z
---

"Vendor X is reliable" (January) contradicts "Vendor X missed three
deadlines" (August). The January assessment was based on a single
project; the August observation covers three quarters of data.
```

**Incremental**: only new/modified memories trigger candidate search. No full N^2 scan. Amortized cost: O(log n) HNSW query + O(K) LLM calls per new memory.

**Heuristics for contradiction vs. evolution**:
- Same entity + high similarity + opposing sentiment = candidate contradiction
- Temporal markers ("now", "updated", "changed my mind", "no longer") = likely evolution/revision
- Existing `REVISED_FROM` edge = explicit revision, skip

### Step 12: Contradiction Surfacing in Queries -- PENDING

- `recall()` and `prime()` results include a `contradictions` field: for each returned memory, list any memories connected by `CONTRADICTS` edges
- Optional filter: `exclude_contradicted: bool` to remove memories that have unresolved contradictions
- Contradiction resolution: user deletes one of the source files, or edits to reconcile. The watcher picks up the change, `forget()` or `revise()` fires, contradiction edge is invalidated

---

## Feature 3: Token-Budgeted prime() -- DEPRIORITIZED

Depends on: Feature 1 (file-backed content reads) -- DONE, Feature 2 (contradiction edges for filtering) -- PENDING

### Step 13: Token Counting -- PENDING

Add token estimation to `hebbs-core` or `hebbs-vault`:
- Default: `content.len() / 4` (bytes-per-token approximation)
- Optional: exact tokenizer via `tiktoken-rs` crate (configurable per model)
- Token count cached per section in manifest (invalidated on content change)

### Step 14: Token-Budgeted Packing in prime() -- PENDING

**New parameter**: `max_tokens: Option<u64>` on `PrimeRequest` (proto + all SDKs).

**Algorithm**:
1. Run normal prime pipeline (temporal + similarity ranking)
2. Instead of slicing at `max_memories`, iterate ranked results and greedily pack:
   ```
   budget_remaining = max_tokens
   for result in ranked_results:
       tokens = estimate_tokens(result.content)
       if tokens <= budget_remaining:
           include(result)
           budget_remaining -= tokens
       else:
           // skip (or truncate if it's the first/only result)
   ```
3. Return packed results with metadata: `total_tokens_used`, `total_candidates_considered`, `candidates_excluded_by_budget`

**Interaction with insights**: when budget is tight, consolidated insights are denser (more info per token). Add `prefer_insights: bool` parameter (default false). When true, insights are scored with a 1.5x multiplier before packing, making them more likely to be included.

**Interaction with contradictions**: if `exclude_contradicted` is set, contradicted memories are removed from candidates before packing.

### Step 15: Proto and SDK Propagation -- PENDING

- Add `max_tokens`, `prefer_insights`, `total_tokens_used` to proto
- Propagate to REST API, Rust client, Python SDK, TypeScript SDK, CLI

---

## Dependency Graph

```
Step 1  (parser)              ─┐
Step 2  (manifest + config)   ─┼─> Step 3 (phase 1) ──> Step 4 (phase 2) ─┐
                               │                                            │
                               └─> Step 5 (watcher, needs 3+4) ───────────┤
                                                                           │
Step 6  (query from files, needs 2+4) ────────────────────────────────────┤
Step 7  (insight output, needs 4+5) ──────────────────────────────────────┤
                                                                           │
Step 8  (rebuild, needs 2+3+4) ───────────────────────────────────────────┤
Step 9  (CLI, needs all above) ───────────────────────────────────────────┘
                                   ^^^ ALL DONE (Feature 1) ^^^

Step 10 (CONTRADICTS edge) ──> Step 11 (detection pipeline) ──> Step 12 ──┐
                                   ^^^ PENDING (Feature 2) ^^^             │
                                                                           │
Step 13 (token counting) ──> Step 14 (budgeted packing) ──> Step 15 ──────┘
                                   ^^^ PENDING (Feature 3) ^^^
```

**Features 2 and 3 are independent of each other.** Feature 3 can optionally use contradiction edges for filtering (`exclude_contradicted`), but the core token-budgeted packing works without it. Both can be built in parallel if desired.

---

## New Dependencies (requires `cargo audit` validation)

| Crate | Purpose | Justification |
|-------|---------|---------------|
| `notify` | Cross-platform file watcher | Only mature Rust file-watching library; recursive, debounce-capable |
| `serde_yaml` | YAML frontmatter parsing | Standard YAML parser; frontmatter is YAML by Obsidian convention |
| `toml` | Config file parsing | Already in ecosystem (Cargo.toml uses it); lightweight |
| `sha2` | File checksums | Standard, audited SHA-256 implementation |
| `slug` | Insight filename generation | Tiny crate for slugifying strings |

`tiktoken-rs` is optional (Feature 3, exact token counting). Default approximation needs no new deps.

---

## Principles Compliance

| Principle | How this plan complies |
|-----------|----------------------|
| P1 (Hot Path) | File reads add <1ms per result (OS page cache). Embedding is background (phase 2). No network calls on query path. |
| P2 (Single Binary) | `hebbs-vault` embeds `hebbs-core` directly. No external processes. |
| P3 (Cognition) | Contradiction detection, insight generation, decay all preserved. Files are just the content plane. |
| P4 (Bounded) | Batch sizes configurable. Debounce timers bounded. Manifest writes incremental. |
| P5 (Background) | Phase 2 embedding and contradiction detection run in background. Queries never wait. |
| P6 (Lineage) | Insight files track `hebbs-sources`. CONTRADICTS edges are bidirectional with confidence. |
| P9 (Measure) | Add metrics: `phase1_latency_ms`, `phase2_batch_size`, `phase2_latency_ms`, `watcher_events_per_sec`, `manifest_files_count`. |
| P10 (API Elegance) | No new operations. `init`, `index`, `watch`, `rebuild` are CLI commands, not API surface. Engine API unchanged. |
| P11 (Correctness) | Manifest written incrementally for crash safety. Rebuild guarantee validates architecture. |
| P12 (Security) | `.hebbs/` is gitignored. No secrets in manifest. Input validation on frontmatter parsing. |

---

## Estimated Effort (relative sizing, not time estimates)

| Step | Size | Status | Notes |
|------|------|--------|-------|
| 1. Parser | Medium | DONE | Regex-based, no AST library, thorough edge-case testing needed |
| 2. Manifest + config | Small | DONE | Data structures + serde, `hebbs init` CLI |
| 3. Phase 1 ingest | Medium | DONE | Section diffing logic, incremental manifest writes |
| 4. Phase 2 ingest | Large | DONE | Engine integration (remember/revise/forget), batch embedding, wiki-link edge resolution |
| 5. Watcher | Large | DONE | Debounce logic, adaptive burst detection, cancellable phase 2, integration testing |
| 6. Query from files | Small | DONE | Wrapper around engine recall/prime, byte-offset reads |
| 7. Insight writer | Medium | DONE | Reflect hook, source path resolution, filename slugification, loop prevention |
| 8. Rebuild | Small | DONE | Orchestration of init + index, equivalence testing |
| 9. CLI | Small | DONE | Command routing, argument parsing |
| 10. CONTRADICTS edge | Small | DEPRIORITIZED | One enum variant + proto update + SDK propagation |
| 11. Detection pipeline | Large | DEPRIORITIZED | LLM entailment classification, background worker, heuristics |
| 12. Contradiction surfacing | Medium | DEPRIORITIZED | Query changes, filtering, resolution flow |
| 13. Token counting | Small | DEPRIORITIZED | Estimation function, optional tiktoken integration |
| 14. Budgeted packing | Medium | DEPRIORITIZED | Greedy algorithm, insight preference, proto changes |
| 15. SDK propagation | Small | DEPRIORITIZED | Mechanical proto -> SDK updates |
