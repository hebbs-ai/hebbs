# HEBBS Cloud: Product Specification

A managed memory service that makes AI agents smarter over time.

---

## Documents

| # | Document | Audience | What it answers |
|---|---|---|---|
| 01 | [Vision](01-vision.md) | Everyone | Why are we building this? Who is it for? What do we believe? |
| 02 | [Customer Journey](02-customer-journey.md) | Product, Sales, Engineering | What does a customer's life look like from sign-up to month 6? |
| 03 | [Architecture](03-architecture.md) | Engineering, Infra | How do the pieces fit together? Central vs regional? Data flow? |
| 04 | [Platform Services](04-platform-services.md) | Backend Engineering | Auth, billing, workspaces, workspace lifecycle — how does each work? |
| 05 | [API Spec](05-api-spec.md) | Backend Engineering, SDK team | Every endpoint, request/response format, error codes |
| 06 | [SDK Spec](06-sdk-spec.md) | SDK team, Developer Relations | Python and TypeScript SDK surface, design principles |
| 07 | [CLI Spec](07-cli-spec.md) | CLI team, Developer Relations | Every command, flags, output format |
| 08 | [Tuning](08-tuning.md) | Engineering, Product, Customer's Agent | User-driven tuning via their own agent + cloud CLI/SDK |
| 09 | [Pricing and Plans](09-pricing-and-plans.md) | Product, Finance, Sales | Tiers, limits, metering, cost model |
| 10 | [Data Residency](10-data-residency.md) | Engineering, Legal, Compliance | What lives where, GDPR, regional isolation |
| 11 | [Deployment](11-deployment.md) | Infra, DevOps | Kubernetes architecture, provisioning, monitoring, backup |
| 12 | [Rollout Plan](12-rollout-plan.md) | Engineering, Product, Leadership | Build order, milestones, timeline, team sizing |
| 13 | [AWS Deployment](13-aws-deployment.md) | DevOps, Infra | Concrete AWS setup: EKS, RDS, ALB, Route 53, CI/CD, costs |

---

## Key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Engine changes for cloud | Zero | Engine is the moat. Don't pollute it. Containerize as-is. |
| Embedding provider | OpenAI text-embedding-3-small | Quality is non-negotiable. +21pp recall over local ONNX. |
| LLM for extraction | OpenAI gpt-4o-mini | Good extraction quality, low cost. |
| Customer-facing API | REST | Universal, debuggable with curl, works everywhere. |
| Internal communication | gRPC (existing) | Gateway → workspace container uses existing hebbs-server API. |
| Storage | RocksDB per workspace (existing) | No engine changes. One container per workspace. |
| Workspace isolation | Container isolation | Not shared storage. Each workspace is a separate process + volume. |
| Regions at launch | US + EU | Covers 90% of initial demand. Architecture supports adding more. |
| New repos | 2 (hebbs-platform, hb) | Clean separation. Engine and existing SDKs get small additions. |
| Tuning | User-driven via their own agent | Customer's agent drives tune via CLI/SDK. Auto-tune is parked for post-launch. |
| Pricing model | Hard limits, not overages | No surprise bills. Clear upgrade triggers. |

---

## Repo map

```
hebbs-repos/
├── hebbs/                  # EXISTING — engine (containerize, never modify)
├── hebbs-platform/         # NEW — gateway, auth, billing, workspaces
├── hb/        # NEW — lightweight customer CLI
├── hebbs-python/           # EXISTING — add REST transport + cloud methods (~500 LOC)
├── hebbs-typescript/       # EXISTING — add REST transport + cloud methods (~500 LOC)
├── hebbs-skill/            # EXISTING — unchanged
└── hebbs-demos/            # EXISTING — unchanged
```
