#!/usr/bin/env bash
# MANUAL AGENT TEST: Proactive Remember
#
# Purpose: Verify the agent stores preferences, corrections, and decisions
#          without being explicitly asked to remember them.
#
# Setup:
#   cd /tmp/hebbs-openclaw-test
#
# Conversation:
#   1. Say: "I always want 2-space indentation, never tabs"
#   2. Wait for response
#   3. Say: "No wait, actually 4 spaces. And always use single quotes in JS."
#   4. Wait for response
#   5. Say: "Our API uses GraphQL, not REST"
#
# Expected agent behavior:
#   - After message 1: agent runs hebbs remember with --global (personal preference)
#   - After message 3: agent stores the CORRECTION (4 spaces, not 2) with high importance (0.9)
#     AND the single quotes preference
#   - After message 5: agent stores in project brain (no --global) since it's project-specific
#   - Agent does NOT announce every store. May confirm the correction ("Got it, 4 spaces").
#
# Verify:
#   hebbs recall "indentation" --strategy similarity --top-k 3 --global --format json
#   # Should show "4 spaces" preference, not "2 spaces" (or both, with correction noted)
#
#   hebbs recall "quotes" --strategy similarity --top-k 3 --global --format json
#   # Should show single quotes preference
#
#   hebbs recall "API protocol" --strategy similarity --top-k 3 --format json
#   # Should show GraphQL (in project brain, not global)
#
#   hebbs recall "API protocol" --strategy similarity --top-k 3 --global --format json
#   # Should NOT find GraphQL in global brain
#
# Pass criteria:
#   1. All three facts were stored without being asked
#   2. Correction was stored with high importance
#   3. Personal prefs went to global brain, project fact to project brain
#   4. Agent did not announce each store verbosely

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
