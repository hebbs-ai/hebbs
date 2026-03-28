# HEBBS Enterprise: Tuning

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
    │  ├── hebbs recall    → test queries, measure quality
    │  ├── hebbs remember  → store retrieval instructions
    │  ├── hebbs forget    → clean up old instructions
    │  └── hebbs prime     → load instructions at conversation start
    │
    │  No special tune API. No tune CLI commands.
    │  Just the skill + existing primitives.
    │
    ▼
HEBBS Server (unchanged, doesn't know tuning is happening)
```

---

## The tune skill process

The skill defines 8 phases. The agent drives all of them using regular HEBBS commands:

### Phase 1: Profile the client
The agent asks the customer about their domain, search patterns, hardest queries. Classifies into an ICP (Legal, Sales, Engineering, Research).

Stores the profile:
```sh
hebbs remember "CLIENT-PROFILE: Domain is legal-compliance. Primary search patterns: factual-lookup, entity-scoped, contradiction. Classification: legal." --importance 0.9 --entity-id retrieval-instructions --format json
```

### Phase 2: Generate evals
The agent generates 20-50 test queries based on the domain and indexed content. Each eval has a query, expected keywords, and query type.

### Phase 3: Run baseline
The agent runs each eval using regular `recall`:
```sh
hebbs recall "What is our ransomware coverage limit?" -k 5 --format json
```
Scores by counting expected keywords in results. Reports baseline to the customer.

### Phase 4: Tune
For each query below 100%, the agent tries variations:
- Increase k
- Expand the cue with entity names and specifics
- Try different strategies (temporal, analogical)
- Adjust weights

All using regular `recall` calls with different parameters.

### Phase 5: Store learnings
The agent stores each winning strategy as a retrieval instruction:
```sh
hebbs remember "RETRIEVAL-INSTRUCTION: For compliance/audit queries, always expand acronyms and include the vendor name in the cue. Use k=10 minimum." --importance 0.9 --entity-id retrieval-instructions --format json
```

### Phase 6: Compress
After multiple tune sessions, the agent recalls all instructions, groups them, writes master rules:
```sh
hebbs recall "retrieval instructions" --entity-id retrieval-instructions -k 50 --format json
```

Then stores compressed master rules at higher importance and forgets the granular ones.

### Phase 7: Export to markdown
The agent compiles all master rules into a `.hebbs/retrieval-rules.md` file. This file loads into the agent's context before any HEBBS calls — so it makes better recall calls from the first query.

### Phase 8: Re-tune
When vault content changes significantly or recall quality drops, run the process again.

---

## How the customer runs it

The customer opens their agent (Claude Code, or whatever they use) with the tune skill loaded:

```
Customer: "Tune my HEBBS workspace. We're in legal/compliance."

Agent: I'll profile your domain. What do you typically search for?

Customer: "Specific clauses, coverage limits, compliance gaps across vendors."

Agent: Got it — Legal/Compliance profile. Let me store that.
  [calls: hebbs remember "CLIENT-PROFILE: ..." --entity-id retrieval-instructions]

  Now I'll generate test queries from your content.
  [calls: hebbs recall ... to sample content]
  [generates 20 evals]

  Here are 20 test queries. Want to change any?

Customer: "Looks good."

Agent: Running baseline...
  [calls: hebbs recall for each eval query]
  Baseline: 58%. 3 zero-hit queries.

  Tuning...
  [tries k, cue, strategy, weight variations via hebbs recall]
  Tuned: 85% (+27pp). 0 zero-hits.

  Storing retrieval instructions...
  [calls: hebbs remember "RETRIEVAL-INSTRUCTION: ..." for each strategy]

  Exporting rules file...
  [writes .hebbs/retrieval-rules.md]

  Done. Load retrieval-rules.md into your agent's context.
```

The agent did everything. HEBBS just served recall and remember calls — it has no idea tuning happened.

---

## How retrieval instructions are used

Two options:

### Option A: Via `hebbs prime` (at conversation start)

The customer's agent calls `prime` to load retrieval instructions before making recall calls:

```sh
hebbs prime retrieval-instructions --format json
```

This returns all memories with `entity_id=retrieval-instructions`, ordered by importance. The agent reads them and adjusts its recall strategy accordingly.

### Option B: Via the rules file (loaded into agent's prompt)

The exported `.hebbs/retrieval-rules.md` file is referenced in the agent's system prompt or skill file. The agent reads the rules before making any HEBBS calls.

This is faster than calling `prime` every conversation — the rules are already in context.

---

## What this means for the platform

**Nothing.** The platform doesn't know tuning exists. From the platform's perspective:
- The customer's agent makes `recall` calls (testing queries)
- The customer's agent makes `remember` calls (storing retrieval instructions)
- The customer's agent makes `forget` calls (cleaning up old instructions)
- The customer's agent makes `prime` calls (loading instructions)

All of these are existing API endpoints. No new endpoints, no new services, no tune-specific code anywhere in the platform or engine.

---

## Where tune state lives

| Data | Stored as | Where |
|---|---|---|
| Client profile | Memory (entity_id: `retrieval-instructions`) | HEBBS workspace |
| Retrieval instructions | Memories (entity_id: `retrieval-instructions`, importance: 0.9) | HEBBS workspace |
| Master rules | Memories (entity_id: `retrieval-instructions`, importance: 0.95) | HEBBS workspace |
| Compiled rules file | `.hebbs/retrieval-rules.md` | Customer's filesystem |

Everything is stored as regular memories. The tune skill is just a pattern of usage, not a feature.
