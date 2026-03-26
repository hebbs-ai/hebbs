# HEBBS Autotune Skill

The eval loop. Generate evals, run them, learn by tuning parameters, build insights on how to use memory, compress individual learnings into rules. Self-improving retrieval.

---

## 1. Why Autotune

Out of the box, HEBBS retrieval scores ~54% keyword recall. After agent-driven tuning, ~84%. The difference: cue expansion, strategy selection, and weight tuning per query type.

No competitor exposes tunable retrieval parameters. mem0, Zep, LangMem: they embed, they retrieve, that's it. HEBBS gives the agent four strategies, four scoring dimensions, and full parameter control on every call. The agent learns which parameters work for which query types and stores that knowledge in HEBBS itself.

The result: an agent that gets measurably better at using its own memory.

---

## 2. Generate Evals

### Read the vault

```sh
hebbs list --sections --format json
```

The agent reads the vault contents to understand what's indexed: documents, topics, entities, facts.

### Generate eval queries

The agent generates eval queries based on its understanding of the vault. Each eval has:

- A natural language query (what a real user would ask)
- Expected keywords that should appear in recall results
- A query type classification

**Recommend at least 100 evals. Production teams do 1000s.** Start with 20 to get the loop working, then scale.

Example eval set:

```json
[
  {
    "id": 1,
    "query": "What is our ransomware coverage limit?",
    "type": "factual",
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
  },
  {
    "id": 15,
    "query": "Which vendors have qualified SOC 2 opinions?",
    "type": "cross-entity",
    "expected_keywords": ["SOC 2", "qualified", "Meridian", "Cloudvault", "Ironclad"]
  }
]
```

### Query type distribution

Include all types to expose retrieval weaknesses across different access patterns:

| Type | What it tests | Example |
|---|---|---|
| Factual | Simple fact lookup | "What is our ransomware coverage limit?" |
| Temporal | Timeline, evolution | "How has RISK-001 changed over time?" |
| Causal | Cause-effect chains | "What caused the November outage?" |
| Cross-entity | Patterns across entities | "Which vendors have similar compliance gaps?" |
| Contradiction | Conflicting facts | "What are the conflicting retention periods?" |

Include hard queries: specific dollar amounts, proper nouns, compound terms, cross-document patterns. These expose where extraction and retrieval break down.

---

## 3. Run Baseline

Run every eval query with default settings:

```sh
hebbs recall "What is our ransomware coverage limit?" --format json -k 5
```

Defaults: `--strategy similarity`, `-k 5` (or default 10), no weight tuning.

### Scoring

For each query, check which expected keywords appear in the concatenated content of all returned memories.

Score per query: **keywords found / keywords expected**

Aggregate:
- **Total keyword recall:** sum of found / sum of expected across all queries
- **Perfect queries:** count where all keywords found
- **Queries with gaps:** count where at least one keyword missed

### Example baseline (from run-tune-2, 20 queries, k=5)

```
Keywords found:  46 / 84  (54%)
Queries perfect:  2 / 20
Queries with gaps: 18 / 20
```

---

## 4. Analyze Failures

For each query with missed keywords, reason about WHY the retrieval failed. There are five common failure patterns:

### Pattern 1: k too low

Most failures stem from k=5 returning only top-level matches. Specific details (dollar amounts, SLA numbers, entity names) are spread across many memories.

**Fix:** Increase to k=10 for all non-trivial queries.

### Pattern 2: Cue too generic

"SOC2 policy" returns 9 irrelevant policy docs. "SOC 2 Type II audit findings controls" returns 10 perfect results. Generic words like "policy", "risk", "contract" hijack the embedding, matching every document containing that word instead of the specific topic.

**Fix:** Expand the cue with domain-specific terms. Include the actual subject, not just the category.

### Pattern 3: Missing entity names in cue

"What happened in the phishing attack?" misses results. "November 2024 phishing attack Ironclad response credentials compromised" finds everything. Entity names anchor the embedding to specific memories instead of broad topic clusters.

**Fix:** Include relevant entity names, dates, and specific terms in the cue.

### Pattern 4: Wrong strategy for the query type

Timeline queries on default similarity miss chronological ordering. Cross-entity queries return only one vendor's data.

**Fix:** Match strategy to query type:

