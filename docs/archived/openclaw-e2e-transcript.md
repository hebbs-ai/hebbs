# OpenClaw HEBBS Skill E2E Test Transcript (2026-03-12)

Full transcript of Heli (openclaw agent, model: openai-codex/gpt-5.4) installing the hebbs skill, running a real E2E test, and reporting findings.

## Session setup

Heli loaded daily memory file `memory/2026-03-12.md` which contained:
- "Parag said he does not prefer em-dashes in writing."
- "Parag explicitly said he never prefers em-dashes in replies."
- "Parag said I should never produce em-dashes or double dashes in replies."

MEMORY.md and 2026-03-11.md were missing (ENOENT errors). BOOTSTRAP.md existed.

---

## Install flow

User asked: "can you install hebbs skill: https://github.com/hebbs-ai/hebbs-skill"

Heli:
1. Checked existing skills directory at `~/.nvm/versions/node/v25.8.0/lib/node_modules/openclaw/skills` (bundled skills)
2. Read openclaw docs for skills config (`skills-config.md`, `cli/skills.md`)
3. Found the `load.extraDirs` config path for managed skills
4. Cloned the repo to `/tmp/hebbs-skill-inspect`, read README.md and SKILL.md
5. Read `~/.openclaw/openclaw.json` to understand existing config
6. Installed via:
   ```
   mkdir -p ~/.openclaw/skills
   git clone --depth 1 https://github.com/hebbs-ai/hebbs-skill /tmp/hebbs-skill-install
   cp -r /tmp/hebbs-skill-install/hebbs ~/.openclaw/skills/hebbs
   ```
7. Ran `brew install hebbs-ai/tap/hebbs` (already installed 0.1.2)
8. Verified with `openclaw skills list` and `openclaw skills info hebbs`

Result: `hebbs Ready` (source: openclaw-managed)

---

## Server verification

```
hebbs-cli recall "test" --format json
```

Returned existing memory:
```json
[{"memory":{"memory_id":"01KKG9GYMXG5RFP1S6ZYS6WKTS","content":"The user never wants em-dashes or double dashes in replies.","importance":0.95,"context":{},"entity_id":"user_prefs",...},"score":0.59580684,"relevance":0.41184366,...}]
```

Server already running: `46022 hebbs-server start --data-dir /Users/paragarora/.hebbs/data`

---

## E2E test attempt 1 (entity: hebbs_e2e_20260312_1148)

Stored 4 memories successfully. Failed on 5th:

```
hebbs-cli remember "Project Aurora launch slipped because API latency exceeded 900ms in production." \
  --importance 0.84 --entity-id hebbs_e2e_20260312_1148 \
  --edge hebbs_e2e_20260312_1148:related_to:0.8

Error: Invalid memory ID 'hebbs_e2e_20260312_1148'. Expected 26-char ULID or 32-char hex string.
Process exited with code 2.
```

**Issue: Agent passed entity ID instead of memory ID to --edge flag.**

---

## E2E test attempt 2 (entity: hebbs_e2e_20260312_1148b)

Stored 4 memories successfully. Failed on 5th again:

```
hebbs-cli remember "Project Aurora launch slipped because API latency exceeded 900ms in production." \
  --importance 0.84 --entity-id hebbs_e2e_20260312_1148b \
  --edge Project:related_to:0.8

Error: Invalid memory ID 'Project'. Expected 26-char ULID or 32-char hex string.
Process exited with code 2.
```

**Issue: Agent's ID extraction regex grabbed the content text "Project" instead of the ULID from human-readable output.**

---

## E2E test attempt 3 (entity: hebbs_e2e_20260312_1148c) - SUCCESS

Agent switched to a ULID-aware regex parser. Full test completed with exit code 0.

### Memories stored (8 total)

| # | Content | Importance |
|---|---------|------------|
| 1 | Parag prefers concise answers with clear recommendations. | 0.92 |
| 2 | Parag dislikes em dashes in replies and wants plain punctuation. | 0.98 |
| 3 | Parag wants top picks first when given multiple options. | 0.88 |
| 4 | Project Aurora is the codename for the internal analytics dashboard refresh. | 0.75 |
| 5 | Project Aurora launch slipped because API latency exceeded 900ms in production. | 0.84 |
| 6 | The team decided to cache dashboard queries for 10 minutes to reduce Aurora latency. | 0.90 |
| 7 | After enabling query caching, median Aurora response time dropped below 250ms. | 0.86 |
| 8 | Parag prefers actionable summaries over long background explanations. | 0.90 |

All writes completed in 7-10ms.

### Prime results

Returned 5 memories: a mix of temporal (most recent) and project context. Mixed topic purity (preferences and project memories blended together under one entity).

### Recall: similarity (preferences query)

Query: "How should I write replies for Parag?"

Top results:
1. "Parag dislikes em dashes..." (relevance: 0.755, score: 0.773)
2. "Parag prefers actionable summaries..." (relevance: 0.643, score: 0.716)
3. "Parag prefers concise answers..." (relevance: 0.627, score: 0.697)
4. "Parag wants top picks first..." (relevance: 0.591, score: 0.671)

**All four preference memories surfaced correctly. No project noise.**

### Recall: similarity (project query)

Query: "What is the status of Project Aurora performance?"

