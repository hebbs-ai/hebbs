#!/usr/bin/env bash
set -euo pipefail

# Scenario 22: Multi-Vault Recall (--all flag)
# Tests Milestone 5 from PLAN-daemon:
#   1. Store memories in global vault and project vault
#   2. Recall with --all merges results from both vaults
#   3. Recall without --all only returns project vault results
#   4. Prime with --all merges results from both vaults
#   5. --all with --global only returns global results (no project vault)
#   6. Scores are properly sorted across vaults

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="22_multi_vault_recall"
echo "=== Scenario 22: Multi-Vault Recall (--all flag) ==="

# --- Setup: isolated HOME with global + project vault ---
FAKE_HOME=$(mktemp -d "/tmp/hb-mvr.XXXXXX")
export HOME="$FAKE_HOME"
PROJECT_DIR="${FAKE_HOME}/my-project"
mkdir -p "$PROJECT_DIR"

cleanup() {
  daemon_stop 2>/dev/null || true
  rm -rf "$FAKE_HOME"
}
trap cleanup EXIT

# Initialize global and project vaults
"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null
"${HEBBS_BIN}" init "$PROJECT_DIR" 2>/dev/null

# Start daemon
daemon_start 120

echo "  Global + project vaults initialized, daemon started"

# =========================================================================
# 1. Store memories in each vault
# =========================================================================
echo ""
echo "--- 1. Store memories in each vault ---"

# Global vault: user preferences
"${HEBBS_BIN}" remember "User always prefers dark mode in all applications" \
  --importance 0.9 --entity-id user_prefs --format json --global 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "User writes documentation in markdown format" \
  --importance 0.8 --entity-id user_prefs --format json --global 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "User timezone is Pacific Standard Time" \
  --importance 0.7 --entity-id user_prefs --format json --global 2>/dev/null > /dev/null

# Project vault: project-specific context
"${HEBBS_BIN}" remember "This project uses Python 3.12 with FastAPI framework" \
  --importance 0.9 --entity-id project_context --format json --vault "$PROJECT_DIR" 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "Database is PostgreSQL 16 with pgvector extension" \
  --importance 0.8 --entity-id project_context --format json --vault "$PROJECT_DIR" 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "All API responses use JSON format with snake_case keys" \
  --importance 0.7 --entity-id project_context --format json --vault "$PROJECT_DIR" 2>/dev/null > /dev/null

echo "  Stored 3 global + 3 project memories"

# =========================================================================
# 2. Recall with --all merges both vaults
# =========================================================================
echo ""
echo "--- 2. Recall --all merges both vaults ---"

RECALL_ALL=$("${HEBBS_BIN}" recall "What preferences and project details should I know?" \
  --strategy similarity --top-k 10 --format json --vault "$PROJECT_DIR" --all 2>/dev/null)
RECALL_ALL_COUNT=$(echo "$RECALL_ALL" | jq 'length')
RECALL_ALL_STR=$(echo "$RECALL_ALL" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')

# Should have results from BOTH vaults
assert_gt "all_recall_count" "$RECALL_ALL_COUNT" "3"
assert_contains_str "all_has_dark_mode" "$RECALL_ALL_STR" "dark mode"
assert_contains_str "all_has_python" "$RECALL_ALL_STR" "Python"

echo "  --all returned $RECALL_ALL_COUNT results from both vaults"

# =========================================================================
# 3. Recall without --all only returns project vault
# =========================================================================
echo ""
echo "--- 3. Recall without --all (project only) ---"

RECALL_PROJECT=$("${HEBBS_BIN}" recall "What preferences and project details?" \
  --strategy similarity --top-k 10 --format json --vault "$PROJECT_DIR" 2>/dev/null)
RECALL_PROJECT_STR=$(echo "$RECALL_PROJECT" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')

assert_contains_str "project_has_python" "$RECALL_PROJECT_STR" "Python"
assert_not_contains_str "project_no_dark_mode" "$RECALL_PROJECT_STR" "dark mode"

echo "  Project-only recall correctly isolated"

# =========================================================================
# 4. Recall without --all (global only)
# =========================================================================
echo ""
echo "--- 4. Recall without --all (global only) ---"

RECALL_GLOBAL=$("${HEBBS_BIN}" recall "What preferences and project details?" \
  --strategy similarity --top-k 10 --format json --global 2>/dev/null)
RECALL_GLOBAL_STR=$(echo "$RECALL_GLOBAL" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')

assert_contains_str "global_has_dark_mode" "$RECALL_GLOBAL_STR" "dark mode"
assert_not_contains_str "global_no_python" "$RECALL_GLOBAL_STR" "Python"

echo "  Global-only recall correctly isolated"

# =========================================================================
# 5. Prime with --all merges both vaults
# =========================================================================
echo ""
echo "--- 5. Prime --all merges both vaults ---"

# Store same entity_id in both vaults for prime test
"${HEBBS_BIN}" remember "Global coding standard: always use type hints" \
  --importance 0.8 --entity-id coding --format json --global 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "Project coding standard: use pydantic for validation" \
  --importance 0.8 --entity-id coding --format json --vault "$PROJECT_DIR" 2>/dev/null > /dev/null

PRIME_ALL=$("${HEBBS_BIN}" prime coding --max-memories 10 \
  --format json --vault "$PROJECT_DIR" --all 2>/dev/null)
PRIME_ALL_STR=$(echo "$PRIME_ALL" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')

assert_contains_str "prime_all_has_type_hints" "$PRIME_ALL_STR" "type hints"
assert_contains_str "prime_all_has_pydantic" "$PRIME_ALL_STR" "pydantic"

echo "  Prime --all returned results from both vaults"

# =========================================================================
# 6. Results are sorted by score
# =========================================================================
echo ""
echo "--- 6. Score ordering across vaults ---"

# Verify scores are monotonically decreasing
SCORES=$("${HEBBS_BIN}" recall "preferences and standards" \
  --strategy similarity --top-k 20 --format json --vault "$PROJECT_DIR" --all 2>/dev/null \
  | jq -r '.[].score')

PREV_SCORE="999.0"
SORTED_OK=true
while IFS= read -r score; do
  if (( $(echo "$score > $PREV_SCORE" | bc -l 2>/dev/null || echo "0") )); then
    SORTED_OK=false
    break
  fi
  PREV_SCORE="$score"
done <<< "$SCORES"

assert_pass "scores_sorted_descending" "$( $SORTED_OK && echo 0 || echo 1 )"

echo "  Scores correctly sorted across vaults"

# =========================================================================
# 7. Daemon health after multi-vault operations
# =========================================================================
echo ""
echo "--- 7. Daemon health after multi-vault operations ---"

assert_pass "daemon_healthy" "$( daemon_is_alive && echo 0 || echo 1 )"

echo "  Daemon healthy after all multi-vault operations"

# =========================================================================
# Summary
# =========================================================================

log_metric "global_memories" "4" "count"
log_metric "project_memories" "4" "count"

save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
