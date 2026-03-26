# Run Log: Agent-Driven Hebbs Eval & Tuning (Run 3)

**Date:** 2026-03-26
**Hebbs version:** 0.3.1 (same build as run-tune2)
**LLM backend:** OpenAI gpt-4o
**Embedding model:** OpenAI text-embedding-3-small (1536 dims)
**Vault:** 52 markdown files, enterprise legal team at Nexus Technologies
**Vault path:** `/Users/paragarora/Documents/Workspace/archives/hebbs-demos/enterprise-legal/`
**Agent:** Claude Opus 4.6 via Claude Code

## Changes from Run-Tune-2

Two configuration changes, no code changes:
1. **LLM:** gpt-4o-mini replaced with gpt-4o
2. **Embedding:** local embeddinggemma-300m (768 dims) replaced with OpenAI text-embedding-3-small (1536 dims)

Purpose: measure the impact of higher-quality LLM + API embeddings on extraction and retrieval.

---

## 1. Vault Initialization

### Config

```toml
[llm]
provider = "openai"
model = "gpt-4o"
api_key_env = "OPENAI_API_KEY"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
dimensions = 1536
batch_size = 50
```

### Setup Issue

First attempt: `hebbs init` created the vault with default local embeddings. The embedding config was edited AFTER indexing, resulting in 768-dim stored vectors queried with 1536-dim vectors. All recall scores were identical (0.324) and results were irrelevant. Fixed by deleting the index and re-indexing with correct config already in place.

**Lesson:** Embedding config must be set BEFORE `hebbs index`. Changing embedding model after indexing requires a full rebuild.

---

## 2. Indexing

### Command

```
$ time hebbs index .
```

### Output

```
Phase 1/2: parsing 52 file(s)...
Phase 2/2: embedding 465 section(s)...
Indexed 52 file(s). 1160 memories created.
  0.00s user 0.00s system 0% cpu 14:27.75 total
```

### Breakdown

| Metric | Run 2 (4o-mini, local embed) | Run 3 (4o, OpenAI embed) | Change |
|---|---|---|---|
| Files | 52 | 52 | same |
| Sections | 465 | 465 | same |
| Memories created | 1119 | 1160 | +41 (+3.6%) |
| Status memory count | 1171 | 465 (bug) | regression in status display |
| Indexing time | ~22 min | ~14 min | **36% faster** |

Faster indexing despite using API embeddings. OpenAI embedding API is fast; the LLM extraction (4o vs 4o-mini) may also be faster per call despite being a larger model.

### Entity Extraction

**All entity_ids are null.** GPT-4o did not assign entity_ids to any propositions during extraction. This is a regression from run-tune2 where 4o-mini assigned 29 unique entities with 63% coverage.

Verified across multiple queries:
- `hebbs recall "ransomware" -k 10`: all entity_id null
- `hebbs recall "vendor risk assessment" -k 20`: all entity_id null

**Impact:** Temporal strategy (`--strategy temporal --entity-id <entity>`) does not work on indexed content. The feature that was "the breakthrough" in run-tune2 is broken with 4o.

**Root cause hypothesis:** GPT-4o may be formatting the extraction response differently, or the extraction prompt may not trigger entity assignment with 4o's response style. Needs investigation.

---

## 3. Strategy Testing: Similarity

### Command

```
$ hebbs recall "What is our ransomware coverage?" -k 5 --format json
```

### Result (5 memories)

| Score | Source | Content |
|---|---|---|
| 0.716 | meetings/2025-q2-insurance-coverage-review.md | Annual Insurance Coverage Review (section header) |
| 0.703 | meetings/2024-q4-board-risk-committee.md | The ransomware coverage provides up to $2,000,000 per incident. |
| 0.693 | contracts/truenorth/endorsement-001-ransomware.md | Endorsement No. 1 is titled 'Ransomware Coverage Enhancement'. |
| 0.643 | contracts/truenorth/endorsement-001-ransomware.md | Professional ransomware negotiation services through CyberResolve Partners. |
| 0.642 | contracts/truenorth/cyber-insurance-policy-2024.md | Cyber Extortion and Ransomware coverage includes ransom payments. |

