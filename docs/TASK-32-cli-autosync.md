# TASK-32: CLI Auto-Sync (hebbs sync --watch)

## Problem

Enterprise customers must manually run `hebbs push ./docs` every time files change. There's no way to keep a local directory in sync with a remote HEBBS workspace automatically.

## Solution

Add `hebbs sync` command with a `--watch` flag that monitors a local directory for file changes and auto-pushes to the remote server.

## Commands

```sh
# One-time sync (diff-based, only uploads changed files)
hebbs sync ./docs

# Watch mode (runs in foreground, pushes on file change)
hebbs sync ./docs --watch

# Background daemon mode
hebbs sync ./docs --watch --daemon
```

## How it works

1. On first sync, push all files (same as `hebbs push`)
2. Record file hashes locally (in `~/.config/hebbs/sync-state.json`)
3. On subsequent syncs, compare hashes, only upload changed/new files
4. In `--watch` mode, use OS file watcher (notify/fsevents) to detect changes
5. Debounce: wait 2 seconds after last change before pushing (batch multiple saves)
6. On delete: optionally trigger re-index (files removed from vault)

## Sync state file

```json
{
  "endpoint": "http://server:8080",
  "workspace": "support-agent",
  "local_path": "/Users/dev/docs",
  "files": {
    "policy.md": { "hash": "abc123", "size": 4521, "synced_at": "2026-03-30T..." },
    "contracts/nda.md": { "hash": "def456", "size": 8932, "synced_at": "2026-03-30T..." }
  }
}
```

## Implementation

In the Rust CLI (`hebbs-cli/src/rest.rs`):
- Add `Sync` command to cli.rs
- Add file hashing (SHA-256)
- Add diff calculation (new, modified, deleted)
- Add file watcher (notify crate, already in workspace)
- Add debounce timer
- Reuse existing upload endpoint

## Priority

After first customer deployment. Push works for now.
