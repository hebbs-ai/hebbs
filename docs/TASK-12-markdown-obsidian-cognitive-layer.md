# TASK-12: Markdown/Obsidian Cognitive Layer

## Context

Every person who sees HEBBS asks the same question: "does it work with Markdown/Obsidian?" HEBBS was not built for this, but the architecture maps cleanly. Separately, a user provided three feature requests that converge on the same thesis: HEBBS should be an invisible intelligence layer over files people already have, not a migration destination.

## Thesis

People don't want a memory database. They want their existing files to become smarter. HEBBS should be a lens you put over data you already have, not a place you put data.

Current HEBBS says "give me your data, I'll make it smart." The file-first model says "keep your data, I'll sit next to it and make it smart." The engine is disposable. The files are permanent.

---

## Architecture: Content Plane vs Cognition Plane

Two planes, strictly separated. Inspired by how git works -- git doesn't own your files, it maintains a parallel data structure (`.git/`) that tracks changes, history, relationships. Delete `.git/` and your files are untouched.

```
vault/                              <- CONTENT PLANE (source of truth)
  notes/
    meeting-jan.md
    api-design.md
  insights/                         <- engine-generated, but still just files
    insight-001.md
  .hebbs/                           <- COGNITION PLANE (gitignored, rebuildable)
    index/
      hnsw.bin                      <- vector index
      temporal.bin                  <- time-sorted index
      graph.bin                     <- edges: backlinks, insights, contradictions
    manifest.json                   <- file -> memory_id mapping, checksums, section offsets
    config.toml                     <- chunking strategy, embedding model, etc.
```

**Content plane**: markdown files. Human-readable, human-editable, git-tracked. Always the truth.

**Cognition plane**: `.hebbs/` directory. Machine-readable. Derived entirely from the content plane. Rebuildable from scratch (like `node_modules` from `package.json`).

**Key property: delete `.hebbs/` and you lose nothing.** Re-index and everything comes back. The files are the database. `.hebbs/` is just a very smart index over them.

### Content is never copied into the index

The cognition plane stores embeddings, scores, edges, and a manifest (file path + checksum + section offsets). When you `recall()` or `prime()`, the engine reads content from the actual file at query time. This means:

- Editing a file immediately changes what recall returns (content is always fresh)
- The cognition plane is small (no content duplication)
- Embedding staleness exists (file changed but not re-embedded yet), but the watcher closes that window

### Unit of memory: heading sections, not whole files

A file with 5 `##` sections becomes 5 memories, linked by PART_OF edges to the file. This gives granular recall without losing file-level context. One file can contain both a correct claim and a contradicted claim -- sub-file resolution is necessary. Chunking strategy is configurable via `config.toml`.

### The daemon

A watcher process that bridges the two planes:
1. Watches the vault for file changes (create, modify, delete, rename)
2. Debounces (don't re-index on every keystroke while editing)
3. Parses markdown: frontmatter becomes metadata, `[[wiki-links]]` become graph edges, `#tags` become memory kinds
4. Embeds and updates indexes
5. Runs background intelligence (contradiction checks, decay updates, periodic reflect)
6. Serves queries (recall, prime, reflect)

---

## Feature 1: Bidirectional Markdown Sync (highest priority)

### Problem

HEBBS currently treats memories as opaque data it owns. Markdown/Obsidian users are deeply attached to files being plain text, portable, and the source of truth. If HEBBS ingests files but doesn't write back, it becomes a black box. If it writes back but corrupts formatting, it's worse than useless.

### Design Direction: Insights as Files

The write-back problem dissolves if you stop thinking about "writing back to user files" and instead think about "creating new files."

- **Insights are markdown files, not database records.** When `reflect()` generates an insight, it writes a new `.md` file into the vault (e.g., `vault/insights/insight-001.md`). The watcher picks it up and indexes it like any other file. The cycle is: **files -> shadow (index) -> operations -> new files -> shadow...**
- **Contradictions are also files** (or just graph edges, depending on user preference). Explicit mode creates a contradiction note linking both sources. Quiet mode adds a CONTRADICTS edge, surfaced via queries only.
- **The engine never modifies user files.** No frontmatter pollution. No sidecar files. No conflict resolution needed because the engine only creates new files, never touches existing ones.
- A file watcher daemon monitors a directory for `.md` file changes.
- Ingest path: new/modified `.md` file is parsed into heading sections, each becoming a memory. Frontmatter becomes metadata. Wiki-links (`[[backlinks]]`) become graph edges. Tags become memory kinds.
- Deleted file maps to `forget()`.

An insight file looks like:
```markdown
---
hebbs-kind: insight
hebbs-sources:
  - notes/meeting-jan.md#vendor-evaluation
  - notes/q3-review.md#vendor-performance
hebbs-confidence: 0.82
---

The initial positive vendor assessment was based on a single
project. By Q3, three missed deadlines revealed a pattern.
```

The user can read it, edit it, disagree with it, delete it. It's just a file. It participates in future recalls, primes, and reflects.

### Open Questions

- How to handle Obsidian-specific features (canvas files, dataview queries, templater output)?
- Should HEBBS track file renames as `revise()` or as `forget()` + `remember()`?
- Configurable insight output directory (default `insights/`, but user may want `_hebbs/` or a dot-prefixed hidden directory)?

---

## Feature 2: Full-Corpus Contradiction Detection (high priority)

### Problem

Current `reflect()` consolidates clusters of similar memories into insights, but it does not systematically scan for semantic contradictions across the full corpus. In large knowledge bases (personal vaults, agent memory stores), contradictions accumulate silently over time. Example: a note from January says "vendor X is reliable," a note from August says "vendor X missed three deadlines." No one catches this.

### Design Direction

- A dedicated pipeline stage (runs as background work, fits Principle 5: background intelligence, foreground speed).
- Approach: cluster-level or pairwise entailment checking. For each new memory or insight, check entailment against semantically similar existing memories.
- Use embedding similarity as a cheap first pass to find candidate contradiction pairs, then use LLM entailment classification on the candidates.
- Output: CONTRADICTS edges in the graph (bidirectional), surfaced via queries. In explicit mode, also writes a contradiction note as a `.md` file linking both sources (same pattern as insight files).
- Contradiction resolution: flag both memories, surface to user (or agent) for resolution. Do not auto-resolve; humans and agents need to make the call.
- Incremental: only check new/modified memories against existing corpus, not full N^2 scan every time.

### Open Questions

- Should contradiction detection be part of `reflect()` or a separate operation/policy?
- What's the right threshold for "contradiction" vs. "evolution of thinking"? (e.g., "I used to think X, now I think Y" is not a contradiction, it's revision.)
- How to handle soft contradictions (different emphasis, partial overlap)?

