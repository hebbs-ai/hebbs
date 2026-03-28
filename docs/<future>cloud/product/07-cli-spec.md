# HEBBS Cloud: CLI Specification

## Overview

`hb` is a standalone lightweight CLI for cloud customers. Written in Rust, compiles to a single static binary (~3MB). It talks REST to the HEBBS Cloud API. No RocksDB, no embedding engine. Optionally runs a lightweight file watcher for continuous sync (not the engine daemon — just watches files and pushes changes).

This is separate from the open-source `hebbs` binary which is the full engine.

---

## Installation

```sh
# macOS
brew install hebbs-ai/tap/hb

# Linux / macOS (curl)
curl -sSf https://hebbs.ai/install-hb | sh

# Verify
hb --version
```

---

## Authentication

### Login

```sh
hb login
```

Opens browser for OAuth flow. On success, stores token and current workspace in `~/.hb/config`.

If the org has **one workspace**, it's auto-selected:

```sh
hb login
# Opening browser for authentication...
# Authenticated as alex@company.com (acme-corp)
# Current workspace: support-agent (us)
```

If the org has **multiple workspaces**, the CLI prompts:

```sh
hb login
# Opening browser for authentication...
# Authenticated as alex@company.com (acme-corp)
#
# You have multiple workspaces. Which one?
#   1. support-agent (us)
#   2. sales-agent (eu)
# Select [1-2]: 1
# Current workspace: support-agent (us)
```

```sh
hb login --api-key hb_live_sk_abc123
```

Directly stores API key without browser flow. For CI/CD or headless environments. API key is scoped to a workspace, so no prompt needed.

### Config file

```toml
# ~/.hb/config
[auth]
token = "hbt_session_abc123..."        # from OAuth login
# OR
api_key = "hb_live_sk_abc123..."       # from --api-key login

[defaults]
org = "acme-corp"
workspace = "support-agent"
region = "us"
```

### Environment variables

```sh
export HEBBS_API_KEY=hb_live_sk_abc123
export HEBBS_WORKSPACE=sales-agent
```

Environment variables override config file. Useful for CI/CD.

---

## Commands

### Workspace management

```sh
# List workspaces
hb workspaces list

#   NAME           REGION   MEMORIES   STATUS
#   support-agent  us       3,152      active    ← current
#   sales-agent    eu       812        active

# Create workspace
hb workspaces create sales-agent --region eu

#   Workspace created: sales-agent (eu)
#   API key: hb_live_sk_sales_789...
#   Save this key — you won't see it again.

# Switch current workspace
hb workspaces switch sales-agent

#   Switched to: sales-agent (eu)

# Delete workspace
hb workspaces delete sales-agent

#   Are you sure? This will delete all memories and files. [y/N]

# Workspace status (after indexing complete)
hb workspaces status

#   Workspace:       support-agent (us)
#   Status:          active
#   Memories:        3,152 (812 from files, 2,340 from conversations)
#   Files uploaded:  34
#   Files indexed:   34/34
#   Last indexed:    2026-03-28 14:30 UTC
#   Tune:            last run 2026-03-25, recall: 85% (+27pp from baseline)
#   Dashboard:       https://app.hebbs.ai/workspaces/support-agent/panel

# Workspace status (during indexing)
hb workspaces status

#   Workspace:       support-agent (us)
#   Status:          indexing
#   Memories:        412 (growing)
#   Files uploaded:  34
#   Files indexed:   18/34
#   Currently:       embedding sections from policies/data-retention.md
#   Dashboard:       https://app.hebbs.ai/workspaces/support-agent/panel
```

### Push documents

```sh
# Push a folder (upload + index)
hb push ./docs

#   Pushing to workspace: support-agent (us)
#   Uploading 34 files (245 KB)...
#   ████████████████████████████████ 34/34
#   Indexing... done.
#   34 files, 812 memories created.

# Push specific files
hb push ./docs/new-policy.md ./docs/update.md

#   Pushing to workspace: support-agent (us)
#   Uploading 2 files (12 KB)...
#   Indexing... done.
#   2 files, 47 memories created.

# Push to a specific workspace (override current)
hb push ./sales-playbook --workspace sales-agent

#   Pushing to workspace: sales-agent (eu)
#   Uploading 8 files...

# Dry run — show what would be uploaded
hb push ./docs --dry-run

#   Would push to workspace: support-agent (us)
#   Would upload 34 files (245 KB):
#     docs/getting-started.md (4.2 KB)
#     docs/api-reference.md (12.1 KB)
#     ...
```

