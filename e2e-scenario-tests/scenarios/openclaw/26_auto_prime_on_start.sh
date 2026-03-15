#!/usr/bin/env bash
# MANUAL AGENT TEST: Auto-Prime on Conversation Start
#
# Purpose: Verify the agent silently primes both brains at the start of every conversation.
#
# Setup:
#   cd /tmp/hebbs-openclaw-test
#   hebbs remember "User prefers vim keybindings" --importance 0.8 --entity-id user_prefs --global --format json
#   hebbs remember "This project uses Express.js" --importance 0.7 --entity-id project_context --format json
#
# Conversation:
#   1. Open a NEW agent conversation in the test directory
#   2. Say: "What do you know about me and this project?"
#
# Expected agent behavior:
#   - Agent runs hebbs prime (global + project) BEFORE responding
#   - Agent knows about vim keybindings (from global brain)
#   - Agent knows about Express.js (from project brain)
#   - Agent does NOT announce "I'm loading your context" or similar
#
# Verify:
#   hebbs queries --limit 5
#   # Should show prime operations from caller "mcp:*" or "cli" at conversation start
#
# Pass criteria:
#   Agent retrieves context from both brains without being asked and without
#   announcing it. The response reflects knowledge from both brains.

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
