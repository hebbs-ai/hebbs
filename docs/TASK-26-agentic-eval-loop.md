# TASK-26: Agentic Eval Loop

**Status:** Design
**Priority:** High (GTM critical, core competitive edge)

## Summary

The eval loop is a skill, not a Hebbs feature. No new CLI commands. No new code in the Hebbs binary.

The agent already has everything it needs: `hebbs recall` to run queries, `hebbs list --sections` to know what's in the vault, and its own intelligence to generate evals, score results, reason about failures, and tune retrieval strategies.

The eval loop is a set of instructions in the SKILL.md that teaches any smart agent how to measure and optimize its own retrieval quality against a Hebbs vault.

## The Core Philosophy

**Hebbs is the instrument. The agent is the musician. The skill teaches the agent how to play.**

Hebbs stays dumb and fast. It exposes knobs (4 strategies, 4 scoring dimensions, tunable weights, edge traversal, ef-search, analogical alpha). The agent decides how to set them. The eval loop teaches the agent to measure whether its choices are working and improve them over time.

No new Hebbs infrastructure. No eval engine. No LLM inside Hebbs generating questions. The agent does all of it.

## Why This Matters

Two structural competitive edges depend on this:

1. **Agent-controlled retrieval**: Hebbs is the only memory engine that lets agents control retrieval parameters per query. Every competitor is a black box: the agent calls `memory.search("query")` and gets back whatever the system decides. With Hebbs, the agent chooses strategy, weights, traversal depth, edge types, ef-search quality, and analogical blending per query.

2. **Autotunable with evals**: Because the parameters are exposed, retrieval quality becomes measurable and optimizable. But the optimization should come from the agent, not from Hebbs, because agents reason about *why* queries fail, not just *which numbers score higher*.

Together these create a closed loop no competitor can enter: you can't reason about retrieval failures if you can't change retrieval parameters. Mem0 and Supermemory are locked out at step one.

## Why Agent Intelligence, Not Hebbs Intelligence

We considered and rejected three alternatives:

### Rejected: `hebbs eval generate` (Hebbs LLM generates evals)

Hebbs uses Haiku-class models for proposition extraction and contradiction classification. That's extraction work, not reasoning. Generating high-quality evals requires understanding what questions a real user would ask, what makes a query hard, which document combinations would expose retrieval weaknesses, and what edge cases would break similarity search but work with temporal or causal strategies. That's Opus/Sonnet-level thinking. The agent the user is already talking to is smarter than anything Hebbs would run internally.

### Rejected: `hebbs eval run` (Hebbs batch-runs queries and scores them)

The agent can already run `hebbs recall` in a loop and compare returned memory IDs against its own expected list. A batch command saves a few seconds of overhead but adds surface area to the binary for no real value. The agent computes precision/recall itself. It's simple arithmetic.

### Rejected: `hebbs eval` as any CLI command

Every eval capability reduces to things the agent can already do with existing Hebbs commands. Adding eval commands to Hebbs means maintaining code, testing edge cases, and documenting features that duplicate what a smart agent does naturally. Zero new code is the right answer.

## The Eval Loop (Skill-Based)

The entire loop lives in the SKILL.md as instructions the agent follows:

```
1. Agent reads the vault
   hebbs list --sections --format json
   Agent now knows every file, section, and memory ID in the vault.

2. Agent generates eval queries
   Using its understanding of the documents, the agent writes
   question/answer pairs:
   - "What is our ransomware coverage?" should return [ID-A, ID-B, ID-C]
   - "What changed in risk posture since Q4?" should return [ID-X, ID-Y, ID-Z]
   - "Why is Meridian flagged as critical?" should return [ID-P, ID-Q, ID-R]

   The agent reasons about query types: factual, temporal, causal,
   cross-entity, contradiction. It generates hard queries that would
   expose retrieval weaknesses, not just easy lookups.

3. Agent runs each query through hebbs recall
   For each eval query:
     result = hebbs recall "query" --strategy X --weights X --format json

   Agent compares returned memory IDs against expected IDs.
   Computes precision (how many returned were correct) and
   recall (how many correct were returned).

4. Agent analyzes failures
   For each query where recall was low, the agent reasons:
   - "Query 14 returned the MSA but missed the amendment.
      This is a temporal question. Similarity found the topic
      but not the update. Temporal strategy would catch it."
   - "Query 7 found the amendment but missed the stale risk
      assessment. Broader top-k with importance weighting needed."
   - "Query 22 asked about cross-vendor patterns. Analogical
      strategy would find structural similarities."

5. Agent adjusts and re-runs
   Agent changes strategy/weights per query type and re-runs.
   Compares before/after scores.

6. Agent persists what it learned
   Option A: hebbs remember "for temporal questions about risk
     evolution, use temporal strategy with weights 0.2:0.8:0:0"
     --importance 0.9
   Option B: Agent updates its own SKILL.md or config with
     learned retrieval patterns
   Option C: Both

7. Next conversation
   Agent recalls its stored strategies before making recall calls.
   Gets smarter at retrieval over time without any manual tuning.

8. Periodic re-evaluation
   Agent re-runs the eval suite periodically (or when the vault
   changes significantly) to catch drift and re-tune.
```

## What the Agent Learns (Example Output)

After running the eval loop on the enterprise legal vault, an agent might learn and store:

**Temporal questions** ("what changed", "timeline of", "since Q4"):
- Use `--strategy temporal --weights 0.2:0.8:0:0`
- Requires `--entity-id` to scope to the right entity
- Always request `--top-k 15` to capture full evolution

