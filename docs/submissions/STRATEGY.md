# Submission Strategy

## Paper Title (Working)

**"HEBBS: A Neuroscience-Grounded Memory Engine for Autonomous AI Agents"**

Alternative titles:
- "From Hippocampus to Vault: Biologically-Inspired Persistent Memory for LLM Agents"
- "HEBBS: Hebbian Memory Consolidation, Contradiction Detection, and Adaptive Decay for AI Agents"

---

## Core Thesis

AI agents using flat markdown or vector-only memory degrade after 1-2 weeks of operation as conflicts, redundancy, and noise compound. HEBBS solves this by implementing the same memory architecture the human brain evolved: fast hippocampal encoding, Hebbian associative graphs, anterior cingulate conflict detection, sleep-like consolidation, and Ebbinghaus-curve adaptive decay -- all compiled to a zero-copy Rust engine.

---

## Venue Strategy

### Phase 1: Establish Priority (Now - April 2026)
- **arXiv preprint** on cs.AI + cs.CL
- Establishes timestamp before NeurIPS deadline
- Gets cited in the fast-moving agent memory space

### Phase 2: Top Conference (May 2026)
- **NeurIPS 2026** main track
  - Abstract deadline: May 4, 2026 (AOE)
  - Full paper deadline: May 6, 2026 (AOE)
  - Conference: Dec 6-12, 2026
  - Format: 8 pages + unlimited appendices
  - Submission via OpenReview

### Phase 3: Neuroscience Angle (Summer 2026)
- **CCN 2026** (Cognitive Computational Neuroscience)
  - Aug 3-6, 2026 at NYU
  - Shorter paper focused on the brain-architecture mapping
  - Different framing: "computational validation of memory consolidation theory"

### Phase 4: Journal (Late 2026)
- **Nature Machine Intelligence** or **TMLR**
  - Extended version with deeper evaluation
  - More ablations, longer-horizon benchmarks
  - Rolling submission

### Backup / Parallel Targets
- **AAAI 2027** (deadline ~Aug 2026)
- **AAMAS 2027** (autonomous agents focus)
- **AAAI-MAKE 2026** (Knowledge-Grounded Semantic Agents, April 7-9)
- **COLM 2026** (if late submissions open)

---

## Paper Structure (NeurIPS Format, 8 pages)

### 1. Introduction (1 page)
- Problem: LLM agents with flat memory degrade over time
- Observation: human brains solved this problem via specific architecture
- Contribution: HEBBS implements this architecture computationally
- Results preview: X% improvement on LongMemEval, contradiction detection accuracy, decay curve matching

### 2. Related Work (1 page)
- Agent memory systems: MemGPT, A-MEM, AgeMem, Memoria, Zep, Mem0
- Neuroscience foundations: Complementary Learning Systems (McClelland 1995), Hebbian theory, memory consolidation
- Gap: no existing system implements the full consolidation pipeline (encode -> associate -> detect conflicts -> consolidate -> decay)

### 3. The HEBBS Architecture (2 pages)
- 3.1 Vault Ingestion (Hippocampal Fast Encoding)
  - Markdown chunking, ONNX embeddings, RocksDB storage
  - Real-time daemon (novelty detection analogue)
- 3.2 Memory Palace (Hebbian Associative Graph)
  - HNSW similarity index
  - Temporal, causal, similarity edge types
  - Graph-based retrieval vs flat vector search
- 3.3 Contradiction Detection (ACC Conflict Monitoring)
  - Heuristic + embedding-based conflict detection
  - CONTRADICTS and REVISED_FROM edge semantics
  - Agent-mediated resolution (PFC analogue)
- 3.4 Reflection & Consolidation (Sleep Replay)
  - Episode clustering into insights
  - Relevance, recency, importance, reinforcement scoring
- 3.5 Adaptive Decay (Ebbinghaus Forgetting Curve)
  - Half-life decay model
  - Reinforcement-based retention
  - Token budget management

### 4. Neuroscience Grounding (1 page)
- Formal mapping between HEBBS stages and brain regions
- Table: Brain Region | Function | HEBBS Component | Mechanism
- Why this mapping matters: convergent design under shared constraints
- Predictions from the analogy (testable hypotheses)

### 5. Experiments (2 pages)
- 5.1 LongMemEval benchmark (long-horizon agent memory)
- 5.2 Contradiction detection precision/recall
- 5.3 Memory quality over time (30-day degradation study)
- 5.4 Ablation: removing consolidation, decay, contradiction detection
- 5.5 Token efficiency: retrieval budget vs accuracy tradeoff

### 6. Discussion & Conclusion (1 page)
- Limitations
- Future work: multi-agent shared memory, cross-vault consolidation
- Broader impact

### Appendix (unlimited)
- Full architecture diagrams
- Implementation details (Rust, RocksDB, gRPC)
- Extended benchmark results
- Neuroscience reference table with citations

---

## Key Differentiators vs Competition

| Feature | HEBBS | AgeMem | A-MEM | Memoria | MemGPT |
|---|---|---|---|---|---|
| Contradiction detection | Yes (pipeline) | No | No | No | No |
| Adaptive decay | Yes (half-life) | No | No | No | No |
| Consolidation (episodes->insights) | Yes | Partial | No | Partial | No |
| Associative graph (not just vectors) | Yes | No | Yes (Zettelkasten) | Yes (KG) | No |
| Neuroscience-grounded design | Yes (explicit) | No | No | No | No |
| Rust/zero-copy engine | Yes | No | No | No | No |
| Real-time file watching | Yes | No | No | No | No |

---

## Competitive Papers to Cite

1. AgeMem (arXiv 2601.01885, Jan 2026)
2. Memory in the Age of AI Agents (arXiv 2512.13564, Dec 2025) -- survey
3. A-MEM (arXiv 2502.12110, Feb 2025)
4. Memoria (arXiv 2512.12686, Dec 2025)
5. AI Meets Brain survey (arXiv 2512.23343)
6. Memory for Autonomous LLM Agents (arXiv 2603.07670)
7. McClelland et al., Complementary Learning Systems (1995)
8. Ebbinghaus, Memory: A Contribution to Experimental Psychology (1885)
9. Hebb, The Organization of Behavior (1949)
10. Botvinick et al., ACC and conflict monitoring (2004)

---

## Timeline

| Date | Milestone |
|---|---|
| Mar 18 - Apr 7 | Draft paper sections 1-4 |
| Apr 7 - Apr 21 | Run experiments, collect benchmark results |
| Apr 21 - Apr 28 | Write sections 5-6, compile figures |
| Apr 28 - May 3 | Internal review, polish |
| May 4 | Submit NeurIPS abstract |
| May 6 | Submit NeurIPS full paper |
| May 7 | Post to arXiv |
| Jun-Jul | Prepare CCN 2026 submission |
| Aug | Prepare AAAI 2027 submission |
