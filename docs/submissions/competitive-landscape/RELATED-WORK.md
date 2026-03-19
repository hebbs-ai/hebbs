# Competitive Landscape: Agent Memory Systems (2025-2026)

## Direct Competitors

### AgeMem (Jan 2026)
- **Paper**: arXiv 2601.01885
- **Approach**: Unified long/short-term memory management exposed as tool-based actions. Agent decides when to store, retrieve, update, summarize, or discard.
- **Strength**: Strong benchmark results on 5 long-horizon benchmarks.
- **Gap**: No contradiction detection, no decay model, no neuroscience grounding, no associative graph.

### A-MEM (Feb 2025, updated Oct 2025)
- **Paper**: arXiv 2502.12110
- **Approach**: Zettelkasten-inspired memory. Dynamic indexing and linking of memory notes.
- **Strength**: Good knowledge graph structure. Open source (GitHub).
- **Gap**: No consolidation pipeline, no contradiction detection, no temporal decay, Python-only.

### Memoria (Dec 2025)
- **Paper**: arXiv 2512.12686
- **Approach**: Modular memory framework with session summarization + weighted knowledge graph for user modeling.
- **Strength**: Scalable, interpretable, context-rich.
- **Gap**: Focused on conversational AI (not general agents), no conflict detection, no decay.

### MemGPT / Letta
- **Approach**: Virtual context management. Pages memory in/out of LLM context window.
- **Strength**: Practical, widely adopted.
- **Gap**: No semantic consolidation, no contradiction detection, no decay. Treats memory as a paging problem, not a learning problem.

### Zep / Mem0
- **Approach**: Commercial memory layers for LLM apps.
- **Strength**: Production-ready, easy integration.
- **Gap**: Black-box, no graph structure, no consolidation, no neuroscience basis.

---

## Survey Papers (Cite for Landscape)

### Memory in the Age of AI Agents (Dec 2025)
- **Paper**: arXiv 2512.13564
- **Key insight**: Traditional long/short-term taxonomy is insufficient. Need richer frameworks.
- **Use**: Cite to establish the problem space and HEBBS' position.

### AI Meets Brain (Dec 2025)
- **Paper**: arXiv 2512.23343
- **Key insight**: Unified survey bridging cognitive neuroscience and autonomous agent memory.
- **Use**: Cite as the closest conceptual framing. HEBBS is the implementation of this vision.

### Memory for Autonomous LLM Agents (Mar 2026)
- **Paper**: arXiv 2603.07670
- **Key insight**: Mechanisms, evaluation, and emerging frontiers for agent memory.
- **Use**: Cite for evaluation methodology and benchmark standards.

---

## Neuroscience Foundations (Cite for Grounding)

| Concept | Key Paper | Year | Relevance to HEBBS |
|---|---|---|---|
| Complementary Learning Systems | McClelland, McNaughton, O'Reilly | 1995 | Fast (hippocampus) vs slow (neocortex) learning = vault ingestion vs consolidation |
| Hebbian Learning | Hebb, "The Organization of Behavior" | 1949 | Namesake. Associative memory graph edges |
| Forgetting Curve | Ebbinghaus | 1885 | Half-life decay model |
| ACC Conflict Monitoring | Botvinick, Cohen, Carter | 2001/2004 | Contradiction detection pipeline |
| Sleep-Dependent Consolidation | Diekelmann & Born | 2010 | Periodic reflection / episode-to-insight |
| Memory Transformation | Winocur & Moscovitch | 2011 | Episodic to semantic conversion |
| Spike-Timing Dependent Plasticity | Bi & Poo | 1998 | Temporal edge weighting |

---

## HEBBS Unique Contributions (vs All Above)

1. **Full consolidation pipeline**: Only system implementing encode -> associate -> detect conflicts -> resolve -> consolidate -> decay as a unified pipeline.
2. **Neuroscience-grounded**: Explicit, testable mapping to brain regions. Not metaphorical.
3. **Contradiction detection**: No other agent memory system detects and resolves conflicting memories.
4. **Adaptive decay**: Half-life model with reinforcement. Others either keep everything or use fixed eviction.
5. **Zero-copy Rust engine**: Performance class apart from Python-based systems.
6. **Bidirectional markdown sync**: Memories are human-readable and editable files, not opaque embeddings.
