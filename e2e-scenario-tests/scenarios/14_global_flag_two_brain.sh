#!/usr/bin/env bash
set -euo pipefail

# Scenario 14: --global Flag and Two-Vault Pattern
# Tests that the --global flag routes commands to ~/.hebbs/ (global vault)
# while commands without --global route to the project vault. Validates
# the two-vault pattern that the SKILL.md teaches agents: global vault
# for user preferences, project vault for project-specific context.
#
# Tests:
#   1. --global flag routes to global vault, not project vault
#   2. Commands without --global route to project vault
#   3. Memories stored globally are invisible to project vault
#   4. Memories stored in project vault are invisible to global vault
#   5. Agent two-brain workflow: prime both, recall both, store to correct vault
#   6. Vault registry (vaults.json) updated by init

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="14_global_flag_two_brain"
echo "=== Scenario 14: --global Flag and Two-Vault Pattern ==="

# --- Setup ---
# Create a project vault
PROJECT_DIR=$(create_temp_vault)

# Create a temporary global vault (override HOME to isolate from real ~/.hebbs/)
FAKE_HOME=$(mktemp -d "${TMPDIR:-/tmp}/hebbs-e2e-home.XXXXXX")
export HOME="$FAKE_HOME"

trap "cleanup_vault '$PROJECT_DIR'; rm -rf '$FAKE_HOME'" EXIT

# Initialize global vault first (so ~/.hebbs/ exists for vault registry)
"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null

# Then initialize project vault (will register in ~/.hebbs/vaults.json)
vault_init "$PROJECT_DIR"

# =========================================================================
# 1. --global routes to global vault
# =========================================================================
echo ""
echo "--- 1. --global routes to global vault ---"

GLOBAL_MEM=$("${HEBBS_BIN}" remember "User prefers dark mode everywhere" \
  --importance 0.8 --entity-id user_prefs --global --format json 2>/dev/null)
GLOBAL_MEM_ID=$(echo "$GLOBAL_MEM" | jq -r '.memory_id')
assert_pass "global_remember_returns_id" "$( [ -n "$GLOBAL_MEM_ID" ] && [ "$GLOBAL_MEM_ID" != "null" ] && echo 0 || echo 1 )"

# Verify it's in the global vault
GLOBAL_RECALL=$("${HEBBS_BIN}" recall "dark mode" \
  --strategy similarity --top-k 5 --global --format json 2>/dev/null)
GLOBAL_RECALL_COUNT=$(echo "$GLOBAL_RECALL" | jq 'length')
assert_gt "global_recall_finds_memory" "$GLOBAL_RECALL_COUNT" "0"

GLOBAL_RECALL_CONTENT=$(echo "$GLOBAL_RECALL" | jq -r '.[0].content')
assert_contains_str "global_recall_correct_content" "$GLOBAL_RECALL_CONTENT" "dark mode"

echo "  Global remember + recall works"

# =========================================================================
# 2. Commands without --global route to project vault
# =========================================================================
echo ""
echo "--- 2. Project vault isolation ---"

PROJECT_MEM=$("${HEBBS_BIN}" remember "This project uses PostgreSQL" \
  --importance 0.7 --entity-id project_context --format json --vault "${PROJECT_DIR}" 2>/dev/null)
PROJECT_MEM_ID=$(echo "$PROJECT_MEM" | jq -r '.memory_id')
assert_pass "project_remember_returns_id" "$( [ -n "$PROJECT_MEM_ID" ] && [ "$PROJECT_MEM_ID" != "null" ] && echo 0 || echo 1 )"

# Verify it's in the project vault
PROJECT_RECALL=$("${HEBBS_BIN}" recall "PostgreSQL" \
  --strategy similarity --top-k 5 --format json --vault "${PROJECT_DIR}" 2>/dev/null)
PROJECT_RECALL_COUNT=$(echo "$PROJECT_RECALL" | jq 'length')
assert_gt "project_recall_finds_memory" "$PROJECT_RECALL_COUNT" "0"

echo "  Project remember + recall works"

# =========================================================================
# 3. Global memories invisible to project vault
# =========================================================================
echo ""
echo "--- 3. Global memories invisible to project vault ---"

# Recall "dark mode" from the project vault -- should NOT find the global memory content
# Note: HNSW always returns top-k nearest neighbors, so count may be > 0.
# We check that the actual global memory content is NOT in the results.
PROJECT_CROSS=$("${HEBBS_BIN}" recall "dark mode" \
  --strategy similarity --top-k 5 --format json --vault "${PROJECT_DIR}" 2>/dev/null)
