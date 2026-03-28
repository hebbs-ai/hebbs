# HEBBS Cloud: Platform Services

## Overview

Platform services handle everything that ISN'T memory — authentication, billing, workspace lifecycle, team management, and usage tracking. They run in the central region and share a Postgres database.

The hebbs engine handles memory. The platform handles everything around it.

---

## Auth Service

### Sign-up flows

**GitHub OAuth (primary):**
1. Customer clicks "Sign up with GitHub"
2. OAuth flow → we get GitHub user ID, email, org membership
3. Create user record
4. Auto-create org (from GitHub org name, or personal username)
5. Prompt: "Name your first workspace" (required — no auto-created "default")
6. Region selection (US default, changeable)
7. Generate first API key
8. Redirect to console with onboarding

**Email + password (fallback):**
1. Customer enters email + password
2. Email verification link sent
3. On verification: create user, org
4. Prompt for workspace name + region
5. Generate API key
6. Redirect to console

### API key management

**Key format:**
```
hb_live_sk_<32 random bytes, base62 encoded>
hb_test_sk_<32 random bytes, base62 encoded>
```

**Key types:**

| Type | Prefix | Scope | Use case |
|---|---|---|---|
| Workspace key | `hb_live_sk_` | Single workspace | Production agent integration |
| Org key | `hb_live_ok_` | All workspaces in org | Multi-workspace agents, admin tooling |
| Test key | `hb_test_sk_` | Sandbox workspace | Development, CI/CD |

**Key lifecycle:**
- Created: via console or CLI
- Active: accepting requests
- Revoked: immediately stops accepting requests
- Rotated: new key created, old key has a grace period (24h) before revocation

**Storage:**
- Only the SHA-256 hash is stored in the platform DB
- The key prefix (`hb_live_sk_`) is stored in plaintext for identification
- The full key is shown exactly once at creation time

### Team management

**Roles:**

| Role | Org management | Workspace management | Data operations | Billing |
|---|---|---|---|---|
| Owner | Yes | Yes | Yes | Yes |
| Admin | Invite/remove members | Create/delete workspaces, manage keys | Yes | View only |
| Developer | No | No | Push, recall, remember, dashboard | No |

**Invitations:**
- Admin or Owner invites by email
- Invitee gets email with link to accept
- If invitee has no account, sign-up flow starts first
- Invitation expires after 7 days

### Session management

- Console access uses JWT (24h access token, 30d refresh token)
- CLI uses a long-lived token stored in `~/.hb/config` (revocable from console)
- API keys are used for SDK/agent access (no expiry, must be manually revoked)

---

## Billing Service

### Plans

| | Free | Pro | Enterprise |
|---|---|---|---|
| Price | $0 | $49/mo | Custom |
| Workspaces | 1 | 10 | Unlimited |
| Memories | 5,000 | 100,000 | Unlimited |
| Recalls/month | 10,000 | 1,000,000 | Unlimited |
| File storage | 50MB | 5GB | Unlimited |
| Team members | 1 | 10 | Unlimited |
| Auto-tune | No | Yes | Yes |
| Regions | US only | US + EU | All |
| Support | Community | Email | Dedicated |
| Data export | No | Yes | Yes |
| SSO/SAML | No | No | Yes |

### Usage metering

Events are emitted by regional gateways and collected centrally.

**Metered dimensions:**

| Dimension | Unit | How counted |
|---|---|---|
| Memories stored | count | Current total in workspace (gauge, not cumulative) |
| Recall calls | count | Per API call |
| Remember calls | count | Per API call |
| Files uploaded | bytes | Cumulative per billing period |
| LLM tokens consumed | tokens | Embedding + extraction (our cost, not billed directly) |

**Metering pipeline:**

```
Regional gateway
  → emits usage events (batched, every 60s)
  → async HTTP POST to central platform
  → platform writes to usage_events table
  → billing service aggregates hourly
  → Stripe usage records updated daily
```

**Quota enforcement:**
- Memory limit: `remember()` returns 402 when workspace hits limit
- Recall rate limit: returns 429 when monthly limit hit
- Workspace limit: workspace creation returns 402 when org hits plan limit
- File storage: upload returns 402 when cumulative storage exceeds plan

### Stripe integration

