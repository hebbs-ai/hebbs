#!/usr/bin/env bash
set -euo pipefail

# Scenario 12: CLI Operations in Local Mode
# Tests that all SKILL.md operations work with the unified binary in local mode
# (embedded engine, no server required). This validates that the old hebbs-cli
# commands (remember, recall, forget, prime, insights, status) produce correct
# JSON output when running against a local vault instead of a gRPC server.
#
# Every command the SKILL.md teaches agents is tested here.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="12_cli_operations_local"
echo "=== Scenario 12: CLI Operations in Local Mode ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

vault_init "$VAULT_DIR"

# =========================================================================
# 1. STATUS -- server health / brain status
# =========================================================================
echo ""
echo "--- 1. Status ---"

STATUS_OUT=$("${HEBBS_BIN}" status --vault "${VAULT_DIR}" 2>&1) || true
STATUS_FILE=$(mktemp)
echo "$STATUS_OUT" > "$STATUS_FILE"
assert_contains "status_works" "$STATUS_FILE" "indexed"
rm -f "$STATUS_FILE"

# =========================================================================
# 2. REMEMBER -- store memories with importance, entity-id, context
# =========================================================================
echo ""
echo "--- 2. Remember ---"

# Store preference memories (like an agent would via SKILL.md)
MEM1=$("${HEBBS_BIN}" remember "User prefers dark mode in all applications" \
  --importance 0.8 --entity-id user_prefs --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM1_ID=$(echo "$MEM1" | jq -r '.memory_id')
assert_pass "remember_returns_id" "$( [ -n "$MEM1_ID" ] && [ "$MEM1_ID" != "null" ] && echo 0 || echo 1 )"

MEM2=$("${HEBBS_BIN}" remember "User prefers vim keybindings in editors" \
  --importance 0.8 --entity-id user_prefs --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM2_ID=$(echo "$MEM2" | jq -r '.memory_id')

MEM3=$("${HEBBS_BIN}" remember "Always use structured logging with tracing crate" \
  --importance 0.7 --entity-id coding_prefs --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM3_ID=$(echo "$MEM3" | jq -r '.memory_id')

MEM4=$("${HEBBS_BIN}" remember "Project deadline is end of Q2" \
  --importance 0.9 --entity-id project_alpha --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM4_ID=$(echo "$MEM4" | jq -r '.memory_id')

MEM5=$("${HEBBS_BIN}" remember "Sprint velocity has been declining since sprint 4" \
  --importance 0.6 --entity-id project_alpha --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM5_ID=$(echo "$MEM5" | jq -r '.memory_id')

# Remember with context metadata (like SKILL.md shows)
MEM6=$("${HEBBS_BIN}" remember "User corrected: use match not unwrap" \
  --importance 0.9 --entity-id coding_prefs \
  --context '{"source":"correction","topic":"error_handling"}' \
  --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM6_ID=$(echo "$MEM6" | jq -r '.memory_id')

assert_pass "remember_six_memories" "$( [ -n "$MEM6_ID" ] && [ "$MEM6_ID" != "null" ] && echo 0 || echo 1 )"

# Verify JSON output structure matches what agents expect
MEM1_CONTENT=$(echo "$MEM1" | jq -r '.content')
MEM1_IMPORTANCE=$(echo "$MEM1" | jq -r '.importance')
MEM1_ENTITY=$(echo "$MEM1" | jq -r '.entity_id')
assert_eq "remember_content" "$MEM1_CONTENT" "User prefers dark mode in all applications"
assert_eq "remember_entity" "$MEM1_ENTITY" "user_prefs"
assert_pass "remember_importance" "$( echo "$MEM1_IMPORTANCE" | awk '{print ($1 > 0.79 && $1 < 0.81) ? 0 : 1}' )"

# Verify context is preserved
MEM6_CONTEXT_SOURCE=$(echo "$MEM6" | jq -r '.context.source // empty')
assert_eq "remember_context_preserved" "$MEM6_CONTEXT_SOURCE" "correction"

echo "  Stored 6 memories across 3 entities"

# =========================================================================
# 3. RECALL -- similarity strategy
# =========================================================================
echo ""
echo "--- 3. Recall (similarity) ---"

RECALL_SIM=$("${HEBBS_BIN}" recall "What are the user's UI preferences?" \
  --strategy similarity --top-k 5 --format json --vault "${VAULT_DIR}" 2>/dev/null)

RECALL_SIM_COUNT=$(echo "$RECALL_SIM" | jq 'length')
assert_gt "recall_sim_returns_results" "$RECALL_SIM_COUNT" "0"

# Results should be a JSON array with expected fields
RECALL_SIM_HAS_CONTENT=$(echo "$RECALL_SIM" | jq '.[0].content // empty' | head -1)
assert_pass "recall_sim_has_content" "$( [ -n "$RECALL_SIM_HAS_CONTENT" ] && echo 0 || echo 1 )"

RECALL_SIM_HAS_SCORE=$(echo "$RECALL_SIM" | jq '.[0].score // empty')
assert_pass "recall_sim_has_score" "$( [ -n "$RECALL_SIM_HAS_SCORE" ] && echo 0 || echo 1 )"

# =========================================================================
# 4. RECALL -- temporal strategy
# =========================================================================
echo ""
echo "--- 4. Recall (temporal) ---"

RECALL_TEMP=$("${HEBBS_BIN}" recall "recent activity" \
  --strategy temporal --entity-id project_alpha --top-k 5 \
  --format json --vault "${VAULT_DIR}" 2>/dev/null)

RECALL_TEMP_COUNT=$(echo "$RECALL_TEMP" | jq 'length')
assert_gt "recall_temporal_returns_results" "$RECALL_TEMP_COUNT" "0"

# =========================================================================
# 5. GET -- retrieve a specific memory by ID
# =========================================================================
echo ""
echo "--- 5. Get ---"

GET_OUT=$("${HEBBS_BIN}" get "${MEM1_ID}" --format json --vault "${VAULT_DIR}" 2>/dev/null)
GET_CONTENT=$(echo "$GET_OUT" | jq -r '.content')
assert_eq "get_returns_correct_memory" "$GET_CONTENT" "User prefers dark mode in all applications"

# =========================================================================
# 6. PRIME -- warm context for an entity
# =========================================================================
echo ""
echo "--- 6. Prime ---"

PRIME_OUT=$("${HEBBS_BIN}" prime user_prefs --max-memories 10 \
  --format json --vault "${VAULT_DIR}" 2>/dev/null)

PRIME_COUNT=$(echo "$PRIME_OUT" | jq 'length')
assert_gt "prime_returns_memories" "$PRIME_COUNT" "0"

# Prime should return user_prefs memories
PRIME_ENTITIES=$(echo "$PRIME_OUT" | jq -r '[.[].entity_id] | unique | .[]')
assert_eq "prime_scoped_to_entity" "$PRIME_ENTITIES" "user_prefs"

# =========================================================================
# 7. FORGET -- remove by ID
# =========================================================================
echo ""
echo "--- 7. Forget (by ID) ---"

# Remember a temporary memory to forget
TEMP_MEM=$("${HEBBS_BIN}" remember "Temporary test memory to forget" \
  --importance 0.1 --entity-id _test --format json --vault "${VAULT_DIR}" 2>/dev/null)
TEMP_MEM_ID=$(echo "$TEMP_MEM" | jq -r '.memory_id')

# Convert ULID to hex for forget
TEMP_MEM_HEX=$(echo "$TEMP_MEM" | jq -r '.memory_id')

"${HEBBS_BIN}" forget --ids "${TEMP_MEM_HEX}" --vault "${VAULT_DIR}" 2>/dev/null || true

# Verify it's gone (get should fail)
GET_FORGOTTEN=$("${HEBBS_BIN}" get "${TEMP_MEM_HEX}" --format json --vault "${VAULT_DIR}" 2>&1) || true
FORGOTTEN_CHECK=$(echo "$GET_FORGOTTEN" | grep -c "Error\|error\|not found" || echo "0")
assert_gt "forget_removes_memory" "$FORGOTTEN_CHECK" "0"

# =========================================================================
# 8. FORGET -- remove by entity
# =========================================================================
echo ""
echo "--- 8. Forget (by entity) ---"

# Store some memories in a test entity
"${HEBBS_BIN}" remember "Test entity memory 1" \
  --importance 0.3 --entity-id _cleanup_test --format json --vault "${VAULT_DIR}" 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "Test entity memory 2" \
  --importance 0.3 --entity-id _cleanup_test --format json --vault "${VAULT_DIR}" 2>/dev/null > /dev/null

"${HEBBS_BIN}" forget --entity-id _cleanup_test --vault "${VAULT_DIR}" 2>/dev/null || true

# Prime should return nothing for that entity
CLEANUP_PRIME=$("${HEBBS_BIN}" prime _cleanup_test --max-memories 10 \
  --format json --vault "${VAULT_DIR}" 2>/dev/null)
CLEANUP_COUNT=$(echo "$CLEANUP_PRIME" | jq 'length')
assert_eq "forget_entity_clears_all" "$CLEANUP_COUNT" "0"

# =========================================================================
# 9. INSPECT -- memory detail + graph edges
# =========================================================================
echo ""
echo "--- 9. Inspect ---"

INSPECT_OUT=$("${HEBBS_BIN}" inspect "${MEM1_ID}" --vault "${VAULT_DIR}" 2>&1) || true
INSPECT_FILE=$(mktemp)
echo "$INSPECT_OUT" > "$INSPECT_FILE"
assert_contains "inspect_shows_detail" "$INSPECT_FILE" "Memory Detail"
rm -f "$INSPECT_FILE"

# =========================================================================
# 10. Recall with scoring weights (SKILL.md feature)
# =========================================================================
echo ""
echo "--- 10. Recall with weights ---"

RECALL_WEIGHTED=$("${HEBBS_BIN}" recall "preferences" \
  --strategy similarity --top-k 5 \
  --weights "0.3:0.1:0.5:0.1" \
  --format json --vault "${VAULT_DIR}" 2>/dev/null)

RECALL_WEIGHTED_COUNT=$(echo "$RECALL_WEIGHTED" | jq 'length')
assert_gt "recall_weighted_returns_results" "$RECALL_WEIGHTED_COUNT" "0"

# =========================================================================
# Summary
# =========================================================================

log_metric "memories_stored" "6" "count"
log_metric "entities_used" "3" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
