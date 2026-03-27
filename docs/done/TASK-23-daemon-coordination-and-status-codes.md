# TASK-23: Daemon Coordination and Status Codes

Discovered during 2026-03-19 testing of the single-path LLM extraction pipeline. Multiple code paths trigger indexing without coordination, causing duplicate LLM calls (wasted money), manifest races, and confusing user experience.

---

## Problem

Three code paths can trigger indexing:

1. **Daemon watch loop**: background indexing on file changes
2. **`hebbs index` command**: explicit Index command sent to daemon
3. **`hebbs init`**: prints "Indexing 36 file(s) in the background" and the daemon starts extracting

None of these check if another is already running. Result:
- Two concurrent LLM extraction passes on the same files (double the API cost)
- Timeouts when two extractors saturate the same API key
- Manifest race: last writer wins, can overwrite good state with stale data (partially fixed in commit f420c84, but concurrent extraction is still possible)

## Fix: Indexing Lock + Status Codes

### Indexing Lock

The daemon should have a single `IndexingLock` per vault. Before any code path starts phase2 (LLM extraction), it must acquire the lock. If the lock is held, the caller gets a clear status code instead of silently starting a second extraction.

```
IndexingLock per vault:
  - state: Idle | Indexing { started_at, triggered_by, files_done, total_files, current_file }
  - acquire() -> Ok(guard) | Err(AlreadyIndexing { status })
  - release() called when guard drops
```

### Behavior Changes

| Trigger | Current behavior | New behavior |
|---------|-----------------|--------------|
| `hebbs init` | Starts daemon, daemon auto-indexes in background | Starts daemon, does NOT auto-index. Prints "Run `hebbs index` to index your vault." |
| `hebbs index` | Sends Index command, daemon starts extracting regardless | Acquires IndexingLock. If locked: returns `INDEXING_IN_PROGRESS` with progress. If free: runs extraction. |
| Watch loop (file change) | Runs phase2 on debounce timer | Acquires IndexingLock. If locked: defers (re-arms timer). If free: runs extraction. |
| `hebbs status` | Shows snapshot from `IndexingSnapshot` | Shows structured status with status code. |

### Status Codes

Every daemon response should include a machine-readable status code. These are used by the CLI for display and by SDKs for programmatic access.

#### Vault Status Codes

| Code | Name | Description |
|------|------|-------------|
| `VAULT_READY` | Ready | Vault is initialized and all sections are synced. |
| `VAULT_NOT_INITIALIZED` | Not initialized | No `.hebbs/` directory found. Run `hebbs init`. |
| `VAULT_NEEDS_INDEX` | Needs indexing | Vault is initialized but has never been indexed. Run `hebbs index`. |
| `VAULT_PARTIALLY_INDEXED` | Partially indexed | Some sections are synced, some are stale (interrupted index). |
| `VAULT_LLM_NOT_CONFIGURED` | LLM not configured | Vault exists but LLM provider is not set. Run `hebbs config set llm.provider`. |

#### Indexing Status Codes

| Code | Name | Description |
|------|------|-------------|
| `INDEXING_IDLE` | Idle | No indexing in progress. |
| `INDEXING_PHASE1` | Parsing | Phase 1: parsing files, computing checksums. |
| `INDEXING_PHASE2` | Extracting | Phase 2: LLM extraction and embedding. Includes `files_done`, `total_files`, `current_file`. |
| `INDEXING_IN_PROGRESS` | Already running | Returned when a second indexing request arrives while one is active. Includes progress of the active run. |
| `INDEXING_COMPLETE` | Complete | Indexing finished successfully. Includes stats. |
| `INDEXING_FAILED` | Failed | Indexing failed. Includes error message. |

#### Operation Status Codes

| Code | Name | Description |
|------|------|-------------|
| `OK` | Success | Operation completed successfully. |
| `ERR_LLM_REQUIRED` | LLM required | Operation requires LLM but none is configured. |
| `ERR_LLM_TIMEOUT` | LLM timeout | LLM call timed out. May need to increase `llm.timeout_secs`. |
| `ERR_LLM_AUTH` | Auth failed | LLM API key is invalid or expired. Check your API key. |
| `ERR_LLM_RATE_LIMITED` | Rate limited | LLM provider rate limited. Will retry automatically. |
| `ERR_MANIFEST_CORRUPT` | Manifest corrupt | Manifest failed to parse. Run `hebbs rebuild` to recover. |
| `ERR_ENGINE_UNAVAILABLE` | Engine unavailable | RocksDB engine could not be opened (locked by another process?). |

### CLI Display

Status codes drive user-facing messages:

```
$ hebbs status
Vault: /path/to/vault
Status: VAULT_READY
Files: 36 (all indexed)
Memories: 467 propositions, 36 documents
LLM: openai/gpt-4o-mini

$ hebbs index  (while another index is running)
Indexing already in progress (12/36 files, currently: ideas/sell-the-outcome.md).
Run `hebbs status` to check progress.

$ hebbs status  (during indexing)
Vault: /path/to/vault
Status: INDEXING_PHASE2
Progress: 12/36 files (ideas/sell-the-outcome.md)
LLM: openai/gpt-4o-mini
```

### Error Messages

All error messages should be actionable. No "Is X running?" guessing. Pattern:

```
Error: LLM validation failed. Check your API key. (HTTP 401)
Error: LLM validation failed. Is Ollama running? (`ollama serve`) (connection refused)
Error: Indexing already in progress. Run `hebbs status` to check progress.
Error: LLM not configured. Run `hebbs init` with --provider/--model.
```

## Implementation Plan

### Step 1: IndexingLock

Add to `daemon/mod.rs`:
- `IndexingLock` struct wrapping `Arc<Mutex<Option<IndexingState>>>` per vault
- `IndexingState { triggered_by: String, started_at: Instant, files_done: usize, total_files: usize, current_file: String }`
- Acquire in Index handler and watch loop phase2 before starting extraction
- Release on completion or error

### Step 2: Remove auto-index from init

In `bin/hebbs.rs`, after `init_with_llm` succeeds:
- Remove "Indexing N file(s) in the background" message
- Print "Run `hebbs index /path` to index your vault."
- Daemon starts but does not trigger background indexing for the new vault

### Step 3: Status codes in daemon protocol

Add `status_code: String` field to `DaemonResponse`. All handlers set it. CLI reads it for display logic.

### Step 4: Actionable error messages

Audit all `VaultError` and daemon error paths. Replace generic messages with actionable ones that include:
- What went wrong
- Why (the specific error)
- What to do about it

## Files to Modify

| File | Change |
|------|--------|
| `hebbs-vault/src/daemon/mod.rs` | Add IndexingLock, acquire in Index handler and watch loop |
| `hebbs-vault/src/bin/hebbs.rs` | Remove auto-index from init, update CLI display for status codes |
| `hebbs-vault/src/lib.rs` | Fix error messages (partially done: 401 detection) |
| `hebbs-vault/src/ingest.rs` | Return structured status codes from phase2 |
| `hebbs-vault/src/error.rs` | Add status code variants to VaultError |

## Verification

1. `hebbs init` does NOT trigger background indexing
2. `hebbs index` while another index is running returns `INDEXING_IN_PROGRESS` with progress
3. Watch loop defers if explicit index is running
4. `hebbs status` shows structured status with correct code
5. Killing daemon and restarting preserves manifest state (already fixed)
6. All error messages are actionable (no "Is X running?" for cloud providers)
