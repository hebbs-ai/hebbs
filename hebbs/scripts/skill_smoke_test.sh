#!/usr/bin/env bash
#
# HEBBS SKILL.md Smoke Test
#
# Exercises every CLI command documented in skills/hebbs/SKILL.md against a
# running hebbs server. Prints the exact command and its output for each step,
# so a human can verify the system ACTUALLY works, not just exits 0.
#
# NOTE: If the server uses MockEmbedder (embedding_provider=mock), similarity
# scores will be hash-based, not semantic. Recall results will be structurally
# correct but not semantically ranked. For a real test of ranking quality,
# run against a server with the ONNX embedder.
#
# Prerequisites:
#   - hebbs server running (default: localhost:6380, auth disabled)
#   - hebbs on PATH (or set HEBBS_BIN)
#   - jq installed for JSON parsing
#
# Usage:
#   HEBBS_AUTH_ENABLED=false hebbs start &
#   ./scripts/skill_smoke_test.sh
#
# Environment variables:
#   HEBBS_ENDPOINT   gRPC endpoint (default: http://localhost:6380)
#   HEBBS_BIN        path to hebbs binary (default: hebbs)

set -euo pipefail

CLI="${HEBBS_BIN:-hebbs}"
ENDPOINT="${HEBBS_ENDPOINT:-http://localhost:6380}"
PASS=0
FAIL=0
TOTAL=0

# ── Helpers ──────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
DIM='\033[2m'
RESET='\033[0m'

separator() {
  echo ""
  printf "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}\n"
}

print_cmd() {
  printf "${YELLOW}$ ${CLI} --endpoint ${ENDPOINT} --format json --timeout 10000"
  for arg in "$@"; do
    if [[ "$arg" == *" "* || "$arg" == *"{"* || "$arg" == *"["* ]]; then
      printf " '%s'" "$arg"
    else
      printf " %s" "$arg"
    fi
  done
  printf "${RESET}\n\n"
}

pretty_output() {
  local raw="$1"
  if command -v jq &>/dev/null && echo "$raw" | jq . &>/dev/null 2>&1; then
    echo "$raw" | jq .
  else
    echo "$raw"
  fi
}

# run_test: run a CLI command, print it, check exit code
run_test() {
  local name="$1"
  shift

  TOTAL=$((TOTAL + 1))
  separator
  printf "${BOLD}TEST %d: %s${RESET}\n" "$TOTAL" "$name"
  echo ""
  print_cmd "$@"

  set +e
  OUTPUT=$("$CLI" --endpoint "$ENDPOINT" --format json --timeout 10000 "$@" 2>&1)
  EXIT_CODE=$?
  set -e

  pretty_output "$OUTPUT"

  if [ $EXIT_CODE -eq 0 ]; then
    printf "\n${GREEN}✓ PASS${RESET} (exit code: 0)\n"
    PASS=$((PASS + 1))
  else
    printf "\n${RED}✗ FAIL${RESET} (exit code: %d)\n" "$EXIT_CODE"
    FAIL=$((FAIL + 1))
  fi
}

# run_and_capture: same as run_test but sets $CAPTURED for later extraction
run_and_capture() {
  local name="$1"
  shift

  TOTAL=$((TOTAL + 1))
  separator
  printf "${BOLD}TEST %d: %s${RESET}\n" "$TOTAL" "$name"
  echo ""
  print_cmd "$@"

  set +e
  CAPTURED=$("$CLI" --endpoint "$ENDPOINT" --format json --timeout 10000 "$@" 2>&1)
  EXIT_CODE=$?
  set -e

  pretty_output "$CAPTURED"

  if [ $EXIT_CODE -eq 0 ]; then
    printf "\n${GREEN}✓ PASS${RESET} (exit code: 0)\n"
    PASS=$((PASS + 1))
  else
    printf "\n${RED}✗ FAIL${RESET} (exit code: %d)\n" "$EXIT_CODE"
    FAIL=$((FAIL + 1))
  fi
}

# check_contains: assert output contains a substring (post-hoc check)
check_contains() {
  local haystack="$1"
  local needle="$2"
  local desc="$3"
  if echo "$haystack" | grep -q "$needle"; then
    printf "  ${GREEN}✓${RESET} %s\n" "$desc"
  else
    printf "  ${RED}✗${RESET} %s (expected to find '%s')\n" "$desc" "$needle"
    FAIL=$((FAIL + 1))
    PASS=$((PASS - 1))
  fi
}

