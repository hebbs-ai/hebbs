# HEBBS Enterprise: Platform Services

## Overview

The enterprise platform is a simplified version of the SaaS platform. No billing, no multi-region, no tenant routing. It handles: onboarding, workspace management, API keys, config, and health reporting.

Runs alongside the engine in the same Docker deployment on the customer's machine.

---

## Onboarding wizard

First time the customer opens the dashboard after deployment.

### Flow

```
Step 1: Create admin account
  Email: [admin@acme.com]
  Password: [********]

Step 2: Name your first workspace
  [support-agent]

Step 3: Configure OpenAI
  API key: [sk-proj-...]    (may already be set via .env during deployment)
  Model for extraction: gpt-4o-mini (default)
  Model for embedding: text-embedding-3-small (default)

  [Test connection] → ✓ Connected

Step 4: Done
  Workspace: support-agent
  API key: hb_live_sk_abc123...
  Endpoint: https://hebbs.acme.com

  Next: push your docs
    brew install hebbs-ai/tap/hb
    hb login --endpoint https://hebbs.acme.com
    hb push ./docs
```

---

## Dashboard

After onboarding, the main dashboard shows:

### Home

```
HEBBS — Acme Corp

Workspaces:
  NAME             MEMORIES    FILES    STATUS      LAST ACTIVITY
  support-agent    3,152       34       active      2 minutes ago
  sales-agent      812         22       active      1 hour ago
  internal-kb      245         8        indexing    now (5/8 files)

System:
  Status:     healthy
  Version:    0.3.3
  Uptime:     30 days
  OpenAI:     connected (gpt-4o-mini + text-embedding-3-small)
  Storage:    2.4 GB used
```

### Workspace detail

Click into a workspace:

```
support-agent

Stats:
  Memories:        3,152 (812 from files, 2,340 from conversations)
  Files:           34 (all indexed)
  Entities:        47 (user_42: 87, billing: 203, ...)
  Insights:        12 auto-generated
  Contradictions:  3 flagged

Indexing:
  Status:          complete
  Last indexed:    2026-03-28 14:30 UTC
  Files changed:   0 pending

Tune:
  Last run:        2026-03-25
  Baseline:        58%
  Current:         85% (+27pp)
  Rules stored:    8

Memory Palace:     [Open →]
API keys:          [Manage →]
```

### Memory Palace (per workspace)

Links to the existing Memory Palace UI built into the engine. Shows:
- Graph visualization (memories as nodes, edges as relationships)
- Search across all recall strategies
- Timeline view
- Entity browser
- Insight list
- Contradiction list (red dashed edges)

The platform proxies to the engine's built-in panel at `/api/panel/`.

---

## Workspace management

### Create

Via dashboard: "New workspace" button → name it → API key generated.

Via API:
```
POST /v1/workspaces
{"name": "sales-agent"}
```

Response:
```json
{
  "workspace": {
    "name": "sales-agent",
    "slug": "sales-agent",
    "status": "active",
    "created_at": "2026-03-28T12:00:00Z"
  },
  "api_key": "hb_live_sk_sales_789..."
}
```

Behind the scenes: creates a new vault in the engine (`hebbs init`). Each workspace is a separate vault with isolated storage.

### Delete

Via dashboard: "Delete workspace" → confirm → vault deleted, volume data wiped.

### List

```
GET /v1/workspaces
```

---

## API key management

### Key types

| Type | Scope | Use case |
|---|---|---|
| Workspace key (`hb_live_sk_`) | Single workspace | Agent integration, developer access |
| Admin key (`hb_admin_sk_`) | All workspaces + management | Admin tooling, CI/CD |

### Operations

Via dashboard or API:
- Create key (name, workspace scope)
- Revoke key (immediate)
- Rotate key (24h grace period for old key)
- List keys (shows prefix + name, never full key)

---

## Config

Via dashboard "Settings" page:

### LLM configuration

