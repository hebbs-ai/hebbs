# TASK-27: Agent Memory Instructions Loop

**Status:** Open
**Priority:** High
**Created:** 2026-03-25

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

## Implementation

### Phase 1: Skill file integration

Update the hebbs SKILL.md to include the prime-before-recall pattern:

```markdown
## Before any recall operation

1. Prime: `hebbs recall "retrieval instructions" --strategy temporal --entity-id retrieval-instructions -k 20`
2. Read returned instructions
3. Match current query to an instruction pattern
4. Apply the specified strategy and flags
5. If no instruction matches, use default (similarity, k=10)
```

### Phase 2: Autotune command

Add `hebbs autotune` CLI command that:

1. Reads all files in the vault
2. Generates eval queries (like the agent did in run-tune1)
3. Runs baseline evals
4. Tunes strategies per query type
5. Stores discovered instructions as memories with `--entity-id retrieval-instructions`

```bash
hebbs autotune --eval-count 20
# Generates 20 evals, runs them, stores instructions
```

### Phase 3: Agent instruction update protocol

Define a protocol for agents to update instructions:

1. After every recall, agent checks: did the results answer the question?
2. If not, agent experiments with different strategies
3. If a better strategy is found, store it as a new instruction with `followed_by` edge
4. Periodically run `hebbs autotune --eval-count 50` to re-evaluate and prune stale instructions

## Architecture Notes

- Instructions are regular memories with `entity_id = "retrieval-instructions"`
- No new engine features needed -- uses existing temporal recall, entity_id, and edges
- The autotune command is a vault-level operation (not engine-level)
- Skill file is the integration point for agent behavior
- Instructions are vault-specific -- each vault develops its own retrieval strategies

## Success Criteria

- Agent can store retrieval instructions after tuning
- Agent can fetch instructions before recall (prime step)
- Updated instructions naturally supersede old ones via temporal ordering
- `hebbs autotune` generates and stores instructions from eval results
- Eval recall improves across sessions (instructions persist and compound)

## Dependencies

- Layer 3 entity extraction (done -- entity_id on propositions)
- Temporal recall on indexed content (done)
- Edge creation via `hebbs remember --edge` (done)

## Estimated Effort

- Phase 1 (skill file): 1 hour
- Phase 2 (autotune command): 4-6 hours
- Phase 3 (agent protocol): 2 hours documentation, agent behavior is emergent
