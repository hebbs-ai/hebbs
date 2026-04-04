# TASK-39: CLI --workspace Flag for Multi-Workspace Support

**Status:** Planned
**Priority:** High
**Scope:** `hebbs-cli` crate (cli.rs, rest.rs), `hebbs-platform` (verify workspace routes)

---

## Problem

The CLI currently routes all requests based on the API key. A workspace-scoped key always hits its workspace. An admin key hits the default vault. There's no way to use one key across multiple workspaces from the CLI.

The `hebbs workspaces switch` command exists but doesn't actually route requests to workspace-scoped endpoints.

## Solution

Add a `--workspace` global flag to the CLI. When set, all REST requests route to `/v1/workspaces/<slug>/...` instead of `/v1/...`.

### Usage

```sh
# Simple (uses saved key, workspace determined by key)
hebbs recall "query"

# Explicit workspace (overrides for this command)
hebbs recall "query" --workspace enterprise-legal

# Explicit workspace + key (fully self-contained)
hebbs recall "query" --workspace sales-team --api-key hb_admin_sk_...

# Push to specific workspace
hebbs push ./docs --workspace sales-team
```

### Behavior

- `--workspace` is optional on all commands
- When provided, REST client routes to `/v1/workspaces/<slug>/<operation>` instead of `/v1/<operation>`
- When not provided, current behavior (key determines workspace)
- `hebbs workspaces switch <slug>` saves default workspace to config; `--workspace` flag overrides it per-command
- Also support `HEBBS_WORKSPACE` env var

### Implementation

1. **cli.rs**: Add `--workspace` global flag (like `--api-key`)
2. **rest.rs**: `RestClient` reads workspace from flag/env/config and prefixes request paths with `/v1/workspaces/<slug>/` when set
3. **config**: `exec_login` should call a new `/v1/auth/whoami` endpoint to display "Connected to workspace: X"
4. **Platform**: Add `/v1/auth/whoami` endpoint that returns workspace info for the authenticated API key

### Server Routes (already exist)

The platform already supports workspace-scoped routes:
- `POST /v1/workspaces/:slug/recall`
- `POST /v1/workspaces/:slug/upload`
- `POST /v1/workspaces/:slug/memories`
- `POST /v1/workspaces/:slug/forget`
- `GET /v1/workspaces/:slug/insights`
- `GET /v1/workspaces/:slug/entities`

### New Server Endpoint

`GET /v1/auth/whoami` (authenticated):
```json
{
  "key_id": 2,
  "role": "workspace",
  "workspace_id": 1,
  "workspace_slug": "enterprise-legal",
  "workspace_name": "enterprise-legal"
}
```

This lets the CLI display which workspace the key belongs to on login.

## Testing

1. Login with admin key, use `--workspace` to hit different workspaces
2. Login with workspace key, verify default routing works without `--workspace`
3. Login with workspace key, use `--workspace` to override (admin keys only)
4. `hebbs workspaces switch` saves default, verify it's used when no flag
5. `--workspace` flag overrides saved default
6. `HEBBS_WORKSPACE` env var works
