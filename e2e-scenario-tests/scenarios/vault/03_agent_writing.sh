#!/usr/bin/env bash
set -euo pipefail

# Scenario 3: Agent Writing
# Tests: AI agent creates 30 files in rapid succession while watcher is running.
# Maps to TASK-13 lifecycle scenario "Agent writing".

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="03_agent_writing"
echo "=== Scenario 3: Agent Writing ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

# Start with a small vault (20 files)
python3 "$SCRIPT_DIR/../fixtures/generate_vault.py" \
    --size small --output "$VAULT_DIR" --seed 200

vault_init "$VAULT_DIR"
vault_index "$VAULT_DIR"

INITIAL_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
INITIAL_SECTIONS=$(get_section_count "$VAULT_DIR")
echo "Initial: $INITIAL_FILES files, $INITIAL_SECTIONS sections"

# --- Start watcher ---
vault_watch_start "$VAULT_DIR"
sleep 2

# --- Burst: create 30 files in rapid succession (~100ms apart) ---
BURST_COUNT=30
echo "Creating $BURST_COUNT files in burst..."
timer_start "burst_create"

for i in $(seq 1 $BURST_COUNT); do
    FILE_NAME="notes/agent-output-$(printf '%03d' $i).md"
    CONTENT="---\ntitle: Agent Output $i\nauthor: ai-agent\n---\n\n## Summary\n\nThis is an AI-generated analysis report number $i.\n\n## Details\n\nDetailed findings from analysis run $i. The system detected $(($i * 3)) anomalies.\n"
    create_md_file "$VAULT_DIR" "$FILE_NAME" "$CONTENT"
    sleep 0.1
done

timer_stop "burst_create"
echo "Burst create complete in $(timer_get 'burst_create')ms"

# Wait for watcher to process
echo "Waiting for watcher to sync..."
wait_for_sync "$VAULT_DIR" 30

# --- Stop watcher ---
vault_watch_stop

# --- Qualitative checks ---
FINAL_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
EXPECTED_TOTAL_FILES=$((INITIAL_FILES + BURST_COUNT))

assert_eq "all_new_files_in_manifest" "$FINAL_FILES" "$EXPECTED_TOTAL_FILES"
assert_all_sections_synced "all_sections_synced" "$VAULT_DIR"

# Check a specific new file exists in manifest
assert_manifest_has_file "agent_file_001" "$VAULT_DIR" "notes/agent-output-001.md"
assert_manifest_has_file "agent_file_030" "$VAULT_DIR" "notes/agent-output-030.md"

# Original files should be untouched (same section count)
# Count sections only in original files
ORIGINAL_SECTIONS=0
MANIFEST=$(get_manifest "$VAULT_DIR")
for file in $(echo "$MANIFEST" | jq -r '.files | keys[]' | grep -v "agent-output"); do
    count=$(echo "$MANIFEST" | jq -r ".files[\"$file\"].sections | length")
    ORIGINAL_SECTIONS=$((ORIGINAL_SECTIONS + count))
done
assert_eq "original_files_untouched" "$ORIGINAL_SECTIONS" "$INITIAL_SECTIONS"

# --- Quantitative checks ---
FINAL_SECTIONS=$(get_section_count "$VAULT_DIR")
NEW_SECTIONS=$((FINAL_SECTIONS - INITIAL_SECTIONS))

log_metric "burst_create_time_ms" "$(timer_get 'burst_create')" "ms"
log_metric "initial_files" "$INITIAL_FILES" "count"
log_metric "burst_files" "$BURST_COUNT" "count"
log_metric "final_files" "$FINAL_FILES" "count"
log_metric "new_sections" "$NEW_SECTIONS" "count"
log_metric "final_sections" "$FINAL_SECTIONS" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
