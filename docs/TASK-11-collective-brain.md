# TASK-11: Collective Brain — Distributed Organizational Learning

**Date:** 2026-03-13
**Status:** Concept
**Priority:** High — this is the flagship use case for HEBBS

---

## The Idea

Every developer in an org runs a local HEBBS instance on their machine. Throughout the day, HEBBS captures what works, what doesn't, decisions made, patterns discovered — automatically, from real work. Each night, local instances consolidate episodes into insights and push them to a central HEBBS server. The central server receives insights from all machines, reflects across them, and identifies convergent patterns. Importance is calculated based on how many independent sources produced similar insights. These org-level insights flow back to every developer's machine.

The org develops expertise the same way a brain does — neurons that fire together across many developers wire together as organizational knowledge.

---

## Why This Is Genuinely New

A wiki is what someone *chose* to write down. A knowledge base is what someone *curated*. This is what the org **actually learned** — automatically, from real work, with importance derived from convergence across independent sources.

The key mechanism: if 5 developers independently discover that "retrying failed gRPC calls with exponential backoff fixes the flaky integration tests," that pattern shows up as 5 separate episodes on 5 machines. When the central server reflects, the frequency signal is unmistakable. That insight gets high importance not because someone decided it was important, but because the org's collective experience proved it.

---

## Architecture

```
Developer machines (edge HEBBS)          Central HEBBS
┌──────────────────────────┐
│  Dev A's machine         │
│  episodes throughout     │             ┌─────────────────────┐
│  the day                 │──nightly──→ │                     │
│  reflect locally → local │  insights   │  Receives insights  │
│  insights                │             │  from all machines   │
└──────────────────────────┘             │                     │
┌──────────────────────────┐             │  Each insight is     │
│  Dev B's machine         │             │  remembered as a     │
│  same thing              │──nightly──→ │  new episode         │
│                          │  insights   │                     │
└──────────────────────────┘             │  Periodic reflect:  │
┌──────────────────────────┐             │  clusters insights   │
│  Dev C's machine         │             │  from ALL machines   │
│  same thing              │──nightly──→ │                     │
│                          │  insights   │  Convergent patterns │
└──────────────────────────┘             │  get high importance │
         ↑                               │                     │
         │                               └────────┬────────────┘
         │                                        │
         └────────────────────────────────────────┘
              org-level insights flow back
              to every developer's machine
```

### Flow

1. **During the day** — Local HEBBS on each developer's machine captures episodes: what worked, what failed, debugging breakthroughs, architectural decisions, patterns noticed. The agent (Claude Code, OpenClaw, etc.) uses `remember` throughout normal work.

2. **Nightly local reflect** — Each machine runs `reflect` to consolidate the day's raw episodes into local insights. "I tried 3 approaches to fix the auth timeout — only the connection pool resize worked" becomes a structured insight with confidence and source lineage.

3. **Sync up** — Local insights are pushed to the central HEBBS server. Each insight is stored as a new episode with metadata about its source machine, entity scope, and local confidence.

4. **Central reflect** — The central server periodically reflects across insights from ALL machines. This is where convergence-based importance kicks in:
   - An insight that appears from 1 developer gets baseline importance
   - The same pattern from 5 developers independently gets importance amplified
   - The same pattern from 10 developers becomes high-confidence org knowledge

5. **Sync down** — Org-level insights flow back to every developer's local HEBBS. When a developer starts working on a problem, `recall` surfaces insights from the entire org's experience — including from people they've never talked to.

---

## What the Importance Signal Captures

### Convergence = Importance

Local insight from one dev: "Adding `--no-cache` to the Docker build fixed the stale layer issue" — importance 0.6.

Central reflect sees this pattern from 8 of 12 developers in the same week. The org-level insight becomes: "Docker layer caching is causing widespread build failures after the base image update. `--no-cache` is the confirmed workaround." Importance: 0.95.

That insight flows back to the 4 developers who haven't hit the problem yet. They get it before they waste time on it.

### Decay = Self-Cleaning

The Docker cache insight has high importance this week. In 3 months, if nobody hits the issue again, it naturally decays. The org's memory stays current without manual curation.

### Reinforcement = Validation

Every time a developer's `recall` surfaces an org insight and they act on it (the agent uses it in a response), that insight gets reinforced. Insights that are actually useful in practice strengthen. Ones that seemed important but never get recalled decay.

---

## What This Replaces

