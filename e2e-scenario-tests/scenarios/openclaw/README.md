# OpenClaw Agent Integration Tests

These tests verify that an AI agent (OpenClaw, Claude Code, Cursor) correctly follows the HEBBS SKILL.md instructions end-to-end.

Unlike vault/ and daemon/ tests which are automated shell scripts, these tests are **manual agent interaction tests**. Each scenario describes a conversation to have with the agent and what to verify.

## Prerequisites

1. HEBBS binary installed: `which hebbs`
2. HEBBS skill installed: `ls ~/.openclaw/skills/hebbs/SKILL.md`
3. Global brain initialized: `ls ~/.hebbs/`
4. A clean test project: `mkdir /tmp/hebbs-openclaw-test && cd /tmp/hebbs-openclaw-test`

## How to run

Open a new conversation with the agent in the test project directory. Follow each scenario in order. Each has a "Verify" section with commands to confirm the agent did the right thing.

## Scenarios

| # | Name | Tests |
|---|------|-------|
| 25 | Skill discovery | Agent finds and loads SKILL.md |
| 26 | Auto-prime on start | Agent primes both brains silently at conversation start |
| 27 | Proactive remember | Agent stores preferences/corrections without being asked |
| 28 | Recall before recommend | Agent recalls before making suggestions |
| 29 | Two-brain routing | Agent uses --global for personal, project brain for project facts |
| 30 | Cross-session persistence | New conversation, agent already knows prior context |
| 31 | Two-step reflect | Agent runs reflect-prepare + reflect-commit correctly |
| 32 | New project init | Agent offers to init HEBBS in uninitialized directory |
| 33 | Error recovery | Agent handles "vault not initialized" gracefully |
| 34 | Memory Palace awareness | Agent mentions panel when relevant |
