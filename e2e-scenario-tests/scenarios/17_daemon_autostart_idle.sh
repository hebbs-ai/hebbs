#!/usr/bin/env bash
set -euo pipefail

# Scenario 17: Daemon Auto-Start and Idle Shutdown
# Tests the daemon lifecycle from PLAN-daemon Milestones 1-2:
#   1. `hebbs serve --foreground` starts and creates socket + PID file
#   2. CLI commands succeed through the daemon (warm engine)
#   3. Daemon responds to ping (health check)
#   4. Short idle timeout causes clean shutdown
#   5. Socket and PID files are cleaned up after shutdown
#   6. CLI auto-starts daemon on next command (Milestone 2 fallback)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="17_daemon_autostart_idle"
echo "=== Scenario 17: Daemon Auto-Start and Idle Shutdown ==="

# --- Setup: isolated HOME ---
FAKE_HOME=$(mktemp -d "/tmp/hb-daemon.XXXXXX")
export HOME="$FAKE_HOME"
VAULT_DIR="${FAKE_HOME}/test-project"
mkdir -p "$VAULT_DIR"
DAEMON_SOCK="${FAKE_HOME}/.hebbs/daemon.sock"
DAEMON_PID_FILE="${FAKE_HOME}/.hebbs/daemon.pid"

cleanup() {
  daemon_stop 2>/dev/null || true
  rm -rf "$FAKE_HOME"
}
trap cleanup EXIT

# Initialize global vault (creates ~/.hebbs/) and project vault
"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null
"${HEBBS_BIN}" init "$VAULT_DIR" 2>/dev/null

# =========================================================================
# 1. Start daemon explicitly
# =========================================================================
echo ""
echo "--- 1. Start daemon explicitly ---"

daemon_start 10  # 10 second idle timeout for fast test

assert_path_exists "socket_created" "$DAEMON_SOCK"
assert_file_exists "pid_file_created" "$DAEMON_PID_FILE"

# Verify PID file contains a valid PID
PID_CONTENT=$(cat "$DAEMON_PID_FILE")
assert_pass "pid_file_has_pid" "$( [[ "$PID_CONTENT" =~ ^[0-9]+$ ]] && echo 0 || echo 1 )"

# Verify daemon process is alive
assert_pass "daemon_process_alive" "$( kill -0 "$PID_CONTENT" 2>/dev/null && echo 0 || echo 1 )"

echo "  Daemon started (PID: $PID_CONTENT)"

# =========================================================================
# 2. Commands through daemon
# =========================================================================
echo ""
echo "--- 2. Commands through daemon ---"

# Remember via daemon
timer_start "remember_via_daemon"
MEM1=$("${HEBBS_BIN}" remember "Daemon test memory: user prefers dark mode" \
  --importance 0.8 --entity-id prefs --format json --vault "$VAULT_DIR" 2>/dev/null)
timer_stop "remember_via_daemon"
MEM1_ID=$(echo "$MEM1" | jq -r '.memory_id')
assert_pass "remember_returns_id" "$( [ -n "$MEM1_ID" ] && [ "$MEM1_ID" != "null" ] && echo 0 || echo 1 )"

MEM2=$("${HEBBS_BIN}" remember "Daemon test memory: always use structured logging" \
  --importance 0.7 --entity-id prefs --format json --vault "$VAULT_DIR" 2>/dev/null)
MEM2_ID=$(echo "$MEM2" | jq -r '.memory_id')
assert_pass "remember_second_memory" "$( [ -n "$MEM2_ID" ] && [ "$MEM2_ID" != "null" ] && echo 0 || echo 1 )"

# Recall via daemon
timer_start "recall_via_daemon"
RECALL_OUT=$("${HEBBS_BIN}" recall "What are the user preferences?" \
  --strategy similarity --top-k 5 --format json --vault "$VAULT_DIR" 2>/dev/null)
timer_stop "recall_via_daemon"
RECALL_COUNT=$(echo "$RECALL_OUT" | jq 'length')
assert_gt "recall_returns_results" "$RECALL_COUNT" "0"

