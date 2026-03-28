# HEBBS Cloud: Architecture

## Design principles

1. **The engine is untouched.** The open-source `hebbs` binary is containerized and deployed as-is. Zero modifications.
2. **Data stays in-region.** Document content, memories, embeddings, and conversation data never leave the customer's chosen region. Only metadata flows to central.
3. **Workspace isolation by default.** Each workspace gets its own hebbs-server container with its own RocksDB volume. No shared storage, no shared indexes.
4. **Central controls, regional executes.** Auth, billing, and workspace management are centralized. Data processing and storage are regional.

---

## System overview

```
                        CUSTOMER SIDE
                  ┌───────────────────────┐
                  │  hb CLI      │
                  │  Python/TS SDK        │
                  │  Their agent code     │
                  └──────────┬────────────┘
                             │ HTTPS
                             ▼
─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
                        HEBBS CLOUD
                             │
                             ▼
┌────────────────────────────────────────────────────────────┐
│                    CENTRAL (US)                             │
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              Global Gateway                           │  │
│  │              api.hebbs.ai                             │  │
│  │                                                       │  │
│  │  • TLS termination                                    │  │
│  │  • API key authentication                             │  │
│  │  • Workspace → region lookup (cached)                 │  │
│  │  • Forward to regional gateway                        │  │
│  │  • Management API (orgs, workspaces, keys)            │  │
│  └──────────────┬──────────────────┬─────────────────────┘  │
│                 │                  │                         │
│  ┌──────────────┴──────────────────┴─────────────────────┐  │
│  │              Platform Services                         │  │
│  │                                                        │  │
│  │  Auth          Billing         Workspace Manager       │  │
│  │  • OAuth       • Stripe        • CRUD                  │  │
│  │  • API keys    • Plans         • Provisioning          │  │
│  │  • Teams       • Usage agg     • Region assignment     │  │
│  │  • Roles       • Invoices      • Quota enforcement     │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                            │
│  ┌────────────────────────────────────────────────────────┐  │
│  │              Platform Database (Postgres)               │  │
│  │                                                        │  │
│  │  orgs, members, workspaces, api_keys, plans,           │  │
│  │  usage_events, region_registry, tune_schedules         │  │
│  └────────────────────────────────────────────────────────┘  │
└────────────────────────┬──────────────────┬─────────────────┘
                         │                  │
                         ▼                  ▼
┌────────────────────────────┐  ┌────────────────────────────┐
│       REGION: US           │  │       REGION: EU           │
│       us.hebbs.ai          │  │       eu.hebbs.ai          │
│                            │  │                            │
│  ┌──────────────────────┐  │  │  ┌──────────────────────┐  │
│  │  Regional Gateway    │  │  │  │  Regional Gateway    │  │
│  │  • Route to workspace│  │  │  │  • Route to workspace│  │
│  │  • File upload       │  │  │  │  • File upload       │  │
│  │  • Usage metering    │  │  │  │  • Usage metering    │  │
│  │  • Health checks     │  │  │  │  • Health checks     │  │
│  └──────┬───────────────┘  │  │  └──────┬───────────────┘  │
│         │                  │  │         │                  │
│   ┌─────┴─────┐            │  │   ┌─────┴─────┐            │
│   │           │            │  │   │           │            │
│   ▼           ▼            │  │   ▼           ▼            │
│ ┌────────┐ ┌────────┐     │  │ ┌────────┐ ┌────────┐     │
│ │support │ │internal│ ... │  │ │ sales  │ │support │ ... │
│ │-agent  │ │-kb     │     │  │ │-agent  │ │-eu     │     │
│ │ hebbs  │ │ hebbs  │     │  │ │ hebbs  │ │ hebbs  │     │
│ │ server │ │ server │     │  │ │ server │ │ server │     │
│ │RocksDB │ │RocksDB │     │  │ │RocksDB │ │RocksDB │     │
│ │ volume │ │ volume │     │  │ │ volume │ │ volume │     │
│ └────────┘ └────────┘     │  │ └────────┘ └────────┘     │
│                            │  │                            │
│                            │  │                            │
└────────────────────────────┘  └────────────────────────────┘
```

---

## Component details

### Global Gateway (`api.hebbs.ai`)

Single entry point for all customer requests. Deployed in the central region (US).

**Responsibilities:**
- TLS termination
- API key validation (cache hot keys in memory, TTL 60s)
- Resolve API key → workspace → region (from platform DB, cached)
- Forward data-plane requests to the correct regional gateway
- Serve management-plane requests directly (org/workspace/key CRUD)

