# HEBBS Enterprise: Architecture

## Design principles

1. **The engine is untouched.** The open-source `hebbs` binary is containerized and deployed as-is.
2. **Customer's data stays on customer's infra.** Documents, memories, embeddings, conversations — never leave their machine.
3. **Single-tenant per deployment.** Each customer gets their own isolated HEBBS instance. No shared anything.
4. **We monitor, they own.** Our central dashboard sees health and usage metrics. Never content.

---

## System overview

```
OUR SIDE                                    CUSTOMER'S INFRASTRUCTURE
┌────────────────────────┐                  ┌─────────────────────────────────────┐
│  Central Dashboard     │                  │  HEBBS Server                       │
│  (our admin panel)     │◄── heartbeat ───│                                     │
│                        │   (health,       │  ┌─────────────────────────────────┐│
│  All deployments:      │    usage,        │  │  hebbs-platform                 ││
│  ├── Acme Corp ✓       │    version)      │  │  - Dashboard + onboarding       ││
│  ├── Beta Inc ✓        │                  │  │  - Workspace management         ││
│  └── Gamma Ltd ⚠       │                  │  │  - API key management           ││
│                        │                  │  │  - Config UI                     ││
│  Health, usage,        │                  │  │  - Port 8080                     ││
│  version, alerts       │                  │  └───────────┬─────────────────────┘│
└────────────────────────┘                  │              │                       │
                                            │              ▼                       │
                                            │  ┌─────────────────────────────────┐│
                                            │  │  hebbs-server (engine)          ││
                                            │  │  - REST API (port 6381)         ││
                                            │  │  - gRPC API (port 6380)         ││
                                            │  │  - Daemon (file watching)       ││
                                            │  │  - Memory Palace panel          ││
                                            │  │  - Indexing, recall, reflect    ││
                                            │  │  - RocksDB (persistent volume)  ││
                                            │  └─────────────────────────────────┘│
                                            │                                     │
                                            │  hebbs.customer.com                 │
                                            │  (their domain, their TLS)          │
                                            └──────────────┬──────────────────────┘
                                                           │
                                              ┌────────────┼────────────┐
                                              │            │            │
                                              ▼            ▼            ▼
                                        ┌──────────┐ ┌──────────┐ ┌──────────┐
                                        │Dev laptop│ │Agent     │ │Another   │
                                        │CLI: push │ │server    │ │team      │
                                        │sync,     │ │SDK:      │ │CLI: push │
                                        │recall    │ │recall,   │ │tune      │
                                        └──────────┘ │remember  │ └──────────┘
                                                     └──────────┘
```

---

## Components

### hebbs-server (existing engine, unchanged)

The unmodified `hebbs` binary running in server mode inside Docker.

**What it does:**
- REST API on port 6381 (all memory operations)
- gRPC API on port 6380 (for SDK clients)
- Daemon: watches uploaded files, auto-indexes on change
- Memory Palace: graph visualization, search, timeline, insights
- Indexing: parse → embed → extract propositions → build graph
- Recall: 4 strategies (similarity, temporal, causal, analogical)
- Reflection: background clustering → insight generation
- Contradiction detection
- Decay: stale memories fade

**Storage:** RocksDB on a persistent Docker volume. All data lives here.

**Config via environment:**
```
HEBBS_LLM_PROVIDER=openai
HEBBS_LLM_API_KEY=<customer's OpenAI key>
HEBBS_LLM_MODEL=gpt-4o-mini
HEBBS_EMBED_PROVIDER=openai
HEBBS_EMBED_MODEL=text-embedding-3-small
HEBBS_EMBED_API_KEY=<customer's OpenAI key>
HEBBS_EMBED_DIMENSIONS=1536
```

### hebbs-platform (new, enterprise dashboard)

Sits alongside the engine. Provides the admin/management experience that the raw engine doesn't have.

