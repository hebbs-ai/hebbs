# Paper Outline

**Title**: HEBBS: A Neuroscience-Grounded Memory Engine for Autonomous AI Agents

**Format**: NeurIPS 2026 (8 pages main + unlimited appendix)

**Authors**: TBD

---

## Abstract (~250 words)

- Problem: AI agents with flat or vector-only memory degrade over weeks as conflicts, redundancy, and noise compound.
- Insight: The human brain solved this exact problem through a specific architecture: fast hippocampal encoding, Hebbian associative binding, anterior cingulate conflict monitoring, sleep-driven consolidation, and adaptive forgetting.
- System: HEBBS implements this pipeline as a zero-copy Rust engine with RocksDB storage, HNSW vector search, ONNX embeddings, and a real-time file-watching daemon.
- Results: [TBD -- benchmarks needed]

---

## 1. Introduction

- Open with the problem: LLM agents work well for days, degrade over weeks
- Why: memory systems lack consolidation, conflict resolution, and principled forgetting
- The brain analogy: same engineering constraints, same solution
- Contributions:
  1. A neuroscience-grounded memory architecture mapping brain regions to computational components
  2. First agent memory system with integrated contradiction detection and resolution
  3. Adaptive decay model based on Ebbinghaus forgetting curves
  4. Empirical evaluation on long-horizon benchmarks

---

## 2. Related Work

- 2.1 Agent memory systems (MemGPT, A-MEM, AgeMem, Memoria, Zep, Mem0)
- 2.2 Neuroscience of memory (CLS theory, Hebbian learning, memory consolidation, ACC)
- 2.3 Knowledge graphs for AI (temporal KGs, contradiction detection in KBs)
- Gap statement: no system unifies all stages of the biological memory pipeline

---

## 3. HEBBS Architecture

- 3.1 System Overview (architecture diagram, data flow)
- 3.2 Vault Ingestion = Hippocampal Fast Encoding
  - Markdown chunking by heading hierarchy
  - ONNX embedding generation (local, no API dependency)
  - RocksDB column-family storage
  - Daemon file watcher (novelty detection)
- 3.3 Memory Palace = Hebbian Associative Graph
  - HNSW approximate nearest neighbor index
  - Edge types: SIMILAR, TEMPORAL, CAUSAL, CONTRADICTS, REVISED_FROM
  - Graph-based retrieval with edge traversal
- 3.4 Contradiction Detection = ACC Conflict Monitoring
  - Heuristic flags: embedding similarity + semantic opposition
  - LLM-agent resolution (PFC analogue)
  - Edge creation: CONTRADICTS or REVISED_FROM
- 3.5 Reflection = Sleep Consolidation
  - Periodic batch processing
  - Episode clustering into generalized insights
  - Insight validation via LLM
- 3.6 Scoring & Decay = Memory Strength + Forgetting Curve
  - Four-weight scoring: relevance, recency, importance, reinforcement
  - Half-life exponential decay
  - Token-budgeted retrieval

---

## 4. Neuroscience Grounding

- Formal mapping table: Brain Region | Biological Function | HEBBS Component | Implementation
- Why convergent design: shared constraints (finite capacity, noisy input, need for consistency)
- Testable predictions derived from the analogy
- Difference from metaphor: HEBBS components are functionally equivalent, not just named similarly

---

## 5. Experiments

- 5.1 Setup
  - Benchmarks: LongMemEval, [other TBD]
  - Baselines: A-MEM, AgeMem, MemGPT, raw vector store
  - LLM backends: Claude, GPT-4, open-source
- 5.2 Long-Horizon Memory Accuracy
  - Task: agent operates over 30+ days of simulated data
  - Metric: retrieval accuracy, answer correctness over time
- 5.3 Contradiction Detection
  - Dataset: synthetic + real-world contradictory memory pairs
  - Metrics: precision, recall, F1
- 5.4 Consolidation Quality
  - Are generated insights factually correct?
  - Do they improve downstream retrieval?
- 5.5 Decay Curve Validation
  - Does HEBBS' decay match Ebbinghaus empirical data?
  - Ablation: with vs without decay
- 5.6 Ablation Study
  - Remove each pipeline stage independently
  - Measure degradation

---

## 6. Discussion

- Limitations: LLM dependency for resolution/reflection, computational cost of full pipeline
- When HEBBS is overkill: short-lived agents, single-session use
- Future: multi-agent shared memory palaces, cross-vault consolidation, collective intelligence

---

## 7. Conclusion

- First system to implement the full biological memory consolidation pipeline for AI agents
- Neuroscience grounding provides both design rationale and testable predictions
- Open source, local-first, zero-dependency inference

---

## Appendix

- A. Full architecture diagrams
- B. Implementation details (Rust crate structure, gRPC API, RocksDB schema)
- C. Extended benchmark tables
- D. Neuroscience reference table with full citations
- E. Example memory traces through the pipeline

---

## Figures Needed

1. Architecture diagram (full pipeline: ingestion -> palace -> contradiction -> consolidation -> decay)
2. Brain region mapping (the split-view from the website, adapted for print)
3. Memory palace graph visualization (nodes + typed edges)
4. Decay curve: HEBBS vs Ebbinghaus empirical data
5. Benchmark comparison charts (bar charts vs baselines)
6. Ablation results (line chart showing degradation per removed component)
7. Contradiction detection examples (before/after)

---

## TODOs

- [ ] Finalize author list
- [ ] Run LongMemEval benchmark
- [ ] Build contradiction detection test dataset
- [ ] Run 30-day degradation study
- [ ] Generate all figures
- [ ] Write each section
- [ ] Internal review
- [ ] Submit abstract by May 4
- [ ] Submit paper by May 6
