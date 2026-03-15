#!/usr/bin/env bash
set -euo pipefail

# Scenario 24: Daemon vaults.json Live Registration (TASK-19 Scenario 7)
# Verifies that the daemon proactively opens vaults when they are registered
# in vaults.json, without requiring a daemon restart or a CLI command to
# "touch" the vault.
#
# Tests:
#   1. On startup, daemon proactively opens vaults already in vaults.json
#   2. Live registration: `hebbs init` on new dir -> daemon picks it up
#   3. Recall works on live-registered vault without explicit open
#   4. File watcher starts on live-registered vault (auto-indexing)
#   5. Multiple vaults registered in quick succession

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="24_daemon_vaults_live_registration"
echo "=== Scenario 24: Daemon vaults.json Live Registration ==="

# --- Setup ---
FAKE_HOME=$(mktemp -d "/tmp/hb-livereg.XXXXXX")
export HOME="$FAKE_HOME"
VAULT_PRE="${FAKE_HOME}/pre-registered"
VAULT_LIVE="${FAKE_HOME}/live-vault"
VAULT_MULTI_A="${FAKE_HOME}/multi-a"
VAULT_MULTI_B="${FAKE_HOME}/multi-b"
mkdir -p "$VAULT_PRE" "$VAULT_LIVE" "$VAULT_MULTI_A" "$VAULT_MULTI_B"

cleanup() {
  daemon_stop 2>/dev/null || true
  rm -rf "$FAKE_HOME"
}
trap cleanup EXIT

# Init global vault (creates ~/.hebbs/)
"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null

# =========================================================================
# 1. Startup proactive open: pre-register a vault, then start daemon
# =========================================================================
echo ""
echo "--- 1. Startup proactive open ---"

# Init a vault before daemon starts -> hebbs init registers it in vaults.json
"${HEBBS_BIN}" init "$VAULT_PRE" 2>/dev/null

# Verify vaults.json has the entry
VAULTS_JSON="${FAKE_HOME}/.hebbs/vaults.json"
if [[ -f "$VAULTS_JSON" ]]; then
  PRE_REG_COUNT=$(jq '.vaults | length' "$VAULTS_JSON" 2>/dev/null || echo "0")
  assert_gt "pre_registered_vault_count" "$PRE_REG_COUNT" "0"

  # Use Python to resolve symlinks (macOS /tmp -> /private/tmp)
  PRE_CANONICAL=$(python3 -c "import os; print(os.path.realpath('$VAULT_PRE'))")
  PRE_IN_JSON=$(jq --arg p "$PRE_CANONICAL" '[.vaults[] | select(.path == $p)] | length' "$VAULTS_JSON" 2>/dev/null || echo "0")
  assert_gt "pre_registered_in_vaults_json" "$PRE_IN_JSON" "0"
else
  assert_pass "pre_registered_vault_count" "1"
  assert_pass "pre_registered_in_vaults_json" "1"
fi

# Add some content before daemon starts
create_md_file "$VAULT_PRE" "pre-notes.md" "## Pre-registered Content

This vault was initialized before the daemon started.
The daemon should proactively open it and start watching.
"

# Start daemon -- it should proactively open the pre-registered vault
daemon_start 600

# Give daemon a moment to proactively open and start watching
sleep 2

# The vault should already be open (watcher started), so file changes
# should be picked up without us needing to "touch" it with a recall.
echo "  Waiting for pre-registered vault sync..."
if wait_for_sync "$VAULT_PRE" 60; then
  SECTIONS=$(get_section_count "$VAULT_PRE")
  assert_gt "startup_proactive_sections" "$SECTIONS" "0"
  assert_pass "startup_proactive_open" "0"
  echo "  Pre-registered vault auto-indexed: $SECTIONS sections"
else
  echo "  WARN: pre-registered vault sync did not complete"
  # Check if manifest at least has the file (phase1 ran)
  if [[ -f "${VAULT_PRE}/.hebbs/manifest.json" ]]; then
    FILE_COUNT=$(jq '.files | length' "${VAULT_PRE}/.hebbs/manifest.json" 2>/dev/null || echo "0")
    echo "  Manifest: $FILE_COUNT files"
  fi
  assert_pass "startup_proactive_open" "1"
fi

# =========================================================================
# 2. Live registration: init new vault while daemon is running
# =========================================================================
echo ""
echo "--- 2. Live registration via hebbs init ---"

# Init a new vault while daemon is running -> registers in vaults.json
"${HEBBS_BIN}" init "$VAULT_LIVE" 2>/dev/null

# Verify it was added to vaults.json
LIVE_CANONICAL=$(python3 -c "import os; print(os.path.realpath('$VAULT_LIVE'))")
LIVE_IN_JSON=$(jq --arg p "$LIVE_CANONICAL" '[.vaults[] | select(.path == $p)] | length' "$VAULTS_JSON" 2>/dev/null || echo "0")
assert_gt "live_registered_in_vaults_json" "$LIVE_IN_JSON" "0"

# Give daemon time to detect vaults.json change and open the vault
sleep 3

# Create content in the live-registered vault
create_md_file "$VAULT_LIVE" "live-notes.md" "## Live Registration Test

This file was created after the vault was live-registered.
The daemon should detect the registration and start watching.
"

echo "  Waiting for live-registered vault sync..."
if wait_for_sync "$VAULT_LIVE" 60; then
  SECTIONS=$(get_section_count "$VAULT_LIVE")
  assert_gt "live_reg_sections" "$SECTIONS" "0"
  assert_pass "live_registration_auto_watch" "0"
  echo "  Live-registered vault auto-indexed: $SECTIONS sections"