```
Provider:     openai
Model:        gpt-4o-mini
API key:      sk-proj-****... [Update]

Embedding:
  Provider:   openai
  Model:      text-embedding-3-small
  Dimensions: 1536
```

### System settings

```
Decay half-life:            30 days (default)
Reflection interval:        1 hour (default)
Contradiction detection:    enabled (automatic when LLM key configured)
```

### OpenAI / API settings

```
Max concurrent requests:    10 (default)
```

This is a **global setting** — applies to all workspaces since they share the same engine instance and OpenAI key. Controls how many simultaneous OpenAI API calls the engine makes (for embedding + extraction).

**Why it matters:** If the customer is on a low-tier OpenAI account, too many concurrent calls cause rate limit errors (429s from OpenAI), which fail embedding → fail recall/indexing. Lower this to 2-3 for new/low-tier accounts.

**Symptoms of too-high concurrency:**
- Recall returns errors intermittently
- Indexing stalls or reports failures
- OpenAI dashboard shows rate limit hits

**Guidance:**

| OpenAI tier | Recommended setting |
|---|---|
| Free / Tier 1 | 2 |
| Tier 2 | 5 |
| Tier 3+ | 10-20 |
| Enterprise | 20-50 |

Can also be set in `.env`:
```
HEBBS_MAX_CONCURRENT=5
```

### Heartbeat

```
Central dashboard:      enabled
Deployment ID:          acme-corp-001
Last heartbeat:         2 minutes ago
[Disable heartbeat]
```

The customer can opt out of sending health metrics to our central dashboard. When disabled, we lose visibility into their deployment.

---

## Team / roles

Simple role model. No invitations or OAuth — admin creates accounts directly.

### Roles

| Role | Dashboard | Workspaces | API keys | Config |
|---|---|---|---|---|
| Admin | Full access | Create/delete/view all | Create/revoke | Full access |
| Developer | View only | View assigned | Use (not create) | No |

### Account management

Admin creates developer accounts via dashboard:
```
Add team member:
  Username: [alice]
  Password: [********]
  Role: [Developer]
  Workspaces: [support-agent, sales-agent]
```

No email, no OAuth, no SSO. Just username + password stored locally. Enterprise customers who need SSO can request it — future enhancement.

---

## Internal storage

The platform uses a lightweight local database (SQLite or embedded) for:
- Admin accounts + sessions
- API key hashes
- Workspace metadata
- Config state
- Heartbeat history

This is separate from the engine's RocksDB. Both live on the persistent Docker volume.

---

## Platform API

All endpoints are served by hebbs-platform on port 8080.

### Management endpoints (admin only)

```
POST   /v1/workspaces              → create workspace
GET    /v1/workspaces              → list workspaces
GET    /v1/workspaces/:slug        → workspace detail
DELETE /v1/workspaces/:slug        → delete workspace

POST   /v1/keys                    → create API key
GET    /v1/keys                    → list keys
DELETE /v1/keys/:prefix            → revoke key

GET    /v1/config                  → get current config
PUT    /v1/config                  → update config

POST   /v1/accounts                → create team member
GET    /v1/accounts                → list team members
DELETE /v1/accounts/:username      → remove team member

GET    /v1/system/health           → system health
GET    /v1/system/version          → version info
```

### Data plane endpoints (proxied to engine)

```
POST   /v1/memories                → remember
POST   /v1/recall                  → recall
POST   /v1/forget                  → forget
GET    /v1/memories                → list memories
GET    /v1/memories/:id            → get memory
PUT    /v1/memories/:id            → revise
GET    /v1/insights                → query insights
GET    /v1/entities                → list entities
POST   /v1/upload                  → upload files
GET    /v1/upload/status/:batch    → upload/index status
GET    /v1/health                  → workspace health (includes indexing status)
```

These are proxied directly to hebbs-server on port 6381. The platform adds API key validation, then forwards.
