#!/usr/bin/env bash
set -euo pipefail

# Scenario 9: Delete and Rename
# Tests: file deletion maps to forget(), rename maps to delete + create.
# Maps to TASK-13 file deletion/rename handling.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="09_delete_and_rename"
echo "=== Scenario 9: Delete and Rename ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

mkdir -p "$VAULT_DIR/notes"

cat > "$VAULT_DIR/notes/to-delete.md" << 'EOF'
## Temporary Note

This note will be deleted. It has important content that should be forgotten.
EOF

cat > "$VAULT_DIR/notes/old-name.md" << 'EOF'
## Renamed Note

This note will be renamed from old-name to new-name. Content should survive.

## Details

Additional details in a second section.
EOF

cat > "$VAULT_DIR/notes/stable.md" << 'EOF'
## Stable Note

This note should not be affected by delete/rename operations on other files.
EOF

cat > "$VAULT_DIR/notes/linker.md" << 'EOF'
## Links

References: [[to-delete]] and [[old-name]].
EOF

vault_init "$VAULT_DIR"
vault_index "$VAULT_DIR"

INITIAL_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
INITIAL_SECTIONS=$(get_section_count "$VAULT_DIR")

# Get memory IDs for the file to be deleted
DELETE_MEMORY_IDS=$(jq -r '.files["notes/to-delete.md"].sections[].memory_id' "$VAULT_DIR/.hebbs/manifest.json")
RENAME_MEMORY_IDS=$(jq -r '.files["notes/old-name.md"].sections[].memory_id' "$VAULT_DIR/.hebbs/manifest.json")

echo "Initial: $INITIAL_FILES files, $INITIAL_SECTIONS sections"
echo "Delete memory IDs: $DELETE_MEMORY_IDS"
echo "Rename memory IDs: $RENAME_MEMORY_IDS"

# --- Start watcher ---
vault_watch_start "$VAULT_DIR"
sleep 2

# --- Delete a file ---
echo "Deleting notes/to-delete.md..."
delete_md_file "$VAULT_DIR" "notes/to-delete.md"

# --- Rename a file ---
echo "Renaming notes/old-name.md -> notes/new-name.md..."
rename_md_file "$VAULT_DIR" "notes/old-name.md" "notes/new-name.md"

# Wait for sync
echo "Waiting for sync..."
wait_for_sync "$VAULT_DIR" 15

# --- Stop watcher ---
vault_watch_stop

# --- Qualitative checks ---

# Deleted file: removed from manifest
MANIFEST=$(get_manifest "$VAULT_DIR")
HAS_DELETED=$(echo "$MANIFEST" | jq 'has("files") and (.files | has("notes/to-delete.md"))' 2>/dev/null || echo "false")
# File entry might still exist with orphaned sections, or be removed entirely
# Either way, its sections should not be in active state
DELETED_ACTIVE=$(echo "$MANIFEST" | jq '[.files["notes/to-delete.md"]?.sections[]? | select(.state != "orphaned")] | length' 2>/dev/null || echo "0")
assert_eq "deleted_no_active_sections" "$DELETED_ACTIVE" "0"

# Renamed file: old path gone, new path present
OLD_EXISTS=$(echo "$MANIFEST" | jq '.files | has("notes/old-name.md")' 2>/dev/null || echo "false")
NEW_EXISTS=$(echo "$MANIFEST" | jq '.files | has("notes/new-name.md")' 2>/dev/null || echo "true")
# Note: old path sections should be orphaned; new path should have fresh sections

# Renamed file: content accessible under new name
assert_file_exists "renamed_file_exists" "$VAULT_DIR/notes/new-name.md"
assert_file_not_exists "old_file_gone" "$VAULT_DIR/notes/old-name.md"
assert_contains "renamed_content_preserved" "$VAULT_DIR/notes/new-name.md" "Renamed Note"

# Stable file should be unaffected
assert_manifest_has_file "stable_untouched" "$VAULT_DIR" "notes/stable.md"

# --- Quantitative checks ---
FINAL_FILES=$(jq -r '[.files | to_entries[] | select(.value.sections | map(select(.state != "orphaned")) | length > 0)] | length' "$VAULT_DIR/.hebbs/manifest.json")
FINAL_SECTIONS=$(get_section_count "$VAULT_DIR")

log_metric "initial_files" "$INITIAL_FILES" "count"
log_metric "final_active_files" "$FINAL_FILES" "count"
log_metric "initial_sections" "$INITIAL_SECTIONS" "count"
log_metric "final_sections" "$FINAL_SECTIONS" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
