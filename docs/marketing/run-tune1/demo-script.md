# Demo Script: "Rewire Your Agent's Brain"

**Format:** Screen recording with CLI commands. Text overlays for context. Agent shown only during eval section. ~5 minutes.
**Demo vault:** `/Users/paragarora/Documents/Workspace/archives/hebbs-demos/enterprise-legal/` (52 files, in-house legal team at Nexus Technologies)
**Tested:** 2026-03-24 with hebbs 0.3.0, OpenAI gpt-4o-mini backend

---

## SCENE 1: "The problem nobody talks about" (30 seconds)

Black screen. Text:

```
You're using a memory solution for your AI agents.

It works. Sort of.

But here's what nobody tells you:
```

Text changes:

```
No single retrieval configuration is optimal
for every query your agent makes.

"What changed since Q3?" needs recency.
"What caused this incident?" needs causal traversal.
"What's our policy on X?" needs importance.
"Are there patterns across vendors?" needs structural similarity.

One-size-fits-all retrieval is leaving answers on the table.
```

Text changes:

```
What if your agent could rewire how it retrieves
for every single query?

Introducing HEBBS.
```

---

## SCENE 2: "Setup in 30 seconds" (40 seconds)

Terminal. Folder of 52 legal markdown files visible.

```
$ brew install hebbs-ai/tap/hebbs
```

```
$ hebbs init . --provider openai --model gpt-4o-mini --api-key-env OPENAI_API_KEY
LLM provider validated successfully
Initialized vault at .
Ensuring embedding model (embeddinggemma-300m)...
Embedding model ready.
Starting daemon...

  Your vault is live. 52 file(s) found.
```

Text overlay: *`.hebbs/` is your brain. Portable, self-contained. Copy it to another machine and your memory comes with you.*

```
$ hebbs index .
  Phase 1/2: parsing 52 file(s)...
  Phase 2/2: embedding 465 section(s)...
Indexed 52 file(s). 949 memories created.
```

Text overlay: *Sections + propositions. Every fact extracted as an atomic memory. Use `.hebbsignore` to control what gets indexed.*

```
$ hebbs panel
```

Memory Palace opens. Graph fills in. Nodes, edges, clusters forming.

Text overlay: *Every fact extracted. Every relationship mapped. Every contradiction detected. This is your brain.*

Hold for 3 seconds on the graph.

---

## SCENE 3: "Recall is not search" (90 seconds)

Back to terminal.

**Query 1: Standard similarity recall**

```
$ hebbs recall "What is our ransomware coverage?" --format json
```

Results come back. Clean JSON. Shows:
- "$2,000,000 per incident" coverage from board minutes
- Ransomware negotiation services via CyberResolve Partners
- $500,000 ransomware payment sublimit from TrueNorth endorsement
- OFAC sanctions compliance requirements
- Cloudvault's $10M cyber liability reference

Text overlay: *That's a standard similarity recall. But Hebbs has four retrieval strategies, not one.*

**Query 2: Recency-weighted recall**

```
$ hebbs recall "RISK-001 single cloud provider Cloudvault dependency risk rating" \
    --weights 0.3:0.5:0.2:0 \
    -k 10 \
    --format json
```

Results: all three risk registers in order + supporting context:
- Q4 2024: HIGH risk, single cloud dependency
- Q1 2025: Cloudvault MSA renewed with improved SLA terms
- Q2 2025: REDUCED TO MEDIUM, EU deployment operational

Text overlay: *Weights are relevance:recency:importance:reinforcement. Tuning recency to 0.5 pulls in time-ordered evolution across risk registers.*

**Query 3: Analogical recall**

```
$ hebbs recall "Which vendors have similar compliance gaps?" \
    --strategy analogical \
    --analogical-alpha 0.3 \
    --format json
```

Results: structural matches across vendor assessments:
- Meridian and Praxis: incomplete risk assessments (SOC 2 finding)
- Multiple vendors lacking EU data processing guarantees
- Cross-vendor Tier 1 classification patterns

