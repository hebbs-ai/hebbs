# TASK-36: Landing Page Enterprise Repositioning

**Status:** Planning
**Date:** 2026-03-31
**Scope:** hebbs-website (Astro + Tailwind)

## Objective

Reposition the landing page from open-source developer tool to enterprise research company. No code changes yet — this document captures the strategic direction and wireframes.

## Current State Diagnosis

The current landing page is textbook developer-tools OSS:

- Hero leads with `curl | sh` install command and "Star on GitHub" CTA
- 22 feature sections creating a long, feature-catalog style page
- Nav anchors to technical features (Vault, Recall, API, Benchmarks)
- No enterprise section (security, compliance, deployment SLAs)
- No social proof (customer logos, testimonials, case studies)
- No use cases or industry verticals
- Comparison table pits HEBBS against OSS competitors (pgvector, Qdrant, Neo4j)
- All CTAs point to self-serve (brew install, GitHub)

## Reference Companies

Positioning shifts referenced as models:
- **Pinecone** — vector DB to enterprise AI infra
- **Elastic** — OSS search to security/observability platform
- **Databricks** — Spark OSS to data intelligence platform
- **Neo4j** — graph DB to enterprise graph analytics
- **Temporal** — OSS workflow to enterprise orchestration

---

## 10 Directional Changes

### 1. Hero: Enterprise Value Proposition over Install Command

**Current:** `curl -sSf https://hebbs.ai/install | sh` + "Memory that wires itself"

**Direction:**

```
┌─────────────────────────────────────────────────────┐
│  [Nav: Platform | Research | Enterprise | Docs | Blog | Talk to Sales]  │
├─────────────────────────────────────────────────────┤
│                                                     │
│      "Cognitive Memory Infrastructure                │
│       for Enterprise AI"                            │
│                                                     │
│   The memory engine that gives your AI agents       │
│   temporal reasoning, causal chains, and            │
│   consolidation. Deployed on-prem or in your VPC.   │
│                                                     │
│   [ Request a Demo ]    [ Read the Research ]       │
│                                                     │
│   Trusted by teams building production AI at:       │
│   [logo] [logo] [logo] [logo]                       │
│                                                     │
└─────────────────────────────────────────────────────┘
```

- Kill `curl | sh` from hero (move to Docs/Developers page)
- Kill "Star on GitHub" as primary CTA
- Primary CTA: "Request a Demo" or "Talk to Sales"
- Secondary CTA: research/whitepaper, not GitHub
- Add trust signals (customer logos, even if "design partners" initially)

### 2. Navigation: Audience Journeys over Feature Anchors

**Current:** `Vault | Recall | API | Benchmarks | Architecture | Docs | GitHub`

**Direction:**

```
Left:   Platform | Research | Enterprise | Pricing
Right:  Docs | Blog | [Talk to Sales button]
```

- "Platform" = product overview (what the engine does)
- "Research" = published papers, benchmarks, technical depth
- "Enterprise" = deployment, security, compliance, support
- Move GitHub to footer or Docs sub-nav

### 3. Social Proof Section (NEW — immediately below hero)

```
┌─────────────────────────────────────────────────────┐
│  "Powering cognitive AI at scale"                   │
│                                                     │
│  ┌─────────┐  ┌─────────┐  ┌──────────────────┐   │
│  │  "10ms   │  │  "100M   │  │  "91% precision  │   │
│  │  p99"    │  │  memories"│  │  temporal recall" │   │
│  │  at 10M  │  │  tested  │  │  vs 23% baseline"│   │
│  └─────────┘  └─────────┘  └──────────────────┘   │
│                                                     │
│  [ Customer quote or design partner testimonial ]   │
│                                                     │
└─────────────────────────────────────────────────────┘
```

### 4. Page Flow: Problem > Solution > Proof > Deploy

**Current flow** (22 sections, feature-exhaustive):
Hero > AgentControl > HowItWorks > BrainAnalogy > OutOfTheBox > Vault > MemoryPalaceDemo > BlogSlider > RecallStrategies > Consolidation > ScoringWeights > Operations > Benchmarks > Problem > Architecture > Comparison > CodeExample > Deployment > CTA

**Proposed enterprise flow:**

