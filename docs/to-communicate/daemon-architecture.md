# The HEBBS Daemon: One Process, All Your Vaults

Status: to write

## Key Points

- **Problem solved**: Every CLI invocation used to cold-start the ONNX embedding model (100-500ms) and open RocksDB (50-100ms). An agent making 3-5 calls per turn paid 0.5-3s in pure overhead.

- **Solution**: A single long-lived daemon at `~/.hebbs/daemon.sock` serves all vaults on the machine. The ONNX model loads once. Vault handles open on demand and close after idle.

- **Auto-start**: The first CLI command auto-starts the daemon if it isn't running. No manual setup. No systemd. No launchd. The user never thinks about it.

- **Proactive vault watching**: When you run `hebbs init` on a new project, the daemon detects the registration in `vaults.json` and immediately opens the vault + starts its file watcher. No restart needed.

- **Multi-vault recall**: `hebbs recall --all` queries both the project vault and the global vault in parallel, merges results by score. An agent working in a project gets both project-specific and cross-project memories in one call.

- **Self-healing**: If a vault's `.hebbs/` directory disappears (accidental delete, git clean), the daemon detects it on the next health check and closes the handle gracefully. `hebbs init` + next command reopens cleanly.

- **Resource bounded**: Max 64 open vaults. Idle vaults evict after 5 minutes (configurable). LRU eviction if capacity is hit.

- **Idle shutdown**: Daemon shuts down after 5 minutes of no requests (configurable). Next command restarts it. Zero resource usage when not needed.

## Technical Flow

```
CLI command (e.g., hebbs recall "query" --vault ./project)
  |
  v
Is daemon running? (check ~/.hebbs/daemon.sock)
  |
  +-- No  --> Start daemon in background, wait for socket
  +-- Yes --> Connect via Unix socket
  |
  v
Daemon receives request
  |
  v
VaultManager.get_or_open(vault_path)
  |
  +-- Already open? Touch last_accessed, return handle
  +-- Not open? Open RocksDB, start file watcher, return handle
  |
  v
Execute operation (recall/remember/forget/prime/reflect)
  |
  v
Return JSON response over socket
```

## What Users Should Know

- `HEBBS_NO_DAEMON=1` forces direct mode (for CI, testing, or Windows fallback)
- First start after install is ~30s (downloads ONNX model). Subsequent starts are ~1s.
- The daemon is invisible. If you never think about it, it's working correctly.
- All daemon tests pass: auto-start, multi-vault, health/self-heal, concurrent access, watch integration, panel serving, live vault registration (scenarios 17-24, 92 assertions total).
