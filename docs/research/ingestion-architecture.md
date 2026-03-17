# HEBBS Ingestion Architecture: Triple-Layer Memory

**Status:** Research complete, ready for implementation
**Date:** 2026-03-17

## Problem

HEBBS currently segments markdown files by headings (configurable `split_on` level, default `##`). Each heading-delimited section becomes one memory with one embedding.

This fails for:

- **Data cards / structured notes** -- a file with 4 headings each containing 1 line produces 4 useless fragments. The whole file is one logical unit.
- **Dense reference material** -- a single section may contain 20 distinct facts that should be individually retrievable.
- **Large documents** -- a 200-page design doc needs multi-granularity representation, not a flat list of heading-chunks.

Every production memory engine (Mem0, Zep/Graphiti, Cognee, Microsoft GraphRAG) has converged on the same insight: **chunks are intermediate processing artifacts, not the final memory units.**

---

## Architecture: Three Layers of Memory

### Layer 1: Document Memory (whole-file context)

Store the **entire file** as a single retrievable unit.

- One memory per file in the engine
- Embedding captures the document's overall semantic identity
- For large files (>4K tokens): store an LLM-generated summary as the memory content instead of raw text, with the full file path as provenance
- This is what Zep calls "episodes" -- the raw ground truth, never lossy

**Purpose:** When someone asks "what do we know about Perplexity AI?", the whole research card surfaces as one hit. Provides context for Layer 2 propositions.

### Layer 2: Proposition Memories (atomic facts)

Run an LLM over each file to extract **atomic, self-contained factual statements**. Each proposition becomes one memory with its own embedding.

Example input (research card):
```
# Source 040
URL: https://...
Perplexity AI funding and growth timeline.
## Data Points
Founded: August 2022
Founders: Aravind Srinivas (CEO, ex-OpenAI), Denis Yarats (CTO, ex-Meta)
Total raised: $1.5B+
ARR: ~$150M
Monthly queries: 780M+
```

Extracted propositions:
1. "Perplexity AI was founded in August 2022"
2. "Perplexity AI's CEO is Aravind Srinivas, formerly of OpenAI"
3. "Perplexity AI's CTO is Denis Yarats, formerly of Meta"
4. "Perplexity AI has raised over $1.5 billion in total funding"
5. "Perplexity AI's annual recurring revenue is approximately $150 million"
6. "Perplexity AI processes over 780 million queries per month"

Each proposition:
- Is self-contained (includes the subject, not just "Founded: August 2022")
- Gets its own embedding vector
- Links back to the source document (Layer 1) and source file
- Has a `kind` of `proposition` (new MemoryKind variant)

**Purpose:** Precision retrieval. "When was Perplexity founded?" hits proposition #1 directly instead of returning the entire file and hoping the LLM finds the answer.

### Layer 3: Knowledge Graph (entities + relationships)

From the same LLM extraction pass, extract **entities and relationships** into the graph.

- "Perplexity AI" -> entity node
- "Aravind Srinivas" -> entity node
- "CEO of" -> directed edge (Srinivas -> Perplexity AI)
- "funded by" -> edge (Perplexity AI -> Nvidia)
- "competes with" -> edge (Perplexity AI -> Google Search)

HEBBS already has the graph layer (edges, wiki-links, similarity edges, contradiction edges). This extends it with LLM-extracted entity-relationship edges.

**Purpose:** Multi-hop reasoning. "Which Y Combinator W23 companies have raised over $1B?" requires traversing entity relationships, not just vector search.

---

## Pipeline: From File to Three Layers

### Small Files (< ~4K tokens)

```
File arrives (create/modify event)
  |
  v
Phase 1 (existing): Parse markdown, extract structure
  |
  v
Phase 2a: Store whole file as Layer 1 document memory
  |
  v
Phase 2b: Send whole file to LLM
  |         Prompt: "Extract all atomic factual propositions
  |                  and all entities with relationships"
  |
  v
Phase 2c: Each proposition -> Layer 2 memory (embed + store)
           Each entity/relationship -> Layer 3 graph edge
           All linked to source file via provenance
```

### Large Files (> ~4K tokens)

```
File arrives (create/modify event)
  |
  v
Phase 1 (existing): Parse markdown, split by headings
  |                  These are PROCESSING UNITS, not memories
  |
  v
Phase 2a: Generate document summary via LLM
  |        Store summary as Layer 1 document memory
  |
  v
Phase 2b: For each structural chunk:
  |          Prepend document context (Anthropic technique):
  |          "This chunk is from [filename], a [type] about [topic].
  |           Previous section covered [X]. This section covers [Y]."
  |
  |          Send contextualized chunk to LLM:
  |          "Extract all atomic factual propositions
  |           and all entities with relationships"
  |
  v
Phase 2c: Each proposition -> Layer 2 memory (embed + store)
           Each entity/relationship -> Layer 3 graph edge
           Entity resolution across chunks (deduplicate entities)
           All linked to source file via provenance
```

