# HEBBS Cloud: Rollout Plan

## Build order

The rollout is sequenced so that each milestone delivers a usable product. No big bang launch. Each phase builds on the previous one and can be shipped independently.

---

## Phase 0: Foundation (Week 1-2)

**Goal:** Containerize the existing engine and prove it works in a managed environment.

### Tasks

- [ ] Build Docker image from `hebbs` repo (no code changes to engine)
- [ ] Verify hebbs-server runs correctly in container with OpenAI config via env vars
- [ ] Verify file upload to volume + daemon auto-indexing works
- [ ] Verify REST API accessible from outside container
- [ ] Verify Memory Palace panel accessible from outside container
- [ ] Set up container registry (push image)
- [ ] Set up Kubernetes cluster in US region
- [ ] Deploy one test workspace manually (kubectl apply)
- [ ] Test full lifecycle: upload files → index → recall → remember → panel

### Deliverable

A manually deployed hebbs-server container that works identically to a local installation, but runs in the cloud. No platform, no auth, no gateway — just the raw engine in a container.

### Validation

Push the `enterprise-legal-mini` demo vault, run the existing tune evals, confirm same recall quality as local.

---

## Phase 1: Regional Gateway (Week 3-4)

**Goal:** A gateway that routes requests to workspace containers and handles file uploads.

### Tasks

- [ ] Create `hebbs-platform` repo
- [ ] Build regional gateway: route requests to workspace container by workspace ID
- [ ] Implement file upload endpoint (write to workspace volume)
- [ ] Implement usage metering (emit events, store locally)
- [ ] Implement workspace provisioning (create k8s resources via API)
- [ ] Implement workspace suspension and resume
- [ ] Implement health monitoring (check workspace readiness probes)
- [ ] API key validation (hardcoded keys for now — auth service comes later)

### Deliverable

`us.api.hebbs.ai` running. Can create tenants, upload files, recall memories, all via REST. Auth is a hardcoded API key list (placeholder).

### Validation

Create 5 workspaces programmatically. Upload different doc sets to each. Recall from each — verify isolation. Suspend and resume a workspace — verify data persists.

---

## Phase 2: Auth and Workspaces (Week 5-6)

**Goal:** Real authentication, org/workspace management, API key generation.

### Tasks

- [ ] Build auth service: GitHub OAuth + email/password sign-up
- [ ] Build API key generation and validation (hashed storage, prefix-based lookup)
- [ ] Build org management (create, members, roles)
- [ ] Build workspace management (create with region, list, delete)
- [ ] Build global gateway (api.hebbs.ai): auth → resolve workspace → forward to region
- [ ] Connect workspace creation to workspace provisioning in regional gateway
- [ ] Set up Postgres (platform DB) with schema from 04-platform-services.md

### Deliverable

A customer can sign up, create a workspace, get an API key, and use it to push docs and recall memories. Multi-user orgs with roles work.

### Validation

End-to-end: sign up → create workspace → push docs → recall → remember → invite teammate → teammate recalls. All via API (no console yet).

---

## Phase 3: SDKs and CLI (Week 7-8)

**Goal:** The customer-facing tools that make the API easy to use.

### Tasks

- [ ] Update `hebbs-python`: add REST transport, cloud constructor, `index()` method, `.text` property, indexing status on recall
- [ ] Update `hebbs-typescript`: same changes
- [ ] Build `hb`: login, push, recall, remember, workspaces, status, keys
- [ ] Write quickstart guide (sign up → push → recall in 10 minutes)
- [ ] Publish updated SDKs to PyPI and npm
- [ ] Publish CLI to Homebrew tap

### Deliverable

Customers can install the SDK or CLI and go from zero to working recall in 10 minutes.

### Validation

Follow the quickstart guide from scratch on a clean machine. Time it. Must be under 10 minutes.

---

## Phase 4: Billing (Week 9-10)

**Goal:** Stripe integration, plan enforcement, usage-based limits.

### Tasks

- [ ] Integrate Stripe: customer creation, subscription management, payment methods
- [ ] Define plans in Stripe (Free, Pro)
- [ ] Implement usage aggregation: collect events from regional gateways, aggregate hourly
- [ ] Implement quota enforcement: 402 on limit exceeded
- [ ] Implement rate limiting: 429 with retry-after headers
- [ ] Build billing API endpoints (plan, upgrade, invoices, portal redirect)
- [ ] Handle upgrade/downgrade flows
- [ ] Handle payment failure → workspace suspension

### Deliverable

Free and Pro plans are live. Customers can upgrade, see usage, and are properly limited.

### Validation

Create a free-tier workspace, hit memory limit, verify 402. Upgrade to Pro, verify limit increases. Generate an invoice via Stripe test mode.

---

## Phase 5: Console (Week 11-13)

**Goal:** Web dashboard for org/workspace management and Memory Palace access.

### Tasks

