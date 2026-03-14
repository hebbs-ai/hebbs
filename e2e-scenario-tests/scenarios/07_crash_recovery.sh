#!/usr/bin/env bash
set -euo pipefail

# Scenario 7: Crash Recovery
# Tests: kill -9 mid-index, resume from where it left off.
# Maps to TASK-13 crash safety (manifest written incrementally).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="07_crash_recovery"
echo "=== Scenario 7: Crash Recovery ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

# Generate a medium vault (500 files)
echo "Generating 500-file vault..."
python3 "$SCRIPT_DIR/../fixtures/generate_vault.py" \
    --size medium --output "$VAULT_DIR" --seed 400

EXPECTED_FILES=$(jq -r '.file_count' "$VAULT_DIR/metadata.json")
echo "Generated $EXPECTED_FILES files"

vault_init "$VAULT_DIR"

# --- Start index in background and kill after ~50% ---
echo "Starting index in background..."
timer_start "first_run"

$HEBBS_BIN index "$VAULT_DIR" &
INDEX_PID=$!

# Wait a bit then kill
sleep 3
echo "Killing index process (PID: $INDEX_PID)..."
kill -9 $INDEX_PID 2>/dev/null || true
wait $INDEX_PID 2>/dev/null || true

timer_stop "first_run"

# --- Check manifest state after crash ---
assert_file_exists "manifest_exists_after_crash" "$VAULT_DIR/.hebbs/manifest.json"

# Manifest should be valid JSON (atomic writes protect against corruption)
MANIFEST_VALID=$(jq -e '.' "$VAULT_DIR/.hebbs/manifest.json" > /dev/null 2>&1 && echo "true" || echo "false")
assert_eq "manifest_valid_json" "$MANIFEST_VALID" "true"

# Count files in manifest (should be partial, not 0 and not all)
PARTIAL_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
echo "Files indexed before crash: $PARTIAL_FILES / $EXPECTED_FILES"

# Partial should be > 0 (some work was done)
assert_gt "some_files_indexed" "$PARTIAL_FILES" "0"

# --- Resume: run index again ---
echo "Resuming index..."
timer_start "resume_run"
vault_index "$VAULT_DIR"
timer_stop "resume_run"

# --- Qualitative checks after resume ---
FINAL_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
assert_eq "all_files_indexed_after_resume" "$FINAL_FILES" "$EXPECTED_FILES"
assert_all_sections_synced "all_synced_after_resume" "$VAULT_DIR"

# --- Quantitative checks ---
FIRST_RUN_MS=$(timer_get "first_run")
RESUME_MS=$(timer_get "resume_run")

log_metric "first_run_time_ms" "$FIRST_RUN_MS" "ms"
log_metric "resume_time_ms" "$RESUME_MS" "ms"
log_metric "partial_files_before_crash" "$PARTIAL_FILES" "count"
log_metric "final_files" "$FINAL_FILES" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