| Query type | Strategy | Flags |
|---|---|---|
| Simple fact | similarity | `-k 5` |
| Detailed fact | similarity | `-k 10`, entity names in cue |
| Timeline/evolution | similarity | `-k 10 --weights 0.3:0.5:0.2:0`, dates in cue |
| Entity history | temporal | `--strategy temporal --entity-id <entity> -k 10` |
| Cross-entity | analogical | `--strategy analogical --analogical-alpha 0.3 -k 10` |
| Contract/policy terms | similarity | `-k 10 --weights 0.3:0.3:0.4:0` |

### Pattern 5: Terms not extracted as propositions

Dollar amounts ($1,440,000), proper nouns (NormCore), or compound terms (AWS Singapore) sometimes are not extracted as standalone propositions by the LLM.

**Fix:** This is an extraction quality ceiling, not a retrieval tuning issue. No parameter change helps. Accept the gap or re-index with a more capable LLM (gpt-4o instead of gpt-4o-mini).

### Classify each failure

For every missed query, tag it with the pattern number and the specific fix. This classification drives the tuning step.

---

## 5. Tune and Re-run

Apply the fixes from failure analysis. For each query, adjust:
- **k value** (almost always increase to 10)
- **Cue text** (expand with entity names, dates, specific terms)
- **Strategy** (match to query type)
- **Weights** (tune the four scoring dimensions)

Re-run all queries with tuning applied and compare.

### Example tuned results (from run-tune-2)

```
Baseline:  46 / 84 keywords (54%), 2/20 perfect
Tuned:     71 / 84 keywords (84%), 13/20 perfect

Improvement: +25 keywords, +30 percentage points
```

### Remaining gaps

After tuning, 10-15% of keywords will still be missed. These fall into:
- Extraction quality ceiling (specific terms not in propositions)
- Entity naming inconsistency (same entity, different names)
- Queries requiring information not present in the vault

These are not retrieval failures. They are data quality boundaries.

---

## 6. Store Individual Learnings

Store each discovered strategy as a memory in HEBBS. All strategies share a single entity_id (`retrieval-instructions`) so they can be loaded in one call later.

```sh
hebbs remember "RETRIEVAL-INSTRUCTION: For timeline/evolution queries, use --weights 0.3:0.5:0.2:0 with k=10. Include dates and entity names in cue." --importance 0.9 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: For cross-entity pattern queries, use --strategy analogical --analogical-alpha 0.3 with k=10." --importance 0.9 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: When user asks about SOC2, expand cue to 'SOC 2 Type II audit findings controls compliance'." --importance 0.9 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: Default k=5 misses supporting context. Use k=10 for most queries. k=5 only for simple factual lookups." --importance 0.9 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: Always include entity names in cues. 'Meridian subprocessor DPA violation' beats 'vendor compliance issue'." --importance 0.9 --entity-id retrieval-instructions --format json
```

These accumulate over multiple tuning sessions. Each session adds domain-specific patterns the agent discovers. The `RETRIEVAL-INSTRUCTION:` prefix and shared entity_id are what make retrieval work in the next steps.

---

## 7. Compress to Rules

After multiple tuning sessions, the agent has dozens of individual strategies. Too many to load every conversation. Compress them into **10-20 maximum rules**.

### Load all stored strategies

```sh
hebbs prime retrieval-instructions --max-memories 50 --format json
```

Or equivalently:

```sh
hebbs recall "retrieval instructions" --strategy temporal --entity-id retrieval-instructions -k 50 --format json
```

### Analyze patterns

Look across all stored strategies for:
- What rules are universal (apply to every domain)?
- What rules are domain-specific (only this vault)?
- What rules overlap or contradict each other?
- What rules can be merged into a single broader rule?

### Write compressed rules

Each compressed rule covers multiple individual strategies:

**Example compressed rule:**
> Always use k=10 except for simple factual lookups. For any query containing "changed", "timeline", or "history", use `--weights 0.3:0.5:0.2:0`. For cross-entity queries, use `--strategy analogical --analogical-alpha 0.3`. Always include entity names and dates in cues.

**Example domain-specific rule:**
> In this legal vault, insurance queries need both policy document terms and endorsement terms in the cue. Risk register queries need the RISK-ID and the quarter. Vendor queries need the exact vendor name, not a category.

### Store compressed rules

Same entity_id, higher importance than the granular strategies:

