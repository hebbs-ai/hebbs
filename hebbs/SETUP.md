# HEBBS Setup Skill

Everything to get HEBBS running. Init, configure, index, test recall, store memories.

---

## 1. Install

**macOS (Apple Silicon + Intel):**

```sh
brew install hebbs-ai/tap/hebbs
```

**Linux / macOS (curl):**

```sh
curl -sSf https://hebbs.ai/install | sh
```

Note: Intel Mac builds do not include local embeddings (ONNX Runtime). Use `--provider openai --key` to configure API embeddings during init.

After the curl install, add hebbs to your PATH. The installer prints the exact line, but it's:

```sh
# Add to ~/.bashrc, ~/.zshrc, or ~/.profile:
export PATH="$HOME/.hebbs/bin:$PATH"
```

Then reload your shell (`source ~/.bashrc` or open a new terminal).

**Verify:**

```sh
hebbs --version
```

If this prints "command not found", the PATH step above was missed. Run the export line and try again.

---

## 2. Initialize

HEBBS uses a two-brain model: a **global brain** for cross-project knowledge (user identity, preferences, corrections) and a **project brain** for project-specific knowledge (architecture, conventions, team context).

### Project brain (inside any project directory)

```sh
hebbs init .
```

### Global brain (user-level, cross-project)

```sh
hebbs init ~
```

What happens:
- Creates a `.hebbs/` directory at the target path
- Auto-starts the daemon (one daemon serves all vaults)
- Validates LLM connectivity if configured
- Downloads the local embedding model (~600MB, once) only if using local embeddings. Skipped when using API embeddings (e.g. OpenAI).

You do NOT need to check if `.hebbs/` exists before running commands. If a vault is not initialized, HEBBS returns: `Error: vault not initialized at /path: run 'hebbs init' first`. Run `hebbs init <path>` and retry.

---

## 3. Configure LLM Provider

**You only do this once.** The LLM config is saved to `~/.hebbs/config.toml` (global) by default. Every project vault you create after this inherits it automatically.

### Option A: Configure during init (recommended)

```sh
hebbs init . --provider openai --key sk-proj-your-key-here
```

One command. `--model` defaults to `gpt-4o-mini` for OpenAI (each provider has a sensible default). Embedding auto-configures to OpenAI `text-embedding-3-small` when the provider is OpenAI, so no manual embedding setup is needed and no local model download occurs.

If you're on a new or low-tier API account and hit rate limits during indexing, lower the concurrency:

```sh
hebbs init . --provider openai --key sk-proj-your-key-here --max-concurrent 2
```

Or add to `~/.hebbs/config.toml` after init:

```toml
[api]
max_concurrent_requests = 2
```

This saves LLM and embedding config to `~/.hebbs/config.toml` (global). Next time you run `hebbs init` in another project, you just need:

```sh
hebbs init /path/to/another/project    # inherits LLM + embedding from ~/.hebbs/config.toml
```

### Option B: Edit config directly (simplest if you have your key ready)

Edit `~/.hebbs/config.toml` and paste your key directly:

```toml
[llm]
provider = "openai"
model = "gpt-4o-mini"
api_key = "sk-proj-your-actual-key-here"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key = "sk-proj-your-actual-key-here"
dimensions = 1536
```

### Option C: Use the config command

```sh
hebbs config set llm.provider openai
hebbs config set llm.model gpt-4o-mini
hebbs config set llm.api_key_env OPENAI_API_KEY
```

### Provider examples

**OpenAI:**
```toml
[llm]
provider = "openai"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"
```

**Anthropic:**
```toml
[llm]
provider = "anthropic"
model = "claude-haiku-4-5-20251001"
api_key_env = "ANTHROPIC_API_KEY"
```

**Google Gemini:**
```toml
[llm]
provider = "gemini"
model = "gemini-2.0-flash"
api_key_env = "GEMINI_API_KEY"
```

**Ollama (local, free):**
```toml
[llm]
provider = "ollama"
model = "qwen3:4b"
```

No `api_key_env` needed for Ollama. It runs locally.

### Config inheritance

Global config at `~/.hebbs/config.toml` is inherited by project configs. A project's `.hebbs/config.toml` overrides the global config for that vault. Set your LLM provider globally once, and every project inherits it.

### Optional fields

- `--key` / `--api-key`: Pass the API key directly. This is the simplest option. The key is saved to `~/.hebbs/config.toml`.
- `--api-key-env`: Reference an environment variable name (e.g. `OPENAI_API_KEY`) instead of passing the key directly. Better for CI/pipelines.
- `--model`: Override the default model for the provider. Defaults: `gpt-4o-mini` (openai), `claude-haiku-4-5-20251001` (anthropic), `gemini-2.0-flash` (gemini), `gemma3:1b` (ollama).
- `base_url`: Override the API endpoint (useful for proxies or self-hosted models)

