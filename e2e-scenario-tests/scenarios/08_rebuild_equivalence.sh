#!/usr/bin/env bash
set -euo pipefail

# Scenario 8: Rebuild Equivalence
# Tests: rebuild produces functionally equivalent index.
# Maps to TASK-13 Milestone 7 (rebuild guarantee).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="08_rebuild_equivalence"
echo "=== Scenario 8: Rebuild Equivalence ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

# Create vault with wiki-links and insight files
mkdir -p "$VAULT_DIR/notes" "$VAULT_DIR/insights"

cat > "$VAULT_DIR/notes/project-overview.md" << 'EOF'
---
title: Project Overview
author: team-lead
---

## Goals

Build a scalable data pipeline for real-time analytics.
See [[api-design]] for the technical approach.

## Timeline

Q1: Design phase. Q2: Implementation. See [[meeting-notes]] for weekly updates.
EOF

cat > "$VAULT_DIR/notes/api-design.md" << 'EOF'
## Endpoints

REST API with versioned routes. See [[project-overview]] for context.

## Authentication

OAuth2 with JWT tokens. #security #api/auth
EOF

cat > "$VAULT_DIR/notes/meeting-notes.md" << 'EOF'
## Week 1

Kicked off the project. Defined initial requirements.
See [[project-overview#goals]] for the full list.

## Week 2

API design review complete. Moving to implementation.
EOF

cat > "$VAULT_DIR/insights/01ABC-pipeline-design-pattern.md" << 'EOF'
---
hebbs-kind: insight
hebbs-sources:
  - notes/project-overview.md#goals
  - notes/api-design.md#endpoints
hebbs-confidence: 0.75
hebbs-created: 2026-03-14T12:00:00Z
---

The pipeline design follows a standard event-driven architecture pattern,
combining REST endpoints with async processing queues.
EOF

vault_init "$VAULT_DIR"

# --- Initial index ---
timer_start "initial_index"
vault_index "$VAULT_DIR"
timer_stop "initial_index"

# Capture pre-rebuild state
PRE_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
PRE_SECTIONS=$(get_section_count "$VAULT_DIR")
PRE_MANIFEST=$(get_manifest "$VAULT_DIR")

# Save pre-rebuild section details for comparison
PRE_SECTION_DETAILS=$(echo "$PRE_MANIFEST" | jq '[.files | to_entries[] | .value.sections[] | {heading_path, content_checksum}] | sort_by(.heading_path)')

echo "Pre-rebuild: $PRE_FILES files, $PRE_SECTIONS sections"

# --- Rebuild ---
timer_start "rebuild"
vault_rebuild "$VAULT_DIR"
timer_stop "rebuild"

# Capture post-rebuild state
POST_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
POST_SECTIONS=$(get_section_count "$VAULT_DIR")
POST_MANIFEST=$(get_manifest "$VAULT_DIR")
POST_SECTION_DETAILS=$(echo "$POST_MANIFEST" | jq '[.files | to_entries[] | .value.sections[] | {heading_path, content_checksum}] | sort_by(.heading_path)')

echo "Post-rebuild: $POST_FILES files, $POST_SECTIONS sections"

# --- Qualitative checks ---
assert_eq "same_file_count" "$POST_FILES" "$PRE_FILES"
assert_eq "same_section_count" "$POST_SECTIONS" "$PRE_SECTIONS"
assert_all_sections_synced "all_synced_after_rebuild" "$VAULT_DIR"

# Content checksums should match (same content = same checksums)
assert_eq "section_details_match" "$POST_SECTION_DETAILS" "$PRE_SECTION_DETAILS"

# Insight files survive rebuild (they're in the vault, not .hebbs/)
assert_file_exists "insight_survives_rebuild" "$VAULT_DIR/insights/01ABC-pipeline-design-pattern.md"
assert_manifest_has_file "insight_reindexed" "$VAULT_DIR" "insights/01ABC-pipeline-design-pattern.md"

# Config preserved
assert_file_exists "config_preserved" "$VAULT_DIR/.hebbs/config.toml"

# Wiki-linked files all present
assert_manifest_has_file "overview_present" "$VAULT_DIR" "notes/project-overview.md"
assert_manifest_has_file "api_present" "$VAULT_DIR" "notes/api-design.md"
assert_manifest_has_file "meetings_present" "$VAULT_DIR" "notes/meeting-notes.md"

# --- Quantitative checks ---
INITIAL_MS=$(timer_get "initial_index")
REBUILD_MS=$(timer_get "rebuild")

log_metric "initial_index_time_ms" "$INITIAL_MS" "ms"
log_metric "rebuild_time_ms" "$REBUILD_MS" "ms"
log_metric "pre_files" "$PRE_FILES" "count"
log_metric "post_files" "$POST_FILES" "count"
log_metric "pre_sections" "$PRE_SECTIONS" "count"
log_metric "post_sections" "$POST_SECTIONS" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
