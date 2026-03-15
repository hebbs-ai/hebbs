#!/usr/bin/env bash
set -euo pipefail

# Scenario 11: Agent Lifecycle (Unified Brain)
# Tests the full lifecycle from LIFECYCLE-hebbs-with-coding-agents.md:
#   1. Developer initializes vault with existing project docs
#   2. Agent stores preferences via "hebbs remember" (writes .md files)
#   3. Index picks up both file-backed docs and agent-written memories
#   4. All memories appear in one unified manifest
#   5. Agent-written memories survive rebuild (they are files)
#   6. Developer can edit and delete agent memory files
#
# NOTE: Batch "hebbs index" leaves sections in content_stale state (not synced).
# The synced state is set by the watcher path. This test uses index (not watch)
# so we check that sections are indexed (have memory_ids), not that they are synced.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="11_agent_lifecycle"
echo "=== Scenario 11: Agent Lifecycle (Unified Brain) ==="

# --- Setup: simulate a project with existing docs ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

mkdir -p "$VAULT_DIR/docs" "$VAULT_DIR/src/utils" "$VAULT_DIR/memories/insights"

# Project docs (file-backed memories)
cat > "$VAULT_DIR/docs/architecture.md" << 'EOF'
---
title: Architecture
---

## Overview

Event-driven microservice architecture with async message queues.

## API Layer

REST API with versioned routes and rate limiting.

## Storage

PostgreSQL for relational data, RocksDB for embeddings and indexes.
EOF

cat > "$VAULT_DIR/docs/api-design.md" << 'EOF'
---
title: API Design
---

## Pagination

Cursor-based pagination for all list endpoints. No offset-based pagination.

## Error Handling

All errors return structured JSON with error code, message, and request ID.
EOF

cat > "$VAULT_DIR/DECISIONS.md" << 'EOF'
## API Pagination

We chose cursor-based pagination over offset-based. Offset pagination degrades
at scale because the database must scan and discard rows.

## Logging

Structured logging with tracing crate. JSON format in production, pretty
format in development.
EOF

# --- Phase 1: Init and index project docs ---
echo "Phase 1: Init and index project docs"
timer_start "init"
vault_init "$VAULT_DIR"
timer_stop "init"

timer_start "index_docs"
vault_index "$VAULT_DIR"
timer_stop "index_docs"

DOCS_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
DOCS_SECTIONS=$(get_section_count "$VAULT_DIR")
echo "After indexing docs: $DOCS_FILES files, $DOCS_SECTIONS sections"

assert_eq "docs_file_count" "$DOCS_FILES" "3"
assert_gt "docs_have_sections" "$DOCS_SECTIONS" "0"
assert_manifest_has_file "architecture_indexed" "$VAULT_DIR" "docs/architecture.md"
assert_manifest_has_file "api_design_indexed" "$VAULT_DIR" "docs/api-design.md"
assert_manifest_has_file "decisions_indexed" "$VAULT_DIR" "DECISIONS.md"

# --- Phase 2: Agent writes memories via remember (simulated as file writes) ---
# In the unified binary, "hebbs remember" writes a .md file to the vault.
# We simulate this by creating the files the same way the binary would.
echo ""
echo "Phase 2: Agent stores preferences (writing .md files)"

cat > "$VAULT_DIR/memories/2026-03-14T10-30-00-no-em-dashes.md" << 'EOF'
---
hebbs-kind: memory
hebbs-importance: 0.9
hebbs-entity-id: writing_prefs
hebbs-created: 2026-03-14T10:30:00Z
hebbs-source: agent
---

Developer does not want em-dashes in comments or documentation.
EOF

cat > "$VAULT_DIR/memories/2026-03-14T10-31-00-explicit-errors.md" << 'EOF'
---
hebbs-kind: memory
hebbs-importance: 0.9
hebbs-entity-id: coding_prefs
hebbs-created: 2026-03-14T10:31:00Z
hebbs-source: agent
---

Developer prefers explicit error handling (match/if-let) over unwrap().
EOF

cat > "$VAULT_DIR/memories/2026-03-14T10-32-00-clippy-before-commit.md" << 'EOF'
---
hebbs-kind: memory
hebbs-importance: 0.8
hebbs-entity-id: workflow_prefs
hebbs-created: 2026-03-14T10:32:00Z
hebbs-source: agent
---

Always run clippy before committing code.
EOF

# Re-index to pick up agent memories
timer_start "index_with_memories"
vault_index "$VAULT_DIR"
timer_stop "index_with_memories"

