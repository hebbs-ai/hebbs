#!/usr/bin/env bash
set -euo pipefail

# Scenario 18: Multi-Vault Routing Through Daemon
# Tests that the daemon correctly manages multiple vault handles:
#   1. Start daemon, store memories in global + project A + project B
#   2. Recall from each vault verifies isolation (no cross-vault leaks)
#   3. Same entity name in different vaults stays isolated
#   4. Daemon opens vault handles on demand
#   5. Status works for each vault independently

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="18_daemon_multi_vault"
echo "=== Scenario 18: Multi-Vault Routing Through Daemon ==="

# --- Setup: isolated HOME with three vaults ---
FAKE_HOME=$(mktemp -d "/tmp/hb-mv.XXXXXX")
export HOME="$FAKE_HOME"

PROJECT_A="${FAKE_HOME}/projects/frontend"
PROJECT_B="${FAKE_HOME}/projects/backend"
mkdir -p "$PROJECT_A" "$PROJECT_B"

cleanup() {
  daemon_stop 2>/dev/null || true
  rm -rf "$FAKE_HOME"
}
trap cleanup EXIT

# Initialize all three vaults
"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null
"${HEBBS_BIN}" init "$PROJECT_A" 2>/dev/null
"${HEBBS_BIN}" init "$PROJECT_B" 2>/dev/null

# Start daemon with a generous timeout
daemon_start 120

echo "  Three vaults initialized, daemon started"

# =========================================================================
# 1. Store project-specific memories in each vault
# =========================================================================
echo ""
echo "--- 1. Store memories in each vault ---"

# Global vault
"${HEBBS_BIN}" remember "User prefers dark mode everywhere" \
  --importance 0.8 --entity-id user_prefs --format json --global 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "Never use em-dashes in writing" \
  --importance 0.9 --entity-id user_prefs --format json --global 2>/dev/null > /dev/null

# Project A: frontend
"${HEBBS_BIN}" remember "Frontend uses React 19 with TypeScript" \
  --importance 0.8 --entity-id project_context --format json --vault "$PROJECT_A" 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "Use Tailwind CSS, never write custom CSS" \
  --importance 0.9 --entity-id project_context --format json --vault "$PROJECT_A" 2>/dev/null > /dev/null

# Project B: backend
"${HEBBS_BIN}" remember "Backend uses Rust with Axum framework" \
  --importance 0.8 --entity-id project_context --format json --vault "$PROJECT_B" 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "All API endpoints return JSON" \
  --importance 0.9 --entity-id project_context --format json --vault "$PROJECT_B" 2>/dev/null > /dev/null

echo "  Stored 2 memories in each vault (6 total)"

# =========================================================================
# 2. Vault isolation: recall from each vault independently
# =========================================================================
echo ""
echo "--- 2. Vault isolation ---"

