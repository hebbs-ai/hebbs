# PLAN-10: hebbs-bench-eval -- LongMemEval + LoCoMo Benchmark Harness

**Parent:** [TASK-10](../TASK-10-competitive-longmemevals-and-managed-cloud.md) (Gap 1: LongMemEvals Benchmark Score)
**Status:** Planned, not started
**Created:** 2026-03-19

---

## Context

HEBBS has no published benchmark scores. Competitors (HydraDB: 90% on LongMemEval, Mem0, Zep) do. For a research paper, we need to run HEBBS against both LongMemEval (ICLR 2025) and LoCoMo (ACL 2024) benchmarks with a 3-config ablation showing incremental value of each architectural layer.

Both benchmarks share the same pattern: ingest timestamped chat sessions, answer questions about them. One adapter serves both.

### Benchmark Summary

| | LongMemEval (ICLR 2025) | LoCoMo (ACL 2024) |
|---|---|---|
| Scale | 500 questions, ~115K tokens (S) or ~1.5M tokens (M) | ~1,500-2,000 QA from 50 conversations, ~9K tokens each |
| Categories | info extraction, multi-session, temporal, knowledge updates, abstention | single-hop, multi-hop, temporal, open-domain, adversarial |
| Input | Timestamped multi-session chat history | Timestamped multi-session dialogues |
| Output | JSONL `{question_id, hypothesis}` | Generated answers |
| Scoring | GPT-4o binary evaluation | Rule-based F1/EM |
| Difficulty | Harder (~38-76% for good systems) | Easier (~67-85% for good systems) |

### Published Baselines

- Full-context LLMs: ~30-40% on both
- Hierarchical (LiCoMemory): ~67-76%
- Event-centric (EMem-G): ~77-85%
- HydraDB claims 90% on LongMemEval

---

## Repo Structure

New repo `hebbs-bench-eval/` at the root of hebbs-repos.

```
hebbs-bench-eval/
  pyproject.toml
  README.md
  configs/
    hebbs_sim.toml              # similarity-only (vector DB baseline)
    hebbs_multi.toml            # + temporal + causal + analogical
    hebbs_full.toml             # + reflect + revise + contradiction + decay
    llm.toml                    # LLM provider for answer gen + eval
  src/bench_eval/
    __init__.py
    config.py                   # TOML config loader
    llm.py                      # LLM client (OpenAI GPT-4o)
    adapter/
      __init__.py
      core.py                   # HebbsAdapter wrapping HebbsClient
      strategies.py             # Strategy configs per ablation
    benchmarks/
      __init__.py
      base.py                   # Session, Turn, Question, BenchmarkRunner ABC
      longmemeval/
        __init__.py
        loader.py               # Parse longmemeval_s/m JSON
        runner.py               # LongMemEvalRunner
      locomo/
        __init__.py
        loader.py               # Parse locomo10.json
        runner.py               # LoCoMoRunner
    evaluation/
      __init__.py
      longmemeval_eval.py       # GPT-4o yes/no scoring
      locomo_eval.py            # F1/EM scoring
      metrics.py                # normalize_answer, f1, exact_match
    results/
      __init__.py
      aggregator.py             # Combine runs into comparison tables
      latex.py                  # Generate LaTeX table fragments
    cli.py                      # Click CLI entry point
  scripts/
    download_data.sh            # Fetch datasets from HuggingFace + GitHub
    run_ablation.sh             # Run all 6 combos (3 configs x 2 benchmarks)
  tests/
    conftest.py
    fixtures/                   # Sample data for unit tests
    test_loader_longmemeval.py
    test_loader_locomo.py
    test_adapter.py
    test_metrics.py
  results/                      # .gitignore'd output dir
  data/                         # .gitignore'd dataset dir
```

---

## Core Architecture

### HebbsAdapter (adapter/core.py)

Single class wrapping `HebbsClient` (Python SDK), used by both benchmark runners:

- `ingest_session(session)` -- each turn becomes one `remember()` call with `created_at` set to the original session timestamp (converted to microseconds). Context metadata includes session_id, turn_index, role.
- `ingest_all_sessions(sessions)` -- ingests all sessions, triggers `reflect_prepare` + LLM + `reflect_commit` every N sessions if enabled.
- `recall_for_question(question, question_date)` -- recalls using configured strategies, returns ranked results.
- `run_reflect_cycle()` -- two-step reflect (prepare -> LLM insight gen -> commit). Only in hebbs-full.
- `run_contradiction_cycle()` -- contradiction prepare -> LLM classification -> commit. Only in hebbs-full.
- `reset()` -- forget all memories for entity between benchmark instances.
- `format_context(results)` -- format recalled memories into context string for answer LLM.

### 3 Ablation Configs

| Config | Strategies | Reflect | Revise | Contradiction | Scoring Weights |
|--------|-----------|---------|--------|---------------|----------------|
| hebbs-sim | similarity only | no | no | no | relevance=1.0, rest=0.0 |
| hebbs-multi | similarity + temporal + causal + analogical | no | no | no | 0.5:0.2:0.2:0.1 |
| hebbs-full | all 4 strategies | yes (every 10 sessions) | yes (detect updates during ingest) | yes (after all sessions) | 0.5:0.2:0.2:0.1 |