**Request classification:**

| Request type | Handled by | Example |
|---|---|---|
| Management plane | Global gateway + platform services | Create workspace, list keys, get usage |
| Data plane | Forwarded to regional gateway | recall, remember, upload, forget |

**Latency budget:** <5ms for routing decisions (cache hit). Data-plane requests add one network hop to the region.

**Optimization:** Customers can call regional endpoints directly (`us.api.hebbs.ai`, `eu.api.hebbs.ai`) to skip the global hop. The SDK supports this via the `region` parameter.

### Platform Services

Stateless services deployed in the central region. Share a Postgres database.

**Auth service:**
- GitHub OAuth + email/password sign-up
- Session management (JWT, 24h expiry)
- API key generation (prefixed: `hb_live_sk_` for production, `hb_test_sk_` for sandbox)
- API key scoping: per-workspace or per-org
- Team management: invite, remove, role assignment

**Billing service:**
- Stripe integration for subscription management
- Plan enforcement (workspace count, memory limits, recall rate limits)
- Usage aggregation: collects metered events from all regional gateways
- Invoice generation

**Workspace manager:**
- CRUD for workspaces within an org
- Region assignment at creation time (immutable after creation)
- Provisioning: calls regional gateway to spin up workspace container
- De-provisioning: calls regional gateway to tear down + delete volume
- Quota tracking: memory count, file count, recall count per billing period

### Platform Database (Postgres)

Central, single-region Postgres instance. Contains only metadata — never customer content.

**Key tables:**

```
orgs
  id, name, slug, plan_id, created_at

members
  id, org_id, user_id, role (owner|admin|developer), invited_at

workspaces
  id, org_id, name, slug, region, container_id, status (provisioning|active|suspended|deleted), created_at

api_keys
  id, org_id, workspace_id (nullable — null means org-scoped), key_hash, prefix, name, created_at, revoked_at

usage_events
  id, workspace_id, region, event_type (recall|remember|upload|reflect), count, bytes, timestamp

plans
  id, name, max_workspaces, max_memories, max_recalls_per_month, price_cents

region_registry
  id, name (us|eu), gateway_endpoint, status (active|draining|offline)

tune_schedules
  id, workspace_id, last_run, next_run, baseline_score, current_score
```

### Regional Gateway

Deployed once per region. Handles data-plane requests for all workspaces in that region.

**Responsibilities:**
- Route requests to the correct workspace container (workspace_id → container endpoint)
- File upload: receive files via multipart HTTP, write to workspace's volume mount
- Usage metering: emit events to central platform (async, batched)
- Health monitoring: check workspace containers, restart if unhealthy
- Workspace lifecycle: provision (start container + volume) and de-provision (stop + delete)

**Does NOT handle:**
- Authentication (already done by global gateway)
- Billing logic
- Workspace CRUD

### Workspace Container (hebbs-server)

The unmodified `hebbs` binary running in server mode. One container per workspace.

**Configuration (injected via environment):**

```
HEBBS_LLM_PROVIDER=openai
HEBBS_LLM_MODEL=gpt-4o-mini
HEBBS_LLM_API_KEY=<our OpenAI key>
HEBBS_EMBED_PROVIDER=openai
HEBBS_EMBED_MODEL=text-embedding-3-small
HEBBS_EMBED_API_KEY=<our OpenAI key>
HEBBS_EMBED_DIMENSIONS=1536
HEBBS_BIND_REST=0.0.0.0:6381
HEBBS_BIND_GRPC=0.0.0.0:6380
```

**Volume mount:**
- `/data/` → persistent volume for RocksDB + uploaded files
- `/data/docs/` → where uploaded files land (daemon watches this)
- `/data/.hebbs/` → vault directory (RocksDB, config, indexes)

**Resource limits (per container, initial defaults):**
- CPU: 0.5 vCPU
- Memory: 512MB (RocksDB is memory-mapped, grows with vault size)
- Disk: 1GB (scales with plan)

**Lifecycle:**
- Provisioned when workspace is created
- Suspended after 30 days of inactivity (free tier) — volume preserved, container stopped
- Resumed on next API call (cold start ~2s)
- Deleted when workspace is deleted — volume wiped

### Tune Runner

A cron job running in each region. Iterates over active workspaces and runs automated tuning.

Tuning is user-driven at launch — the customer's agent calls tune commands via the CLI/SDK. No platform-side tune service needed. **Covered in detail in [08-tuning.md](08-tuning.md).**

---