| Current practice | Problem | Collective brain |
|---|---|---|
| Standup meetings | "Oh yeah I hit that too" — reactive, delayed | System already knows. Surfaces proactively. |
| Slack threads | Knowledge dies after 2 days | Persists, consolidates, strengthens with use |
| Internal wikis | Nobody updates them, always stale | Auto-generated from real work, always current |
| Onboarding docs | Written once, outdated in months | New devs get live insights from the org's actual recent experience |
| Post-mortems | Written once, forgotten | Causal chains live in memory, surface via analogical recall when similar patterns emerge |
| Tribal knowledge | Exists only in senior devs' heads | Captured automatically, shared automatically |

---

## What HEBBS Already Has

- **Edge mode** — runs on developer laptops, no network dependency during work
- **Entity scoping** — each dev's memories scoped to their entity, org insights are cross-entity
- **Reflect** — local consolidation already works
- **Decay + reinforcement** — stale org insights naturally lose relevance, useful ones strengthen
- **Analogical recall** — "have we seen a pattern like this before?" across the entire org's experience
- **Causal recall** — "what caused this failure?" traverses org-wide causal chains
- **Importance scoring** — weighted at encoding time, tunable per query

---

## What Needs to Be Built

### 1. Sync Protocol
- Local insights → central: push over gRPC, probably a new `sync` RPC
- Central insights → local: pull on startup or periodic poll
- Conflict resolution: insights are additive (no conflicts), but duplicate detection needed
- Bandwidth: only insights sync, not raw episodes — lightweight

### 2. Convergence-Based Importance Scoring
- Central reflect needs a new signal: how many independent sources contributed similar patterns
- Not just clustering by content — clustering by *structural similarity* across source machines
- Importance = f(confidence, source_count, source_diversity, recency)
- A pattern from 3 devs on the same team is less significant than 3 devs on different teams

### 3. Privacy Controls
- What stays local vs what gets synced
- Personal workflow preferences: local only
- Codebase patterns, debugging solutions, architectural decisions: sync
- Configurable per entity, per importance threshold, per content category
- Option: sync insights but not raw episodes (default)

### 4. Source Attribution
- Org insights carry "derived from N developers' experience" metadata
- Individual episodes are not exposed — only the consolidated insight
- Optional: attribute to teams, not individuals

### 5. Onboarding Mode
- New developer joins → local HEBBS bootstraps with all current org insights
- Effectively downloads the org's collective expertise on day one
- Scoped by team/project entity for relevance

---

## Example Scenarios

### Scenario 1: Debugging Pattern Propagation

Dev A spends 2 hours debugging a memory leak in the event processing pipeline. Discovers it's caused by unclosed gRPC streams in the error path. HEBBS captures the debugging journey and the fix.

That night, local reflect produces: "gRPC streams must be explicitly closed in error handlers — the default cleanup doesn't trigger on panics in async contexts."

Central server receives this. Two weeks later, Dev C hits a similar issue in a different service. Their `recall "memory leak in async handler"` surfaces Dev A's insight via analogical recall — structurally similar pattern (async + resource leak + error path), different domain.

Dev C solves it in 10 minutes instead of 2 hours.

### Scenario 2: Architecture Decision Memory

Over 3 months, 6 different developers try different approaches to rate limiting across the platform. HEBBS captures each attempt, what worked, what didn't.

Central reflect identifies the convergence: "Token bucket with per-tenant quotas stored in the request context outperforms sliding window approaches. Three teams independently arrived at this after trying sliding window first."

This becomes an org-level architectural insight. When Dev G starts building rate limiting for a new service, the insight surfaces immediately.

### Scenario 3: Onboarding Acceleration

New developer joins. Their local HEBBS syncs org insights on first boot. Before writing a single line of code, they have access to:
- "The CI pipeline requires protoc installed locally — brew install protobuf"
- "Integration tests must hit a real database, not mocks — we got burned by mock/prod divergence"
- "The auth middleware is being rewritten for compliance — don't build on the old one"
- "Deploy to staging before opening PR — the staging smoke test catches 80% of issues that CI misses"

All of this learned from real developer experience, not a wiki page written 18 months ago.

---

## Positioning

This is not "shared memory for agents." This is **an organization that learns from its own experience, automatically, and distributes that learning to every developer in real time.**

Git is distributed version control for code. HEBBS Collective Brain is distributed learning for knowledge.

---

## Open Questions

1. **Who controls the sync?** Per-org admin? Per-developer opt-in? Always-on?
2. **How granular is privacy?** File-level? Entity-level? Content classification?
3. **Central server hosting** — self-hosted? HEBBS Cloud? Both?
4. **Cross-org insights** — anonymized pattern sharing across organizations? (future, if ever)
5. **Feedback loop** — should developers be able to downvote/flag org insights that are wrong or outdated?