---

## 5. Configure Embedding Provider

**When you use `--provider openai --key ...` during init, embedding is auto-configured.** You get `text-embedding-3-small` with the same key, no manual setup needed. This section is only relevant if you want to customize the embedding provider independently.

LLM and embedding are independent configurations. You can use Anthropic for LLM extraction and OpenAI for embeddings. They are separate `[llm]` and `[embedding]` sections in config.

### Default behavior

- **OpenAI provider**: embedding auto-configures to `text-embedding-3-small` (1536 dims), inherits the LLM API key. No local model download.
- **Other providers** (anthropic, gemini, ollama): embedding defaults to local `embeddinggemma-300m` (768 dims, ONNX, ~600MB download on first use).

### Manual embedding config

To override the auto-configured embedding, edit `~/.hebbs/config.toml`:

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key = "sk-proj-your-key-here"    # or use api_key_env = "OPENAI_API_KEY"
dimensions = 1536
batch_size = 50
# base_url = "https://api.openai.com"   # optional, for proxies or Azure
```

The `api_key` field accepts the key directly. Alternatively, `api_key_env` references an environment variable name. If both are set, `api_key` takes precedence.

### Mixed provider example

Use Anthropic for LLM (extraction, contradictions) and OpenAI for embeddings:

```toml
[llm]
provider = "anthropic"
model = "claude-haiku-4-5-20251001"
api_key_env = "ANTHROPIC_API_KEY"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
dimensions = 1536
```

### When to use which

| | Local (default) | OpenAI API |
|---|---|---|
| Cost | Free | ~$0.02 per 1M tokens |
| Speed | Fast (~5ms, no network) | Slower (~100ms, API call) |
| Quality | Good | Better |
| Setup | Nothing | API key required |
| Offline | Yes | No |
| Dimensions | 768 | 1536 |

**Start with local.** Switch to API embeddings only if recall quality is insufficient after tuning.

### Changing embedding model after indexing

If you change the embedding model or dimensions after files are already indexed, you must re-index:

1. Stop the daemon (it will restart on next command)
2. Run `hebbs rebuild <path>` to delete the index and rebuild
3. Run `hebbs index <path>` to re-embed everything

Embeddings from different models are incompatible. You cannot mix local 768-dim vectors with OpenAI 1536-dim vectors in the same vault.

---

## 6. Index Files

```sh
hebbs index .
```

This indexes every `.md` file in the vault. Indexing runs in two phases:

- **Phase 1 (parse):** Splits files into sections by headings. Fast, local only.
- **Phase 2 (embed + extract):** Embeds sections, extracts atomic propositions and entities via the LLM. Slower, depends on LLM provider speed.

Output example (52 files, OpenAI gpt-4o-mini):

```
Phase 1/2: parsing 52 file(s)...
Phase 2/2: embedding 465 section(s)...
Indexed 52 file(s). 1119 memories created.
```

Each file produces:
- **Document-level memories** (one per section)
- **Proposition memories** (atomic facts extracted by the LLM)
- **Entity extraction** (named entities with auto-assigned entity_ids)
- **Graph edges** (relationships between memories: revised_from, related_to, etc.)

### .hebbsignore

Create a `.hebbsignore` file in the vault root (next to `.hebbs/`) to exclude files:

```
# .hebbsignore (same syntax as .gitignore)
vendor/
generated/
drafts/
*.tmp
```

Built-in excludes (always active): `.hebbs/`, `.git/`, `.obsidian/`, `node_modules/`, `contradictions/`

You can also set excludes in config:

```toml
[watch]
ignore_patterns = ["vendor/", "generated/"]
```

### After indexing

The daemon watches for file changes and re-indexes automatically. Edit a `.md` file, and HEBBS updates the index in seconds without re-running `hebbs index`.

---

## 7. Add More Folders

Each vault is independent with its own `.hebbs/` directory and config.

```sh
# Initialize another project
hebbs init /path/to/another/project

