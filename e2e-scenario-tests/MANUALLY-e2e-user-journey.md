# Manual E2E: Complete User Journey

A human tester follows this document end-to-end. Start from scratch, finish with a fully working multi-brain daemon setup. Every step has an expected result. If any step fails, stop and file a bug.

Estimated time: 30-40 minutes.

Prerequisites: macOS or Linux, Homebrew (macOS), a terminal, a text editor, and an AI coding agent (OpenClaw, Claude Code, or Cursor).

---

## Phase 1: Discovery and Installation

### 1.1 Visit the website

Open https://hebbs.ai in a browser.

**Expected:** Landing page loads. You see "The memory engine for AI agents" (or similar). The page explains recall strategies, scoring, and shows a code example. There is a clear install command.

### 1.2 Install the binary

```bash
brew install hebbs-ai/tap/hebbs
```

Or if not on macOS:

```bash
curl -sSf https://hebbs.ai/install | sh
```

**Expected:** Installation completes without errors. The binary is available on your PATH.

### 1.3 Verify installation

```bash
hebbs version
```

**Expected:** Prints version and architecture, e.g. `hebbs 0.2.0 (aarch64)`.

```bash
which hebbs
```

**Expected:** Prints the binary path (e.g. `/opt/homebrew/bin/hebbs` or `~/.hebbs/bin/hebbs`).

---

## Phase 2: First Brain

### 2.1 Initialize a project brain

Create a test project and initialize HEBBS:

```bash
mkdir -p ~/hebbs-test-project
cd ~/hebbs-test-project
hebbs init .
```

**Expected:**
- Prints `Initialized vault at .` (or similar).
- A `.hebbs/` directory exists with `config.toml`, `manifest.json`, and an `index/` folder.
- If this is a git repo, `.hebbs/` is added to `.gitignore`.

### 2.2 Verify the pipeline

Store a test memory, recall it, then clean up:

```bash
hebbs remember "HEBBS setup verified" --importance 0.1 --entity-id _system --format json
```

**Expected:** JSON output with `memory_id`, `content`, `importance`, `entity_id` fields. The `memory_id` is a 26-character ULID string.

```bash
hebbs recall "setup verified" --top-k 1 --format json
```

**Expected:** JSON array with at least one result. The result contains `content: "HEBBS setup verified"` and a `score` field.

```bash
hebbs forget --entity-id _system
```

**Expected:** Prints `Forgotten: 1 memories (0 cascade)` (or similar count).

```bash
hebbs recall "setup verified" --top-k 1 --format json
```

**Expected:** Empty JSON array `[]` or no results. The test memory is gone.

### 2.3 Check status

```bash
hebbs status
```

**Expected:** Shows vault path, `0 indexed` files, `0 total` sections. The brain is empty and healthy.

---

## Phase 3: Core Memory Operations

### 3.1 Remember with importance and entity scoping

```bash
hebbs remember "This project uses Rust with the Axum web framework" \
  --importance 0.8 --entity-id project_context --format json
```

Save the `memory_id` from the output. Call it `MEM_A`.

```bash
hebbs remember "All API endpoints must return JSON with proper status codes" \
  --importance 0.9 --entity-id project_context --format json
```

Save this `memory_id` as `MEM_B`.

```bash
hebbs remember "Database layer uses SQLx with PostgreSQL" \
  --importance 0.7 --entity-id project_context --format json
```

```bash
hebbs remember "Sprint velocity has been declining since sprint 4" \
  --importance 0.6 --entity-id project_status --format json
```

```bash
hebbs remember "Alice owns the auth module, Bob owns payments" \
  --importance 0.5 --entity-id team_context --format json
```

**Expected:** All five return valid JSON with `memory_id`. Five different entities are scoped: `project_context` (3), `project_status` (1), `team_context` (1).

### 3.2 Remember with context metadata

```bash
hebbs remember "User corrected: use match expressions, never unwrap()" \
  --importance 0.9 --entity-id project_context \
  --context '{"source":"code_review","topic":"error_handling"}' \
  --format json
```

**Expected:** JSON output. Context is preserved in the response as a nested JSON object.

### 3.3 Remember with graph edges

```bash
hebbs remember "Switched from MySQL to PostgreSQL due to JSON column support" \
  --importance 0.8 --entity-id project_context \
  --edge "${MEM_A}:related_to" \
  --format json
```

