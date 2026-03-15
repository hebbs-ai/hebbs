#!/usr/bin/env bash
set -euo pipefail

# Scenario 20: Concurrent Access Through Daemon
# Tests that the single-daemon model handles concurrent writes safely:
#   1. Spawn N parallel `hebbs remember` calls to the same vault
#   2. All succeed (no corruption, no RocksDB lock errors)
#   3. Recall all N memories, verify all present
#   4. Parallel recalls while writes are in progress

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="20_daemon_concurrent"
echo "=== Scenario 20: Concurrent Access Through Daemon ==="

# --- Setup ---
FAKE_HOME=$(mktemp -d "/tmp/hb-conc.XXXXXX")
export HOME="$FAKE_HOME"
VAULT_DIR="${FAKE_HOME}/test-project"
mkdir -p "$VAULT_DIR"

cleanup() {
  daemon_stop 2>/dev/null || true
  rm -rf "$FAKE_HOME"
}
trap cleanup EXIT

"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null
"${HEBBS_BIN}" init "$VAULT_DIR" 2>/dev/null

daemon_start 600

# =========================================================================
# 1. Parallel writes
# =========================================================================
echo ""
echo "--- 1. Parallel writes (5 concurrent) ---"

PARALLEL_COUNT=5
RESULTS_DIR=$(mktemp -d)

timer_start "parallel_writes"

for i in $(seq 1 $PARALLEL_COUNT); do
  (
    OUT=$("${HEBBS_BIN}" remember "Concurrent memory number ${i}: unique content ${RANDOM}" \
      --importance 0.$((i + 3)) --entity-id concurrent_test --format json \
      --vault "$VAULT_DIR" 2>/dev/null) || OUT=""
    echo "$OUT" > "$RESULTS_DIR/write_${i}.json"
  ) &
done

# Wait for all background jobs
wait

timer_stop "parallel_writes"

# Check all writes succeeded
WRITE_SUCCESS=0
for i in $(seq 1 $PARALLEL_COUNT); do
  if [[ -f "$RESULTS_DIR/write_${i}.json" ]]; then
    CONTENT=$(cat "$RESULTS_DIR/write_${i}.json")
    ID=$(echo "$CONTENT" | jq -r '.memory_id' 2>/dev/null || echo "")
    if [[ -n "$ID" && "$ID" != "null" ]]; then
      WRITE_SUCCESS=$((WRITE_SUCCESS + 1))
    fi
  fi
done

assert_eq "all_parallel_writes_succeeded" "$WRITE_SUCCESS" "$PARALLEL_COUNT"

echo "  $WRITE_SUCCESS / $PARALLEL_COUNT writes succeeded"

# =========================================================================
# 2. Verify all memories are present
# =========================================================================
echo ""
echo "--- 2. Verify all memories present ---"

RECALL_ALL=$("${HEBBS_BIN}" recall "Concurrent memory" \
  --strategy similarity --top-k 10 --format json --vault "$VAULT_DIR" 2>/dev/null)
RECALL_ALL_COUNT=$(echo "$RECALL_ALL" | jq 'length')

# Should have at least PARALLEL_COUNT results
assert_pass "all_memories_recallable" "$( [ "$RECALL_ALL_COUNT" -ge "$PARALLEL_COUNT" ] && echo 0 || echo 1 )"

echo "  $RECALL_ALL_COUNT memories recallable (expected >= $PARALLEL_COUNT)"

# =========================================================================
# 3. Parallel reads
# =========================================================================
echo ""
echo "--- 3. Parallel reads (5 concurrent) ---"

timer_start "parallel_reads"

for i in $(seq 1 $PARALLEL_COUNT); do
  (
    OUT=$("${HEBBS_BIN}" recall "concurrent memory number $i" \
      --strategy similarity --top-k 3 --format json \
      --vault "$VAULT_DIR" 2>/dev/null) || OUT="[]"
    echo "$OUT" > "$RESULTS_DIR/read_${i}.json"
  ) &
done

wait

timer_stop "parallel_reads"

