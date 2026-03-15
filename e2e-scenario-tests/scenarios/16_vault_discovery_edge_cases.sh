#!/usr/bin/env bash
set -euo pipefail

# Scenario 16: Vault Discovery Edge Cases
# Tests vault discovery fallback, --vault override, conflicting content
# across vaults, and concurrent access patterns.
#
# Tests:
#   1. Discovery fallback to global when no .hebbs/ in tree (no --global flag)
#   2. --vault flag overrides discovery (in project A, target project B)
#   3. SKIP: Conflicting content across vaults (global vs project)
#   4. SKIP: Concurrent access to same vault

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="16_vault_discovery_edge_cases"
echo "=== Scenario 16: Vault Discovery Edge Cases ==="

# --- Setup ---
FAKE_HOME=$(mktemp -d "${TMPDIR:-/tmp}/hebbs-e2e-home.XXXXXX")
export HOME="$FAKE_HOME"

PROJECT_A="${FAKE_HOME}/projects/alpha"
PROJECT_B="${FAKE_HOME}/projects/beta"
BARE_DIR="${FAKE_HOME}/projects/bare-no-vault"
mkdir -p "$PROJECT_A/src/deep/nested"
mkdir -p "$PROJECT_B/src"
mkdir -p "$BARE_DIR/src/deep/nested"

trap "rm -rf '$FAKE_HOME'" EXIT

# Initialize global vault first
"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null

# Initialize project vaults
"${HEBBS_BIN}" init "$PROJECT_A" 2>/dev/null
"${HEBBS_BIN}" init "$PROJECT_B" 2>/dev/null

# Seed global vault
"${HEBBS_BIN}" remember "User prefers vim keybindings everywhere" \
  --importance 0.8 --entity-id user_prefs --global --format json 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "Always write commit messages in imperative mood" \
  --importance 0.7 --entity-id user_prefs --global --format json 2>/dev/null > /dev/null

# Seed project A
"${HEBBS_BIN}" remember "Alpha uses Next.js with app router" \
  --importance 0.8 --entity-id project_context --format json --vault "$PROJECT_A" 2>/dev/null > /dev/null

# Seed project B
"${HEBBS_BIN}" remember "Beta uses FastAPI with Python 3.12" \
  --importance 0.8 --entity-id project_context --format json --vault "$PROJECT_B" 2>/dev/null > /dev/null

echo "  Setup complete: global + alpha + beta vaults, bare dir with no vault"

# =========================================================================
# 1. Discovery fallback to global when no .hebbs/ in tree
# =========================================================================
echo ""
echo "--- 1. Discovery fallback to global ---"

# From a directory with no .hebbs/ anywhere above it, and WITHOUT --global,
# discovery should walk up, find nothing, then fall back to ~/.hebbs/
FALLBACK_RECALL=$(cd "$BARE_DIR/src/deep/nested" && \
  "${HEBBS_BIN}" recall "vim keybindings" \
  --strategy similarity --top-k 5 --format json 2>/dev/null)
FALLBACK_CONTENT=$(echo "$FALLBACK_RECALL" | jq -r '[.[].content] | join(" ")')
assert_contains_str "fallback_finds_global_memory" "$FALLBACK_CONTENT" "vim keybindings"

# Also verify a different global memory is reachable
FALLBACK_RECALL2=$(cd "$BARE_DIR/src/deep/nested" && \
  "${HEBBS_BIN}" recall "commit messages" \
  --strategy similarity --top-k 5 --format json 2>/dev/null)
FALLBACK_CONTENT2=$(echo "$FALLBACK_RECALL2" | jq -r '[.[].content] | join(" ")')
assert_contains_str "fallback_finds_second_global" "$FALLBACK_CONTENT2" "imperative mood"

# Verify it does NOT see project-specific memories
assert_not_contains_str "fallback_no_project_a_leak" "$FALLBACK_CONTENT" "Next.js"
assert_not_contains_str "fallback_no_project_b_leak" "$FALLBACK_CONTENT" "FastAPI"

# Store a memory from the bare dir (should land in global)
BARE_STORE=$(cd "$BARE_DIR" && \
  "${HEBBS_BIN}" remember "Stored from bare directory" \
  --importance 0.5 --entity-id misc --format json 2>/dev/null)
BARE_MEM_ID=$(echo "$BARE_STORE" | jq -r '.memory_id')
assert_pass "bare_dir_store_returns_id" "$( [ -n "$BARE_MEM_ID" ] && [ "$BARE_MEM_ID" != "null" ] && echo 0 || echo 1 )"

