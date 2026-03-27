---
name: hebbs-tune
description: "Retrieval tuning skill for HEBBS: profile the client, generate domain-specific evals, run baseline, tune retrieval parameters, store learnings, and export compiled rules to a markdown file for prompt injection."
homepage: https://hebbs.ai
metadata:
  {
    "openclaw":
      {
        "emoji": "🎯",
        "requires": { "bins": ["hebbs"], "skills": ["hebbs"] },
      },
  }
---

# HEBBS Tune: Teach Your Agent to Retrieve Better

This skill turns retrieval from guesswork into a measured, repeatable process. You profile the client, generate evals that match their domain, run baselines, tune parameters, and store what works. The end state is a compiled rules file that loads into every conversation before the first tool call.

---

## When to activate

- After `hebbs init` + `hebbs index` completes on a new vault
- When the user says "tune", "optimize", "improve recall", "why can't it find X"
- When recall results are visibly poor (low scores, missing obvious facts)
- When vault content changes significantly (new batch of files indexed)

---

## Phase 1: Profile the client

**Do not generate evals yet.** Ask the user first. You need to understand what they search for, not what's in the files.

### Questions to ask

Ask these conversationally. Don't dump a questionnaire. Weave them into the conversation based on what you already know.

1. **What's the domain?** Legal contracts, sales calls, engineering docs, research papers, support tickets?
2. **What do you typically search for?** Specific facts, timelines, decisions, comparisons across documents, contradictions?
3. **Who uses the results?** You (the agent) autonomously, or a human reviewing your output?
4. **What does a wrong answer cost?** Compliance risk, lost deal, wasted debugging time, or just mild annoyance?
5. **What's the hardest thing to find right now?** This becomes your first hard eval.

### ICP classification

Based on answers, classify the client into one of these profiles. This determines eval distribution.

| Profile | Heavy on | Medium on | Light on |
|---|---|---|---|
| **Legal/Compliance** | Factual lookup (40%), contradiction (15%) | Entity-scoped (20%), temporal (15%) | Cross-entity (10%) |
| **Sales/Revenue** | Recency-weighted (30%), entity-scoped (25%) | Factual lookup (20%) | Cross-entity (15%), temporal (10%) |
| **Engineering** | Causal (25%), entity-scoped (25%) | Factual lookup (20%), temporal (20%) | Broad sweep (10%) |
| **Research/Knowledge** | Factual lookup (30%), analogical (20%) | Broad sweep (20%), temporal (15%) | Entity-scoped (15%) |

Store the profile:

```sh
hebbs remember "CLIENT-PROFILE: Domain is [domain]. Primary search patterns: [list]. Hardest queries: [what they said]. Classification: [profile]." --importance 0.9 --entity-id retrieval-instructions --global --format json
```

---

## Phase 2: Generate evals

### How many

| Vault size | Start with | Expand to |
|---|---|---|
| 5-10 files | 10 | 20 |
| 20-50 files | 20 | 50 |
| 50-200 files | 30 | 100 |
| 200+ files | 50 | 200+ |

Start small. You can always add more after the first pass reveals gaps.

### How to generate

1. **Read the vault.** Skim 5-10 representative files. Note key entities, facts, dates, relationships, contradictions.
2. **Map to query types.** Use the ICP distribution above. If the client is legal, 40% of evals should be factual lookups.
3. **Include 3-4 hard queries.** These are the ones the user said are hard to find. They're the most valuable evals.
4. **Each eval has three parts:**

```
Q[N]: "[natural language query the user would actually type]"
  Expected: [keyword1, keyword2, keyword3, keyword4, keyword5]
  Type: factual_lookup | entity_scoped | temporal | cross_entity | causal | recency_weighted | contradiction | broad_sweep
```

### What makes a good eval

- Uses vocabulary the client actually uses, not technical HEBBS terms
- Has 3-5 specific expected keywords (concrete facts, not vague concepts)
- Covers multiple files (not just one document)
- Includes entity names that exist in the corpus
- Has at least 3-4 queries you expect to fail on baseline

### What makes a bad eval

- Too vague: "tell me about compliance" (what keywords do you expect?)
- Too easy: query is a section heading verbatim
- Wrong vocabulary: terms the corpus doesn't contain
- All similarity: ignoring temporal, causal, analogical
- All from one file: doesn't test cross-document retrieval

---

## Phase 3: Run baseline

Run every eval with defaults:

