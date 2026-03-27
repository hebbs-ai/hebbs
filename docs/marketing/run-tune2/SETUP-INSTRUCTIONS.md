# Hebbs Setup Instructions

## Install

```bash
brew install hebbs-ai/tap/hebbs
hebbs --version   # should show 0.3.2
```

## Initialize a vault

```bash
cd /path/to/your/markdown/files

# OpenAI (recommended)
hebbs init . --provider openai --key $OPENAI_API_KEY

# Or Anthropic
hebbs init . --provider anthropic --key $ANTHROPIC_API_KEY

# Or local (no API key, uses Ollama)
hebbs init . --provider ollama
```

## Index your files

```bash
hebbs index .
```

This parses all `.md` files, extracts propositions and entities via the LLM, embeds everything locally, and creates memories. Takes ~30 seconds per file with gpt-4o-mini.

## Use it

### Recall (search your memory)

```bash
# Simple factual lookup
hebbs recall "What is the SLA uptime?" -k 5

# Detailed query (include entity names for better results)
hebbs recall "Cloudvault uptime SLA terms" -k 10

# Timeline / evolution query (weight recency)
hebbs recall "RISK-001 cloud dependency changes" -k 10 --weights 0.3:0.5:0.2:0

# Cross-entity comparison (analogical strategy)
hebbs recall "vendor compliance gaps" --strategy analogical --analogical-alpha 0.3 -k 10

# Entity timeline (temporal strategy)
hebbs recall "Cloudvault risk" --strategy temporal --entity-id cloudvault -k 10
```

### Remember (store a memory)

```bash
hebbs remember "The board approved a 1.2M security budget for 2025" --importance 0.9

# With entity scoping (enables temporal recall for this entity)
hebbs remember "Cloudvault SLA renegotiated to 99.95%" --entity-id cloudvault --importance 0.8

# With edges (enables causal traversal)
hebbs remember "SLA breach caused contract renegotiation" --entity-id cloudvault --edge MEMORY_ID:caused_by:0.9
```

### Inspect

```bash
hebbs status              # vault stats
hebbs inspect MEMORY_ID   # see edges, entity_id, decay score
hebbs panel               # open Memory Palace in browser
```

## Retrieval tuning rules

These are the rules an agent should follow when choosing recall parameters:

| Query type | Flags |
|---|---|
| Simple fact ("What is X?") | `-k 5` |
| Detailed fact ("What happened during X?") | `-k 10`, include entity names in cue |
| Timeline ("How has X changed?") | `-k 10 --weights 0.3:0.5:0.2:0`, include dates |
| Cross-entity ("Which vendors?") | `-k 10 --strategy analogical --analogical-alpha 0.3` |
| Entity history | `--strategy temporal --entity-id <entity> -k 10` |
| Causal chain ("Why did X happen?") | `--strategy causal --seed <id> --max-depth 3 --edge-types caused_by` |
| Contract/policy terms | `-k 10 --weights 0.3:0.3:0.4:0` (importance-weighted) |

### Key rules

1. **Default to k=10.** k=5 misses supporting details on most queries.
2. **Include entity names in cues.** "Cloudvault SLA" beats "vendor SLA".
3. **Include dates.** "Q4 2024 to Q2 2025" beats "recent changes".
4. **Use analogical for cross-entity.** Any query comparing or finding patterns across entities.
5. **Weights format:** `relevance:recency:importance:reinforcement` (sum to ~1.0).

## Config reference

Config lives in `.hebbs/config.toml`:

```toml
[llm]
provider = "openai"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"

[embedding]
model = "embeddinggemma-300m"    # local ONNX (default)
dimensions = 768
batch_size = 50
# For OpenAI embeddings instead:
# provider = "openai"
# model = "text-embedding-3-small"
# api_key_env = "OPENAI_API_KEY"
# dimensions = 1536

[scoring]
w_relevance = 0.5
w_recency = 0.2
w_importance = 0.2
w_reinforcement = 0.1

[contradiction]
enabled = true
min_similarity = 0.7
```

## Ignore files

Create `.hebbsignore` (same syntax as `.gitignore`):

```
node_modules/
*.draft.md
private/
```

## Tested results

On an 8-file enterprise legal vault (v0.3.2):

- **Baseline recall:** 70% (similarity, k=5)
- **Tuned recall:** 94% (k=10, entity names, weights)
- **10 entity_ids** auto-assigned during indexing
- **4 revision edges** detected between policy versions
- **Index time:** 3:32 for 8 files with gpt-4o-mini