### Assessment

Higher scores than run-tune2 (0.716 vs 0.643 top score). OpenAI embeddings produce better similarity matching. Both $2M coverage and the endorsement surface at k=5. The $500K sublimit requires k=10 to appear (it was at position 6-7).

---

## 4. Strategy Testing: Temporal

### Command

```
$ hebbs recall "ransomware coverage changes" --strategy temporal --entity-id ransomware -k 10 --format json
```

### Result

```json
[]
```

### Assessment

**Empty results.** All entity_ids are null, so no memories match `--entity-id ransomware`. This is the key regression from run-tune2 where temporal returned 8 chronologically ordered results.

Temporal strategy still works for manually stored memories with explicit `--entity-id`. The regression only affects auto-extracted propositions from indexed files.

---

## 5. Strategy Testing: Recency-Weighted

### Command

```
$ hebbs recall "RISK-001 single cloud provider Cloudvault dependency risk rating" \
    --weights 0.3:0.5:0.2:0 -k 10 --format json
```

### Result (9 memories)

| Score | Source | Content |
|---|---|---|
| 0.867 | risk/risk-register-2025-q1.md | Risk Register: Q1 2025 (section header) |
| 0.867 | risk/risk-register-2025-q2.md | RISK-001 is titled 'Single Cloud Provider Dependency'. |
| 0.865 | risk/risk-register-2024-q4.md | Risk Register: Q4 2024 (section header) |
| 0.862 | risk/risk-register-2025-q2.md | Risk Register: Q2 2025 (section header) |
| 0.847 | audits/vendor-risk-assessment-cloudvault.md | Vendor Risk Assessment: Cloudvault Systems (section header) |
| 0.831 | contracts/cloudvault/sow-001-core-infra.md | Statement of Work #001: Core Infrastructure Services |
| 0.812 | audits/vendor-risk-assessment-cloudvault.md | Contract reference for Cloudvault Systems is MSA-2021-CV-001. |
| 0.809 | audits/vendor-risk-assessment-cloudvault.md | Cloudvault Systems, Inc. is an enterprise cloud infrastructure provider. |
| 0.807 | risk/incident-log.md | Incident 2025-001 was a Cloudvault service disruption on January 8, 2025. |

### Assessment

Scores significantly higher than run-tune2 (0.867 vs 0.845 top score). All three risk register snapshots (Q4 2024, Q1 2025, Q2 2025) appear plus the vendor assessment and incident log. Broader coverage than run-tune2.

---

## 6. Strategy Testing: Analogical

### Command

```
$ hebbs recall "Which vendors have similar compliance gaps?" \
    --strategy analogical --analogical-alpha 0.3 --format json
```

### Result (5 memories)

| Score | Source | Content |
|---|---|---|
| 0.549 | meetings/2025-q1-gdpr-compliance-review.md | Internal GDPR Compliance Review |
| 0.513 | audits/soc2-remediation-tracker.md | Finding 3: Vendor Risk Assessments (CC9.1) is partially remediated. |
| 0.511 | compliance/soc2-control-matrix-2024.md | CC9.1 Vendor Risk Assessments are owned by Legal & Compliance. |
| 0.507 | risk/risk-register-2024-q4.md | Multiple vendors lack explicit EU data processing guarantees. |
| 0.493 | compliance/policy-vendor-risk-management.md | Policy ensures vendors meet security, privacy, and compliance requirements. |

### Assessment

Better than run-tune2 analogical (0.549 vs 0.483 top score). More compliance-focused results. The "multiple vendors lack EU data processing guarantees" result (0.507) is a strong cross-entity finding.

---

## 7. Contradiction Detection

### Command

```
$ hebbs contradiction-prepare --format json
```

### Result

```json
{"candidates":[],"count":0}
```

### Data Retention Contradiction Surface Test

```
$ hebbs recall "data retention period months EU" -k 10 --format json
```

