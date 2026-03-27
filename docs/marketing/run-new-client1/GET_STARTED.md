# HEBBS: Get Started

Your files are your brain. HEBBS indexes them into atomic facts, watches for changes, and gives your agent four retrieval strategies with tunable parameters. This guide gets you from zero to tuned retrieval.

---

## Phase 1: Install and Index (5 minutes)

### 1. Install HEBBS

```sh
brew install hebbs-ai/tap/hebbs
```

If not on macOS with Homebrew:

```sh
curl -sSf https://hebbs.ai/install | sh
```

After curl install, add to PATH (the installer prints the exact line):

```sh
echo 'export PATH="$HOME/.hebbs/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
```

Verify:

```sh
hebbs --version
```

### 2. Set your API key

```sh
echo 'export OPENAI_API_KEY="sk-proj-your-key-here"' >> ~/.zshrc && source ~/.zshrc
```

Or for Anthropic: `export ANTHROPIC_API_KEY="sk-ant-your-key-here"`

### 3. Initialize your vault

```sh
cd /path/to/your/files
hebbs init . --provider openai --key $OPENAI_API_KEY
```

One command. `--model` is optional (defaults to `gpt-4o-mini` for OpenAI). Embedding auto-configures to OpenAI `text-embedding-3-small` with the same key. No local model download needed.

This creates a `.hebbs/` directory, validates LLM connectivity, and starts the daemon. LLM and embedding config are saved globally to `~/.hebbs/config.toml`, so future projects just need `hebbs init .` with no flags.

Other providers:

```sh
hebbs init . --provider anthropic --key $ANTHROPIC_API_KEY
hebbs init . --provider ollama    # local, free, no API key
```

### 4. (Optional) Override embedding config

When using OpenAI, embedding is auto-configured. For other providers, embedding defaults to local ONNX (768 dims, ~600MB download). To override, edit `~/.hebbs/config.toml` BEFORE indexing:

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
dimensions = 1536
batch_size = 50
```

If you skip this step, the default local embeddings work fine. You can always re-index later with better embeddings.

**Important:** Embedding config must be set before `hebbs index`. Changing it after requires `hebbs rebuild .` and a full re-index.

### 5. Index your files

```sh
hebbs index .
```

HEBBS indexes all `.md` files. Each file is split into sections, then each section is decomposed into atomic propositions by the LLM and embedded. Expect ~1 minute per 3-5 files depending on LLM speed.

Exclude files with `.hebbsignore` (same syntax as `.gitignore`):

```
drafts/
*.tmp
node_modules/
```

### 6. Verify it works

```sh
hebbs status
hebbs recall "a question about your content" --format json
```

If recall returns relevant results, you're live. The daemon now watches for file changes and re-indexes automatically. Edit a file, wait ~30-40 seconds, recall again.

---

## Phase 2: Use It (your agent's daily loop)

### Store non-file memories

Not everything lives in files. Store decisions, preferences, corrections:

```sh
hebbs remember "Always use UTC timestamps in API responses" --importance 0.9 --entity-id architecture --format json
hebbs remember "Client prefers bullet points over paragraphs" --importance 0.7 --entity-id user_prefs --format json
```

Importance scale: 0.9 = corrections/rules, 0.7 = decisions, 0.5 = facts, 0.3 = transient.

### Recall with different strategies

```sh
# Simple fact lookup
hebbs recall "what database are we using" --format json

# Detailed fact (increase k to catch supporting context)
hebbs recall "Cloudvault vendor risk assessment findings" -k 10 --format json

# Timeline / what changed (recency-weighted)
hebbs recall "how has the data retention policy changed" -k 10 --weights 0.3:0.5:0.2:0 --format json

# Cross-entity patterns
hebbs recall "which vendors have compliance gaps" --strategy analogical --analogical-alpha 0.3 -k 10 --format json

