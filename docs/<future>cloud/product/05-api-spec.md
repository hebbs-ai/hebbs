# HEBBS Cloud: API Specification

## Base URL

```
Global:   https://api.hebbs.ai
Regional: https://us.api.hebbs.ai  |  https://eu.api.hebbs.ai
```

All endpoints accept and return JSON. All requests require an API key.

## Authentication

```
Authorization: Bearer hb_live_sk_abc123...
```

Every request must include the API key in the Authorization header. The key determines the org and workspace scope.

For org-scoped keys (`hb_live_ok_`), include the workspace in the header:

```
Authorization: Bearer hb_live_ok_abc123...
X-Hebbs-Workspace: sales-agent
```

---

## Data Plane (forwarded to regional hebbs-server)

These are the core memory operations. They are proxied through the gateway to the workspace's hebbs-server container. The request/response format matches the existing hebbs REST API — no translation layer.

### Remember

Store a memory.

```
POST /v1/memories
```

**Request:**
```json
{
  "content": "User prefers dark mode in all editors",
  "importance": 0.7,
  "entity_id": "user_42",
  "context": {
    "source": "conversation",
    "session_id": "sess_abc"
  },
  "edges": [
    {
      "target_id": "01JAB123...",
      "edge_type": "related_to",
      "confidence": 0.8
    }
  ]
}
```

**Response (201):**
```json
{
  "memory_id": "01JABCDEF123456789...",
  "created_at": 1711612800000000
}
```

**Simplified form (most common):**
```json
{
  "content": "User prefers dark mode",
  "entity_id": "user_42"
}
```

When `importance` is omitted, the system assigns a default (0.5). Auto-importance inference upgrades this later based on content analysis.

### Recall

Retrieve relevant memories.

```
POST /v1/recall
```

**Request:**
```json
{
  "cue": "What are the user's display preferences?",
  "entity_id": "user_42",
  "top_k": 10,
  "strategy": "similarity",
  "weights": {
    "relevance": 0.5,
    "recency": 0.2,
    "importance": 0.2,
    "reinforcement": 0.1
  },
  "include_global": false
}
```

**Simplified form (most common):**
```json
{
  "cue": "What are the user's display preferences?",
  "entity_id": "user_42"
}
```

When `strategy`, `weights`, and `top_k` are omitted, the system auto-selects based on the query and workspace tuning state. This is the recommended usage for cloud customers.

**Response (200):**
```json
{
  "memories": [
    {
      "memory_id": "01JABCDEF...",
      "content": "User prefers dark mode in all editors",
      "importance": 0.7,
      "score": 0.92,
      "entity_id": "user_42",
      "created_at": 1711612800000000,
      "last_accessed": 1711699200000000,
      "access_count": 3,
      "context": {
        "source": "conversation"
      }
    }
  ],
  "strategy_used": "similarity",
  "query_time_ms": 12,
  "indexing": {
    "in_progress": false,
    "files_indexed": 34,
    "files_total": 34,
    "memories": 812
  }
}
```

**Indexing in progress:**

When `indexing.in_progress` is `true`, the recall results are partial — only memories from files indexed so far are searchable. If no files have been indexed yet, `memories` will be an empty array. The `indexing` object is always present so the caller can inform the user or decide to wait.

```json
{
  "memories": [],
  "strategy_used": "similarity",
  "query_time_ms": 2,
  "indexing": {
    "in_progress": true,
    "files_indexed": 0,
    "files_total": 34,
    "memories": 0
  }
}
```

**Convenience field — `text`:**

The response also includes a pre-formatted text representation for direct injection into prompts:

```json
{
  "memories": [...],
  "text": "- User prefers dark mode in all editors (importance: 0.7, 3 days ago)\n- User uses VS Code as primary editor (importance: 0.5, 1 week ago)\n..."
}
```

This allows:
```python
memories = hb.recall("preferences", entity_id="user_42")
prompt = f"Context:\n{memories.text}\n\nUser: {message}"
```

### Forget