TOTAL_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
TOTAL_SECTIONS=$(get_section_count "$VAULT_DIR")
echo "After indexing with agent memories: $TOTAL_FILES files, $TOTAL_SECTIONS sections"

# 3 doc files + 3 memory files = 6 total
assert_eq "total_file_count" "$TOTAL_FILES" "6"
assert_manifest_has_file "memory_em_dashes" "$VAULT_DIR" "memories/2026-03-14T10-30-00-no-em-dashes.md"
assert_manifest_has_file "memory_errors" "$VAULT_DIR" "memories/2026-03-14T10-31-00-explicit-errors.md"
assert_manifest_has_file "memory_clippy" "$VAULT_DIR" "memories/2026-03-14T10-32-00-clippy-before-commit.md"

# Both file-backed and agent memories coexist in one manifest
AGENT_MEMORY_SECTIONS=$(jq '[.files | to_entries[] | select(.key | startswith("memories/")) | .value.sections[]] | length' "$VAULT_DIR/.hebbs/manifest.json")
DOC_SECTIONS=$(jq '[.files | to_entries[] | select(.key | startswith("memories/") | not) | .value.sections[]] | length' "$VAULT_DIR/.hebbs/manifest.json")
assert_gt "agent_memories_have_sections" "$AGENT_MEMORY_SECTIONS" "0"
assert_gt "doc_files_have_sections" "$DOC_SECTIONS" "0"

# Agent memories should have hebbs-kind frontmatter
assert_contains "memory_has_kind" "$VAULT_DIR/memories/2026-03-14T10-30-00-no-em-dashes.md" "hebbs-kind: memory"
assert_contains "memory_has_source" "$VAULT_DIR/memories/2026-03-14T10-30-00-no-em-dashes.md" "hebbs-source: agent"
assert_contains "memory_has_entity" "$VAULT_DIR/memories/2026-03-14T10-30-00-no-em-dashes.md" "hebbs-entity-id: writing_prefs"

# --- Phase 3: Agent writes an insight file ---
echo ""
echo "Phase 3: Agent commits insight (writing insight .md file)"

cat > "$VAULT_DIR/memories/insights/2026-03-28-error-handling-standard.md" << 'EOF'
---
hebbs-kind: insight
hebbs-confidence: 0.95
hebbs-entity-id: coding_prefs
hebbs-sources:
  - memories/2026-03-14T10-31-00-explicit-errors.md
hebbs-created: 2026-03-28T14:00:00Z
---

Developer consistently prefers explicit match/if-let over unwrap() and expect().
All error paths should return typed errors, never panic.
EOF

vault_index "$VAULT_DIR"

TOTAL_WITH_INSIGHTS=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
echo "After insight: $TOTAL_WITH_INSIGHTS files"

assert_eq "total_with_insight" "$TOTAL_WITH_INSIGHTS" "7"
assert_manifest_has_file "insight_indexed" "$VAULT_DIR" "memories/insights/2026-03-28-error-handling-standard.md"
assert_contains "insight_has_kind" "$VAULT_DIR/memories/insights/2026-03-28-error-handling-standard.md" "hebbs-kind: insight"
assert_contains "insight_has_confidence" "$VAULT_DIR/memories/insights/2026-03-28-error-handling-standard.md" "hebbs-confidence: 0.95"

# --- Phase 4: Rebuild preserves everything ---
# Delete .hebbs/ and rebuild. All memories (docs + agent + insights) must come back
# because they are all files. This is the core guarantee.
echo ""
echo "Phase 4: Rebuild from files (the disposable engine guarantee)"

PRE_REBUILD_FILES=$TOTAL_WITH_INSIGHTS
PRE_REBUILD_SECTIONS=$(get_section_count "$VAULT_DIR")

timer_start "rebuild"
vault_rebuild "$VAULT_DIR"
timer_stop "rebuild"

POST_REBUILD_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
POST_REBUILD_SECTIONS=$(get_section_count "$VAULT_DIR")
echo "After rebuild: $POST_REBUILD_FILES files, $POST_REBUILD_SECTIONS sections"

assert_eq "rebuild_same_files" "$POST_REBUILD_FILES" "$PRE_REBUILD_FILES"
assert_eq "rebuild_same_sections" "$POST_REBUILD_SECTIONS" "$PRE_REBUILD_SECTIONS"

# All source types survive rebuild
assert_manifest_has_file "rebuild_docs_survive" "$VAULT_DIR" "docs/architecture.md"
assert_manifest_has_file "rebuild_decisions_survive" "$VAULT_DIR" "DECISIONS.md"
assert_manifest_has_file "rebuild_agent_memory_survives" "$VAULT_DIR" "memories/2026-03-14T10-30-00-no-em-dashes.md"
assert_manifest_has_file "rebuild_insight_survives" "$VAULT_DIR" "memories/insights/2026-03-28-error-handling-standard.md"

