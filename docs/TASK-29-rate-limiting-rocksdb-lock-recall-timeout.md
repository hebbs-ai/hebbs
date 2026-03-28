# TASK-29: Rate Limiting, RocksDB Lock, Recall Timeout

**Status:** Fixed
**Priority:** Critical (blocks all Tier 1 OpenAI users from indexing)
**Reported by:** Customer on macOS Intel (v0.3.3)
**Created:** 2026-03-27

---

## Issue 1: Phase 2 fires all requests in parallel with no rate limiting

**Severity:** Critical (100% failure rate on Tier 1 OpenAI accounts)

When indexing 26 files (374 sections), HEBBS sends hundreds of OpenAI API requests simultaneously. On Tier 1 accounts (low RPM/TPM limits), this triggers HTTP 429 on every request. Retry logic gives up after a few retries. Result: 0 memories created, 100% failure.

Daemon log:
```
extraction request failed: LLM provider error: exhausted retries: http status: 429
# (repeated 374+ times, all at the same timestamp)
failed to create Document memory for CLAUDE.md: embedding error: inference failed: exhausted retries: http status: 429
```

**Fixed:**
1. `--max-concurrent` flag on `hebbs init` (default: 10, lower to 1-2 for Tier 1)
2. `[api] max_concurrent_requests` in config.toml
3. `complete_parallel()` accepts configurable concurrency
4. Max retries increased from 3 to 6
5. Exponential backoff with jitter to prevent thundering herd
6. Extra 5-40s sleep on 429 with user-visible message

---

## Issue 2: RocksDB LOCK file prevents restart after crash

**Severity:** High (requires manual intervention to recover)

After force-kill or unclean shutdown, RocksDB LOCK file persists. All commands fail:
```
Error setting up engine: storage I/O error in open: failed to open RocksDB at .hebbs/index/db:
IO error: While lock file: .hebbs/index/db/LOCK: Resource temporarily unavailable
```

Only recovery: `rm -f .hebbs/index/db/LOCK`

**Fixed:**
1. Stale PID cleanup extended to also clean socket file
2. PID reuse detection (checks process name on macOS + Linux)
3. RocksDB lock error triggers stale lock detection: if daemon PID is dead, removes LOCK and retries
4. If daemon is alive, clear error: "Another HEBBS daemon is likely running. Only one daemon can access the database at a time."

---

## Issue 3: `hebbs recall` hangs indefinitely when daemon is in bad state

**Severity:** High (UX dead end, requires kill -9)

After failed indexing, `hebbs recall "any query"` hangs forever. No output, no timeout, no error.

**Fixed:**
1. `send()` now uses 30s default timeout
2. `send_with_timeout()` for custom timeouts
3. Clear error on timeout: "Request timed out after Xs. The daemon may be unresponsive. Check status with `hebbs status`."

---

## Reproduction

```sh
brew install hebbs-ai/tap/hebbs
mkdir test && cd test && cp some_md_files_here/
hebbs init . --provider openai --key sk-proj-<tier-1-key>
hebbs index .          # parses OK but 0 memories
hebbs recall "anything"  # hangs forever
kill -9 $(pgrep hebbs)
hebbs index .          # RocksDB LOCK error
```