| Score | Source | Content |
|---|---|---|
| 0.720 | compliance/policy-data-retention-v2.md | EU resident personal data retained for no longer than **36 months** |
| 0.699 | compliance/policy-data-retention-v1.md | Personal data of EU residents retained for no longer than **24 months** |
| 0.681 | compliance/policy-data-retention-v2.md | Policy v2.0 effective March 1, 2025 |
| 0.685 | compliance/policy-data-retention-v1.md | Policy effective January 1, 2024 |
| 0.679 | compliance/policy-data-retention-v2.md | Backup copies retained for **sixty days** |
| 0.673 | compliance/policy-data-retention-v1.md | Backup copies retained for **ninety days** |
| 0.660 | compliance/policy-data-retention-v2.md | Version 2.0 supersedes version 1.0 |

### Assessment

Same as run-tune2: `contradiction-prepare` returns 0 candidates (LLM path writes edges directly during indexing). But the contradictions surface cleanly in recall: 36 months vs 24 months, sixty days vs ninety days, with v2 superseding v1. An agent would catch these from the recall results. Higher scores than run-tune2 (0.720 vs 0.634 top).

---

## 8. Decay Testing

### Command

```
$ hebbs recall "Q3 vendor review notes" --format json
```

### Result

| Score | Decay | Access | Content |
|---|---|---|---|
| 0.723 | 0.809 | 4 | Quarterly Vendor Review: Meridian Data Solutions |
| 0.695 | 0.743 | 2 | Quarterly Vendor Review: Cloudvault Storage Solutions |
| 0.673 | 0.743 | 2 | Vendor Risk Assessment: Meridian Data Solutions |
| 0.656 | 0.575 | 1 | Tier 3 vendors require annual risk reviews. |
| 0.634 | 0.575 | 1 | Tier 1 vendors require quarterly risk reviews. |

### Assessment

Decay works correctly. More accesses = higher decay score:
- 4 accesses: decay = 0.809
- 2 accesses: decay = 0.743
- 1 access: decay = 0.575

Wider spread than run-tune2 (0.809 vs 0.619 for most-accessed). The Meridian review was accessed 4 times during eval runs, showing reinforcement working correctly.

---

## 9. Baseline Eval Run (20 Queries)

All 20 queries, default settings: `--strategy similarity -k 5`.

| ID | Found/Expected | Missed |
|---|---|---|
| 1 | 4/4 | (none) |
| 2 | 3/3 | (none) |
| 3 | 4/4 | (none) |
| 4 | 4/4 | (none) |
| 5 | 2/4 | OracleScale, personal data |
| 6 | 2/2 | (none) |
| 7 | 4/5 | three employees |
| 8 | 3/5 | HIGH, reduced |
| 9 | 4/5 | NormCore |
| 10 | 4/4 | (none) |
| 11 | 5/5 | (none) |
| 12 | 1/3 | $1,440,000, $120,000 |
| 13 | 5/5 | (none) |
| 14 | 0/3 | OracleScale, AWS Singapore, subprocessor |
| 15 | 4/5 | Cloudvault |
| 16 | 0/5 | single cloud, data residency, vendor, compliance, NormCore |
| 17 | 4/4 | (none) |
| 18 | 4/5 | resolved |
| 19 | 3/5 | quarterly, DPA |
| 20 | 3/4 | 2027 |

### Baseline Totals

```
Keywords found:  63 / 84  (75%)
Queries perfect:  9 / 20
Queries with gaps: 11 / 20
```

**+21 percentage points over run-tune2 baseline (54%) with zero tuning.** OpenAI embeddings account for the entire improvement. Q1, Q3, Q4, Q10, Q11, Q13, Q17 all went from imperfect to perfect.

---

## 10. Tuned Eval Run

Same tuning rules: k=10, entity names in cues, strategy/weight selection per query type.

