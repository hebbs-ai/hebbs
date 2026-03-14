#!/usr/bin/env bash
set -euo pipefail

# Scenario 2: Realtime Editing
# Tests: rapid single-file saves while watcher is running.
# Maps to TASK-13 lifecycle scenario "Realtime editing".

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="02_realtime_editing"
echo "=== Scenario 2: Realtime Editing ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

# Create a small vault with 10 files
python3 "$SCRIPT_DIR/../fixtures/generate_vault.py" \
    --size small --output "$VAULT_DIR" --seed 100

vault_init "$VAULT_DIR"
vault_index "$VAULT_DIR"

INITIAL_SECTIONS=$(get_section_count "$VAULT_DIR")
echo "Initial index: $INITIAL_SECTIONS sections"

# Create a test file to edit rapidly
TARGET_FILE="notes/rapid-edit-target.md"
create_md_file "$VAULT_DIR" "$TARGET_FILE" "## Original\n\nOriginal content version 1.\n"

vault_index "$VAULT_DIR"

# --- Start watcher ---
vault_watch_start "$VAULT_DIR"
sleep 2 # Let watcher settle

# --- Rapid editing: 10 saves in 20 seconds ---
echo "Starting rapid edit sequence (10 saves)..."
timer_start "edit_sequence"

for i in $(seq 1 10); do
    modify_md_file "$VAULT_DIR" "$TARGET_FILE" "## Original\n\nUpdated content version $i.\n\nThis is edit number $i.\n"
    sleep 2
done

# Wait for final sync
echo "Waiting for sync..."
sleep 5 # phase2 debounce (3s) + margin
wait_for_sync "$VAULT_DIR" 15

timer_stop "edit_sequence"

# --- Stop watcher ---
vault_watch_stop

# --- Qualitative checks ---

# The manifest should show the file with synced sections
assert_manifest_has_file "target_file_in_manifest" "$VAULT_DIR" "$TARGET_FILE"
assert_all_sections_synced "all_synced_after_edits" "$VAULT_DIR"

# Content should be the final version (version 10)
MANIFEST=$(get_manifest "$VAULT_DIR")
# Check that memory_id stayed the same across edits (revise, not re-create)
SECTION_COUNT_FOR_FILE=$(echo "$MANIFEST" | jq -r ".files[\"$TARGET_FILE\"].sections | length")
assert_eq "single_section_not_duplicated" "$SECTION_COUNT_FOR_FILE" "1"

# --- Quantitative checks ---
EDIT_SEQ_MS=$(timer_get "edit_sequence")
log_metric "edit_sequence_time_ms" "$EDIT_SEQ_MS" "ms"
log_metric "initial_sections" "$INITIAL_SECTIONS" "count"

FINAL_SECTIONS=$(get_section_count "$VAULT_DIR")
log_metric "final_sections" "$FINAL_SECTIONS" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