Delete memories matching criteria.

```
POST /v1/forget
```

**Request (by IDs):**
```json
{
  "ids": ["01JABCDEF...", "01JABCDE2..."]
}
```

**Request (by entity):**
```json
{
  "entity_id": "user_42"
}
```

**Request (by staleness):**
```json
{
  "staleness_days": 90
}
```

**Response (200):**
```json
{
  "forgotten": 12
}
```

### Get memory

Retrieve a single memory by ID.

```
GET /v1/memories/:id
```

**Response (200):**
```json
{
  "memory_id": "01JABCDEF...",
  "content": "User prefers dark mode",
  "importance": 0.7,
  "entity_id": "user_42",
  "created_at": 1711612800000000,
  "edges": [
    {
      "target_id": "01JAB123...",
      "edge_type": "related_to",
      "confidence": 0.8,
      "direction": "outgoing"
    }
  ]
}
```

### List memories

Paginated list of all memories in the workspace.

```
GET /v1/memories?entity_id=user_42&limit=50&cursor=01JABCDEF...
```

**Response (200):**
```json
{
  "memories": [...],
  "next_cursor": "01JABCDF0...",
  "total": 342
}
```

### Revise

Update an existing memory. Creates a revision chain (original is preserved).

```
PUT /v1/memories/:id
```

**Request:**
```json
{
  "content": "User prefers dark mode, specifically Monokai theme",
  "importance": 0.8
}
```

**Response (200):**
```json
{
  "memory_id": "01JABNEW...",
  "revised_from": "01JABCDEF...",
  "created_at": 1711699200000000
}
```

### Upload files

Upload documents for indexing.

```
POST /v1/upload
Content-Type: multipart/form-data
```

**Request:**
- `files`: one or more files (multipart)
- `path` (optional): subdirectory to place files in (e.g., `policies/`)

**Response (202):**
```json
{
  "uploaded": 5,
  "total_bytes": 245760,
  "status": "indexing",
  "status_url": "/v1/upload/status/batch_01JAB..."
}
```

Indexing is async. Poll the status URL or check workspace status.

### Upload status

```
GET /v1/upload/status/:batch_id
```

**Response (200):**
```json
{
  "batch_id": "batch_01JAB...",
  "status": "complete",
  "files": 5,
  "memories_created": 127,
  "duration_ms": 4500
}
```

### Insights

Query auto-generated insights from the reflection engine.

```
GET /v1/insights?entity_id=user_42&min_confidence=0.7
```

**Response (200):**
```json
{
  "insights": [
    {
      "insight_id": "01JAB...",
      "content": "User consistently prefers dark themes across all tools",
      "confidence": 0.85,
      "source_memories": ["01JAB1...", "01JAB2...", "01JAB3..."],
      "entity_id": "user_42",
      "created_at": 1711699200000000
    }
  ]
}
```

### Entities

List all entity IDs in the workspace.

```
GET /v1/entities
```

**Response (200):**
```json
{
  "entities": [
    {"entity_id": "user_42", "memory_count": 87},
    {"entity_id": "user_15", "memory_count": 64},
    {"entity_id": "billing", "memory_count": 203}
  ]
}
```

### Health

```
GET /v1/health
```

**Response (200):**
```json
{
  "status": "ok",
  "workspace": "support-agent",
  "region": "us",
  "memories": 3152,
  "daemon": "running",
  "last_reflection": 1711699200000000,
  "indexing": {
    "in_progress": false,
    "files_uploaded": 34,
    "files_indexed": 34,
    "files_changed": 0,
    "currently_processing": null,
    "memories_from_files": 812,
    "memories_from_remember": 2340,
    "last_indexed_at": 1711612800000000
  }
}
```

**During active indexing:**
```json
{
  "status": "indexing",
  "workspace": "support-agent",
  "region": "us",
  "memories": 412,
  "daemon": "running",
  "last_reflection": null,
  "indexing": {
    "in_progress": true,
    "files_uploaded": 34,
    "files_indexed": 18,
    "files_changed": 0,
    "currently_processing": "policies/data-retention.md",
    "memories_from_files": 412,
    "memories_from_remember": 0,
    "last_indexed_at": null
  }
}
```