```sh
hebbs recall "[query]" -k 5 --format json
```

Score each query: count how many expected keywords appear in the returned results.

```
Per query:   keywords_found / keywords_expected
Overall:     sum(all_found) / sum(all_expected) = baseline %
Perfect:     queries where all keywords found
Zero-hit:    queries where no keywords found
```

**Report to the user.** Do not silently tune. Show them:

```
Baseline results (20 queries, default settings):
  Keyword recall: 54% (46/84 keywords found)
  Perfect queries: 2/20
  Zero-hit queries: 3/20

  Worst performers:
    Q7:  0/5 - "cross-vendor compliance gaps" (cross_entity)
    Q12: 1/4 - "latest risk register update" (recency_weighted)
    Q15: 0/5 - "contradicting coverage limits" (contradiction)
```

Ask: "Want me to run optimizations on these?"

---

## Phase 4: Tune

For every query below 100%, classify the failure and apply the fix:

| Pattern | Symptom | Fix |
|---|---|---|
| **k too low** | Keywords exist in results 6-10 | Increase to k=10 or k=15 |
| **Cue too generic** | Results are topically related but wrong section | Expand cue with entity names and specifics |
| **Missing entity names** | Right topic, wrong entity's version | Add entity name to cue |
| **Wrong strategy** | Timeline query returns random order | Switch to temporal/analogical with appropriate weights |
| **Extraction ceiling** | Fact was never extracted as a proposition | Accept gap or re-index with better LLM |

The first three patterns cover 80% of failures. Fix those first.

### Apply and re-run

```sh
# Was: hebbs recall "SOC2 policy" -k 5
# Now:
hebbs recall "SOC 2 Type II audit findings access controls Cloudvault" -k 10 --format json

# Temporal query:
hebbs recall "data retention policy changes" --strategy temporal --entity-id data_retention -k 10 --format json

# Recency-weighted:
hebbs recall "latest risk register update" --weights 0.3:0.5:0.2:0 -k 10 --format json
```

Score again. Compare before/after per query and overall.

### Report to user

```
Tuned results (20 queries):
  Keyword recall: 84% (71/84 keywords found) [was 54%, +30pp]
  Perfect queries: 13/20 [was 2]
  Zero-hit queries: 0/20 [was 3]

  Biggest improvements:
    Q7:  0/5 -> 4/5 (expanded cue + analogical strategy)
    Q12: 1/4 -> 4/4 (recency weights + k=10)

  Still below 100%:
    Q15: 2/5 (extraction ceiling - dollar amounts not in propositions)
```

Ask: "Want me to keep tuning, or store what we've learned?"

---

## Phase 5: Store learnings

Store each successful strategy as a retrieval instruction:

```sh
hebbs remember "RETRIEVAL-INSTRUCTION: For compliance/audit queries, always expand acronyms and include the vendor name in the cue. Use k=10 minimum. Example: 'SOC2 policy' becomes 'SOC 2 Type II audit findings access controls [vendor name]'" --importance 0.9 --entity-id retrieval-instructions --global --format json
```

```sh
hebbs remember "RETRIEVAL-INSTRUCTION: For timeline/change queries, use --strategy temporal with --entity-id set to the subject. Use --weights 0.2:0.6:0.1:0.1 to prioritize recency." --importance 0.9 --entity-id retrieval-instructions --global --format json
```

```sh
hebbs remember "RETRIEVAL-INSTRUCTION: For cross-entity comparison queries, use --strategy analogical --analogical-alpha 0.5 and include all entity names in the cue separated by spaces." --importance 0.9 --entity-id retrieval-instructions --global --format json
```

Store 5-15 individual strategies from each tune pass.

---

## Phase 6: Compress and iterate

After 2-3 tune sessions, you'll have 20-50 individual retrieval instructions. Compress them.

### Read all stored strategies

```sh
hebbs recall "retrieval instructions" --entity-id retrieval-instructions -k 50 --global --format json
```

### Group and compress

Group by pattern:
- Cue expansion rules (how to rewrite queries)
- k sizing rules (when to use k=5 vs k=10 vs k=15)
- Strategy selection rules (when to use similarity vs temporal vs analogical)
- Weight tuning rules (which weight profiles for which query types)

Write 10-20 master rules that subsume the individual ones. Store at higher importance:

```sh
hebbs remember "MASTER-RULE: Default k=10 for all non-trivial queries. Only use k=5 for simple factual lookups with unique entity names." --importance 0.95 --entity-id retrieval-instructions --global --format json
```

