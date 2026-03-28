# HEBBS Enterprise: Central Dashboard

## Overview

The central dashboard is our admin panel. We run it. It shows the health and status of every enterprise deployment across all customers. It never sees customer data — only metrics.

---

## What it shows

```
HEBBS Central Dashboard

┌──────────────────────────────────────────────────────────────────────┐
│  Deployments                                                 3 total │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │ ✓ Acme Corp                                                    │  │
│  │   Endpoint:    hebbs.acme.com                                  │  │
│  │   Version:     0.3.3 (latest)                                  │  │
│  │   Status:      healthy                                         │  │
│  │   Uptime:      30 days                                         │  │
│  │   Workspaces:  3                                               │  │
│  │   Memories:    12,450                                          │  │
│  │   Files:       156                                             │  │
│  │   Last recall: 2 minutes ago                                   │  │
│  │   Last heartbeat: 1 minute ago                                 │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │ ✓ Beta Inc                                                     │  │
│  │   Endpoint:    hebbs.beta.io                                   │  │
│  │   Version:     0.3.3 (latest)                                  │  │
│  │   Status:      healthy                                         │  │
│  │   Workspaces:  1                                               │  │
│  │   Memories:    2,100                                           │  │
│  │   Last recall: 1 hour ago                                      │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │ ⚠ Gamma Ltd                                                    │  │
│  │   Endpoint:    hebbs.gamma.com                                 │  │
│  │   Version:     0.3.1 (outdated — 0.3.3 available)              │  │
│  │   Status:      healthy                                         │  │
│  │   Workspaces:  2                                               │  │
│  │   Memories:    8,300                                           │  │
│  │   Last recall: 3 days ago                                      │  │
│  │   ⚠ Version outdated    ⚠ Low activity                        │  │
│  └────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Data we receive (heartbeat payload)

Every enterprise deployment sends a heartbeat every 5 minutes:

```json
{
  "deployment_id": "acme-corp-001",
  "version": "0.3.3",
  "status": "healthy",
  "uptime_seconds": 2592000,
  "workspaces": 3,
  "total_memories": 12450,
  "total_files": 156,
  "recalls_24h": 4230,
  "remembers_24h": 890,
  "storage_bytes": 2576980377,
  "engine_healthy": true,
  "openai_connected": true,
  "last_recall_at": "2026-03-28T14:28:00Z",
  "last_index_at": "2026-03-28T14:30:00Z"
}
```

**What we NEVER receive:**
- Document content
- Memory content
- Entity IDs
- Query text
- User data
- API keys
- OpenAI keys

Only counts, timestamps, and health booleans.

---

## Features

### Deployment list

All customer deployments at a glance. Sorted by status (unhealthy first, then warnings, then healthy).

### Alerts

| Alert | Trigger | Action |
|---|---|---|
| Deployment down | No heartbeat for 15 minutes | Notify us (email/Slack) |
| Version outdated | Running < latest release for 7+ days | Show warning badge |
| Low activity | No recalls in 7+ days | Show warning badge |
| Engine unhealthy | `engine_healthy: false` in heartbeat | Notify us immediately |
| OpenAI disconnected | `openai_connected: false` | Notify us + customer |
| Storage high | `storage_bytes` > 80% of estimated capacity | Show warning badge |

### Deployment detail

Click into a deployment to see:
- Version history (when they upgraded)
- Workspace count over time (chart)
- Memory count over time (chart)
- Recall volume over time (chart)
- Uptime history
- Alert history

### Push updates

When a new engine version is released:
1. We see which deployments are outdated
2. We schedule upgrades with the customer
3. Customer runs `docker compose pull && docker compose up -d`
4. Central dashboard confirms the version updated

---

## Architecture

### Our side

```
central.hebbs.ai
├── Central dashboard (web app)
│   ├── Deployment list
│   ├── Deployment detail
│   ├── Alert configuration
│   └── Version management
└── Heartbeat receiver
    ├── POST /heartbeat (receives from customer deployments)
    ├── Postgres (stores heartbeat history)
    └── Alert engine (checks thresholds, sends notifications)
```

### Hosting

Simple. The central dashboard is:
- A small web app (React + API)
- A Postgres database (deployment metadata + heartbeat history)
- Hosted anywhere (a single VPS, a small AWS setup, Vercel + managed DB)

This is NOT the heavy multi-region SaaS platform from the cloud docs. It's a lightweight admin panel. Could run on a $20/mo VPS.

### Heartbeat flow

```
Customer's machine (hebbs-platform)
  → Every 5 min: POST https://central.hebbs.ai/heartbeat
  → Payload: deployment_id + health metrics (no content)
  → Central dashboard stores in Postgres
  → Alert engine checks thresholds
  → If alert: notify us via email/Slack
```

### If customer opts out of heartbeat

They set `HEBBS_HEARTBEAT_ENABLED=false` in `.env`. We lose visibility. The deployment disappears from our dashboard. We rely on manual check-ins.

---

## Security

- Heartbeat is outbound HTTPS only from customer's machine — we never connect inbound
- Deployment ID is the only identifier (no customer PII in heartbeat)
- Heartbeat payload is fixed schema — no arbitrary data
- Central dashboard has its own auth (our team only, not customer-facing)
- Postgres encrypted at rest, standard backup practices
