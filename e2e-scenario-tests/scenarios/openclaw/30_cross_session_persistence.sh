#!/usr/bin/env bash
# MANUAL AGENT TEST: Cross-Session Persistence
#
# Purpose: Verify memories persist across conversations. The agent should know
#          things from previous sessions without the user repeating them.
#
# Setup (Session 1):
#   cd /tmp/hebbs-openclaw-test
#   Open a new agent conversation.
#   Say: "My name is Alex. I'm building a payments service. I hate ORMs."
#   Wait for the agent to store these facts.
#   Close the conversation.
#
# Test (Session 2):
#   Open a BRAND NEW agent conversation in the same directory.
#   Say: "What do you know about me?"
#
# Expected agent behavior:
#   - Agent primes at conversation start (hebbs prime)
#   - Agent knows your name is Alex
#   - Agent knows you're building a payments service
#   - Agent knows you dislike ORMs
#   - Agent did NOT need to be told any of this again
#
# Verify:
#   hebbs queries --limit 5
#   # The new session should show prime operations at the start
#
# Pass criteria:
#   Agent recalls all three facts from the previous session without being
#   reminded. This is the core value proposition of HEBBS.

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