# Recall from Project A: should get React/Tailwind, not Rust/Axum
RECALL_A=$("${HEBBS_BIN}" recall "What framework does this project use?" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_A" 2>/dev/null)
RECALL_A_STR=$(echo "$RECALL_A" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')
assert_contains_str "project_a_has_react" "$RECALL_A_STR" "React"
assert_not_contains_str "project_a_no_rust" "$RECALL_A_STR" "Rust"
assert_not_contains_str "project_a_no_dark_mode" "$RECALL_A_STR" "dark mode"

# Recall from Project B: should get Rust/Axum, not React/Tailwind
RECALL_B=$("${HEBBS_BIN}" recall "What framework does this project use?" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_B" 2>/dev/null)
RECALL_B_STR=$(echo "$RECALL_B" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')
assert_contains_str "project_b_has_rust" "$RECALL_B_STR" "Rust"
assert_not_contains_str "project_b_no_react" "$RECALL_B_STR" "React"
assert_not_contains_str "project_b_no_dark_mode" "$RECALL_B_STR" "dark mode"

# Recall from global: should get user prefs, not project context
RECALL_G=$("${HEBBS_BIN}" recall "What does the user prefer?" \
  --strategy similarity --top-k 5 --format json --global 2>/dev/null)
RECALL_G_STR=$(echo "$RECALL_G" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')
assert_contains_str "global_has_dark_mode" "$RECALL_G_STR" "dark mode"
assert_not_contains_str "global_no_react" "$RECALL_G_STR" "React"
assert_not_contains_str "global_no_rust" "$RECALL_G_STR" "Rust"

echo "  Vault isolation verified"

# =========================================================================
# 3. Same entity name, different vaults
# =========================================================================
echo ""
echo "--- 3. Same entity, different vaults ---"

# Both project vaults have entity "project_context" but different content
PRIME_A=$("${HEBBS_BIN}" prime project_context --max-memories 10 \
  --format json --vault "$PROJECT_A" 2>/dev/null)
PRIME_A_STR=$(echo "$PRIME_A" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')

PRIME_B=$("${HEBBS_BIN}" prime project_context --max-memories 10 \
  --format json --vault "$PROJECT_B" 2>/dev/null)
PRIME_B_STR=$(echo "$PRIME_B" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')

assert_contains_str "prime_a_has_react" "$PRIME_A_STR" "React"
assert_not_contains_str "prime_a_no_rust" "$PRIME_A_STR" "Rust"
assert_contains_str "prime_b_has_rust" "$PRIME_B_STR" "Rust"
assert_not_contains_str "prime_b_no_react" "$PRIME_B_STR" "React"

echo "  Same entity name isolated across vaults"

# =========================================================================
# 4. Status works for each vault
# =========================================================================
echo ""
echo "--- 4. Per-vault status ---"

STATUS_A=$("${HEBBS_BIN}" status --vault "$PROJECT_A" 2>&1) || true
STATUS_A_FILE=$(mktemp)
echo "$STATUS_A" > "$STATUS_A_FILE"
assert_contains "status_a_works" "$STATUS_A_FILE" "indexed"
rm -f "$STATUS_A_FILE"

STATUS_B=$("${HEBBS_BIN}" status --vault "$PROJECT_B" 2>&1) || true
STATUS_B_FILE=$(mktemp)
echo "$STATUS_B" > "$STATUS_B_FILE"
assert_contains "status_b_works" "$STATUS_B_FILE" "indexed"
rm -f "$STATUS_B_FILE"

echo "  Status works for each vault"

# =========================================================================
# 5. Sequential vault hopping (agent pattern)
# =========================================================================
echo ""
echo "--- 5. Agent vault hopping ---"

# Simulate an agent that hops: prime A, work in A, prime B, work in B, check global
"${HEBBS_BIN}" prime project_context --max-memories 5 --vault "$PROJECT_A" 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "Added new Button component with hover state" \
  --importance 0.5 --entity-id project_context --format json --vault "$PROJECT_A" 2>/dev/null > /dev/null

"${HEBBS_BIN}" prime project_context --max-memories 5 --vault "$PROJECT_B" 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "Added /api/users endpoint with pagination" \
  --importance 0.5 --entity-id project_context --format json --vault "$PROJECT_B" 2>/dev/null > /dev/null

# Verify new memories landed in correct vaults
RECALL_A2=$("${HEBBS_BIN}" recall "Button component" \
  --strategy similarity --top-k 3 --format json --vault "$PROJECT_A" 2>/dev/null)
RECALL_A2_STR=$(echo "$RECALL_A2" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')
assert_contains_str "hop_a_has_button" "$RECALL_A2_STR" "Button"

RECALL_B2=$("${HEBBS_BIN}" recall "users endpoint" \
  --strategy similarity --top-k 3 --format json --vault "$PROJECT_B" 2>/dev/null)
RECALL_B2_STR=$(echo "$RECALL_B2" | jq -r '.[].content' 2>/dev/null | tr '\n' ' ')
assert_contains_str "hop_b_has_endpoint" "$RECALL_B2_STR" "endpoint"

# Cross-check: Button not in B, endpoint not in A
assert_not_contains_str "hop_b_no_button" "$RECALL_B2_STR" "Button"
assert_not_contains_str "hop_a_no_endpoint" "$RECALL_A2_STR" "endpoint"

echo "  Agent vault hopping maintains isolation"

# =========================================================================
# Summary
# =========================================================================

log_metric "vaults_tested" "3" "count"
log_metric "total_memories" "8" "count"

save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
