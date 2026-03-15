# Use Case Analysis: Voice Call Sales Agent (Powered by HEBBS)

## Overview

A voice call sales agent that converts prospects into customers is one of the most demanding tests for an agentic memory primitive. It is high-stakes (revenue), high-volume (hundreds/thousands of calls), and deeply relational (memory of *people* and *patterns* over time).

This document maps the five proposed memory operations — `remember`, `recall`, `reflect`, `revise`, `forget` — to the real workflow of such an agent.

## The Agent's Lifecycle

A sales call agent operates across thousands of calls:

1. **Before a call** — prepare by recalling past interactions with this prospect, and patterns from similar deals.
2. **During a call** — listen, adapt pitch, handle objections, pick up on signals.
3. **After a call** — log what mattered, schedule follow-ups.
4. **Over time** — get *better* at selling by learning what works.

Current systems dump transcripts into a CRM and maybe a vector DB. The agent starts every call approximately from scratch, with keyword-searchable notes at best.

---

## Mapping the Five Operations

### 1. `remember` — What to Encode from a Call

A 30-minute sales call produces ~5,000 words of transcript. 95% is noise. What matters:

- "Prospect mentioned their contract with Competitor X expires in 3 weeks" → **high importance**
- "Prospect's tone shifted negative when pricing came up" → **medium importance**
- "Prospect said 'uh-huh' 47 times" → **discard**

The primitive scores importance at write time:

```
remember(
  experience: "Prospect expressed urgency — competitor contract ending March 15",
  importance: 0.95,
  context: { prospect_id, deal_stage: "discovery", signal: "urgency" }
)
```

**What breaks without this:** Current systems store the entire transcript or nothing. Retrieval drowns in noise. The agent has no sense of what *mattered*.

---

### 2. `recall` — The Four Retrieval Paths a Sales Agent Needs

#### Temporal — "What did I promise this prospect last Tuesday?"

The most basic sales need, and current systems fail at it. Vector similarity search on "what did I discuss with Acme Corp" might return a random high-similarity chunk from any call. The agent needs chronologically ordered recall tied to a specific prospect.

```
recall(cue: { prospect_id: "acme-corp" }, strategy: temporal)
→ [Call 3: promised ROI calculator by Friday]
→ [Call 2: discussed security concerns, sent SOC2 doc]
→ [Call 1: initial discovery, pain point = manual reporting]
```

#### Causal — "Why did the last similar deal fall through?"

```
recall(cue: "deal lost after pricing discussion", strategy: causal)
→ [Prospect went silent after we quoted annual pricing]
→ [Root cause: didn't establish ROI before pricing]
→ [Pattern: deals that skip ROI stage close at 12% vs 45%]
```

#### Analogical — "I've never sold to healthcare, but what's structurally similar?"

```
recall(cue: "healthcare compliance objection", strategy: analogical)
→ [Structurally similar to finance compliance objections]
→ [What worked: lead with audit trail, not features]
→ [Adaptation: replace SOC2 with HIPAA framing]
```

This is how top human salespeople work — they don't have a playbook for every industry, they pattern-match from past experience.

#### Similarity — "Has anyone asked this exact objection before?"

```
recall(cue: "we already built this in-house", strategy: similarity)
→ [12 past prospects raised this objection]
→ [Best response: "How much engineering time does maintenance cost?"]
```

**What breaks without multi-path recall:** With only similarity search, the agent can surface "related" chunks but can't reconstruct timelines, can't trace cause-and-effect, and can't transfer knowledge across domains.

---

### 3. `reflect` — Turning 500 Calls into a Sales Playbook

After hundreds of calls, individual episodes should consolidate into general knowledge:

```
reflect()

// Input: 500 episodic memories of calls
// Output:
→ "When prospects mention Competitor X, leading with integration
   story closes 3x better than leading with price"
→ "Healthcare prospects won't discuss technical details until
   compliance questions are addressed"
→ "Deals where ROI is established before pricing close at 45%
   vs 12% when pricing comes first"
→ "Tuesday 10am calls convert 40% higher than Friday 3pm calls"
```

This is the **compounding advantage**. Call 501 is dramatically better than call 1, not because the agent has more data, but because it has *distilled knowledge*.

**What breaks without this:** The agent has 500 stored transcripts but no wisdom. Every insight requires a human sales manager to manually review calls and update playbooks.

---

### 4. `revise` — Updating Beliefs, Not Appending Contradictions

**Scenario:** The agent "learned" that a 20% discount closes hesitant prospects. Then pricing changes and that discount is no longer available.

```
revise(
  memory_id: "discount-closing-strategy",
  new_evidence: "Max discount now 10%. Shift to value-add strategy
                 (extended onboarding, dedicated CSM) instead."
)
```

Other examples:
- A product capability the agent has been pitching gets deprecated.
- A competitive comparison turns out to be based on outdated information.
- A previously effective objection-handling script starts underperforming.

**What breaks without this:** The agent retrieves both "offer 20% discount to close" AND "20% discount no longer available" and must resolve the contradiction in-context every time, wasting tokens and risking errors.

---

### 5. `forget` — Active Pruning

| Scenario | Action |
|---|---|
| Prospect went dark 8 months ago | Decay urgency signals, retain company profile |
| Competitive intelligence from 2024 | Prune — the landscape changed |
| GDPR deletion request | Targeted, compliant erasure |
| Abandoned experimental pitch approaches | Remove so they don't resurface |

```
forget(criteria: { staleness: "> 6 months", access_count: 0, type: "urgency_signal" })
```

**What breaks without this:** Memory grows unboundedly. Retrieval quality degrades as the ratio of stale-to-relevant memories increases. Eventually the agent is slower and *less* accurate than when it started.

---

## The Compound Effect

The full loop in practice:

```
Call #1 with prospect
  └─ remember(key signals, importance, context)

Call #2 with same prospect
  └─ recall(prospect_id, temporal) → pick up exactly where you left off
  └─ recall(objection, similarity) → handle with proven response

After 100 calls
  └─ reflect() → distill patterns into playbook knowledge

Call #101 with new prospect
  └─ recall(industry + role, analogical) → apply cross-domain patterns
  └─ Agent now performs like a rep with months of experience

Market changes
  └─ revise() → update outdated competitive intel, pricing strategies
  └─ forget() → prune stale signals, maintain retrieval quality
```

The agent gets **compounding returns** on every call. That's what no current system delivers — they give you storage with retrieval, but not learning.

---

## Key Insight

The voice sales agent use case reveals that memory is not a feature — it is the **differentiator between a stateless chatbot and a top-performing sales rep**. Every dimension of the proposed primitive (`remember`, `recall`, `reflect`, `revise`, `forget`) maps directly to something a great human salesperson does instinctively. The primitive's job is to make this systematic and scalable.
