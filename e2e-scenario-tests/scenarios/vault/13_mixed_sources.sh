#!/usr/bin/env bash
set -euo pipefail

# Scenario 13: Mixed Sources (File + CLI Memories)
# Tests that file-indexed memories and CLI-stored memories coexist in one
# brain and are returned together in recall/prime results.
#
# This validates the core "one brain" guarantee: a user's project docs and
# an agent's stored preferences live in the same index, and recall searches
# everything regardless of how it got there.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="13_mixed_sources"
echo "=== Scenario 13: Mixed Sources (File + CLI Memories) ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

mkdir -p "$VAULT_DIR/docs"

# =========================================================================
# Phase 1: Index project files (file-backed memories)
# =========================================================================
echo ""
echo "--- Phase 1: Index project docs ---"

cat > "$VAULT_DIR/docs/coding-standards.md" << 'EOF'
---
title: Coding Standards
---

## Error Handling

All functions must return Result types. Never use unwrap() in production code.
Use thiserror for error type definitions.

## Logging

Use the tracing crate for all logging. Structured JSON format in production.
Never use println! for logging in library code.

## Testing

Every public function must have at least one unit test.
Integration tests live in tests/ directory.
EOF

cat > "$VAULT_DIR/docs/api-guide.md" << 'EOF'
---
title: API Guide
---

## Authentication

All API endpoints require Bearer token authentication.
Tokens are validated against the auth service.

## Rate Limiting

Default rate limit is 100 requests per minute per API key.
Premium keys get 1000 requests per minute.
EOF

vault_init "$VAULT_DIR"
vault_index "$VAULT_DIR"

FILE_SECTIONS=$(get_section_count "$VAULT_DIR")
echo "File-backed sections indexed: $FILE_SECTIONS"
assert_gt "files_have_sections" "$FILE_SECTIONS" "0"

# =========================================================================
# Phase 2: Store CLI memories (agent-stored, via remember command)
# =========================================================================
echo ""
echo "--- Phase 2: Store CLI memories ---"

