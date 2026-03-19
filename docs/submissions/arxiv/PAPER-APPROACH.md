# HEBBS Paper Approach

## The Thesis

Every competitor -- MAGMA, Zep, Mem0, AgeMem -- treats memory as a **retrieval problem**. "Given a query, find the right stuff." They're building better search engines for memories.

HEBBS treats memory as a **learning problem**. Memories aren't static objects you store and fetch. They evolve, conflict, strengthen, decay, and consolidate into knowledge. That's what brains do. That's what nobody else does.

**One-sentence soul: "Agent memory isn't retrieval. It's learning."**

---

## Paper Structure

Not the standard "we built a system" paper. This should read as a **paradigm argument with proof**.

### Section 1: The Degradation Problem (1 page)
Don't open with "LLMs have limited context." Everyone does that. Open with a concrete failure: an agent running for 30 days, its memory filling with contradictions, stale facts, redundant entries. Show the decay curve. Then ask: why don't brains have this problem?

### Section 2: What Brains Actually Do (1 page)
Not a literature review. A design specification extracted from neuroscience. Six mechanisms, six citations, one table. The reader should finish this section thinking "obviously an AI memory system should work this way" before you've even introduced HEBBS.

### Section 3: HEBBS Architecture (2 pages)
Map each brain mechanism to its computational implementation. The structure mirrors Section 2 exactly -- the reader sees the 1:1 correspondence. Lead with contradiction detection (the unique hook) and Hebbian associative embeddings (the novel mechanism). Don't bury them in subsection 3.4.

### Section 4: What Others Miss (0.5 pages)
The comparison table. Not a related work dump -- a pointed gap analysis. One table showing the full pipeline: encode, associate, detect, resolve, consolidate, decay. Checkmarks for HEBBS. Partial marks for everyone else. Nobody fills the row.

### Section 5: Experiments (3 pages)
Five experiments, each proving a different claim:
- **Experiment 1a**: LongMemEval (ICLR 2025) -- tests memory at scale (115K-1.5M tokens). Focus on knowledge updates, temporal reasoning, and abstention categories where flat memory systems drop ~30%. Shows Hebbs handles the hard cases where retrieval-only approaches fail.
- **Experiment 1b**: LoCoMo (ACL 2024) -- tests multi-hop reasoning and adversarial questions across long multi-session conversations (~600 turns, 32 sessions). LoCoMo's adversarial questions (correct answer is "I don't know") map directly to contradiction detection. Its temporal reasoning category maps to Hebbs' decay/consolidation pipeline. Two top-venue benchmarks make the eval section reviewer-proof.
- **Experiment 2**: Contradiction detection precision/recall -- a metric nobody else can even report
- **Experiment 3**: 30-day degradation study -- HEBBS vs flat memory vs vector-only. The money shot: other systems degrade, HEBBS doesn't
- **Experiment 4**: Ablation -- remove each pipeline stage, show how much each one matters

### Section 6: Discussion (1 page)
Limitations honestly. Future work: multi-agent shared memory, cross-vault consolidation. The "testable predictions" from the neuroscience mapping -- what the analogy predicts that we haven't built yet.

---

## Steps to Submission

### Step 1: Build the benchmark harness
Before writing a word of the paper, build the experiment infrastructure. Set up LongMemEval, design the contradiction detection eval dataset, design the 30-day degradation study protocol. The paper is only as good as its numbers.

### Step 2: Run experiments, collect results
Run all four experiments. Let the numbers shape the narrative. If contradiction detection gets 95% precision, that becomes the headline. If ablation shows decay is the most important stage, restructure the architecture section to emphasize it.

### Step 3: Write the figures first
Before prose, design 5-6 figures:
- The architecture diagram (brain regions mapped to components)
- The pipeline comparison table (the "nobody fills the row" table)
- The degradation curve (30 days, HEBBS vs others)
- Contradiction detection examples (before/after resolution)
- Ablation bar chart
- The Hebbian embedding evolution visualization

These figures ARE the paper. The prose connects them.

### Step 4: Write the narrative
Sections 1 and 2 first (the problem and the neuroscience). Then Section 5 (results). Then Section 3 (architecture). Section 4 (comparison) last. Write from the outside in -- motivation and results frame everything.

### Step 5: LaTeX it
NeurIPS template. Get the formatting right. References in BibTeX. Every figure placed for maximum impact.

### Step 6: Internal red team
Read it as a hostile reviewer:
- "Is this just engineering?" (No -- the Hebbian offset vector mechanism and contradiction detection are algorithmic contributions.)
- "Where are the baselines?" (LongMemEval, degradation study.)
- "Is the neuroscience real or decoration?" (Explicit, testable, 1:1 mapping with citations.)

### Step 7: arXiv first, then NeurIPS
Post to arXiv to establish priority. Then submit to NeurIPS with the same paper, polished by a round of feedback.
