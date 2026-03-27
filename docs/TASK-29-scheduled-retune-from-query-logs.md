# TASK-29: Scheduled Retune from Query Logs

**Status:** Open
**Priority:** Medium
**Created:** 2026-03-27

## Problem

After the initial tune session (tune/SKILL.md), retrieval rules are exported to `.hebbs/retrieval-rules.md` and the agent uses them daily. Over time, the vault changes (new files, updated content), query patterns shift, and the rules drift. Companies need a scheduled retune cycle, not real-time opportunistic fixing.

The agent should not experiment with strategies mid-conversation. It should use the rules file predictably, then retune on a schedule.

## The Retune Cycle

```
Week 1-4: Agent uses rules, query log accumulates
    |
    v
End of month: Pull query logs, identify failures
    |
    v
Generate new evals from real failures
    |
    v
Run tune/SKILL.md with expanded eval set
    |
    v
Re-export .hebbs/retrieval-rules.md
    |
    v
Next month: repeat
```

## Prerequisites

### Build HEBBS from source

```sh
cd hebbs/
cargo build --release
# Binary at: target/release/hebbs
# Or install globally:
cargo install --path crates/hebbs-vault
```

### Init and index a vault

```sh
hebbs init /path/to/vault --provider openai --key $OPENAI_API_KEY
hebbs index /path/to/vault
```

### Run initial tune

Follow `hebbs-skill/tune/SKILL.md` phases 1-7. This produces:
- Baseline and tuned eval scores
- Compressed retrieval rules stored in HEBBS
- Exported `.hebbs/retrieval-rules.md`

## Step 1: Pull Query Logs

```sh
hebbs queries --limit 200 --format json
```

Returns recent queries with metadata: cue text, strategy used, result count, caller, timestamp.

Review the logs for:
- **Low result count** (0-2 results returned): query may need better cue expansion or higher k
- **Repeated queries on the same topic**: user keeps asking because results are unsatisfying
- **Queries from new content areas**: vault grew, rules don't cover the new domain

## Step 2: Identify Failures

The agent (Claude Code, Cursor, etc.) reads the query logs and classifies them:

| Pattern | Signal | Action |
|---|---|---|
| 0 results returned | Query completely missed | Turn into eval, investigate why |
| User asked same topic 2-3 times | First results were wrong | Turn the refined query into an eval |
| Query on content added after last tune | Rules don't cover new files | Generate evals for new content area |
| Query type not in current rules | Missing strategy rule | Add eval for this query type |

## Step 3: Generate New Evals from Failures

For each identified failure, the agent creates an eval:

```
Q[N]: "[the actual query from the log, or a cleaned version]"
  Expected: [keywords the user was looking for, inferred from the query and context]
  Type: factual_lookup | entity_scoped | temporal | cross_entity | causal | recency_weighted
```

Combine with the existing eval set from the previous tune session. The eval set grows over time.

### Sizing

| Retune cycle | Existing evals | New from logs | Total |
|---|---|---|---|
| First retune (month 1) | 20-50 | 10-20 | 30-70 |
| Second retune (month 2) | 30-70 | 5-15 | 35-85 |
| Steady state | 50-100 | 5-10 | 55-110 |

New evals from logs are the most valuable because they represent real user needs, not synthetic test cases.

## Step 4: Run Tune Again

Follow `hebbs-skill/tune/SKILL.md` phases 3-6 with the expanded eval set:

1. Run baseline on ALL evals (existing + new)
2. Compare to previous tune session scores
3. Tune the new failures (existing rules should still pass)
4. Store new learnings as RETRIEVAL-INSTRUCTION memories

### Watch for regressions

If existing evals that previously scored 100% now score lower:
- Vault content changed (file deleted, section rewritten)
- New content diluted the search space (more memories, same k)
- Fix: update the eval's expected keywords, or increase k in the rule

## Step 5: Re-compress and Re-export