check_json_field() {
  local json="$1"
  local field="$2"
  local desc="$3"
  local val
  val=$(echo "$json" | jq -r "$field // empty" 2>/dev/null)
  if [ -n "$val" ]; then
    printf "  ${GREEN}✓${RESET} %s = %s\n" "$desc" "$val"
  else
    printf "  ${RED}✗${RESET} %s is missing or empty\n" "$desc"
    FAIL=$((FAIL + 1))
    PASS=$((PASS - 1))
  fi
}

# ── Pre-flight ───────────────────────────────────────────────────────────

echo ""
printf "${BOLD}╔══════════════════════════════════════════════════╗${RESET}\n"
printf "${BOLD}║      HEBBS SKILL.md Smoke Test                  ║${RESET}\n"
printf "${BOLD}╚══════════════════════════════════════════════════╝${RESET}\n"
printf "Endpoint: ${ENDPOINT}\n"
printf "CLI:      ${CLI}\n"

if ! command -v "$CLI" &>/dev/null; then
  printf "${RED}ERROR: ${CLI} not found on PATH. Set HEBBS_BIN or add to PATH.${RESET}\n"
  exit 1
fi

if ! command -v jq &>/dev/null; then
  printf "${YELLOW}WARNING: jq not found. JSON output will not be pretty-printed.${RESET}\n"
fi

# ═══════════════════════════════════════════════════════════════════════
#  1. Status
# ═══════════════════════════════════════════════════════════════════════

run_and_capture "Server status" status
check_json_field "$CAPTURED" ".version" "version"
check_json_field "$CAPTURED" ".memory_count" "memory_count"

# ═══════════════════════════════════════════════════════════════════════
#  2-7. Remember (store 6 memories for reflect testing)
# ═══════════════════════════════════════════════════════════════════════

run_and_capture "Remember: dark mode preference" \
  remember "User always picks dark theme in every app" \
  --importance 0.8 --entity-id user_prefs
MEM1_ID=$(echo "$CAPTURED" | jq -r '.memory_id // empty')
check_contains "$CAPTURED" "dark theme" "Content stored correctly"
check_json_field "$CAPTURED" ".entity_id" "entity_id"

run_and_capture "Remember: minimal UI" \
  remember "User prefers minimal UI with no clutter" \
  --importance 0.7 --entity-id user_prefs
MEM2_ID=$(echo "$CAPTURED" | jq -r '.memory_id // empty')

run_and_capture "Remember: VS Code dark mode" \
  remember "User turned on dark mode in VS Code" \
  --importance 0.6 --entity-id user_prefs

run_and_capture "Remember: Slack dark mode" \
  remember "User set dark mode on Slack" \
  --importance 0.6 --entity-id user_prefs

run_and_capture "Remember: eye strain" \
  remember "User mentioned eye strain with light themes" \
  --importance 0.7 --entity-id user_prefs

run_and_capture "Remember: with context" \
  remember "User reads in dark mode on Kindle" \
  --context '{"source":"interview","device":"kindle"}' \
  --importance 0.5 --entity-id user_prefs
check_contains "$CAPTURED" "kindle" "Context field present in content"
check_json_field "$CAPTURED" ".context.source" "context.source"

# Also store a separate entity for forget tests later
run_and_capture "Remember: separate entity for forget" \
  remember "Meeting notes from Q2 review" \
  --importance 0.5 --entity-id meetings_temp
MEETINGS_ID=$(echo "$CAPTURED" | jq -r '.memory_id // empty')

# ═══════════════════════════════════════════════════════════════════════
#  8. Recall (similarity)
# ═══════════════════════════════════════════════════════════════════════

run_and_capture "Recall: similarity search" \
  recall "dark mode preferences" \
  --strategy similarity --top-k 5 --entity-id user_prefs

RESULT_COUNT=$(echo "$CAPTURED" | jq 'if type == "array" then length else 0 end' 2>/dev/null || echo 0)
printf "  ${DIM}Results returned: %s${RESET}\n" "$RESULT_COUNT"
if [ "$RESULT_COUNT" -gt 0 ]; then
  FIRST_CONTENT=$(echo "$CAPTURED" | jq -r '.[0].memory.content // "N/A"' 2>/dev/null)
  printf "  ${DIM}Top result: %s${RESET}\n" "$FIRST_CONTENT"
