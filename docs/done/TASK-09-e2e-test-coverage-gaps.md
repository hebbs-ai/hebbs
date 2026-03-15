# TASK-09: E2E Test Coverage & Observability Gaps

Remaining P2 issues from the March 11 E2E log analysis. These are test/logging gaps, not engine bugs — the features likely work but the E2E suite doesn't verify or surface them.

## Issues

### 1. ScoringWeights order verification

The E2E test calls recall with recency-heavy and relevance-heavy weights but only asserts `results=5` for both. It does not compare result ordering or scores between the two calls.

**What to do:** Print both result lists side-by-side (id, score, content) and assert that the top result differs, or that at least one pair of results swaps rank.

### 2. Insight lineage (`source_memory_ids`) not visible in E2E output

The P1 fix added `source_memory_ids` to REST and gRPC responses for Insight-kind memories. The E2E test doesn't print or assert on this field, so we can't verify the fix from logs alone.

**What to do:** After reflect, fetch an insight and assert `source_memory_ids` is a non-empty array. Print the IDs in the log.

### 3. Multi-strategy recall: no per-result provenance

When recall uses multiple strategies (e.g. similarity + temporal), results are a flat scored list with no indication of which strategy found each result.

**What to do:** Evaluate whether `strategy_source` or similar metadata should be added to `RecallResult`. If not worth the API surface, document this as by-design in SKILL.md.

### 4. Subscribe/Feed: verify push count expectations

The post-fix Subscribe/Feed test receives 1 push. The debug session during the fix showed 4 pushes. The E2E test doesn't document whether 1-per-feed-chunk is by design or if additional matches are being filtered.

**What to do:** Add a comment in the test explaining expected push count, or add a test with a feed chunk that should match multiple subscriptions.

### 5. 70-char content truncation in E2E logs

Insight content is truncated to ~70 characters in the E2E output, making qualitative analysis of generated insights difficult.

**What to do:** Increase or remove the truncation limit in the test logger, or add a `--verbose` flag that prints full content.

## Priority

All P2. None of these block agent usage — they block our ability to verify quality from E2E logs.