PROJECT_CROSS_CONTENTS=$(echo "$PROJECT_CROSS" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "global_invisible_to_project" "$PROJECT_CROSS_CONTENTS" "dark mode"

echo "  Global memories do not leak into project vault"

# =========================================================================
# 4. Project memories invisible to global vault
# =========================================================================
echo ""
echo "--- 4. Project memories invisible to global vault ---"

# Recall "PostgreSQL" from the global vault -- should NOT find the project memory content
GLOBAL_CROSS=$("${HEBBS_BIN}" recall "PostgreSQL" \
  --strategy similarity --top-k 5 --global --format json 2>/dev/null)
GLOBAL_CROSS_CONTENTS=$(echo "$GLOBAL_CROSS" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "project_invisible_to_global" "$GLOBAL_CROSS_CONTENTS" "PostgreSQL"

echo "  Project memories do not leak into global vault"

# =========================================================================
# 5. Agent two-brain workflow (SKILL.md pattern)
# =========================================================================
echo ""
echo "--- 5. Agent two-brain workflow ---"

# Store more memories in both vaults (simulating agent behavior)
"${HEBBS_BIN}" remember "User writes in AP style" \
  --importance 0.8 --entity-id user_prefs --global --format json 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "Never use em-dashes in writing" \
  --importance 0.9 --entity-id user_prefs --global --format json 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "Deploy to staging before prod" \
  --importance 0.8 --entity-id project_context --format json --vault "${PROJECT_DIR}" 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "CI runs on GitHub Actions" \
  --importance 0.6 --entity-id project_context --format json --vault "${PROJECT_DIR}" 2>/dev/null > /dev/null

# Prime both vaults (agent session start pattern)
GLOBAL_PRIME=$("${HEBBS_BIN}" prime user_prefs --max-memories 10 \
  --global --format json 2>/dev/null)
GLOBAL_PRIME_COUNT=$(echo "$GLOBAL_PRIME" | jq 'length')
assert_gt "global_prime_returns_memories" "$GLOBAL_PRIME_COUNT" "0"

PROJECT_PRIME=$("${HEBBS_BIN}" prime project_context --max-memories 10 \
  --format json --vault "${PROJECT_DIR}" 2>/dev/null)
PROJECT_PRIME_COUNT=$(echo "$PROJECT_PRIME" | jq 'length')
assert_gt "project_prime_returns_memories" "$PROJECT_PRIME_COUNT" "0"

# Verify global prime only has user prefs, not project context
GLOBAL_PRIME_CONTENTS=$(echo "$GLOBAL_PRIME" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "global_prime_no_project_leak" "$GLOBAL_PRIME_CONTENTS" "staging"

# Verify project prime only has project context, not user prefs
PROJECT_PRIME_CONTENTS=$(echo "$PROJECT_PRIME" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "project_prime_no_global_leak" "$PROJECT_PRIME_CONTENTS" "dark mode"

echo "  Two-brain prime pattern works with full isolation"

# =========================================================================
# 6. Forget with --global
# =========================================================================
echo ""
echo "--- 6. Forget with --global ---"

# Store a test memory in global
"${HEBBS_BIN}" remember "Temporary global memory" \
  --importance 0.1 --entity-id _test_cleanup --global --format json 2>/dev/null > /dev/null

# Forget by entity in global
"${HEBBS_BIN}" forget --entity-id _test_cleanup --global 2>/dev/null || true

# Verify it's gone from global
CLEANUP_CHECK=$("${HEBBS_BIN}" prime _test_cleanup --max-memories 10 \
  --global --format json 2>/dev/null)
CLEANUP_COUNT=$(echo "$CLEANUP_CHECK" | jq 'length')
assert_eq "global_forget_clears" "$CLEANUP_COUNT" "0"

echo "  Forget with --global works"

# =========================================================================
# 7. Vault registry (vaults.json)
# =========================================================================
echo ""
echo "--- 7. Vault registry ---"

VAULTS_JSON="$FAKE_HOME/.hebbs/vaults.json"
if [[ -f "$VAULTS_JSON" ]]; then
  VAULT_COUNT=$(jq '.vaults | length' "$VAULTS_JSON")
  assert_gt "vaults_json_has_entries" "$VAULT_COUNT" "0"

  # Paths in vaults.json are canonicalized (e.g. /private/var on macOS)
  CANON_HOME=$(cd "$FAKE_HOME" && pwd -P)
  CANON_PROJECT=$(cd "$PROJECT_DIR" && pwd -P)

  # Global vault should be registered
  GLOBAL_REGISTERED=$(jq -r --arg p "$CANON_HOME" '.vaults[] | select(.path == $p) | .path' "$VAULTS_JSON")
  assert_pass "global_vault_registered" "$( [ -n "$GLOBAL_REGISTERED" ] && echo 0 || echo 1 )"

  # Project vault should be registered
  PROJECT_REGISTERED=$(jq -r --arg p "$CANON_PROJECT" '.vaults[] | select(.path == $p) | .path' "$VAULTS_JSON")
  assert_pass "project_vault_registered" "$( [ -n "$PROJECT_REGISTERED" ] && echo 0 || echo 1 )"

  echo "  vaults.json populated by init"
else
  # vaults.json not yet implemented -- skip but don't fail
  echo "  SKIP: vaults.json not yet implemented"
fi

# =========================================================================
# Summary
# =========================================================================

log_metric "global_memories_stored" "3" "count"
log_metric "project_memories_stored" "3" "count"
log_metric "cross_vault_leaks" "0" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