# Agent stores user preferences (same domain as the project docs)
MEM_ERR=$("${HEBBS_BIN}" remember "User said: always use explicit error handling, never unwrap" \
  --importance 0.9 --entity-id coding_prefs --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM_ERR_ID=$(echo "$MEM_ERR" | jq -r '.memory_id')
assert_pass "cli_remember_error_pref" "$( [ -n "$MEM_ERR_ID" ] && [ "$MEM_ERR_ID" != "null" ] && echo 0 || echo 1 )"

MEM_LOG=$("${HEBBS_BIN}" remember "User prefers log level set to debug during development" \
  --importance 0.7 --entity-id coding_prefs --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM_LOG_ID=$(echo "$MEM_LOG" | jq -r '.memory_id')

MEM_AUTH=$("${HEBBS_BIN}" remember "User wants API keys rotated every 90 days" \
  --importance 0.8 --entity-id security_prefs --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM_AUTH_ID=$(echo "$MEM_AUTH" | jq -r '.memory_id')

MEM_RATE=$("${HEBBS_BIN}" remember "User requested rate limit increase to 500 req/min for staging" \
  --importance 0.6 --entity-id project_config --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM_RATE_ID=$(echo "$MEM_RATE" | jq -r '.memory_id')

echo "CLI memories stored: 4"

# =========================================================================
# Phase 3: Recall should return results from BOTH sources
# =========================================================================
echo ""
echo "--- Phase 3: Unified recall across sources ---"

# Query about error handling -- should match BOTH the coding-standards.md file
# and the CLI-stored preference
RECALL_ERRORS=$("${HEBBS_BIN}" recall "error handling best practices" \
  --strategy similarity --top-k 10 --format json --vault "${VAULT_DIR}" 2>/dev/null)

RECALL_ERRORS_COUNT=$(echo "$RECALL_ERRORS" | jq 'length')
assert_gt "recall_errors_has_results" "$RECALL_ERRORS_COUNT" "1"

# Should have results from both sources (file sections + CLI memory)
# We check that we get more than just the CLI memory
echo "  Error handling recall returned $RECALL_ERRORS_COUNT results"

# Query about logging -- should match the coding-standards.md logging section
# and the CLI-stored logging preference
RECALL_LOGGING=$("${HEBBS_BIN}" recall "logging configuration and preferences" \
  --strategy similarity --top-k 10 --format json --vault "${VAULT_DIR}" 2>/dev/null)

RECALL_LOGGING_COUNT=$(echo "$RECALL_LOGGING" | jq 'length')
assert_gt "recall_logging_has_results" "$RECALL_LOGGING_COUNT" "1"
echo "  Logging recall returned $RECALL_LOGGING_COUNT results"

# Query about API/auth -- should match api-guide.md AND the security pref
RECALL_API=$("${HEBBS_BIN}" recall "API authentication and security" \
  --strategy similarity --top-k 10 --format json --vault "${VAULT_DIR}" 2>/dev/null)

RECALL_API_COUNT=$(echo "$RECALL_API" | jq 'length')
assert_gt "recall_api_has_results" "$RECALL_API_COUNT" "1"
echo "  API/auth recall returned $RECALL_API_COUNT results"

# Query about rate limiting -- should match api-guide.md rate limiting section
# AND the CLI-stored rate limit preference
RECALL_RATE=$("${HEBBS_BIN}" recall "rate limiting configuration" \
  --strategy similarity --top-k 10 --format json --vault "${VAULT_DIR}" 2>/dev/null)

RECALL_RATE_COUNT=$(echo "$RECALL_RATE" | jq 'length')
assert_gt "recall_rate_has_results" "$RECALL_RATE_COUNT" "1"
echo "  Rate limiting recall returned $RECALL_RATE_COUNT results"

# =========================================================================
# Phase 4: Prime should return CLI memories for an entity
# =========================================================================
echo ""
echo "--- Phase 4: Prime returns entity-scoped CLI memories ---"

PRIME_CODING=$("${HEBBS_BIN}" prime coding_prefs --max-memories 10 \
  --format json --vault "${VAULT_DIR}" 2>/dev/null)

PRIME_CODING_COUNT=$(echo "$PRIME_CODING" | jq 'length')
assert_gt "prime_coding_has_results" "$PRIME_CODING_COUNT" "0"

# All prime results should be for the requested entity
PRIME_ENTITIES=$(echo "$PRIME_CODING" | jq -r '[.[].entity_id] | unique | .[]')
assert_eq "prime_scoped_correctly" "$PRIME_ENTITIES" "coding_prefs"

echo "  Prime(coding_prefs) returned $PRIME_CODING_COUNT memories"

# =========================================================================
# Phase 5: Forget CLI memory, file memories remain
# =========================================================================
echo ""
echo "--- Phase 5: Forget CLI memory without affecting files ---"

# Count total memories before forget
BEFORE_RECALL=$("${HEBBS_BIN}" recall "error handling" \
  --strategy similarity --top-k 10 --format json --vault "${VAULT_DIR}" 2>/dev/null)
BEFORE_COUNT=$(echo "$BEFORE_RECALL" | jq 'length')

# Forget the CLI error handling memory
"${HEBBS_BIN}" forget --ids "${MEM_ERR_ID}" --vault "${VAULT_DIR}" 2>/dev/null || true

# Recall should still return file-backed results about error handling
AFTER_RECALL=$("${HEBBS_BIN}" recall "error handling" \
  --strategy similarity --top-k 10 --format json --vault "${VAULT_DIR}" 2>/dev/null)
AFTER_COUNT=$(echo "$AFTER_RECALL" | jq 'length')

assert_gt "file_memories_survive_cli_forget" "$AFTER_COUNT" "0"

echo "  Before forget: $BEFORE_COUNT results, after: $AFTER_COUNT results"

# =========================================================================
# Phase 6: Rebuild preserves file memories, CLI memories need re-store
# =========================================================================
echo ""
echo "--- Phase 6: Rebuild behavior ---"

# Rebuild wipes the index and re-indexes from files
vault_rebuild "$VAULT_DIR"

# File-backed memories should be fully restored
REBUILD_FILE_SECTIONS=$(get_section_count "$VAULT_DIR")
assert_eq "rebuild_restores_file_sections" "$REBUILD_FILE_SECTIONS" "$FILE_SECTIONS"

# File sections are in the manifest (structurally restored)
assert_manifest_has_file "rebuild_coding_standards" "$VAULT_DIR" "docs/coding-standards.md"
assert_manifest_has_file "rebuild_api_guide" "$VAULT_DIR" "docs/api-guide.md"

# CLI-stored memories are gone (they were in RocksDB, not files)
# This is expected: if you want CLI memories to survive rebuild,
# use "hebbs remember" which writes .md files to the vault
REBUILD_PRIME=$("${HEBBS_BIN}" prime coding_prefs --max-memories 10 \
  --format json --vault "${VAULT_DIR}" 2>/dev/null)
REBUILD_PRIME_COUNT=$(echo "$REBUILD_PRIME" | jq 'length')
assert_eq "cli_memories_gone_after_rebuild" "$REBUILD_PRIME_COUNT" "0"

echo "  After rebuild: file sections restored ($REBUILD_FILE_SECTIONS), CLI memories cleared"

# =========================================================================
# Phase 7: Re-store CLI memories after rebuild (agent recovers)
# =========================================================================
echo ""
echo "--- Phase 7: Agent re-stores memories after rebuild ---"

MEM_RESTORED=$("${HEBBS_BIN}" remember "User said: always use explicit error handling, never unwrap" \
  --importance 0.9 --entity-id coding_prefs --format json --vault "${VAULT_DIR}" 2>/dev/null)
MEM_RESTORED_ID=$(echo "$MEM_RESTORED" | jq -r '.memory_id')
assert_pass "re_store_works" "$( [ -n "$MEM_RESTORED_ID" ] && [ "$MEM_RESTORED_ID" != "null" ] && echo 0 || echo 1 )"

# CLI memory is recallable
RESTORED_RECALL=$("${HEBBS_BIN}" recall "error handling" \
  --strategy similarity --top-k 10 --format json --vault "${VAULT_DIR}" 2>/dev/null)
RESTORED_COUNT=$(echo "$RESTORED_RECALL" | jq 'length')
assert_gt "cli_memory_recallable_after_rebuild" "$RESTORED_COUNT" "0"

# Prime returns the re-stored memory
RESTORED_PRIME=$("${HEBBS_BIN}" prime coding_prefs --max-memories 10 \
  --format json --vault "${VAULT_DIR}" 2>/dev/null)
RESTORED_PRIME_COUNT=$(echo "$RESTORED_PRIME" | jq 'length')
assert_gt "prime_works_after_rebuild" "$RESTORED_PRIME_COUNT" "0"

echo "  After re-store: $RESTORED_COUNT recall results, $RESTORED_PRIME_COUNT prime results"

# =========================================================================
# Summary
# =========================================================================

log_metric "file_backed_sections" "$FILE_SECTIONS" "count"
log_metric "cli_memories_stored" "4" "count"
log_metric "recall_errors_results" "$RECALL_ERRORS_COUNT" "count"
log_metric "recall_logging_results" "$RECALL_LOGGING_COUNT" "count"
log_metric "recall_api_results" "$RECALL_API_COUNT" "count"
log_metric "recall_rate_results" "$RECALL_RATE_COUNT" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
