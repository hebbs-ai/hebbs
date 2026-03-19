# TASK-22: New Customer Onboarding Issues

Issues discovered during a fresh onboarding walkthrough (2026-03-18).

---

## Issues Log

### 22-01: `cargo build --release` fails due to hebbs-python-native (P1) -- FIXED

**Symptom:** Default `cargo build --release` from workspace root fails with linker errors: undefined Python symbols (`_PyBaseObject_Type`, `_PyDict_New`, etc.) from the `hebbs-python-native` (PyO3) crate.

**Impact:** A new developer or customer building from source hits a wall immediately. The default build command doesn't work without a Python dev environment configured.

**Expected:** `cargo build --release` should succeed out of the box for the CLI binary. Either:
- Make `hebbs-python-native` an opt-in feature flag (e.g., `--features python`)
- Exclude it from default workspace members
- Document `cargo build --release -p hebbs-cli` as the primary build command

**Fix:** Moved `hebbs-python-native` from `members` to `exclude` in workspace `Cargo.toml`. Build it explicitly with `cargo build --release -p hebbs-python-native` when Python bindings are needed.

---

### 22-02: No install step; user must know full path to binary (P1)

**Symptom:** After `cargo build --release`, the binary lives at `hebbs/target/release/hebbs`. If the user navigates to a different directory (e.g., their project folder), `./target/release/hebbs` or just `hebbs` doesn't work.

**Impact:** Breaks the "just run `hebbs init .`" promise from docs/website. Every new user will hit this friction.

**Expected:** Either:
- `cargo build --release` followed by `cargo install --path crates/hebbs-cli` to put it in `~/.cargo/bin/`
- Or the onboarding docs should tell the user to add the binary to PATH / symlink it
- Or the install script (`curl -sSf https://hebbs.ai/install | sh`) should be the primary path, with build-from-source as a documented alternative

---

### 22-03: `hebbs init` asks all setup questions before checking if vault exists (P2)

**Symptom:** Running `hebbs init .` on a directory that already has a vault prompts the user for LLM provider and model, collects all answers, then fails with `Error: vault already initialized at .: use --force to reinitialize`.

**Impact:** Wastes user's time. Feels broken: "why did you ask me all that if you were going to reject it?"

**Expected:** Check for existing vault *first*. If it exists and `--force` was not passed, bail immediately with the error before prompting for anything.

---

### 22-04: Status shows wrong LLM model after init (P0)

**Symptom:** User selected `gemma3:1b` during `hebbs init`, but `hebbs status` shows `ollama/gemini-3-flash-preview`. The user's choice was either not persisted or overwritten by a default.

**Impact:** Trust-breaking. User explicitly chose a model and the system is using a different one. Could also mean LLM extraction is silently failing if the displayed model doesn't exist locally.

**Expected:** `hebbs status` should reflect the exact model the user selected during init.

---

### 22-05: Indexing silently stops; 0% indexed after successful init (P0)

**Symptom:** `hebbs init` reports "Indexing 36 file(s) in the background" and daemon started successfully. Seconds later, `hebbs status` shows "Files: 36 (0% indexed), Memories: 0, Indexing: waiting for next change to re-index." Indexing appears to have silently failed or never started.

**Impact:** Core functionality broken. The user's files are not being processed. No error is shown.

**Expected:** Either indexing should be in progress (with a percentage > 0), or if it failed, there should be an error message explaining why.

**Root cause (partial):** Daemon process is not running. `ps aux | grep hebbs` shows no `hebbs serve` process. Init reported "daemon started, connected" but the daemon died shortly after. Likely because the wrong model name caused an immediate failure.

---

### 22-06: Daemon dies silently after init with no logs (P0)

**Symptom:** `hebbs init` reports "daemon started, connected" but the daemon is not running seconds later. No log file found at `~/.local/share/hebbs/logs/`, `~/.hebbs/`, or `.hebbs/`. No stderr output, no crash report.

**Impact:** User has no way to diagnose what went wrong. The system looks like it's working but nothing is happening.

**Expected:** Daemon logs should be written to a discoverable location (e.g., `.hebbs/daemon.log`). If the daemon crashes, `hebbs status` should report "daemon not running" instead of silently showing stale state.