The paper story: 3-config ablation across both benchmarks shows incremental value of each HEBBS architectural layer over vanilla vector search.

### Benchmark Runners

Both runners follow the same flow:
1. Load dataset -> list of BenchmarkInstance (sessions + questions)
2. For each instance: reset adapter, ingest sessions, answer questions
3. Format predictions for evaluation

**LongMemEval specifics:**
- 500 questions, each with ~40 (S) or ~500 (M) sessions
- Many questions share haystacks -- group by shared haystack to avoid re-ingestion
- Output: JSONL `{question_id, hypothesis}`
- Eval: GPT-4o binary scoring per category (6 categories + abstention)

**LoCoMo specifics:**
- 10 conversations, ~150-200 QA pairs each
- 5 categories: single-hop, multi-hop, temporal, open-domain, adversarial
- Output: generated answers
- Eval: stem-based F1 + exact match (rule-based, no GPT needed except adversarial)

### Answer Generation

Retrieved context + question -> GPT-4o generates answer:
```
Given the following relevant memories from past conversations:
{formatted_context}

Current date: {question_date}
Question: {question}
Answer concisely based only on the provided memories. If the information is not available, say so.
```

### Results & LaTeX

Aggregator combines all 6 runs into comparison matrices. LaTeX generator outputs:
1. Main results table (3 configs x 2 benchmarks, per-category)
2. Ablation delta table (sim -> multi -> full incremental gains)
3. Comparison vs published baselines (HydraDB, LiCoMemory, EMem, flat-bm25, etc.)

---

## Prerequisites

### Add created_at to remember()

Before building the harness, add optional `created_at` parameter to `remember()`. This lets the harness set original session dates, which is critical for temporal reasoning questions.

| Layer | File | Change |
|-------|------|--------|
| Proto | `hebbs/proto/hebbs.proto` | Add `optional uint64 created_at_us = N` to `RememberRequest` |
| Server | `hebbs-core/src/engine.rs` | If `created_at_us` is set, use it instead of `now_us()`. Default: current time. |
| Python SDK | `hebbs-python/src/hebbs/client.py` | Add `created_at: int | None = None` param to `remember()` |
| TypeScript SDK | `hebbs-typescript/src/index.ts` | Add `createdAt?: number` param to `remember()` |

Small, backward-compatible change. Existing callers unaffected.

---

## Key Design Decisions

1. **Python SDK over CLI** -- programmatic control, async, proper error handling
2. **Assume server running** -- user starts `hebbs serve` before running benchmarks. No process management in harness.
3. **OpenAI GPT-4o** -- single provider for answer gen and LongMemEval evaluation. Matches upstream eval protocol.
4. **Reimplemented evaluation** -- avoids upstream script dependencies (Dragon retriever, HF tokenizers), keeps repo self-contained
5. **Haystack grouping** -- critical for LongMemEval, avoids re-ingesting identical sessions across questions
6. **Revise detection in hebbs-full** -- during ingestion, lightweight similarity check identifies updates, LLM confirms, then revise(). This is what makes knowledge-update category work.

---

## CLI Usage

```bash
# Download datasets
./scripts/download_data.sh

# Run one config on one benchmark
bench-eval run --benchmark longmemeval_s --config configs/hebbs_sim.toml

# Evaluate predictions
bench-eval evaluate --results-dir results/ --benchmark longmemeval_s

# Generate LaTeX tables
bench-eval latex --results-dir results/

# Run full ablation (all 6 combos)
./scripts/run_ablation.sh
```

---

## Dependencies

```
hebbs>=0.3.0          # HEBBS Python SDK
click>=8.1            # CLI
rich>=13.0            # Progress bars
openai>=1.30          # Answer gen + LongMemEval eval
tomli>=2.0            # Config parsing (stdlib in 3.11+)
```

---

## Verification

1. Unit tests: loaders parse sample fixtures correctly, metrics match upstream implementations
2. Smoke test: run hebbs-sim on 5 LongMemEval instances, verify JSONL output format
3. Eval test: run evaluation on sample predictions, verify scores match manual calculation
4. Full run: all 6 combos, generate LaTeX tables, verify numbers are reasonable

---

## Estimated Costs (per full run)

| Dataset | Estimate |
|---------|----------|
| LongMemEval S (500 Q, ~40 sessions each) | ~$20-30 |
| LongMemEval M (500 Q, ~500 sessions each) | ~$50-100 |
| LoCoMo (10 convos, ~1500 QA) | ~$10-15 |
| **Total for full ablation (3 configs x 3 datasets)** | **~$150-300** |

---

## References

- LongMemEval: https://github.com/xiaowu0162/LongMemEval (ICLR 2025, https://arxiv.org/abs/2410.10813)
- LoCoMo: https://github.com/snap-research/locomo (ACL 2024, https://arxiv.org/abs/2402.17753)
- Papers running both benchmarks: LiCoMemory (Huang 2025), EMem (Zhou 2025), ENGRAM-R (Patel 2025)
