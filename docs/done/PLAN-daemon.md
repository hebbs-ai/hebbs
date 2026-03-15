# PLAN: Unified Daemon Architecture

Related: [PLAN-12](./PLAN-12.md), [TASK-14](../TASK-14-vault-lifecycle-scenarios.md), [TASK-19](../TASK-19-daemon-e2e-tests.md)

---

## Problem

The `hebbs` CLI currently has two modes, both broken in different ways:

1. **Local mode (default):** Every invocation cold-starts the ONNX embedding model (~100-500ms) and opens RocksDB (~50-100ms). An agent making 3-5 calls per turn pays 0.5-3s in pure overhead. Unusable at interactive latency.

2. **Remote mode (`--endpoint`):** Connects to `hebbs-server` over gRPC. Fast queries, but the server is a manually-managed daemon with no self-healing. When the data directory disappeared under a running server, OpenClaw went down silently (see `docs/openclaw-e2e-findings.md:26-31`).

Additionally, a per-vault daemon model does not scale. A user with 10 open workspaces would spawn 10 daemon processes, each loading its own ONNX model (~50MB resident per instance), totaling ~500MB+ of duplicated memory.

---

## Design: One Daemon, Multiple Vaults

A single long-lived daemon process at `~/.hebbs/daemon.sock` serves all vaults on the machine. The ONNX model loads once. Vault-scoped RocksDB handles open on demand and close after idle.

```
~/.hebbs/
  daemon.sock              ← Unix domain socket (single daemon)
  daemon.pid               ← PID file for liveness checks
  global/                  ← global brain (user prefs, cross-project memories)
  vaults.json              ← known vault paths + labels (already implemented by hebbs init)

/project-a/.hebbs/         ← project-scoped brain (index, manifest, config)
/project-b/.hebbs/         ← project-scoped brain
```

### Architecture

```
Agent / User
    |
    v
hebbs CLI (thin client, same binary)
    |
    v  (Unix socket: ~/.hebbs/daemon.sock)
hebbs daemon (auto-started on first use)
    |--- ONNX embedder (loaded once, shared across all vaults)
    |--- vault manager
    |      |--- global vault (always open)
    |      |--- project vault A (open on demand, close after idle)
    |      |--- project vault B (open on demand, close after idle)
    |--- file watcher (watches all registered vaults)
    |--- idle shutdown timer
```

### CLI Request Flow

```
1. User runs:   hebbs recall --query "deploy config"
2. CLI resolves vault path (walk up for .hebbs/, or --vault, or --global)
3. CLI checks ~/.hebbs/daemon.sock
   3a. Socket alive (connect + health ping succeeds):
       → Send request with vault_path field
       → Print response
   3b. Socket dead or missing:
       → Fork: hebbs serve --daemonize
       → Poll daemon.sock with backoff (50ms, 100ms, 200ms, 400ms, 800ms)
       → Timeout at 2s if daemon fails to start
       → Send request, print response
4. Daemon routes request to correct vault's RocksDB instance
5. If vault not yet opened, daemon opens it (~50ms), adds to registry
```

### Vault Routing

The daemon resolves which vault(s) to query based on CLI flags:

| Flag | Behavior |
|---|---|
| `--global` | Routes to `~/.hebbs/global/` only |
| `--vault /path` | Routes to `/path/.hebbs/` only |
| (neither) | Routes to project vault (discovered by walking up from cwd), falls back to global |
| `--all` | Searches both project + global, merges and deduplicates results |

The `--all` merge uses the same scoring/ranking as a single-vault query. Results from both vaults are scored, sorted, and the top-k returned. No special merge logic needed; the engine's scoring weights handle it.

---

## Implementation

### Milestone 1: `hebbs serve` Command

Add a `Serve` variant to the CLI's `Commands` enum in `hebbs-vault/src/bin/hebbs.rs`.

```
hebbs serve [--foreground] [--idle-timeout 300]
```

The serve command:

1. Binds `~/.hebbs/daemon.sock` (Unix domain socket)
2. Writes `~/.hebbs/daemon.pid`
3. Loads ONNX embedder once (`OnnxEmbedder::new()`)
4. Opens global vault RocksDB (`~/.hebbs/global/`)
5. Enters event loop: accept connections, dispatch requests, watch files
6. On `--daemonize`: fork, setsid, redirect stdout/stderr to `~/.hebbs/daemon.log`, return immediately

**Protocol:** Length-prefixed JSON over Unix socket. Each request is a JSON object with `command`, `vault_path`, and command-specific fields. Response is a JSON object with `status` and `data`. This avoids pulling in a full gRPC/HTTP stack for local IPC.

```json
// Request
{"command": "recall", "vault_path": "/project-a", "query": "deploy", "strategy": "similarity", "top_k": 5}

// Response
{"status": "ok", "data": {"memories": [...]}}
```

Why JSON over Unix socket instead of gRPC:
- No port allocation or conflicts
- Filesystem-scoped (dies with user session)
- No TLS/auth needed for local-only communication
- Simpler dependency footprint (no tonic/hyper for local mode)
- gRPC via `hebbs-server` remains available for remote/cloud deployments

**New files:**
- `hebbs-vault/src/daemon.rs`: socket listener, request dispatch, vault manager
- `hebbs-vault/src/daemon/protocol.rs`: request/response types, serialization
- `hebbs-vault/src/daemon/vault_manager.rs`: open/close/idle-evict vault handles

**Modified files:**
- `hebbs-vault/src/bin/hebbs.rs`: add `Serve` command, add client-mode dispatch to all existing commands

### Milestone 2: Auto-Start from CLI

Modify the CLI dispatch path (`run()` in `hebbs.rs`) so that local-mode commands automatically start the daemon if it is not running.

```
async fn ensure_daemon() -> Result<UnixStream> {
    // Try connect
    match UnixStream::connect("~/.hebbs/daemon.sock") {
        Ok(stream) => {
            // Health ping
            send_ping(&stream)?;
            Ok(stream)
        }
        Err(_) => {
            // Check stale PID file
            cleanup_stale_pid("~/.hebbs/daemon.pid")?;
            // Fork daemon
            spawn_daemon()?;
            // Poll for socket with backoff
            poll_for_socket("~/.hebbs/daemon.sock", Duration::from_secs(2))?
        }
    }
}
```

Every command (remember, recall, forget, prime, reflect-prepare, reflect-commit, insights, inspect, list) goes through `ensure_daemon()` first, then sends the request over the socket. The only commands that bypass the daemon are `init`, `version`, and `serve` itself.

### Milestone 3: Merge `watch` into `serve`

The current `hebbs watch` command becomes a no-op alias for `hebbs serve`. The daemon always watches all registered vaults for file changes.

When a new vault is first accessed, the daemon:
1. Opens its RocksDB
2. Runs a startup diff scan (checksums vs manifest, same as current watch startup)
3. Registers a file watcher for `.md` changes
4. Adds the vault path to `~/.hebbs/vaults.json`

When a vault is idle-evicted:
1. File watcher for that vault is stopped
2. RocksDB handle is closed
3. Registry entry remains (so next access skips discovery)

### Milestone 4: Health Checks and Self-Healing

The daemon validates its own health on a periodic heartbeat (default: 30s):

1. **Data directory check:** For each open vault, verify `.hebbs/` directory exists and the RocksDB lock file is held by this process. If the directory vanished (the OpenClaw failure mode), close the handle, log an error, and mark the vault as unhealthy. Next CLI request to that vault gets a clear error: "vault data directory missing, run `hebbs init` to recreate".

2. **PID file check:** On startup, if `daemon.pid` exists but the PID is not alive, delete the stale PID and socket files before binding.

3. **Idle shutdown:** If no request has been received across all vaults for `--idle-timeout` seconds (default: 300), the daemon shuts down cleanly. Closes all RocksDB handles, removes `daemon.sock` and `daemon.pid`. Next CLI call auto-starts a fresh daemon.

4. **Graceful shutdown on signals:** SIGTERM and SIGINT trigger clean shutdown (flush RocksDB WAL, close handles, remove socket/PID files).

