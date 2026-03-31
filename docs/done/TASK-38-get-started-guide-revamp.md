# TASK-38: Get Started Guide Revamp — Generic + Vertical Guides

**Status:** Planned  
**Priority:** High  
**Scope:** `hebbs-docs` (Astro pages)  
**Depends on:** TASK-37 (auto entity_id from folder convention)

---

## Problem

The current guide at `docs.hebbs.ai/guide/` is a single page showing Install → Index → Recall → Panel → Tune → CTA using a legal/compliance example. It doesn't show:

- The `entities/` folder convention (TASK-37)
- Multiple input paths (files, CLI, SDK)
- Contradiction detection
- Reflect/insights workflow
- How different verticals use the same primitives

## Solution

### Two-tier guide architecture

```
docs.hebbs.ai/guide/          ← Generic: "How any agent uses Hebbs"
docs.hebbs.ai/guide/crm       ← "Memory-First CRM"
docs.hebbs.ai/guide/legal     ← "Legal Research Agent"
docs.hebbs.ai/guide/finance   ← "Finance & Compliance Agent"
docs.hebbs.ai/guide/support   ← "Customer Support Agent"
docs.hebbs.ai/guide/coding    ← "Coding Agent Memory"
```

Navigation bar at top of all guide pages:

```
[Guide]  [CRM]  [Legal]  [Finance]  [Support]  [Coding]
```

---

## Default Guide (`/guide/`) — 8 Stages

Assumes server already installed. Teaches primitives with multi-vertical examples.

### Stage 1: Index Your Knowledge

**Concept:** Workspace structure — `entities/` folder for scoped content, everything else is shared knowledge.

```
workspace/
├── entities/              ← auto-tagged by subfolder name
│   ├── acme-corp/
│   └── project-alpha/
├── docs/                  ← shared knowledge, no entity_id
├── case-studies/
└── playbooks/
```

Show `hebbs index` and file watcher.

**Multi-vertical callouts:**
- CRM: product docs, case studies, blogs + deal folders per account
- Legal: statutes, precedents + case folders per client
- Coding: architecture docs, ADRs + project folders

### Stage 2: Remember Everything

**Concept:** Three paths to get data in. All produce the same memories. entity_id scoping, importance scoring.

**Path A — Files:** Drop into `entities/{name}/`, run `hebbs index` (or auto via watcher).

**Path B — CLI:**
```bash
hebbs remember "Sarah confirmed budget approved" \
  --entity-id acme-corp --importance 0.8
```

**Path C — SDK:**
```python
await engine.remember(
    content="Discovery call went well...",
    entity_id="initech",
    importance=0.8
)
```

Also mention frontmatter `entity_id:` override for files that need scoping outside `entities/`.

**Multi-vertical callouts:**
- CRM: call notes, emails, Slack messages
- Legal: deposition summaries, case memos
- Support: ticket threads, resolution notes
- Coding: "this approach failed because..."

### Stage 3: Prep with Prime

**Concept:** `prime` crosses BOTH entity-scoped memory AND shared knowledge in one call.

```bash
hebbs prime --entity-id acme-corp
```

Returns:
- Entity memory (temporal): recent interactions, decisions, timeline
- Shared knowledge (similarity): relevant docs, case studies, playbooks

**Multi-vertical callouts:**
- CRM: deal history + relevant case study + competitive intel
- Legal: client history + relevant precedents + statute references
- Support: ticket history + similar resolved tickets + KB articles

### Stage 4: Ask Strategic Questions

**Concept:** 4 recall strategies — same data, different questions.

| Strategy | Question | Example |
|----------|----------|---------|
| Similarity | "What looks like this?" | Find relevant docs for a topic |
| Temporal | "What happened, in order?" | Reconstruct history of an entity |
| Causal | "What led to this?" | Trace cause-and-effect chains |
| Analogical | "What's structurally similar?" | Apply patterns across entities |

Show one terminal demo per strategy with clickable cards (same UX as current guide).

### Stage 5: Catch Contradictions

**Concept:** Hebbs auto-detects contradictions when new memories conflict with existing ones.

Show:
- Two memories that contradict (e.g., "budget $100K" then "budget $50K")
- Hebbs flags it automatically — CONTRADICTS edge created
- Mention that contradictions surface in recall results and insights

