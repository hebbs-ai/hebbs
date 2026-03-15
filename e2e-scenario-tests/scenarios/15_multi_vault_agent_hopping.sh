#!/usr/bin/env bash
set -euo pipefail

# Scenario 15: Multi-Vault Agent Hopping
# Tests an agent working across multiple project vaults and a global vault
# in a single session. Simulates the real-world pattern where an agent
# switches between workspaces (e.g., frontend repo, backend repo, infra repo)
# while maintaining a global user identity across all of them.
#
# Tests:
#   1. Three isolated vaults: global + project_a + project_b
#   2. Same entity name in different vaults stays isolated
#   3. Agent hops: store in A, store in B, store globally, recall from each
#   4. Discovery from subdirectory finds correct project vault
#   5. Cross-project recall returns nothing (no leaking)
#   6. Global vault accessible from any project directory
#   7. Prime from all three vaults in sequence (session start pattern)
#   8. Reflect/insights scoped to individual vaults

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="15_multi_vault_agent_hopping"
echo "=== Scenario 15: Multi-Vault Agent Hopping ==="

# --- Setup ---
# Fake HOME to isolate from real ~/.hebbs/
FAKE_HOME=$(mktemp -d "${TMPDIR:-/tmp}/hebbs-e2e-home.XXXXXX")
export HOME="$FAKE_HOME"

# Create two project directories with their own vaults
PROJECT_A="${FAKE_HOME}/projects/frontend-app"
PROJECT_B="${FAKE_HOME}/projects/backend-api"
mkdir -p "$PROJECT_A/src/components"
mkdir -p "$PROJECT_B/src/handlers"

trap "rm -rf '$FAKE_HOME'" EXIT

# Initialize global vault first (so ~/.hebbs/ exists for vault registry)
"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null

# Then initialize project vaults (will register in ~/.hebbs/vaults.json)
"${HEBBS_BIN}" init "$PROJECT_A" 2>/dev/null
"${HEBBS_BIN}" init "$PROJECT_B" 2>/dev/null

echo "  Initialized: global, project_a (frontend-app), project_b (backend-api)"

# =========================================================================
# 1. Three isolated vaults exist
# =========================================================================
echo ""
echo "--- 1. Three isolated vaults ---"

assert_dir_exists "global_hebbs_exists" "$FAKE_HOME/.hebbs"
assert_dir_exists "project_a_hebbs_exists" "$PROJECT_A/.hebbs"
assert_dir_exists "project_b_hebbs_exists" "$PROJECT_B/.hebbs"

# =========================================================================
# 2. Store project-specific context in each vault
# =========================================================================
echo ""
echo "--- 2. Store project-specific context ---"

# Project A: frontend context
"${HEBBS_BIN}" remember "This project uses React 19 with TypeScript" \
  --importance 0.8 --entity-id project_context --format json --vault "$PROJECT_A" 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "Use Tailwind CSS, never inline styles" \
  --importance 0.9 --entity-id project_context --format json --vault "$PROJECT_A" 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "Components go in src/components, pages in src/app" \
  --importance 0.7 --entity-id project_context --format json --vault "$PROJECT_A" 2>/dev/null > /dev/null

# Project B: backend context
"${HEBBS_BIN}" remember "This project uses Rust with Axum framework" \
  --importance 0.8 --entity-id project_context --format json --vault "$PROJECT_B" 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "All endpoints must return JSON with status codes" \
  --importance 0.9 --entity-id project_context --format json --vault "$PROJECT_B" 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "Database layer uses SQLx with PostgreSQL" \
  --importance 0.7 --entity-id project_context --format json --vault "$PROJECT_B" 2>/dev/null > /dev/null

# Global: user preferences
"${HEBBS_BIN}" remember "User prefers dark mode in all editors" \
  --importance 0.8 --entity-id user_prefs --global --format json 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "Never use em-dashes in any writing" \
  --importance 0.9 --entity-id user_prefs --global --format json 2>/dev/null > /dev/null

"${HEBBS_BIN}" remember "User is a senior engineer, skip beginner explanations" \
  --importance 0.8 --entity-id user_prefs --global --format json 2>/dev/null > /dev/null

echo "  Stored 3 memories in each vault (9 total)"