**What it does:**
- **Onboarding wizard**: first-time setup (workspace name, OpenAI key, admin account)
- **Dashboard**: workspace list, memory counts, indexing status, health
- **Workspace management**: create, delete, view details
- **API key management**: create, revoke, rotate per workspace
- **Config UI**: update LLM provider, embedding settings, decay parameters
- **Memory Palace**: link/proxy to the engine's built-in Memory Palace per workspace
- **Heartbeat**: sends health + usage metrics to our central dashboard (opt-out available)

**What it does NOT do:**
- Billing (enterprise pricing is negotiated, not metered)
- Multi-region routing (single machine)
- Multi-tenant isolation (one customer per deployment)
- User sign-up (admin account created during deployment)

**Port:** 8080 (the customer-facing entry point). Proxies API calls to hebbs-server on 6381.

### hb (new, remote client)

Lightweight CLI installed on team members' laptops/servers. Talks REST to the HEBBS server.

**Not the full engine binary.** No RocksDB, no daemon, no embedding. Just HTTP calls.

Commands: login, push, sync, recall, remember, forget, workspaces, tune, status, dashboard.

### SDKs (existing, updated)

`hebbs-python` and `hebbs-typescript` with added REST transport. Point at the customer's server endpoint.

```python
hb = Hebbs(api_key="hb_live_sk_abc123", endpoint="https://hebbs.acme.com")
```

---

## Data flow

### Recall request

```
Developer's agent server
  → POST https://hebbs.acme.com/v1/recall {"cue": "password reset", "entity_id": "user_42"}
  → hebbs-platform (port 8080): validates API key, proxies to engine
  → hebbs-server (port 6381):
      1. Embed query via OpenAI API (outbound from customer's machine)
      2. HNSW search in RocksDB (local)
      3. Composite scoring
      4. Return results
  → Response flows back to agent
```

### File push from CLI

```
Developer's laptop
  → hb push ./docs
  → POST https://hebbs.acme.com/v1/upload (multipart, file bytes)
  → hebbs-platform: validates auth, writes files to engine's volume
  → hebbs-server daemon: detects new files, indexes in background
```

### Heartbeat to central dashboard

```
hebbs-platform on customer's machine
  → Every 5 minutes, POST https://central.hebbs.ai/heartbeat
  → Payload:
      {
        "deployment_id": "acme-corp-001",
        "version": "0.3.3",
        "status": "healthy",
        "workspaces": 3,
        "total_memories": 12450,
        "total_files": 156,
        "uptime_hours": 720,
        "last_recall": "2026-03-28T14:30:00Z"
      }
  → NO content data. Just counts and health.
```

---

## What's on the customer's machine

```
/opt/hebbs/                          (or wherever they choose)
├── docker-compose.yml               # Orchestrates everything
├── .env                             # Customer's config (OpenAI key, domain, etc.)
├── data/                            # Persistent volume (mounted into containers)
│   ├── docs/                        # Uploaded files land here
│   └── .hebbs/                      # RocksDB, indexes, vault data
└── tls/                             # Optional: customer's TLS certs
    ├── cert.pem
    └── key.pem
```

Everything in `data/` persists across container restarts and upgrades.

---

## Security

### Network
- Platform exposed on port 8080 (HTTP/HTTPS depending on TLS config)
- Engine ports (6380, 6381) internal to Docker network — not exposed externally
- All external access goes through hebbs-platform
- Heartbeat to central dashboard is outbound HTTPS only — no inbound from us

### Authentication
- API keys hashed (SHA-256) in platform's local DB
- Admin account created during deployment (username + password)
- Dashboard access via session (JWT)
- API access via API key in Authorization header

### Data isolation
- Single-tenant: one customer per HEBBS instance
- All data on customer's machine, in their Docker volume
- OpenAI API calls originate from customer's machine (their network, their IP)
- Our central dashboard never receives content — only health metrics

### Upgrades
- We push a new Docker image tag
- Customer runs `docker compose pull && docker compose up -d`
- RocksDB recovers from WAL on restart — no data loss
- Or we SSH in and do it during a maintenance window
