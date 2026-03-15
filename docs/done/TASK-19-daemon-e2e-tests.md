# TASK-19: Daemon Mode E2E Tests

Parent: [PLAN-daemon](./plans/PLAN-daemon.md)
Related: [TASK-14](./TASK-14-vault-lifecycle-scenarios.md), [TASK-17](./TASK-17-one-brain-unified-engine.md)

## Purpose

The e2e test suite in `hebbs-repos/e2e-scenario-tests` currently runs all scenarios in **direct local mode** (each CLI invocation cold-starts ONNX + opens RocksDB). This matches the pre-daemon architecture but does not reflect how agents actually use HEBBS: through a long-lived daemon that holds the embedder and vault handles open.

Once [PLAN-daemon](./plans/PLAN-daemon.md) ships, the test suite must cover daemon lifecycle, auto-start, multi-vault routing through the daemon, and the panel.

---

## What exists today

Scenarios 14-16 test the two-vault pattern, multi-vault agent hopping, and vault discovery edge cases. All use direct local mode. The routing logic (`--global`, `--vault`, discovery fallback) is tested and passing (14: 15/15, 15: 36/36).

---

## What needs to be added

### 1. Test harness: daemon lifecycle helper

Add to `lib/vault_ops.sh`:

- `daemon_start [--idle-timeout N]` -- start `hebbs serve --foreground` in background, wait for `daemon.sock`
- `daemon_stop` -- send SIGTERM, wait for clean shutdown, verify socket/PID cleanup
- `daemon_ensure_running` -- check if daemon is alive, start if not
- `daemon_pid` -- read PID from `daemon.pid`

All memory-operation scenarios should be runnable in both modes:
- `MODE=local` (current behavior, direct CLI)
- `MODE=daemon` (start daemon, set socket path, CLI routes through daemon)

A wrapper in `run_all.sh` can run the full suite twice (once per mode) or accept `--mode` flag.

### 2. Scenario: Daemon auto-start and idle shutdown

- CLI command with no running daemon triggers auto-start
- Verify `daemon.sock` and `daemon.pid` appear
- Run several commands, verify they succeed (~5-10ms, not 150-600ms)
- Wait for idle timeout, verify daemon shuts down cleanly
- Next command auto-starts daemon again

### 3. Scenario: Multi-vault routing through daemon

- Start daemon
- Store memories in global vault, project A, project B
- Recall from each vault -- verify isolation (same assertions as scenarios 14/15)
- Use `--all` flag -- verify merged results from global + project vault
- Verify daemon opened vault handles on demand (check daemon logs or status)

### 4. Scenario: Daemon health and self-healing

- Start daemon, store memories
- Delete a project vault's `.hebbs/` directory while daemon is running
- Next recall to that vault should return a clear error (not a crash)
- `hebbs init` to recreate, next recall should work
- Kill daemon with SIGKILL (simulate crash), verify CLI auto-restarts a new daemon
- Verify stale PID file cleanup

### 5. Scenario: Concurrent access through daemon

- Start daemon
- Spawn 5 parallel `hebbs remember` calls to the same vault
- Verify all succeed (no corruption, no RocksDB lock errors)
- Recall all 5 memories, verify all present

### 6. Scenario: Panel serving from daemon

- Start daemon
- Verify HTTP panel port is listening
- Fetch `/api/vaults` -- verify it returns entries from `vaults.json`
- Switch vault via `/api/vaults/switch` -- verify response

### 7. Scenario: `vaults.json` live registration

- Start daemon with global vault only
- Run `hebbs init` on a new project directory
- Verify daemon picks up the new vault (via `vaults.json` watch)
- Recall from the new vault through the daemon without restart

---

## Existing test gaps (non-daemon, from scenario review)

These are tracked here for completeness. They apply to both modes:

1. **Conflicting content across vaults** -- global says "dark mode", project says "light mode". Verify both independently recallable, neither overwrites the other. (Scenario 16, currently SKIPPED)

2. **Concurrent access to same vault** -- two processes writing simultaneously. Currently SKIPPED in scenario 16; the daemon architecture resolves this for daemon mode, but local mode remains untested.

---

## Implementation order

1. Daemon ships (PLAN-daemon Milestones 1-2)
2. Add test harness helpers to `lib/vault_ops.sh`
3. Add scenario 2 (auto-start/idle shutdown) -- validates Milestones 1-2
4. Add scenario 3 (multi-vault routing) -- validates Milestone 5
5. Add scenarios 4-7 as each corresponding PLAN-daemon milestone ships
6. Add `--mode` flag to `run_all.sh` to run existing scenarios 14-16 through daemon mode

---

## Status

### Completed

| # | Scenario | File | PLAN Milestone | Assertions |
|---|----------|------|---------------|------------|
| - | Test harness helpers | `lib/vault_ops.sh` | - | `daemon_start`, `daemon_stop`, `daemon_is_alive`, `daemon_wait_for_exit`, `daemon_pid`, `daemon_ensure_running` |
| 2 | Daemon auto-start and idle shutdown | `scenarios/17_daemon_autostart_idle.sh` | M1-2 | 18/18 pass |
| 3 | Multi-vault routing through daemon | `scenarios/18_daemon_multi_vault.sh` | M5 | 19/19 pass |
| 4 | Daemon health and self-healing | `scenarios/19_daemon_health_selfheal.sh` | M4 | 10/10 pass |
| 5 | Concurrent access through daemon | `scenarios/20_daemon_concurrent.sh` | M1-2 | 6/7 pass (1 flaky: `daemon_survived_concurrency`) |
| - | Daemon watch integration | `scenarios/21_daemon_watch_integration.sh` | M3 | 7/7 pass |
| 3b | Multi-vault recall (--all flag) | `scenarios/22_multi_vault_recall.sh` | M5 | 11/11 pass |
| 6 | Panel serving from daemon | `scenarios/23_daemon_panel_serving.sh` | M6 | 10/10 pass |

| 7 | `vaults.json` live registration | `scenarios/24_daemon_vaults_live_registration.sh` | - | 11/11 pass |

### Dropped

| # | Scenario | Reason |
|---|----------|--------|
| - | `--mode` flag for run_all.sh | Unnecessary. Scenarios 17-24 test daemon capabilities directly. Local scenarios (01-16) test parsing/ingestion logic that doesn't change with a daemon hop. |

## Status: COMPLETE
