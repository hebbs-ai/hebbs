# PLAN-13: File-First Markdown Sync

Parent task: [TASK-13](../TASK-13-file-first-markdown-sync.md)
Parent feature: [TASK-12](../TASK-12-markdown-obsidian-cognitive-layer.md)

**Status: COMPLETE** (2026-03-14)

---

## Implementation Summary

All 10 steps implemented. 45 unit tests passing. E2E test infrastructure in place.

| Step | Module | Status |
|------|--------|--------|
| 1. Crate scaffold | `Cargo.toml`, `error.rs` | Done |
| 2. Markdown parser | `parser.rs` (12 tests) | Done |
| 3. Manifest + config | `manifest.rs` (5 tests), `config.rs` (4 tests) | Done |
| 4. Phase 1 ingest | `ingest.rs` (6 tests) | Done |
| 5. Phase 2 ingest | `ingest.rs` | Done |
| 6. File watcher | `watcher.rs` | Done |
| 7. Query from files | `query.rs` (2 tests) | Done |
| 8. Insight writer | `insight_writer.rs` (5 tests) | Done |
| 9. Rebuild guarantee | `lib.rs` (7 tests) | Done |
| 10. CLI integration | `bin/hebbs_vault.rs` | Done |
| E2E test suite | `e2e-scenario-tests/` (10 scenarios) | Done |

### Build and Run

```bash
# Build
cd hebbs && cargo build -p hebbs-vault --release

# Unit tests
cargo test -p hebbs-vault

# E2E tests
cd ../e2e-scenario-tests
./run_all.sh --binary ../hebbs/target/release/hebbs-vault
```

---

## Scope

Build the content plane / cognition plane architecture. Markdown files are the source of truth; `.hebbs/` is a rebuildable index. The foundational loop: **files -> index -> queries -> new files (insights) -> index**.

No contradiction detection. No token budgeting. Just the sync.

---

## New Crate: `hebbs-vault`

