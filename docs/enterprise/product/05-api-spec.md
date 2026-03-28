# HEBBS Enterprise: API Specification

## Overview

The API is identical to the cloud/SaaS version. The only difference is the base URL — it's the customer's server, not `api.hebbs.ai`.

```
Enterprise:  https://hebbs.customer.com
Cloud (future): https://api.hebbs.ai
```

## Full API reference

See [Cloud API Spec](../../cloud/product/05-api-spec.md) for the complete endpoint reference. Every endpoint, request/response format, error code, and rate limit applies identically to the enterprise deployment.

## Key differences from cloud

| Aspect | Cloud | Enterprise |
|---|---|---|
| Base URL | `api.hebbs.ai` | Customer's domain |
| Auth | API key (managed by cloud platform) | API key (managed by local platform) |
| Workspace routing | Gateway routes to workspace container | Platform proxies to local engine |
| TLS | Managed by us (ACM) | Managed by customer |
| Rate limits | Per plan | Configurable by admin |

## Quick reference

### Data plane (proxied to engine)

```
POST   /v1/memories          Remember (store a memory)
POST   /v1/recall            Recall (search memories)
POST   /v1/prime             Prime (load all memories for an entity — context priming)
POST   /v1/forget            Forget (delete memories)
GET    /v1/memories           List memories
GET    /v1/memories/:id       Get single memory
PUT    /v1/memories/:id       Revise (update a memory, creates revision chain)
GET    /v1/insights           Query insights (auto-generated entity profiles)
GET    /v1/entities           List entities
POST   /v1/upload             Upload files for indexing
GET    /v1/upload/status/:id  Check indexing progress
GET    /v1/health             Workspace health + indexing status
```

### Key endpoints explained

**`POST /v1/prime`** — Load all context for an entity at conversation start.

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

**`PUT /v1/memories/:id`** — Revise an existing memory.

```json
{
  "content": "User prefers light mode (changed from dark mode)",
  "importance": 0.8
}
```

Creates a revision chain — the original memory is preserved with a `revised_from` edge to the new version. Use when correcting a fact rather than adding a new one (which would create a contradiction).

**Contradiction detection** is automatic when the LLM key is configured (always in enterprise). The engine detects and resolves contradictions in the background — no manual prepare/commit workflow needed. Contradictions are visible in the Memory Palace dashboard.

### Management plane (platform)

```
POST   /v1/workspaces         Create workspace
GET    /v1/workspaces         List workspaces
DELETE /v1/workspaces/:slug   Delete workspace
POST   /v1/keys               Create API key
GET    /v1/keys               List keys
DELETE /v1/keys/:prefix       Revoke key
GET    /v1/config             Get config
PUT    /v1/config             Update config
GET    /v1/system/health      System health
```

## Authentication

Same as cloud: `Authorization: Bearer hb_live_sk_abc123...`

Admin endpoints require an admin key (`hb_admin_sk_`).
