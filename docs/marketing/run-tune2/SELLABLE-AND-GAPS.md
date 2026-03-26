# Run-Tune-2: What's Sellable and What's Not

**Date:** 2026-03-25
**Tested on:** 0.3.1, 52 files, 1171 memories, enterprise legal vault

---

## Sellable Now

### 1. The Eval Tuning Loop (strongest differentiator)

54% → 84% keyword recall with agent-driven tuning. The agent generates eval queries, runs them against defaults, reasons about failures, tunes strategy/weights per query type, and proves measurable improvement. No competitor can show this because no competitor exposes tunable parameters.

**Demo format:** Show Claude Code generating 20 evals, running baseline, analyzing 18 failures, tuning, re-running. 54% → 84% in 60 seconds.

**Blog format:** "How an AI agent taught itself to remember better, with proof." Real numbers, real commands, reproducible.

### 2. Temporal Recall on Indexed Content

`hebbs index .` then `--strategy temporal --entity-id ransomware` returns 8 chronologically ordered propositions about insurance coverage. No manual entity tagging required. The LLM extracted "ransomware" as an entity during indexing automatically. This was completely broken in run-tune-1. Now it works out of the box.

**Demo format:** One command. 8 results in chronological order. "Your agent can answer 'what changed over time' without you building a timeline."

### 3. Four Strategies, Visibly Different Results

Same vault, four recall commands, four types of answers:
- Similarity: "What is our ransomware coverage?" returns $2M and $500K facts
- Temporal: "ransomware coverage changes" returns chronological evolution with entity scoping
- Recency-weighted: "RISK-001 Cloudvault dependency" returns risk register evolution HIGH → MEDIUM
- Analogical: "Which vendors have similar compliance gaps?" returns cross-entity structural patterns

The flags change per query. The viewer sees the agent choosing its retrieval approach.

**Demo format:** Side-by-side or sequential, showing the different `--strategy` and `--weights` flags producing qualitatively different results.

### 4. Proposition Extraction Quality

1119 memories from 52 files. Dense legal contracts decomposed into atomic facts. "The ransomware coverage is up to $2,000,000 per incident" as its own searchable memory, not a 200-line contract section. This is why recall works.

**Demo format:** Show a raw contract file, then show the extracted propositions. "Every fact becomes a searchable memory."

### 5. Decay Scoring

Clean correlation visible in a single query:
- 0 accesses: decay = 0.500 (baseline)
- 1 access: decay = 0.575
- 2 accesses: decay = 0.619

"Your brain forgets what you don't use. Frequently accessed memories strengthen."

**Demo format:** 10-second visual. Show scores side by side.

### 6. Document-Level Revision Detection

RevisedFrom edges (conf=0.90) correctly link data retention policy v2 back to v1. The graph knows which document supersedes which without anyone telling it.

**Demo format:** `hebbs inspect` on a policy memory showing the RevisedFrom edge. "Hebbs knows v2 replaced v1."

---

## Gaps (Not Sellable Yet)

### 1. Cue Quality Sensitivity

"SOC2 policy" returns 9 irrelevant policy docs. "SOC 2 Type II audit findings controls" returns 10 perfect results. The word "policy" hijacks the embedding, matching every policy document instead of SOC 2 content.

**Impact:** An agent without learned cue expansion strategies will get poor results on ambiguous queries. The eval loop fixes this (agent learns to expand "SOC2" → "SOC 2 Type II audit findings controls"), but it's not automatic.

**Fix:** This is actually the eval loop's value proposition. The gap IS the pitch: "out of the box, retrieval is 54%. After the agent self-tunes, it's 84%. The difference is learned cue expansion and strategy selection."

### 2. Entity ID Naming Inconsistency

Same vendor appears as three entity_ids:
- "cloudvault"
- "cloudvault systems, llc"
- "cloudvault systems, inc."

Temporal queries on one variant miss memories tagged with another. 29 unique entities extracted, but some are duplicates with different naming.

**Impact:** Temporal recall works but is fragmented. `--entity-id cloudvault` misses memories tagged as `cloudvault systems, llc`.

**Fix:** Entity normalization during extraction. Deduplicate entity names to canonical forms.

### 3. Proposition-Level Contradiction Edges Not Created

Document-level RevisedFrom edges work (v2 revises v1). But specific proposition contradictions ($2M vs $500K, 24 months vs 36 months, 15 minutes vs 30 minutes) don't get Contradicts edges between the individual propositions.

**Impact:** No red edges in Memory Palace for the dramatic contradiction demo moment. The agent still catches contradictions through recall (both values surface in the same query), but it's not automatic flagging.

**Fix:** Contradiction detection needs to compare propositions across documents, not just detect document-level revision.

### 4. Analogical Recall Inconsistency

Works well for some queries (vendor risk assessment gaps: finds Meridian + Praxis). Fails completely for others (cross-vendor compliance gaps: 0/5 keywords). Can't predict when it will find structural patterns.

**Impact:** Can't reliably demo analogical as a feature. It works sometimes but not consistently enough to put in a video.

**Fix:** Needs investigation into why structural matching connects some entity types but not others.

### 5. Indexing Speed

22 minutes for 52 files with gpt-4o-mini. The parallel extraction commit landed but didn't improve wall-clock time. Still sequential per-file LLM calls effectively.

**Impact:** "One command, your brain is ready" has a 22-minute gap. For demo, must pre-index.

**Fix:** Concurrent real-time API calls (10-20 in flight simultaneously) instead of batch API. Would reduce to 2-3 minutes.

### 6. Q15 Consistently Fails

"Cross-vendor compliance gaps" scores 0/5 in both run-tune-1 and run-tune-2. Analogical returns policy docs instead of vendor-specific assessments. The structural matching can't connect entities across document types for this particular query pattern.

**Impact:** Minor (one query), but it shows analogical's limits.

---

## Recording Priority

For a video or blog right now:

1. **Eval loop** (centerpiece, unique, proven)
2. **Temporal on indexed content** (new, impressive, one command)
3. **Four strategies visual** (differentiation from competitors)
4. **Proposition extraction** (shows why it works)
5. **Decay** (quick visual, easy to understand)

Skip: contradictions (not flagged automatically), analogical (inconsistent), live indexing (too slow).
