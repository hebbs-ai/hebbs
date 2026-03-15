# PLAN: Full-Corpus Contradiction Detection

Parent: [TASK-12 Feature 2](../TASK-12-markdown-obsidian-cognitive-layer.md#feature-2-full-corpus-contradiction-detection-high-priority)
Related: [PLAN-12](./PLAN-12.md), [TASK-20](../TASK-20-panel-polish.md) (panel visualization)

---

## Goal

Detect semantic contradictions across the full memory corpus. Surface them as `CONTRADICTS` graph edges and optionally as contradiction `.md` files in the vault. Incremental (per-memory, not N^2). Runs in background.

---

## Architecture

```
New/modified memory
        |
        v
  [1. Candidate selection]   <-- HNSW k-NN (O(log n), already built)
        |
        v
  [2. Entailment filter]     <-- LLM classification (background)
        |
        v
  [3. Edge creation]         <-- CONTRADICTS edges in graph CF
        |
        v
  [4. File output]           <-- optional contradiction .md file in vault
        |
        v
  [5. Panel visualization]   <-- red edges + contradiction tab (TASK-20)
```

---

## Steps

### Step 1: Add `Contradicts` edge type -- DONE

**Files:** `hebbs-index/src/graph.rs` + ~12 crate files

- Added `Contradicts = 0x06` to `EdgeType` enum across all crates
- Proto, client, server, CLI, FFI, engine all updated

---

### Step 2: Candidate pair extraction -- DONE

Reuses existing `IndexManager::search_vector_for_tenant()` directly in the pipeline.
No new function needed -- the HNSW search API already supports this.

---

### Step 3: Entailment classification -- DONE

**Files:** New module `hebbs-reflect/src/contradict.rs` (or `hebbs-core/src/contradict.rs`)

Define the classification interface:

```rust
pub enum EntailmentResult {
    Contradiction { confidence: f32 },
    Revision { confidence: f32 },
    Neutral,
}

pub trait EntailmentClassifier: Send + Sync {
    fn classify(&self, memory_a: &str, memory_b: &str) -> Result<EntailmentResult>;
}
```

Two implementations:

**A. LLM-based (primary):** Send a structured prompt to the LLM:
```
Given two statements from a knowledge base, classify their relationship:

Statement A: "{content_a}"
Statement B: "{content_b}"

Classify as one of:
- CONTRADICTION: The statements assert opposing or incompatible facts
- REVISION: Statement B updates or supersedes Statement A (evolution of thinking)
- NEUTRAL: The statements are compatible or unrelated

Consider temporal context. If one statement says "I used to think X" or "updated:", treat it as revision, not contradiction.

Output JSON: {"result": "CONTRADICTION|REVISION|NEUTRAL", "confidence": 0.0-1.0, "explanation": "..."}
```

**B. Embedding-based heuristic (fallback, no LLM required):**
- High similarity (>0.85) + opposite sentiment signals = candidate contradiction
- Uses negation detection: one memory contains "not", "no longer", "failed", "missed" where the other doesn't
- Lower confidence than LLM, but works offline

---

### Step 4: Contradiction pipeline -- DONE (inline mode)

**Files:** `hebbs-core/src/contradict.rs` (new), `hebbs-core/src/engine.rs`

The pipeline, triggered on `remember()` or `ingest()`:

1. Extract top-K candidates for the new memory (Step 2)
2. Filter out pairs that already have a CONTRADICTS or REVISED_FROM edge
3. For each candidate pair, run entailment classification (Step 3)
4. If CONTRADICTION with confidence >= threshold (default 0.7):
   - Create bidirectional CONTRADICTS edges with weight = confidence
   - Optionally write a contradiction `.md` file (Step 5)
5. If REVISION with confidence >= threshold:
   - Suggest a REVISED_FROM edge (or flag for user review)

**Trigger modes:**
- **Inline (small vaults):** Run synchronously after `remember()`. Adds latency but catches contradictions immediately.
- **Background (large vaults):** Queue new memory IDs, process in a background worker (same pattern as reflect worker). Default mode.
- **Batch (initial scan):** On vault init or rebuild, scan all pairs. Uses the neighborhood snapshot for efficient batch processing.

**Bounded:** Max K=10 candidates per memory. Max 1 LLM call per candidate pair. Total cost per new memory: O(K) LLM calls.

---

### Step 5: Contradiction file output

**Files:** `hebbs-vault/src/contradiction_writer.rs` (new)

When a contradiction is detected in a vault context, optionally write a `.md` file:

```markdown
---
hebbs-kind: contradiction
hebbs-sources:
  - notes/meeting-jan.md#vendor-evaluation
  - notes/q3-review.md#vendor-performance
hebbs-confidence: 0.87
hebbs-detected: 2026-03-15T12:00:00Z
---

## Contradiction detected

**Statement A** (notes/meeting-jan.md > Vendor Evaluation):
> Vendor X has been reliable and delivered on time for all three milestones.

**Statement B** (notes/q3-review.md > Vendor Performance):
> Vendor X missed three consecutive deadlines in Q3.

These statements make opposing claims about Vendor X's delivery reliability.
```

Controlled by config: `[contradiction] output_mode = "file" | "edge_only" | "off"`

The watcher picks up the contradiction file and indexes it like any other file, creating bidirectional INSIGHT_FROM edges back to the source memories.

---

### Step 6: Engine API -- DONE

**Files:** `hebbs-core/src/engine.rs`, `hebbs-core/src/contradict.rs`

Public API on Engine:

```rust
/// Check a single memory against the corpus for contradictions.
pub fn check_contradictions(&self, memory_id: &[u8]) -> Result<Vec<ContradictionResult>>;

/// Scan all memories for contradictions (batch mode).
pub fn scan_contradictions(&self, config: ContradictionConfig) -> Result<ContradictionScanOutput>;

/// Get all known contradictions for a memory.
pub fn contradictions(&self, memory_id: &[u8]) -> Result<Vec<Contradiction>>;
```

---

### Step 7: Panel visualization -- DONE (basic)

**Files:** `hebbs-vault/src/panel/routes.rs`, `graph.js`, `app.js`

Once CONTRADICTS edges exist in the graph:

- `graph_data` handler already returns graph edges with types. CONTRADICTS edges will appear automatically.
- **graph.js:** Render CONTRADICTS edges as red/dashed lines (currently all edges are green)
- **Side panel:** When a node has contradictions, show a warning badge and list the contradicting memories
- **Optional: Contradiction tab** in the panel showing all detected contradictions with resolution actions (dismiss, mark as revision, delete one)

This is the bridge back to TASK-20: add a panel item "10. Contradiction visualization" that depends on this plan completing Steps 1-6.

---

## Dependency Graph

```
Step 1 (edge type)
    |
Step 2 (candidates)  -----> already built (HNSW + neighborhood)
    |
Step 3 (classifier)
    |
Step 4 (pipeline) ---------> Step 5 (file output, vault only)
    |
Step 6 (engine API)
    |
Step 7 (panel viz) --------> TASK-20 Item 10
```

Steps 1-2 are trivial. Step 3 is the core design work. Steps 4-5 follow the existing reflect worker pattern. Step 7 is panel work that returns to TASK-20.

---

## Open Design Decisions

1. **Contradiction vs Revision threshold:** Same LLM call classifies both. Default: confidence >= 0.7 for either. Below that, classify as Neutral.

2. **Temporal heuristics:** If memory B was created significantly after memory A (configurable, default 7 days) and covers the same entity, bias toward REVISION over CONTRADICTION.

3. **Resolution flow:** Contradictions are flagged, never auto-resolved. User (or agent) must explicitly:
   - Dismiss (remove CONTRADICTS edge)
   - Mark as revision (convert to REVISED_FROM edge)
   - Delete one of the memories

4. **Batch scan cost:** Full corpus scan of N memories = N * K candidate pairs = N * K LLM calls. For 10K memories with K=10, that's 100K calls. Need rate limiting and incremental processing. The batch scan should be opt-in, not automatic.

---

## LLM vs Non-LLM Mode (auto-detected)

Both modes are always implemented. The engine auto-detects which to use based on config.

### How detection works

The `[reflect]` section in `config.toml` (or vault's `.hebbs/config.toml`) contains LLM settings (API key, model, endpoint). On startup and on config reload:

```rust
if config.reflect.api_key.is_some() && config.reflect.model.is_some() {
    // LLM mode: high-quality entailment classification
    classifier = LlmEntailmentClassifier::new(config.reflect);
} else {
    // Heuristic mode: embedding-based, no external calls
    classifier = EmbeddingEntailmentClassifier::new();
}
```

No user action needed. Add an API key and model to config, contradictions automatically upgrade to LLM quality. Remove them, it falls back to heuristics silently.

### LLM mode (high quality)

- Sends candidate pairs to LLM with structured entailment prompt
- Classifies as CONTRADICTION, REVISION, or NEUTRAL with confidence score
- Distinguishes temporal evolution ("I used to think X") from true contradiction
- Generates human-readable explanation for the contradiction file
- Cost: 1 LLM call per candidate pair (bounded by K=10 per memory)
- Typical accuracy: high (LLM understands nuance, negation, context)

### Heuristic mode (no LLM, works offline)

- Uses embedding similarity + lexical signals, no external API calls
- Detection signals (combined scoring):
  - **Negation asymmetry:** One memory contains negation markers ("not", "no longer", "never", "failed", "missed", "stopped") where the other asserts the positive
  - **Antonym detection:** Simple antonym pairs in overlapping context ("reliable" vs "unreliable", "increase" vs "decrease", "success" vs "failure")
  - **Numeric disagreement:** Same entity + same attribute + different numbers ("3 milestones" vs "0 milestones", "99.9% uptime" vs "frequent outages")
  - **Temporal markers:** Presence of "used to", "previously", "updated", "changed", "now" biases toward REVISION
- Confidence scores are lower (capped at 0.75) to reflect reduced accuracy
- No explanation text generated (contradiction files show source quotes only)
- Typical accuracy: moderate (catches obvious contradictions, misses subtle ones)

### Behavior differences

| Aspect | LLM Mode | Heuristic Mode |
|--------|----------|----------------|
| Accuracy | High | Moderate |
| Latency per pair | ~500ms-2s (API call) | ~1ms (local) |
| Works offline | No | Yes |
| Contradiction file explanation | LLM-generated | Source quotes only |
| Revision detection | Strong (understands context) | Weak (keyword-based) |
| Max confidence | 1.0 | 0.75 |
| Cost | LLM API usage | Zero |

### Config example

```toml
[contradiction]
enabled = true
output_mode = "file"          # "file" | "edge_only" | "off"
min_confidence = 0.7          # below this, classify as Neutral
candidates_k = 10             # max neighbors to check per memory
min_similarity = 0.7          # skip pairs below this similarity

# LLM mode activates automatically when reflect config has API key + model
# No separate contradiction LLM config needed; reuses reflect settings
```

---

## To Communicate

When this ships, document and communicate:

1. **Blog post / changelog:** "HEBBS now detects contradictions in your knowledge base"
   - Works out of the box without LLM (heuristic mode)
   - Add an API key to config for higher-quality LLM-powered detection
   - Contradictions appear as red edges in the Memory Palace panel
   - Optional: contradiction `.md` files written to vault for human review

2. **Docs update:** Add to hebbs-docs:
   - `contradiction-detection.md`: How it works, LLM vs heuristic, config options
   - Update `config-reference.md` with `[contradiction]` section
   - Update `operations.md` with `check_contradictions()` and `scan_contradictions()` API

3. **Key messaging:**
   - "No Obsidian plugin or note-taking tool does this today" (from TASK-12)
   - Zero-config: works immediately with heuristic mode on any vault
   - Scales with you: add LLM config for production-grade accuracy
   - Never auto-resolves: flags contradictions for human/agent review

---

## Estimated Scope

| Step | Size | Blocked by |
|------|------|------------|
| 1. Edge type | Small | Nothing |
| 2. Candidates | Small | Nothing (HNSW exists) |
| 3a. Heuristic classifier | Medium | Nothing |
| 3b. LLM classifier | Medium | hebbs-reflect config |
| 4. Pipeline + auto-detect | Medium | Steps 1-3 |
| 5. File output | Small | Step 4 |
| 6. Engine API | Small | Step 4 |
| 7. Panel viz | Small | Step 6, TASK-20 |

Steps 1-3a can ship immediately (heuristic mode, no LLM needed). Step 3b adds LLM mode. Step 7 returns to TASK-20.

---

## Implementation Status (2026-03-15)

| Step | Status | Notes |
|------|--------|-------|
| 1. Edge type | DONE | `Contradicts = 0x06` across all ~12 crate files |
| 2. Candidates | DONE | Reuses `IndexManager::search_vector_for_tenant()` directly |
| 3a. Heuristic classifier | DONE | `hebbs-core/src/contradict.rs` - negation, antonyms, numeric, revision markers |
| 3b. LLM classifier | DONE | `llm_classify()` via `hebbs_reflect::LlmProvider` with structured JSON prompt |
| 4. Pipeline | DONE (inline) | `check_memory_contradictions()` called after `engine.remember()` in ingest |
| 5. File output | DONE | `contradiction_writer.rs`, writes `.md` files to `contradictions/` dir |
| 6. Engine API | DONE | `check_contradictions()`, `contradictions()` on Engine |
| 7. Panel viz | DONE (basic) | Red dashed edges for CONTRADICTS type in graph.js |

### Key files
- `hebbs-core/src/contradict.rs` - Core module (~400 lines, 12 unit tests)
- `hebbs-core/tests/contradiction_tests.rs` - Integration tests (25 tests)
- `hebbs-vault/src/config.rs` - `ContradictionConfig`, `OutputConfig.contradiction_dir`
- `hebbs-vault/src/contradiction_writer.rs` - File output (7 tests)
- `hebbs-vault/src/ingest.rs` - Pipeline hook after `engine.remember()`, file writing after manifest update
- `hebbs-vault/src/panel/static/graph.js` - Red dashed edge rendering

### Test coverage
- 12 unit tests in `contradict.rs` (heuristic classifier, LLM parsing, numerics)
- 25 integration tests in `contradiction_tests.rs` (engine API, pipeline, bidirectional edges, config, edge cases)
- 7 unit tests in `contradiction_writer.rs` (file creation, frontmatter, source resolution, multiple contradictions)
- All workspace tests passing
