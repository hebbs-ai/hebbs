# HEBBS Enterprise: Rollout Plan

## Build order

Each phase delivers something usable. The goal is to go from "engine exists" to "deploying at first enterprise customer" as fast as possible.

---

## Phase 0: Containerize the engine (Week 1)

**Goal:** The existing `hebbs` binary runs correctly in Docker with OpenAI config via env vars.

### Tasks

- [ ] Write Dockerfile for hebbs-server (no ONNX, OpenAI only)
- [ ] Verify engine runs in container: init vault, index files, recall, dashboard
- [ ] Verify file upload to volume + daemon auto-indexing works
- [ ] Verify Memory Palace panel accessible from outside container
- [ ] Push image to container registry (ghcr.io/hebbs-ai/hebbs-server)

### Deliverable

A Docker image of the engine that works identically to a local install.

### Validation

Push the `enterprise-legal-mini` demo vault, run recall queries, confirm same quality as local.

---

## Phase 1: Enterprise platform (Week 2-4)

**Goal:** The dashboard, onboarding, workspace management, API keys — everything the customer sees on top of the engine.

### Tasks

- [ ] Create `hebbs-platform` repo
- [ ] Build onboarding wizard (create admin, name workspace, configure OpenAI, test connection)
- [ ] Build dashboard home (workspace list, health, stats)
- [ ] Build workspace detail (memory counts, file list, indexing status, Memory Palace link)
- [ ] Build workspace CRUD (create, delete)
- [ ] Build API key management (create, revoke, rotate)
- [ ] Build config UI (LLM provider, embedding, decay, reflection settings)
- [ ] Build team/account management (admin creates developer accounts, roles)
- [ ] Build API proxy (validate API key → forward to engine on port 6381)
- [ ] Build file upload endpoint (receive files → write to engine's volume)
- [ ] Build heartbeat sender (POST health metrics to central.hebbs.ai every 5 min)
- [ ] Dockerize platform, add to docker-compose.yml
- [ ] Write .env.example with all config options

### Deliverable

`docker compose up` gives you the full HEBBS experience: dashboard, onboarding, workspaces, API keys, Memory Palace, file upload, recall, everything.

### Validation

Deploy on a fresh VM. Run through the complete customer journey from 02-customer-journey.md. Start to first recall in under 1 hour.

---

## Phase 2: CLI (Week 5-6)

**Goal:** A lightweight remote client CLI that team members install on their machines to interact with the HEBBS server.

### Tasks

- [ ] Create `hb` repo (Rust, lightweight, ~3MB binary)
- [ ] Build: login (with endpoint), push, recall, remember, forget, status
- [ ] Build: sync (one-time diff + upload changed files)
- [ ] Build: sync --watch (file watcher, auto-push on change)
- [ ] Build: sync --watch --daemon (background mode)
- [ ] Build: workspaces list, create, switch, delete, status
- [ ] No tune commands — tuning is a skill using existing recall/remember/prime
- [ ] Build: keys create, list, revoke
- [ ] Build: dashboard (opens browser to server URL)
- [ ] Multi-workspace behavior: prompt if multiple exist, no current set
- [ ] All output shows workspace name and endpoint
- [ ] Indexing-in-progress handling in recall output
- [ ] Publish to Homebrew tap + curl installer

### Deliverable

Customer's engineers install the CLI, login to their HEBBS server, push docs, test recall, all from their laptops.

### Validation

Install CLI on a clean laptop. Login to a deployed HEBBS server. Push docs, sync, recall. Everything works remotely.

---

## Phase 3: SDK updates (Week 6-7)

**Goal:** Python and TypeScript SDKs support REST transport with endpoint parameter.

### Tasks

- [ ] Update `hebbs-python`: add REST transport, `endpoint` param, `index()` method, `.text` property, indexing status, `insights()` as entity profiling
- [ ] Update `hebbs-typescript`: same changes
- [ ] Add best practices section to SDK docs (atomic facts, entity_id usage)
- [ ] Publish updated SDKs to PyPI and npm

### Deliverable

Customer's agent code integrates with HEBBS via `Hebbs(api_key="...", endpoint="https://hebbs.customer.com")`.

### Validation

Write a test agent that connects to a remote HEBBS server, pushes docs, recalls, remembers, checks insights. Verify all SDK methods work over REST.

---

## Phase 4: Central dashboard (Week 7-8)

**Goal:** Our admin panel to monitor all enterprise deployments.

### Tasks

- [ ] Build heartbeat receiver (POST /heartbeat endpoint)
- [ ] Build deployment list view (all customers, health, version, stats)
- [ ] Build deployment detail view (version history, memory/recall charts, alerts)
- [ ] Build alert engine (deployment down, version outdated, low activity, engine unhealthy)
- [ ] Set up notifications (email/Slack on alerts)
- [ ] Set up Postgres for heartbeat history
- [ ] Deploy on a VPS or small cloud instance (central.hebbs.ai)
- [ ] Auth for our team (simple, just us)

### Deliverable

We see all customer deployments, their health, and get alerted when something's wrong.

### Validation

Deploy 3 test HEBBS instances. Verify all appear in central dashboard. Kill one. Verify alert fires within 15 minutes.

---

## Phase 5: First enterprise deployment (Week 8-9)

**Goal:** Deploy at the first real customer.

### Tasks

- [ ] Prepare deployment package (docker-compose, .env, README)
- [ ] Schedule deployment session with customer
- [ ] Deploy on their machine, run through onboarding
- [ ] Customer pushes their docs, verifies recall
- [ ] Customer's engineers install CLI, connect from laptops
- [ ] Customer integrates SDK into their agent
- [ ] Verify heartbeat appears in our central dashboard
- [ ] Customer runs tune session with their agent (using tune skill + regular HEBBS calls)
- [ ] Gather feedback

### Deliverable

First paying enterprise customer running HEBBS in production on their infrastructure.

---

## Timeline summary

| Phase | Weeks | Deliverable |
|---|---|---|
| 0: Containerize engine | 1 | Engine runs in Docker |
| 1: Enterprise platform | 2-4 | Dashboard, onboarding, workspaces, API keys |
| 2: CLI | 5-6 | Remote client for team members |
| 3: SDK updates | 6-7 | Python + TypeScript with REST transport |
| 4: Central dashboard | 7-8 | Our admin panel for all deployments |
| 5: First deployment | 8-9 | First enterprise customer live |

**Total: ~9 weeks to first enterprise customer.**

Phase 2-3 can overlap (CLI and SDK in parallel). Phase 4 can overlap with Phase 3.

Aggressive: 6-7 weeks with parallel work.

---

## Team

| Role | Phase 0-2 | Phase 3-5 |
|---|---|---|
| Backend (platform) | 1-2 engineers | 1 engineer |
| Backend (engine) | 0 (no changes) | 0 |
| Frontend (dashboard) | 0-1 engineer | 1 engineer (central dashboard) |
| CLI | 0 | 1 engineer |
| **Total** | **2-3** | **2-3** |

---

## What's deferred (future phases)

- SaaS / multi-tenant cloud deployment
- Auto-tune (platform-side automated tuning)
- SSO / SAML for customer's team
- Audit logs
- Workspace export/import
- Multi-node horizontal scaling
- Billing integration
