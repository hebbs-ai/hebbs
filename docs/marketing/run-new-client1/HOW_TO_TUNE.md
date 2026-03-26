# How to Tune HEBBS for a New Client

A step-by-step guide for running the eval-tune loop on a new client's vault. The goal: take their real content, prove HEBBS retrieves it well, and show measurable improvement through tuning.

---

## Step 0: Understand the Client

Before touching HEBBS, understand who you're tuning for.

### Identify the ICP

| Question | Why it matters | How it shapes evals |
|---|---|---|
| What is their domain? | Legal, sales, engineering, research, support? | Determines query vocabulary and expected precision level |
| What do they search for? | Facts, timelines, decisions, contradictions? | Determines which strategies matter (similarity vs temporal vs causal) |
| Who is the end user? | AI agent, human analyst, both? | Agents need JSON + high k; humans need readable + focused results |
| What does "wrong answer" cost? | Compliance risk, lost deal, wasted time? | High-stakes domains need higher recall targets (90%+) |
| How big is their corpus? | 10 files, 100, 1000? | Affects eval count, indexing time, and k sizing |

### Common ICP Profiles

**Legal/Compliance teams**: Search contracts, policies, audit findings. Care about exact dollar amounts, dates, entity names. Need temporal recall ("what changed since last audit") and contradiction detection ("policy A says X, policy B says Y").

**Sales/Revenue teams**: Search call notes, deal context, competitor intel. Care about recency ("latest conversation with Acme") and cross-entity patterns ("which deals have similar objections"). Need fast priming at conversation start.

**Engineering teams**: Search architecture decisions, incident postmasters, runbooks. Care about causal chains ("what caused the outage") and revision history ("when did we change the auth approach"). Need entity-scoped recall.

**Research/Knowledge teams**: Search papers, notes, meeting summaries. Care about analogical recall ("what other projects had similar findings") and importance-weighted results. Need broad k with good ranking.

---

## Step 1: Index Their Content

```sh
hebbs init /path/to/client/vault --provider openai --key $OPENAI_API_KEY
hebbs index /path/to/client/vault
```

Note the output:
- How many files indexed
- How many memories created (propositions + document sections)
- How many entities extracted

These numbers set expectations. A 50-file vault with 1000+ memories has enough density for meaningful eval. A 5-file vault with 30 memories will hit extraction ceilings fast.

---

## Step 2: Generate Evals

### How many evals?

| Vault size | Minimum evals | Target evals | Why |
|---|---|---|---|
| 5-10 files | 10 | 20 | Small corpus, limited query diversity |
| 20-50 files | 20 | 50 | Enough to cover all strategies and entity groups |
| 50-200 files | 30 | 100 | Need cross-file queries, analogical, temporal coverage |
| 200+ files | 50 | 200+ | Scale testing, entity fragmentation surfaces here |

Start with 20. You can always add more after the first pass reveals gaps.

### How to create good evals

Each eval is a tuple: `(query, expected_keywords, query_type)`

