# HEBBS Cloud: Product Vision in future 2027 after enterprise

## One-liner

HEBBS Cloud is a managed memory service that makes AI agents smarter over time — zero infrastructure, one API key.

## The problem

Companies are building AI agents. These agents are stateless. Every conversation starts from zero. The agent doesn't remember what it learned yesterday, what the user corrected last week, or what the docs actually say.

The companies that build these agents know they need memory. But building a memory system is hard:

- Vector search alone isn't enough (no temporal reasoning, no contradiction detection, no causal chains)
- Tuning retrieval for a specific domain requires expertise most teams don't have
- Managing embedding models, storage, indexing pipelines, and decay is infrastructure work that distracts from the product

HEBBS solves all of this — but today it requires installing a binary, configuring TOML files, managing a daemon, choosing embedding models, and running an 8-phase manual tuning process. That's a weekend project before a customer sees any value.

## The opportunity

Wrap the existing HEBBS engine in a managed cloud service. Customers sign up, get an API key, push their docs, wire two SDK calls into their agent, and they're done. Everything else — embedding, indexing, reflection, tuning, contradiction detection — happens behind the scenes.

The engine is production-ready. The REST API exists. Multi-tenancy exists. Auth exists. The gap is a platform layer (sign-up, billing, workspace management, regional deployment) and a simplified customer interface (cloud CLI + updated SDKs).

## Who it's for

**Primary buyer:** A company that has built (or is building) an AI agent and wants it to accumulate knowledge over time.

They have:

- A document folder (product docs, policies, SOPs, knowledge base)
- Users having conversations with their agent
- A desire for the agent to get better without manual intervention

They are:

- Developers or small engineering teams
- Using Python or TypeScript
- Deploying agents in the cloud (not local)
- Willing to pay for infrastructure that saves them engineering time

**They are NOT:**

- Researchers building their own memory systems
- Teams that need on-prem only (that's the open-source tier)
- Companies looking for a chatbot builder (we're infrastructure, not application)

## What we believe

1. **Memory is the next infrastructure primitive for AI agents.** Context windows are a hack. RAG is a partial fix. Real memory requires temporal reasoning, importance weighting, contradiction detection, and causal graph traversal. HEBBS has all four.
2. **Quality is non-negotiable.** We use OpenAI embeddings (text-embedding-3-small, 1536d) because they're the best available at scale. We don't compromise on recall quality to save pennies.
3. **The engine is the moat.** The platform layer (auth, billing, routing) is table stakes. The four recall strategies, composite scoring, automatic reflection, and contradiction detection — that's what no one else has. We don't change the engine for cloud. We wrap it.
4. **Open source and cloud coexist.** The open-source `hebbs` binary is the full engine. Cloud is the same engine, managed. Customers can migrate between them. This builds trust and adoption.
5. **Tuning is in the customer's hands.** The customer's own AI agent (Claude, GPT, etc.) drives the tuning process — define ICP, generate evals, review them, run baselines, and tune. We provide the tune commands via the cloud CLI and SDK. The customer doesn't need us to tune; their agent follows the tune skill and uses `hb` commands. Auto-tune is a future enhancement, not a launch feature.

## Success metrics

- **Time to first recall:** Under 10 minutes from sign-up to a working `recall()` call
- **Activation rate:** 60%+ of sign-ups push at least one document within 24 hours
- **Retention signal:** Customers making recall calls in week 4+
- **Expansion signal:** Customers creating a second workspace

## Constraints

- The `hebbs` engine repo is never modified for cloud. Zero changes to the open-source codebase.
- Data never leaves the customer's chosen region. Only metadata (workspace name, usage counts) flows to the central platform.
- We start with 2 regions (US, EU). Architecture must support adding regions without platform changes.
- OpenAI is the embedding and LLM provider at launch. The architecture should allow swapping providers later, but we don't build for it now.