**Multi-workspace behavior:** If the customer has multiple workspaces and no current workspace is set (e.g., they created workspaces via the dashboard before using CLI), the CLI prompts:

```sh
hb push ./docs

#   You have multiple workspaces. Which one?
#     1. support-agent (us)
#     2. sales-agent (eu)
#   Select [1-2]: 1
#   Pushing to workspace: support-agent (us)
#   Uploading 34 files...
```

This prompt applies to all data-plane commands (`push`, `recall`, `remember`, `forget`) when no current workspace is set and multiple exist. Use `--workspace` flag or `hb workspaces switch` to avoid the prompt.

Only `.md` files are uploaded by default (matching hebbs index behavior). Use `--include "*.txt"` to include other file types if the engine supports them.

### Sync

`sync` is the smart alternative to `push`. It diffs local files against what's already in the workspace and only uploads changes. It can also run as a continuous file watcher.

**`push` vs `sync`:**

| | `push` | `sync` |
|---|---|---|
| Uploads | All files, every time | Only new/changed files |
| Deletes remote | No | Yes (if file removed locally, with `--delete`) |
| Continuous mode | No | Yes (`--watch`) |
| Use case | First-time upload, CI/CD | Day-to-day development |

```sh
# One-time smart sync (diff and upload changes only)
hb sync ./docs

#   Workspace: support-agent (us)
#   Comparing local ./docs with remote...
#
#   New:      2 files (policies/new-policy.md, guides/onboarding.md)
#   Changed:  1 file  (docs/security.md)
#   Deleted:  0 files
#   Unchanged: 31 files
#
#   Uploading 3 files (18 KB)...
#   Done. Indexing will proceed in background.

# Sync with deletion (remove remote files that no longer exist locally)
hb sync ./docs --delete

#   ...
#   Deleted:  1 file  (docs/old-faq.md) — will be removed from workspace
#   ...

# Dry run — see what would change
hb sync ./docs --dry-run

#   Would upload 3 files, delete 0 files.
```

**Continuous file watching:**

```sh
# Watch for changes and auto-push (foreground, Ctrl+C to stop)
hb sync ./docs --watch

#   Workspace: support-agent (us)
#   Watching ./docs for changes... (Ctrl+C to stop)
#
#   [14:32:01] Changed: docs/security.md → uploading... done
#   [14:35:12] New:     policies/gdpr-v2.md → uploading... done
#   [14:40:03] Deleted: drafts/old-notes.md → removed from workspace
```

**Background daemon mode:**

```sh
# Start watcher as background daemon
hb sync ./docs --watch --daemon

#   Sync daemon started (PID 48201)
#   Watching: ./docs → workspace: support-agent (us)
#   Logs: ~/.hb/sync.log
#   Stop with: hb sync stop

# Check daemon status
hb sync status

#   Sync daemon: running (PID 48201)
#   Watching: ./docs
#   Workspace: support-agent (us)
#   Last sync: 2026-03-28 14:40:03 UTC
#   Files synced today: 7

# Stop the daemon
hb sync stop

#   Sync daemon stopped.
```

**What the sync daemon is NOT:**
- It is NOT the HEBBS engine daemon (no embedding, no indexing, no RocksDB)
- It is just a file watcher that calls the upload API when files change
- All the heavy work (indexing, embedding, extraction) happens server-side in the workspace container
- It's ~50 lines of logic: watch folder → detect change → HTTP POST to upload endpoint

**Sync flags:**

```
--watch           Start continuous file watcher (foreground)
--daemon          Run watcher in background (requires --watch)
--delete          Remove remote files that no longer exist locally
--dry-run         Show what would change without uploading
--include "*.txt" Include non-default file types
--interval 5      Polling interval in seconds (default: 2s, for systems without native fs events)
```

### Recall

```sh
# Basic recall
hb recall "password reset process"

#   Workspace: support-agent (us)
#
#   1. [0.92] Password Reset Documentation
#      Users can reset passwords via Settings > Security > Reset Password.
#      The reset link expires after 24 hours.
#      (importance: 0.7, source: docs/security.md)
#
#   2. [0.85] User #42 conversation: password reset
#      User asked about password reset. Resolution: guided through
#      Settings > Security. Completed in under a minute.
#      (importance: 0.5, 1 week ago)
#
#   3. [0.78] ...

# Recall for a specific user
hb recall "preferences" --entity-id user_42

# JSON output (for scripting)
hb recall "query" --format json

# Include global brain
hb recall "company founding" --global
```

**During indexing:**