### Milestone 5: Multi-Vault Recall

Add `--all` flag to recall and prime commands. When set, the daemon queries both the project vault and the global vault, merges results by score, and returns the top-k.

Implementation: run two `engine.recall_for_tenant()` calls in parallel (tokio::join), concatenate results, sort by score, truncate to top_k.

Entity namespacing: global vault memories use `global/` prefix on entity_id. Project vault memories are unprefixed. This prevents entity collisions across vaults (e.g., both vaults have a "preferences" entity).

### Milestone 6: Panel Serving from Daemon

The daemon also serves the control panel's HTTP UI ([TASK-16](../done/TASK-16-memory-palace-control-panel.md)). One daemon process = query engine + file watcher + panel server. This avoids a third process for the panel.

The daemon serves the panel on a local HTTP port (default: 6381). The panel reads `vaults.json` for the vault dropdown and queries memories through the daemon's socket internally (no extra hop).

`hebbs panel` becomes a convenience command that opens the browser to `http://localhost:6381` after ensuring the daemon is running.

---

## What This Replaces

| Current | After |
|---|---|
| `hebbs-server` (separate binary, gRPC, manual lifecycle) | `hebbs serve` (same binary, auto-managed, Unix socket) |
| `hebbs watch` (file sync only, no query serving) | Merged into `hebbs serve` |
| `hebbs panel` (separate process for control panel) | Merged into `hebbs serve` (panel served on HTTP port) |
| `hebbs-cli` crate (gRPC client for remote mode) | Retained for remote/cloud mode only (`--endpoint`) |
| Cold-start local mode (150-600ms per call) | Eliminated (daemon keeps engine warm, ~5-10ms per query) |
| Per-vault daemon (N processes, N model loads) | Single daemon, one model load, N vault handles |
| Concurrent vault access (undefined behavior) | Single daemon serializes writes, concurrent reads safe |

---

## Interaction with Remote Mode

The `--endpoint` flag continues to work as before, bypassing the local daemon entirely and talking to `hebbs-server` over gRPC. This is the path for cloud deployments, multi-machine setups, and the managed HEBBS service.

The local daemon and remote mode are mutually exclusive per command. The CLI never proxies through the local daemon to reach a remote server.

---

## Rollout

1. **Milestone 1-2 first.** Get `hebbs serve` working and auto-starting. This immediately eliminates cold-start latency for all local users and agents. **DONE.**
2. **Milestone 3.** Merge watch into serve. Removes the confusing two-daemon situation (watch + serve). **DONE.**
3. **Milestone 4.** Health checks. Prevents the OpenClaw-class failure from recurring. **DONE.**
4. **Milestone 5.** Multi-vault recall. Enables the "global prefs + project context" workflow. **DONE.**
5. **Milestone 6.** Panel serving. One process for everything local. **DONE.**

Each milestone is independently shippable and testable. No milestone depends on changes to `hebbs-server`, `hebbs-core`, or `hebbs-storage`.

**After Milestone 2 ships:** Update SKILL.md to remove the "No server required" line and replace with "Auto-managed, zero config." The commands stay identical -- agents don't need to know about the daemon. The only visible change is that the first command in a session takes ~1s (daemon cold start) and subsequent commands are ~5-10ms.

---

## Open Questions (Resolved)

1. **Log rotation for daemon.log?** No rotation needed. Idle shutdown naturally bounds log size -- daemon restarts fresh and the client's `start_daemon()` opens the log in append mode. If someone disables idle shutdown for a long-running deployment, cap at 10MB with truncate-on-open. No external dependency.

2. **Multiple users on same machine?** Already isolated: `~/.hebbs/` is per-OS-user, so each user gets their own daemon and socket. For CI runners with a shared HOME, add a `HEBBS_RUNTIME_DIR` env override to relocate the socket/PID -- trivial to implement when needed.

3. **Embedding model updates?** Check model file mtime on the 30s health heartbeat. If changed, log a warning: "model updated, restart daemon to use new version." Do NOT hot-reload the ONNX session mid-flight (risky with in-flight embeddings). User kills daemon, next CLI command auto-starts with new model. Safe, simple.