READ_SUCCESS=0
for i in $(seq 1 $PARALLEL_COUNT); do
  if [[ -f "$RESULTS_DIR/read_${i}.json" ]]; then
    COUNT=$(jq 'length' "$RESULTS_DIR/read_${i}.json" 2>/dev/null || echo "0")
    if [[ "$COUNT" -gt 0 ]]; then
      READ_SUCCESS=$((READ_SUCCESS + 1))
    fi
  fi
done

assert_eq "all_parallel_reads_succeeded" "$READ_SUCCESS" "$PARALLEL_COUNT"

echo "  $READ_SUCCESS / $PARALLEL_COUNT reads returned results"

# =========================================================================
# 4. Mixed parallel reads and writes
# =========================================================================
echo ""
echo "--- 4. Mixed parallel reads and writes ---"

timer_start "mixed_parallel"

# 3 writes + 3 reads simultaneously
for i in $(seq 1 3); do
  (
    "${HEBBS_BIN}" remember "Mixed test write ${i}: ${RANDOM}" \
      --importance 0.5 --entity-id mixed_test --format json \
      --vault "$VAULT_DIR" 2>/dev/null > "$RESULTS_DIR/mixed_write_${i}.json"
  ) &
done

for i in $(seq 1 3); do
  (
    "${HEBBS_BIN}" recall "concurrent memory" \
      --strategy similarity --top-k 3 --format json \
      --vault "$VAULT_DIR" 2>/dev/null > "$RESULTS_DIR/mixed_read_${i}.json"
  ) &
done

wait

timer_stop "mixed_parallel"

MIXED_WRITE_OK=0
for i in $(seq 1 3); do
  if [[ -f "$RESULTS_DIR/mixed_write_${i}.json" ]]; then
    ID=$(jq -r '.memory_id' "$RESULTS_DIR/mixed_write_${i}.json" 2>/dev/null || echo "")
    if [[ -n "$ID" && "$ID" != "null" ]]; then
      MIXED_WRITE_OK=$((MIXED_WRITE_OK + 1))
    fi
  fi
done

MIXED_READ_OK=0
for i in $(seq 1 3); do
  if [[ -f "$RESULTS_DIR/mixed_read_${i}.json" ]]; then
    COUNT=$(jq 'length' "$RESULTS_DIR/mixed_read_${i}.json" 2>/dev/null || echo "0")
    if [[ "$COUNT" -gt 0 ]]; then
      MIXED_READ_OK=$((MIXED_READ_OK + 1))
    fi
  fi
done

assert_eq "mixed_writes_ok" "$MIXED_WRITE_OK" "3"
assert_eq "mixed_reads_ok" "$MIXED_READ_OK" "3"

echo "  Mixed parallel: $MIXED_WRITE_OK writes, $MIXED_READ_OK reads succeeded"

# =========================================================================
# 5. Daemon still healthy after all concurrency
# =========================================================================
echo ""
echo "--- 5. Daemon health after concurrency ---"

# Brief pause to let any in-flight requests settle
sleep 1

if ! daemon_is_alive; then
  echo "  WARNING: daemon PID $_DAEMON_PID is no longer running"
  echo "  --- Daemon log (last 30 lines) ---"
  tail -30 "${HOME}/.hebbs/daemon-test.log" 2>/dev/null || echo "  (no log found)"
  echo "  --- End daemon log ---"
fi

assert_pass "daemon_survived_concurrency" "$( daemon_is_alive && echo 0 || echo 1 )"

# One more recall to verify no corruption
FINAL_RECALL=$("${HEBBS_BIN}" recall "test" \
  --strategy similarity --top-k 20 --format json --vault "$VAULT_DIR" 2>/dev/null)
FINAL_COUNT=$(echo "$FINAL_RECALL" | jq 'length')
assert_gt "final_recall_healthy" "$FINAL_COUNT" "0"

echo "  Daemon healthy, data intact"

# Cleanup temp results
rm -rf "$RESULTS_DIR"

# =========================================================================
# Summary
# =========================================================================

log_metric "parallel_writes_ms" "$(timer_get parallel_writes)" "ms"
log_metric "parallel_reads_ms" "$(timer_get parallel_reads)" "ms"
log_metric "mixed_parallel_ms" "$(timer_get mixed_parallel)" "ms"
log_metric "total_memories_written" "$((PARALLEL_COUNT + 3))" "count"

save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
