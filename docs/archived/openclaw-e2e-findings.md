# OpenClaw E2E Findings (2026-03-12)

Heli (openclaw agent) installed the hebbs skill, ran a real E2E test with 8 memories, exercised all operations, and gave a qualitative assessment. Key takeaways below.

## What worked well

- **Similarity recall** was the strongest feature. Querying "How should I write replies for Parag?" correctly surfaced all four preference memories (no em dashes, concise answers, top picks first, actionable summaries). This is the primary value prop.
- **Remember** was fast (single-digit ms) and reliable.
- **Prime** returned a blend of recent + relevant memories. Good for session startup context loading.
- **Reflect-prepare** clustered 8 memories into two sensible groups: project performance cluster and communication preference cluster. Clustering quality was genuinely good.
- **Reflect-commit** worked. The synthesized insight ("Parag consistently prefers concise, actionable replies with top picks first and plain punctuation") was accurate.
- **Insights** returned the committed insight correctly after reflection.
- **Forget** removed the targeted memory. Subsequent recall excluded it while keeping related memories.

## What was weak

- **Causal recall** was noisy. It pulled unrelated preference memories into Aurora project causal chains. Graph traversal blends semantic similarity too loosely, or entity scoping needs to be stricter.
- **Analogical recall** was the weakest. Structural similarity scores were too fuzzy with a small dataset. More experimental than essential.
- **Temporal recall** did not care about topic purity. It surfaced the latest memories regardless of relevance. Works as a recency feed but not for nuanced questions.
- **Reflection LLM** fell back to mock provider because no OpenAI API key was configured. Automatic proposal/validation is not fully functional without LLM provider setup. Manual prepare + agent reasoning + commit still works.

## Real issues found

### Storage reliability
During cleanup, the agent hit a write failure:
- Server process was still running
- `~/.hebbs` directory was missing on disk
- Writes failed with `storage I/O error: No such file or directory`
- After recreating the directory and restarting the server, everything recovered

This is a real reliability concern. Unknown whether something external removed the directory or whether the storage handling is fragile. Recoverable, but needs investigation.

### CLI ergonomics for agents
- `--edge` flag expects a 26-char ULID or 32-char hex memory ID, not entity IDs. The agent tried `--edge entity_id:related_to:0.8` and got a parse error. The SKILL.md documents this correctly but the agent still got it wrong on first attempt.
- Human-readable output is easy to misparse in scripts. The agent's ID extractor grabbed content text instead of the memory ID on the second attempt. `--format json` is the only safe path for programmatic use.

## Skill adoption observations

### Why the agent used memory_search instead of HEBBS initially
Before the skill was installed, the agent had a `memory_search` tool with explicit policy backing ("before answering anything about prior work, decisions, dates, people, preferences, or todos, run memory_search"). HEBBS was not installed as a skill yet, and even after installation, skills are weaker than first-class tools in priority hierarchy.

The updated SKILL.md (with Trigger section and Priority/fallback chain) addresses this by declaring HEBBS as the preferred memory source. But it still competes with any native memory tool that has stronger policy backing in the system prompt.

### What would make HEBBS win consistently
From the conversation, the agent identified these levers (in order of impact):
1. Make HEBBS operations first-class tools (not just a skill/CLI)
2. Rewrite the memory recall policy in the system/developer prompt to say "HEBBS first"
3. Expose tools like `hebbs_recall`, `hebbs_remember`, `hebbs_insights` directly
4. Make file memory explicitly secondary in the prompt hierarchy
5. Add an aggregate `recall_context(query)` tool that internally does HEBBS then file memory fallback

### Recommended operating model
Use HEBBS + file memory together:
- HEBBS: preferences, decisions, corrections, project checkpoints, conversational continuity
- MEMORY.md / daily files: durable human-readable notes, summaries, backup

### Entity discipline matters
Mixed-context entities made retrieval noisy. Recommended buckets:
- `user_prefs` for lasting preferences
- `writing_prefs` for style rules
- `project_<name>` for project context
- `people_<name>` for people context

## Verdict

Good enough to use. Very good for similarity-based recall of preferences and decisions. Not mature enough to trust blindly as the sole memory system. Best paired with file memory as backup, with clean entity scoping, and with a health check for the data directory.