Text overlay: *Analogical strategy. Low alpha finds structural patterns across entities, not just textual similarity.*

**Query 4: Causal traversal** (requires entity-scoped memories with edges)

```
$ hebbs recall "Why is Meridian flagged as critical?" \
    --strategy causal \
    --seed <MERIDIAN_RISK_MEMORY_ID> \
    --max-depth 3 \
    --edge-types caused_by \
    --format json
```

Results: the full causal chain:
1. DPA signed covering EU data processing (March 2024)
2. Subprocessor change without notification (Q4 2024)
3. DPA Section 4.2 violation identified
4. Elevated to critical risk tier (Q1 2025)

Text overlay: *Causal strategy. Traverses the knowledge graph along cause-effect edges. Not similarity. Causality. Requires entity-scoped memories with edges -- your agent creates these as it learns.*

Pause. Text overlay on black:

```
Four strategies. Four scoring dimensions.
Every parameter tunable per query.

Your agent doesn't use one setting for everything.
It decides how to retrieve based on what it's retrieving.

This is not hardcoded. Every situation demands different tuning.
```

Brief mention with text overlay:

```
Memories also decay over time. Stale knowledge fades.
Frequently accessed memories strengthen.
Contradictions between memories are detected automatically.

More on both of these in a moment.
```

---

## SCENE 4: "Your agent can learn how to use this" (30 seconds)

Text overlay on black:

```
Your agent can choose these parameters by itself.
A well-written skill file teaches it when to use
temporal vs. causal vs. analogical.

But how do you KNOW it's choosing well?
```

Text:

```
You train it.

Not with hope. With evals.
```

---

## SCENE 5: "The eval loop" (90 seconds)

Now we show the agent. Claude Code opens.

User types:

```
> Generate 20 eval queries for this legal vault.
  For each query, specify expected keywords that
  should appear in results and query type.
```

Agent reads the vault with `hebbs list --format json`. Reasons about the documents. Generates eval queries:

```json
[
  {
    "id": 1,
    "query": "What is our ransomware coverage limit?",
    "type": "similarity",
    "expected_keywords": ["$2,000,000", "500,000", "ransomware", "TrueNorth"]
  },
  {
    "id": 5,
    "query": "Why is Meridian flagged as critical risk?",
    "type": "causal",
    "expected_keywords": ["subprocessor", "OracleScale", "DPA", "critical"]
  },
  {
    "id": 8,
    "query": "How has RISK-001 changed from Q4 2024 to Q2 2025?",
    "type": "temporal",
    "expected_keywords": ["single cloud", "Cloudvault", "HIGH", "MEDIUM", "reduced"]
  },
  ...17 more
]
```

Text overlay: *The agent generates the evals. Not Hebbs. The agent is smarter than any built-in eval generator would be.*

User types:

```
> Now run each eval query through hebbs recall with
  default settings (similarity, k=5). Score keyword recall.
```

Agent runs 20 `hebbs recall` commands. Compares results against expected keywords. Reports:

```
Baseline: 50/85 keywords found (59% recall)
16/20 queries with missed keywords

  Q4  [temporal]:   "Timeline of Ironclad SLA changes"
      Missed: ['15 minutes', '30 minutes']
  Q5  [causal]:     "Why is Meridian flagged as critical?"
      Missed: ['OracleScale', 'DPA']
  Q7  [similarity]: "Nov 2024 phishing attack?"
      Missed: ['three employees', 'credentials', 'Ironclad', '42 minutes']
  Q8  [temporal]:   "RISK-001 changed Q4 2024 to Q2 2025?"
      Missed: ['single cloud', 'Cloudvault', 'HIGH', 'reduced']
  ...12 more
```

User types:

```
> Analyze the failures. Figure out what strategy, weights,
  and k would fix each one. Re-run and compare.
```

Agent reasons through each failure:

```
Q4: "Timeline of Ironclad SLA changes"
  Missed SLA numbers. Only 5 results, not enough context.
  Fix: k=10 + importance weights 0.3:0.3:0.4:0

Q5: "Why is Meridian flagged as critical?"
  Missing OracleScale and DPA. Too few results to surface full chain.
  Fix: Include entity names in cue, k=10

Q7: "November 2024 phishing attack?"
  Propositions spread across sections. k=5 only gets top-level.
  Fix: Include entity name + k=10

Q8: "RISK-001 evolution"
  Need cross-file temporal context. Default weights miss it.
  Fix: Recency weights 0.3:0.5:0.2:0 + k=10 + specific entity names in cue

Q15: "Cross-vendor compliance gaps"
  Single-vendor results. Need structural matching.
  Fix: --strategy analogical --analogical-alpha 0.3 + k=10
```

Agent re-runs with adjusted strategies:

```
Tuned: 75/85 keywords found (88% recall)
4/20 queries with minor gaps (specific dollar amounts, proper nouns)

  59% -> 88% keyword recall
  25 additional keywords surfaced
```

User types:

```
> Store what you learned so you use these strategies
  in every future conversation.
```

Agent runs:

```
$ hebbs remember "For temporal queries (timeline, evolution, changes over time), \
    use --weights 0.3:0.5:0.2:0 with k=10" --importance 0.9

$ hebbs remember "For cross-entity pattern queries (which vendors, compare across), \
    use --strategy analogical --analogical-alpha 0.3 with k=10" --importance 0.9

$ hebbs remember "For causal queries (why did X happen), include specific entity \
    names in the cue and use k=10" --importance 0.9

$ hebbs remember "Default k=5 often misses supporting context. Use k=10 for most \
    queries and k=5 only for simple factual lookups" --importance 0.85
```

Text overlay:

```
The agent evaluated its own retrieval.
Reasoned about failures. Tuned parameters.
Stored strategies in the same memory engine it optimized.

59% to 88% keyword recall. In 60 seconds.

We ran 20 evals here. In production, you run thousands.
The more evals, the sharper your agent's retrieval becomes.
And it's different for every domain, every use case, every agent.
```

---

## SCENE 6: "Every agent gets its own brain" (20 seconds)

Text overlay on black:

```
A legal agent tunes for contradiction detection
and temporal document evolution.

A sales agent tunes for cross-entity pattern
recognition and recency.

A coding agent tunes for causal traversal
and high-importance recall.

Same engine. Different tuning. Every agent
gets a brain optimized for its job.

This is what "rewire your brain" means.
```

---

## SCENE 7: "It's alive" (60 seconds)

Back to terminal. Two features we mentioned earlier.

**Decay:**

```
$ hebbs recall "Q3 vendor review notes" --format json
```

Results come back. Show the scores -- frequently accessed memories have higher decay scores:
```
  score=0.612  decay=0.694  access=5  | vendor-risk-assessment-cloudvault.md
  score=0.579  decay=0.650  access=3  | soc2-remediation-tracker.md
  score=0.542  decay=0.575  access=1  | vendor-risk-assessment-cloudvault.md
  score=0.534  decay=0.500  access=0  | memo-data-residency-requirements.md
  score=0.530  decay=0.500  access=0  | soc2-audit-findings-2024.md
```

Text overlay: *Memories decay over time unless reinforced. Stale knowledge fades. Important, frequently accessed memories strengthen (0.694 vs 0.500). Your brain stays current automatically.*

**Contradictions:**

Open a markdown file in the editor. Change Cloudvault's SLA from 99.9% to 99.95%.

Save the file. Wait for the daemon to re-index.

```
$ hebbs recall "Cloudvault SLA" --format json
```

Results show both the original and the updated value. A contradiction edge is flagged.

Cut to Memory Palace. Red dashed edge between the two nodes.

```
$ hebbs contradiction-prepare --format json
```

Shows the pending contradiction: original SLA vs. updated SLA.