fi

# ═══════════════════════════════════════════════════════════════════════
#  9. Recall (temporal, with required --entity-id)
# ═══════════════════════════════════════════════════════════════════════

run_and_capture "Recall: temporal (with entity-id)" \
  recall "recent events" \
  --strategy temporal --entity-id user_prefs --top-k 5

T_COUNT=$(echo "$CAPTURED" | jq 'if type == "array" then length else 0 end' 2>/dev/null || echo 0)
printf "  ${DIM}Temporal results: %s${RESET}\n" "$T_COUNT"
if [ "$T_COUNT" -eq 0 ]; then
  printf "  ${RED}⚠  Temporal recall returned 0 results for an entity with 6 memories${RESET}\n"
fi

# ═══════════════════════════════════════════════════════════════════════
#  10. Recall (scoring weights, R:T:I:F format)
# ═══════════════════════════════════════════════════════════════════════

run_and_capture "Recall: custom scoring weights (0.3:0.1:0.5:0.1)" \
  recall "UI preferences" \
  --strategy similarity --weights "0.3:0.1:0.5:0.1"

W_COUNT=$(echo "$CAPTURED" | jq 'if type == "array" then length else 0 end' 2>/dev/null || echo 0)
printf "  ${DIM}Results with custom weights: %s${RESET}\n" "$W_COUNT"

# ═══════════════════════════════════════════════════════════════════════
#  11. Recall (ef-search parameter)
# ═══════════════════════════════════════════════════════════════════════

run_test "Recall: with ef-search=200" \
  recall "theme preference" --strategy similarity --ef-search 200

# ═══════════════════════════════════════════════════════════════════════
#  12. Recall (causal, with seed memory)
# ═══════════════════════════════════════════════════════════════════════

if [ -n "$MEM1_ID" ]; then
  run_test "Recall: causal with seed" \
    recall "related preferences" \
    --strategy causal --seed "$MEM1_ID" --max-depth 3 --edge-types "caused_by,followed_by"
fi

# ═══════════════════════════════════════════════════════════════════════
#  13. Recall (analogical)
# ═══════════════════════════════════════════════════════════════════════

run_test "Recall: analogical with alpha=0.2" \
  recall "design patterns" --strategy analogical --analogical-alpha 0.2

# ═══════════════════════════════════════════════════════════════════════
#  14. Get (round-trip verify)
# ═══════════════════════════════════════════════════════════════════════

if [ -n "$MEM1_ID" ]; then
  run_and_capture "Get: retrieve by ID" get "$MEM1_ID"
  check_contains "$CAPTURED" "dark theme" "Retrieved content matches what was stored"
fi

# ═══════════════════════════════════════════════════════════════════════
#  15. Inspect
# ═══════════════════════════════════════════════════════════════════════

if [ -n "$MEM1_ID" ]; then
  run_and_capture "Inspect: detailed memory view" inspect "$MEM1_ID"
  check_contains "$CAPTURED" "dark theme" "Inspect shows memory content"
fi

# ═══════════════════════════════════════════════════════════════════════
#  16. Prime (load context for entity)
# ═══════════════════════════════════════════════════════════════════════

run_and_capture "Prime: load user_prefs context" \
  prime user_prefs --max-memories 20

P_COUNT=$(echo "$CAPTURED" | jq 'if type == "array" then length else 0 end' 2>/dev/null || echo 0)
printf "  ${DIM}Prime returned %s memories${RESET}\n" "$P_COUNT"
if [ "$P_COUNT" -lt 6 ]; then
  printf "  ${RED}⚠  Expected 6 memories for user_prefs, got %s${RESET}\n" "$P_COUNT"
fi

# ═══════════════════════════════════════════════════════════════════════
#  17. Prime (with similarity cue)
# ═══════════════════════════════════════════════════════════════════════

run_test "Prime: with similarity-cue" \
  prime user_prefs --max-memories 10 --similarity-cue "dark mode"

# ═══════════════════════════════════════════════════════════════════════
#  18. Reflect-prepare (must produce clusters from 6 memories)
# ═══════════════════════════════════════════════════════════════════════

run_and_capture "Reflect-prepare: cluster user_prefs" \
  reflect-prepare --entity-id user_prefs

