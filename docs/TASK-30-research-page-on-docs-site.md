# TASK-30: Research Page on Docs Site

**Status:** Not started
**Priority:** High
**Target:** docs.hebbs.ai/research (Starlight/Astro .mdx page)

---

## Goal

Build a dedicated `/research` page on the docs site (hebbs-docs) that presents HEBBS research with the polish and clarity of supermemory.ai/research, but with stronger substance: real eval data, nine-system comparison, self-tuning as the lead innovation.

This page serves dual purpose: marketing (credibility for developers evaluating HEBBS) and academic (supplements the arXiv paper with interactive visuals).

---

## Page Structure

### 1. Hero
- Title: "HEBBS: A Self-Tuning Memory Engine for AI Agents"
- One-line claim: the agent optimizes its own memory retrieval
- Lead number: 59% to 88% keyword recall via agent-driven tuning
- Link to arXiv paper

### 2. The Problem (Two Problems)
- Problem 1: Memory degradation (contradictions, stale facts, noise)
- Problem 2: Retrieval rigidity (one config for all query types)
- Concrete example: 30-day coding assistant with contradictory database entries
- Visual: before/after showing degradation over time

### 3. Architecture Pipeline
- Diagram: full pipeline flow (ingest, self-tune, detect contradictions, consolidate, decay)
- Brief description of each stage
- Visual: pipeline diagram (SVG or embedded component)

### 4. Self-Tuning Retrieval (The Lead Innovation)
- The eval-tune-store loop explained visually
- Diagram: cycle of eval, diagnose, tune, store, recall
- Why it matters: domain-specific, agent-driven, self-reinforcing
- Key insight: strategies stored as memories that strengthen through use

### 5. Brain Mapping
- Visual figure: 6 brain regions mapped to 6 HEBBS components
- Table with brain region, biological function, HEBBS component, mechanism
- Tagline connecting neuroscience to engineering

### 6. Results
- Tuning results table (baseline 59% vs tuned 88%)
- Per-query-type breakdown (similarity, temporal, causal, analogical, contradiction)
- Strategy differentiation: same query, 3 strategies, 3 different results
- Decay validation: access count vs decay score

### 7. Comparison Table
- HEBBS vs MAGMA vs Kairos vs Zep vs Mem0 vs AgeMem vs A-MEM vs MemGPT vs Memoria
- Columns: Fast Encode, Hebbian Assoc., Conflict Detect, Conflict Resolve, Consolidation, Adaptive Decay, Self-Tune
- Checkmarks, partial marks, X marks

### 8. Portable Cognition
- How .hebbs/ works (two-plane architecture)
- .hebbsignore (privacy by design)
- Rebuild guarantee
- Visual: content plane vs cognition plane diagram

### 9. Citation
- BibTeX block for the arXiv paper
- Link to GitHub
- Link to full paper PDF

---

## Graphics Needed

All diagrams should be SVG (inline or in public/), black/dark theme matching the docs site.

1. **Architecture pipeline diagram**: ingest -> self-tune -> detect -> consolidate -> decay
2. **Self-tuning eval loop**: circular diagram showing eval -> diagnose -> tune -> store -> recall
3. **Brain mapping figure**: 6 brain regions to 6 HEBBS components (color-coded)
4. **Tuning results bar chart**: baseline vs tuned, per-query breakdown
5. **Strategy differentiation visual**: same query, different strategies, different results
6. **Comparison checkmark table**: 9 systems x 7 mechanisms
7. **Two-plane architecture diagram**: content plane (files) vs cognition plane (.hebbs/)

---

## Reference

- Supermemory research page: https://supermemory.ai/research/
- ArXiv paper source: docs/submissions/arxiv/paper/main.tex
- Run-tune1 eval data: docs/marketing/run-tune1/RUN-LOG.md
- Competitive analysis: docs/submissions/competitive-landscape/COMPETITIVE-ANALYSIS.md
- Brain mapping data: Appendix C of main.tex

---

## Implementation Notes

- File location: hebbs-docs/src/content/docs/research.mdx (or research/index.mdx)
- Use Starlight components for layout, custom Astro components for diagrams
- All SVGs should be self-contained (no external dependencies)
- Page should work without JavaScript (static SVGs, not canvas)
- Mobile responsive (diagrams should stack vertically on small screens)
- No em-dashes or double dashes in copy