Text overlay: *Change a file. The daemon re-indexes. If new content conflicts with existing knowledge, Hebbs flags it automatically. Red edges in the Memory Palace. Your agent sees them before you do.*

---

## CLOSING (15 seconds)

Black screen.

```
One binary. Zero infrastructure. Your data never leaves.

Four retrieval strategies your agent controls.
Eval-driven tuning that makes every agent sharper.
Automatic decay, contradiction detection, and insight generation.

The memory engine for AI agents.
```

```
brew install hebbs-ai/tap/hebbs

hebbs.ai
```

---

## Runtime: ~5 minutes 15 seconds

## Clips to Cut

| Clip | Scene | Length | Hook |
|---|---|---|---|
| "Four strategies, not one" | 3 | 40s | Show similarity vs analogical vs recency-weighted side by side |
| "59% to 88%" | 5 | 45s | Agent tuning its own retrieval |
| "It remembers how to remember" | 5, end | 20s | Agent storing strategies |
| "Every agent, different brain" | 6 | 20s | Legal vs sales vs coding tuning |
| "Change a file, catch a contradiction" | 7 | 25s | Live contradiction detection |
| "One command" | 2 | 15s | Install to Memory Palace |

## Pre-Recording Checklist

- [ ] Hebbs binary installed and working via brew
- [ ] Enterprise legal vault (52 files) at demo path
- [ ] Vault indexed, daemon running, 949+ memories created
- [ ] Memory Palace accessible and visually populated
- [ ] Terminal font and theme camera-ready (large font, dark theme, high contrast)
- [ ] Claude Code configured with hebbs skill for Scene 5
- [ ] Pre-store entity-scoped memories for causal demo (Meridian chain with caused_by edges)
- [ ] Test similarity, analogical, and recency-weighted recall produce clean output
- [ ] Test causal traversal with seed memory ID produces the Meridian chain
- [ ] Test contradiction detection with a live file edit (Cloudvault SLA 99.9% -> 99.95%)
- [ ] Test decay is visible in score differences (accessed vs unaccessed memories)
- [ ] Run 20 eval queries baseline and verify 59% recall
- [ ] Run tuned queries and verify 88% recall

## Key Findings from Testing

1. **Temporal strategy** requires entity_id on memories. Indexed file memories don't have entity_ids.
   - **Workaround:** Use recency-weighted similarity (`--weights 0.3:0.5:0.2:0`) for temporal queries on indexed content. Reserve `--strategy temporal` for agent-created memories with `--entity-id`.
2. **Causal strategy** requires graph edges (caused_by, followed_by) and a `--seed` memory ID.
   - **Workaround:** Pre-store entity-scoped memories with edges via `hebbs remember --entity-id X --edge TARGET:caused_by:0.9`. Include specific entity names in similarity cues for fallback.
3. **k=5 is too few** for most queries. The vault has 949 memories; k=10 catches supporting details that k=5 misses.
4. **Analogical works out of the box** with `--analogical-alpha 0.3` for cross-entity structural matching.
5. **Decay works** -- access_count drives decay_score reinforcement (0.500 baseline, 0.694 with 5 accesses).
6. **Including entity names in cues** dramatically improves results for specific queries.
7. **`hebbs rebuild`** takes ~40 minutes for 52 files with gpt-4o-mini. Plan accordingly.
8. **Contradiction detection** requires the `reflect` pipeline to run after indexing. On a fresh vault, `contradiction-prepare` returns 0 candidates until reflect has clustered memories and compared them. The vault has real contradictions baked in (ransomware coverage: board told $2M, endorsement limits to $500K; data retention: v1 says 3 years, v2 says 2 years; Ironclad P1 SLA: original 15 min, amended to 30 min). These should surface after reflect runs successfully.
9. **Rebuild resets propositions.** Initial `index` creates 949 memories (sections + propositions). `rebuild` creates only 465 (sections only, no propositions). If you need propositions, avoid rebuild and use incremental `index` instead.
