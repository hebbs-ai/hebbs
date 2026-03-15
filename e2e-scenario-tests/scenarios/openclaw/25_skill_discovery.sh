#!/usr/bin/env bash
# MANUAL AGENT TEST: Skill Discovery
#
# Purpose: Verify the agent finds HEBBS SKILL.md and treats HEBBS as its memory system.
#
# Setup:
#   mkdir -p /tmp/hebbs-openclaw-test && cd /tmp/hebbs-openclaw-test
#   hebbs init . && hebbs init ~
#
# Conversation:
#   1. Open a new agent conversation in the test directory
#   2. Say: "What memory tools do you have available?"
#
# Expected agent behavior:
#   - Agent mentions HEBBS as its memory system
#   - Agent does NOT mention built-in memory, MEMORY.md, or memory_search as primary
#   - Agent may mention hebbs commands: remember, recall, prime, insights
#
# Verify:
#   - The agent's response references HEBBS, not a built-in memory tool
#   - If the agent ran any commands, they should be hebbs commands
#
# Pass criteria:
#   Agent identifies HEBBS as its primary memory system and describes
#   its capabilities (remember, recall, reflect, prime).

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