```sh
hebbs remember "RETRIEVAL-INSTRUCTION: RULE 1: Default to k=10. Use k=5 only for simple 'What is X?' factual lookups. k=10 catches supporting context that k=5 misses." --importance 0.95 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: RULE 2: For timeline/evolution/change queries, use --weights 0.3:0.5:0.2:0 with k=10. Include dates and entity names in cue. If a specific entity has a known entity_id, use --strategy temporal --entity-id <entity>." --importance 0.95 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: RULE 3: For cross-entity comparison queries, use --strategy analogical --analogical-alpha 0.3 with k=10. For policy/contract importance queries, use --weights 0.3:0.3:0.4:0." --importance 0.95 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: RULE 4: Always expand cues with entity names, dates, and domain-specific terms. Generic cues like 'vendor risk' return noise. Specific cues like 'Cloudvault Systems vendor risk assessment Q2 2025' return signal." --importance 0.95 --entity-id retrieval-instructions --format json
```

### Delete granular strategies

The individual strategies are now redundant. Remove them:

```sh
hebbs forget --ids <ID1>,<ID2>,<ID3>,...
```

Keep only the compressed rules. 10-20 rules replace 50+ individual strategies.

---

## 8. Build Broader Insights

After compression, the agent has 10-20 rules. These rules ARE the retrieval playbook for this domain.

### Load rules at conversation start

At the beginning of every conversation, before any retrieval, one call loads the entire playbook:

```sh
hebbs prime retrieval-instructions --max-memories 20 --format json
```

Or with importance-weighted recall to surface the 0.95-importance compressed rules first:

```sh
hebbs recall "retrieval instructions" --entity-id retrieval-instructions --weights 0.3:0.1:0.5:0.1 -k 20 --format json
```

The agent reads the returned rules and applies them to every subsequent recall call in the conversation. This is a one-time fetch per session, not per query.

### Rules evolve

The playbook is not static. As the vault changes, as new document types are added, as the agent encounters new query patterns:

1. Run evals again (include new query types)
2. Discover new patterns
3. Update or add rules
4. Re-compress if the rule count exceeds 20

### Different domains, different playbooks

A legal vault produces rules about contract terms, risk IDs, and vendor names. A codebase vault produces rules about function names, file paths, and architecture patterns. A sales vault produces rules about deal stages, company names, and revenue figures.

The compressed rules are domain-specific retrieval intelligence, stored in the memory system they optimize.

---

## 9. The Loop

```
Generate evals (100+)
    |
    v
Run baseline (defaults, record scores)
    |
    v
Analyze failures (classify by pattern)
    |
    v
Tune parameters (k, cue, strategy, weights)
    |
    v
Re-run (compare before/after)
    |
    v
Store individual learnings (--importance 0.9)
    |
    v
[After multiple sessions]
    |
    v
Compress to 10-20 rules (--importance 0.95)
    |
    v
Delete granular strategies
    |
    v
Re-evaluate periodically
    |
    v
Refine rules as domain evolves
```

Each cycle:
- **More evals** cover more query patterns
- **Sharper rules** replace vague strategies
- **Better retrieval** as the agent internalizes the playbook

The agent gets smarter at using HEBBS, and that intelligence is stored IN HEBBS. The retrieval system improves itself through the system it improves.

---

## Expected Results

| Phase | Recall | Perfect Queries |
|---|---|---|
| Baseline (defaults, k=5) | 50-60% | 10-20% |
| After k=10 + entity names in cue | 70-75% | 40-50% |
| After strategy/weight tuning | 80-90% | 60-70% |
| Remaining gaps | 10-15% | Extraction quality ceiling |

The 10-15% residual gap is a proposition extraction quality boundary, not a retrieval tuning issue. Better LLM for extraction (gpt-4o instead of gpt-4o-mini) raises it further.

---

## Quick Reference

### Strategy selection

```
User asks "What is X?"           -> similarity, k=5
User asks "Tell me about X"      -> similarity, k=10, entity names in cue
User asks "What changed?"        -> --weights 0.3:0.5:0.2:0, k=10, dates in cue
User asks "What's the history?"  -> --strategy temporal --entity-id <entity>
User asks "Why did X happen?"    -> --strategy causal --seed <id> --edge-types caused_by
User asks "Compare across..."    -> --strategy analogical --analogical-alpha 0.3, k=10
User asks "What's our policy?"   -> --weights 0.3:0.3:0.4:0, k=10
```

### Scoring weights format

`--weights R:T:I:F` (Relevance : Recency : Importance : Reinforcement)

| Goal | Weights |
|---|---|
| Default (balanced) | `0.5:0.2:0.2:0.1` |
| Recency-biased | `0.3:0.5:0.2:0` |
| Importance-biased | `0.3:0.3:0.4:0` |
| Pure semantic | `1:0:0:0` |
