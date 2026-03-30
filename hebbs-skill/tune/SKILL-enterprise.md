---
name: hebbs-tune-enterprise
description: "Retrieval tuning skill for HEBBS Enterprise: profile the client, generate domain-specific evals, run baseline, tune retrieval parameters, store learnings, and export compiled rules."
homepage: https://hebbs.ai
---

# HEBBS Enterprise Tune: Teach Your Agent to Retrieve Better

This skill turns retrieval from guesswork into a measured, repeatable process. You profile the client, generate evals that match their domain, run baselines, tune parameters, and store what works. The end state is a compiled rules file that loads into every conversation before the first tool call.

This is the enterprise version. It works against a remote HEBBS server via the REST API. Prerequisites: a running HEBBS Enterprise server and a workspace with indexed documents.

---

## Prerequisites

The customer must have:
1. HEBBS server running (`http://server:8080`)
2. CLI connected: `hebbs login --endpoint http://server:8080 --api-key hb_live_sk_...`
3. Documents uploaded and indexed (via dashboard or `hebbs push ./docs`)

Or using the Python SDK:
```python
from hebbs.rest_client import HebbsRestClient
hb = HebbsRestClient("http://server:8080", api_key="hb_live_sk_...")
```

---

## When to activate

- After documents are uploaded and indexed in a workspace
- When the user says "tune", "optimize", "improve recall", "why can't it find X"
- When recall results are visibly poor (low scores, missing obvious facts)
- When workspace content changes significantly (new batch of files uploaded)

---

## Phase 1: Profile the client

**Do not generate evals yet.** Ask the user first. You need to understand what they search for, not what's in the files.

### Questions to ask

1. **What's the domain?** Legal contracts, sales calls, engineering docs, research papers, support tickets?
2. **What do you typically search for?** Specific facts, timelines, decisions, comparisons across documents, contradictions?
3. **Who uses the results?** The agent autonomously, or a human reviewing output?
4. **What does a wrong answer cost?** Compliance risk, lost deal, wasted debugging time?
5. **What's the hardest thing to find right now?** This becomes your first hard eval.

### ICP classification

| Profile | Heavy on | Medium on | Light on |
|---|---|---|---|
| **Legal/Compliance** | Factual lookup (40%), contradiction (15%) | Entity-scoped (20%), temporal (15%) | Cross-entity (10%) |
| **Sales/Revenue** | Recency-weighted (30%), entity-scoped (25%) | Factual lookup (20%) | Cross-entity (15%), temporal (10%) |
| **Engineering** | Causal (25%), entity-scoped (25%) | Factual lookup (20%), temporal (20%) | Broad sweep (10%) |
| **Research/Knowledge** | Factual lookup (30%), analogical (20%) | Broad sweep (20%), temporal (15%) | Entity-scoped (15%) |

Store the profile:

**CLI:**
```sh
hebbs remember "CLIENT-PROFILE: Domain is [domain]. Primary search patterns: [list]. Hardest queries: [what they said]. Classification: [profile]." --importance 0.9 --entity-id retrieval-instructions
```

**Python SDK:**
```python
await hb.remember(
    "CLIENT-PROFILE: Domain is [domain]. Primary search patterns: [list]. Classification: [profile].",
    importance=0.9,
    entity_id="retrieval-instructions"
)
```

---

## Phase 2: Generate evals

### How many

| Workspace size | Start with | Expand to |
|---|---|---|
| 5-10 files | 10 | 20 |
| 20-50 files | 20 | 50 |
| 50-200 files | 30 | 100 |
| 200+ files | 50 | 200+ |

### Each eval has three parts

```
Q[N]: "[natural language query the user would actually type]"
  Expected: [keyword1, keyword2, keyword3, keyword4, keyword5]
  Type: factual_lookup | entity_scoped | temporal | cross_entity | causal | recency_weighted | contradiction | broad_sweep
```

---

## Phase 3: Run baseline

**CLI:**
```sh
hebbs recall "[query]" -k 5 --format json
```

**Python SDK:**
```python
results = await hb.recall("[query]", top_k=5)
```

Score each query: count how many expected keywords appear in the returned results.