**Contradiction questions** ("are there conflicts", "is this consistent"):
- Run two recalls: broad similarity first (`--top-k 20`), then inspect results for conflicting propositions
- Weight importance high (`--weights 0.4:0.1:0.4:0.1`) because contradictions tend to live in high-importance documents

**Causal questions** ("why is X critical", "what caused", "trace the chain"):
- Use `--strategy causal --max-depth 4 --edge-types caused_by,followed_by`
- Start from the most relevant memory found via similarity, then traverse

**Cross-entity questions** ("across all vendors", "common patterns"):
- Use `--strategy analogical --analogical-alpha 0.3`
- Low alpha biases toward structural similarity (same entity roles, same edge patterns) rather than text similarity

**Synthesis questions** ("are we audit-ready", "launch blockers"):
- Run multiple targeted recalls with different strategies
- Combine results, deduplicate, synthesize
- Weight importance high to surface policy documents over meeting notes

## SKILL.md Addition

Add this section to the SKILL.md:

```markdown
## Self-tuning: the eval loop

When asked to optimize retrieval, or when recall quality seems poor:

1. Read the vault: `hebbs list --sections --format json`
2. Generate 20-50 eval queries with expected memory IDs based on your
   understanding of the indexed documents. Include a mix of query types:
   factual, temporal, causal, cross-entity, contradiction.
3. Run each query: `hebbs recall "query" --strategy X --weights X --format json`
4. Compare returned memory IDs against expected. Compute precision and recall.
5. For each failure, reason about WHY the memory was missed:
   wrong strategy? wrong weights? top-k too low? wrong entity scope?
6. Adjust strategy and weights per query type. Re-run. Compare.
7. Store learned strategies:
   `hebbs remember "for temporal questions use strategy temporal
   with weights 0.2:0.8:0:0" --importance 0.9`
8. Use stored strategies in all future conversations.
9. Re-run evals periodically to catch drift after vault changes.

You are the intelligence. Hebbs is the instrument. Tune it.
```

## What This Enables for GTM

### The demo moment

The viewer watches an agent (Claude Code, Codex, or OpenClaw):

1. Index 52 legal documents
2. Answer questions, choosing strategies per query type
3. User says "optimize your retrieval"
4. Agent generates 30 evals from its understanding of the vault
5. Agent runs them, scores 71% precision
6. Agent analyzes 8 failures, reasons about each one out loud
7. Agent adjusts strategies per query type, re-runs, scores 94%
8. Agent stores what it learned in Hebbs memory
9. New conversation: agent uses learned strategies automatically

The viewer sees: the agent taught itself how to remember better, with measurable proof, in 60 seconds. No human tuning. No configuration files. No new tools. Just a smart agent with a controllable engine.

### The pitch line

"Mem0 and Supermemory are products you use. Hebbs is infrastructure you tune. And your agent does the tuning."

### Enterprise value

- VP of Engineering: "The agent optimized retrieval for our domain with measurable precision improvement. We can re-run this eval suite to catch regressions."
- Security team: "Deterministic, auditable retrieval. Same weights, same results."
- ML team: "This is the train/eval loop we already understand, applied to memory retrieval."

### Competitive positioning

No competitor can enter this loop:
- Step 1 (expose parameters): only Hebbs does this
- Step 2 (agent generates evals): requires exposed parameters to vary
- Step 3 (agent tunes): requires exposed parameters to change
- Step 4 (agent persists strategies): requires a memory system that stores strategies (Hebbs storing knowledge about itself)

The flywheel: agent uses Hebbs -> agent evaluates Hebbs -> agent tunes Hebbs -> agent stores tuning in Hebbs -> agent uses tuning from Hebbs -> repeat. The engine gets better because the agent remembers how to use it better. That's Hebbs remembering how to be Hebbs.

## Implementation Scope

### Phase 1: SKILL.md update (zero code)

Add the self-tuning section to SKILL.md. This is all that's needed for the demo and for enterprise conversations. The agent does everything with existing commands.

### Phase 2: Eval suite format specification (documentation only)

Publish a recommended JSON format for eval suites so teams can version, share, and run them in CI (via agent scripts, not Hebbs CLI):

```json
{
  "version": 1,
  "vault": "/path/to/vault",
  "evals": [
    {
      "id": "eval-001",
      "query": "What is our ransomware coverage limit?",
      "query_type": "factual_contradiction",
      "expected_memory_ids": ["01ABC...", "01DEF...", "01GHI..."],
      "strategy": "similarity",
      "weights": "0.5:0.2:0.2:0.1",
      "top_k": 10
    }
  ]
}
```

This is a spec, not a feature. Any agent can read and execute it.

### Phase 3: Community benchmark (if eval gains traction)

If the eval loop proves valuable, publish a benchmark methodology and dataset. Let people compare memory systems using a standardized eval suite. Hebbs wins because it's the only one with tunable parameters. This positions Hebbs as the definer of "how to measure memory quality," not just a competitor.

## Non-Goals

- No new CLI commands in Hebbs. Zero new code.
- No LLM intelligence inside Hebbs for eval generation.
- No eval UI or dashboard.
- No batch runner. The agent runs recall in a loop.
- No strategy storage mechanism in Hebbs. The agent stores strategies as Hebbs memories or in SKILL.md.

## The Deeper Insight

The eval loop is Hebbs remembering how to be Hebbs. The agent stores retrieval strategies as memories in the same engine it's optimizing. When the agent recalls "for temporal questions use strategy temporal with weights 0.2:0.8:0:0" before making a recall call, it's using Hebbs to make Hebbs better. The engine's own memory system improves how the engine is used. No new infrastructure needed. The architecture already supports it.