# Entity history (only works if entity_ids were assigned during extraction)
hebbs recall "ransomware coverage changes" --strategy temporal --entity-id ransomware -k 10 --format json
```

### Weights format

`--weights R:T:I:F` controls four scoring dimensions:
- **R** = Relevance (semantic similarity to your query)
- **T** = Recency (newer memories score higher)
- **I** = Importance (the importance score you assigned)
- **F** = Reinforcement (frequently recalled memories score higher)

Default: `0.5:0.2:0.2:0.1`. Change per query type.

### Agent conversation startup

At the start of every conversation, your agent should:

```sh
hebbs prime user_prefs --max-memories 20 --format json        # load user preferences
hebbs prime project_context --max-memories 15 --format json   # load project context
```

Then load retrieval rules (how to use HEBBS effectively for this vault):

1. If `.hebbs/retrieval-rules.md` exists: read it directly (fastest, no daemon call)
2. If not: `hebbs prime retrieval-instructions --max-memories 20 --format json`
3. If neither exists: use defaults (similarity, k=10)

The rules file is generated after tuning (Phase 4). Until then, defaults work fine.

### Check daemon logs

If something seems off (file not re-indexed, recall stale), check:

```sh
tail -50 ~/.hebbs/daemon.log
```

### Open Memory Palace

```sh
hebbs panel
```

Visual graph of every memory, edge, and relationship in your brain. Search, filter, inspect.

---

## Phase 3: Tune Retrieval (the part that makes HEBBS different)

Out of the box, HEBBS retrieval scores ~50-60% keyword recall. After tuning, ~80-90%. No other memory system lets you do this.

### 7. Read your vault

```sh
hebbs list --sections --format json
```

Understand what's indexed: documents, topics, entities, facts.

### 8. Generate eval queries

Your agent (Claude Code, Cursor, etc.) generates 20+ eval queries based on the vault contents. Each eval has a query, expected keywords, and a query type.

Example:

```json
[
  {
    "id": 1,
    "query": "What is our data retention period for EU customers?",
    "expected_keywords": ["36 months", "EU", "retention", "GDPR"],
    "type": "factual"
  },
  {
    "id": 2,
    "query": "How has RISK-001 changed over time?",
    "expected_keywords": ["single cloud", "Cloudvault", "HIGH", "reduced"],
    "type": "temporal"
  }
]
```

Start with 20 evals to get the loop working. Scale to 100+ for production.

### 9. Run baseline

Run every eval with defaults:

```sh
hebbs recall "What is our data retention period for EU customers?" --format json -k 5
```

For each query, check which expected keywords appear in the returned content. Score: keywords found / keywords expected.

Typical baseline: **50-60% recall, 10-20% perfect queries.**

### 10. Analyze failures

Five common failure patterns:

| Pattern | Symptom | Fix |
|---|---|---|
| k too low | Specific details missing | Increase to k=10 |
| Cue too generic | Returns unrelated docs | Add entity names, dates, specific terms |
| Missing entity names | Broad results, wrong entities | Include the actual name in the cue |
| Wrong strategy | Timeline query returns unordered results | Match strategy to query type (see table below) |
| Term not extracted | Specific dollar amount or proper noun missing | Extraction quality ceiling, no tuning fix |

### 11. Tune and re-run

Apply fixes per query type:

| Query type | Strategy | Flags |
|---|---|---|
| Simple fact ("What is X?") | similarity | `-k 5` |
| Detailed fact ("Tell me about X") | similarity | `-k 10`, entity names in cue |
| Timeline ("What changed?") | similarity | `-k 10 --weights 0.3:0.5:0.2:0`, dates in cue |
| Entity history | temporal | `--strategy temporal --entity-id <entity> -k 10` |
| Cross-entity ("Compare vendors") | analogical | `--strategy analogical --analogical-alpha 0.3 -k 10` |
| Policy/contract importance | similarity | `-k 10 --weights 0.3:0.3:0.4:0` |

Re-run all queries with tuning. Typical tuned result: **80-90% recall, 60-70% perfect queries.**

### 12. Store what you learned

Store each retrieval strategy as a memory. All use the same entity_id so you can load them in one call:

```sh
hebbs remember "RETRIEVAL-INSTRUCTION: Default to k=10. Use k=5 only for simple factual lookups." --importance 0.95 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: For timeline queries, use --weights 0.3:0.5:0.2:0 with k=10. Include dates and entity names in cue." --importance 0.95 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: Always include entity names in cues. Specific beats generic." --importance 0.95 --entity-id retrieval-instructions --format json
```

### 13. Load rules at conversation start

Next conversation, your agent loads the learned rules before any recall:

1. If `.hebbs/retrieval-rules.md` exists: read it directly (fastest)
2. If not: `hebbs prime retrieval-instructions --max-memories 20 --format json`

After Phase 4 (compress), export rules to `.hebbs/retrieval-rules.md` for instant loading without a daemon call. See `hebbs-skill/tune/SKILL.md` Phase 7 for the export format.

---

## Phase 4: Compress and Evolve

After multiple tuning sessions, your agent has dozens of individual strategies. Compress them.

### 14. Load all strategies

```sh
hebbs prime retrieval-instructions --max-memories 50 --format json
```

### 15. Compress to 10-20 rules

Analyze patterns across all strategies. What's universal? What's domain-specific? Write compressed rules that cover the general cases:

```sh
hebbs remember "RETRIEVAL-INSTRUCTION: RULE 1: Default k=10. k=5 only for simple factual lookups." --importance 0.95 --entity-id retrieval-instructions --format json