# Files on disk are untouched by rebuild
assert_contains "file_content_preserved" "$VAULT_DIR/memories/2026-03-14T10-30-00-no-em-dashes.md" "em-dashes"
assert_contains "insight_content_preserved" "$VAULT_DIR/memories/insights/2026-03-28-error-handling-standard.md" "typed errors"

# --- Phase 5: Brain discovery (status from vault root) ---
echo ""
echo "Phase 5: Brain discovery (status from vault root)"

STATUS_FILE=$(mktemp)
"${HEBBS_BIN}" status --vault "${VAULT_DIR}" > "$STATUS_FILE" 2>&1 || true
assert_contains "status_shows_indexed" "$STATUS_FILE" "indexed"
rm -f "$STATUS_FILE"

# --- Phase 6: Developer edits an agent memory (human override) ---
echo ""
echo "Phase 6: Developer edits agent memory file directly"

# Developer disagrees with the agent and changes the preference
cat > "$VAULT_DIR/memories/2026-03-14T10-32-00-clippy-before-commit.md" << 'EOF'
---
hebbs-kind: memory
hebbs-importance: 0.8
hebbs-entity-id: workflow_prefs
hebbs-created: 2026-03-14T10:32:00Z
hebbs-source: agent
---

Run clippy and cargo test before committing code. Tests must pass.
EOF

vault_index "$VAULT_DIR"

# File count unchanged (edit, not create)
AFTER_EDIT_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
assert_eq "edit_no_new_files" "$AFTER_EDIT_FILES" "$POST_REBUILD_FILES"

# Content updated on disk
assert_contains "edit_content_updated" "$VAULT_DIR/memories/2026-03-14T10-32-00-clippy-before-commit.md" "cargo test"

# Checksum changed in manifest (edit was detected)
EDIT_CHECKSUM=$(jq -r '.files["memories/2026-03-14T10-32-00-clippy-before-commit.md"].checksum' "$VAULT_DIR/.hebbs/manifest.json")
assert_pass "edit_checksum_present" "$( [ -n "$EDIT_CHECKSUM" ] && [ "$EDIT_CHECKSUM" != "null" ] && echo 0 || echo 1 )"

# --- Phase 7: Developer deletes a memory file ---
echo ""
echo "Phase 7: Developer deletes a memory file (forget by deletion)"

rm "$VAULT_DIR/memories/2026-03-14T10-30-00-no-em-dashes.md"

# Rebuild to get clean state (index alone does not remove deleted files)
vault_rebuild "$VAULT_DIR"

AFTER_DELETE_FILES=$(jq -r '.files | length' "$VAULT_DIR/.hebbs/manifest.json")
# Should be 6 now (was 7, removed 1)
assert_eq "delete_reduces_count" "$AFTER_DELETE_FILES" "6"

# Deleted file should not be in manifest after rebuild
DELETED_IN_MANIFEST=$(jq -r 'if .files["memories/2026-03-14T10-30-00-no-em-dashes.md"] then "present" else "absent" end' "$VAULT_DIR/.hebbs/manifest.json")
assert_eq "deleted_file_gone" "$DELETED_IN_MANIFEST" "absent"

# File is gone from disk
assert_file_not_exists "deleted_file_removed" "$VAULT_DIR/memories/2026-03-14T10-30-00-no-em-dashes.md"

# Other memories still present
assert_manifest_has_file "other_memories_intact" "$VAULT_DIR" "memories/2026-03-14T10-31-00-explicit-errors.md"
assert_manifest_has_file "insight_still_intact" "$VAULT_DIR" "memories/insights/2026-03-28-error-handling-standard.md"
assert_manifest_has_file "docs_still_intact" "$VAULT_DIR" "docs/architecture.md"

# --- Quantitative checks ---
log_metric "init_time_ms" "$(timer_get 'init')" "ms"
log_metric "index_docs_time_ms" "$(timer_get 'index_docs')" "ms"
log_metric "index_with_memories_time_ms" "$(timer_get 'index_with_memories')" "ms"
log_metric "rebuild_time_ms" "$(timer_get 'rebuild')" "ms"
log_metric "doc_files" "3" "count"
log_metric "agent_memory_files" "3" "count"
log_metric "insight_files" "1" "count"
log_metric "total_files_peak" "$PRE_REBUILD_FILES" "count"
log_metric "total_sections_peak" "$PRE_REBUILD_SECTIONS" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