---

### 22-07: `--provider` and `--model` CLI flags ignored; interactive wizard still shown (P2)

**Symptom:** Running `hebbs init . --force --provider ollama --model qwen2.5:3b` still prompts the interactive wizard for provider and model selection instead of using the CLI flags.

**Impact:** Non-interactive/scripted installs are broken. Users who pass flags explicitly still have to answer prompts manually.

**Expected:** If `--provider` and `--model` are passed, skip the interactive prompts and use the provided values directly.

---

### 22-08: Recall results dominated by template/boilerplate files (P2)

**Symptom:** `hebbs recall "what content ideas do I have"` returns template placeholder text (`{{Title}}`, "What triggered this?") from `templates/idea.md` and `templates/twitter.md` instead of actual content ideas.

**Impact:** First recall experience feels broken. User sees boilerplate instead of useful results. Bad first impression.

**Expected:** Either:
- Templates/boilerplate should be lower-ranked (detect placeholder patterns like `{{...}}`)
- `watch.ignore_patterns` should include `templates/` by default or the init wizard should let users exclude folders
- LLM extraction should recognize template scaffolding vs. actual content and score it lower

---

### 22-09: Duplicate memories from same file section (P1)

**Symptom:** Recall results contain exact duplicates (same text, same source file, same section). E.g., `templates/idea.md > {{Title}} > The Spark` appears twice in the same result set.

**Impact:** Wastes result slots. With top_k=5, two duplicates means the user only sees 3 unique results.

**Expected:** Dedup at recall time (same memory_id or same content hash should collapse to one result). Also investigate why the same section produces multiple memory records during indexing.

---

### 22-10: Retrieval pipeline missing multi-granularity routing, reranking, and quality filtering (P0, design)

**Symptom:** Recall quality is poor because raw heading chunks are the only thing searched. LLM extraction (propositions, document summaries) is skipped on first index and may never run. No reranking or granularity routing at query time.

**Current flow:**
1. Parse file on `##` headings
2. Embed each heading chunk as a memory (this IS the memory)
3. LLM extraction skipped on first index
4. Recall: embed query, HNSW nearest-neighbor, composite score, return top-k
5. No dedup, no reranking, no granularity awareness

**Impact:** The 3-layer architecture (document/proposition/graph) exists in code but doesn't participate in retrieval for most users. What they get is basic heading-chunk vector search, equivalent to a simple RAG pipeline. Templates, boilerplate, and short fragments compete equally with substantive content.

**Expected flow:**
1. Parse + embed heading chunks (fast, immediate, same as now)
2. Background LLM: extract propositions (with quality filtering: skip templates, boilerplate, short text), generate per-file document summaries
3. Recall: search ALL granularities (documents, chunks, propositions)
4. Rerank: broad queries favor document-level, specific queries favor propositions
5. Dedup: collapse near-identical results by content hash

**Research basis:** Dense X Retrieval (+10.1% Recall@20), Anthropic Contextual Retrieval (67% fewer failures with reranking), Mix-of-Granularity (query-adaptive routing). All three papers are referenced in docs/research/ingestion-architecture.md but the retrieval-time intelligence they describe is not implemented.

---

### 22-11: Chunking has no smartness; edge cases produce junk memories (P1)

**Three problems in parser.rs / ingest.rs:**

1. **No headings = entire file is one memory.** A 5000-word flat file becomes one embedding that averages everything, matching nothing well. Should be split by paragraph or sentence boundaries as fallback.

2. **Empty heading sections still get stored.** `## Title` with no body content creates a memory with empty content. An empty string gets embedded and takes a result slot. Parser should skip or merge these.

3. **`min_section_length` config is defined but never enforced.** The config has `min_section_length = 50`, it's exposed in the panel UI, but neither `parser.rs` nor `ingest.rs` ever checks it. Short fragments like "$5,000" become standalone memories. The config is dead code.

**Expected:**
- No-heading files: fall back to paragraph-level splitting (split on double newlines)
- Empty sections: merge with next section or skip entirely
- Short sections: enforce `min_section_length` by merging with parent or adjacent section
- Consider semantic chunking as a future enhancement for files that don't follow markdown structure