hebbs remember "RETRIEVAL-INSTRUCTION: RULE 2: For any query with 'changed', 'timeline', or 'history', use --weights 0.3:0.5:0.2:0. For cross-entity, use --strategy analogical --analogical-alpha 0.3." --importance 0.95 --entity-id retrieval-instructions --format json
```

### 16. Delete the granular strategies

```sh
hebbs forget --ids <OLD_STRATEGY_ID_1>,<OLD_STRATEGY_ID_2>,...
```

Keep only the compressed rules. 10-20 rules replace 50+ individual strategies.

### 17. Re-evaluate periodically

As your vault grows (new files, updated content), re-run evals to catch drift. Generate new eval queries for new content. Refine rules. The loop never stops:

```
Generate evals -> Run baseline -> Analyze -> Tune -> Store -> Compress -> Re-evaluate
```

---

## Quick Reference

### Commands you'll use daily

```sh
hebbs recall "query" --format json              # search memory
hebbs remember "fact" --importance 0.7 --format json   # store a fact
hebbs status                                     # vault health
hebbs panel                                      # open Memory Palace
```

### Strategy selection cheat sheet

```
"What is X?"              -> similarity, k=5
"Tell me about X"         -> similarity, k=10, entity names in cue
"What changed?"           -> --weights 0.3:0.5:0.2:0, k=10
"What's the history of?"  -> --strategy temporal --entity-id <entity>
"Compare across..."       -> --strategy analogical --analogical-alpha 0.3, k=10
"What's our policy on?"   -> --weights 0.3:0.3:0.4:0, k=10
```

### Config locations

| File | Purpose |
|---|---|
| `~/.hebbs/config.toml` | Global config (LLM provider, inherited by all vaults) |
| `.hebbs/config.toml` | Per-vault config (overrides global) |
| `.hebbsignore` | Files to exclude from indexing |
| `~/.hebbs/daemon.log` | Daemon logs (file watch, extraction, errors) |

### Recommended config (best quality, tested)

```toml
[llm]
provider = "openai"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
dimensions = 1536
```

gpt-4o-mini for extraction (better entity assignment, cheaper) + OpenAI embeddings for recall quality. This combination tested at 75% baseline, 90% tuned on a 52-file enterprise legal vault.

### Expected results

| Phase | Recall | Perfect Queries |
|---|---|---|
| Baseline (defaults) | 50-60% | 10-20% |
| After k=10 + entity names | 70-75% | 40-50% |
| After strategy/weight tuning | 80-90% | 60-70% |
| Remaining gaps | 10-15% | Extraction quality ceiling |

### Add more projects

```sh
hebbs init /path/to/another/project
hebbs index /path/to/another/project
```

The daemon discovers new vaults instantly. Each vault is independent with its own config and index. Use `--global` flag on remember/recall to access cross-project memories.

### Troubleshooting

| Problem | Fix |
|---|---|
| "command not found" | Add hebbs to PATH: `export PATH="$HOME/.hebbs/bin:$PATH"` |
| "vault not initialized" | Run `hebbs init .` in the project directory |
| "OpenAI provider requires api_key" | Set the env var: `export OPENAI_API_KEY="your-key"` |
| Recall returns stale content after file edit | Check daemon: `tail -20 ~/.hebbs/daemon.log`. If daemon is idle, run any command to restart it. |
| All scores identical / irrelevant results | Embedding model changed after indexing. Run `hebbs rebuild .` then `hebbs index .` |
| Slow first command after inactivity | Normal. Daemon auto-shuts down after 5 min idle. Next command restarts it (~10-20s). |
