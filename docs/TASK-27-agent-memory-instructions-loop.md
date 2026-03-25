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

After an agent runs evals and discovers what works, it stores crisp retrieval instructions as memories:

```bash
hebbs remember "RETRIEVAL-INSTRUCTION: For temporal queries about vendor risk evolution, \
  use --weights 0.3:0.5:0.2:0 -k 10 with entity names in the cue" \
  --importance 0.95 --entity-id retrieval-instructions
```

Key rules:
- All retrieval instructions use `--entity-id retrieval-instructions` so they're grouped as one entity
- Prefix with `RETRIEVAL-INSTRUCTION:` for easy filtering
- Set high importance (0.9+) so they don't decay
- Include the query pattern, the strategy, and the flags
- Store the *why* not just the *what* ("because k=5 misses supporting details in dense legal docs")

### 2. Prime: Fetch instructions before recall

Before any recall operation, the agent primes itself by fetching stored retrieval instructions:

```bash
hebbs recall "retrieval instructions" \
  --strategy temporal --entity-id retrieval-instructions \
  --format json -k 20
```

This returns all stored instructions in chronological order. The agent reads them and applies the matching strategy for its current query.

The prime step should be:
- Automatic (skill file triggers it before any recall)
- Lightweight (temporal on a single entity is O(log n + k), fast)
- Cached per session (fetch once at start, not before every recall)

### 3. Agents update instructions based on new evidence

When an agent encounters a query where existing instructions don't work, it:

1. Tries the instructed strategy
2. If results are poor, experiments with alternatives
3. Stores an updated instruction with a `followed_by` edge to the old one

```bash
# Old instruction was k=10, but this vault needs k=15
hebbs remember "RETRIEVAL-INSTRUCTION: For this vault, use k=15 instead of k=10 \
  because proposition density is 20+ per file" \
  --importance 0.95 --entity-id retrieval-instructions \
  --edge OLD_INSTRUCTION_ID:followed_by:0.9
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
