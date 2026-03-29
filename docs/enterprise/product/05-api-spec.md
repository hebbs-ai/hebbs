# HEBBS Enterprise: API Specification

## Overview

The API is identical to the cloud/SaaS version. The only difference is the base URL; it is the customer's server, not `api.hebbs.ai`.

```
Enterprise:  https://hebbs.customer.com
Cloud (future): https://api.hebbs.ai
```

## Authentication

Two auth modes, both via `Authorization: Bearer <token>`:

| Mode | Token format | Use case |
|------|-------------|----------|
| API key | `hb_live_sk_...` or `hb_admin_sk_...` | SDK, CLI, external integrations |
| Session token | Base64url string from login | Dashboard UI |

Admin endpoints require either an admin API key (`hb_admin_sk_`) or a session from an admin account.

---

## Data plane (proxied to engine via daemon socket)

All data-plane endpoints require a valid API key or session.

### Via API key (workspace resolved from key)

```
POST   /v1/memories          Remember (store a memory)
GET    /v1/memories/:id      Get single memory
POST   /v1/recall            Recall (search memories)
POST   /v1/prime             Prime (load all memories for an entity)
POST   /v1/forget            Forget (delete memories)
GET    /v1/entities          List entities
GET    /v1/insights          Query insights (auto-generated entity profiles)
POST   /v1/upload            Upload files for indexing
```

### Via session (workspace specified in URL)

Used by the dashboard UI where session auth does not bind to a workspace.

```
POST   /v1/workspaces/:slug/memories    Remember into workspace
POST   /v1/workspaces/:slug/recall      Recall from workspace
POST   /v1/workspaces/:slug/prime       Prime from workspace
POST   /v1/workspaces/:slug/forget      Forget from workspace
GET    /v1/workspaces/:slug/entities    List entities in workspace
GET    /v1/workspaces/:slug/insights    Query insights in workspace
GET    /v1/workspaces/:slug/files       List indexed files in workspace
GET    /v1/workspaces/:slug/stats       Detailed workspace statistics
```

---

## Management plane (platform)

### Health (public, no auth)

```
GET    /v1/health/live       Liveness check
GET    /v1/system/health     System health (engine status, version)
```

### Onboarding (public, works once)

```
GET    /v1/onboarding/status   Check if onboarding is completed
POST   /v1/onboarding          Complete onboarding (create admin + workspace)
```

Request body:
```json
{
  "email": "admin@acme.com",
  "password": "...",
  "workspace_name": "support-agent"
}
```

Response:
```json
{
  "account": { "id": 1, "email": "admin@acme.com", "role": "admin" },
  "workspace": { "id": 1, "slug": "support-agent", "name": "support-agent" },
  "api_key": "hb_live_sk_..."
}
```

### Authentication (session-based for dashboard)

```
POST   /v1/auth/login        Login (email + password -> session token)
POST   /v1/auth/logout       Logout (invalidate session)
GET    /v1/auth/me           Get current user + workspace assignments
```

Login response:
```json
{
  "token": "base64url-session-token",
  "expires_at": "2026-04-04T...",
  "account": { "id": 1, "email": "admin@acme.com", "role": "admin" }
}
```

### Workspaces (admin only)

```
POST   /v1/workspaces         Create workspace (returns workspace + API key)
GET    /v1/workspaces         List all workspaces with stats
GET    /v1/workspaces/:slug   Get workspace detail
DELETE /v1/workspaces/:slug   Delete workspace (revokes all keys)
```

### API Keys (admin only)

```
POST   /v1/keys               Create API key
GET    /v1/keys               List all keys (prefix only, never full key)
DELETE /v1/keys/:id            Revoke key
```

### Accounts / Team (admin only)

```
POST   /v1/accounts           Create team member
GET    /v1/accounts           List all accounts with workspace assignments
PUT    /v1/accounts/:id       Update account (role, password, workspace assignments)
DELETE /v1/accounts/:id       Delete account (prevents self-deletion)
```

Create account request:
```json
{
  "email": "dev1@acme.com",
  "password": "...",
  "role": "developer",
  "workspace_ids": [1, 2]
}
```

### Configuration (admin only)

```
GET    /v1/config             Get current config (LLM, embedding, decay, deployment)
PUT    /v1/config             Update config (writes to engine's config.toml)
```

Config response:
```json
{
  "config": {
    "llm": { "provider": "openai", "model": "gpt-4o-mini" },
    "embedding": { "provider": "openai", "model": "text-embedding-3-small", "dimensions": 1536 },
    "decay": { "enabled": true, "half_life_days": 30 },
    "reflection": { "enabled": false, "interval_hours": 1 },
    "contradiction": { "enabled": true },
    "api": { "max_concurrent_requests": 10 },
    "deployment": { "id": "acme-corp-001", "heartbeat_enabled": true, "central_url": "https://central.hebbs.ai" }
  }
}
```

---

## Key endpoints explained

**`POST /v1/prime`**: Load all context for an entity at conversation start.

```json
{
  "entity_id": "retrieval-instructions",
  "max_memories": 30
}
```

Returns all memories for that entity, ranked by importance. Use cases:
- Load retrieval instructions before making recall calls (tuning)
- Load entity profile before a conversation ("what do we know about user_42?")
- Pre-fetch context for a topic ("everything about compliance")

Unlike `recall` (which searches by semantic similarity to a cue), `prime` returns everything for an entity without needing a query.

**`PUT /v1/memories/:id`** (future): Revise an existing memory. Creates a revision chain. The original memory is preserved with a `revised_from` edge to the new version.

**Contradiction detection** is automatic when the LLM key is configured (always in enterprise). The engine detects and resolves contradictions in the background. Contradictions are visible in the Memory Palace dashboard.

---

## Engine panel proxy (Memory Palace)

The platform proxies the engine's built-in Memory Palace panel. No auth required for panel routes (access controlled at the network level).

```
GET    /                      Memory Palace UI (HTML)
GET    /static/*              Static assets (JS, CSS)
GET    /api/panel/graph       Graph data (nodes + edges)
POST   /api/panel/recall      Search from panel
GET    /api/panel/health      Engine health detail
GET    /api/panel/dashboard   Dashboard data
GET    /api/panel/files       File listing
GET    /api/panel/memories    Memory listing
GET    /api/panel/vaults      Vault listing
POST   /api/panel/vaults/switch  Switch active vault (for workspace-scoped palace)
```

---

## Error responses

All errors follow:

```json
{
  "error": "Human-readable error message"
}
```

| Status | Meaning |
|--------|---------|
| 400 | Bad request (missing fields, invalid input) |
| 401 | Not authenticated (missing/invalid key or session) |
| 403 | Forbidden (insufficient role) |
| 404 | Not found |
| 409 | Conflict (duplicate slug, existing account) |
| 500 | Internal error (engine error, daemon timeout) |