(Replace `${MEM_A}` with the actual ULID from step 3.1.)

**Expected:** JSON output. The memory is linked to MEM_A via a `related_to` edge.

### 3.4 Recall: similarity strategy

```bash
hebbs recall "What web framework does this project use?" \
  --strategy similarity --top-k 5 --format json
```

**Expected:** Results include the Axum memory. Score is > 0. Results are sorted by descending score.

### 3.5 Recall: temporal strategy

```bash
hebbs recall "recent activity" \
  --strategy temporal --entity-id project_context --top-k 5 --format json
```

**Expected:** Returns project_context memories in reverse chronological order (newest first). Only project_context entity memories appear.

### 3.6 Recall with custom scoring weights

```bash
hebbs recall "database" --strategy similarity --top-k 5 \
  --weights "0.3:0.1:0.5:0.1" --format json
```

**Expected:** Results returned. The high-importance memories (0.9, 0.8) are ranked higher than they would be with default weights, because importance weight is 0.5 instead of 0.2.

### 3.7 Get a specific memory

```bash
hebbs get <MEM_B>
```

(Replace `<MEM_B>` with the ULID from step 3.1.)

**Expected:** Prints full memory detail: ID, content ("All API endpoints..."), importance (0.90), access count, timestamps.

### 3.8 Inspect a memory (detail + graph)

```bash
hebbs inspect <MEM_A>
```

**Expected:** Shows memory detail plus a "Graph Neighbors" section. If the edge from step 3.3 was created, at least one outgoing edge appears.

### 3.9 Prime an entity

```bash
hebbs prime project_context --max-memories 10 --format json
```

**Expected:** JSON array of memories. All belong to `project_context`. Mix of recent and relevant. Count shows temporal and similarity contributions.

### 3.10 Forget by ID

```bash
hebbs remember "Temporary test memory" --importance 0.1 --entity-id _tmp --format json
```

Save the memory_id, then:

```bash
hebbs forget --ids <TEMP_ID>
```

```bash
hebbs get <TEMP_ID>
```

**Expected:** The get command fails with an error (memory not found).

### 3.11 Forget by entity

```bash
hebbs remember "Test cleanup 1" --importance 0.1 --entity-id _cleanup --format json
hebbs remember "Test cleanup 2" --importance 0.1 --entity-id _cleanup --format json
hebbs forget --entity-id _cleanup
hebbs prime _cleanup --max-memories 10 --format json
```

**Expected:** Prime returns an empty array. Both memories are gone.

### 3.12 Export

```bash
hebbs export --limit 100
```

**Expected:** JSONL output (one JSON object per line) to stdout. Each line has `memory_id`, `content`, `importance`. stderr shows the count.

---

## Phase 4: Vault Indexing (File-Backed Memories)

### 4.1 Create markdown files

```bash
cd ~/hebbs-test-project

cat > architecture.md << 'EOF'
# Architecture

## Backend

The backend uses Rust with Axum. All routes are defined in `src/routes/`.

## Frontend

The frontend uses React 19 with TypeScript. Components live in `src/components/`.

## Database

PostgreSQL via SQLx. Migrations are in `migrations/`.
EOF

cat > meeting-notes.md << 'EOF'
# Q1 Planning Meeting

## Priorities

1. Ship v2.0 API by end of March
2. Migrate auth to OAuth2
3. Reduce p99 latency below 50ms

## Action Items

- Alice: draft OAuth2 migration plan by Friday
- Bob: set up load testing for payments endpoint
EOF

cat > conventions.md << 'EOF'
# Coding Conventions

## Error Handling

Use `Result<T, AppError>` everywhere. Never use `unwrap()` in production code.
Use `anyhow` in binaries, `thiserror` in libraries.

## Naming

- Files: snake_case
- Types: PascalCase
- Functions: snake_case
- Constants: SCREAMING_SNAKE_CASE
EOF
```

### 4.2 Index the vault

```bash
hebbs index .
```

**Expected:** Output shows phase 1 (parsing files, counting sections) and phase 2 (embedding sections). Should report 3 files and multiple sections (one per heading).

### 4.3 Verify indexed content

```bash
hebbs list --sections
```

**Expected:** Lists all three files with their sections. Each section shows `[Synced]` state, heading path, memory ID prefix, and byte range.

### 4.4 Recall from indexed files

