# Benchmark Analysis: LoCoMo & LongMemEval

## LoCoMo (Long-term Conversational Memory)

**Paper:** Snap Research + UNC Chapel Hill, ACL 2024 ([arXiv:2402.17753](https://arxiv.org/abs/2402.17753))
**Repo:** `snap-research/locomo`

Tests how well agents retain and reason over information from very long conversations spanning multiple sessions over weeks/months.

### Tasks

1. **Question Answering** -- 5 reasoning types:
   - Single-hop: answers from a single turn
   - Multi-hop: combine info across multiple turns/sessions
   - Temporal: reason about when events happened or their order
   - Commonsense/World Knowledge: external knowledge + conversational context
   - Adversarial: correct answer is "I don't know" (tests against fabrication)

2. **Event Summarization** -- summarize history into a structured event graph (causal and temporal understanding)

3. **Multimodal Dialogue Generation** -- generate responses consistent with past conversations including shared images

### Dataset

- 10 conversations, ~600 turns each, ~16K tokens average
- Up to 32 sessions per conversation, spread over simulated weeks/months
- Machine-human pipeline: LLM agents generate dialogues grounded on personas and temporal event graphs, human annotators verify
- Includes shared images integrated into dialogue

### Metrics

| Task | Metrics |
|------|---------|
| QA | Partial-match F1; LLM-as-judge accuracy (binary) |
| Event Summarization | ROUGE; FactScore (atomic fact precision/recall) |
| Multimodal Dialogue Generation | BLEU, ROUGE-L, MM-Relevance |

---

## LongMemEval (Long-Term Interactive Memory)

**Paper:** UCLA + Tencent AI Lab, ICLR 2025 ([arXiv:2410.10813](https://arxiv.org/abs/2410.10813))
**Repo:** `xiaowu0162/LongMemEval`

Tests whether chat assistants can remember, retrieve, and reason over long interaction histories.

### 5 Core Abilities Tested

1. **Information Extraction** -- recall specific details from extensive interactive histories
2. **Multi-Session Reasoning** -- synthesize across sessions (aggregation, comparison)
3. **Temporal Reasoning** -- awareness of time aspects, explicit time mentions and timestamp metadata
4. **Knowledge Updates** -- recognize changed user info, dynamically update and use latest version
5. **Abstention** -- correctly say "I don't know" for never-discussed info

### Question Types

| Question Type | Description |
|---|---|
| single-session-user | Retrieve info mentioned by the user in a single session |
| single-session-assistant | Retrieve info mentioned by the assistant in a single session |
| single-session-preference | Extract implicit user preferences for personalized responses |
| multi-session | Synthesize across multiple sessions (aggregation, comparison) |
| temporal-reasoning | Reason about time-related aspects |
| knowledge-update | Recognize changed user info, use latest version |
| abstention (_abs variants) | Correctly refuse when info was never discussed |

### Dataset

- 500 curated questions, embedded within scalable chat histories
- Two scale settings:
  - **LongMemEval_S (small):** ~115K tokens, ~30-40 sessions
  - **LongMemEval_M (medium):** ~1.5M tokens, ~500 sessions
- Evidence diversified across multi-session, multi-turn interactions with realistic distractor sessions

### Metrics

- **Answer quality:** GPT-4o as LLM judge (>97% agreement with human experts), accuracy per question type
- **Retrieval quality:** Recall@k and NDCG@k

### Proposed Framework

The paper also proposes a unified three-stage framework for long-term memory: **indexing, retrieval, and reading**, with optimizations (session decomposition, fact-augmented key expansion, time-aware query expansion).

---

## Comparative Summary

| Dimension | LoCoMo | LongMemEval |
|---|---|---|
| Focus | Conversational memory + multimodal | Interactive memory at scale |
| Scale | ~16K tokens/conversation | 115K to 1.5M tokens |
| Modality | Text + images | Text only |
| Tasks | QA + summarization + generation | QA focused (7 question types) |
| Knowledge updates | Not explicit | Explicitly tested |
| Venue | ACL 2024 | ICLR 2025 |
| Dataset size | 10 conversations | 500 questions |
| Key finding | -- | ~30% accuracy drop in commercial assistants for sustained interactions |
