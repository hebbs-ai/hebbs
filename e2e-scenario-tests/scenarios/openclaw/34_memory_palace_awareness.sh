#!/usr/bin/env bash
# MANUAL AGENT TEST: Memory Palace Awareness
#
# Purpose: Verify the agent knows about the Memory Palace and mentions
#          it when relevant.
#
# Setup:
#   cd /tmp/hebbs-openclaw-test
#   hebbs init . 2>/dev/null || true
#   # Store some memories so there's something to visualize
#   hebbs remember "Auth uses JWT tokens" --importance 0.7 --format json
#   hebbs remember "Migrated to session cookies" --importance 0.7 --format json
#
# Conversation:
#   1. Say: "I want to see all the knowledge you have about this project"
#   2. Say: "Are there any contradictions in what you know?"
#   3. Say: "How can I see what queries agents have been making?"
#
# Expected agent behavior:
#   - Q1: Agent mentions `hebbs panel` as a way to visualize all knowledge
#   - Q2: Agent mentions contradictions show as red dashed edges in the panel
#   - Q3: Agent mentions the Queries tab in the Memory Palace
#
# Pass criteria:
#   Agent is aware of the Memory Palace as a user-facing tool and recommends
#   it for visualization, contradiction review, and query audit log viewing.

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
