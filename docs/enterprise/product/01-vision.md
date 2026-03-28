# HEBBS Enterprise: Product Vision

## One-liner

HEBBS Enterprise is a memory engine for AI agents — deployed on the customer's infrastructure, managed by us.

## The problem

Companies building AI agents need memory. Their agents answer questions from docs, have conversations with users, learn from corrections — but every conversation starts from zero. The context window is a hack. RAG is a partial fix. Real memory requires temporal reasoning, importance weighting, contradiction detection, and causal graph traversal.

HEBBS has all four. The engine is production-ready. But getting enterprise customers started has been too hard: install a binary, configure TOML, manage a daemon, choose embedding models, run manual tuning. That's a project before they see any value.

## The solution

We deploy HEBBS on the customer's machine. One Docker package, everything included. We configure it, hand them the dashboard, and walk them through their first recall — on their data, on their infrastructure, in under an hour.

Their team connects from their laptops and servers via CLI and SDK. Their agents integrate with two API calls. The customer's data never leaves their infrastructure. We monitor health and usage from our central dashboard.

## Who it's for

**Enterprise buyers** who:

- Have built (or are building) AI agents
- Have a document corpus their agent needs to know
- Have users generating conversations the agent should learn from
- Need data to stay on their infrastructure (compliance, security, policy)
- Want a managed solution, not a DIY toolkit

## What we believe

1. **Memory is the next infrastructure primitive for AI agents.** HEBBS has four recall strategies (similarity, temporal, causal, analogical), composite scoring, automatic reflection, and contradiction detection. No one else has all of these.
2. **Quality is non-negotiable.** OpenAI embeddings (text-embedding-3-small, 1536d) for embedding quality. Customer brings their own API key — they control the provider relationship and costs.
3. **The engine is the moat.** The dashboard, CLI, and SDKs are the experience layer. The engine is what's irreplaceable. We never modify the engine for enterprise packaging — we wrap it.
4. **Deploy on their infra, monitor from ours.** The customer owns their data and infrastructure. We deploy, configure, and monitor. They see the dashboard. We see health and usage from our central admin panel.
5. **Tuning is in the customer's hands.** Their own AI agent drives the tuning process via CLI/SDK. We provide the tools. They don't need us to tune.

## Enterprise-first, SaaS later

Enterprise proves the product works, generates revenue, and builds case studies. Once we have 10+ enterprise deployments, we wrap the same platform into a multi-tenant SaaS offering. The codebase is identical — the only difference is who runs the server.

## Success metrics

- **Time to first recall:** Under 1 hour from deployment start to a working recall on customer's data
- **Integration time:** Under 1 day from deployment to agent integration in customer's codebase
- **Retention signal:** Customer's agents making recall calls daily in month 2+
- **Expansion signal:** Customer creating additional workspaces for more agents

## Constraints

- The `hebbs` engine repo is never modified for enterprise. Containerize as-is.
- Customer brings their own OpenAI API key.
- Customer's data never leaves their infrastructure. Our central dashboard only sees health metrics and usage counts.
- The Docker package must be self-contained — customer should be able to `docker compose up` and have everything running.

