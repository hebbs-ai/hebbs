# TASK-33: CLI Multi-Workspace Support

## Problem

The CLI saves one endpoint + one API key. Customers with multiple workspaces must re-login every time they switch. No way to manage multiple workspace contexts.

## Solution

Support named workspace contexts in the CLI config, similar to kubectl contexts.

## Commands

```sh
# Login with workspace name
hebbs login --endpoint https://hebbs.acme.com --api-key <key> --workspace legal-docs

# Add another workspace
hebbs login --endpoint https://hebbs.acme.com --api-key <key2> --workspace sales-agent

# Switch active workspace
hebbs workspaces switch legal-docs

# List saved workspaces
hebbs workspaces list

# Commands use active workspace key
hebbs recall "query"

# Override per-command
hebbs recall "query" --workspace sales-agent
```

## Config format

`~/.config/hebbs/cli.toml`:

```json
{
  "endpoint": "https://hebbs.acme.com",
  "active_workspace": "legal-docs",
  "workspaces": {
    "legal-docs": { "api_key": "hb_live_sk_abc..." },
    "sales-agent": { "api_key": "hb_live_sk_xyz..." }
  }
}
```

## Behavior

- `hebbs login` without `--workspace` saves as "default"
- `hebbs workspaces switch` changes the active workspace
- All commands use the active workspace's API key unless `--workspace` overrides
- `hebbs workspaces list` shows all saved workspaces with active marker
- Backward compatible: existing single-key configs still work

## Priority

Before second customer. First customer can manage with single workspace or manual re-login.
