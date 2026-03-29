# HEBBS Enterprise: Product Specification

A memory engine for AI agents — deployed on the customer's infrastructure, managed by us.

---

## Documents

| # | Document | Audience | What it answers |
|---|---|---|---|
| 01 | [Vision](01-vision.md) | Everyone | Why enterprise-first? Who is it for? |
| 02 | [Customer Journey](02-customer-journey.md) | Product, Sales, Engineering | Deploy → onboard → integrate → tune → production |
| 03 | [Architecture](03-architecture.md) | Engineering | Single-tenant on customer infra + our central dashboard |
| 04 | [Platform Services](04-platform-services.md) | Backend Engineering | Dashboard, onboarding, workspaces, API keys, config |
| 05 | [API Spec](05-api-spec.md) | Engineering | REST API reference (same as cloud) |
| 06 | [SDK Spec](06-sdk-spec.md) | SDK team | Python + TypeScript with endpoint param |
| 07 | [CLI Spec](07-cli-spec.md) | CLI team | Remote client CLI with endpoint param |
| 08 | [Tuning](08-tuning.md) | Engineering, Customer | Same tune flow as cloud |
| 09 | [Deployment](09-deployment.md) | DevOps, Customer | Docker package, docker-compose, config, operations |
| 10 | [Central Dashboard](10-central-dashboard.md) | Engineering, Ops | Our admin panel for all deployments |
| 11 | [Rollout Plan](11-rollout-plan.md) | Engineering, Leadership | Build order, ~9 weeks to first customer |
| 12 | [Dashboard Wireframes](12-dashboard-wireframes.md) | Engineering, Design | All 13 screens: login, onboarding, workspaces, search, settings, team |
| -- | [Future Tasks](FUTURE-TASKS.md) | Engineering, Product | Parked capabilities: global brain, edge linking, reflection control, etc. |

---

## Key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Engine changes | Zero | Containerize as-is. Engine is the moat. |
| Deployment model | Docker Compose on customer's machine | Self-contained, no dependency on us. |
| Embedding provider | Customer's OpenAI key | They control the cost and provider relationship. |
| LLM for extraction | Customer's OpenAI key (gpt-4o-mini default) | Same key, same control. |
| Customer-facing API | REST | Same as cloud. Universal, debuggable. |
| Monitoring | Heartbeat to our central dashboard | Health + usage metrics only. Never content. |
| Tuning | Customer's agent drives it | Same as cloud. We provide tools, they tune. |
| Platform | hebbs-platform alongside engine in Docker | Dashboard, onboarding, workspace CRUD, API keys. |

---

## Repo map

```
hebbs-repos/
├── hebbs/                  # EXISTING — engine (containerize, never modify)
├── hebbs-platform/         # NEW — dashboard, onboarding, workspaces, API keys
│                           #        + heartbeat sender to central dashboard
├── hb/                     # NEW — lightweight remote client CLI (binary: `hb`)
├── hebbs-python/           # EXISTING — add REST transport + endpoint param
├── hebbs-typescript/       # EXISTING — add REST transport + endpoint param
├── hebbs-skill/            # EXISTING — unchanged
└── hebbs-demos/            # EXISTING — unchanged
```

**New repos: 2** (`hebbs-platform`, `hb`)
**Updated repos: 2** (`hebbs-python`, `hebbs-typescript`)
**Unchanged: 3** (`hebbs`, `hebbs-skill`, `hebbs-demos`)

---

## Relationship to cloud/SaaS

The enterprise and cloud products share the same codebase:

| Component | Enterprise | Cloud (future) |
|---|---|---|
| hebbs-server (engine) | Same | Same |
| hebbs-platform | Single-tenant, no billing | Multi-tenant, billing, regions |
| hb | Same | Same |
| SDKs | Same | Same |
| Deployment | Customer's Docker | Our Kubernetes |
| Monitoring | Heartbeat → central dashboard | Built-in platform metrics |

Enterprise ships first. Cloud wraps the same platform into a multi-tenant SaaS offering later.

See [Cloud Product Spec](../../cloud/product/README.md) for the future SaaS architecture.
