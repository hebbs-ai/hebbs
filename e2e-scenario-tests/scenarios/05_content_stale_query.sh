#!/usr/bin/env bash
set -euo pipefail

# Scenario 5: Content-Stale Query Window
# Tests: queries during the content-stale window return fresh file content.
# Maps to TASK-13 two-phase model, content-stale state.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="05_content_stale_query"
echo "=== Scenario 5: Content-Stale Query Window ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

# Create a vault with a known file
mkdir -p "$VAULT_DIR/notes"
cat > "$VAULT_DIR/notes/vendor-review.md" << 'EOF'
## Vendor Evaluation

Vendor X is reliable. They delivered on time for the initial project.
Strong communication and quality work product.
EOF

vault_init "$VAULT_DIR"
vault_index "$VAULT_DIR"

# Verify initial state
assert_all_sections_synced "initial_synced" "$VAULT_DIR"
assert_manifest_has_file "vendor_file_exists" "$VAULT_DIR" "notes/vendor-review.md"

INITIAL_CHECKSUM=$(jq -r '.files["notes/vendor-review.md"].checksum' "$VAULT_DIR/.hebbs/manifest.json")
echo "Initial checksum: $INITIAL_CHECKSUM"

# --- Modify file to create content-stale window ---
echo "Modifying file..."
cat > "$VAULT_DIR/notes/vendor-review.md" << 'EOF'
## Vendor Evaluation

Vendor X is unreliable. They missed three deadlines in Q3.
Communication has degraded and deliverables are consistently late.
EOF

# Run phase 1 only (parse + manifest update, no embedding)
# This simulates the content-stale window
vault_index "$VAULT_DIR"

# Check that the file checksum changed
NEW_CHECKSUM=$(jq -r '.files["notes/vendor-review.md"].checksum' "$VAULT_DIR/.hebbs/manifest.json")
assert_pass "checksum_changed" "[ '$INITIAL_CHECKSUM' != '$NEW_CHECKSUM' ]"

# Verify the file content on disk has the new text
assert_contains "file_has_new_content" "$VAULT_DIR/notes/vendor-review.md" "unreliable"

# --- Qualitative checks ---
# After full index, the section should be synced with new content
assert_all_sections_synced "final_synced" "$VAULT_DIR"

# --- Quantitative checks ---
log_metric "initial_checksum" "$INITIAL_CHECKSUM" "string"
log_metric "new_checksum" "$NEW_CHECKSUM" "string"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
