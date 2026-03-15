#!/usr/bin/env bash
# MANUAL AGENT TEST: Recall Before Recommend
#
# Purpose: Verify the agent checks HEBBS before making recommendations,
#          especially to avoid suggesting things the user previously rejected.
#
# Setup:
#   cd /tmp/hebbs-openclaw-test
#   hebbs remember "User rejected Redux for state management, prefers Zustand" \
#     --importance 0.9 --entity-id user_prefs --global --format json
#   hebbs remember "Never suggest Prettier, user uses Biome for formatting" \
#     --importance 0.9 --entity-id user_prefs --global --format json
#
# Conversation:
#   1. Say: "What state management library should I use for this React project?"
#   2. Wait for response
#   3. Say: "What code formatter should I set up?"
#
# Expected agent behavior:
#   - Before answering Q1: agent runs hebbs recall about state management
#   - Agent recommends Zustand (or at least NOT Redux as first choice)
#   - Before answering Q2: agent runs hebbs recall about formatting
#   - Agent recommends Biome (or at least NOT Prettier)
#
# Verify:
#   hebbs queries --limit 10
#   # Should show recall operations for "state management" and "formatter/formatting"
#   # BEFORE the agent's recommendations
#
# Pass criteria:
#   1. Agent recalled before recommending (visible in query log)
#   2. Agent did NOT recommend Redux or Prettier as primary choices
#   3. Agent recommended Zustand and Biome (or acknowledged the stored preferences)

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
