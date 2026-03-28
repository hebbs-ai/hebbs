# HEBBS Cloud: Tuning

## Overview

Tuning is not a HEBBS feature. There are no tune commands, no tune API, no tune service. Tuning is a **skill** — a set of instructions that the customer's AI agent follows using the existing HEBBS primitives (`recall`, `remember`, `forget`, `prime`).

The tune skill lives at `hebbs-skill/tune/SKILL.md`. Any agent that can read instructions and call HEBBS can tune.

---

## How it works

```
Customer's agent (Claude, GPT, etc.)
    │
    │  Loads tune/SKILL.md into context
    │
    │  Uses only existing HEBBS commands:
    │  ├── recall     → test queries, measure quality
    │  ├── remember   → store retrieval instructions
    │  ├── forget     → clean up old instructions
    │  └── prime      → load instructions at conversation start
    │
    │  No special tune API. No tune CLI commands.
    │  Just the skill + existing primitives.
    │
    ▼
HEBBS (unchanged, doesn't know tuning is happening)
```

---

## The tune skill process

The skill defines 8 phases. The agent drives all of them using regular HEBBS calls via CLI or SDK:

### Phase 1: Profile the client
The agent asks the customer about their domain, search patterns, hardest queries. Classifies into an ICP (Legal, Sales, Engineering, Research).

Stores the profile:
```python
hb.remember(
    "CLIENT-PROFILE: Domain is legal-compliance. Primary search patterns: factual-lookup, entity-scoped, contradiction. Classification: legal.",
    entity_id="retrieval-instructions",
    importance=0.9,
)
```

### Phase 2: Generate evals
The agent generates 20-50 test queries based on the domain and indexed content. Each eval has a query, expected keywords, and query type.

### Phase 3: Run baseline
The agent runs each eval using regular `recall`:
```python
result = hb.recall("What is our ransomware coverage limit?", k=5)
# Score: count expected keywords in results
```
Reports baseline to the customer.

### Phase 4: Tune
For each query below 100%, the agent tries variations — increase k, expand cue, try different strategies and weights. All using regular `recall` calls with different parameters.

### Phase 5: Store learnings
The agent stores each winning strategy as a retrieval instruction:
```python
hb.remember(
    "RETRIEVAL-INSTRUCTION: For compliance/audit queries, always expand acronyms and include the vendor name in the cue. Use k=10 minimum.",
    entity_id="retrieval-instructions",
    importance=0.9,
)
```

### Phase 6: Compress
After multiple tune sessions, the agent recalls all instructions, groups them, writes master rules, and forgets the granular ones.

### Phase 7: Export to markdown
The agent compiles all master rules into a `.hebbs/retrieval-rules.md` file that loads into the agent's context before any HEBBS calls.

### Phase 8: Re-tune
When vault content changes significantly or recall quality drops, run the process again.

---

## How the customer runs it

The customer opens their agent (Claude Code, GPT, or whatever they use) with the tune skill loaded:

```
Customer: "Tune my HEBBS workspace. We're in legal/compliance."

Agent: I'll profile your domain. What do you typically search for?

Customer: "Specific clauses, coverage limits, compliance gaps across vendors."

Agent: Got it — Legal/Compliance profile. Let me store that.
  [calls: hb.remember("CLIENT-PROFILE: ...", entity_id="retrieval-instructions")]

  Now I'll generate test queries from your content.
  [calls: hb.recall(...) to sample content]
  [generates 20 evals]

  Here are 20 test queries. Want to change any?

Customer: "Looks good."

Agent: Running baseline...
  [calls: hb.recall() for each eval query, scores results]
  Baseline: 58%. 3 zero-hit queries.

  Tuning...
  [tries k, cue, strategy, weight variations via hb.recall()]
  Tuned: 85% (+27pp). 0 zero-hits.

  Storing retrieval instructions...
  [calls: hb.remember("RETRIEVAL-INSTRUCTION: ...", ...) for each strategy]

  Exporting rules file...
  [writes .hebbs/retrieval-rules.md]

  Done. Load retrieval-rules.md into your agent's context.
```

---

## How retrieval instructions are used

### Option A: Via `prime` (at conversation start)

The agent loads retrieval instructions before making recall calls:

```python
instructions = hb.prime(entity_id="retrieval-instructions")
# Returns all memories with entity_id=retrieval-instructions, ordered by importance
# Agent reads them and adjusts recall strategy
```

### Option B: Via the rules file (loaded into agent's prompt)

The exported `.hebbs/retrieval-rules.md` file is referenced in the agent's system prompt or skill file. The agent reads the rules before making any HEBBS calls.

---

## What this means for the platform

**Nothing.** The platform doesn't know tuning exists. The customer's agent makes `recall`, `remember`, `forget`, and `prime` calls — all existing API endpoints. No new endpoints, no new services, no tune-specific code anywhere.

Tuning is a pattern of usage, not a feature.

---

## Auto-tune (future, parked)

A future enhancement where the platform runs tuning automatically:
- Monitor recall query logs for quality signals
- Auto-generate evals from real queries
- Run the tune skill's phases as an automated pipeline
- Apply winning configs without customer intervention

This builds on the same primitives (recall, remember, prime) — the auto-tune runner would just be another "agent" following the tune skill, except it's our agent running on our infrastructure on a schedule.

**Not needed for launch.** The customer's agent can tune effectively using the skill.
