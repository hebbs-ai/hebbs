#!/usr/bin/env bash
# MANUAL AGENT TEST: New Project Init
#
# Purpose: Verify the agent offers to initialize HEBBS when working in
#          a directory without .hebbs/.
#
# Setup:
#   mkdir -p /tmp/hebbs-uninit-test
#   # Do NOT run hebbs init here
#
# Conversation:
#   1. Open agent conversation in /tmp/hebbs-uninit-test
#   2. Say: "Create a simple Express.js server in this project"
#
# Expected agent behavior:
#   - Agent detects no .hebbs/ directory (either by checking or by getting
#     "vault not initialized" error on first hebbs command)
#   - Agent offers to init: "Want me to index this project for HEBBS?"
#   - If user says yes, agent runs: hebbs init . && hebbs index .
#   - Agent stores project context (Express.js) in the new project brain
#
# Verify:
#   ls /tmp/hebbs-uninit-test/.hebbs/
#   # Should exist after agent inits
#
#   hebbs recall "Express" --strategy similarity --top-k 3 --format json
#   # Should return results from the new project brain
#
# Pass criteria:
#   Agent handles the missing vault gracefully and offers to set it up,
#   rather than silently failing or crashing.

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