Note: Keep it as auto-detection. Do NOT show prepare/commit flow (not in enterprise yet, CLI-only — see docs/parked/contradiction-prepare-commit-enterprise.md).

### Stage 6: Learn Over Time

**Concept:** `reflect` consolidates memories into insights. Insights compound across entities.

```bash
hebbs reflect
hebbs insights --min-confidence 0.7
```

Show example insights:
- "Deals that close fast always had a champion identified by week 2"
- "Security review delays correlate with missing SOC2 documentation upfront"

Show weekly digest idea (pipe to email, Slack, or skill auto-generates).

### Stage 7: Tune Retrieval

**Concept:** The agent learns how to retrieve better through a measured eval-tune loop. This is not manual configuration — it's a self-improving cycle that compounds over time.

Reference: `hebbs-skill/tune/SKILL.md` defines the full 8-phase tuning skill.

**The loop (show visually as a cycle):**

1. **Profile** — Classify the domain (Legal, Sales, Engineering, Research). Each profile has a different eval distribution (e.g., Legal is 40% factual lookup, 15% contradiction; Sales is 30% recency-weighted, 25% entity-scoped).

2. **Generate evals** — Domain-specific queries with expected keywords. Not synthetic — based on what the user actually searches for.

3. **Run baseline** — Default settings (similarity, k=5). Show the scorecard:
   ```
   Baseline: 54% keyword recall
   Perfect queries: 2/20
   Zero-hit queries: 3/20
   ```

4. **Diagnose failures** — Classify each miss:
   | Pattern | Fix |
   |---------|-----|
   | k too low | Increase to k=10 or k=15 |
   | Cue too generic | Expand with entity names and specifics |
   | Wrong strategy | Switch to temporal/analogical/causal |
   | Extraction ceiling | Better LLM or accept gap |

5. **Re-run tuned** — Apply fixes, measure again:
   ```
   Tuned: 84% keyword recall (+30pp)
   Perfect queries: 13/20
   Zero-hit queries: 0/20
   ```

6. **Store learnings** — Successful strategies stored as retrieval instructions in HEBBS itself:
   ```bash
   hebbs remember "RETRIEVAL-INSTRUCTION: For compliance queries, expand acronyms \
     and include vendor names. Use k=10 minimum." \
     --importance 0.9 --entity-id retrieval-instructions
   ```

7. **Compile rules file** — After 2-3 tune sessions, compress individual strategies into `.hebbs/retrieval-rules.md`:
   ```markdown
   # Retrieval Rules
   ## Cue Construction
   - Always expand acronyms: "SOC2" -> "SOC 2 Type II"
   - Always include entity names in cues
   ## k Sizing
   - Default: k=10
   - Simple factual: k=5
   - Broad sweep: k=15
   ## Strategy Selection
   - Factual lookup: similarity
   - Timeline: temporal + entity-id
   - Cross-entity: analogical, alpha=0.5
   ```
   This file loads into agent context before every conversation. The agent retrieves better from the first query.

8. **Re-tune when needed** — New content, reported misses, model changes, or 30+ days since last tune.

**Show the before/after scorecard** (same visual as current guide — the score rows with green/red, the result banner with 54% -> 84%).

**Multi-vertical callouts:**
- Each domain profile tunes differently: Legal prioritizes factual lookup + contradiction detection. Sales prioritizes recency + entity scoping. Engineering prioritizes causal chains.
- The compiled rules file is domain-specific — a legal vault's rules look completely different from a sales vault's rules.

### Stage 8: CTA — Pick Your Vertical

Cards linking to each vertical guide:

```
[CRM]  [Legal]  [Finance]  [Support]  [Coding]
```

Each card: icon, title, one-line hook, link.

---

## Vertical Guides — Shorter, Domain-Specific

Each vertical guide does NOT re-teach primitives. It shows:

1. **Their folder structure** — what goes in `entities/`, what's shared
2. **Their workflow** — day-in-the-life with domain-specific examples
3. **Their killer feature** — the one Hebbs capability that sells it

### CRM (`/guide/crm`)

**Folder structure:**
```
entities/{company-name}/     ← per account
shared: products/, case-studies/, blogs/, competitive-intel/, training/
```

**Workflow:** New deal → dump call notes → prep before calls → work the deal → spot risks → weekly team insights