# Index it
hebbs index /path/to/another/project
```

The daemon discovers new vaults instantly. No restart needed. It watches all initialized vaults simultaneously.

Each vault can have its own LLM and embedding config, or inherit from the global `~/.hebbs/config.toml`.

---

## 8. Test Recall

### Basic similarity search

```sh
hebbs recall "your query here" --format json
```

### Verify results are relevant

Check that returned memories contain the facts you expect. If results are poor:
- Try a more specific cue: "ransomware coverage limit" beats "insurance"
- Include entity names: "Cloudvault vendor risk" beats "vendor risk"
- Increase k: `hebbs recall "query" -k 10 --format json`

### Try different strategies

**Similarity** (default, semantic search):
```sh
hebbs recall "What is our ransomware coverage?" --format json
```

**Temporal** (chronological order, requires entity_id):
```sh
hebbs recall "ransomware coverage changes" --strategy temporal --entity-id ransomware -k 10 --format json
```

**Recency-weighted** (prioritize recent information):
```sh
hebbs recall "RISK-001 Cloudvault dependency" --weights 0.3:0.5:0.2:0 -k 10 --format json
```

**Analogical** (cross-entity structural patterns):
```sh
hebbs recall "Which vendors have similar compliance gaps?" --strategy analogical --analogical-alpha 0.3 --format json
```

### Weights format

`--weights R:T:I:F` controls the four scoring dimensions:
- **R** = Relevance (semantic similarity)
- **T** = Recency (temporal, newer scores higher)
- **I** = Importance (importance score assigned at storage)
- **F** = Reinforcement (access frequency, more recalled = higher)

Default: `0.5:0.2:0.2:0.1`. Adjust per query type.

---

## 9. Store Non-File Memories

Not everything lives in files. Store conversations, decisions, corrections, and preferences directly.

### Basic store

```sh
hebbs remember "User prefers dark mode in all code editors" --importance 0.7 --format json
```

Returns:
```json
{"memory_id": "01JABCDEF..."}
```

### Importance scale

| Score | Use for | Examples |
|---|---|---|
| 0.9 | Corrections, standing instructions | "Never use em-dashes", "Always run tests before commit" |
| 0.7 | Decisions, preferences | "We chose Postgres over MySQL", "User prefers dark mode" |
| 0.5 | General facts (default) | "Team has 5 engineers", "Deploy on Fridays" |
| 0.3 | Transient, low-priority | "Currently debugging auth issue", "Meeting notes from today" |

### Entity IDs for grouping

Group related memories with `--entity-id`:

```sh
hebbs remember "Never use ORM, raw SQL only" --importance 0.9 --entity-id architecture --format json
hebbs remember "Alice owns the auth module" --importance 0.5 --entity-id team --format json
hebbs remember "User dislikes verbose output" --importance 0.8 --entity-id user_prefs --global --format json
```

Entity IDs enable temporal recall: `hebbs recall "architecture decisions" --strategy temporal --entity-id architecture`

### Global vs project

- `--global`: Stores in `~/.hebbs/`. Cross-project knowledge (user identity, preferences).
- No flag: Stores in the current project's `.hebbs/`. Project-specific knowledge.

### Linking memories with edges

Connect related memories:

```sh
# Store a fact
hebbs remember "Migrated from MySQL to Postgres" --importance 0.7 --entity-id architecture --format json
# Returns: {"memory_id": "01JAB123..."}

# Store a follow-up, linked
hebbs remember "MySQL migration caused 2 hours downtime" --importance 0.6 --entity-id architecture --edge "01JAB123:caused_by:0.9" --format json
```

Edge format: `TARGET_ID:TYPE[:CONFIDENCE]`

Edge types: `caused_by`, `related_to`, `followed_by`, `revised_from`, `contradicts`

### Pipe long content via stdin

```sh
echo "Long memory content here..." | hebbs remember --importance 0.6 --format json
```

---

## 10. Verify Everything

### Check vault health

```sh
hebbs status
```

Shows: daemon status, memory count, indexed files, vault path.

### Open Memory Palace

```sh
hebbs panel
```

Opens `http://127.0.0.1:6381` in your browser. Interactive graph of every memory: nodes are memories, edges are relationships, red dashed lines are contradictions. Search, filter, adjust ranking weights, view timeline.

### Test recall

```sh
hebbs recall "test query relevant to your content" --format json
```

Verify that results contain relevant content from your indexed files.

### Checklist

- [ ] `hebbs --version` prints a version number
- [ ] `hebbs status` shows daemon running and memory count > 0
- [ ] `hebbs recall "test" --format json` returns results from indexed files
- [ ] `hebbs panel` opens Memory Palace in browser
- [ ] `hebbs remember "test" --format json` returns a memory_id
- [ ] (If global brain) `hebbs recall "test" --global --format json` works

Your brain is ready. Every file change is auto-indexed by the daemon. Every `hebbs remember` is instantly searchable. Every conversation starts with full context via `hebbs prime`.
