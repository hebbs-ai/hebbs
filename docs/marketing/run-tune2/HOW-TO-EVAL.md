# How to Run the Eval Loop

Step-by-step guide for running the agent-driven eval and tuning loop on a Hebbs vault.

---

## Prerequisites

- Hebbs installed and a vault indexed (`hebbs init . && hebbs index .`)
- An agent (Claude Code, Codex, or any LLM with tool use)
- The agent has access to `hebbs recall`, `hebbs remember`, and `hebbs list`

---

## Step 1: Understand the vault

The agent reads the vault contents to understand what's indexed.

```bash
hebbs list --sections --format json
```

The agent should understand: what documents exist, what topics they cover, what entities are mentioned, what facts would be important to retrieve.

---

## Step 2: Generate eval queries

The agent generates 20-50 eval queries based on its understanding of the vault. Each eval has:
- A natural language query (what a real user would ask)
- Expected keywords that should appear in the recall results
- A query type (similarity, temporal, causal, analogical, contradiction)

**Example eval set:**

```json
[
  {
    "id": 1,
    "query": "What is our ransomware coverage limit?",
    "type": "similarity",
    "expected_keywords": ["$2,000,000", "500,000", "ransomware", "TrueNorth"]
  },
  {
    "id": 4,
    "query": "Timeline of Ironclad SLA changes",
    "type": "temporal",
    "expected_keywords": ["15 minutes", "30 minutes", "P1", "amendment"]
  },
  {
    "id": 8,
    "query": "How has RISK-001 changed Q4 2024 to Q2 2025?",
    "type": "temporal",
    "expected_keywords": ["single cloud", "Cloudvault", "HIGH", "MEDIUM", "reduced"]
  }
]
```

**Key:** The agent generates the evals, not Hebbs. The agent is smarter than any built-in eval generator.

**Query type distribution matters.** Include:
- Simple factual lookups (similarity)
- Timeline/evolution questions (temporal, recency-weighted)
- "Why did X happen?" questions (causal)
- Cross-entity pattern questions (analogical)
- Queries where known contradictions exist

---

## Step 3: Run baseline

Run every eval query with default settings: `--strategy similarity -k 5`.

```bash
hebbs recall "What is our ransomware coverage limit?" --format json -k 5
```

For each query, check which expected keywords appear in the concatenated content of returned memories. Score:
- **Keywords found / keywords expected** per query
- **Total keywords found / total expected** across all queries
- Count of **perfect queries** (all keywords found)

**Run-tune-2 baseline:** 46/84 keywords (54%), 2/20 perfect queries.

---

## Step 4: Analyze failures

For each query with missed keywords, the agent reasons about WHY:

### Common failure patterns

**Pattern 1: k=5 too few**
Most failures stem from k=5 returning only top-level results. Propositions containing specific details (dollar amounts, SLA numbers, entity names) are spread across many memories.
- **Fix:** Increase to k=10 for all non-trivial queries.

**Pattern 2: Cue too generic**
"SOC2 policy" returns generic policy docs, not SOC 2 content. "SOC 2 Type II audit findings controls" returns 10 perfect results. Generic words like "policy", "risk", "contract" hijack the embedding.
- **Fix:** Expand the cue with domain-specific terms. "SOC2" → "SOC 2 Type II audit findings controls compliance."

**Pattern 3: Missing entity names in cue**
"What happened in the phishing attack?" misses results. "November 2024 phishing attack Ironclad response credentials compromised" finds everything.
- **Fix:** Include relevant entity names, dates, and specific terms in the cue.

**Pattern 4: Wrong strategy for the query type**
Timeline queries on default similarity miss chronological ordering. Cross-entity queries return only one vendor.
- **Fix:** Match strategy to query type:
  - Timeline/evolution → `--weights 0.3:0.5:0.2:0` (recency-weighted) or `--strategy temporal --entity-id <entity>`
  - Cross-entity patterns → `--strategy analogical --analogical-alpha 0.3`
  - Contract/policy terms → `--weights 0.3:0.3:0.4:0` (importance-weighted)

**Pattern 5: Specific terms not extracted as propositions**
Dollar amounts ($1,440,000), proper nouns (NormCore), or compound terms (AWS Singapore) sometimes aren't extracted as standalone propositions by the LLM.
- **Fix:** This is a proposition extraction quality issue. No tuning fix. Accept the gap or re-index with a more capable LLM.