else
  echo "  WARN: live-registered vault sync did not complete"
  assert_pass "live_registration_auto_watch" "1"
fi

# =========================================================================
# 3. Recall works on live-registered vault
# =========================================================================
echo ""
echo "--- 3. Recall on live-registered vault ---"

RECALL_OUTPUT=$("${HEBBS_BIN}" recall "live registration" --strategy similarity --top-k 5 --format json --vault "$VAULT_LIVE" 2>/dev/null) || true

RECALL_COUNT=$(echo "$RECALL_OUTPUT" | jq 'length' 2>/dev/null || echo "0")
if [[ "$RECALL_COUNT" -gt 0 ]]; then
  assert_pass "recall_on_live_vault" "0"
  echo "  Recall returned $RECALL_COUNT result(s)"
else
  # Vault may still be embedding; try waiting and retrying
  echo "  No results yet, waiting for embedding..."
  sleep 5
  RECALL_OUTPUT=$("${HEBBS_BIN}" recall "live registration" --strategy similarity --top-k 5 --format json --vault "$VAULT_LIVE" 2>/dev/null) || true
  RECALL_COUNT=$(echo "$RECALL_OUTPUT" | jq 'length' 2>/dev/null || echo "0")
  if [[ "$RECALL_COUNT" -gt 0 ]]; then
    assert_pass "recall_on_live_vault" "0"
    echo "  Recall returned $RECALL_COUNT result(s) after retry"
  else
    assert_pass "recall_on_live_vault" "1"
    echo "  WARN: recall returned no results"
  fi
fi

# =========================================================================
# 4. File watcher active on live-registered vault (modify triggers re-index)
# =========================================================================
echo ""
echo "--- 4. File watcher active on live vault ---"

SECTIONS_BEFORE=$(get_section_count "$VAULT_LIVE" 2>/dev/null || echo "0")

modify_md_file "$VAULT_LIVE" "live-notes.md" "## Live Registration Test

This file was created after the vault was live-registered.
The daemon should detect the registration and start watching.

## Additional Section

This section was added to verify the watcher is active on live-registered vaults.
"

echo "  Waiting for section count to increase from $SECTIONS_BEFORE..."
DEADLINE_MOD=$(( $(date +%s) + 30 ))
MODIFY_DETECTED=false
while (( $(date +%s) < DEADLINE_MOD )); do
  SECTIONS_AFTER=$(get_section_count "$VAULT_LIVE" 2>/dev/null || echo "0")
  if [[ "$SECTIONS_AFTER" -gt "$SECTIONS_BEFORE" ]]; then
    MODIFY_DETECTED=true
    break
  fi
  sleep 0.5
done

if $MODIFY_DETECTED; then
  assert_pass "live_vault_watcher_active" "0"
  echo "  Watcher active: sections $SECTIONS_BEFORE -> $SECTIONS_AFTER"
else
  echo "  WARN: section count did not increase (stayed at $SECTIONS_BEFORE)"
  assert_pass "live_vault_watcher_active" "1"
fi

# =========================================================================
# 5. Multiple vaults registered in quick succession
# =========================================================================
echo ""
echo "--- 5. Multiple vaults registered rapidly ---"

"${HEBBS_BIN}" init "$VAULT_MULTI_A" 2>/dev/null
"${HEBBS_BIN}" init "$VAULT_MULTI_B" 2>/dev/null

# Give daemon time to detect both registrations
sleep 3

# Create content in both
create_md_file "$VAULT_MULTI_A" "alpha.md" "## Alpha Vault

First of two rapidly registered vaults.
"

create_md_file "$VAULT_MULTI_B" "beta.md" "## Beta Vault

Second of two rapidly registered vaults.
"

echo "  Waiting for multi-vault A sync..."
MULTI_A_OK=false
if wait_for_sync "$VAULT_MULTI_A" 60; then
  A_SECTIONS=$(get_section_count "$VAULT_MULTI_A")
  if [[ "$A_SECTIONS" -gt 0 ]]; then
    MULTI_A_OK=true
    echo "  Vault A auto-indexed: $A_SECTIONS sections"
  fi
fi

echo "  Waiting for multi-vault B sync..."
MULTI_B_OK=false
if wait_for_sync "$VAULT_MULTI_B" 60; then
  B_SECTIONS=$(get_section_count "$VAULT_MULTI_B")
  if [[ "$B_SECTIONS" -gt 0 ]]; then
    MULTI_B_OK=true
    echo "  Vault B auto-indexed: $B_SECTIONS sections"
  fi
fi

if $MULTI_A_OK && $MULTI_B_OK; then
  assert_pass "multi_vault_rapid_registration" "0"
  echo "  Both rapidly registered vaults indexed successfully"
else
  assert_pass "multi_vault_rapid_registration" "1"
  echo "  WARN: not all rapidly registered vaults indexed"
fi

# Verify all 4 vaults are in vaults.json
TOTAL_VAULTS=$(jq '.vaults | length' "$VAULTS_JSON" 2>/dev/null || echo "0")
# global + pre + live + multi_a + multi_b = 5
assert_gt "total_registered_vaults" "$TOTAL_VAULTS" "3"
echo "  Total vaults in registry: $TOTAL_VAULTS"

# =========================================================================
# Summary
# =========================================================================

log_metric "live_reg_tests_run" "5" "count"

save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
