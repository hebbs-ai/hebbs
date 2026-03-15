# Plan: Policy Bootstrap for HEBBS Skill

## Problem

When a new user installs the HEBBS skill, the agent starts storing memories without understanding the user's preferences about what should and shouldn't be stored. This creates a trust gap — the user doesn't know what's being stored, and the agent doesn't know the user's boundaries.

The OpenClaw testing session revealed that even a well-behaved agent will skip HEBBS writes when it "already knows" a fact from another source. A stored policy would make the expected behavior explicit for both the agent and the user.

## Design Principles

1. **Good defaults** — the skill works immediately without policy setup
2. **One-time prompt** — ask only once, only if no policy exists
3. **No forced wizard** — if the user declines, proceed with defaults
4. **Policy stored in HEBBS** — dogfooding: first real use of the system stores its own operating rules
5. **Policy check is just a recall** — no separate mechanism, just `hebbs-cli recall` on a known entity

## Where It Lives

New section in SKILL.md called "Policy bootstrap", placed between "First-run setup" (Phase 4) and "Before every command".

## Flow

```
Session start
  → Phase 1-4 (first-run setup, if needed)
  → Prime user entity
  → Recall policy: hebbs-cli recall "memory policy" --entity-id _policy --top-k 3 --format json
  → If results found: load policy, proceed
  → If no results: run policy bootstrap (once)
  → Normal operation
```

## Policy Bootstrap

### Trigger Condition

All of the following must be true:
- HEBBS server is healthy (status = SERVING)
- `hebbs-cli recall "memory policy" --entity-id _policy --top-k 1 --format json` returns empty
- This is not a one-off test or smoke test (user has asked something substantive)

### What the Agent Says

Something like:

> HEBBS is ready. Before I start using it as your memory system, I'd like to understand your preferences. This takes about 30 seconds and I'll only ask once.
>
> 1. **What should I store?** (e.g., preferences, decisions, project context, corrections, everything)
> 2. **What should I NOT store?** (e.g., personal info, credentials, temporary thoughts, nothing off-limits)
> 3. **Should I store proactively** when you mention something, or **only when you explicitly ask** me to remember?
> 4. **Any privacy boundaries?** (e.g., no names of other people, no financial info)
> 5. **Should I mirror important memories to file memory** as a backup, or keep everything in HEBBS only?
>
> If you'd rather skip this, I'll use sensible defaults: store preferences and decisions proactively, skip sensitive personal info, HEBBS-only storage.

### What the Agent Stores

One memory per policy answer, all under entity `_policy`:

```bash
hebbs-cli remember "Store policy: user wants [their answer about what to store]" \
  --importance 0.95 --entity-id _policy --format json

hebbs-cli remember "Privacy policy: user wants [their answer about boundaries]" \
  --importance 0.95 --entity-id _policy --format json

hebbs-cli remember "Storage mode: [proactive|explicit-only]" \
  --importance 0.95 --entity-id _policy --format json

hebbs-cli remember "Mirror policy: [hebbs-only|mirror-to-files]" \
  --importance 0.95 --entity-id _policy --format json
```

If the user skips setup, store the defaults:

```bash
hebbs-cli remember "Memory policy: defaults active — store preferences and decisions proactively, skip sensitive personal info, HEBBS-only storage, no file mirroring" \
  --importance 0.95 --entity-id _policy --format json
```

### How Future Sessions Use It

At session start, after prime:

```bash
hebbs-cli recall "memory policy" --entity-id _policy --top-k 5 --format json
```

The agent reads the policy memories and applies them for the entire session. No re-asking.

## Default Policy (When User Skips)

| Setting | Default |
|---|---|
| What to store | Preferences, decisions, corrections, project context |
| What not to store | Credentials, API keys, sensitive personal info |
| Storage mode | Proactive (store when user mentions something worth retaining) |
| Privacy | No credentials or secrets |
| File mirroring | HEBBS-only, no file mirror |

## Changes Required

### 1. SKILL.md — new "Policy bootstrap" section

Add after Phase 4 (verify), before "Before every command". Contains:
- Trigger condition
- The 5 questions
- Default policy
- Storage format (entity `_policy`, importance 0.95)

### 2. SKILL.md — update "Decision guide" step 1

Currently: "Always `hebbs-cli prime <entity>` to load context."

Change to: "Always prime, then check for policy. If no policy found and this is not a trivial interaction, run policy bootstrap."

### 3. SKILL.md — update "Before every command"

Add: "If this is the first substantive interaction of a session, check `_policy` entity before proceeding."

## What This Does NOT Change

- No server-side changes
- No CLI changes
- No Homebrew formula changes
- No new binaries or configs
- Pure skill-layer behavior change

## Risks

- **Over-prompting**: If the policy check fails (e.g., HEBBS was wiped), the agent will re-ask. Mitigation: the trigger condition requires a substantive interaction, so smoke tests won't trigger it.
- **Policy drift**: User preferences change but old policy stays. Mitigation: user can say "update my memory policy" and the agent overwrites. Add a note in the skill about this.
- **Entity collision**: `_policy` is a reserved-feeling name. Should be fine since users pick their own entity names and underscore-prefix signals "system use".