```sh
# Partial results available
hb recall "password reset"

#   Workspace: support-agent (us)
#   ⏳ Indexing in progress: 18/34 files indexed, 412 memories so far.
#      Showing results from indexed content only.
#
#   1. [0.89] Password Reset Documentation
#      Users can reset passwords via Settings > Security...

# No results yet (first-time indexing just started)
hb recall "password reset"

#   Workspace: support-agent (us)
#   ⏳ First-time indexing in progress: 0/34 files indexed.
#      No memories available yet. Check progress:
#        hb workspaces status
#        hb dashboard
```

### Remember

```sh
# Store a memory
hb remember "User prefers dark mode in all editors"

#   Stored: 01JABCDEF... (importance: 0.5)

# With user and importance
hb remember "Never suggest light mode" --entity-id user_42 --importance 0.9

# Pipe content
echo "Meeting notes: decided to use Postgres" | hb remember --importance 0.7

# Store in global brain
hb remember "Company was founded in 2019" --global
```

### Forget

```sh
# Forget by user
hb forget --entity-id user_42

#   Are you sure? This will delete 87 memories. [y/N]
#   Forgotten: 87 memories

# Forget by ID
hb forget --id 01JABCDEF...

# Forget old memories
hb forget --older-than 90d

# Skip confirmation
hb forget --entity-id user_42 --yes
```

### Dashboard

```sh
# Open Memory Palace in browser
hb dashboard

#   Opening https://app.hebbs.ai/workspaces/sales-agent/panel
```

### API keys

```sh
# Create a new key for current workspace
hb keys create --name production

#   API key: hb_live_sk_newkey789...
#   Save this key — you won't see it again.

# List keys
hb keys list

#   PREFIX              NAME          WORKSPACE      CREATED
#   hb_live_sk_abc1...  default       default        2026-03-28
#   hb_live_sk_newk...  production    sales-agent    2026-03-28

# Revoke a key
hb keys revoke hb_live_sk_abc1

# Rotate a key (24h grace period for old key)
hb keys rotate hb_live_sk_abc1
```

### Team

```sh
# Invite a member
hb members invite alice@company.com --role admin

# List members
hb members list

#   EMAIL                ROLE        JOINED
#   alex@company.com     owner       2026-03-01
#   alice@company.com    admin       2026-03-15

# Change role
hb members role alice@company.com developer

# Remove
hb members remove alice@company.com
```

### Usage and billing

```sh
# Current usage
hb usage

#   Period: March 2026
#   Plan: Pro ($49/mo)
#
#   WORKSPACE      MEMORIES   RECALLS    STORAGE
#   default        3,152      45,230     12 MB
#   sales-agent    812        8,400      3 MB
#   TOTAL          3,964      53,630     15 MB
#
#   Limits: 100,000 memories | 1,000,000 recalls | 5 GB storage

# Billing portal
hb billing

#   Opening Stripe billing portal...
```

### Tuning

There are no tune CLI commands. Tuning is a skill — the customer's agent uses regular `recall`, `remember`, `forget`, and `prime` commands to tune. See [08-tuning.md](08-tuning.md).

### Export / Import

```sh
# Export workspace (for migration to self-hosted)
hb export --output ./backup/

#   Exporting workspace: sales-agent
#   Memories: 812
#   Files: 22
#   Output: ./backup/sales-agent-2026-03-28.tar.gz

# Import is done via the open-source hebbs binary:
# hebbs import ./backup/sales-agent-2026-03-28.tar.gz
```

---

## Global flags

```
--workspace, -w     Override current workspace
--format, -f        Output format: human (default), json
--yes, -y           Skip confirmation prompts
--verbose, -v       Show request/response details
--quiet, -q         Suppress non-essential output
--endpoint          Override API endpoint (for self-hosted platform)
```

---

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | General error |
| 2 | Authentication error |
| 3 | Quota exceeded |
| 4 | Resource not found |
| 5 | Network error |

---

## Shell completions

```sh
# Generate completions
hb completions bash > /etc/bash_completion.d/hb
hb completions zsh > ~/.zfunc/_hb
hb completions fish > ~/.config/fish/completions/hb.fish
```

---

## CI/CD usage

```yaml
# GitHub Actions example
- name: Push docs to HEBBS
  env:
    HEBBS_API_KEY: ${{ secrets.HEBBS_API_KEY }}
    HEBBS_WORKSPACE: support-agent
  run: |
    curl -sSf https://hebbs.ai/install-hb | sh
    hb push ./docs --yes
```
