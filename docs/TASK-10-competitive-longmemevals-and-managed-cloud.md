# TASK-10: LongMemEvals Benchmark

**Created:** 2026-03-13
**Source:** Competitive analysis of HydraDB (Cortex) at hydradb.com

---

## The Problem

HydraDB claims **90% accuracy on LongMemEvals** (ICLR 2025 benchmark). This is their single strongest marketing asset. LongMemEvals tests five core long-term memory abilities:

1. **Information extraction** -- can the system extract and retain key facts from conversations?
2. **Multi-session reasoning** -- can it reason across information spread over multiple sessions?
3. **Temporal reasoning** -- can it understand and reason about the order and timing of events?
4. **Knowledge updates** -- can it handle contradictions and updated information correctly?
5. **Abstention** -- does it know when it doesn't have enough information to answer?

HEBBS has no published LongMemEvals score. Enterprise buyers and technical evaluators checking benchmarks will find HydraDB with a number and HEBBS without one. This is a credibility gap regardless of actual capability.

## Why HEBBS Should Win This

HEBBS has architectural advantages on at least 3 of the 5 dimensions:

- **Temporal reasoning:** Native temporal index (B-tree on entity_id + timestamp) with dedicated temporal recall strategy. +68% precision vs similarity-only on temporal queries.
- **Knowledge updates:** `revise()` operation with lineage tracking. Predecessor edges in graph. Decay system that naturally deprioritizes stale information.
- **Multi-session reasoning:** Causal graph traversal + analogical recall + reflect pipeline that consolidates cross-session patterns into insights.

The reflect pipeline (clustering + LLM consolidation) should give HEBBS a strong edge on abstention too -- insights have confidence scores, and the system can distinguish "I have high-confidence knowledge" from "I have noisy, low-confidence fragments."

## Action Items

1. **Set up LongMemEvals evaluation harness.** Clone https://github.com/xiaowu0162/LongMemEval. Understand the evaluation protocol, dataset format, and scoring methodology.

2. **Build a HEBBS adapter for LongMemEvals.** The benchmark expects a chat assistant interface. Build a thin wrapper that:
   - Uses `remember()` to store conversation turns
   - Uses `recall()` (multi-strategy) to retrieve context for each evaluation question
   - Uses `reflect()` periodically to consolidate sessions
   - Uses `revise()` when knowledge updates are detected
   - Responds with retrieved context + LLM generation

3. **Run baseline evaluation.** Test with:
   - Similarity-only recall (to match what vector DBs do)
   - Multi-strategy recall (similarity + temporal + causal)
   - Multi-strategy + reflect (the full HEBBS stack)

   This gives three scores showing the incremental value of each HEBBS capability.

4. **Optimize for weak dimensions.** If any of the 5 sub-scores are below 85%, investigate and improve. Likely candidates:
   - Abstention may need explicit confidence thresholds on recall results
   - Knowledge updates may need tighter integration between `revise()` and recall ranking

5. **Publish results.** Add to README, website, and docs. If score > 90%, lead with it. If 85-90%, lead with sub-dimension breakdowns where HEBBS wins (temporal, knowledge updates). If < 85%, fix before publishing.

## Success Criteria

- Published LongMemEvals score >= 90% overall
- Published per-dimension breakdown showing HEBBS advantages
- Reproducible evaluation script in repo (so others can verify)

## Relationship to Existing Tasks

- TASK-02 (killer demo strategy) -- LongMemEvals score strengthens the demo narrative
- TASK-08 (prime + partitioned HNSW) -- benchmark results may inform whether HNSW optimization is needed