```bash
hebbs recall "How should errors be handled?" --strategy similarity --top-k 3 --format json
```

**Expected:** Returns the "Error Handling" section from `conventions.md`. Content includes "Result<T, AppError>" and "Never use unwrap()".

```bash
hebbs recall "What are the Q1 priorities?" --strategy similarity --top-k 3 --format json
```

**Expected:** Returns the "Priorities" section from `meeting-notes.md`.

### 4.5 Mixed recall (file-backed + agent-stored)

File-indexed memories and agent-stored memories coexist in the same brain:

```bash
hebbs recall "What framework does this project use?" --strategy similarity --top-k 5 --format json
```

**Expected:** Returns results from both sources: the "Backend" section from `architecture.md` AND the agent-stored "This project uses Rust with Axum" memory from Phase 3.

### 4.6 Edit a file and re-index

```bash
echo -e "\n## Caching\n\nRedis is used for session caching. TTL is 1 hour." >> architecture.md
hebbs index .
```

**Expected:** Phase 1 reports 1 modified file, 1 new section. Phase 2 embeds the new "Caching" section.

```bash
hebbs recall "What caching strategy does the project use?" --strategy similarity --top-k 3 --format json
```

**Expected:** Returns the new "Caching" section with Redis content.

### 4.7 Status after indexing

```bash
hebbs status
```

**Expected:** Shows `3 indexed` files, multiple sections, all synced.

---

## Phase 5: Two-Brain Architecture

### 5.1 Initialize the global brain

```bash
hebbs init ~
```

**Expected:** Creates `~/.hebbs/` directory. Prints success message.

### 5.2 Store global preferences

```bash
hebbs remember "I prefer dark mode in all editors and terminals" \
  --importance 0.8 --entity-id user_prefs --global --format json

hebbs remember "Never use em-dashes in any writing" \
  --importance 0.9 --entity-id user_prefs --global --format json

hebbs remember "I am a senior Rust engineer with 5 years experience" \
  --importance 0.7 --entity-id user_prefs --global --format json

hebbs remember "Keep responses terse, no trailing summaries" \
  --importance 0.9 --entity-id user_prefs --global --format json
```

**Expected:** All four stored in the global brain (`~/.hebbs/`).

### 5.3 Verify brain isolation

From the project directory:

```bash
cd ~/hebbs-test-project

hebbs recall "dark mode" --strategy similarity --top-k 3 --format json
```

**Expected:** Returns nothing (or only loosely related results). The global memory is NOT in the project brain.

```bash
hebbs recall "dark mode" --strategy similarity --top-k 3 --global --format json
```

**Expected:** Returns the dark mode preference memory. The `--global` flag routes to `~/.hebbs/`.

```bash
hebbs recall "Axum framework" --strategy similarity --top-k 3 --global --format json
```

**Expected:** Returns nothing. The project-specific memory is NOT in the global brain.

### 5.4 Prime both brains (session start pattern)

This is what an agent does at the start of every conversation:

```bash
hebbs prime user_prefs --max-memories 10 --global --format json
hebbs prime project_context --max-memories 10 --format json
```

**Expected:** First command returns global user preferences (dark mode, writing style, etc.). Second command returns project context (Axum, PostgreSQL, etc.). No cross-contamination.

### 5.5 Second project brain

```bash
mkdir -p ~/hebbs-test-project-2
cd ~/hebbs-test-project-2
hebbs init .

hebbs remember "This project uses Python with FastAPI" \
  --importance 0.8 --entity-id project_context --format json

hebbs remember "Deploy to AWS Lambda via SAM" \
  --importance 0.7 --entity-id project_context --format json
```

```bash
hebbs recall "What framework?" --strategy similarity --top-k 3 --format json
```

**Expected:** Returns FastAPI memories. No Axum, no React.

```bash
cd ~/hebbs-test-project
hebbs recall "What framework?" --strategy similarity --top-k 3 --format json
```

**Expected:** Returns Axum/React memories. No FastAPI.

**The key insight:** Same command, different directory, different brain, different results.

---

## Phase 6: Reflection Pipeline

### 6.1 Accumulate enough memories

You need at least 5 memories for an entity to trigger reflection. Add more to `project_context` in the first project:

```bash
cd ~/hebbs-test-project

hebbs remember "API versioning uses URL path: /v1/, /v2/" \
  --importance 0.6 --entity-id project_context --format json

hebbs remember "Rate limiting is 100 req/s per API key" \
  --importance 0.7 --entity-id project_context --format json

hebbs remember "Logging uses structured JSON via tracing crate" \
  --importance 0.6 --entity-id project_context --format json
```

### 6.2 Reflect-prepare

```bash
hebbs reflect-prepare --entity-id project_context --format json
```

**Expected:** JSON output with:
- `session_id`: a string identifier (save this)
- `memories_processed`: count of memories analyzed
- `clusters`: array of cluster objects, each with `cluster_id`, `member_count`, `system_prompt`, `user_prompt`, `memory_ids`

If clusters is empty, you need more memories. Add a few more and retry.

### 6.3 Read the clusters and reason

Look at the cluster contents. The `proposal_user_prompt` contains the memories grouped by theme. Read them and write an insight.

For example, if a cluster contains memories about Rust, Axum, PostgreSQL, SQLx, you might write:

> "This is a Rust-based web service using Axum for HTTP routing and PostgreSQL/SQLx for data persistence. The codebase follows strict error handling (no unwrap, Result everywhere) and uses structured JSON logging."

### 6.4 Reflect-commit

```bash
hebbs reflect-commit \
  --session-id <SESSION_ID> \
  --insights '[{
    "content": "This is a Rust-based web service using Axum for HTTP routing and PostgreSQL/SQLx for data persistence, with strict error handling and structured JSON logging.",
    "confidence": 0.85,
    "source_memory_ids": ["<HEX_ID_1>", "<HEX_ID_2>", "<HEX_ID_3>"],
    "tags": ["architecture", "stack"],
    "cluster_id": 0
  }]'
```

Replace `<SESSION_ID>` with the session ID from step 6.2. Replace `<HEX_ID_*>` with hex-encoded memory IDs from the cluster's `memory_ids` array.

**Expected:** Prints `Committed: 1 insights created`.

### 6.5 Query insights

```bash
hebbs insights --entity-id project_context --format json
```

**Expected:** Returns the insight you just committed. It has `content`, `importance`, and the source lineage.

```bash
hebbs insights --entity-id project_context --min-confidence 0.9 --format json
```

**Expected:** Empty (your insight was 0.85 confidence, below the 0.9 threshold).

---

## Phase 7: Agent Integration

### 7.1 Install the HEBBS skill

For OpenClaw:

```bash
mkdir -p ~/.openclaw/skills/hebbs
cp "$(brew --prefix)/share/hebbs/SKILL.md" ~/.openclaw/skills/hebbs/SKILL.md
# Or download: curl -o ~/.openclaw/skills/hebbs/SKILL.md https://raw.githubusercontent.com/hebbs-ai/hebbs/main/skills/hebbs/SKILL.md
```

For Claude Code:

```bash
# The SKILL.md goes into your project's .claude/ or the global ~/.claude/ config
# Follow Claude Code's skill installation docs
```

**Expected:** The agent framework recognizes HEBBS as an available skill.

### 7.2 Policy bootstrap (first conversation)

Start a new conversation with the agent. The agent should:

1. Check for existing policy: `hebbs recall "memory policy" --entity-id _policy --top-k 1 --global --format json`
2. If no results, ask you the four policy questions
3. Store your answers under `_policy` entity in the global brain

**Expected:** The agent asks about storage preferences, exclusions, proactive vs explicit mode, and privacy boundaries. After you answer (or skip), a policy memory exists in the global brain.

Verify:

```bash
hebbs prime _policy --max-memories 5 --global --format json
```

**Expected:** Returns the stored policy.

### 7.3 Agent-driven conversation

Have a real conversation with the agent. Tell it some facts:

- "I prefer tabs over spaces, 4-width"
- "Our deployment uses GitHub Actions for CI and ArgoCD for CD"
- "The staging environment is on AWS us-east-1"

**Expected:** The agent runs `hebbs remember` for each fact. Global preferences go to `--global`, project facts go to the project brain.

### 7.4 Verify agent stored memories

```bash
hebbs recall "tabs or spaces" --strategy similarity --top-k 3 --global --format json
```

**Expected:** Returns the tabs preference, stored by the agent.

```bash
hebbs recall "deployment pipeline" --strategy similarity --top-k 3 --format json
```

**Expected:** Returns the CI/CD and staging memories.

### 7.5 Cross-session persistence

Close the conversation. Open a new one. The agent should:

1. Prime both brains at session start
2. Already know your preferences without you repeating them

Ask the agent: "What are my coding preferences?"

**Expected:** The agent recalls your stored preferences (dark mode, tabs, no em-dashes, terse responses) without you having to re-state them.

---

## Phase 8: Daemon Mode

### 8.1 Start the daemon

```bash
hebbs serve --foreground --idle-timeout 300
```

**Expected:** Daemon starts, prints "loading embedding model...", "daemon listening on ~/.hebbs/daemon.sock". The terminal is blocked (foreground mode).

Open a second terminal for the remaining steps.

### 8.2 Verify daemon files

```bash
ls -la ~/.hebbs/daemon.sock ~/.hebbs/daemon.pid
```

**Expected:** Both files exist. The socket is a Unix domain socket. The PID file contains a numeric PID.

```bash
cat ~/.hebbs/daemon.pid
```

**Expected:** A PID number. Verify it matches:

```bash
ps -p $(cat ~/.hebbs/daemon.pid)
```

**Expected:** Shows the hebbs process.

### 8.3 Commands through the daemon

All commands should now route through the daemon automatically:

```bash
cd ~/hebbs-test-project

hebbs recall "What framework?" --strategy similarity --top-k 3 --format json
```

**Expected:** Returns Axum memories. Response is noticeably faster than cold-start mode (the first time may still be quick; subsequent calls are near-instant because the engine is warm).

```bash
hebbs remember "Daemon test: this memory went through the daemon" \
  --importance 0.5 --entity-id daemon_test --format json
```

**Expected:** Returns valid JSON with memory_id.

```bash
hebbs recall "daemon test" --strategy similarity --top-k 3 --format json
```

**Expected:** Returns the memory just stored.

### 8.4 Multi-vault through daemon

The daemon serves all vaults on the machine:

```bash
cd ~/hebbs-test-project
hebbs recall "Axum" --strategy similarity --top-k 3 --format json
```

```bash
cd ~/hebbs-test-project-2
hebbs recall "FastAPI" --strategy similarity --top-k 3 --format json
```

```bash
hebbs recall "dark mode" --strategy similarity --top-k 3 --global --format json
```

**Expected:** Each recall targets the correct brain. No cross-contamination. The daemon opened vault handles on demand.

### 8.5 Latency comparison

Time a recall with and without the daemon. Stop the daemon first (Ctrl-C in the first terminal), then:

```bash
cd ~/hebbs-test-project

# Cold start (no daemon)
time hebbs recall "framework" --strategy similarity --top-k 3 --format json 2>/dev/null

# Start daemon again
hebbs serve --foreground --idle-timeout 300 &
sleep 2

# Warm (through daemon)
time hebbs recall "framework" --strategy similarity --top-k 3 --format json 2>/dev/null
```

**Expected:** The daemon call is significantly faster (typically 5-50ms vs 200-600ms cold start). The exact speedup depends on your machine.

### 8.6 Daemon idle shutdown

Start the daemon with a short idle timeout:

```bash
# Stop existing daemon
kill $(cat ~/.hebbs/daemon.pid) 2>/dev/null; sleep 1

hebbs serve --foreground --idle-timeout 10 &
sleep 2
```

Run one command to prove it's alive:

```bash
hebbs recall "test" --format json 2>/dev/null
```

Now wait 15 seconds without running any commands.

**Expected:** The daemon prints "idle shutdown" and exits. The socket and PID files are removed:

```bash
ls ~/.hebbs/daemon.sock 2>&1
ls ~/.hebbs/daemon.pid 2>&1
```

**Expected:** Both files are gone (file not found).

### 8.7 Auto-restart after idle

Run a command after the daemon has shut down:

```bash
hebbs recall "framework" --strategy similarity --top-k 3 --format json
```

**Expected:** The CLI falls back to direct local mode (or auto-starts a new daemon). Either way, the command succeeds. The user never sees a "daemon not running" error.

---

## Phase 9: Resilience

### 9.1 Delete a vault's data while daemon is running

Start the daemon:

```bash
hebbs serve --foreground --idle-timeout 300 &
sleep 2
```

Delete a project's `.hebbs/` directory:

```bash
rm -rf ~/hebbs-test-project/.hebbs
```

Try to recall from that project:

```bash
cd ~/hebbs-test-project
hebbs recall "framework" --strategy similarity --top-k 3 --format json 2>&1
```