### Key Difference from Current Pipeline

| Aspect | Current | New |
|--------|---------|-----|
| What becomes a memory | Heading-delimited text chunk | Atomic proposition extracted by LLM |
| Memory granularity | Varies wildly (1 line to 5 pages) | Consistent (1 fact = 1 memory) |
| Document-level representation | None | Layer 1 document memory |
| Graph construction | Wiki-links + similarity only | Wiki-links + similarity + LLM-extracted entities/edges |
| Structural splitting role | Defines memory boundaries | Feeds LLM extraction (intermediate artifact) |
| Context preservation | None (chunk is orphaned) | Anthropic contextual prepend for large files |

---

## Research Evidence

### Proposition-Level Retrieval (Dense X Retrieval)

**Paper:** Chen et al., "Dense X Retrieval: What Retrieval Granularity Should We Use?" EMNLP 2024

- Propositions defined as "atomic expressions, each encapsulating a distinct factoid, presented in concise, self-contained natural language"
- **+10.1% Recall@20** over passage-level retrieval on unsupervised dense retrievers
- **+2.7% Recall@20** on supervised retrievers
- Downstream QA: **+2.7 to +4.1 exact match points** with LLaMA-2-7B at 500-token budget
- Propositions deliver "higher density of question-related information"

Source: https://aclanthology.org/2024.emnlp-main.845/

### Adaptive Chunking (Logical Boundaries)

**Paper:** Clinical Decision Support study, MDPI Bioengineering, November 2025

- Compared: recursive character-based, semantic cluster, proposition-based, adaptive chunking
- Adaptive chunking (aligns with logical discourse units): **87% accuracy**
- Fixed-size baseline: **13% accuracy** (p=0.001)
- Key insight: boundaries must align with how information is actually structured

Source: https://pmc.ncbi.nlm.nih.gov/articles/PMC12649634/

### Structural vs Semantic Chunking (Comprehensive Taxonomy)

**Paper:** "Beyond Chunk-Then-Embed: A Comprehensive Taxonomy and Evaluation of Document Chunking Strategies," February 2026

- Structure-based methods (paragraph splitting) **outperform** semantic/LLM-guided methods for in-corpus retrieval
- Proposition-based methods show **15-27% degradation** when used alone (over-fragmentation)
- **Contextualized chunking recovers** proposition performance: +22.87% to +27.11% improvement
- Recommendation: structural splitting for recall, propositions for precision. **Use both.**

Source: https://arxiv.org/html/2602.16974

### Anthropic Contextual Retrieval

**Technique:** Prepend chunk-specific context before embedding

- Pass whole document + target chunk to LLM
- LLM generates 50-100 token context prefix explaining the chunk's role in the document
- **49% reduction in retrieval failures** (combined with BM25)
- **67% reduction** with reranking added
- Cost: **$1.02 per million document tokens** with prompt caching

Exact prompt used:
```
<document>
{{WHOLE_DOCUMENT}}
</document>
Here is the chunk we want to situate within the whole document
<chunk>
{{CHUNK_CONTENT}}
</chunk>
Please give a short succinct context to situate this chunk within
the overall document for the purposes of improving search retrieval
of the chunk. Answer only with the succinct context and nothing else.
```

Source: https://www.anthropic.com/news/contextual-retrieval

### How Other Memory Engines Ingest

**Mem0** -- Does NOT chunk documents. LLM extracts facts/propositions from conversations. Each extracted fact is one memory. No headings, no splitting.
Source: https://arxiv.org/html/2504.19413v1

**Zep/Graphiti** -- Treats inputs as "episodes" (messages, text, JSON). LLM extracts entities and relationships into a temporal knowledge graph. Episodes are ground truth, entities/edges are the queryable layer.
Source: https://arxiv.org/html/2501.13956v1

**Cognee** -- Modular ECL pipeline: extract, cognify, load. Chunks are intermediate; entity extraction and relationship mapping produce the actual knowledge graph.
Source: https://docs.cognee.ai/core-concepts

**Microsoft GraphRAG** -- Chunks documents into 50-100 token units, extracts entities and relationships from each chunk via LLM, builds community-structured knowledge graph. Chunks are discarded after extraction.
Source: https://microsoft.github.io/graphrag/index/methods/

### Mix-of-Granularity (Query-Adaptive Retrieval)