4. **Should `hebbs init` auto-register with running daemon?** Already solved by design. `init` writes to `vaults.json`. The daemon opens vaults on demand via `get_or_open()` when the CLI sends a request with that vault path. No socket message or vault-watching needed -- the daemon doesn't need to proactively know about vaults.

5. **Windows support?** Deferred. Unix domain sockets work on Win10+ (AF_UNIX) but `setsid`/fork needs `CreateProcess` + `DETACHED_PROCESS`. Not worth the complexity until demand exists. `HEBBS_NO_DAEMON=1` fallback gives Windows users a working (slower) path today.

6. **Concurrent access is solved.** Confirmed by e2e scenario 20: 5 parallel writes, 5 parallel reads, and mixed read/write all succeed through the single daemon. RocksDB handles concurrent reads natively; the daemon serializes writes.

### Implementation Notes (from Milestone 1-2 build)

- **First-run startup takes ~30s** due to ONNX model download from Hugging Face. The auto-start polling timeout was increased from the planned 2s to 60s with continued backoff at 1.2s intervals to accommodate this. Subsequent starts take ~1s (model cached on disk).
- **`HEBBS_NO_DAEMON=1` env var** added to force direct local mode, useful for testing and as a Windows fallback.
- **Idle check interval** scales with timeout: `min(30s, timeout/2)` so short timeouts (e.g., 10s for tests) trigger promptly.
- **JSON output normalization**: daemon wraps recall/prime responses in `{"results":[...]}` internally, but the CLI unwraps to a plain array `[...]` for `--format json` to match the local-mode output shape. Agents and scripts see identical output regardless of daemon vs direct mode.

### Implementation Notes (from Milestone 3 build)

- **Per-vault file watchers**: each vault gets its own `notify::RecommendedWatcher` started in `VaultManager::start_watcher()`. Events are tagged with the vault path via `VaultFsEvent` and sent through a shared `mpsc::channel(2000)`. Watchers are automatically stopped when vaults are evicted (watcher handle dropped).
- **Watch event processing**: a background task (`run_watch_loop`) receives events from all vault watchers. Per-vault state (`VaultWatchState`) tracks pending creates/deletes, debounce timers, and the manifest. A 200ms polling tick checks all vaults' phase1/phase2 deadlines.
- **Startup catch-up**: when a vault receives its first filesystem event, the watch loop runs `find_changed_files()` to detect files changed since last manifest update, similar to the old standalone `hebbs watch` startup.
- **macOS FSEvents quirk**: on macOS, `notify` (FSEvents backend) batches events and may deliver `Modify` events after a `Remove` for the same file in the same batch. The fix: for Create/Modify events, check `path.exists()` before adding to `pending_creates`. If the file doesn't exist, treat it as a delete instead.
- **`hebbs watch` is now a daemon alias**: running `hebbs watch` auto-starts the daemon (if not running) and prints a message confirming the daemon is watching all open vaults. No separate watcher process needed.
- **Phase2 manifest sync bug (pre-existing, fixed)**: `phase2_ingest` was failing to mark sections as `Synced` after embedding because `engine.remember()` assigns a new memory ID, updating the manifest section's `memory_id`, but the `was_processed` check compared against the original (stale) ID. Fixed by tracking processed sections via a `HashSet<(rel_path, assigned_id)>`.
- **E2e test**: scenario 21 (`21_daemon_watch_integration.sh`) validates create, modify, delete, `hebbs watch` alias, and multi-vault watching through the daemon.

### Implementation Notes (from Milestone 4 build)