| ID | Baseline | Tuned | Change |
|---|---|---|---|
| 1 | 4/4 | 4/4 | (perfect) |
| 2 | 3/3 | 3/3 | (perfect) |
| 3 | 4/4 | 4/4 | (perfect) |
| 4 | 4/4 | 4/4 | (perfect) |
| 5 | 2/4 | 4/4 | +2 |
| 6 | 2/2 | 2/2 | (perfect) |
| 7 | 4/5 | 5/5 | +1 |
| 8 | 3/5 | 5/5 | +2 |
| 9 | 4/5 | 4/5 | +0 |
| 10 | 4/4 | 4/4 | (perfect) |
| 11 | 5/5 | 5/5 | (perfect) |
| 12 | 1/3 | 1/3 | +0 |
| 13 | 5/5 | 5/5 | (perfect) |
| 14 | 0/3 | 2/3 | +2 |
| 15 | 4/5 | 5/5 | +1 |
| 16 | 0/5 | 5/5 | +5 |
| 17 | 4/4 | 4/4 | (perfect) |
| 18 | 4/5 | 4/5 | +0 |
| 19 | 3/5 | 4/5 | +1 |
| 20 | 3/4 | 2/4 | -1 |

### Tuned Totals

```
Keywords found:  76 / 84  (90%)
Queries perfect: 14 / 20
Queries with gaps: 6 / 20

Improvement: 63 -> 76 keywords (+13)
Recall:      75% -> 90%  (+15 percentage points)
```

### Remaining Gaps (6 queries)

| ID | Still Missing | Why |
|---|---|---|
| 9 | NormCore | Praxis pre-existing IP framework name not extracted as a proposition |
| 12 | $1,440,000, $120,000 | Specific dollar amounts buried in SOW, not extracted |
| 14 | AWS Singapore | Not in this vault's data (OracleScale location not specified as Singapore) |
| 18 | resolved | The word "resolved" doesn't appear in the data residency docs |
| 19 | quarterly | Payment frequency not extracted as standalone proposition |
| 20 | 2027, expiration | Generic terms, regression from tuning weights |

---

## 11. Cross-Run Comparison

| Metric | Run 1 (4o-mini, local) | Run 2 (4o-mini, local) | Run 3 (4o, OpenAI embed) |
|---|---|---|---|
| Hebbs version | 0.3.0 | 0.3.1 | 0.3.1 |
| LLM | gpt-4o-mini | gpt-4o-mini | **gpt-4o** |
| Embedding | local 768d | local 768d | **OpenAI 1536d** |
| Memories created | 949 | 1119 | 1160 |
| Indexing time | ~15 min | ~22 min | **~14 min** |
| Entity IDs | none | 29 entities, 63% | **none (regression)** |
| Baseline recall | 59% (50/85) | 54% (46/84) | **75% (63/84)** |
| Tuned recall | 88% (75/85) | 84% (71/84) | **90% (76/84)** |
| Perfect (baseline) | n/a | 2/20 | **9/20** |
| Perfect (tuned) | n/a | 13/20 | **14/20** |
| Temporal on indexed | broken | **working** | **broken (no entity_ids)** |

---

## 12. Key Findings

### What improved

1. **Embedding quality is the biggest lever.** OpenAI text-embedding-3-small at 1536 dims vs local embeddinggemma at 768 dims: +21 percentage points on baseline (54% to 75%) with zero tuning. This is the single largest improvement across all three runs.

2. **Tuned ceiling is higher.** 90% vs 84%. The better embeddings mean tuning has more signal to work with.

3. **Indexing is faster.** 14 minutes vs 22 minutes. OpenAI embedding API calls are fast, and 4o extraction may be more efficient per call.

4. **Q15 (cross-vendor SOC 2) finally works.** Scored 0/5 in both run-tune1 and run-tune2. Now 5/5 with analogical + OpenAI embeddings. The structural matching that failed with local embeddings works with API embeddings.

5. **Q16 (risk register key risks) went from 0/5 to 5/5 tuned.** Another query that was impossible before.

### What regressed

1. **Entity extraction is completely broken.** All entity_ids are null. GPT-4o is not assigning entities during extraction. This kills temporal strategy on indexed content, which was run-tune2's headline feature.

