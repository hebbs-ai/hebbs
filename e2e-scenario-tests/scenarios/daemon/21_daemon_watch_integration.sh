#!/usr/bin/env bash
set -euo pipefail

# Scenario 21: Daemon Watch Integration (Milestone 3)
# Verifies that the unified daemon watches vaults for file changes and
# automatically runs the two-phase ingest pipeline (parse + embed).
#
# Tests:
#   1. Create .md file while daemon is running -> sections appear in manifest
#   2. Modify .md file -> sections are updated
#   3. Delete .md file -> sections are orphaned/removed
#   4. `hebbs watch` command reports daemon is handling watching
#   5. Multiple vaults: changes in both are picked up

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="21_daemon_watch_integration"
echo "=== Scenario 21: Daemon Watch Integration (Milestone 3) ==="

# --- Setup ---
FAKE_HOME=$(mktemp -d "/tmp/hb-watch.XXXXXX")
export HOME="$FAKE_HOME"
VAULT_A="${FAKE_HOME}/va"
VAULT_B="${FAKE_HOME}/vb"
mkdir -p "$VAULT_A" "$VAULT_B"

cleanup() {
  daemon_stop 2>/dev/null || true
  rm -rf "$FAKE_HOME"
}
trap cleanup EXIT

# Init global + both vaults
"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null
"${HEBBS_BIN}" init "$VAULT_A" 2>/dev/null
"${HEBBS_BIN}" init "$VAULT_B" 2>/dev/null

# Start daemon with long idle timeout
daemon_start 600

# "Touch" the vaults so the daemon opens them and starts watchers.
# A simple recall (even with no results) triggers get_or_open().
"${HEBBS_BIN}" recall "anything" --strategy similarity --top-k 1 --format json --vault "$VAULT_A" 2>/dev/null || true
"${HEBBS_BIN}" recall "anything" --strategy similarity --top-k 1 --format json --vault "$VAULT_B" 2>/dev/null || true

# =========================================================================
# 1. Create a .md file -> daemon should pick it up and index
# =========================================================================
echo ""
echo "--- 1. Create .md file, expect auto-indexing ---"

create_md_file "$VAULT_A" "notes.md" "## Design Decisions

We chose Rust for performance and safety.

## Architecture

The system uses a daemon pattern with Unix sockets.
"

# Wait for the watcher pipeline: phase1 (500ms debounce) + phase2 (3000ms debounce)
# First embedding batch may be slower, so allow 60s.
echo "  Waiting for sync (up to 60s for first embed)..."
if wait_for_sync "$VAULT_A" 60; then
  assert_pass "auto_index_on_create" "0"
  SECTION_COUNT=$(get_section_count "$VAULT_A")
  assert_gt "section_count_after_create" "$SECTION_COUNT" "0"
  echo "  File auto-indexed: $SECTION_COUNT sections"
else
  # Check if file at least appeared in manifest (phase 1 ran)
  if [[ -f "${VAULT_A}/.hebbs/manifest.json" ]]; then
    FILE_COUNT=$(jq '.files | length' "${VAULT_A}/.hebbs/manifest.json" 2>/dev/null || echo "0")
    SYNCED=$(jq '[.files[].sections[] | select(.state == "synced")] | length' "${VAULT_A}/.hebbs/manifest.json" 2>/dev/null || echo "0")
    echo "  Manifest: $FILE_COUNT files, $SYNCED synced sections"
  fi
  assert_pass "auto_index_on_create" "1"
  echo "  WARN: sync did not complete within 60s"
fi

# =========================================================================
# 2. Modify the .md file -> sections should update
# =========================================================================
echo ""
echo "--- 2. Modify .md file, expect sections to update ---"

SECTIONS_BEFORE=$(get_section_count "$VAULT_A" 2>/dev/null || echo "0")

modify_md_file "$VAULT_A" "notes.md" "## Design Decisions

We chose Rust for performance and safety. Memory management is zero-cost.

## Architecture

The system uses a daemon pattern with Unix sockets for local IPC.

## Performance

Cold start is eliminated by the daemon keeping the ONNX model warm.
"

# Wait for the section count to change (we added ## Performance)
echo "  Waiting for section count to increase from $SECTIONS_BEFORE..."
DEADLINE_MOD=$(( $(date +%s) + 30 ))
MODIFY_DETECTED=false
while (( $(date +%s) < DEADLINE_MOD )); do
  SECTIONS_AFTER=$(get_section_count "$VAULT_A" 2>/dev/null || echo "0")
  if [[ "$SECTIONS_AFTER" -gt "$SECTIONS_BEFORE" ]]; then
    MODIFY_DETECTED=true
    break
  fi
  sleep 0.5