## Data flow diagrams

### Recall request

```
Customer agent
  → POST api.hebbs.ai/v1/recall {"cue": "password reset", "entity_id": "user_42"}
  → Global gateway: validate API key, resolve region=us
  → Forward to us.hebbs.ai/v1/recall
  → Regional gateway: route to workspace container (support-agent)
  → hebbs-server (support-agent):
      1. Embed "password reset" via OpenAI (from US region)
      2. HNSW search in RocksDB (local, <5ms)
      3. Composite scoring (relevance + recency + importance + reinforcement)
      4. Return top-k memories
  → Response flows back through gateways to customer
```

**Total latency budget:** ~50-100ms (20ms embedding API + 5ms search + 25-75ms network)

### File upload

```
Customer
  → POST api.hebbs.ai/v1/upload (multipart, file bytes)
  → Global gateway: validate, resolve region=eu
  → Forward to eu.hebbs.ai/v1/upload
  → Regional gateway:
      1. Write file to workspace volume at /data/docs/filename.md
      2. Return 202 Accepted
  → hebbs-server (sales-agent) daemon:
      1. Detects new file via file watcher
      2. Parses, chunks by headings
      3. Embeds via OpenAI (from EU region)
      4. Extracts propositions + entities via gpt-4o-mini (from EU region)
      5. Stores in RocksDB
  → Done (async, customer doesn't wait for indexing)
```

### Workspace provisioning

```
Customer
  → POST api.hebbs.ai/v1/workspaces {"name": "sales-agent", "region": "eu"}
  → Global gateway: validate, check plan quota
  → Platform services:
      1. Create workspace record in Postgres
      2. Generate API key, store hash
      3. Call eu.hebbs.ai/provision {"workspace": "sales-agent", "config": {...}}
  → Regional gateway (EU):
      1. Create persistent volume
      2. Start hebbs-server container with config
      3. Wait for health check to pass
      4. Return endpoint
  → Platform stores: workspace sales-agent → eu → container endpoint
  → Return API key to customer
```

---

## Failure modes and recovery

| Failure | Impact | Recovery |
|---|---|---|
| Workspace container crashes | One workspace down | Regional gateway detects via health check, restarts container. RocksDB recovers from WAL. |
| Regional gateway down | All workspaces in region unreachable | Kubernetes restarts. Global gateway returns 503 with region status. |
| Global gateway down | All requests fail | Load balancer failover to standby instance. |
| Platform DB down | No new sign-ups, no workspace creation. Existing data-plane requests still work (API key cache). | Postgres standby promotion. |
| OpenAI API down | Embedding and extraction fail | Recall of existing memories still works (pre-computed embeddings). New remember/index calls queue and retry. |
| Volume corruption | One workspace loses data | Restore from volume snapshot (daily backup). |

---

## Security

### Network

- All external traffic over TLS 1.3
- Workspace containers not exposed to internet — only reachable via regional gateway
- Regional gateways only accept requests from global gateway (IP allowlist) or direct regional calls (with API key)

### Authentication

- API keys are hashed (SHA-256) in the platform DB — plaintext never stored
- Keys are scoped: workspace-level (can only access one workspace) or org-level (can access all workspaces in org)
- JWT sessions for console access (24h expiry, refresh tokens)

### Workspace isolation

- Each workspace gets its own container and volume
- No shared RocksDB instances
- No shared network namespace
- Container runs as non-root user
- Volume is encrypted at rest (cloud provider KMS)

### Data handling

- Customer data (documents, memories, embeddings) encrypted at rest and in transit
- Platform DB contains only metadata (names, IDs, usage counts)
- API keys rotatable without downtime
- Workspace deletion wipes volume — no tombstones, no recovery after 30 days

---

## Scalability

### Horizontal

- Global gateway: stateless, scale by adding instances behind load balancer
- Regional gateway: stateless, scale per region
- Platform services: stateless, scale by adding instances
- Workspace containers: scale by adding nodes to regional k8s cluster

### Vertical

- Workspace containers: adjust CPU/memory limits based on workspace size
- Platform DB: scale Postgres vertically (or read replicas for usage queries)

### Limits at launch

| Metric | Free | Pro | Enterprise |
|---|---|---|---|
| Workspaces | 1 | 10 | Unlimited |
| Memories per workspace | 5,000 | 100,000 | Unlimited |
| Recalls per month | 10,000 | 1,000,000 | Unlimited |
| File upload size | 10MB | 100MB | 1GB |
| Team members | 1 | 10 | Unlimited |