**Expected:** Returns an error message (not initialized, vault missing). The daemon does NOT crash. Other vaults still work:

```bash
hebbs recall "dark mode" --strategy similarity --top-k 3 --global --format json
```

**Expected:** Global recall still works.

### 9.2 Recreate and recover

```bash
cd ~/hebbs-test-project
hebbs init .
hebbs remember "Recovery test: vault has been recreated" \
  --importance 0.5 --entity-id recovery --format json
hebbs recall "Recovery test" --strategy similarity --top-k 3 --format json
```

**Expected:** Init succeeds, remember succeeds, recall returns the new memory. The brain is back.

Note: Previously stored memories are gone (the index was deleted). Only file-backed memories can be recovered by re-indexing:

```bash
hebbs index .
hebbs recall "error handling" --strategy similarity --top-k 3 --format json
```

**Expected:** File-backed memories (from `conventions.md`, etc.) are re-indexed and recallable again.

### 9.3 Kill the daemon hard

```bash
kill -9 $(cat ~/.hebbs/daemon.pid)
sleep 1
```

**Expected:** Stale PID and socket files remain (SIGKILL gives no cleanup chance).

Run a command:

```bash
hebbs recall "test" --format json 2>/dev/null
```

**Expected:** The CLI handles the stale state gracefully. Either it cleans up and auto-starts a new daemon, or it falls back to direct mode. The command succeeds.

---

## Phase 10: Cleanup

```bash
# Stop daemon
kill $(cat ~/.hebbs/daemon.pid 2>/dev/null) 2>/dev/null || true

# Remove test projects
rm -rf ~/hebbs-test-project
rm -rf ~/hebbs-test-project-2

# Optionally remove global brain (careful: this deletes all global memories)
# rm -rf ~/.hebbs
```

---

## Checklist

| # | Test | Result |
|---|---|---|
| 1.1 | Website loads | |
| 1.2 | Binary installs | |
| 1.3 | Version prints | |
| 2.1 | Init creates .hebbs/ | |
| 2.2 | Remember/recall/forget roundtrip | |
| 2.3 | Status works on empty vault | |
| 3.1 | Remember with importance + entity | |
| 3.2 | Remember with context metadata | |
| 3.3 | Remember with graph edges | |
| 3.4 | Recall: similarity strategy | |
| 3.5 | Recall: temporal strategy | |
| 3.6 | Recall: custom scoring weights | |
| 3.7 | Get by ID | |
| 3.8 | Inspect (detail + graph) | |
| 3.9 | Prime an entity | |
| 3.10 | Forget by ID | |
| 3.11 | Forget by entity | |
| 3.12 | Export as JSONL | |
| 4.1 | Create markdown files | |
| 4.2 | Index the vault | |
| 4.3 | List with sections | |
| 4.4 | Recall from indexed files | |
| 4.5 | Mixed recall (files + agent) | |
| 4.6 | Edit file and re-index | |
| 4.7 | Status after indexing | |
| 5.1 | Global brain init | |
| 5.2 | Store global preferences | |
| 5.3 | Brain isolation verified | |
| 5.4 | Prime both brains | |
| 5.5 | Second project brain isolation | |
| 6.1 | Accumulate 5+ memories | |
| 6.2 | Reflect-prepare returns clusters | |
| 6.3 | Read and reason about clusters | |
| 6.4 | Reflect-commit stores insight | |
| 6.5 | Insights query returns insight | |
| 7.1 | SKILL.md installed in agent | |
| 7.2 | Agent runs policy bootstrap | |
| 7.3 | Agent stores memories from conversation | |
| 7.4 | Agent-stored memories are recallable | |
| 7.5 | Cross-session persistence works | |
| 8.1 | Daemon starts | |
| 8.2 | Socket + PID files exist | |
| 8.3 | Commands route through daemon | |
| 8.4 | Multi-vault through daemon | |
| 8.5 | Daemon is faster than cold start | |
| 8.6 | Idle shutdown cleans up | |
| 8.7 | Auto-restart after idle | |
| 9.1 | Vault deletion: error, not crash | |
| 9.2 | Recreate and recover | |
| 9.3 | SIGKILL: graceful fallback | |
| 10 | Cleanup complete | |

**Total: 46 checks**

Tester: _______________  Date: _______________  All passed: [ ] Yes  [ ] No