---

## Step 5: Tune and re-run

Apply fixes per query: adjusted k, expanded cue, different strategy/weights. Re-run all queries with tuning applied.

**Example tuning per query type:**

| Query Type | Strategy | Flags |
|---|---|---|
| Simple fact | similarity | `-k 5` (default is fine) |
| Detailed fact | similarity | `-k 10`, entity names in cue |
| Timeline/evolution | similarity | `-k 10 --weights 0.3:0.5:0.2:0`, dates in cue |
| Cross-entity | analogical | `-k 10 --strategy analogical --analogical-alpha 0.3` |
| Contract terms | similarity | `-k 10 --weights 0.3:0.3:0.4:0` |
| Entity history | temporal | `--strategy temporal --entity-id <entity> -k 10` |

**Run-tune-2 tuned:** 71/84 keywords (84%), 13/20 perfect queries.

---

## Step 6: Store learned strategies

The agent stores what it learned in Hebbs itself:

```bash
hebbs remember "RETRIEVAL STRATEGY: For timeline/evolution queries, use \
    --weights 0.3:0.5:0.2:0 with k=10. Include dates and entity names in cue." \
    --importance 0.9

hebbs remember "RETRIEVAL STRATEGY: For cross-entity pattern queries, use \
    --strategy analogical --analogical-alpha 0.3 with k=10." \
    --importance 0.9

hebbs remember "RETRIEVAL STRATEGY: When user asks about SOC2, expand cue to \
    'SOC 2 Type II audit findings controls compliance'." \
    --importance 0.9

hebbs remember "RETRIEVAL STRATEGY: Default k=5 misses supporting context. \
    Use k=10 for most queries. k=5 only for simple factual lookups." \
    --importance 0.85

hebbs remember "RETRIEVAL STRATEGY: Always include entity names in cues. \
    'Meridian subprocessor DPA violation' beats 'vendor compliance issue'." \
    --importance 0.9
```

These strategies are now retrievable. Next conversation, the agent runs:

```bash
hebbs recall "retrieval strategy" --importance 0.9 -k 20
```

And loads all its learned strategies before making any recall calls.

---

## Step 7: Verify persistence

New conversation. Agent loads strategies from Hebbs. User asks "What's our SOC 2 readiness?" Agent recalls its stored strategy, expands the cue, uses k=10, and gets perfect results on the first try.

The loop is closed: agent learned how to use Hebbs better, stored the knowledge in Hebbs, and uses Hebbs to recall how to use Hebbs.

---

## Step 8: Scale and iterate

- Start with 20 evals. Get the tuning loop working.
- Scale to 100-200 evals for production domains.
- Re-run evals after vault changes (new files indexed) to catch drift.
- Store domain-specific cue expansions as they're discovered.
- Different domains need different strategies. Legal agents, sales agents, coding agents each develop their own retrieval playbook.

---

## Expected Results

| Phase | Recall | Perfect Queries |
|---|---|---|
| Baseline (defaults, k=5) | 50-60% | 10-20% |
| After k=10 + entity names in cue | 70-75% | 40-50% |
| After strategy/weight tuning | 80-90% | 60-70% |
| Remaining gaps | 10-15% | Specific terms not extracted as propositions |

The 10-15% residual gap is a proposition extraction quality ceiling, not a retrieval tuning issue. Better LLM for extraction (gpt-4o instead of gpt-4o-mini) would raise it further.

---

## Quick Reference: Strategy Selection

```
User asks "What is X?"           → similarity, k=5
User asks "Tell me about X"      → similarity, k=10, entity names in cue
User asks "What changed?"        → --weights 0.3:0.5:0.2:0, k=10, dates in cue
User asks "What's the history?"  → --strategy temporal --entity-id <entity>
User asks "Why did X happen?"    → --strategy causal --seed <id> (if edges exist)
                                   or similarity k=10 with entity names (fallback)
User asks "Compare across..."    → --strategy analogical --analogical-alpha 0.3, k=10
User asks "What's our policy?"   → --weights 0.3:0.3:0.4:0, k=10 (importance-weighted)
```