Lives at `hebbs/crates/hebbs-vault/`. Depends on `hebbs-core`, `hebbs-embed`, `hebbs-index`, `hebbs-storage`. Does NOT depend on `hebbs-server` or `hebbs-client` (embedded-first, per TASK-13 open decision #2).

```
hebbs-vault/
  src/
    lib.rs              -- public API: init, index, watch, rebuild, status
    parser.rs           -- markdown parsing
    manifest.rs         -- .hebbs/manifest.json read/write, section state machine
    ingest.rs           -- two-phase pipeline (parse then embed)
    watcher.rs          -- file watcher daemon
    query.rs            -- file-backed recall/prime
    insight_writer.rs   -- write reflect output as .md files
    config.rs           -- .hebbs/config.toml schema and defaults
    error.rs            -- thiserror error types (library crate)
  Cargo.toml
```

`hebbs-vault` talks to `hebbs-core::Engine` directly (embedded backend). The trait boundary makes adding a `RemoteBackend` via `hebbs-client` straightforward later.

### New Dependencies

| Crate | Purpose | Justification |
|-------|---------|---------------|
| `notify` (~6.x) | Cross-platform file watcher | Only mature Rust file-watching library; recursive, debounce-capable |
| `serde_yaml` | YAML frontmatter parsing | Standard YAML parser; frontmatter is YAML by Obsidian/markdown convention |
| `toml` | Config file parsing | Already in ecosystem (Cargo.toml uses it); lightweight |
| `sha2` | File + section checksums | Standard, audited SHA-256; used for change detection |
| `slug` | Insight filename generation | Tiny, no transitive deps, converts prose to file-safe slugs |

All must pass `cargo audit` before merging.

---

## Step 1: Crate Scaffold + Data Types

**Goal**: Create `hebbs-vault` crate with all shared data types, error types, and the `Cargo.toml` wiring. No logic yet.

**Work**:
1. Create `hebbs/crates/hebbs-vault/Cargo.toml` with dependencies on `hebbs-core`, `hebbs-embed`, `hebbs-storage`, `serde`, `serde_json`, `serde_yaml`, `sha2`, `toml`, `chrono`, `ulid`, `thiserror`, `tracing`
2. Add `hebbs-vault` to workspace `Cargo.toml` members
3. Define core data types in `lib.rs` / submodules:

```rust
// parser.rs types
pub struct ParsedFile {
    pub frontmatter: Option<HashMap<String, serde_yaml::Value>>,
    pub sections: Vec<ParsedSection>,
}

pub struct ParsedSection {
    pub heading_path: Vec<String>,       // ["Design", "API"] for ## Design > ### API
    pub heading_level: u8,               // 0 = preamble (before first heading)
    pub content: String,                 // prose under the heading
    pub byte_start: usize,              // offset from file start
    pub byte_end: usize,
    pub wiki_links: Vec<WikiLink>,
    pub tags: Vec<String>,
}

pub struct WikiLink {
    pub target: String,                  // file path or note name
    pub alias: Option<String>,           // [[target|alias]]
    pub section: Option<String>,         // [[target#section]]
}

// manifest.rs types
pub struct Manifest {
    pub version: u32,                    // 1
    pub files: HashMap<String, FileEntry>,  // relative path -> entry
}

pub struct FileEntry {
    pub checksum: String,                // "sha256:<hex>"
    pub last_parsed: DateTime<Utc>,
    pub last_embedded: Option<DateTime<Utc>>,
    pub sections: Vec<SectionEntry>,
}

pub struct SectionEntry {
    pub memory_id: String,               // ULID
    pub heading_path: Vec<String>,
    pub byte_start: usize,
    pub byte_end: usize,
    pub state: SectionState,
    pub content_checksum: String,        // per-section SHA-256
}

pub enum SectionState {
    ContentStale,  // parsed + byte offsets current, embedding outdated
    Synced,        // embedding matches content
    Orphaned,      // heading removed from file, pending forget()
}

// config.rs types
pub struct VaultConfig {
    pub chunking: ChunkingConfig,
    pub embedding: EmbeddingConfig,
    pub watch: WatchConfig,
    pub output: OutputConfig,
}

pub struct ChunkingConfig {
    pub split_on: String,                // "##" default
    pub min_section_length: usize,       // 50 chars default
}

pub struct WatchConfig {
    pub ignore_patterns: Vec<String>,
    pub phase1_debounce_ms: u64,         // 500 default
    pub phase2_debounce_ms: u64,         // 3000 default
}

pub struct OutputConfig {
    pub insight_dir: String,             // "insights/" default
}
```

4. Define `VaultError` in `error.rs` using `thiserror`:
   - `Io(#[from] std::io::Error)`
   - `Parse { path: PathBuf, reason: String }`
   - `Manifest { reason: String }`
   - `Config { reason: String }`
   - `Engine(#[from] hebbs_core::Error)`
   - `NotInitialized { path: PathBuf }`
   - `AlreadyInitialized { path: PathBuf }`

**Tests**: `cargo check` passes, all types derive `Debug, Clone, Serialize, Deserialize` where appropriate.

---

## Step 2: Markdown Parser (`parser.rs`)

**Goal**: Parse a single `.md` file into `ParsedFile`. Pure function, no side effects.

**Input**: `&[u8]` (file bytes) or `&str`

**Algorithm**:
1. **Frontmatter extraction**: if file starts with `---\n`, find closing `---\n`, parse YAML between them via `serde_yaml`. Byte offset after frontmatter becomes body start.
2. **Section splitting**: scan body for lines matching `^#{1,6}\s+(.+)$`. The configured `split_on` level (default `##` = level 2) determines which headings create new sections. Headings at deeper levels nest under their parent.
3. **Heading path construction**: maintain a stack. When a level-2 heading appears, push it and clear deeper levels. When a level-3 appears, push under the current level-2. Result: `["Design", "API"]` for `## Design` > `### API`.
4. **Preamble handling**: content before the first heading (after frontmatter) becomes section 0 with `heading_level: 0` and empty `heading_path`.
5. **Wiki-link extraction** per section: regex `\[\[([^\]#|]+)(?:#([^\]|]+))?(?:\|([^\]]+))?\]\]`
   - Group 1: target (required)
   - Group 2: section anchor (optional)
   - Group 3: alias (optional)
   - Skip matches inside fenced code blocks (track ``` state)
6. **Tag extraction** per section: regex `(?:^|[\s(])#([a-zA-Z][a-zA-Z0-9_/-]*)`
   - Must start with letter (avoids `#123` issue numbers)
   - Skip matches inside fenced code blocks
7. **Byte offsets**: track start/end for each section relative to file start (not body start). This allows reading section content directly from the file without re-parsing.

**Edge cases**:
- Empty file: returns `ParsedFile { frontmatter: None, sections: [] }`
- Frontmatter-only file (no body after closing `---`): same, empty sections
- No headings: single section with full body, `heading_level: 0`
- Heading with no body: section with empty content string, valid byte range (start == end)
- Nested code blocks with `#` inside: must not split on headings inside fenced code
- YAML frontmatter parse failure: log warning, treat as no frontmatter (don't reject the file)
- Non-UTF-8 file: return error (`.md` files must be valid UTF-8)

**Tests** (property-based where possible):
- Roundtrip: for any `ParsedFile`, concatenating all section byte ranges reproduces the original body
- Frontmatter + 3 headings: correct section count (4 including preamble if it exists)
- Nested headings: `## A` > `### B` > `## C` produces paths `["A"]`, `["A", "B"]`, `["C"]`
- Wiki-links: `[[note]]`, `[[note|alias]]`, `[[note#section]]`, `[[note#section|alias]]`
- Tags: `#design`, `#api/v2`, tag-in-code-block ignored
- No headings: single section
- Empty file: no sections
- Code block with `## fake heading`: not treated as heading

---

## Step 3: Manifest + Config + `hebbs init` (`manifest.rs`, `config.rs`)

**Goal**: `.hebbs/` directory management, manifest CRUD, config read/write, `init` command.

### Manifest operations

```rust
impl Manifest {
    pub fn load(hebbs_dir: &Path) -> Result<Self, VaultError>;
    pub fn save(&self, hebbs_dir: &Path) -> Result<(), VaultError>;
    pub fn save_atomic(&self, hebbs_dir: &Path) -> Result<(), VaultError>;
}
```

- `save_atomic`: write to `.hebbs/manifest.json.tmp`, then `rename()` over the real file. Crash between write and rename = old manifest survives (stale but consistent). Crash after rename = new manifest.
- File-level locking: `flock()` on `.hebbs/manifest.lock` during write. Prevents concurrent `index` and `watch` from corrupting the manifest.

### Config operations

```rust
impl VaultConfig {
    pub fn load(hebbs_dir: &Path) -> Result<Self, VaultError>;
    pub fn default() -> Self;
    pub fn save(&self, hebbs_dir: &Path) -> Result<(), VaultError>;
}
```

Defaults:
```toml
[chunking]
split_on = "##"
min_section_length = 50

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

### `hebbs init <vault-path>`

1. Validate vault path exists and is a directory
2. Check `.hebbs/` does not exist (error unless `--force`)
3. Create `.hebbs/`, `.hebbs/index/`
4. Write default `config.toml`
5. Write empty `manifest.json` (`{"version": 1, "files": {}}`)
6. If `.gitignore` exists in vault root, append `.hebbs/` if not already present. If no `.gitignore` but `.git/` exists, create `.gitignore` with `.hebbs/`

**Tests**:
- Init creates correct structure, files are valid JSON/TOML
- Manifest round-trip: save then load, assert equality
- Config round-trip: save then load, assert equality
- Double init errors without `--force`
- Double init with `--force` overwrites
- Atomic save: corrupt `.tmp` file doesn't break existing manifest
- `.gitignore` handling: appends, doesn't duplicate, creates if needed

---

## Step 4: Ingest Phase 1 (Parse + Manifest Update)

**Goal**: For changed files, parse into sections, diff against manifest, update manifest. Cheap, runs on every file change.

### Algorithm: `phase1_ingest(paths: &[PathBuf], vault_root: &Path, manifest: &mut Manifest, config: &VaultConfig)`

For each file path:
1. Compute file SHA-256 checksum
2. Look up in manifest by relative path (relative to vault root)
3. If checksum matches: skip entirely (file unchanged)
4. Read file bytes, parse via Step 2 parser
5. Diff new sections against existing manifest sections:

**Section matching** is by `heading_path` (not by position/index):
- **Match found, same `content_checksum`**: unchanged. Keep `memory_id`, keep `state`, update `byte_start`/`byte_end` (they may shift if earlier content changed)
- **Match found, different `content_checksum`**: modified. Keep `memory_id`, update byte offsets, update `content_checksum`, set `state = ContentStale`
- **No match in manifest** (new heading): generate new ULID `memory_id`, set `state = ContentStale`
- **In manifest but not in parsed output** (heading removed): set `state = Orphaned`

6. Update `FileEntry`: new checksum, `last_parsed = Utc::now()`, new sections list
7. Save manifest (atomic write)

### Handling deleted files

`phase1_delete(path: &Path, manifest: &mut Manifest)`:
1. Look up file in manifest
2. Mark all sections `Orphaned`
3. Save manifest

### Batch mode

`phase1_ingest` accepts a slice of paths. Processes each file, writes manifest once per file (not once at end) for crash safety. If process dies at file 300/500, files 1-299 are in the manifest and won't be re-parsed on restart.

**Complexity**: O(S) per file where S = number of sections. Heading-path matching is O(S_old * S_new) but both are bounded (a file rarely has >50 headings), so effectively O(1) per file.

**Tests**:
- New file: all sections get new `memory_id`s, all `ContentStale`
- Unchanged file: skipped (checksum match), manifest untouched
- Modified section: same `memory_id`, `ContentStale`, updated byte offsets
- Added heading: new section appears
- Removed heading: old section marked `Orphaned`
- Reordered headings (same content): byte offsets update, checksums unchanged, state preserved
- Deleted file: all sections `Orphaned`
- Crash mid-batch: re-run processes remaining files, already-processed files skipped

---

## Step 5: Ingest Phase 2 (Embed + Index)

**Goal**: For all `ContentStale` and `Orphaned` sections, embed content and push to the engine via `remember()`/`revise()`/`forget()`. Expensive, batched.

### Algorithm: `async phase2_ingest(vault_root: &Path, manifest: &mut Manifest, engine: &Engine, embedder: &Embedder)`

1. **Collect work**: scan manifest for all sections with `state != Synced`:
   - `ContentStale` where `memory_id` has no existing memory in engine -> NEW
   - `ContentStale` where `memory_id` exists in engine -> MODIFIED
   - `Orphaned` -> DELETE

2. **Read content from files**: for each NEW/MODIFIED section, read bytes from file at `[byte_start..byte_end]`. If file read fails (deleted between phase 1 and phase 2), mark `Orphaned` instead.

3. **Batch embed**: collect all NEW/MODIFIED section texts, call `embedder.embed_batch()`. Batch size from config (default 50). Multiple batches if needed. O(N * D) where N = sections, D = embedding dimensions.

4. **Engine operations** (within a single operation batch where possible):
   - **NEW**: `engine.remember()` with:
     - `content`: section text
     - `embedding`: from step 3
     - `importance`: from frontmatter `hebbs-importance` field (default 0.5)
     - `context`: JSON of { `file_path`, `heading_path`, non-hebbs frontmatter fields }
     - `entity_id`: from frontmatter `hebbs-entity` field (optional)
     - `kind`: from frontmatter `hebbs-kind` field (default `Episode`)
   - **MODIFIED**: `engine.revise()` with same fields, using existing `memory_id`
   - **DELETE**: `engine.forget()` by `memory_id`

5. **Wiki-link edge resolution**: for each section with wiki-links:
   - Resolve wiki-link target to a file in the manifest (by filename match or relative path)
   - If target file + optional section anchor resolves to a `memory_id`, create `RELATED_TO` edge between source section's `memory_id` and target's `memory_id`
   - Unresolvable links (target file not indexed yet): store in a pending-links list, re-attempt on next phase 2 run

6. **Frontmatter-to-engine mapping for engine-generated files**:
   - `hebbs-kind: insight` -> `MemoryKind::Insight`
   - `hebbs-sources` -> create `INSIGHT_FROM` edges to each source memory
   - `hebbs-confidence` -> stored in context metadata

7. **Update manifest**: mark all processed sections as `Synced`, update `last_embedded` timestamp. Save atomically.

### Error handling

- Individual section failures (embedding error, engine error) log the error and skip the section. It remains `ContentStale` and will be retried on next phase 2 run.
- Manifest is saved after each batch (not after all batches), so progress is preserved on crash.

### Metrics (Principle 9)

- `vault_phase2_sections_total` (counter): sections processed per run
- `vault_phase2_embed_latency_ms` (histogram): per-batch embedding time
- `vault_phase2_engine_op_latency_ms` (histogram): per-operation engine call time
- `vault_phase2_batch_size` (gauge): sections per batch

**Tests**:
- Ingest vault with 3 files, 10 sections total: 10 memories exist in engine, all `Synced`
- Modify one section, phase 2: `revise()` called, memory content updated
- Delete a file (sections `Orphaned`), phase 2: `forget()` called, memories removed
- Wiki-link `[[other-note]]`: `RELATED_TO` edge exists between memories
- Wiki-link to non-existent file: no edge created, no error
- Frontmatter `hebbs-kind: insight` + `hebbs-sources`: correct `MemoryKind` and `INSIGHT_FROM` edges
- Batch size boundary: 51 sections with batch size 50 = 2 embed calls
- Section embed failure: section stays `ContentStale`, others still processed

---

## Step 6: File Watcher Daemon (`watcher.rs`)

**Goal**: Real-time bridge. Watch vault, debounce, run phase 1 + phase 2.

### Dependencies

`notify` crate (v6+) for cross-platform file system events with recursive watching.

### Architecture

```
  FS events (notify)
       |
       v
  [event receiver] --filter--> [pending_files: HashSet<PathBuf>]
       |                                    |
       |                          phase1 debounce timer (500ms)
       |                                    |
       |                                    v
       |                          phase1_ingest(pending_files)
       |                                    |
       |                          phase2 debounce timer (3s idle)
       |                                    |
       |                                    v
       |                          phase2_ingest (tokio::spawn)
       |                                    |
       └────── new events reset timers ─────┘
```

### Event processing

1. **Filter**: only `.md` files. Skip paths matching `config.watch.ignore_patterns` (glob matching). Skip files inside `.hebbs/`.
2. **Event mapping**:
   - `EventKind::Create` / `EventKind::Modify` -> add to `pending_creates_or_modifies`
   - `EventKind::Remove` -> add to `pending_deletes`
   - `EventKind::Rename(from, to)` -> add `from` to `pending_deletes`, add `to` to `pending_creates_or_modifies`
3. **Debounce**: implemented with `tokio::time::sleep` + select:
   - Every new event resets the phase 1 timer to 500ms from now
   - When phase 1 timer fires: drain `pending_*` sets, run `phase1_ingest` + `phase1_delete`
   - After phase 1 completes, start phase 2 timer (3s). New events during this window reset the phase 2 timer.
   - When phase 2 timer fires: spawn `phase2_ingest` as a background tokio task

### Adaptive burst detection

If >20 events arrive within a single phase 1 debounce window:
- Extend phase 2 debounce to `max(config.phase2_debounce_ms, 10_000)` for this cycle
- Log: `"burst detected ({n} events), extending phase 2 debounce to 10s"`
- Resets to normal after the burst settles

### Phase 2 cancellation

Phase 2 runs in a spawned task. If new phase 1 activity occurs while phase 2 is running:
- Phase 2 is NOT cancelled mid-batch (that would leave partial state)
- Phase 2 checks a `CancellationToken` between batches
- If cancelled, it saves manifest progress and exits. Remaining `ContentStale` sections are picked up on the next phase 2 run.

### Startup catch-up

On `hebbs watch` startup:
1. Load manifest
2. Walk vault directory, collect all `.md` files
3. Compare file checksums against manifest
4. Run phase 1 + phase 2 for any changed files (catch up since last run)
5. Then start the watcher loop

### Graceful shutdown

On SIGINT/SIGTERM:
1. Stop accepting new events
2. If phase 2 is running, let current batch finish (bounded by batch size)
3. Save manifest
4. Drop engine cleanly (flush RocksDB)

### CLI: `hebbs watch <vault-path>`

- Verify `.hebbs/` exists (error with "run `hebbs init` first")
- Load config + manifest
- Run catch-up
- Start watcher (foreground, Ctrl-C to stop)
- Log format: `[phase1] parsed 3 files (12 sections)`, `[phase2] embedded 8 sections in 1.2s`

### Metrics

- `vault_watcher_events_total` (counter, labeled by event type)
- `vault_watcher_phase1_runs_total` (counter)
- `vault_watcher_phase2_runs_total` (counter)
- `vault_watcher_burst_detections_total` (counter)
- `vault_watcher_pending_files` (gauge)

**Tests**:
- Create file while watching: appears in manifest within phase1_debounce + phase2_debounce
- Rapid-save 10 times in 5s: phase 2 runs once (check embed call count)
- Drop 50 files: single phase 2 batch (not 50 individual runs)
- Delete file while watching: sections orphaned, memories forgotten
- Rename file: old path forgotten, new path remembered
- Shutdown during phase 2: manifest saved, no corruption
- Startup catch-up: files changed while watcher was offline get indexed

---

## Step 7: Query from Files (`query.rs`)

**Goal**: Recall and prime return content read from the actual file, not from the engine's stored copy.

### Design

A thin wrapper around the engine's recall/prime that intercepts results and replaces content:

```rust
pub struct VaultQuery<'a> {
    engine: &'a Engine,
    manifest: &'a Manifest,
    vault_root: &'a Path,
}

impl VaultQuery<'_> {
    pub async fn recall(&self, request: RecallRequest) -> Result<Vec<RecallResult>, VaultError>;
    pub async fn prime(&self, request: PrimeRequest) -> Result<PrimeOutput, VaultError>;
}
```

### Algorithm (same for recall and prime)

1. Call `engine.recall()` / `engine.prime()` as normal
2. For each result:
   a. Look up `memory_id` in manifest (reverse lookup: scan files/sections for matching `memory_id`). Build a `HashMap<String, (PathBuf, usize, usize)>` (memory_id -> file_path + byte offsets) at `VaultQuery` construction for O(1) lookups.
   b. If found: read file at `[byte_start..byte_end]`, replace `result.content` with file content
   c. If file doesn't exist or read fails: keep engine's stored content, add `stale: true` to result metadata
   d. If byte offsets are out of bounds (file changed but not re-indexed): read from file start to min(byte_end, file_len), or fall back to stored content with `stale: true`
3. Return modified results

### Performance

- File reads: O(1) per result via `pread()` / `seek()` (no need to read entire file)
- OS page cache: hot files (recently accessed/edited) will be cached
- Added latency: <1ms per result on warm cache, <5ms cold (SSD)
- Memory ID reverse-lookup map: built once, O(N) where N = total sections across all files. For 10K files with avg 5 sections = 50K entries, ~4MB memory. Rebuilt on manifest reload.

**Tests**:
- Recall a section, get correct file content
- Edit file (without re-indexing), recall same section: content reflects edit
- Delete file (without re-indexing), recall: falls back to stored content, `stale: true`
- Corrupt byte offsets: graceful fallback
- Performance: recall 20 results with file reads < 5ms total

---

## Step 8: Insight Output as Files (`insight_writer.rs`)

**Goal**: Intercept reflect output, write insights as `.md` files into the vault.

### Hook into reflect

The engine's `reflect()` returns `Vec<InsightResult>`. The insight writer is called after a successful reflect:

```rust
pub struct InsightWriter<'a> {
    vault_root: &'a Path,
    manifest: &'a Manifest,
    config: &'a VaultConfig,
}

impl InsightWriter<'_> {
    pub fn write_insights(&self, insights: &[InsightResult]) -> Result<Vec<PathBuf>, VaultError>;
}
```

### Per-insight file generation

1. **Source resolution**: for each `source_memory_id` in the insight, look up in manifest to get `file_path#heading` (human-readable). If memory_id not in manifest (engine-only memory), use raw memory_id as fallback.