**Paper:** "Mix-of-Granularity: Optimize the Chunking Granularity for Retrieval-Augmented Generation," 2024

- Trained router selects optimal chunk granularity per query at retrieval time
- Fine-grained queries -> proposition-level; broad queries -> document-level
- **Consistently outperforms** fixed-granularity baselines across 5 datasets
- Validates the need for multi-granularity storage (Layers 1 + 2)

Source: https://arxiv.org/html/2406.00456v1

---

## Cost Estimates

Assuming Haiku-class model for extraction ($0.25/MTok input, $1.25/MTok output):

| Vault Size | Est. Tokens | Extraction Cost | Notes |
|-----------|------------|----------------|-------|
| 100 files, 1K avg | 100K | ~$0.15 | Small personal vault |
| 1,000 files, 2K avg | 2M | ~$1.50 | Medium knowledge base |
| 10,000 files, 3K avg | 30M | ~$15 | Large enterprise vault |

One-time cost per file, re-run only when file content changes (checksum-based, same as current pipeline).

With Anthropic prompt caching: document context is cached across chunk extractions within the same file, reducing cost further for large files.

---

## Implementation Plan

### Phase 1: Proposition Extraction (Layer 2)

1. Add `MemoryKind::Proposition` variant
2. Add LLM extraction step to Phase 2 of ingest pipeline
3. Extraction prompt: extract atomic propositions from file/chunk content
4. Each proposition becomes a memory linked to source file
5. Existing heading-based sections become processing units (not stored as memories)
6. Fallback: if no LLM configured, fall back to current heading-based chunking

### Phase 2: Document Memory (Layer 1)

1. Store one document-level memory per file
2. Small files: embed the full content
3. Large files: LLM-generated summary as content, full file as provenance
4. Link all Layer 2 propositions to their Layer 1 document memory

### Phase 3: Entity-Relationship Extraction (Layer 3)

1. Extend extraction prompt to also output entities and relationships
2. Add entity resolution (dedup "Perplexity AI" / "Perplexity" / "the company")
3. Store as graph edges (new edge types: `entity`, `relationship`)
4. Temporal metadata on edges (when was this fact true?)

### Phase 4: Contextual Extraction for Large Files

1. For files > 4K tokens, use structural splits as processing units
2. Prepend document context to each chunk before LLM extraction
3. Entity resolution across chunks within the same file
4. Document summary generation for Layer 1

### Configuration

```toml
[extraction]
# LLM provider for proposition/entity extraction
provider = "anthropic"  # or "openai", "ollama"
model = "claude-haiku-4-5-20251001"

# Extraction mode
mode = "propositions"  # "propositions" | "headings" (legacy) | "off"

# Large file threshold (tokens) -- above this, use chunked extraction
large_file_threshold = 4096

# Max propositions per file (safety limit)
max_propositions_per_file = 200
```

---

## References

1. Chen et al. "Dense X Retrieval: What Retrieval Granularity Should We Use?" EMNLP 2024. https://aclanthology.org/2024.emnlp-main.845/
2. "Comparative Evaluation of Advanced Chunking for RAG in LLMs for Clinical Decision Support." MDPI Bioengineering, Nov 2025. https://pmc.ncbi.nlm.nih.gov/articles/PMC12649634/
3. "Beyond Chunk-Then-Embed: A Comprehensive Taxonomy and Evaluation of Document Chunking Strategies." arXiv, Feb 2026. https://arxiv.org/html/2602.16974
4. Anthropic. "Introducing Contextual Retrieval." Sep 2024. https://www.anthropic.com/news/contextual-retrieval
5. Mem0. "Building Production-Ready AI Agents with Scalable Long-Term Memory." arXiv, Apr 2025. https://arxiv.org/html/2504.19413v1
6. Rasmussen. "Zep: A Temporal Knowledge Graph Architecture for Agent Memory." arXiv, Jan 2025. https://arxiv.org/html/2501.13956v1
7. "Mix-of-Granularity: Optimize the Chunking Granularity for RAG." arXiv, Jun 2024. https://arxiv.org/html/2406.00456v1
8. "Efficient Knowledge Graph Construction and Retrieval from Unstructured Text for Large-Scale RAG Systems." arXiv, Jul 2025. https://arxiv.org/html/2507.03226v2
9. Microsoft. "GraphRAG Methods." https://microsoft.github.io/graphrag/index/methods/
10. Cognee. "Core Concepts." https://docs.cognee.ai/core-concepts
11. Jina AI. "Late Chunking: Contextual Chunk Embeddings Using Long-Context Embedding Models." arXiv, Sep 2024. https://arxiv.org/abs/2409.04701
