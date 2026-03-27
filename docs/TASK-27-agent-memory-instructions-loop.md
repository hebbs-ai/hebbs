# TASK-27: Agent Memory Instructions Loop

**Status:** Done (end-to-end testing moved to TASK-29)
**Priority:** High
**Created:** 2026-03-25
**Updated:** 2026-03-27

## Problem

Today, retrieval tuning knowledge is scattered. An agent discovers that `--weights 0.3:0.5:0.2:0` works for temporal queries, but that knowledge lives in the conversation context or gets lost. There's no systematic loop where:

1. The agent tunes retrieval strategies
2. Stores what it learned as memories
3. Retrieves those instructions before future recall operations
4. Updates instructions based on new evidence

## Design

Three components working together:

### 1. Autotune: Store retrieval instructions in memory

After an agent runs evals and discovers what works, it stores crisp retrieval instructions as memories. All instructions share a single entity_id so they can be loaded in one call.

#### How to save

```bash
hebbs remember "RETRIEVAL-INSTRUCTION: Default to k=10. Use k=5 only for simple factual lookups. k=5 misses supporting details in dense docs." --importance 0.95 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: For timeline/evolution queries, use --weights 0.3:0.5:0.2:0 with k=10. Include dates and entity names in cue." --importance 0.95 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: For cross-entity comparison queries, use --strategy analogical --analogical-alpha 0.3 with k=10." --importance 0.95 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: Always expand cues with entity names, dates, and domain terms. 'Cloudvault Systems vendor risk Q2 2025' beats 'vendor risk'." --importance 0.95 --entity-id retrieval-instructions --format json
```

Key rules:
- All retrieval instructions use `--entity-id retrieval-instructions` so they're grouped as one entity
- Prefix with `RETRIEVAL-INSTRUCTION:` for easy filtering
- Set high importance (0.95) so they don't decay
- Include the query pattern, the strategy, and the flags
- Include the *why* not just the *what* ("because k=5 misses supporting details in dense legal docs")
- Each instruction is a single `hebbs remember` call on one line (no line breaks)

#### Why this works

These are manually stored memories with an explicit `--entity-id`. Entity_id is guaranteed to be set. This is different from auto-extracted propositions from files, where entity_id assignment depends on the LLM extraction quality and may be null.

### 2. Prime: Fetch instructions before recall

At conversation start, the agent loads all stored instructions in one call:

#### How to retrieve

**Option A: prime (recommended)**

```bash
hebbs prime retrieval-instructions --max-memories 20 --format json
```

Loads up to 20 memories scoped to the `retrieval-instructions` entity. One call, all rules.

**Option B: temporal recall**

```bash
hebbs recall "retrieval instructions" --strategy temporal --entity-id retrieval-instructions -k 20 --format json
```

Returns instructions in chronological order. Newest (updated) rules come last.

**Option C: importance-weighted recall**

```bash
hebbs recall "retrieval instructions" --entity-id retrieval-instructions --weights 0.3:0.1:0.5:0.1 -k 20 --format json
```

Returns highest-importance instructions first.

#### When to retrieve

- Once at conversation start (cache for the session)
- NOT before every recall call (wasteful)
- The skill file should trigger this automatically as part of the conversation startup loop

### 3. Agents update instructions based on new evidence

When an agent encounters a query where existing instructions don't work, it:

1. Tries the instructed strategy
2. If results are poor, experiments with alternatives
3. Stores an updated instruction with a `followed_by` edge to the old one

```bash
# Old instruction was k=10, but this vault needs k=15
hebbs remember "RETRIEVAL-INSTRUCTION: For this vault, use k=15 instead of k=10 because proposition density is 20+ per file" --importance 0.95 --entity-id retrieval-instructions --edge OLD_INSTRUCTION_ID:followed_by:0.9 --format json
```

The `followed_by` edge creates a revision chain. The temporal strategy returns the latest instruction first, so updated instructions naturally supersede old ones.

## Compression Step

After multiple tuning sessions, the agent accumulates dozens of individual instructions. Too many to load every conversation. The agent should periodically compress them:

1. Load all: `hebbs prime retrieval-instructions --max-memories 50 --format json`
2. Analyze patterns: what's universal vs domain-specific? What overlaps?
3. Write 10-20 compressed rules that cover the general cases
4. Store compressed rules at importance 0.95 with `--entity-id retrieval-instructions`
5. Delete granular strategies: `hebbs forget --ids <OLD_ID_1>,<OLD_ID_2>,...`

See `docs/marketing/run-tune2/AUTOTUNE-ONLY-SKILL.md` sections 7-8 for the full compression protocol.

## Eval Scoring Method

Strict keyword matching against top-k concatenated results. No fuzzy matching, no semantic similarity, no partial credit. A keyword is either present in the returned content or it's missed.

- Baseline: k=5, default strategy (similarity)
- Tuned: k=10, strategy/weight selection per query type, entity names in cues

### Results across runs

| Run | LLM | Embedding | Baseline | Tuned |
|---|---|---|---|---|
| Run 1 (0.3.0) | gpt-4o-mini | local 768d | 59% | 88% |
| Run 2 (0.3.1) | gpt-4o-mini | local 768d | 54% | 84% |
| Run 3 (0.3.1) | gpt-4o | OpenAI 1536d | 75% | 90% |

Full logs: `docs/marketing/run-tune2/RUN-LOG.md`, `docs/marketing/run-tune3/RUN-LOG.md`

## Implementation

### Phase 1: Skill file integration (done)

The main SKILL.md (`hebbs/skills/hebbs/SKILL.md`) conversation startup loop now includes retrieval rules loading with a three-tier fallback:

1. Check if `.hebbs/retrieval-rules.md` exists: read it directly (fastest, no daemon call)
2. If not: `hebbs prime retrieval-instructions --max-memories 20 --format json`
3. If neither exists: use defaults (similarity, k=10)

The rules file is the compiled output of the tune loop, generated by `hebbs-skill/tune/SKILL.md` Phase 7. It lives at `.hebbs/retrieval-rules.md` (per-vault) or `~/.hebbs/retrieval-rules.md` (global).

### Phase 2: Agent-driven eval loop (done, documented)

Originally planned as a `hebbs autotune` CLI command. Replaced by agent-driven documentation and a dedicated tune skill:

- `hebbs-skill/tune/SKILL.md`: The canonical tune skill (8 phases: profile, generate, baseline, tune, store, compress, export, re-tune)
- `docs/marketing/run-tune2/AUTOTUNE-ONLY-SKILL.md`: Marketing version of the eval loop
- `docs/marketing/run-new-client1/HOW_TO_TUNE.md`: Client-facing tuning guide with ICP profiling
- `docs/marketing/run-tune2/HOW-TO-EVAL.md`: Step-by-step eval methodology

The agent (Claude Code, Cursor, etc.) runs the loop manually using `hebbs list`, `hebbs recall`, and `hebbs remember`. A built-in `hebbs autotune` command is no longer needed; the agent IS the autotune engine.

### Phase 3: Agent instruction update protocol (in progress)

The protocol is defined in the AUTOTUNE-ONLY-SKILL.md (sections 6-9) and HOW_TO_TUNE.md. What's still missing:

1. **No real-world test of the full loop.** We've stored instructions manually and verified recall works (run-tune2/3), but no agent has completed the full cycle: prime rules at conversation start, use them for recall, discover a failure, store an updated rule, verify it persists.
2. **Compression not tested.** The compress-to-rules protocol is documented but never executed on real accumulated strategies.

## What Changed Since TASK-27 Was Written

### TASK-28 fixes (commit `5c4578c`)

These changes affect how TASK-27 examples should be written:

1. **`--key` flag replaces `--api-key-env` for interactive use.** Init is now `hebbs init . --provider openai --key $OPENAI_API_KEY`. `--api-key-env` still works for CI/pipelines.
2. **Auto-embedding for OpenAI.** When provider is openai, embedding auto-configures to text-embedding-3-small (1536 dims) with the same key. No manual embedding config needed.
3. **Default models per provider.** `--model` is optional: openai defaults to gpt-4o-mini, anthropic to claude-haiku-4-5-20251001, gemini to gemini-2.0-flash, ollama to gemma3:1b.
4. **No local model download for OpenAI users.** Embedding model download is skipped when API embeddings are configured.
5. **Global config inheritance works.** LLM + embedding config saved to `~/.hebbs/config.toml` once, inherited by all project vaults.

### Run-tune3 findings

See `docs/marketing/run-tune3/RUN-LOG.md`:
- OpenAI embeddings are the biggest recall lever (+21pp baseline over local)
- GPT-4o entity extraction regressed (all null entity_ids)
- Recommended config: gpt-4o-mini for extraction + OpenAI embeddings for recall quality
- Daemon file watch works: edit a file, new content recallable in ~38 seconds

## Architecture Notes

- Instructions are regular memories with `entity_id = "retrieval-instructions"`
- No new engine features needed; uses existing prime, temporal recall, entity_id, and edges
- Skill file is the integration point for agent behavior
- Instructions are vault-specific; each vault develops its own retrieval strategies
- Instructions use manually set entity_id, NOT auto-extracted entity_ids (which may be null depending on LLM)

## Success Criteria

- [x] Agent can store retrieval instructions after tuning
- [x] Agent can fetch instructions before recall (prime step)
- [x] Updated instructions naturally supersede old ones via temporal ordering + followed_by edges
- [ ] Compression tested on real accumulated strategies
- [ ] Full loop tested end-to-end (prime rules, use them, discover failure, store update, verify persistence)
- [x] Works regardless of LLM extraction quality (manual entity_id, not dependent on auto-extraction)
- [x] Eval loop documented for client-facing use

## Dependencies

- [x] `hebbs remember --entity-id` (done)
- [x] `hebbs prime <entity_id>` (done)
- [x] `hebbs remember --edge` for revision chains (done)
- [x] `hebbs forget --ids` for cleanup after compression (done)
- [x] `--key` flag for clean init (done, TASK-28)
- [x] Auto-embedding for OpenAI (done, TASK-28)

## Known Issues

- **Entity extraction regression with GPT-4o:** Run-tune3 showed all auto-extracted entity_ids are null with 4o. Does NOT affect the instruction loop (manually stored entity_ids work), but breaks temporal recall on indexed file content.
- **Recommended config:** gpt-4o-mini for extraction + OpenAI embeddings. See run-tune3 section 12.
- **macOS Intel (x86_64) not supported.** TASK-28 Issue 2, still open.

## Related Docs

| Doc | What it covers |
|---|---|
| `docs/marketing/run-tune2/AUTOTUNE-ONLY-SKILL.md` | Full eval loop skill (9 sections) |
| `docs/marketing/run-new-client1/HOW_TO_TUNE.md` | Client-facing tuning guide with ICP profiling |
| `docs/marketing/run-new-client1/GET_STARTED.md` | Client onboarding (install through tune) |
| `docs/marketing/run-tune2/HOW-TO-EVAL.md` | Eval methodology |
| `docs/marketing/run-tune2/RUN-LOG.md` | Run 2 results (4o-mini, local embed) |
| `docs/marketing/run-tune3/RUN-LOG.md` | Run 3 results (4o, OpenAI embed) |