---

## Feature 3: Token-Budgeted Retrieval for prime() (lower priority)

### Problem

`prime()` currently returns top-K memories. But callers (agents, plugins) have a fixed context window budget. Returning 20 memories that total 8,000 tokens when the caller only has 2,000 tokens of budget is wasteful. The caller has to truncate, losing the importance ranking HEBBS already computed.

### Design Direction

- Add a `max_tokens` parameter to `prime()`.
- HEBBS ranks candidate memories by composite score (importance, recency, reinforcement, decay), then greedily packs memories into the token budget, highest-score first.
- Token counting: use a fast tokenizer (tiktoken or similar) or a configurable bytes-per-token approximation.
- In the file-first architecture, content is read from files at query time (not stored in the index), so token-budgeted packing naturally becomes a "read what you need" operation.
- For the Markdown use case: opening a note triggers `prime(context=current_note, max_tokens=2000)`, and the sidebar shows the most relevant, contradiction-free, importance-ranked content from the entire vault, fitted exactly to the display budget.
- Works equally well for agent callers: `prime(context=current_task, max_tokens=model_context_limit - prompt_size)`.

### Open Questions

- Should token counting be exact (requires knowing the target model's tokenizer) or approximate (bytes/4)?
- Should `prime()` support summarization of memories to fit more into the budget, or strictly return verbatim content?
- How does this interact with `insights()`? Should consolidated insights be preferred over raw memories when budget is tight?

---

## Correlation

All three features and the Obsidian/Markdown demand converge:

- (1) says the data format must remain human-readable and human-editable.
- (2) says HEBBS should actively maintain knowledge consistency, the biggest pain point in large vaults and long-running agent memory stores.
- (3) says retrieval should be smart about what fits in a context window (for agents) or a UI panel (for humans).

The Obsidian crowd is thinking about it from the human side (my files, my format). The agent builders are thinking about it from the agent side (token budgets, contradiction safety). Same architecture serves both.

---

## Implementation Order

1. ~~Bidirectional Markdown sync~~ **DONE** ([TASK-13](./done/TASK-13-file-first-markdown-sync.md), 7 milestones complete)
2. ~~Contradiction detection pipeline~~ **DONE** ([PLAN-contradiction](./plans/PLAN-contradiction.md), all 7 steps)
3. Token-budgeted prime() -- deprioritized (agents handle truncation themselves for now)

## Codebase Analysis (snapshot from 2026-03-13, pre-implementation)

### What exists today

- **Storage**: RocksDB with bitcode serialization (gamma-encoded, CRC-32C checksummed). Five column families: default, temporal, vectors, graph, meta. Completely opaque to humans.
- **Memory records**: ~200-220 bytes per record (without embedding). Content field is unstructured plain text (max 64KB). Context stored as pre-serialized JSON. No markdown parsing or awareness anywhere in the engine.
- **prime()**: Two-stage (temporal then optional similarity re-rank). Bounded by `max_memories` (default ~50, hard limit `MAX_PRIME_MEMORIES=200`). No token awareness. `PRIME_ENTITY_SCAN_LIMIT=500` caps brute-force scans.
- **reflect()**: Four-stage pipeline (cluster via k-means on embeddings, LLM proposal, LLM validation, storage). Validation checks candidate insights against source memories + existing insights, but only within the same cluster. Agent-driven two-step mode available (`reflect-prepare` / `reflect-commit`). Background worker with staggered scheduling.
- **Graph layer**: Tracks edges typed as CAUSED_BY, RELATED_TO, FOLLOWED_BY, REVISED_FROM, INSIGHT_FROM. No CONTRADICTS edge type exists yet.
- **Indexes**: Temporal (B-tree), HNSW (vector), graph (causal), associative. All internal to RocksDB.

### Gap analysis per feature

**Feature 1 (Markdown sync):**
- Zero markdown infrastructure exists. No file watcher, no parser, no write-back mechanism.
- The `subscribe()` event stream exists (real-time memory change notifications), which could drive write-back.
- `remember()` accepts arbitrary content + context metadata, so ingesting parsed frontmatter is straightforward.
- The causal graph already supports typed edges, so wiki-link `[[backlinks]]` can map to RELATED_TO edges naturally.
- Biggest engineering gap: the write-back loop. Need debouncing, file-wins conflict resolution, and careful separation of "HEBBS cognition metadata" from "user prose" in the file.

**Feature 2 (Contradiction detection):**
- reflect()'s validation stage is cluster-scoped. Two memories that contradict but live in different clusters (different topics, different time periods) will never be compared.
- The HNSW index can serve as the cheap first pass: for any new memory, query top-K similar, then run entailment classification on candidates. This is O(embed) + O(log n) per new memory, not O(n^2).
- No CONTRADICTS edge type in the graph schema yet, but adding one is trivial (the graph layer is edge-type-agnostic internally).
- The hard problem is temporal: "I used to think X, now I think Y" has the REVISED_FROM edge type for explicit revisions, but implicit evolution (two separate notes months apart) lacks signal. Need heuristics: same entity + high similarity + opposing sentiment/claims = candidate contradiction; presence of temporal markers ("now," "updated," "changed my mind") = likely revision.
- Could run as a background worker alongside the existing reflect worker, triggered by the same policy (threshold-based or scheduled).

**Feature 3 (Token-budgeted prime):**
- prime() already ranks by composite score (relevance 0.5, recency 0.2, importance 0.2, reinforcement 0.1). Adding token packing on top of this ranking is additive, not architectural.
- Content field is plain text, so token estimation via `len(content) / 4` is reasonable as a default. Exact tokenizer (tiktoken) can be optional.
- PrimeOutput already returns scored results in ranked order. The change is: instead of slicing at `max_memories`, slice at `max_tokens` by cumulative content length.
- Interaction with insights: consolidated insights are denser (more information per token). When budget is tight, preferring insights over raw episode memories gives better information density. This could be a `prefer_insights: bool` parameter.

---

## Implementation Order

1. ~~Bidirectional Markdown sync~~ **DONE** ([TASK-13](./done/TASK-13-file-first-markdown-sync.md))
2. ~~Contradiction detection pipeline~~ **DONE** ([PLAN-contradiction](./plans/PLAN-contradiction.md))
3. Token-budgeted prime() (glue that makes 1 and 2 useful for both humans and agents) -- next

## Status

**Feature 1: COMPLETE** (2026-03-14). Implemented as [TASK-13](./done/TASK-13-file-first-markdown-sync.md). See [PLAN-13](./done/PLAN-13.md) for implementation details and [TEST-PLAN-13](./done/TEST-PLAN-13-manual.md) for quality analysis.

**Feature 2: COMPLETE** (2026-03-15). See [PLAN-contradiction](./plans/PLAN-contradiction.md). All 7 steps done. Heuristic mode works out of the box; LLM mode auto-activates when reflect config has API key. Pipeline hooks into ingest after `engine.remember()`. Panel shows CONTRADICTS edges as red dashed lines. Contradiction files written to `contradictions/` directory with frontmatter (`hebbs-kind: contradiction`, sources, confidence, classification). 44 tests (12 unit + 25 integration + 7 contradiction_writer).

**Feature 3 (Token-budgeted retrieval):** Deprioritized. Right now agents calling `prime()` handle truncation themselves -- they receive ranked results and cut at their own budget. The optimization of letting HEBBS do the packing is a quality-of-life improvement, not a blocker. Revisit when agent integrations are in production with tight context windows or when the Obsidian sidebar ships with a fixed display budget.
