#!/usr/bin/env bash
# MANUAL AGENT TEST: Error Recovery
#
# Purpose: Verify the agent handles HEBBS errors gracefully without
#          breaking the conversation.
#
# Setup:
#   cd /tmp/hebbs-openclaw-test
#   # Ensure vault exists
#   hebbs init . 2>/dev/null || true
#
# Conversation:
#   1. Say: "What was the decision about database migration?"
#      (Agent should recall, get no results, and say it doesn't know)
#
#   2. Say: "Reflect on the entity nonexistent_entity"
#      (reflect-prepare will fail with not enough memories)
#
#   3. Say: "Forget the memory with ID 00000000000000000000000000"
#      (forget with invalid ID should fail gracefully)
#
# Expected agent behavior:
#   - Error 1: Agent recalls, gets empty results, tells user honestly
#     "I don't have any stored context about database migration"
#   - Error 2: Agent runs reflect-prepare, gets error (not enough memories),
#     tells user "Not enough memories to reflect on that entity yet"
#   - Error 3: Agent handles the error, does NOT crash or loop
#   - In ALL cases: conversation continues normally after the error
#
# Pass criteria:
#   1. Agent does not hallucinate answers when recall returns empty
#   2. Agent reports errors clearly without panicking
#   3. Conversation continues normally after each error

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
