#!/usr/bin/env bash
set -euo pipefail

# Scenario 6: Insight Loop
# Tests: reflect -> insight file -> re-index -> no infinite loop.
# Maps to TASK-13 Milestone 6 (insight output as files) + loop prevention.
#
# NOTE: This scenario requires a running LLM provider for reflect().
# If unavailable, it tests the insight file format and indexing only.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="06_insight_loop"
echo "=== Scenario 6: Insight Loop ==="

# --- Setup ---
VAULT_DIR=$(create_temp_vault)
trap "cleanup_vault '$VAULT_DIR'" EXIT

# Create vault with related content
mkdir -p "$VAULT_DIR/notes" "$VAULT_DIR/insights"

# Create 5 semantically related notes
for i in $(seq 1 5); do
    cat > "$VAULT_DIR/notes/project-update-$i.md" << ENDFILE
---
title: Project Update $i
date: 2026-0$i-15
---

## Progress

Sprint $i completed with $((i * 2)) story points delivered.
Team velocity has been $( [ $i -gt 3 ] && echo "declining" || echo "increasing" ) over the past $i weeks.

## Risks

The main risk for sprint $i is dependency on the vendor delivery timeline.
$( [ $i -gt 3 ] && echo "Vendor has missed the last two milestones." || echo "Vendor is currently on track." )
ENDFILE
done

vault_init "$VAULT_DIR"
vault_index "$VAULT_DIR"

INITIAL_SECTIONS=$(get_section_count "$VAULT_DIR")
echo "Initial: $INITIAL_SECTIONS sections"

# --- Simulate insight generation ---
# Since reflect() requires an LLM, we simulate by creating insight files
# in the same format that InsightWriter would produce.

echo "Simulating insight generation..."
cat > "$VAULT_DIR/insights/01ABCDEF-velocity-trend-detected.md" << 'EOF'
---
hebbs-kind: insight
hebbs-sources:
  - notes/project-update-1.md#progress
  - notes/project-update-3.md#progress
  - notes/project-update-5.md#progress
hebbs-confidence: 0.78
hebbs-created: 2026-03-14T10:00:00Z
---

Team velocity shows a clear declining trend starting from sprint 4.
The initial velocity gains from sprints 1-3 have not been sustained.
EOF

cat > "$VAULT_DIR/insights/01ABCDEG-vendor-risk-escalation.md" << 'EOF'
---
hebbs-kind: insight
hebbs-sources:
  - notes/project-update-4.md#risks
  - notes/project-update-5.md#risks
hebbs-confidence: 0.85
hebbs-created: 2026-03-14T10:01:00Z
---

Vendor reliability has degraded. Initial assessments were positive but
recent sprints show missed milestones, indicating systemic delivery issues.
EOF

# Re-index to pick up insight files
vault_index "$VAULT_DIR"

POST_INSIGHT_SECTIONS=$(get_section_count "$VAULT_DIR")
echo "After insights: $POST_INSIGHT_SECTIONS sections"

# --- Qualitative checks ---
assert_manifest_has_file "insight_1_indexed" "$VAULT_DIR" "insights/01ABCDEF-velocity-trend-detected.md"
assert_manifest_has_file "insight_2_indexed" "$VAULT_DIR" "insights/01ABCDEG-vendor-risk-escalation.md"
assert_all_sections_synced "insights_synced" "$VAULT_DIR"

# Check insight file structure
assert_contains "insight_has_kind" "$VAULT_DIR/insights/01ABCDEF-velocity-trend-detected.md" "hebbs-kind: insight"
assert_contains "insight_has_sources" "$VAULT_DIR/insights/01ABCDEF-velocity-trend-detected.md" "hebbs-sources:"
assert_contains "insight_has_confidence" "$VAULT_DIR/insights/01ABCDEF-velocity-trend-detected.md" "hebbs-confidence:"

# Simulate another round of insight generation (should not create duplicates)
# In a real system, the reflect pipeline would reject near-duplicate insights.
# Here we just verify that re-indexing the same insights doesn't create extra sections.
vault_index "$VAULT_DIR"
FINAL_SECTIONS=$(get_section_count "$VAULT_DIR")

assert_eq "no_duplicate_sections" "$FINAL_SECTIONS" "$POST_INSIGHT_SECTIONS"

# --- Quantitative checks ---
INSIGHT_SECTIONS=$((POST_INSIGHT_SECTIONS - INITIAL_SECTIONS))

log_metric "initial_sections" "$INITIAL_SECTIONS" "count"
log_metric "insight_sections_added" "$INSIGHT_SECTIONS" "count"
log_metric "final_sections" "$FINAL_SECTIONS" "count"
log_metric "insight_files" "2" "count"

# --- Save results ---
save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