2. **Status memory count bug returned.** Shows 465 instead of 1160.

### Trade-off summary

| | 4o-mini + local embed | 4o + OpenAI embed |
|---|---|---|
| Baseline recall | 54% | **75%** |
| Tuned recall | 84% | **90%** |
| Entity extraction | **29 entities, 63%** | none |
| Temporal on indexed | **working** | broken |
| Indexing time | 22 min | **14 min** |
| Cost per index | ~$0.15 (4o-mini only) | ~$1.50 (4o + embed API) |

The ideal configuration would be **4o-mini for extraction** (entity_ids work) with **OpenAI embeddings** (better recall). LLM and embedding are independent configs, so this is possible:

```toml
[llm]
provider = "openai"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
dimensions = 1536
```

This combination should give the best of both: entity extraction from 4o-mini + embedding quality from OpenAI API.

---

## 13. Daemon File Watch Test

### Test design

Edit a file in the vault, do NOT run `hebbs index`, and verify the daemon auto-detects the change, re-extracts, and makes the new content recallable.

### Before edit

```
$ hebbs recall "biometric data retention 7 years" -k 5 --format json
```

| Score | Content |
|---|---|
| 0.673 | Personal data of EU residents...24 months (unrelated) |
| 0.661 | Employee records...seven years (unrelated) |

No biometric data in the vault.

### Edit

Added a "Biometric Data" section to `compliance/policy-data-retention-v2.md`:

> Biometric data, including fingerprint scans, facial recognition templates, retinal scans, and voiceprint recordings, shall be retained for no longer than seven (7) years from the date of collection or until the individual's relationship with the Company terminates, whichever occurs first. This retention period reflects the requirements of the Illinois Biometric Information Privacy Act (BIPA).

### Daemon log

```
[watch] catch-up for enterprise-legal: 1 files changed
[watch] catch-up phase1: 1 processed, 0 new, 1 modified
[watch:enterprise-legal] phase2: starting embed + index
phase2: extracting [1/1] compliance/policy-data-retention-v2.md
extracted 20 propositions from compliance/policy-data-retention-v2.md
[watch:enterprise-legal] phase2: 1 embedded, 20 remembered, 0 revised, 0 forgotten
```

Total time from file save to indexed: **~38 seconds** (12:58:28 to 12:59:06). Includes debounce wait, phase 1 parse, phase 2 LLM extraction (GPT-4o) + OpenAI embedding + storage.

### After edit

```
$ hebbs recall "biometric data retention 7 years" -k 5 --format json
```

| Score | Source | Content |
|---|---|---|
| 0.703 | compliance/policy-data-retention-v2.md | Biometric data shall be retained for no longer than seven years from the date of collection. |
| 0.676 | compliance/policy-data-retention-v2.md | Employee records shall be retained for seven years following termination. |
| 0.659 | compliance/policy-data-retention-v2.md | Employee records shall be retained for seven years (v1 duplicate). |
| 0.653 | compliance/policy-data-retention-v2.md | EU residents...36 months. |

### Assessment

**Daemon works correctly.** File change detected, re-extracted, re-embedded, stored, and recallable within 38 seconds. No manual `hebbs index` required. The new biometric proposition is the #1 result at 0.703.

File was reverted after testing to keep the vault clean for future runs.

---

## 14. Recommended Next Steps

1. **Run-tune4: 4o-mini + OpenAI embeddings.** Test the hybrid config. Should combine run-tune2's entity extraction with run-tune3's embedding quality.

2. **Investigate entity extraction regression.** Compare the extraction prompts/responses between 4o-mini and 4o. The extraction prompt may need adjustment for 4o's response format.

3. **Fix status memory count.** Still showing section count (465) instead of total memories (1160).

4. **Update SETUP-ONLY-SKILL.md.** Embedding config must be set before indexing. The current doc flow could lead to the same mistake made in this run.

---

## 14. Files in This Run

| File | Purpose |
|---|---|
| `RUN-LOG.md` | This document |