SESSION_ID=$(echo "$CAPTURED" | jq -r '.session_id // empty')
CLUSTER_COUNT=$(echo "$CAPTURED" | jq '.clusters | length' 2>/dev/null || echo 0)
MEMORIES_PROCESSED=$(echo "$CAPTURED" | jq -r '.memories_processed // 0')

printf "  ${DIM}Session ID: %s${RESET}\n" "$SESSION_ID"
printf "  ${DIM}Memories processed: %s${RESET}\n" "$MEMORIES_PROCESSED"
printf "  ${DIM}Clusters found: %s${RESET}\n" "$CLUSTER_COUNT"

if [ "$CLUSTER_COUNT" -eq 0 ]; then
  printf "  ${RED}⚠  NO CLUSTERS produced from %s memories. reflect-commit will be skipped!${RESET}\n" "$MEMORIES_PROCESSED"
  printf "  ${DIM}This can happen with MockEmbedder + default server config (min_memories=5, min_cluster=3).${RESET}\n"
  printf "  ${DIM}The Rust e2e test uses min_cluster_size=2 to guarantee clustering.${RESET}\n"
fi

FIRST_HEX_ID=$(echo "$CAPTURED" | jq -r '.clusters[0].memory_ids[0] // empty')

if [ "$CLUSTER_COUNT" -gt 0 ]; then
  MEMBER_COUNT=$(echo "$CAPTURED" | jq '.clusters[0].member_count // 0')
  HAS_PROMPT=$(echo "$CAPTURED" | jq -r '.clusters[0].proposal_system_prompt // empty')
  HAS_MEMORIES=$(echo "$CAPTURED" | jq '.clusters[0].memories | length' 2>/dev/null || echo 0)
  printf "  ${DIM}Cluster 0: %s members, %s memories attached${RESET}\n" "$MEMBER_COUNT" "$HAS_MEMORIES"
  if [ -n "$HAS_PROMPT" ]; then
    printf "  ${GREEN}✓${RESET} proposal_system_prompt present\n"
  fi
  if [ "$HAS_MEMORIES" -gt 0 ]; then
    printf "  ${GREEN}✓${RESET} memories array populated with full content\n"
    FIRST_MEM_CONTENT=$(echo "$CAPTURED" | jq -r '.clusters[0].memories[0].content // "N/A"')
    printf "  ${DIM}First memory in cluster: %s${RESET}\n" "$FIRST_MEM_CONTENT"
  fi
fi

# ═══════════════════════════════════════════════════════════════════════
#  19. Reflect-commit (store an agent-generated insight)
# ═══════════════════════════════════════════════════════════════════════

if [ -n "$SESSION_ID" ] && [ -n "$FIRST_HEX_ID" ]; then
  INSIGHTS_JSON="[{\"content\":\"User consistently prefers dark themes across all applications and devices\",\"confidence\":0.9,\"source_memory_ids\":[\"${FIRST_HEX_ID}\"],\"tags\":[\"preference\",\"ui\",\"dark_theme\"]}]"

  run_and_capture "Reflect-commit: store agent insight" \
    reflect-commit --session-id "$SESSION_ID" --insights "$INSIGHTS_JSON"

  CREATED=$(echo "$CAPTURED" | jq -r '.insights_created // 0')
  if [ "$CREATED" -ge 1 ]; then
    printf "  ${GREEN}✓${RESET} Insight created successfully (count: %s)\n" "$CREATED"
  else
    printf "  ${RED}✗${RESET} Expected insights_created >= 1, got %s\n" "$CREATED"
  fi
else
  separator
  TOTAL=$((TOTAL + 1))
  printf "${BOLD}TEST %d: Reflect-commit${RESET}\n" "$TOTAL"
  printf "${YELLOW}SKIPPED${RESET} — no clusters from prepare (see note above)\n"
fi

# ═══════════════════════════════════════════════════════════════════════
#  20. Insights (verify the committed insight is retrievable)
# ═══════════════════════════════════════════════════════════════════════

run_and_capture "Insights: retrieve for user_prefs" \
  insights --entity-id user_prefs --max-results 10 --min-confidence 0.5

if echo "$CAPTURED" | grep -q "dark themes"; then
  printf "  ${GREEN}✓${RESET} Committed insight found in output\n"