- [ ] Build web console at `app.hebbs.ai`
- [ ] Dashboard: org overview, workspace list, usage charts
- [ ] Workspace detail: memory count, file list, tune status, Memory Palace link
- [ ] Memory Palace: proxy to regional workspace panel (embed or redirect)
- [ ] Team management: invite, roles, remove
- [ ] API key management: create, revoke, rotate
- [ ] Billing: current plan, usage, upgrade button, Stripe portal link
- [ ] Onboarding: post-sign-up flow with quickstart code snippets

### Deliverable

Customers have a web UI to manage everything. Memory Palace is accessible from the browser.

### Validation

Complete the customer journey from 02-customer-journey.md entirely through the console + SDK.

---

## Phase 6: EU Region (Week 14-15)

**Goal:** Second region live, data residency working.

### Tasks

- [ ] Set up Kubernetes cluster in EU region (eu-west-1)
- [ ] Deploy regional gateway in EU
- [ ] Register EU in region_registry
- [ ] Verify workspace creation with `--region eu`
- [ ] Verify data stays in EU (audit network traffic)
- [ ] Verify direct regional access (`eu.api.hebbs.ai`) works
- [ ] Update console to show region per workspace
- [ ] Update SDK to accept `region` parameter

### Deliverable

Customers can create EU workspaces. Data residency is enforced.

### Validation

Create EU workspace, upload docs, recall. Inspect: no data in US infrastructure. Direct regional call works without transiting US.

---

## Phase 7: Tune Commands (Week 16-17)

**Goal:** CLI and SDK tune commands so the customer's own agent can drive tuning.

### Tasks

- [ ] Add `hb tune profile` command (stores ICP as memory)
- [ ] Add `hb tune eval add/list/remove/import` commands (stores evals as memories)
- [ ] Add `hb tune baseline` command (runs evals, scores, reports)
- [ ] Add `hb tune run` command (tries parameter variations, reports improvements)
- [ ] Add `hb tune store` command (stores winning strategies as retrieval-instructions)
- [ ] Add `hb tune export` command (exports compiled rules to markdown)
- [ ] Add `hb tune status` command (shows profile, eval count, scores)
- [ ] Add equivalent `hb.tune.*` methods to Python and TypeScript SDKs
- [ ] Add tune status to console dashboard
- [ ] Write tune guide: "How to tune HEBBS with your AI agent"

### Deliverable

Customer's own agent (Claude, GPT, etc.) can drive the full tune workflow via CLI or SDK. No HEBBS-side automation needed — the customer's agent does the thinking, HEBBS stores the results.

### Validation

Load the tune skill into Claude Code, point it at a cloud workspace with indexed docs. Run the full tune flow conversationally. Verify recall improvement of >10pp. Verify exported rules file is usable.

---

## Phase 8: Polish and Launch (Week 18-19)

**Goal:** Production-ready for public launch.

### Tasks

- [ ] Load testing: 100 concurrent tenants, 1000 req/s
- [ ] Security audit: penetration test on gateway and console
- [ ] Documentation: API reference, SDK guides, architecture overview
- [ ] Landing page update: hebbs.ai with cloud offering
- [ ] Status page: status.hebbs.ai
- [ ] Monitoring and alerting: all alerts from 11-deployment.md configured
- [ ] Backup verification: restore a workspace from snapshot
- [ ] Onboarding email sequence (welcome, quickstart, week-1 check-in)
- [ ] Pricing page

### Deliverable

HEBBS Cloud is live, documented, monitored, and ready for customers.

---

## Timeline summary

| Phase | Weeks | Deliverable |
|---|---|---|
| 0: Foundation | 1-2 | Engine runs in container |
| 1: Regional Gateway | 3-4 | Routing and file upload work |
| 2: Auth & Workspaces | 5-6 | Sign-up → API key → working recall |
| 3: SDKs & CLI | 7-8 | 10-minute quickstart |
| 4: Billing | 9-10 | Free and Pro plans live |
| 5: Console | 11-13 | Web dashboard |
| 6: EU Region | 14-15 | Data residency |
| 7: Tune Commands | 16-17 | User-driven tuning via agent + CLI/SDK |
| 8: Launch | 18-19 | Production-ready |

**Total: ~19 weeks from start to public launch.**

### What can ship early (private beta)

After Phase 3 (Week 8), you have a working product:
- Sign up, get API key
- Push docs via SDK/CLI
- Recall and remember via SDK
- Free tier (no billing enforcement yet — just tracking)
- US region only

This is enough for 5-10 beta customers. Their feedback shapes Phase 4-8.

### What can be deferred post-launch

- Auto-tune (platform-driven automated tuning — the customer's agent handles tuning at launch)
- Export/import (workspace migration)
- SSO/SAML (Enterprise only)
- Audit logs (Enterprise only)
- Additional regions beyond US + EU
- Workspace region migration
- Cross-region replication

---

## Team

| Role | Phase 0-3 | Phase 4-8 |
|---|---|---|
| Backend (platform) | 1-2 engineers | 1-2 engineers |
| Backend (engine) | 0 (no changes) | 0 (no changes) |
| Frontend (console) | 0 | 1 engineer |
| DevOps / Infra | 1 engineer | 1 engineer |
| **Total** | **2-3** | **3-4** |

The engine team is free to continue open-source development. Cloud doesn't block them or depend on them.