1. Hero (enterprise value prop + demo CTA)
2. Trust bar (logos / stats)
3. Problem (why vector DBs aren't enough) — keep, reframe for CTO audience
4. Platform overview (consolidate 6+ feature sections into 3 pillars)
5. Research / Benchmarks (secret weapon — keep, elevate)
6. Enterprise section (NEW: security, compliance, deployment, SLAs)
7. Use cases (NEW: 2-3 industry verticals or agent patterns)
8. Architecture (keep, condense)
9. Customers / Case studies (NEW, even if "design partner" stories)
10. CTA (demo / contact sales)

**Cut or move to sub-pages:** MemoryPalaceDemo, ScoringWeights, Operations detail, CodeExample, Deployment modes, BlogSlider, BrainAnalogy, Comparison table. Great for /developers or /docs — dilutes the enterprise message on landing page.

### 5. Consolidate Features into 3 Enterprise Pillars

Instead of 10+ individual feature sections:

```
┌───────────────┐ ┌───────────────┐ ┌────────────┐
│ Cognitive      │ │ Production    │ │ Enterprise │
│ Recall         │ │ Performance   │ │ Ready      │
│                │ │               │ │            │
│ 4 recall       │ │ <10ms p99     │ │ On-prem /  │
│ strategies     │ │ at 100M       │ │ VPC deploy │
│ Temporal,      │ │ Zero network  │ │ SOC2 ready │
│ causal,        │ │ hops          │ │ Tenant     │
│ analogical,    │ │ Single binary │ │ isolation  │
│ similarity     │ │ No JVM/Python │ │ Audit logs │
└───────────────┘ └───────────────┘ └────────────┘
```

### 6. Enterprise / Security Section (NEW)

```
┌─────────────────────────────────────────────────────┐
│  "Built for regulated, air-gapped,                  │
│   and compliance-driven environments"               │
│                                                     │
│  ✓ Single binary, no external dependencies          │
│  ✓ On-prem / VPC / air-gapped deployment            │
│  ✓ Structural tenant isolation                      │
│  ✓ Local embeddings (no data leaves your network)   │
│  ✓ Audit trail with full memory lineage             │
│  ✓ BSL 1.1 licensing (convert to Apache after 3yr)  │
│                                                     │
│  [ Download Security Whitepaper ]                   │
│                                                     │
└─────────────────────────────────────────────────────┘
```

### 7. Use Cases / Verticals Section (NEW)

```
┌──────────────────────────────────────────────┐
│  "How teams use HEBBS"                        │
│                                               │
│  [Sales Intelligence]                         │
│   Agents that remember every deal, surface    │
│   patterns across accounts, catch conflicts   │
│                                               │
│  [Knowledge Agents]                           │
│   Internal copilots with institutional memory │
│   that consolidates and decays automatically  │
│                                               │
│  [Autonomous Operations]                      │
│   Edge agents on-device with offline memory,  │
│   fleet sync when connected                   │
│                                               │
└──────────────────────────────────────────────┘
```

### 8. Research as Brand Identity

This is the moat. Pinecone and Qdrant don't publish neuroscience-inspired research.

```
┌──────────────────────────────────────────────┐
│  "Built on cognitive science,                 │
│   not just linear algebra"                    │
│                                               │
│  HEBBS implements Hebbian learning theory     │
│  as engineered infrastructure: consolidation, │
│  temporal decay, causal graphs, analogical    │
│  transfer. Not wrapper tricks on vector DBs.  │
│                                               │
│  [Benchmark methodology]                      │
│  [Architecture paper]                         │
│  [Blog: research deep dives]                  │
│                                               │
└──────────────────────────────────────────────┘
```

### 9. CTA: Enterprise Close

**Current:** `brew install` + "Star on GitHub" + "Talk to the founder"

**Direction:**

```
┌──────────────────────────────────────────────┐
│  "Ready to give your agents real memory?"     │
│                                               │
│  [ Schedule a Demo ]     [ Read the Docs ]    │
│                                               │
│  Or: deploy the open-source engine yourself   │
│  github.com/hebbs-ai/hebbs                    │
│                                               │
└──────────────────────────────────────────────┘
```

Enterprise CTA is primary. OSS is a secondary escape hatch, not the headline.

### 10. Footer: Enterprise-Complete

**Current:** Logo, GitHub, Contact, copyright.

**Direction:**

```
Platform     |  Research      |  Enterprise    |  Developers    |  Company
Overview     |  Benchmarks    |  Security      |  Documentation |  About
Use Cases    |  Architecture  |  Deployment    |  GitHub        |  Blog
Pricing      |  Publications  |  Support/SLAs  |  SDKs          |  Careers
                                                                |  Contact
```

---

## Summary: The 5 Biggest Moves

| # | Change | Why |
|---|--------|-----|
| 1 | Hero: demo CTA over curl install | Enterprise buyers don't curl-pipe-sh |
| 2 | Add trust signals (logos, stats, quotes) | Social proof is #1 enterprise conversion driver |
| 3 | Add Enterprise section (security, compliance, deployment) | Table stakes for any enterprise conversation |
| 4 | Collapse 22 sections into ~8 with clear narrative arc | Problem > Solution > Proof > Deploy |
| 5 | Create sub-pages for developers (/developers) and research (/research) | Preserve technical depth without diluting enterprise message |

The OSS content doesn't disappear — it moves to `/developers`. The research content gets elevated to `/research` as a credibility asset. The landing page becomes a tight enterprise pitch: problem, solution, proof, trust, CTA.