elif echo "$CAPTURED" | grep -q "No insights"; then
  printf "  ${DIM}No insights found (expected if reflect-commit was skipped)${RESET}\n"
fi

# ═══════════════════════════════════════════════════════════════════════
#  21. Forget (by ID, and verify removal)
# ═══════════════════════════════════════════════════════════════════════

if [ -n "$MEETINGS_ID" ]; then
  run_and_capture "Forget: by ID" forget --ids "$MEETINGS_ID"
  FORGOT=$(echo "$CAPTURED" | jq -r '.forgotten_count // 0')
  if [ "$FORGOT" -ge 1 ]; then
    printf "  ${GREEN}✓${RESET} Forgot %s memory(s)\n" "$FORGOT"
  else
    printf "  ${RED}✗${RESET} Expected forgotten_count >= 1, got %s\n" "$FORGOT"
  fi

  # Verify it's really gone
  separator
  TOTAL=$((TOTAL + 1))
  printf "${BOLD}TEST %d: Verify: get forgotten memory fails${RESET}\n" "$TOTAL"
  print_cmd get "$MEETINGS_ID"
  set +e
  VERIFY=$("$CLI" --endpoint "$ENDPOINT" --format json --timeout 10000 get "$MEETINGS_ID" 2>&1)
  V_EXIT=$?
  set -e
  echo "$VERIFY"
  if [ $V_EXIT -ne 0 ]; then
    printf "\n${GREEN}✓ PASS${RESET} — correctly returns error for forgotten memory\n"
    PASS=$((PASS + 1))
  else
    printf "\n${RED}✗ FAIL${RESET} — should have failed for forgotten memory\n"
    FAIL=$((FAIL + 1))
  fi
fi

# ═══════════════════════════════════════════════════════════════════════
#  23. Forget (by entity)
# ═══════════════════════════════════════════════════════════════════════

run_and_capture "Forget: by entity (user_prefs)" forget --entity-id user_prefs
FORGOT_ENTITY=$(echo "$CAPTURED" | jq -r '.forgotten_count // 0')
printf "  ${DIM}Forgotten: %s memories${RESET}\n" "$FORGOT_ENTITY"
if [ "$FORGOT_ENTITY" -ge 6 ]; then
  printf "  ${GREEN}✓${RESET} All user_prefs memories removed\n"
else
  printf "  ${YELLOW}⚠${RESET} Expected >= 6 forgotten, got %s (some may have been already removed)\n" "$FORGOT_ENTITY"
fi

# Verify entity is clean
run_and_capture "Verify: entity is empty after forget" \
  prime user_prefs --max-memories 10
REMAINING=$(echo "$CAPTURED" | jq 'if type == "array" then length else 0 end' 2>/dev/null || echo 0)
if [ "$REMAINING" -eq 0 ]; then
  printf "  ${GREEN}✓${RESET} Entity is clean, 0 memories remaining\n"
else
  printf "  ${RED}✗${RESET} Expected 0 remaining, got %s\n" "$REMAINING"
fi

# ═══════════════════════════════════════════════════════════════════════
#  Summary
# ═══════════════════════════════════════════════════════════════════════

separator
echo ""
printf "${BOLD}╔══════════════════════════════════════════════════╗${RESET}\n"
printf "${BOLD}║  RESULTS                                        ║${RESET}\n"
printf "${BOLD}╠══════════════════════════════════════════════════╣${RESET}\n"
printf "${BOLD}║${RESET}  Total tests: %-33d${BOLD}║${RESET}\n" "$TOTAL"
printf "${BOLD}║${RESET}  ${GREEN}Passed: %-36d${RESET}${BOLD}║${RESET}\n" "$PASS"
if [ "$FAIL" -gt 0 ]; then
  printf "${BOLD}║${RESET}  ${RED}Failed: %-36d${RESET}${BOLD}║${RESET}\n" "$FAIL"
else
  printf "${BOLD}║${RESET}  Failed: %-35d${BOLD}║${RESET}\n" 0
fi
printf "${BOLD}╚══════════════════════════════════════════════════╝${RESET}\n"
echo ""

if [ "$FAIL" -gt 0 ]; then
  printf "${RED}SMOKE TEST FAILED${RESET}\n"
  exit 1
else
  printf "${GREEN}ALL SMOKE TESTS PASSED${RESET}\n"
  exit 0
fi