**Killer feature:** Prime before a call crosses deal history + company knowledge (case studies, blogs, competitive intel). Rep walks in armed.

**Input paths:** Files for bulk notes, CLI for quick logging after calls, SDK for Gong/email/Slack webhooks.

**Tuning profile:** Sales/Revenue — 30% recency-weighted, 25% entity-scoped, 20% factual lookup, 15% cross-entity, 10% temporal. Rules optimize for "find the latest on this account" and "what worked with similar companies."

### Legal (`/guide/legal`)

**Folder structure:**
```
entities/{case-name}/        ← per case/client
shared: statutes/, precedents/, templates/, firm-policies/
```

**Workflow:** New case → index case files → research precedents → trace causal chains → detect conflicting clauses → consolidate case strategy

**Killer feature:** Causal recall through case law — "what precedents led to this ruling?"

**Tuning profile:** Legal/Compliance — 40% factual lookup, 20% entity-scoped, 15% temporal, 15% contradiction, 10% cross-entity. Rules optimize for precise citation retrieval and acronym expansion ("SOC2" -> "SOC 2 Type II").

### Finance (`/guide/finance`)

**Folder structure:**
```
entities/{audit-or-project}/  ← per audit, review, or project
shared: regulations/, policies/, standards/, past-audits/
```

**Workflow:** New audit → index financial docs → check compliance → detect policy contradictions → generate findings report

**Killer feature:** Contradiction detection — "policy says X but practice says Y"

**Tuning profile:** Legal/Compliance — same distribution as Legal, but rules optimize for regulatory cross-referencing and numeric disagreement detection (e.g., "$2M coverage" vs "$500K sublimit").

### Support (`/guide/support`)

**Folder structure:**
```
entities/{ticket-id}/        ← per ticket or customer
shared: kb-articles/, runbooks/, product-docs/, past-resolutions/
```

**Workflow:** New ticket → prime with customer history + similar tickets → resolve → reflect generates runbooks from patterns

**Killer feature:** Reflect auto-generates runbooks from repeated resolution patterns. Org gets smarter with every ticket.

**Tuning profile:** Sales/Revenue (adapted) — 30% recency-weighted (latest ticket state), 25% entity-scoped (this customer's history), 25% factual lookup (KB articles), 20% cross-entity (similar tickets). Rules optimize for "find how this was resolved before."

### Coding (`/guide/coding`)

**Folder structure:**
```
entities/{project-or-feature}/ ← per project, feature branch, or refactor
shared: architecture/, adrs/, conventions/, debugging-logs/
```

**Workflow:** Start feature → remember decisions and approaches → recall what was tried before → avoid repeating failures → reflect generates "lessons learned"

**Killer feature:** Temporal recall — "what approaches were tried on this problem and why they failed?"

**Tuning profile:** Engineering — 25% causal, 25% entity-scoped, 20% factual lookup, 20% temporal, 10% broad sweep. Rules optimize for "what caused this bug" and "what was tried on this problem before."

---

## Implementation

### Files to create/modify

| File | Action |
|------|--------|
| `hebbs-docs/src/pages/guide.astro` | Rewrite — new 7-stage generic guide |
| `hebbs-docs/src/pages/guide/crm.astro` | New — CRM vertical guide |
| `hebbs-docs/src/pages/guide/legal.astro` | New — Legal vertical guide |
| `hebbs-docs/src/pages/guide/finance.astro` | New — Finance vertical guide |
| `hebbs-docs/src/pages/guide/support.astro` | New — Support vertical guide |
| `hebbs-docs/src/pages/guide/coding.astro` | New — Coding vertical guide |

### Design

- Same visual style as current guide (dark theme, terminal demos, scroll-snap stages, Space Grotesk + JetBrains Mono)
- Navigation bar shared across all guide pages
- Vertical guides are shorter (3-4 stages, not 7)
- Each vertical guide links back to the default guide for full primitives explanation

### Notes

- Contradiction: show as auto-detection only, not prepare/commit flow (see docs/parked/contradiction-prepare-commit-enterprise.md)
- TASK-37 must land first — the `entities/` folder convention is central to Stage 1
- Keep terminal demos realistic but concise
- No pricing or sales language in guides — these are technical docs