**Read the content first.** Skim 5-10 representative files. Note:
- Key entities (company names, people, products, project names)
- Key facts (dollar amounts, dates, decisions, metrics)
- Relationships (who depends on what, what caused what, what revised what)
- Contradictions (if two files disagree on a fact, that's a gold eval)

### Query type distribution

Not all queries are the same. Distribute evals across types based on the client's ICP:

| Query type | What it tests | Example | Strategy |
|---|---|---|---|
| **Factual lookup** | Can HEBBS find a specific fact? | "What is the ransomware coverage limit?" | similarity |
| **Entity-scoped** | Can HEBBS isolate by entity? | "All findings related to Cloudvault" | similarity + entity-id |
| **Temporal** | Can HEBBS order by time? | "How has the data retention policy changed?" | temporal |
| **Cross-entity** | Can HEBBS find patterns across entities? | "Which vendors have similar compliance gaps?" | analogical |
| **Causal** | Can HEBBS trace cause-effect? | "What led to the contract renegotiation?" | causal |
| **Recency-weighted** | Can HEBBS prioritize recent info? | "Latest update on RISK-001" | similarity + weights 0.3:0.5:0.2:0 |
| **Contradiction** | Can HEBBS surface conflicting facts? | "What are the conflicting coverage limits?" | similarity (high k) |
| **Broad sweep** | Can HEBBS find across many files? | "All references to SOC 2 Type II" | similarity (k=15+) |

**For a legal ICP**: heavy on factual lookup (40%), entity-scoped (20%), temporal (15%), contradiction (15%), cross-entity (10%).

**For a sales ICP**: heavy on recency-weighted (30%), entity-scoped (25%), factual lookup (20%), cross-entity (15%), temporal (10%).

**For an engineering ICP**: heavy on causal (25%), entity-scoped (25%), factual lookup (20%), temporal (20%), broad sweep (10%).

### What makes a good eval vs a bad one

**Good evals:**
- Use natural language the client would actually type
- Have 3-5 specific expected keywords (not vague concepts)
- Include at least 3-4 "hard" queries that test edge cases
- Cover multiple files (not just one document)
- Include entity names that exist in the corpus

**Bad evals:**
- Too vague: "tell me about compliance" (what keywords do you even expect?)
- Too easy: query is literally a section heading (of course it'll match)
- Wrong domain vocabulary: using terms the corpus doesn't contain
- Only testing similarity: ignoring temporal, causal, analogical entirely
- All from one file: doesn't test cross-document retrieval

### Eval template

```
Q1: "What is Acme Corp's data retention policy?"
   Expected: [data_retention, 90_days, acme, quarterly_review, compliance]
   Type: factual_lookup

Q2: "How has the vendor risk assessment changed over the past year?"
   Expected: [risk_assessment, cloudvault, q1_review, updated_controls, remediation]
   Type: temporal

Q3: "Which vendors have similar gaps in their SOC 2 reports?"
   Expected: [soc2, cloudvault, ironclad, access_controls, monitoring_gaps]
   Type: cross_entity
```

---

## Step 3: Run Baseline

Run every eval query with default settings:

```sh
hebbs recall "query text here" -k 5 --format json
```

Score each query: count how many expected keywords appear in the returned results.

```
Per query:   keywords_found / keywords_expected
Overall:     sum(found) / sum(expected) = baseline %
```

Track:
- Total keyword recall %
- Number of perfect queries (all keywords found)
- Number of zero-hit queries (no keywords found)
- Which query types perform worst

**Expected baseline**: 50-60% with local embeddings, 70-75% with OpenAI embeddings.

---

## Step 4: Analyze Failures

For every query that scored below 100%, classify the failure:

| Pattern | Symptom | Fix |
|---|---|---|
| **k too low** | Keywords exist in results 6-10, but you only pulled 5 | Increase to k=10 or k=15 |
| **Cue too generic** | Results are topically related but wrong section | Expand cue: "SOC2" becomes "SOC 2 Type II audit findings access controls" |
| **Missing entity names** | Right topic, wrong entity's version | Add entity name to cue: "Cloudvault vendor risk assessment" |
| **Wrong strategy** | Timeline question returns random order | Switch to temporal with entity-id, or adjust weights |
| **Extraction ceiling** | The fact was never extracted as a proposition | Accept gap or re-index with better LLM |

The first three patterns cover 80% of failures. Fix those first.

---

## Step 5: Tune and Re-run

Apply fixes per query and re-run:

```sh
# Was: hebbs recall "SOC2 policy" -k 5
# Now:
hebbs recall "SOC 2 Type II audit findings access controls Cloudvault" -k 10 --format json

# Temporal query:
hebbs recall "data retention policy changes" --strategy temporal --entity-id data_retention -k 10 --format json

# Recency-weighted:
hebbs recall "latest Cloudvault risk update" --weights 0.3:0.5:0.2:0 -k 10 --format json
```

Score again with the same methodology. Compare before/after per query and overall.

**Expected improvement**: +20-30pp over baseline. If you're not seeing this, the evals may be too easy (already high baseline) or the extraction quality is the bottleneck.

---

## Step 6: Store Learnings

Store each successful tuning strategy as a retrieval instruction:

```sh
hebbs remember "RETRIEVAL-INSTRUCTION: For compliance/audit queries, always include the entity name (vendor, standard) and expand acronyms. Use k=10 minimum. Example: 'SOC2 policy' becomes 'SOC 2 Type II audit findings access controls [vendor name]'" --importance 0.9 --entity-id retrieval-instructions --global --format json
```

Store 5-15 individual strategies from the first tune pass.

---

## Step 7: Compress Rules

After 2-3 tune sessions, the agent will have 20-50 individual retrieval instructions. Compress them:

1. `hebbs recall "retrieval instructions" --entity-id retrieval-instructions -k 50 --global --format json`
2. Read all stored strategies
3. Group by pattern (cue expansion, k sizing, strategy selection, weight tuning)
4. Write 10-20 master rules that subsume the individual ones
5. Store master rules at importance 0.95
6. Delete the granular ones: `hebbs forget --entity-id retrieval-instructions --access-floor 2 --global`

Master rules are what the agent loads at conversation start. They're the compressed knowledge of how to retrieve well from this client's domain.

---

## Step 8: Iterate

Run the eval loop again after:
- New content is added to the vault
- Client reports retrieval misses
- Strategy coverage expands (new query types)
- LLM or embedding model changes

Each iteration should show diminishing but real improvement. The first pass gets the biggest gains (20-30pp). Subsequent passes refine edge cases (2-5pp each).

---

## Scorecard Template

```
Client: _______________
Domain: _______________
Vault:  ___ files, ___ memories, ___ entities
Evals:  ___ queries

| Run | Embedding | LLM | Baseline | Tuned | Delta | Perfect | Zero-hit |
|-----|-----------|-----|----------|-------|-------|---------|----------|
| 1   |           |     |    %     |   %   |  +pp  |   /     |    /     |
| 2   |           |     |    %     |   %   |  +pp  |   /     |    /     |
| 3   |           |     |    %     |   %   |  +pp  |   /     |    /     |

Top failure patterns:
1. _______________
2. _______________
3. _______________

Master rules stored: ___
```

---

## Reference: What Worked in Previous Runs

| Config | Baseline | Tuned | Notes |
|---|---|---|---|
| gpt-4o-mini + local gemma (768d) | 54% | 84% | Entity extraction worked (63% coverage), slow indexing |
| gpt-4o + OpenAI embed (1536d) | 75% | 90% | Embedding quality is biggest lever (+21pp), but entity extraction broke |
| Ideal (untested) | ~75% | ~92% | gpt-4o-mini for extraction + OpenAI embeddings for retrieval |
