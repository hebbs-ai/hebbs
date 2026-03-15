# TASK-05: SDK Parity & E2E Validation (Python & TypeScript)

This task tracks the remaining work to bring both the Python and TypeScript SDKs to full feature parity with the HEBBS Proto definitions and CLI, and to address findings from the March 9th E2E validation.

## 1. Python SDK (`hebbs-python`) — Missing Fields

The Python SDK is missing several critical fields across multiple operations.

### High Priority: Recall Strategy Config
- [ ] `strategies[].top_k` — Per-strategy top_k override
- [ ] `strategies[].ef_search` — HNSW ef_search tuning
- [ ] `strategies[].time_range` — Temporal window filter
- [ ] `strategies[].seed_memory_id` — Causal graph seed
- [ ] `strategies[].edge_types` — Causal edge type filter
- [ ] `strategies[].max_depth` — Causal traversal depth
- [ ] `strategies[].analogical_alpha` — Analogical structural weight
- [ ] `cue_context` — Context for cue disambiguation

### Medium Priority: Core Operations
- [ ] **Forget:** Add `staleness_threshold_us`, `access_count_floor`, `memory_kind`, and `decay_score_floor`.
- [ ] **Subscribe:** Add `kind_filter`, `time_scope_us`, `output_buffer_size`, and `coarse_threshold`.
- [ ] **Revise:** Add `context_mode` (merge vs replace) and `edges` (add/update edges).
- [ ] **Prime:** Add `context` and `recency_window_us`.

### Low Priority: Reflect & Types
- [ ] **Reflect/Insights:** Add `since_us` and `min_confidence`.
- [ ] **Memory Type:** Add `device_id` and `logical_clock`.

---

## 2. TypeScript SDK (`@hebbs/sdk`) — Missing Fields

The TypeScript SDK already supports `RecallStrategyConfig` but is missing the following:

- [ ] **Forget:** Add `stalenessThresholdUs`, `accessCountFloor`, `memoryKind`, and `decayScoreFloor`.
- [ ] **Subscribe:** Add `kindFilter`, `timeScopeUs`, `outputBufferSize`, and `coarseThreshold`.
- [ ] **Revise:** Add `contextMode` and `edges`.
- [ ] **Prime:** Add `context` and `recencyWindowUs`.
- [ ] **Reflect/Insights:** Add `sinceUs` and `minConfidence`.
- [ ] **Memory Type:** Add `deviceId` and `logicalClock`.

---

## 3. E2E Validation & Quality (Both SDKs)

Address findings from `TEST_ANALYSIS_MAR9_TYPESCRIPT.md`:

- [ ] **Subscribe Investigation:** Debug why `subscribe` returns 0 pushes in E2E tests despite relevant feeds.
- [ ] **Scoring Assertions:** Update `ScoringWeights` tests to verify that weight changes actually affect result ordering (not just success).
- [ ] **Revise Round-trip:** Add a `revise` -> `get` test to verify content updates and `kind=revision` persistence.
- [ ] **Fresh Start Validation:** Run full E2E suites for both SDKs on a fresh server (`rm -rf hebbs-data`) to ensure clean state and correct insight generation.
- [ ] **Documentation:** Document that per-strategy `topK` is a strategy-level hint, not a response-level cap.

---

## Success Criteria

1. `hebbs-python` supports all 74 proto fields (currently 49/74).
2. `@hebbs/sdk` supports all 74 proto fields.
3. E2E test suites for both SDKs pass on a fresh server with 100% functional coverage.
4. Subscribe push delivery is verified and reliable.
