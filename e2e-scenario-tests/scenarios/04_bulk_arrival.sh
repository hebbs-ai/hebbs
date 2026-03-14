#!/usr/bin/env bash
set -euo pipefail

# Scenario 4: Bulk Arrival
# Tests: large batch of files dropped at once (cp 200 files).
# Maps to TASK-13 lifecycle scenario "Bulk arrival".

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="04_bulk_arrival"
echo "=== Scenario 4: Bulk Arrival ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
STAGING_DIR=$(mktemp -d)
trap "cleanup_vault '$VAULT_DIR'; rm -rf '$STAGING_DIR'" EXIT

# Start with a tiny vault (10 files)
mkdir -p "$VAULT_DIR/notes"
for i in $(seq 1 10); do
    echo -e "## Original Note $i\n\nContent for note $i.\n" > "$VAULT_DIR/notes/original-$i.md"
done

vault_init "$VAULT_DIR"
vault_index "$VAULT_DIR"

INITIAL_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
echo "Initial: $INITIAL_FILES files"

# Generate 200 files in staging directory
BULK_COUNT=200
echo "Generating $BULK_COUNT files in staging..."
python3 "$SCRIPT_DIR/../fixtures/generate_vault.py" \
    --size custom --count "$BULK_COUNT" --output "$STAGING_DIR" --seed 300 2>/dev/null || {
    # Fallback: generate files manually if custom size not supported
    mkdir -p "$STAGING_DIR/notes"
    for i in $(seq 1 $BULK_COUNT); do
        cat > "$STAGING_DIR/notes/bulk-$(printf '%03d' $i).md" << ENDFILE
---
title: Bulk Note $i
---

## Section A

Content for bulk note $i, section A. This is a test of the bulk arrival scenario.

## Section B

More content for note $i. Testing batch processing of many files arriving at once.
ENDFILE
    done
}

# --- Start watcher ---
vault_watch_start "$VAULT_DIR"
sleep 2

# --- Bulk drop: copy all 200 files at once ---
echo "Copying $BULK_COUNT files into vault..."
timer_start "bulk_copy"
cp "$STAGING_DIR"/notes/*.md "$VAULT_DIR/notes/" 2>/dev/null || \
    cp "$STAGING_DIR"/*.md "$VAULT_DIR/notes/" 2>/dev/null || true
timer_stop "bulk_copy"
echo "Copy complete in $(timer_get 'bulk_copy')ms"

# Wait for watcher to process everything
echo "Waiting for watcher to sync (this may take a while)..."
wait_for_sync "$VAULT_DIR" 60

# --- Stop watcher ---
vault_watch_stop

# --- Qualitative checks ---
FINAL_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")

assert_gt "new_files_indexed" "$FINAL_FILES" "$INITIAL_FILES"
assert_all_sections_synced "all_sections_synced" "$VAULT_DIR"

# Original files should still be present
for i in $(seq 1 10); do
    assert_manifest_has_file "original_file_$i" "$VAULT_DIR" "notes/original-$i.md"
done

# --- Quantitative checks ---
FINAL_SECTIONS=$(get_section_count "$VAULT_DIR")

log_metric "initial_files" "$INITIAL_FILES" "count"
log_metric "bulk_files_added" "$BULK_COUNT" "count"
log_metric "final_files" "$FINAL_FILES" "count"
log_metric "final_sections" "$FINAL_SECTIONS" "count"
log_metric "bulk_copy_time_ms" "$(timer_get 'bulk_copy')" "ms"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