### Delete granular strategies

Once master rules are stored:

```sh
hebbs forget --entity-id retrieval-instructions --access-floor 2 --global
```

This removes low-engagement individual strategies while keeping the master rules (which were just stored and have low access count, so use `--access-floor 2` to only remove the old ones that have been accessed during compression).

---

## Phase 7: Export to markdown

This is the end state. Tell the user:

"I've learned [N] retrieval strategies for your vault. I can save these as a file that loads into my context before every conversation. This is faster than recalling from HEBBS each time, and you can review and edit the rules yourself."

### Generate the rules file

```sh
# Read all master rules
hebbs recall "retrieval instructions master rules" --entity-id retrieval-instructions -k 30 --global --format json
```

Compile into a markdown file at `.hebbs/retrieval-rules.md` in the vault (or `~/.hebbs/retrieval-rules.md` for global):

```markdown
# Retrieval Rules

These rules were learned through eval-tune cycles on this vault.
Load this file into agent context before making recall calls.

## Cue Construction
- Always expand acronyms: "SOC2" -> "SOC 2 Type II"
- Always include entity names in cues: "Cloudvault", "Ironclad"
- For broad queries, list 2-3 specific subtopics in the cue

## k Sizing
- Default: k=10
- Simple factual with unique entity: k=5
- Broad sweep or cross-entity: k=15

## Strategy Selection
- Factual lookup: similarity (default)
- Timeline/change: temporal + entity-id + weights 0.2:0.6:0.1:0.1
- Cross-entity comparison: analogical, alpha=0.5
- Cause-effect: causal + seed ID + max-depth 3

## Weight Profiles
- Default: 0.5:0.2:0.2:0.1
- Recency-first: 0.2:0.6:0.1:0.1
- Importance-first: 0.3:0.1:0.5:0.1
- Pure semantic: 0.8:0.1:0.05:0.05

## Domain-Specific Rules
[Rules specific to this vault's content]
```

Tell the user: "This file should be referenced in your agent's prompt or SKILL.md so it loads before any HEBBS tool calls. The agent reads these rules, then makes better recall calls from the first query."

### How the rules file gets used

The agent (or the skill that loads the agent) should read `.hebbs/retrieval-rules.md` at conversation start, before making any `hebbs recall` calls. The rules modify how the agent constructs cues, selects strategies, sets weights, and sizes k. This is the compiled output of all tuning work.

---

## Phase 8: Re-tune when needed

Run the eval loop again when:
- New content is added to the vault (significant batch, not single files)
- The user reports retrieval misses ("it couldn't find X")
- You switch LLM or embedding model
- 30+ days since last tune

Each iteration:
1. Re-run existing evals against current vault state
2. Add new evals for new content
3. Score, analyze, tune
4. Update master rules and the rules file

First pass: biggest gains (20-30pp). Subsequent passes: refinement (2-5pp each).

---

## Scorecard

Track results across tune sessions:

```
Client: _______________
Domain: _______________
Profile: Legal / Sales / Engineering / Research
Vault:  ___ files, ___ memories, ___ entities

| Run  | Date | Embedding | LLM | Evals | Baseline | Tuned | Delta | Perfect | Zero-hit |
|------|------|-----------|-----|-------|----------|-------|-------|---------|----------|
| 1    |      |           |     |       |    %     |   %   |  +pp  |   /     |    /     |
| 2    |      |           |     |       |    %     |   %   |  +pp  |   /     |    /     |

Top failure patterns:
1. _______________
2. _______________

Master rules stored: ___
Rules file: .hebbs/retrieval-rules.md
```

---

## Reference: expected results

| Config | Baseline | Tuned | Notes |
|---|---|---|---|
| gpt-4o-mini + local gemma (768d) | 54% | 84% | Entity extraction works, slow indexing |
| gpt-4o + OpenAI embed (1536d) | 75% | 90% | Embedding quality is biggest lever |
| gpt-4o-mini + OpenAI embed (1536d) | ~75% | ~92% | Best of both: extraction + embeddings |

**Biggest lever**: embedding model quality (+21pp baseline with zero tuning). After that: cue expansion + k sizing (+15pp). After that: strategy/weight tuning (+5-10pp). The extraction ceiling (facts not extracted as propositions) is the final gap (5-10pp), addressable only by using a better LLM for indexing.
