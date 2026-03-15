#!/usr/bin/env bash
# MANUAL AGENT TEST: Two-Step Reflect
#
# Purpose: Verify the agent can run the full reflect pipeline:
#          reflect-prepare (get clusters) then reflect-commit (store insights).
#
# Setup:
#   cd /tmp/hebbs-openclaw-test
#   # Seed enough memories for reflection (need 5+ on one entity)
#   for i in $(seq 1 8); do
#     hebbs remember "Architecture fact $i: component $i uses pattern $i for reliability" \
#       --importance 0.6 --entity-id project_arch --format json
#   done
#
# Conversation:
#   1. Say: "Can you reflect on the project_arch entity and consolidate insights?"
#
# Expected agent behavior:
#   - Agent runs: hebbs reflect-prepare --entity-id project_arch --format json
#   - Agent reads the returned clusters and their memory contents
#   - Agent reasons about patterns across the cluster
#   - Agent runs: hebbs reflect-commit --session-id <ID> --insights '[...]'
#   - Agent uses hex-encoded IDs from cluster memory_ids (NOT ULID memory_id)
#   - Agent reports what insight was created
#
# Verify:
#   hebbs insights --entity-id project_arch --format json
#   # Should return at least one insight with content, confidence, source lineage
#
#   hebbs queries --limit 5
#   # Should show the reflect-related recall operations
#
# Common failures:
#   - Agent uses ULID memory_id instead of hex memory_ids -> reflect-commit fails
#   - Agent skips reflect-prepare and tries to create insights directly
#   - Agent announces "I'm going to reflect now" instead of just doing it
#
# Pass criteria:
#   1. Agent ran both reflect-prepare and reflect-commit
#   2. Used correct hex IDs from clusters
#   3. At least one insight was created and is queryable

echo "MANUAL TEST: Run this as an agent conversation, not as a script."
echo "See comments in this file for instructions."
exit 0
