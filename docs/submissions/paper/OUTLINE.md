# Paper Outline

**Title**: HEBBS: A Self-Tuning Memory Engine for AI Agents

**Format**: NeurIPS 2026 (8 pages main + unlimited appendix)

**Authors**: TBD

---

## Abstract (~250 words)

- Two problems: memory degrades as contradictions compound, and retrieval degrades because no single configuration is optimal across query types.
- No existing system addresses the second problem: agents retrieve the same way regardless of query intent.
- System: HEBBS exposes four recall strategies with four tunable scoring dimensions. Agents generate evals, diagnose failures, tune parameters, and store strategies as memories. Six neuroscience-grounded mechanisms maintain the store.
- Results: 59% to 88% keyword recall via agent-driven tuning on a 52-document legal vault.

---

## 1. Two Problems with Agent Memory

- Open with the degradation problem: contradictions, stale facts, noise
- Introduce the second problem: retrieval rigidity (one config for all queries)
- The brain analogy: solved both via maintenance mechanisms + adaptive retrieval
- Contributions:
  1. Self-tuning retrieval: agents evaluate, tune, and store retrieval strategies as memories
  2. Contradiction detection: first agent memory system with explicit conflict detection pipeline
  3. Hebbian associative embeddings: dual embeddings with per-edge-type offset vectors
  4. File-first portable cognition: `.hebbs/` + `.hebbsignore`
  5. Complete engineered system: six neuroscience mechanisms in one Rust binary

---

## 2. Related Work

- 2.1 Agent memory systems (MemGPT, A-MEM, AgeMem, Memoria, Zep, Mem0)
- 2.2 Neuroscience of memory (CLS theory, Hebbian learning, memory consolidation, ACC)
- 2.3 Knowledge graphs for AI (temporal KGs, contradiction detection in KBs)
- Gap statement: no system unifies all stages of the biological memory pipeline

---

## 3. HEBBS Architecture

- 3.1 Self-Tuning Retrieval (THE LEAD)
  - Retrieval as a parameter space: 4 strategies x 4 scoring dimensions x per-strategy params
  - The eval-tune-store loop: generate evals, diagnose, tune, store strategies as memories
  - Domain-specific, agent-driven, self-reinforcing
- 3.2 Contradiction Detection = ACC Conflict Monitoring
  - LLM structured classifier (contradiction/revision/dismiss)
  - Two-phase commit (detect, then commit with agent review)
- 3.3 Hebbian Associative Embeddings
  - Dual embeddings (content + associative)
  - Per-edge-type offset vectors
  - Analogical reasoning via vector arithmetic
- 3.4 Vault Ingestion = Hippocampal Fast Encoding
  - Markdown chunking + proposition extraction
  - ONNX embedding generation (local, no API dependency)
  - RocksDB column-family storage
  - Daemon file watcher (novelty detection)
- 3.5 File-First Architecture = Portable Cognition
  - Two-plane separation: content plane (source files) vs cognition plane (`.hebbs/`)
  - Portability: self-contained cognition artifact, copy/share across machines and agents
  - Rebuild guarantee: delete `.hebbs/`, reconstruct from source files
  - `.hebbsignore`: gitignore-style selective indexing, privacy at the boundary
- 3.6 Four Recall Strategies
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

- 5.1 Agent-Driven Retrieval Tuning (REAL DATA: 59% to 88%)
  - 52-doc legal vault, 949 memories, 20 eval queries
  - Baseline: similarity k=5, 59% keyword recall
  - Tuned: per-query strategy/weights/k, 88% keyword recall
  - 4 failure patterns identified, 5 strategies stored
- 5.2 Strategy Differentiation
  - Same query across strategies produces qualitatively different results
  - Similarity vs analogical vs temporal on cross-vendor query
- 5.3 Adaptive Decay Validation (REAL DATA)
  - Access count vs decay score correlation
  - 0 accesses: 0.500, 5 accesses: 0.694
- 5.4 Ablation Study
  - Self-tuning removal: 88% to 59% (29pp drop, already measured)
  - Proposition extraction, decay, multi-strategy: TBD

---

## 6. Discussion

- Limitations: LLM dependency for resolution/reflection, computational cost of full pipeline
- When HEBBS is overkill: short-lived agents, single-session use
- Future: multi-agent shared memory via `.hebbs/` sharing, cross-vault consolidation, collective intelligence

---

## 7. Conclusion

- Agent memory has two problems: degradation and retrieval rigidity
- HEBBS addresses both: six neuroscience mechanisms + self-tuning retrieval
- 59% to 88% keyword recall via agent-driven tuning
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

1. Architecture diagram (full pipeline: ingestion -> self-tune -> contradiction -> consolidation -> decay)
2. Self-tuning eval loop diagram (eval -> diagnose -> tune -> store -> recall cycle)
3. Brain region mapping (the split-view from the website, adapted for print)
4. Tuning results bar chart (59% baseline vs 88% tuned, per-query breakdown)
5. Strategy differentiation (same query, different strategies, different results)
6. Decay score vs access count (table already in paper, could be a plot)
7. Ablation results (bar chart showing contribution of each component)

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