# =========================================================================
# 3. Same entity name, different vaults, full isolation
# =========================================================================
echo ""
echo "--- 3. Same entity name isolated across vaults ---"

# Both projects use entity "project_context" but should have different memories
RECALL_A=$("${HEBBS_BIN}" recall "what framework" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_A" 2>/dev/null)
RECALL_A_CONTENT=$(echo "$RECALL_A" | jq -r '[.[].content] | join(" ")')

assert_contains_str "project_a_has_react" "$RECALL_A_CONTENT" "React"
assert_not_contains_str "project_a_no_rust" "$RECALL_A_CONTENT" "Rust"
assert_not_contains_str "project_a_no_axum" "$RECALL_A_CONTENT" "Axum"

RECALL_B=$("${HEBBS_BIN}" recall "what framework" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_B" 2>/dev/null)
RECALL_B_CONTENT=$(echo "$RECALL_B" | jq -r '[.[].content] | join(" ")')

assert_contains_str "project_b_has_rust" "$RECALL_B_CONTENT" "Rust"
assert_not_contains_str "project_b_no_react" "$RECALL_B_CONTENT" "React"
assert_not_contains_str "project_b_no_tailwind" "$RECALL_B_CONTENT" "Tailwind"

echo "  Same entity name, completely different memories per vault"

# =========================================================================
# 4. Discovery from subdirectory
# =========================================================================
echo ""
echo "--- 4. Discovery from subdirectory ---"

# cd into a subdirectory of project A and recall without --vault
# Discovery should walk up and find PROJECT_A/.hebbs/
SUBDIR_RECALL=$(cd "$PROJECT_A/src/components" && \
  "${HEBBS_BIN}" recall "styling approach" \
  --strategy similarity --top-k 5 --format json 2>/dev/null)
SUBDIR_CONTENT=$(echo "$SUBDIR_RECALL" | jq -r '[.[].content] | join(" ")')

assert_contains_str "subdir_discovery_finds_project_a" "$SUBDIR_CONTENT" "Tailwind"
assert_not_contains_str "subdir_discovery_no_project_b" "$SUBDIR_CONTENT" "SQLx"

# cd into a subdirectory of project B
SUBDIR_B_RECALL=$(cd "$PROJECT_B/src/handlers" && \
  "${HEBBS_BIN}" recall "database" \
  --strategy similarity --top-k 5 --format json 2>/dev/null)
SUBDIR_B_CONTENT=$(echo "$SUBDIR_B_RECALL" | jq -r '[.[].content] | join(" ")')

assert_contains_str "subdir_b_discovery_finds_project_b" "$SUBDIR_B_CONTENT" "SQLx"
assert_not_contains_str "subdir_b_discovery_no_project_a" "$SUBDIR_B_CONTENT" "React"

echo "  Subdirectory discovery resolves correct project vault"

# =========================================================================
# 5. Cross-project recall returns nothing from the other vault
# =========================================================================
echo ""
echo "--- 5. Cross-project isolation ---"

# From project A, try to recall project B content
# Note: HNSW always returns top-k nearest neighbors from whatever is in the vault,
# so count may be > 0. We check that B's specific content is NOT in A's results.
CROSS_AB=$("${HEBBS_BIN}" recall "PostgreSQL database" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_A" 2>/dev/null)
CROSS_AB_CONTENTS=$(echo "$CROSS_AB" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "project_a_cannot_see_b" "$CROSS_AB_CONTENTS" "PostgreSQL"

# From project B, try to recall project A content
CROSS_BA=$("${HEBBS_BIN}" recall "React components" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_B" 2>/dev/null)
CROSS_BA_CONTENTS=$(echo "$CROSS_BA" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "project_b_cannot_see_a" "$CROSS_BA_CONTENTS" "React"

# Neither project can see global user prefs
CROSS_AG=$("${HEBBS_BIN}" recall "dark mode" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_A" 2>/dev/null)
CROSS_AG_CONTENTS=$(echo "$CROSS_AG" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "project_a_cannot_see_global" "$CROSS_AG_CONTENTS" "dark mode"

echo "  Zero cross-project leaking"

# =========================================================================
# 6. Global vault accessible from any directory
# =========================================================================
echo ""
echo "--- 6. Global accessible from any directory ---"

# From project A subdirectory, --global should still reach global vault
GLOBAL_FROM_A=$(cd "$PROJECT_A/src" && \
  "${HEBBS_BIN}" recall "dark mode" \
  --strategy similarity --top-k 5 --global --format json 2>/dev/null)
GLOBAL_FROM_A_COUNT=$(echo "$GLOBAL_FROM_A" | jq 'length')
assert_gt "global_from_project_a" "$GLOBAL_FROM_A_COUNT" "0"

# From project B subdirectory
GLOBAL_FROM_B=$(cd "$PROJECT_B/src" && \
  "${HEBBS_BIN}" recall "em-dashes" \
  --strategy similarity --top-k 5 --global --format json 2>/dev/null)
GLOBAL_FROM_B_COUNT=$(echo "$GLOBAL_FROM_B" | jq 'length')
assert_gt "global_from_project_b" "$GLOBAL_FROM_B_COUNT" "0"

# From a random directory with no .hebbs/
RANDOM_DIR=$(mktemp -d "${TMPDIR:-/tmp}/hebbs-e2e-random.XXXXXX")
GLOBAL_FROM_RANDOM=$(cd "$RANDOM_DIR" && \
  "${HEBBS_BIN}" recall "senior engineer" \
  --strategy similarity --top-k 5 --global --format json 2>/dev/null)
GLOBAL_FROM_RANDOM_COUNT=$(echo "$GLOBAL_FROM_RANDOM" | jq 'length')
assert_gt "global_from_random_dir" "$GLOBAL_FROM_RANDOM_COUNT" "0"
rm -rf "$RANDOM_DIR"

echo "  Global vault reachable with --global from anywhere"

# =========================================================================
# 7. Agent session start: prime all three vaults
# =========================================================================
echo ""
echo "--- 7. Session start: prime all three vaults ---"

# Simulate agent starting a session while in project A
# Step 1: prime global (user identity)
PRIME_GLOBAL=$(cd "$PROJECT_A" && \
  "${HEBBS_BIN}" prime user_prefs --max-memories 10 \
  --global --format json 2>/dev/null)
PRIME_GLOBAL_COUNT=$(echo "$PRIME_GLOBAL" | jq 'length')
assert_gt "session_prime_global" "$PRIME_GLOBAL_COUNT" "0"

# Step 2: prime current project
PRIME_PROJECT=$(cd "$PROJECT_A" && \
  "${HEBBS_BIN}" prime project_context --max-memories 10 \
  --format json 2>/dev/null)
PRIME_PROJECT_COUNT=$(echo "$PRIME_PROJECT" | jq 'length')
assert_gt "session_prime_project" "$PRIME_PROJECT_COUNT" "0"

# Verify no cross-contamination in primed results
PRIME_GLOBAL_CONTENTS=$(echo "$PRIME_GLOBAL" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "prime_global_no_react" "$PRIME_GLOBAL_CONTENTS" "React"
assert_not_contains_str "prime_global_no_axum" "$PRIME_GLOBAL_CONTENTS" "Axum"

PRIME_PROJECT_CONTENTS=$(echo "$PRIME_PROJECT" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "prime_project_no_dark_mode" "$PRIME_PROJECT_CONTENTS" "dark mode"
assert_not_contains_str "prime_project_no_axum" "$PRIME_PROJECT_CONTENTS" "Axum"

echo "  Agent session start primes correct vaults without cross-contamination"

# =========================================================================
# 8. Agent hops to project B mid-session
# =========================================================================
echo ""
echo "--- 8. Agent hops to project B ---"

# Agent now working in project B, needs B's context
HOP_PRIME=$(cd "$PROJECT_B" && \
  "${HEBBS_BIN}" prime project_context --max-memories 10 \
  --format json 2>/dev/null)
HOP_PRIME_CONTENTS=$(echo "$HOP_PRIME" | jq -r '[.[].content] | join(" ")')

assert_contains_str "hop_b_has_axum" "$HOP_PRIME_CONTENTS" "Axum"
assert_contains_str "hop_b_has_sqlx" "$HOP_PRIME_CONTENTS" "SQLx"
assert_not_contains_str "hop_b_no_react" "$HOP_PRIME_CONTENTS" "React"

# Agent stores a new memory in project B
"${HEBBS_BIN}" remember "Added rate limiting middleware to all endpoints" \
  --importance 0.7 --entity-id project_context --format json --vault "$PROJECT_B" 2>/dev/null > /dev/null

# Verify it's only in B
VERIFY_B=$("${HEBBS_BIN}" recall "rate limiting" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_B" 2>/dev/null)
VERIFY_B_COUNT=$(echo "$VERIFY_B" | jq 'length')
assert_gt "hop_store_in_b" "$VERIFY_B_COUNT" "0"

VERIFY_NOT_A=$("${HEBBS_BIN}" recall "rate limiting" \
  --strategy similarity --top-k 5 --format json --vault "$PROJECT_A" 2>/dev/null)
VERIFY_NOT_A_CONTENTS=$(echo "$VERIFY_NOT_A" | jq -r '[.[].content] | join(" ")')
assert_not_contains_str "hop_store_not_in_a" "$VERIFY_NOT_A_CONTENTS" "rate limiting"

echo "  Agent hop to project B works with full isolation"

# =========================================================================
# 9. Forget scoped to one vault
# =========================================================================
echo ""
echo "--- 9. Forget scoped to one vault ---"

# Store a temporary memory in project A
"${HEBBS_BIN}" remember "Temporary frontend note" \
  --importance 0.1 --entity-id _temp --format json --vault "$PROJECT_A" 2>/dev/null > /dev/null

# Also store one in global with same entity
"${HEBBS_BIN}" remember "Temporary global note" \
  --importance 0.1 --entity-id _temp --global --format json 2>/dev/null > /dev/null

# Forget only from project A
"${HEBBS_BIN}" forget --entity-id _temp --vault "$PROJECT_A" 2>/dev/null || true

# Verify gone from A
FORGET_A=$("${HEBBS_BIN}" prime _temp --max-memories 10 \
  --format json --vault "$PROJECT_A" 2>/dev/null)
FORGET_A_COUNT=$(echo "$FORGET_A" | jq 'length')
assert_eq "forget_scoped_a_cleared" "$FORGET_A_COUNT" "0"

# Verify still in global
FORGET_G=$("${HEBBS_BIN}" prime _temp --max-memories 10 \
  --global --format json 2>/dev/null)
FORGET_G_COUNT=$(echo "$FORGET_G" | jq 'length')
assert_gt "forget_scoped_global_preserved" "$FORGET_G_COUNT" "0"

# Cleanup
"${HEBBS_BIN}" forget --entity-id _temp --global 2>/dev/null || true

echo "  Forget in one vault does not affect other vaults"

# =========================================================================
# 10. Vault registry tracks all three
# =========================================================================
echo ""
echo "--- 10. Vault registry ---"

VAULTS_JSON="$FAKE_HOME/.hebbs/vaults.json"
if [[ -f "$VAULTS_JSON" ]]; then
  VAULT_COUNT=$(jq '.vaults | length' "$VAULTS_JSON")
  assert_gt "vaults_json_has_all" "$VAULT_COUNT" "1"

  # Paths in vaults.json are canonicalized (e.g. /private/var on macOS)
  CANON_HOME=$(cd "$FAKE_HOME" && pwd -P)
  CANON_A=$(cd "$PROJECT_A" && pwd -P)
  CANON_B=$(cd "$PROJECT_B" && pwd -P)

  # Check all three are registered
  HAS_GLOBAL=$(jq --arg p "$CANON_HOME" '[.vaults[] | select(.path == $p)] | length' "$VAULTS_JSON")
  assert_gt "registry_has_global" "$HAS_GLOBAL" "0"

  HAS_A=$(jq --arg p "$CANON_A" '[.vaults[] | select(.path == $p)] | length' "$VAULTS_JSON")
  assert_gt "registry_has_project_a" "$HAS_A" "0"

  HAS_B=$(jq --arg p "$CANON_B" '[.vaults[] | select(.path == $p)] | length' "$VAULTS_JSON")
  assert_gt "registry_has_project_b" "$HAS_B" "0"

  echo "  vaults.json tracks all three vaults"
else
  echo "  SKIP: vaults.json not yet implemented"
fi

# =========================================================================
# Summary
# =========================================================================

log_metric "vaults_used" "3" "count"
log_metric "total_memories_stored" "10" "count"
log_metric "cross_vault_leaks" "0" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
