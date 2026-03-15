#!/usr/bin/env bash
set -euo pipefail

# Scenario 1: First Install
# Tests: bulk initial index of an existing vault (100-10,000 files).
# Maps to TASK-13 lifecycle scenario "First install".

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

VAULT_SIZE="${VAULT_SIZE:-medium}"

scenario_name="01_first_install"
echo "=== Scenario 1: First Install (vault size: $VAULT_SIZE) ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

echo "Generating vault..."
python3 "$SCRIPT_DIR/../fixtures/generate_vault.py" \
    --size "$VAULT_SIZE" --output "$VAULT_DIR" --seed 42

METADATA="$VAULT_DIR/metadata.json"
EXPECTED_FILES=$(jq -r '.file_count' "$METADATA")
EXPECTED_SECTIONS=$(jq -r '.total_sections' "$METADATA")
echo "Generated $EXPECTED_FILES files, $EXPECTED_SECTIONS expected sections"

# --- Test: hebbs init ---
timer_start "init"
vault_init "$VAULT_DIR"
timer_stop "init"

assert_dir_exists "hebbs_dir_created" "$VAULT_DIR/.hebbs"
assert_file_exists "config_created" "$VAULT_DIR/.hebbs/config.toml"
assert_file_exists "manifest_created" "$VAULT_DIR/.hebbs/manifest.json"
assert_dir_exists "index_dir_created" "$VAULT_DIR/.hebbs/index"

# --- Test: hebbs index ---
timer_start "phase1"
vault_index "$VAULT_DIR"
timer_stop "phase1"

# Qualitative checks
assert_manifest_file_count "all_files_in_manifest" "$VAULT_DIR" "$EXPECTED_FILES"
assert_all_sections_synced "all_sections_synced" "$VAULT_DIR"

ACTUAL_SECTIONS=$(get_section_count "$VAULT_DIR")
assert_eq "section_count_matches" "$ACTUAL_SECTIONS" "$EXPECTED_SECTIONS"

# Check that status command works
STATUS_OUTPUT=$(vault_status "$VAULT_DIR")
assert_contains "status_shows_files" <(echo "$STATUS_OUTPUT") "indexed"

# Quantitative checks
PHASE1_MS=$(timer_get "phase1")
log_metric "phase1_time_ms" "$PHASE1_MS" "ms"
log_metric "manifest_files" "$EXPECTED_FILES" "count"
log_metric "manifest_sections" "$ACTUAL_SECTIONS" "count"

MANIFEST_SIZE=$(stat -f%z "$VAULT_DIR/.hebbs/manifest.json" 2>/dev/null || stat -c%s "$VAULT_DIR/.hebbs/manifest.json" 2>/dev/null || echo "0")
log_metric "manifest_size_bytes" "$MANIFEST_SIZE" "bytes"

# --- Test: Re-index (should skip all unchanged files) ---
timer_start "reindex"
vault_index "$VAULT_DIR"
timer_stop "reindex"

REINDEX_MS=$(timer_get "reindex")
log_metric "reindex_skip_time_ms" "$REINDEX_MS" "ms"

# Re-index should be fast (all checksums match)
if [ "$VAULT_SIZE" = "medium" ]; then
    assert_lt "reindex_fast" "$REINDEX_MS" "5000"
fi

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