# Get via daemon
GET_OUT=$("${HEBBS_BIN}" get "$MEM1_ID" --format json --vault "$VAULT_DIR" 2>/dev/null)
GET_CONTENT=$(echo "$GET_OUT" | jq -r '.content')
assert_eq "get_returns_correct_memory" "$GET_CONTENT" "Daemon test memory: user prefers dark mode"

# Prime via daemon
PRIME_OUT=$("${HEBBS_BIN}" prime prefs --max-memories 10 --format json --vault "$VAULT_DIR" 2>/dev/null)
PRIME_COUNT=$(echo "$PRIME_OUT" | jq 'length')
assert_gt "prime_returns_memories" "$PRIME_COUNT" "0"

# Forget via daemon
TEMP=$("${HEBBS_BIN}" remember "Temporary memory to forget" \
  --importance 0.1 --entity-id _tmp --format json --vault "$VAULT_DIR" 2>/dev/null)
TEMP_ID=$(echo "$TEMP" | jq -r '.memory_id')
"${HEBBS_BIN}" forget --ids "$TEMP_ID" --vault "$VAULT_DIR" 2>/dev/null || true
GET_FORGOTTEN=$("${HEBBS_BIN}" get "$TEMP_ID" --format json --vault "$VAULT_DIR" 2>&1) || true
FORGOTTEN_CHECK=$(echo "$GET_FORGOTTEN" | grep -ci "error\|not found" || echo "0")
assert_gt "forget_via_daemon" "$FORGOTTEN_CHECK" "0"

# Status via daemon
STATUS_OUT=$("${HEBBS_BIN}" status --vault "$VAULT_DIR" 2>&1) || true
STATUS_FILE=$(mktemp)
echo "$STATUS_OUT" > "$STATUS_FILE"
assert_contains "status_via_daemon" "$STATUS_FILE" "indexed"
rm -f "$STATUS_FILE"

echo "  All operations succeeded through daemon"

# =========================================================================
# 3. Daemon latency check (warm engine should be fast)
# =========================================================================
echo ""
echo "--- 3. Latency (warm engine) ---"

REMEMBER_MS=$(timer_get "remember_via_daemon")
RECALL_MS=$(timer_get "recall_via_daemon")

log_metric "remember_latency_ms" "$REMEMBER_MS" "ms"
log_metric "recall_latency_ms" "$RECALL_MS" "ms"

# Warm engine calls should be well under 5 seconds (generous for CI)
assert_lt "remember_under_5s" "$REMEMBER_MS" "5000"
assert_lt "recall_under_5s" "$RECALL_MS" "5000"

# =========================================================================
# 4. Daemon still alive after operations
# =========================================================================
echo ""
echo "--- 4. Daemon still alive ---"

assert_pass "daemon_still_alive" "$( daemon_is_alive && echo 0 || echo 1 )"

# =========================================================================
# 5. Idle shutdown
# =========================================================================
echo ""
echo "--- 5. Idle shutdown ---"

echo "  Waiting for idle shutdown (10s timeout)..."
if daemon_wait_for_exit 20; then
  assert_pass "daemon_idle_shutdown" "0"
else
  assert_pass "daemon_idle_shutdown" "1"
fi

# Socket and PID file should be cleaned up
sleep 0.5
assert_file_not_exists "socket_cleaned_up" "$DAEMON_SOCK"
assert_file_not_exists "pid_cleaned_up" "$DAEMON_PID_FILE"

echo "  Daemon shut down cleanly"

# =========================================================================
# 6. CLI auto-start fallback
# =========================================================================
echo ""
echo "--- 6. Auto-start fallback ---"

# With daemon stopped, CLI should fall back to direct local mode
RECALL_FALLBACK=$("${HEBBS_BIN}" recall "preferences" \
  --strategy similarity --top-k 5 --format json --vault "$VAULT_DIR" 2>/dev/null)
FALLBACK_COUNT=$(echo "$RECALL_FALLBACK" | jq 'length')
assert_gt "fallback_recall_works" "$FALLBACK_COUNT" "0"

echo "  CLI fallback to direct mode works"

# =========================================================================
# Summary
# =========================================================================

log_metric "memories_stored" "3" "count"

save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
