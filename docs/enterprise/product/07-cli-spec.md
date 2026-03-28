# HEBBS Enterprise: CLI Specification

## Overview

The remote client CLI is called `hb`. It's a lightweight binary that talks REST to the customer's HEBBS server. Same binary works for both enterprise and cloud.

For full CLI reference (all commands, flags, output), see [Cloud CLI Spec](../../<future>cloud/product/07-cli-spec.md).

This document covers only the enterprise-specific differences.

---

## Installation

```sh
brew install hebbs-ai/tap/hb
# or: curl -sSf https://hebbs.ai/install-hb | sh
```

---

## Login

```sh
hb login --endpoint https://hebbs.acme.com
# Opens browser → authenticates with admin/developer account
# Token saved to ~/.hb/config

# Or direct (for CI/CD, headless)
hb login --endpoint https://hebbs.acme.com --api-key hb_live_sk_abc123
```

### Config file

```toml
# ~/.hb/config
[auth]
endpoint = "https://hebbs.acme.com"
token = "hbt_session_abc123..."

[defaults]
workspace = "support-agent"
```

### Environment variables

```sh
export HEBBS_ENDPOINT=https://hebbs.acme.com
export HEBBS_API_KEY=hb_live_sk_abc123
export HEBBS_WORKSPACE=support-agent
```

---

## All commands

Every command from the [Cloud CLI Spec](../../<future>cloud/product/07-cli-spec.md) works identically:

- **Workspace management**: `hb workspaces list`, `create`, `switch`, `delete`, `status`
- **Push**: `hb push ./docs`
- **Sync**: `hb sync ./docs`, `hb sync --watch`, `hb sync --watch --daemon`
- **Recall**: `hb recall "query"` with indexing-in-progress handling
- **Remember**: `hb remember "fact" --entity-id X`
- **Forget**: `hb forget --entity-id X`
- **No tune commands** — tuning is a skill, not a CLI feature. The customer's agent uses `recall`, `remember`, `forget`, and `prime` to tune. See [08-tuning.md](08-tuning.md).
- **Keys**: `hb keys create`, `hb keys list`, `hb keys revoke`
- **Dashboard**: `hb dashboard` (opens browser to customer's dashboard URL)
- **Status**: `hb status` (shows workspace health, indexing progress)

---

## Enterprise-specific notes

### Endpoint is always required (first time)

Unlike cloud (which defaults to `api.hebbs.ai`), enterprise requires the endpoint to be specified during login. After login, it's saved in config and used for all subsequent commands.

### Dashboard opens customer's server

```sh
hb dashboard
# Opens https://hebbs.acme.com/workspaces/support-agent/panel
```

### No billing commands

The cloud CLI has `usage` and `billing` commands. These don't apply to enterprise. The CLI silently ignores them or returns "not available in enterprise mode."

### Multi-workspace behavior

Same as cloud: if multiple workspaces exist and no current workspace is set, CLI prompts. Workspace selection is persisted in config.