Top results:
1. "After enabling query caching, median Aurora response time dropped below 250ms." (relevance: 0.709)
2. "Project Aurora launch slipped because API latency exceeded 900ms..." (relevance: 0.723)
3. "Project Aurora is the codename..." (relevance: 0.645)
4. "The team decided to cache dashboard queries..." (relevance: 0.553)
5. "Parag dislikes em dashes..." (relevance: 0.403) - noise

**Four relevant results plus one unrelated preference memory at the tail.**

### Recall: temporal

Returned memories in newest-first order regardless of topic. Latest preference memory ranked #1 even for project-related context. Works as a recency feed but not topic-aware.

### Recall: causal

Query seeded from memory ID7 (caching result). Results:
1. "The team decided to cache dashboard queries..." (causal relevance: 0.828)
2. "Project Aurora launch slipped..." (causal relevance: 0.837)
3. **"Parag dislikes em dashes..."** (causal relevance: 0.349) - **unrelated noise**
4. "Project Aurora is the codename..." (causal relevance: 0.399)

**Causal chain included an unrelated preference memory. Graph traversal blends semantic similarity too loosely.**

### Recall: analogical

Query: "A performance bottleneck was resolved by adding a caching layer."

Results included all project memories (good) but also preference memories (noise). Structural similarity scores were uniformly 0.5, with only embedding similarity varying. The strategy appears to not differentiate structure well with a small dataset.

### Insights (before reflection)

```
No insights found.
```

### Reflect-prepare

Returned 2 clusters:
- **Cluster 0** (4 memories): All Aurora/project performance memories
- **Cluster 1** (4 memories): All communication preference memories

**Clustering was accurate. Clean topic separation.**

Included pre-built `proposal_system_prompt` and `proposal_user_prompt` for LLM-driven insight generation.

Server logs showed:
```
WARN hebbs_server::server: failed to create reflect proposal LLM provider, falling back to mock
  error=configuration error: OpenAI provider requires api_key
WARN hebbs_server::server: failed to create reflect validation LLM provider, falling back to mock
  error=configuration error: OpenAI provider requires api_key
```

### Reflect-commit

Agent manually crafted an insight and committed:
```
hebbs-cli reflect-commit --session-id 01KKGB7WQR0K1RDS8V3PT6NXTN \
  --insights '[{"content":"Parag consistently prefers concise, actionable replies with top picks first and plain punctuation.","confidence":0.89,"source_memory_ids":["019ce0b3f1fdebf6bd998599c4d82dcd","019ce0b3f20f968e4602cb79c683cce3"],"tags":["preference","communication"]}]'
```

Result: `Insights committed: 1 (9.8ms)`

### Insights (after reflection)

Returned the committed insight:
```json
{"memory_id":"01KKGB7WRS7WTJPP02ZCPRTKT7","content":"Parag consistently prefers concise, actionable replies with top picks first and plain punctuation.","importance":0.89,"kind":"insight","context":{"reflect_cluster_id":0.0,"reflect_confidence":0.89,"reflect_source_count":2.0,"reflect_tags":["preference","communication"]}}
```

### Forget

Forgot memory #3 ("Parag wants top picks first..."):
```
Forgotten: 1, Cascaded: 0, Tombstones: 1 (2.2ms)
```

Post-forget recall confirmed: memory #3 absent, insight and other memories still present.

---

## Storage failure during cleanup

Agent attempted to clean up test entity:

```
hebbs-cli forget --entity-id hebbs_e2e_20260312_1148c

Error: Server unavailable at http://localhost:6380. Is hebbs-server running?
(storage error: storage I/O error in write_batch: atomic batch write failed:
IO error: No such file or directory: While open a file for appending:
/Users/paragarora/.hebbs/data/000008.log: No such file or directory)
```

Server process was still running (`46022 hebbs-server start --data-dir /Users/paragarora/.hebbs/data`) but the data directory was gone:

```
ls: /Users/paragarora/.hebbs: No such file or directory
```

### Recovery

Agent recreated the directory, killed the old process, and restarted:
```
mkdir -p /Users/paragarora/.hebbs/data
pkill -f 'hebbs-server start --data-dir /Users/paragarora/.hebbs/data'
HEBBS_AUTH_ENABLED=false nohup hebbs-server start --data-dir /Users/paragarora/.hebbs/data \
  > /tmp/hebbs-server-restart.log 2>&1 &
```

After 20 seconds (ONNX embedder initialization), reads and writes worked again:
```
--- remember test ---
01KKGBAJ9QTFXQ6WA94MGK8VER episode imp=0.20 0s ago
Temporary post-restart write test.
(10.8ms)

--- forget test ---
Forgotten: 1, Cascaded: 0, Tombstones: 1 (5.9ms)
```

All previously stored memories were lost (fresh data directory).

---

## Agent spelling issue

Throughout its prose analysis, the agent consistently wrote "HEBS" instead of "HEBBS":
- "but ~/.hebs is gone on disk"
- "HEBS fell back to a mock provider"
- "HEBS hit a real write failure"
- "Use HEBS as the working memory layer"

CLI commands used the correct spelling (`hebbs-cli`, `~/.hebbs`). The misspelling occurred only in free-text output, likely from prior session memory where the project was named "hebs."

---

## Agent's verdict (verbatim summary)

> Good enough to use. Very good for similarity-based recall. Not mature enough to trust blindly for all memory tasks. Worth adopting carefully, with backups and cleaner entity structure.