2. **Frontmatter**:
```yaml
---
hebbs-kind: insight
hebbs-sources:
  - notes/meeting-jan.md#vendor-evaluation
  - notes/q3-review.md#vendor-performance
hebbs-confidence: 0.82
hebbs-created: 2026-03-14T10:30:00Z
---
```

3. **Filename**: `{ulid_8char}-{slug_max50}.md`
   - ULID prefix (first 8 chars) for uniqueness + sortability
   - Slug from first ~50 chars of insight content
   - Example: `01JABCDE-vendor-assessment-revealed-pattern.md`

4. **Write path**: `{vault_root}/{config.output.insight_dir}/{filename}`
   - Create insight dir if it doesn't exist
   - Write atomically (write to `.tmp`, rename)

### The loop

1. `reflect()` produces insights
2. `InsightWriter.write_insights()` writes `.md` files
3. File watcher detects new files in insight dir
4. Phase 1 parses them (frontmatter -> metadata, `hebbs-kind: insight` recognized)
5. Phase 2 embeds and indexes them (`MemoryKind::Insight`, `INSIGHT_FROM` edges from `hebbs-sources`)
6. Insights now participate in future recalls, primes, and reflects

### Infinite loop prevention

Risk: insight file gets indexed -> next reflect includes it -> produces similar insight -> infinite cycle.