```
Per query:   keywords_found / keywords_expected
Overall:     sum(all_found) / sum(all_expected) = baseline %
```

**Report to the user:**

```
Baseline results (20 queries, default settings):
  Keyword recall: 54% (46/84 keywords found)
  Perfect queries: 2/20
  Zero-hit queries: 3/20

  Worst performers:
    Q7:  0/5 - "cross-vendor compliance gaps"
    Q12: 1/4 - "latest risk register update"
```

---

## Phase 4: Tune

For every query below 100%, classify the failure and apply the fix:

| Pattern | Symptom | Fix |
|---|---|---|
| **k too low** | Keywords exist in results 6-10 | Increase to k=10 or k=15 |
| **Cue too generic** | Results are topically related but wrong section | Expand cue with entity names and specifics |
| **Missing entity names** | Right topic, wrong entity's version | Add entity name to cue |
| **Wrong strategy** | Timeline query returns random order | Switch to temporal with appropriate weights |

**CLI:**
```sh
hebbs recall "SOC 2 Type II audit findings access controls Cloudvault" -k 10 --format json
```

**Python SDK:**
```python
results = await hb.recall(
    "SOC 2 Type II audit findings access controls Cloudvault",
    top_k=10
)
```

Score again. Compare before/after.

---

## Phase 5: Store learnings

Store each winning strategy as a retrieval instruction:

**CLI:**
```sh
hebbs remember "RETRIEVAL-INSTRUCTION: For compliance/audit queries, always expand acronyms and include the vendor name in the cue. Use k=10 minimum." --importance 0.9 --entity-id retrieval-instructions
```

**Python SDK:**
```python
await hb.remember(
    "RETRIEVAL-INSTRUCTION: For compliance/audit queries, always expand acronyms and include the vendor name. Use k=10 minimum.",
    importance=0.9,
    entity_id="retrieval-instructions"
)
```

Store 5-15 individual strategies from each tune pass.

---

## Phase 6: Compress and iterate

After 2-3 tune sessions, recall all instructions and compress:

**CLI:**
```sh
hebbs prime retrieval-instructions --max 50
```

**Python SDK:**
```python
all_instructions = await hb.prime("retrieval-instructions", max_memories=50)
print(all_instructions.text)
```

Group by pattern, write 10-20 master rules. Store at higher importance (0.95). Forget granular ones:

```sh
hebbs forget --entity-id retrieval-instructions
```

Then re-store only the master rules.

---

## Phase 7: Export to markdown

Compile all master rules into a markdown file. The agent reads this file before making any HEBBS calls.

```markdown
# Retrieval Rules

## Cue Construction
- Always expand acronyms: "SOC2" -> "SOC 2 Type II"
- Always include entity names in cues

## k Sizing
- Default: k=10
- Simple factual with unique entity: k=5
- Broad sweep or cross-entity: k=15

## Strategy Selection
- Factual lookup: similarity (default)
- Timeline/change: temporal + entity-id
- Cross-entity comparison: analogical, alpha=0.5

## Domain-Specific Rules
[Rules specific to this workspace's content]
```

Tell the customer: "Load this file into your agent's prompt before any HEBBS calls."

---

## Phase 8: Re-tune when needed

Run again when:
- New content uploaded to the workspace
- Recall quality drops
- Embedding or LLM model changes
- 30+ days since last tune

---

## How retrieval instructions are used at runtime

The customer's agent, at the start of each conversation:

**Option A: Via `prime` (dynamic)**
```python
instructions = await hb.prime("retrieval-instructions", max_memories=20)
# Agent reads instructions.text, then makes better recall calls
```

**Option B: Via rules file (static, faster)**
The exported markdown file is loaded into the agent's system prompt. No API call needed.

---

## Scorecard

```
Client: _______________
Domain: _______________
Workspace: _______________
Endpoint: _______________

| Run  | Date | Evals | Baseline | Tuned | Delta | Perfect | Zero-hit |
|------|------|-------|----------|-------|-------|---------|----------|
| 1    |      |       |    %     |   %   |  +pp  |   /     |    /     |
| 2    |      |       |    %     |   %   |  +pp  |   /     |    /     |

Master rules stored: ___
Rules file exported: yes / no
```