done

if $MODIFY_DETECTED; then
  assert_pass "auto_index_on_modify" "0"
  echo "  Sections updated: $SECTIONS_BEFORE -> $SECTIONS_AFTER"
  # Now wait for sync to complete
  wait_for_sync "$VAULT_A" 30 || true
else
  echo "  WARN: section count did not increase within 30s (stayed at $SECTIONS_BEFORE)"
  assert_pass "auto_index_on_modify" "1"
fi

# =========================================================================
# 3. Delete the .md file -> sections should be orphaned
# =========================================================================
echo ""
echo "--- 3. Delete .md file, expect sections orphaned ---"

SECTIONS_BEFORE_DELETE=$(get_section_count "$VAULT_A" 2>/dev/null || echo "0")

delete_md_file "$VAULT_A" "notes.md"

# Wait for phase1 delete processing: 200ms tick + phase2 debounce (3s)
# Allow up to 30s for the full delete pipeline
echo "  Waiting for delete to propagate..."
DEADLINE_DEL=$(( $(date +%s) + 30 ))
DELETE_DETECTED=false
while (( $(date +%s) < DEADLINE_DEL )); do
  if [[ -f "${VAULT_A}/.hebbs/manifest.json" ]]; then
    CUR_FILES=$(jq '.files | length' "${VAULT_A}/.hebbs/manifest.json" 2>/dev/null || echo "?")
    CUR_ORPHANED=$(jq '[.files[].sections[] | select(.state == "orphaned")] | length' "${VAULT_A}/.hebbs/manifest.json" 2>/dev/null || echo "0")
    if [[ "$CUR_FILES" == "0" ]] || [[ "$CUR_ORPHANED" -gt 0 ]]; then
      DELETE_DETECTED=true
      break
    fi
  fi
  sleep 0.5
done

if $DELETE_DETECTED; then
  assert_pass "auto_orphan_on_delete" "0"
  echo "  Delete detected (files: $CUR_FILES, orphaned: $CUR_ORPHANED)"
else
  # Debug: show what the manifest looks like
  echo "  Delete not detected within 30s"
  echo "  Manifest state: files=$(jq '.files | length' "${VAULT_A}/.hebbs/manifest.json" 2>/dev/null || echo '?')"
  echo "  File keys: $(jq '.files | keys' "${VAULT_A}/.hebbs/manifest.json" 2>/dev/null || echo '?')"
  assert_pass "auto_orphan_on_delete" "1"
fi

# =========================================================================
# 4. `hebbs watch` command should report daemon is active
# =========================================================================
echo ""
echo "--- 4. hebbs watch -> daemon message ---"

WATCH_OUTPUT=$("${HEBBS_BIN}" watch --vault "$VAULT_A" 2>&1) || true
WATCH_DAEMON=$(echo "$WATCH_OUTPUT" | grep -ci "daemon\|watching\|built.into" || true)
if [[ "$WATCH_DAEMON" -gt 0 ]]; then
  assert_pass "watch_reports_daemon" "0"
  echo "  \`hebbs watch\` correctly reports daemon is handling watching"
else
  echo "  Watch output: $WATCH_OUTPUT"
  assert_pass "watch_reports_daemon" "1"
fi

# =========================================================================
# 5. Multi-vault: changes in vault B are also picked up
# =========================================================================
echo ""
echo "--- 5. Multi-vault watch ---"

create_md_file "$VAULT_B" "project.md" "## Goals

Build a fast memory engine for AI agents.

## Timeline

Ship Milestone 3 by end of sprint.
"

echo "  Waiting for vault B sync..."
if wait_for_sync "$VAULT_B" 30; then
  B_SECTIONS=$(get_section_count "$VAULT_B")
  assert_gt "vault_b_auto_index" "$B_SECTIONS" "0"
  echo "  Vault B auto-indexed: $B_SECTIONS sections"
  assert_pass "multi_vault_watch" "0"
else
  assert_pass "multi_vault_watch" "1"
  echo "  WARN: vault B sync did not complete"
fi

# =========================================================================
# Summary
# =========================================================================

log_metric "watch_tests_run" "5" "count"

save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