Mitigation (configurable, default both enabled):
- **Option A**: `config.output.exclude_insight_dir_from_reflect = true` (default). The reflect pipeline's memory selection excludes memories whose context contains `file_path` matching the insight dir pattern.
- **Option B**: The reflect pipeline already deduplicates: it checks proposed insights against existing insights in the validation stage. If the new insight is too similar to an existing one (cosine similarity > 0.95), it's rejected.

Both are defense-in-depth. Option A is the primary guard; Option B is the existing engine behavior.

### Metrics

- `vault_insights_written_total` (counter)
- `vault_insight_write_latency_ms` (histogram)

**Tests**:
- Trigger reflect with sufficient memories: `.md` file created in insight dir
- File has correct frontmatter (kind, sources, confidence, created)
- Sources are human-readable file paths (not raw memory_ids)
- Watcher picks up the file and indexes it
- Insight appears in recall results
- Reflect with insight-dir exclusion: no infinite loop after 3 reflect cycles

---

## Step 9: Rebuild Guarantee

**Goal**: `hebbs rebuild` deletes `.hebbs/` and recreates a functionally equivalent index.

### Algorithm

```rust
pub async fn rebuild(vault_root: &Path) -> Result<(), VaultError> {
    let config = VaultConfig::load(&vault_root.join(".hebbs"))?;  // save config before delete
    std::fs::remove_dir_all(vault_root.join(".hebbs"))?;
    init(vault_root, /* force */ false)?;
    // restore user's config (not the defaults)
    config.save(&vault_root.join(".hebbs"))?;
    index(vault_root).await?;
    Ok(())
}
```