- Each org maps to a Stripe Customer
- Each plan maps to a Stripe Price
- Subscription created on plan upgrade
- Usage-based billing reported via Stripe Metered Billing (for overages on Enterprise)
- Webhook handles: payment success, payment failure, subscription cancelled

### Upgrade/downgrade

- Upgrade: immediate, prorated
- Downgrade: effective at next billing period
- Downgrade with over-limit resources: workspace access becomes read-only until under limits (no new memories, recalls still work)

---

## Workspace Manager

### Workspace lifecycle

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ provisioning │────►│    active    │────►│  suspended   │
└──────────────┘     └──────┬───────┘     └──────┬───────┘
                            │                     │
                            │                     │ API call received
                            │                     ▼
                            │              ┌──────────────┐
                            │              │   resuming   │──► active
                            │              └──────────────┘
                            │
                            ▼
                     ┌──────────────┐
                     │   deleted    │──► volume wiped after 30d
                     └──────────────┘
```

**States:**

| State | Data plane | Management | Trigger |
|---|---|---|---|
| Provisioning | Unavailable | Visible | Workspace created |
| Active | Full access | Full access | Provisioning complete |
| Suspended | Read-only (recall works, remember blocked) | Full access | 30d inactivity (free) or payment failure |
| Resuming | Unavailable (~2s) | Full access | API call to suspended workspace |
| Deleted | Unavailable | Removed | Owner deletes workspace |

### Provisioning flow

When a workspace is created:

1. **Platform** validates: org has quota, region exists and is active
2. **Platform** creates workspace record in Postgres (status: provisioning)
3. **Platform** generates tenant_id (ULID)
4. **Platform** calls regional gateway: `POST /internal/provision`

```json
{
  "tenant_id": "01JAB123...",
  "config": {
    "llm_provider": "openai",
    "llm_model": "gpt-4o-mini",
    "llm_api_key": "<our key>",
    "embed_provider": "openai",
    "embed_model": "text-embedding-3-small",
    "embed_dimensions": 1536
  },
  "resources": {
    "cpu": "500m",
    "memory": "512Mi",
    "disk": "1Gi"
  }
}
```

5. **Regional gateway** creates Kubernetes resources:
   - PersistentVolumeClaim (for RocksDB + uploaded files)
   - Deployment (hebbs-server container with config)
   - Service (ClusterIP, internal only)

6. **Regional gateway** waits for readiness probe (`/v1/health/ready`)
7. **Regional gateway** returns container endpoint to platform
8. **Platform** updates workspace record (status: active, endpoint stored)
9. **Platform** runs `hebbs init` inside the container (via exec or init container)

### Suspension

Free tier workspaces with no API calls for 30 days:
- Container is stopped (Deployment scaled to 0)
- Volume is preserved
- Status set to suspended
- On next API call: regional gateway scales Deployment to 1, waits for ready, resumes

This keeps infrastructure costs low for inactive free-tier users.

### Deletion

When workspace is deleted:
- Container stopped immediately
- Volume marked for deletion (30-day retention for accidental deletes)
- After 30 days: volume wiped, irrecoverable
- Workspace record soft-deleted in platform DB

### Global brain mapping

Each org gets an implicit "global" workspace for cross-workspace knowledge:
- Auto-created when org is created
- Accessible from any workspace via `global=true` parameter
- Same provisioning as regular workspace
- Not counted against workspace quota

```python
# Store cross-workspace knowledge
hb.remember("Company was founded in 2019", global=True)

# Recall from current workspace + global
memories = hb.recall("founding", include_global=True)
```

---

## Internal API (platform ↔ regional gateway)

These endpoints are NOT exposed to customers. Used for orchestration between central platform and regional gateways.

```
# Workspace lifecycle
POST   /internal/provision              → create container + volume
POST   /internal/suspend/:workspace     → scale to 0
POST   /internal/resume/:workspace      → scale to 1
DELETE /internal/teardown/:workspace    → stop + delete

# Health
GET    /internal/workspaces             → list all workspaces in region with status
GET    /internal/workspaces/:id/health  → health of specific workspace

# Usage
POST   /internal/usage/flush            → force flush usage events to central

# File management
POST   /internal/workspaces/:id/upload  → write file to workspace volume
```
