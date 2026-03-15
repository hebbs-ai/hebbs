#!/usr/bin/env bash
# MANUAL AGENT TEST: Two-Brain Routing
#
# Purpose: Verify the agent correctly routes memories to global vs project brain.
#
# Setup:
#   cd /tmp/hebbs-openclaw-test
#   # Clean slate for this test
#   hebbs forget --entity-id test_routing 2>/dev/null || true
#   hebbs forget --entity-id test_routing --global 2>/dev/null || true
#
# Conversation:
#   1. Say: "I'm a senior engineer who's been writing Go for 10 years"
#   2. Say: "This project's CI runs on GitHub Actions with a 10-minute timeout"
#   3. Say: "I never want trailing commas in any language"
#   4. Say: "We use protobuf for all service-to-service communication in this project"
#   5. Say: "My timezone is PST"
#
# Expected routing:
#   Message 1: GLOBAL (personal identity)
#   Message 2: PROJECT (project-specific CI config)
#   Message 3: GLOBAL (cross-project coding preference)
#   Message 4: PROJECT (project architecture)
#   Message 5: GLOBAL (personal fact)
#
# Verify:
#   # Personal facts should be in global brain only
#   hebbs recall "Go experience" --strategy similarity --top-k 3 --global --format json
#   # Should find it
#
#   hebbs recall "Go experience" --strategy similarity --top-k 3 --format json
#   # Should NOT find it (project brain)
#
#   # Project facts should be in project brain only
#   hebbs recall "GitHub Actions CI" --strategy similarity --top-k 3 --format json
#   # Should find it
#
#   hebbs recall "GitHub Actions CI" --strategy similarity --top-k 3 --global --format json
#   # Should NOT find it (global brain)
#
# Pass criteria:
#   Personal/cross-project facts go to global brain (--global).
#   Project-specific facts go to project brain (no --global).
#   No cross-contamination.

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
