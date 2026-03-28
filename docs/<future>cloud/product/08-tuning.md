# HEBBS Cloud: Tuning

## Overview

Tuning is user-driven. After indexing, the customer uses their own AI agent (Claude, GPT, or whatever they've built) to run the tune process via `hb` CLI or SDK. The agent follows the tune skill — profiles the domain, generates evals, runs baselines, tunes parameters, and stores the winning config.

The customer doesn't need HEBBS to tune for them. They already have an AI agent. That agent can drive the entire process.

---

## How it works

The existing [tune skill](../../hebbs-skill/tune/SKILL.md) defines an 8-phase process. In cloud mode, the customer's agent follows the same process but uses cloud commands instead of local `hebbs` commands.

### The customer's agent drives tuning

```
Customer's agent (Claude, GPT, etc.)
    │
    │  Loads tune skill instructions
    │  (from SKILL.md or system prompt)
    │
    ├── Phase 1: Asks the customer about their domain, ICP, hard queries
    ├── Phase 2: Generates eval queries based on the conversation
    ├── Phase 3: Runs each eval via hb recall, scores results
    ├── Phase 4: Tries parameter variations, finds best settings
    ├── Phase 5: Stores winning strategies via hb remember
    ├── Phase 6: Compresses rules into master rules
    ├── Phase 7: Exports compiled rules to a file
    └── Phase 8: Re-tunes when needed
    │
    │  All commands go through hb CLI / SDK
    │
    ▼
HEBBS Cloud (unchanged engine, just serving recall/remember)
```

The HEBBS platform doesn't need to know tuning is happening. The agent is just making recall and remember calls — the same calls it always makes. Tuning is a pattern of usage, not a platform feature.

---

## Cloud CLI tune commands

These are convenience wrappers that make the tune workflow smoother. They're not strictly necessary — the agent could do everything with `recall` and `remember` — but they make the process cleaner.

### Define ICP profile

```sh
hb tune profile \
  --domain "legal-compliance" \
  --search-patterns "factual-lookup,entity-scoped,contradiction" \
  --hard-queries "cross-vendor compliance gaps" \
  --classification "legal"

# Stored as a high-importance memory:
# "CLIENT-PROFILE: Domain is legal-compliance. Primary search patterns:
#  factual-lookup, entity-scoped, contradiction. Classification: legal."
```

Or via SDK (the agent calls this programmatically):

```python
hb.tune.profile(
    domain="legal-compliance",
    search_patterns=["factual-lookup", "entity-scoped", "contradiction"],
    hard_queries=["cross-vendor compliance gaps"],
    classification="legal",
)
```

### Generate and manage evals

```sh
# Add an eval
hb tune eval add \
  --query "What is our ransomware coverage limit?" \
  --expected "ransomware,coverage,limit,5M,cyber-policy" \
  --type factual_lookup

# List evals
hb tune eval list

#   #   QUERY                                    TYPE              EXPECTED
#   1   What is our ransomware coverage limit?   factual_lookup    ransomware,coverage,limit,5M,cyber-policy
#   2   Cross-vendor compliance gaps             cross_entity      SOC2,Cloudvault,Ironclad,gaps,remediation
#   ...

# Remove an eval
hb tune eval remove 3

# Import evals from file (agent can generate this)
hb tune eval import ./evals.json
```

Evals are stored as memories with entity_id `tune-evals` and high importance. The agent can also just generate them programmatically via the SDK.

```python
hb.tune.add_eval(
    query="What is our ransomware coverage limit?",
    expected=["ransomware", "coverage", "limit", "5M", "cyber-policy"],
    type="factual_lookup",
)
```

### Run baseline

```sh
hb tune baseline

#   Running 20 evals against current settings...
#
#   Baseline results:
#     Keyword recall: 54% (46/84 keywords found)
#     Perfect queries: 2/20
#     Zero-hit queries: 3/20
#
#     Worst performers:
#       Q7:  0/5 - "cross-vendor compliance gaps" (cross_entity)
#       Q12: 1/4 - "latest risk register update" (recency_weighted)
#       Q15: 0/5 - "contradicting coverage limits" (contradiction)
```

SDK:

```python
baseline = hb.tune.run_baseline()

# baseline.recall_pct      → 54.0
# baseline.perfect         → 2
# baseline.zero_hit        → 3
# baseline.results         → list of per-query scores
# baseline.worst           → sorted by score ascending
```

### Run tune

```sh
# Tune all queries below 100%
hb tune run

#   Tuning 18 queries below 100%...
#
#   Tuned results:
#     Keyword recall: 84% (71/84) [was 54%, +30pp]
#     Perfect queries: 13/20 [was 2]
#     Zero-hit queries: 0/20 [was 3]
#
#     Biggest improvements:
#       Q7:  0/5 → 4/5 (expanded cue + analogical strategy)
#       Q12: 1/4 → 4/4 (recency weights + k=10)
#
#     Still below 100%:
#       Q15: 2/5 (extraction ceiling)
```

SDK:

```python
tuned = hb.tune.run()

# tuned.recall_pct          → 84.0
# tuned.delta               → 30.0
# tuned.improvements        → list of per-query before/after
# tuned.remaining_gaps      → queries still below 100%
```

### Store learnings

```sh
# Store the tuned strategies as retrieval instructions
hb tune store

#   Stored 8 retrieval instructions as master rules.
#   These will be used as defaults for future recall calls.
```

This stores the winning parameter sets (k, strategy, weights per query type) as high-importance memories with entity_id `retrieval-instructions`. The customer's agent can recall these at conversation start and use them to construct better recall calls.

### Export rules

```sh
# Export compiled rules to markdown
hb tune export --output ./retrieval-rules.md

#   Exported 8 rules to ./retrieval-rules.md
#   Load this file into your agent's context for optimal recall.
```

The exported file follows the same format as the tune skill's Phase 7 output. The customer adds it to their agent's system prompt or skill file.

### View tune status

```sh
hb tune status

#   Workspace:      sales-agent
#   Profile:        legal-compliance
#   Evals:          20 queries
#   Baseline:       54%
#   Best tuned:     84% (+30pp)
#   Last run:       2026-03-28
#   Rules stored:   8 master rules
#   Rules file:     ./retrieval-rules.md
```

---

## How the customer's agent uses this

### Example: Claude Code driving the tune

The customer opens Claude Code in their project, with the tune skill loaded:

```
Customer: "tune my HEBBS workspace"

Claude: I'll help you tune retrieval. First, let me understand your domain.

  What kind of documents are in your workspace?

Customer: "Legal contracts, compliance reports, vendor risk assessments"

Claude: Got it. And what do you typically search for?

Customer: "Specific clauses, coverage limits, compliance gaps across vendors,
  and contradictions between contract versions"

Claude: That maps to a Legal/Compliance profile. Let me set that up.

  [runs: hb tune profile --domain legal-compliance ...]

  Now I'll generate evals based on your content. Let me look at your docs first.

  [runs: hb recall "contracts" to sample content]
  [generates 20 eval queries]

  Here are the evals I generated:
  1. "What is our ransomware coverage limit?" — expected: ransomware, 5M, cyber
  2. "Cross-vendor compliance gaps" — expected: SOC2, Cloudvault, remediation
  ...

  Do these look right? Want to add or change any?

Customer: "Add one about data retention contradictions"

Claude: Added.

  [runs: hb tune eval add ...]

  Running baseline...

  [runs: hb tune baseline]

  Baseline: 54%. 3 zero-hit queries. Want me to tune?

Customer: "yes"

Claude:
  [runs: hb tune run]

  Tuned to 84% (+30pp). 0 zero-hits now.

  Q15 is still at 2/5 — that's an extraction ceiling, the dollar amounts
  weren't extracted as propositions. Want me to store these results?

Customer: "yes, store and export"

Claude:
  [runs: hb tune store]
  [runs: hb tune export --output ./retrieval-rules.md]

  Done. 8 master rules stored and exported to retrieval-rules.md.
  Add this file to your agent's context for optimal recall.
```

The entire process is human-in-the-loop but agent-executed. The customer reviews evals, approves tuning, and decides what to store. Their agent does the mechanical work.

---

## SDK tune interface

```python
# Full tune interface on the SDK
hb.tune.profile(domain=..., classification=..., search_patterns=..., hard_queries=...)
hb.tune.add_eval(query=..., expected=[...], type=...)
hb.tune.list_evals() → list[Eval]
hb.tune.remove_eval(index)
hb.tune.import_evals(path_or_list)
hb.tune.run_baseline() → BaselineResult
hb.tune.run() → TuneResult
hb.tune.store() → int (rules stored)
hb.tune.export(output_path) → path
hb.tune.status() → TuneStatus
```

All of these are thin wrappers around recall/remember calls with specific entity_ids and formatting. The tune interface is a convenience layer, not a new backend service.

---

## What this means for the platform

### No tune-specific backend code

The platform doesn't know tuning exists. From the platform's perspective, the customer's agent is just making recall and remember calls. The tune commands in the CLI/SDK are client-side logic that:

1. Stores evals as memories (entity_id: `tune-evals`)
2. Stores profiles as memories (entity_id: `tune-profile`)
3. Runs recall calls and scores results locally
4. Stores winning strategies as memories (entity_id: `retrieval-instructions`)
5. Exports rules as a local markdown file

No new API endpoints. No new platform services. No tune runner. The intelligence lives in the customer's agent and the CLI/SDK convenience wrappers.

### Where tune state lives

| Data | Stored as | Where |
|---|---|---|
| ICP profile | Memory (entity_id: `tune-profile`) | Tenant's RocksDB |
| Eval queries | Memories (entity_id: `tune-evals`) | Tenant's RocksDB |
| Baseline scores | Memory (entity_id: `tune-results`) | Tenant's RocksDB |
| Tuned scores | Memory (entity_id: `tune-results`) | Tenant's RocksDB |
| Master rules | Memories (entity_id: `retrieval-instructions`) | Tenant's RocksDB |
| Exported rules file | Local file on customer's machine | Customer's filesystem |

Everything is stored as memories in the workspace. The tune state is queryable via normal recall. If the customer's agent wants to check the current tune status, it recalls from `tune-profile`, `tune-evals`, and `tune-results`.

---

## Auto-tune (future, parked)

A future enhancement where the platform runs tuning automatically:

- Monitor query audit logs for quality signals
- Auto-generate evals from real queries
- Run tune cycles in the background
- Apply winning configs without customer intervention

This builds on the same tune primitives (evals, baseline, tune, store) but executes them as a platform-side cron job instead of customer-agent-driven.

**Not needed for launch.** The customer's agent can tune effectively. Auto-tune is a retention/expansion feature for customers who want hands-off optimization after initial setup.

When we build auto-tune, the tune commands and data format are already defined. The auto-tune runner just becomes another "agent" that calls the same tune SDK methods — except it's our agent, running on our infrastructure, on a schedule.