1. Load all instructions: `hebbs prime retrieval-instructions --max-memories 50 --format json`
2. Merge new learnings with existing master rules
3. Compress back to 10-20 rules
4. Delete granular strategies
5. Re-export to `.hebbs/retrieval-rules.md`

The rules file is the single artifact that changes. The agent picks it up automatically next conversation.

## Step 6: Save Outputs

Each retune cycle produces three persistent artifacts:

### 1. Updated rules file

```
.hebbs/retrieval-rules.md          <- per-vault (or ~/.hebbs/retrieval-rules.md for global)
```

This is what the agent loads every conversation. It's the only artifact that affects daily behavior.

### 2. Eval results file

Save the full eval results to `.hebbs/tune-history/run-N.json` (or `.md`):

```sh
mkdir -p .hebbs/tune-history
```

Each run file contains:
- Date, LLM, embedding model, memory count
- All eval queries with expected keywords
- Baseline scores per query
- Tuned scores per query
- Failure analysis (which pattern, which fix)
- New evals added from query logs (with source query IDs)

This is the evidence trail. Without it, you can't compare runs or debug regressions.

### 3. Scorecard (appended each cycle)

Save to `.hebbs/tune-history/SCORECARD.md`:

```markdown
# Tune Scorecard

| Run | Date       | Evals | Baseline | Tuned | Delta | New from logs | Rules |
|-----|------------|-------|----------|-------|-------|---------------|-------|
| 1   | 2026-03-25 | 20    | 54%      | 84%   | +30pp | n/a           | 12    |
| 2   | 2026-04-25 | 35    | 72%      | 88%   | +16pp | 15            | 14    |
| 3   | 2026-05-25 | 42    | 80%      | 91%   | +11pp | 7             | 15    |

Top failure patterns (current):
1. Dollar amounts not extracted as propositions
2. Entity name variants (Cloudvault vs Cloudvault Systems, LLC)

Rules file: .hebbs/retrieval-rules.md
```

### Output summary

```
.hebbs/
  retrieval-rules.md              <- live rules (agent reads this)
  tune-history/
    SCORECARD.md                  <- one table, appended each cycle
    run-1.md                      <- full eval results, first tune
    run-2.md                      <- full eval results, first retune
    run-3.md                      <- full eval results, second retune
```

Expected pattern over time:
- Baseline improves each cycle (rules from last session become the new floor)
- Delta shrinks (diminishing returns per tune session)
- Eval count grows (real failures added each cycle)
- Steady state: 85-92% baseline, 90-95% tuned

## When to Retune

| Trigger | Why |
|---|---|
| Monthly schedule | Regular maintenance, catches drift |
| Significant vault change (20+ new files) | New content not covered by existing rules |
| User reports retrieval miss | Specific failure to investigate |
| LLM or embedding model change | Extraction and similarity behavior changes, re-index + retune |
| Baseline drops below 75% on existing evals | Rules are stale |

## Architecture Notes

- Query log is already built (`hebbs queries` command, `[query_log]` config section)
- No new engine features needed
- The agent does the analysis, not HEBBS
- Retune is a human-scheduled event, not automated
- The tune skill (`hebbs-skill/tune/SKILL.md`) is the execution framework; this task defines when and why to run it

## Dependencies

- [x] Query logging (`hebbs queries` command)
- [x] Tune skill (`hebbs-skill/tune/SKILL.md`)
- [x] Rules file export (`.hebbs/retrieval-rules.md`)
- [x] Main SKILL.md loads rules file at conversation start
- [ ] End-to-end test of the full retune cycle on a real vault

## Related

- TASK-27: Agent Memory Instructions Loop (the store/prime/compress design)
- TASK-28: Install and Init UX (the setup flow)
- `hebbs-skill/tune/SKILL.md`: The tune execution framework
- `docs/marketing/run-tune2/RUN-LOG.md`: Run 2 results
- `docs/marketing/run-tune3/RUN-LOG.md`: Run 3 results