- **SIGTERM handler added**: daemon now handles both SIGINT (Ctrl-C) and SIGTERM (`kill`) via `tokio::signal::unix::SignalKind::terminate()`. Either signal triggers the same clean shutdown path.
- **Vault epoch marker**: `hebbs init` writes a random ULID to `.hebbs/epoch`. On each request, `VaultManager::get_or_open()` compares the on-disk epoch with the cached one. If the vault was re-initialized (epoch changed) or deleted (epoch file missing), the stale handle is evicted and the vault is reopened with a fresh engine. This prevents the "stale file handle" problem where the daemon holds open RocksDB handles to a deleted `.hebbs/` directory.
- **Phase2 manifest sync bug (pre-existing, fixed in M3)**: also fixed the root cause of scenario 19's `recall_after_sigkill_direct` failure -- data was being written to a stale (deleted) RocksDB via open file handles, then lost on SIGKILL.
- **Socket path length on macOS**: all daemon e2e tests now use short `/tmp/hb-*.XXXXXX` paths instead of `$TMPDIR` to avoid exceeding the 104-char Unix socket path limit on macOS.
- **E2e test**: scenario 19 (`19_daemon_health_selfheal.sh`) validates all 4 health scenarios: vault deletion survival, vault recreation recovery, SIGKILL + direct-mode fallback, and stale PID cleanup on restart. All 10 assertions pass.

### Implementation Notes (from Milestone 5 build)

- **`--all` flag on `recall` and `prime`**: added to CLI (`Commands::Recall` and `Commands::Prime` in `hebbs.rs`). When set, the CLI resolves both the project vault path and the global vault path (`~/.hebbs/`), sending both via `DaemonRequest.vault_paths`.
- **Protocol extension**: `DaemonRequest` gained an optional `vault_paths: Option<Vec<PathBuf>>` field (serde `skip_serializing_if` for backward compatibility). The daemon opens all specified vault engines via `VaultManager::get_or_open()`, runs parallel queries, merges results by score descending, and truncates to `top_k` (recall) or `max_memories` (prime).
- **No entity namespacing needed yet**: the PLAN mentioned `global/` prefix on entity_id for collision avoidance, but in practice `--all` queries use the same entity_id across vaults and the results are simply merged by score. Namespacing can be added later if entity collisions become a real issue.
- **Score type mismatch**: `engine.recall()` returns `f32` scores but the merge sort used `f64`. Fixed with `as f64` cast.
- **E2e test**: scenario 22 (`22_multi_vault_recall.sh`) validates: `--all` recall merges both vaults (6 results from 3+3), project-only and global-only isolation, `--all` prime merges both vaults, score ordering across vaults. All 11 assertions pass.

### Implementation Notes (from Milestone 6 build)

- **`panel_port` in `DaemonConfig`**: defaults to 6381. Set to 0 to disable panel serving. The daemon starts the panel HTTP server after binding the Unix socket and opening the global vault via `VaultManager`.
- **`start_panel_server_from_daemon()`**: new function in `panel/mod.rs` that accepts an `Arc<Engine>` and `Arc<TokioMutex<VaultManager>>` from the daemon. The engine is the global vault's engine (from `vault_manager.get_or_open(HOME)`). This shares the same engine instance used by daemon socket commands, so recall results are consistent.
- **`PanelState.vault_manager`**: optional field (`None` in standalone mode, `Some(...)` in daemon mode). Lays groundwork for vault switching via the panel API in a future iteration.
- **`hebbs panel` command updated**: tries to connect to the daemon first. If daemon is running, just opens browser to `http://127.0.0.1:6381` (panel already served by daemon). Falls back to standalone mode if daemon is unavailable.
- **`--panel-port` flag on `hebbs serve`**: passes through to `DaemonConfig.panel_port`.
- **Panel serves from global vault**: the graph view shows file-synced memories (from manifest), while the recall API shows all memories including API-stored ones. This matches the panel's design (graph = file structure, recall = semantic search).
- **Pre-existing panel build breakage fixed**: `load_pinned_positions()`, `compute_cluster_labels()`, `pin_position`, `unpin_position` were defined but not wired up. Fixed by re-enabling the route and removing duplicate stubs.
- **E2e test**: scenario 23 (`23_daemon_panel_serving.sh`) validates: HTTP port reachable, static files served (index.html, app.js, panel.css), vault status API, graph API, recall through panel, vault listing, dashboard API, Unix socket still works alongside HTTP. All 10 assertions pass.