---

## Management Plane (handled by central platform)

These endpoints manage orgs, workspaces, API keys, and billing. Handled directly by the platform services, not forwarded to workspace containers.

### Workspaces

```
POST   /v1/workspaces                    → create workspace
GET    /v1/workspaces                    → list workspaces in org
GET    /v1/workspaces/:slug              → get workspace details
DELETE /v1/workspaces/:slug              → delete workspace
POST   /v1/workspaces/:slug/export       → export workspace data
```

**Create workspace request:**
```json
{
  "name": "sales-agent",
  "region": "eu"
}
```

**Create workspace response (201):**
```json
{
  "workspace": {
    "id": "ws_01JAB...",
    "name": "sales-agent",
    "slug": "sales-agent",
    "region": "eu",
    "status": "provisioning",
    "created_at": "2026-03-28T12:00:00Z"
  },
  "api_key": "hb_live_sk_abc123..."
}
```

### API Keys

```
POST   /v1/keys                          → create API key
GET    /v1/keys                          → list keys (shows prefix only)
DELETE /v1/keys/:prefix                  → revoke key
POST   /v1/keys/:prefix/rotate           → rotate key (24h grace period)
```

**Create key request:**
```json
{
  "name": "production",
  "workspace": "sales-agent",
  "type": "workspace"
}
```

### Members

```
POST   /v1/members/invite                → invite member
GET    /v1/members                       → list members
PUT    /v1/members/:id/role              → change role
DELETE /v1/members/:id                   → remove member
```

### Org

```
GET    /v1/org                           → get org details
PUT    /v1/org                           → update org (name)
GET    /v1/org/usage                     → usage summary across workspaces
```

**Usage response:**
```json
{
  "period": "2026-03",
  "workspaces": [
    {
      "slug": "support-agent",
      "region": "us",
      "memories": 3152,
      "recalls": 45230,
      "remembers": 2340,
      "files": 34,
      "storage_bytes": 12582912
    }
  ],
  "totals": {
    "memories": 3152,
    "recalls": 45230,
    "storage_bytes": 12582912
  }
}
```

### Billing

```
GET    /v1/billing/plan                  → current plan
POST   /v1/billing/upgrade               → upgrade plan
GET    /v1/billing/invoices              → list invoices
GET    /v1/billing/portal                → redirect to Stripe customer portal
```

---

## Error format

All errors follow a consistent format:

```json
{
  "error": {
    "code": "workspace_not_found",
    "message": "Workspace 'sales-agent' not found in your org",
    "status": 404
  }
}
```

**Common error codes:**

| Code | Status | Meaning |
|---|---|---|
| `unauthorized` | 401 | Invalid or missing API key |
| `forbidden` | 403 | Key doesn't have access to this workspace |
| `workspace_not_found` | 404 | Workspace doesn't exist |
| `memory_not_found` | 404 | Memory ID doesn't exist |
| `quota_exceeded` | 402 | Plan limit reached (memories, recalls, storage) |
| `rate_limited` | 429 | Too many requests per second |
| `workspace_suspended` | 403 | Workspace suspended (inactivity or payment failure) |
| `region_unavailable` | 503 | Regional infrastructure issue |
| `indexing_in_progress` | 409 | File upload conflicts with active indexing |

---

## Rate limits

| Plan | Recalls/sec | Remembers/sec | Uploads/min |
|---|---|---|---|
| Free | 10 | 5 | 5 |
| Pro | 100 | 50 | 30 |
| Enterprise | Custom | Custom | Custom |

Rate limit headers included in all responses:

```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 87
X-RateLimit-Reset: 1711612860
```

---

## Versioning

API is versioned via URL path (`/v1/`). Breaking changes get a new version. Non-breaking additions (new fields, new optional parameters) are added to the current version.