# Verify it landed in global
BARE_CHECK=$("${HEBBS_BIN}" recall "bare directory" \
  --strategy similarity --top-k 5 --global --format json 2>/dev/null)
BARE_CHECK_CONTENT=$(echo "$BARE_CHECK" | jq -r '[.[].content] | join(" ")')
assert_contains_str "bare_dir_stored_in_global" "$BARE_CHECK_CONTENT" "bare directory"

# Verify it's not in either project vault
BARE_NOT_A=$("${HEBBS_BIN}" recall "bare directory" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_A" 2>/dev/null)
BARE_NOT_A_CONTENT=$(echo "$BARE_NOT_A" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "bare_dir_not_in_alpha" "$BARE_NOT_A_CONTENT" "bare directory"

echo "  Discovery falls back to global vault when no .hebbs/ in tree"

# =========================================================================
# 2. --vault flag overrides discovery
# =========================================================================
echo ""
echo "--- 2. --vault overrides discovery ---"

# Standing in project A, but explicitly target project B with --vault
OVERRIDE_RECALL=$(cd "$PROJECT_A/src" && \
  "${HEBBS_BIN}" recall "FastAPI" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_B" 2>/dev/null)
OVERRIDE_CONTENT=$(echo "$OVERRIDE_RECALL" | jq -r '[.[].content] | join(" ")')
assert_contains_str "vault_override_finds_beta" "$OVERRIDE_CONTENT" "FastAPI"

# Confirm it's actually project B content, not project A
assert_not_contains_str "vault_override_not_alpha" "$OVERRIDE_CONTENT" "Next.js"

# Reverse: standing in project B, target project A
OVERRIDE_REV=$(cd "$PROJECT_B/src" && \
  "${HEBBS_BIN}" recall "Next.js" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_A" 2>/dev/null)
OVERRIDE_REV_CONTENT=$(echo "$OVERRIDE_REV" | jq -r '[.[].content] | join(" ")')
assert_contains_str "vault_override_rev_finds_alpha" "$OVERRIDE_REV_CONTENT" "Next.js"
assert_not_contains_str "vault_override_rev_not_beta" "$OVERRIDE_REV_CONTENT" "FastAPI"

# Store in B while standing in A
CROSS_STORE=$(cd "$PROJECT_A" && \
  "${HEBBS_BIN}" remember "Cross-stored from alpha into beta" \
  --importance 0.5 --entity-id cross_test --format json --vault "$PROJECT_B" 2>/dev/null)
CROSS_MEM_ID=$(echo "$CROSS_STORE" | jq -r '.memory_id')
assert_pass "cross_store_returns_id" "$( [ -n "$CROSS_MEM_ID" ] && [ "$CROSS_MEM_ID" != "null" ] && echo 0 || echo 1 )"

# Verify it landed in B
CROSS_CHECK=$("${HEBBS_BIN}" recall "Cross-stored" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_B" 2>/dev/null)
CROSS_CHECK_CONTENT=$(echo "$CROSS_CHECK" | jq -r '[.[].content] | join(" ")')
assert_contains_str "cross_store_in_beta" "$CROSS_CHECK_CONTENT" "Cross-stored"

# Verify it's NOT in A
CROSS_NOT_A=$("${HEBBS_BIN}" recall "Cross-stored" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_A" 2>/dev/null)
CROSS_NOT_A_CONTENT=$(echo "$CROSS_NOT_A" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "cross_store_not_in_alpha" "$CROSS_NOT_A_CONTENT" "Cross-stored"

echo "  --vault flag correctly overrides directory-based discovery"

# =========================================================================
# 3. SKIP: Conflicting content across vaults
# =========================================================================
echo ""
echo "--- 3. Conflicting content across vaults ---"
echo "  SKIP: Conflicting content test not yet implemented"
echo "  (global says 'dark mode', project says 'light mode' -- verify both"
echo "   are independently recallable and neither overwrites the other)"

# =========================================================================
# 4. SKIP: Concurrent access to same vault
# =========================================================================
echo ""
echo "--- 4. Concurrent access ---"
echo "  SKIP: Concurrent access test not yet implemented"
echo "  (two processes writing to same vault simultaneously -- verify"
echo "   RocksDB handles it without corruption or data loss)"

# =========================================================================
# Summary
# =========================================================================

log_metric "discovery_fallback_tests" "7" "count"
log_metric "vault_override_tests" "8" "count"
log_metric "skipped_tests" "2" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