Key: user's `config.toml` is preserved (loaded before delete, restored after init).

### What is preserved (lives in content plane)

- All `.md` file content
- All frontmatter metadata
- All insight files (they're in `insights/`, not `.hebbs/`)
- All wiki-link relationships (re-created from file content)
- All tags (re-extracted from file content)

### What is lost (cognition-plane-only state)

- Decay scores (rebuild to defaults, re-accumulate through usage)
- Access counts (reset to 0)
- Reinforcement signals (reset)
- Pending wiki-links that were never resolved

Acceptable per TASK-13 spec. These rebuild naturally over time.

### Equivalence test

After rebuild:
- Same number of memories (within tolerance for edge cases)
- Same content for each memory_id-equivalent section
- Same graph edges (RELATED_TO from wiki-links, INSIGHT_FROM from frontmatter)
- Scores may differ (no access history)

**Tests**:
- Index vault, run queries (build up access counts), rebuild, verify:
  - Same recall content results
  - Same edge relationships
  - Insight files still present and indexed
- Config preserved across rebuild
- Rebuild on already-clean vault: idempotent

---

## Step 10: CLI Integration

**Goal**: Wire all functionality into `hebbs-cli`.

### Commands

| Command | Description |
|---------|-------------|
| `hebbs init <vault-path> [--force]` | Initialize `.hebbs/` in vault |
| `hebbs index <vault-path>` | Full re-index (phase 1 + phase 2), with progress |
| `hebbs watch <vault-path>` | Start file watcher daemon (foreground) |
| `hebbs rebuild <vault-path>` | Preserve config, delete `.hebbs/`, re-index |
| `hebbs status <vault-path>` | Manifest stats: files, sections, states, last indexed |

### `hebbs index` progress reporting

```
[phase 1] parsing: 145/500 files (1,230 sections)
[phase 1] complete: 500 files, 4,120 sections (380 new, 12 modified, 3,728 unchanged)
[phase 2] embedding: batch 3/83 (150/4,120 sections)
[phase 2] complete: 392 sections embedded in 12.4s
```

### `hebbs status` output

```
Vault: /Users/me/notes
Initialized: 2026-03-14T10:00:00Z

Files:  500 indexed
Sections: 4,120 total
  synced:        4,100
  content-stale:    18
  orphaned:          2

Last phase 1: 2026-03-14T14:32:00Z
Last phase 2: 2026-03-14T14:32:03Z
```

### All commands

- Validate vault path exists and is a directory
- All except `init`: verify `.hebbs/` exists, error with "run `hebbs init <vault-path>` first"
- `--verbose` flag: enable `TRACE` level logging
- `--config <path>`: override config file location

**Tests**:
- Each command with valid input: succeeds
- Each command (except init) without `.hebbs/`: clear error message
- Init + index + status: end-to-end smoke test
- Watch + create file + status: watcher picked it up

---

## Dependency Graph

```
Step 1 (scaffold + types) ──────────────────────────────────┐
                                                             │
Step 2 (parser, needs types) ───────────┐                   │
                                        │                   │
Step 3 (manifest + config + init) ──────┤                   │
                                        │                   │
                                        v                   │
                              Step 4 (phase 1 ingest) ──────┤
                                        │                   │
                                        v                   │
                              Step 5 (phase 2 ingest) ──────┤
                                        │                   │
                                        v                   │
                              Step 6 (watcher) ─────────────┤
                                                            │
Step 7 (query from files, needs 3+5) ──────────────────────┤
                                                            │
Step 8 (insight writer, needs 5+6) ────────────────────────┤
                                                            │
Step 9 (rebuild, needs 3+4+5) ─────────────────────────────┤
                                                            │
Step 10 (CLI, needs all above) ────────────────────────────┘
```

**Parallelizable work**:
- Steps 2 and 3 can be built in parallel (both depend only on Step 1 types)
- Steps 7 and 8 can be built in parallel once Step 5 is done
- Steps 7, 8, and 9 can all be built in parallel

---

## Principles Compliance

| Principle | Compliance |
|-----------|-----------|
| P1 (Hot Path) | Engine query path unchanged. File reads add <1ms (OS page cache). Embedding is always background (phase 2). |
| P2 (Single Binary) | `hebbs-vault` embeds `hebbs-core`. No external processes required. 5 new deps, all justified. |
| P3 (Cognition) | All cognitive features (decay, importance, reflect, graph) preserved. Files are just the content plane. |
| P4 (Bounded) | Batch sizes configurable. Debounce timers bounded. Manifest writes incremental (crash-safe). Adaptive burst caps. |
| P5 (Background) | Phase 2 embedding runs in background tokio task. Queries never blocked by indexing. |
| P6 (Lineage) | Insight files track `hebbs-sources` with human-readable paths. `INSIGHT_FROM` edges created from frontmatter. |
| P8 (Events) | Each section is an independent memory event. File modify = section revise events. Append-only at engine level. |
| P9 (Measure) | Metrics for phase 1/2 latency, batch sizes, watcher events, insight writes. |
| P10 (API Elegance) | No new engine operations. CLI commands are vault management, not API surface. |
| P11 (Correctness) | Atomic manifest writes. Crash-safe incremental processing. Rebuild guarantee as acid test. |
| P12 (Security) | `.hebbs/` gitignored. No secrets in manifest. UTF-8 validation on file parse. Frontmatter YAML depth-limited. |

---

## Out of Scope

Per TASK-13:
- Contradiction detection (TASK-12 Feature 2)
- Token-budgeted retrieval (TASK-12 Feature 3)
- Obsidian plugin / UI integration
- Daemonization (systemd, launchd)
- Multi-vault support
- Encryption on `.hebbs/`
- Remote backend (gRPC client mode)

---

## E2E Scenario Tests

Standalone test suite at `hebbs-repos/e2e-scenario-tests/`. Lives outside the main `hebbs/` repo. Added to the root `.gitignore` so it never gets committed to any subtree.

### Structure

```
hebbs-repos/e2e-scenario-tests/
  README.md                      -- how to run, what to expect
  run_all.sh                     -- runs all scenarios, collects results
  fixtures/
    generate_vault.py            -- generates test vaults of configurable size
    templates/
      note_simple.md             -- single heading, no frontmatter
      note_complex.md            -- frontmatter, 5 headings, wiki-links, tags
      note_insight.md            -- hebbs-kind: insight, hebbs-sources
      note_linked_a.md           -- [[linked_b]] wiki-link
      note_linked_b.md           -- [[linked_a]] wiki-link (bidirectional)
      note_frontmatter_only.md   -- frontmatter, no body
      note_no_headings.md        -- prose only, no ## headings
      note_code_blocks.md        -- fenced code with ## and #tags inside
      note_empty.md              -- 0 bytes
      note_large.md              -- 60KB content (near max)
  scenarios/
    01_first_install.sh          -- Scenario 1: bulk initial index
    02_realtime_editing.sh       -- Scenario 2: rapid single-file saves
    03_agent_writing.sh          -- Scenario 3: burst multi-file creation
    04_bulk_arrival.sh           -- Scenario 4: large batch drop
    05_content_stale_query.sh    -- query during content-stale window
    06_insight_loop.sh           -- reflect -> insight file -> re-index -> no infinite loop
    07_crash_recovery.sh         -- kill mid-index, resume
    08_rebuild_equivalence.sh    -- rebuild produces equivalent index
    09_delete_and_rename.sh      -- file operations map to forget/revise
    10_wiki_link_edges.sh        -- cross-file links become RELATED_TO edges
  lib/
    assertions.sh                -- shared assertion helpers (check_manifest, check_memory, etc.)
    metrics.sh                   -- timing/counting helpers
    vault_ops.sh                 -- vault manipulation helpers (create_file, modify_file, etc.)
  results/                       -- generated per run (gitignored)
    YYYY-MM-DD_HH-MM-SS/
      scenario_01.json
      scenario_02.json
      ...
      summary.json
```

### Fixture Generator (`generate_vault.py`)

Generates vaults of configurable size for different scenarios:

```
python generate_vault.py --size small    # 50 files,  ~200 sections
python generate_vault.py --size medium   # 500 files, ~2,000 sections
python generate_vault.py --size large    # 5,000 files, ~20,000 sections
python generate_vault.py --size xl       # 10,000 files, ~50,000 sections
```

Each generated file:
- Random but deterministic content (seeded RNG for reproducibility)
- 1-8 headings per file (weighted toward 3-5)
- 30% of files have frontmatter with assorted keys
- 20% of files contain wiki-links to other files in the vault
- 10% of files contain tags
- 5% of files are edge cases (empty, no headings, frontmatter-only, near max size)

### Scenario 1: First Install (`01_first_install.sh`)

**Maps to**: TASK-13 lifecycle scenario "First install"

**Setup**: generate medium vault (500 files), run `hebbs init`, then `hebbs index`.

**Qualitative checks**:
- [ ] `.hebbs/` directory created with `manifest.json`, `config.toml`, `index/`
- [ ] Manifest has entries for all 500 files
- [ ] Every section across all files is `Synced`
- [ ] Total section count matches expected (from generator metadata)
- [ ] Files with frontmatter: frontmatter keys appear in memory context
- [ ] Files with wiki-links: `RELATED_TO` edges exist between linked memories
- [ ] Files with tags: tags appear in memory context/kind
- [ ] Empty files: no sections in manifest (not an error)
- [ ] Frontmatter-only files: no sections (no body content to index)
- [ ] Files with code blocks containing `##`: fake headings not treated as sections
- [ ] `hebbs status` output matches manifest state

**Quantitative checks**:
- [ ] Phase 1 total time for 500 files: < 2s (target: microseconds per file)
- [ ] Phase 2 total time for ~2,000 sections: record, no hard threshold (depends on machine/model)
- [ ] Phase 2 embed call count: ceil(sections / batch_size), NOT one call per section
- [ ] Manifest file size: reasonable (< 5MB for 500 files)
- [ ] Re-run `hebbs index` on same vault: completes in < 1s (all checksums match, everything skipped)
- [ ] Memory count in engine matches section count in manifest

**Scale variant**: repeat with large vault (5,000 files) and xl vault (10,000 files). Record times. Phase 1 should scale linearly. Phase 2 scales linearly with section count.

---

### Scenario 2: Realtime Editing (`02_realtime_editing.sh`)

**Maps to**: TASK-13 lifecycle scenario "Realtime editing"

**Setup**: index a small vault (50 files). Start `hebbs watch`. Pick one file and simulate rapid editing.

**Procedure**:
1. Start watcher in background, capture PID
2. Wait for startup catch-up to complete
3. Modify a single file 10 times in 20 seconds (append a line, save, sleep 2s, repeat)
4. Wait for final phase 2 to complete (watch logs for `[phase2] complete`)
5. Stop watcher (SIGINT)

**Qualitative checks**:
- [ ] After all saves, the file's sections are `Synced` in manifest
- [ ] `recall()` for content from the file returns the final (10th) version, not any intermediate
- [ ] Memory content in engine matches the final file content
- [ ] No duplicate memories created (same `memory_id` throughout, revise not re-create)

**Quantitative checks**:
- [ ] Phase 1 run count: ~10 (one per save, after debounce)
- [ ] Phase 2 run count: < 5 (debounced; ideally 1-3, not 10)
- [ ] Embed call count: < 5 (batched, not 10)
- [ ] Time from last save to section `Synced`: < phase1_debounce + phase2_debounce + embed_time (~5s)
- [ ] Phase 1 latency per run: < 50ms (single file parse)

---

### Scenario 3: Agent Writing (`03_agent_writing.sh`)

**Maps to**: TASK-13 lifecycle scenario "Agent writing"

**Setup**: index a small vault (20 files). Start `hebbs watch`. Simulate an AI agent creating 30 new files in rapid succession.

**Procedure**:
1. Start watcher in background
2. Wait for catch-up
3. Create 30 `.md` files in a loop with ~100ms delay between each (total burst: ~3s)
4. Wait for phase 2 to complete
5. Stop watcher

**Qualitative checks**:
- [ ] All 30 new files appear in manifest
- [ ] All sections across all 30 files are `Synced`
- [ ] `recall()` can find content from the new files
- [ ] Wiki-links between new files resolve to `RELATED_TO` edges
- [ ] Original 20 files untouched in manifest (no spurious re-processing)

**Quantitative checks**:
- [ ] Phase 1 run count: 1-3 (batch, not 30 individual runs)
- [ ] Phase 2 run count: 1 (all new sections embedded in one run)
- [ ] Embed call count: ceil(total_new_sections / batch_size) (batched)
- [ ] Total time from first file create to all `Synced`: < 15s (burst + debounce + embed)
- [ ] No burst detection if <20 events per window; burst detection triggered if >20 events per window (verify log message)

---

### Scenario 4: Bulk Arrival (`04_bulk_arrival.sh`)

**Maps to**: TASK-13 lifecycle scenario "Bulk arrival"

**Setup**: index a small vault (10 files). Start `hebbs watch`. Simulate bulk drop by copying 200 pre-generated files into the vault in one operation.

**Procedure**:
1. Generate 200 files in a staging directory (outside vault)
2. Start watcher in background
3. Wait for catch-up
4. `cp staging/*.md vault/notes/` (all 200 files appear at once)
5. Wait for phase 2 to complete
6. Stop watcher

**Qualitative checks**:
- [ ] All 200 new files in manifest, all sections `Synced`
- [ ] `recall()` finds content from new files
- [ ] Original 10 files untouched
- [ ] Cross-file wiki-links resolve correctly

**Quantitative checks**:
- [ ] Burst detection triggered (log message confirms adaptive debounce)
- [ ] Phase 2 debounce extended to 10s (adaptive burst behavior)
- [ ] Phase 2 run count: 1 (single run after burst settles)
- [ ] Embed calls: ceil(total_sections / batch_size), confirms batching not per-file
- [ ] Total time from copy to all `Synced`: record (machine-dependent, but verify linear scaling)
- [ ] Peak memory usage during phase 2: bounded (not loading all 200 files into memory at once)

---

### Scenario 5: Content-Stale Query Window (`05_content_stale_query.sh`)

**Maps to**: TASK-13 two-phase model, content-stale state

**Setup**: index a vault. Start watcher. Modify a file. Query DURING the content-stale window (after phase 1, before phase 2).

**Procedure**:
1. Index vault with a known file containing "vendor X is reliable"
2. Start watcher
3. Modify the file: change "reliable" to "unreliable"
4. Wait for phase 1 to complete (watch logs) but NOT phase 2
5. Immediately run `recall("vendor X")` via the vault query layer
6. Wait for phase 2, run `recall("vendor X")` again

**Qualitative checks**:
- [ ] Query during content-stale: returns "unreliable" (file content, not engine's stored "reliable")
- [ ] `stale` flag is NOT set (byte offsets are valid, file exists)
- [ ] Query after phase 2: returns "unreliable" and embedding now matches
- [ ] Section state transitions: `Synced` -> `ContentStale` (after phase 1) -> `Synced` (after phase 2)

**Quantitative checks**:
- [ ] Recall latency during content-stale window: < 15ms (file read adds <1ms to normal recall)
- [ ] Content freshness delay: 0ms (file is read at query time, not from engine cache)

---

### Scenario 6: Insight Loop (`06_insight_loop.sh`)

**Maps to**: TASK-13 Milestone 6 (insight output as files) + loop prevention

**Setup**: index a vault with enough related content to trigger insight generation during reflect.

**Procedure**:
1. Generate vault with 100 files, 20 of which are semantically related (similar topic, different details)
2. `hebbs init` + `hebbs index`
3. Start watcher
4. Trigger `reflect()`
5. Wait for insight files to appear in `insights/`
6. Wait for watcher to index the insight files
7. Trigger `reflect()` again
8. Repeat step 7 two more times (total: 4 reflect cycles)
9. Stop watcher

**Qualitative checks**:
- [ ] Insight `.md` files created in `insights/` with correct frontmatter
- [ ] `hebbs-sources` in frontmatter are human-readable file paths, not memory_ids
- [ ] `hebbs-confidence` is a float between 0.0 and 1.0
- [ ] Insight files are indexed by watcher (appear in manifest as `Synced`)
- [ ] Insight memories have `MemoryKind::Insight` and `INSIGHT_FROM` edges
- [ ] No infinite loop: insight count stabilizes (does not grow unboundedly across reflect cycles)
- [ ] `recall()` returns insight content alongside regular note content

**Quantitative checks**:
- [ ] Insight count after cycle 1: N (baseline)
- [ ] Insight count after cycle 2: N + M where M < N (diminishing returns, not doubling)
- [ ] Insight count after cycle 4: converged (M approaches 0)
- [ ] No duplicate insights (cosine similarity of any two insight contents < 0.95)

---

### Scenario 7: Crash Recovery (`07_crash_recovery.sh`)

**Maps to**: TASK-13 crash safety (manifest written incrementally)

**Setup**: generate a large vault (1,000 files). Start `hebbs index`. Kill mid-run. Resume.

**Procedure**:
1. Generate 1,000-file vault
2. `hebbs init`
3. Start `hebbs index` in background
4. Wait until progress shows ~50% parsed (watch stdout)
5. `kill -9` the process
6. Examine manifest: should have entries for ~500 files (incremental writes)
7. Run `hebbs index` again
8. Wait for completion

**Qualitative checks**:
- [ ] Manifest after kill: valid JSON, not corrupted
- [ ] Manifest contains entries only for fully-processed files (no partial file entries)
- [ ] Resume run: skips already-indexed files (checksum match), processes remaining
- [ ] Final state: all 1,000 files indexed, all sections `Synced`
- [ ] `recall()` works correctly after resume

**Quantitative checks**:
- [ ] Resume processes ~500 files (not 1,000; skips already-done)
- [ ] Total wall time (first run + resume) < 1.5x single full run (overhead of restart is small)
- [ ] No orphaned memories (kill during phase 2 may leave content-stale sections, but resume cleans them up)

---

### Scenario 8: Rebuild Equivalence (`08_rebuild_equivalence.sh`)

**Maps to**: TASK-13 Milestone 7 (rebuild guarantee)

**Setup**: index a vault with wiki-links and insight files. Run queries. Rebuild. Compare.

**Procedure**:
1. Generate 200-file vault with 30% wiki-links, include 5 pre-made insight files
2. `hebbs init` + `hebbs index`
3. Run 10 recall queries, record results (content, scores, edge counts)
4. Run 5 prime queries, record results
5. `hebbs rebuild`
6. Run same 10 recall queries, record results
7. Run same 5 prime queries, record results
8. Compare pre/post

**Qualitative checks**:
- [ ] Same files in manifest pre/post
- [ ] Same section count per file pre/post
- [ ] Same memory content for each section pre/post (content comes from files, so identical)
- [ ] Same `RELATED_TO` edges (wiki-links re-parsed identically)
- [ ] Same `INSIGHT_FROM` edges (insight frontmatter re-parsed identically)
- [ ] Insight files survive rebuild (they're in vault, not `.hebbs/`)
- [ ] Config.toml preserved across rebuild

**Quantitative checks**:
- [ ] Recall content match: 100% (byte-for-byte identical content)
- [ ] Recall rank order: may differ (access counts reset, so scores change)
- [ ] Edge count match: 100% (same wiki-links, same insight sources)
- [ ] Rebuild time vs initial index time: within 10% (same work)

---

### Scenario 9: Delete and Rename (`09_delete_and_rename.sh`)

**Maps to**: TASK-13 file deletion/rename handling

**Setup**: index a vault. Start watcher. Delete files. Rename files. Verify engine state.

**Procedure**:
1. Index vault with 20 files, each having known content
2. Start watcher
3. Delete file `notes/to-delete.md`
4. Rename file `notes/old-name.md` to `notes/new-name.md`
5. Wait for phase 2
6. Stop watcher

**Qualitative checks**:
- [ ] Deleted file: removed from manifest, all its sections `Orphaned` then cleaned up
- [ ] Deleted file: `forget()` called, memories no longer returned by `recall()`
- [ ] Deleted file: `RELATED_TO` edges from other files to this file's memories are cleaned up
- [ ] Renamed file: old path removed from manifest, new path added
- [ ] Renamed file: old memories forgotten, new memories created (new `memory_id`s)
- [ ] Renamed file: content accessible via `recall()` at new identity
- [ ] Other files' wiki-links pointing to renamed file: links become unresolvable (expected; re-resolve when user updates the wiki-link text)

**Quantitative checks**:
- [ ] `forget()` call count: matches section count of deleted + renamed files
- [ ] `remember()` call count for renamed file: matches its section count (new memories)
- [ ] Total memories in engine: original - deleted_sections - renamed_sections + renamed_sections = original - deleted_sections

---

### Scenario 10: Wiki-Link Edges (`10_wiki_link_edges.sh`)

**Maps to**: TASK-13 wiki-link edge resolution

**Setup**: generate vault with deliberate cross-file link topology. Verify graph edges.

**Fixture** (hand-crafted, not generated):
```
notes/
  project-overview.md       -- [[meeting-notes]], [[api-design]]
  meeting-notes.md           -- [[project-overview]], [[action-items#task-1]]
  api-design.md              -- [[project-overview]]
  action-items.md            -- ## Task 1, ## Task 2 (section-level link target)
  orphan.md                  -- no links to or from anything
  broken-link.md             -- [[nonexistent-file]]
```

**Qualitative checks**:
- [ ] `project-overview` -> `meeting-notes`: bidirectional `RELATED_TO` edges (both files link to each other)
- [ ] `project-overview` -> `api-design`: `RELATED_TO` edge
- [ ] `api-design` -> `project-overview`: `RELATED_TO` edge
- [ ] `meeting-notes` -> `action-items#task-1`: `RELATED_TO` edge to the specific section memory (not the whole file)
- [ ] `orphan.md`: no `RELATED_TO` edges (isolated node in graph)
- [ ] `broken-link.md` -> `nonexistent-file`: no edge created, no error (logged as warning)
- [ ] Causal recall from `project-overview`: traversal reaches `meeting-notes` and `api-design` via edges

**Quantitative checks**:
- [ ] Edge count: matches expected link count (count all `[[...]]` across all files that resolve)
- [ ] No duplicate edges (same source->target pair appears only once regardless of how many times linked)

---

### Result Collection

Each scenario script outputs a JSON result:

```json
{
  "scenario": "01_first_install",
  "vault_size": { "files": 500, "sections": 2034 },
  "pass": true,
  "qualitative": {
    "total": 11,
    "passed": 11,
    "failed": 0,
    "checks": [
      { "name": "all_files_in_manifest", "pass": true },
      { "name": "all_sections_synced", "pass": true },
      ...
    ]
  },
  "quantitative": {
    "phase1_time_ms": 1230,
    "phase2_time_ms": 8450,
    "embed_call_count": 41,
    "manifest_size_bytes": 1048576,
    "memory_count": 2034,
    "reindex_skip_time_ms": 340
  },
  "errors": []
}
```

`run_all.sh` aggregates into `results/<timestamp>/summary.json`:

```json
{
  "run_date": "2026-03-14T15:00:00Z",
  "platform": "darwin-arm64",
  "scenarios": {
    "01_first_install": { "pass": true, "qualitative": "11/11", "duration_ms": 12000 },
    "02_realtime_editing": { "pass": true, "qualitative": "4/4", "duration_ms": 25000 },
    ...
  },
  "overall": { "pass": true, "scenarios_passed": 10, "scenarios_failed": 0 }
}
```

### Running

```bash
cd hebbs-repos/e2e-scenario-tests
./run_all.sh                    # all scenarios, medium vault
./run_all.sh --scenario 02      # single scenario
./run_all.sh --vault-size large # scale test
./run_all.sh --verbose          # show all assertion output
```

Requires: `hebbs-vault` binary built (or specify `--binary ../hebbs/target/release/hebbs-vault`), Python 3.8+ for fixture generation, `jq` for JSON assertions.
