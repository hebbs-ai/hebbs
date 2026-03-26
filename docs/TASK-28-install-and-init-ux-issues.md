# TASK-28: Install and Init UX Issues

**Status:** Open
**Priority:** High
**Created:** 2026-03-26
**Found during:** v0.3.2 user testing on Linux (Ubuntu) and macOS (Intel)

---

## Issue 1: Default embedding provider should be OpenAI, not local gemma

**Severity:** High (every user hits this)

The default embedding model is `embeddinggemma-300m` (local ONNX, 1.2GB download) regardless of what LLM provider the user picks. This should be changed:

**Current behavior:** Embedding always defaults to local gemma. User selects OpenAI as LLM provider, enters their OpenAI key, then watches a 1.2GB model download they didn't ask for. The downloaded model isn't even OpenAI -- it's a local ONNX model that has nothing to do with their provider choice.

**Expected behavior:** Default embedding provider should be `openai` with `text-embedding-3-small`. When a user picks OpenAI as their LLM provider, embeddings should use OpenAI too -- same key, no extra download, no confusion.

For non-OpenAI providers (Anthropic, Gemini, Ollama), the init should either:
- Ask "Use OpenAI for embeddings? (requires OPENAI_API_KEY)"
- Or default to local gemma since those providers don't have embedding APIs

**Default config for OpenAI users:**

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"   # same env var as [llm]
dimensions = 1536
```

No model download. No confusion. Just works.

**Fix location:**
1. Change the default in `hebbs-vault/src/config.rs` -- `EmbeddingConfig::default()` should check if an OpenAI LLM config exists
2. `hebbs-vault/src/bin/hebbs.rs` init handler -- after LLM provider is configured, if provider is `openai`, auto-set embedding to OpenAI defaults
3. Skip the `ensure_model_files` download when embedding provider is not local

---

## Issue 2: macOS Intel (x86_64) not supported

**Severity:** Medium (affects older Macs)

- `curl -sSf https://hebbs.ai/install | sh` fails: "macOS x86_64 (Intel) is not supported"
- `brew install hebbs-ai/tap/hebbs` fails: "formula requires at least a URL" (no x86_64 binary in the formula)
- Only Apple Silicon (M1+) is supported

**Fix:** Add `x86_64-apple-darwin` target to the release workflow matrix in `.github/workflows/release.yml`:

```yaml
- os: macos-13          # Intel runner
  target: x86_64-apple-darwin
  artifact: hebbs-macos-x86_64
```

Also update the Homebrew formula template to include the Intel URL/SHA, and update the install script to support `x86_64-apple-darwin`.

**Note:** ONNX Runtime may have issues on older Intel Macs. Test before releasing.

---

## Issue 3: Interactive init prompt for api_key_env is confusing

**Severity:** High (user pasted actual key instead of env var name)

The interactive prompt says:

```
API key env var [OPENAI_API_KEY]:
```

User typed their actual API key (`sk-proj-...`) instead of the env var name (`OPENAI_API_KEY`). The prompt doesn't explain that it wants the **name** of an environment variable, not the key itself.

**What happened:**
```
API key env var [OPENAI_API_KEY]: sk-proj-oZ-GMWPX1SXx...
```

This stores the literal key string as the "env var name" in config, which then fails at runtime because there's no environment variable named `sk-proj-...`.

**Fix options:**

A. Change the prompt to be clearer:
```
Environment variable containing your API key [OPENAI_API_KEY]:
(Your key should be exported as: export OPENAI_API_KEY="sk-...")
```

B. Auto-detect if the user pasted an actual key (starts with `sk-`) and handle it:
- Store it as `api_key` (direct) instead of `api_key_env`
- Or warn and ask again

C. Ask for the key directly and store it, skip the env var indirection for interactive mode:
```
API key: sk-proj-...
```
Then store as `api_key` in config. Simpler UX at the cost of having the key in the config file.

**Recommendation:** Option B. Detect `sk-` prefix, store as `api_key` directly, print a warning that storing keys in config is less secure than using env vars.

---

## Issue 4: Embedding model download is slow and unexpected

**Severity:** Low (one-time, but bad first impression)

Even when not needed (e.g., OpenAI embedding selected), the init downloads `embeddinggemma-300m` (1.2GB). On slow connections this takes minutes and there's no way to skip it.

The message says "Ensuring embedding model" which sounds mandatory. If the user configured API embeddings, this download should be skipped entirely.

**Fix:** In the init flow, check the embedding config. If `provider = "openai"` (or any API provider), skip the local model download.

---

## Issue 5: Embedding config ignores `api_key`, only reads `api_key_env`

**Severity:** High (blocks anyone using `api_key` for embeddings)

The `[embedding]` section in config supports both `api_key` (direct key) and `api_key_env` (env var name). But the embedding provider code in `setup_engine()` (`hebbs-vault/src/bin/hebbs.rs`) and `create_embedder_from_daemon_config()` (`daemon/mod.rs`) only read `api_key_env`. If a user sets `api_key = "sk-proj-..."` in the `[embedding]` section, it's silently ignored and they get:

```
Error setting up engine: OpenAI embedding requires API key. Set OPENAI_API_KEY env var.
```

The `[llm]` section correctly reads both `api_key` and `api_key_env`. The embedding code should do the same.

**Fix:** In `setup_engine()` and `create_embedder_from_daemon_config()`, check `api_key` first, then fall back to `api_key_env` lookup. Also try inheriting from `[llm]` config if both are empty (same provider, same key).

**Fix locations:**
- `hebbs-vault/src/bin/hebbs.rs` line ~658 in `setup_engine()`
- `hebbs-vault/src/daemon/mod.rs` in `create_embedder_from_daemon_config()`

---

## Issue 6: Empty local `[llm]` overrides global config instead of inheriting

**Severity:** High (breaks the "configure once" promise)

When `hebbs init .` runs without `--provider`, it creates a local `.hebbs/config.toml` with an empty `[llm]` section. If the user later configures `~/.hebbs/config.toml` (global) with their LLM provider, the local empty `[llm]` **overrides** the global config instead of inheriting from it.

**What happens:**
1. User runs `hebbs init .` (no provider flag)
2. Local `.hebbs/config.toml` gets `[llm]` with empty provider/model
3. User edits `~/.hebbs/config.toml` with OpenAI config
4. `hebbs index .` fails: "LLM provider not configured"
5. The empty local `[llm]` shadows the global config

**Expected:** An empty `[llm]` in local config should mean "inherit from global", not "override with nothing".

**Fix:** During config loading, if `[llm].provider` is empty in local config, fall through to global. Or better: don't write `[llm]` to local config at all when it's not configured during init (already partially done with `skip_serializing_if`).

---

## Root Cause: The init UX is too many steps

All of issues 1-6 stem from the same problem: init requires too many decisions and too many things to go right. The cleanest solution for 0.3.3:

### One command, one key, done

```bash
hebbs init . --provider openai --key sk-proj-your-key-here
```

This single command should:

1. Take the key directly (not an env var name)
2. Auto-configure both `[llm]` (gpt-4o-mini) and `[embedding]` (text-embedding-3-small) for that provider
3. Save to global `~/.hebbs/config.toml`
4. NOT write `[llm]` or `[embedding]` to local config (inherits from global)
5. NOT download local embedding model (uses API)
6. Validate LLM connectivity
7. Start daemon

### Interactive mode should be equally simple

```
$ hebbs init .

  Provider [openai]: openai
  API key: sk-proj-...

  Done. Using openai/gpt-4o-mini + text-embedding-3-small.
```

Two prompts. Not four. No "env var name" confusion. No separate embedding config.

### What this eliminates

- No `api_key` vs `api_key_env` confusion (just `--key`)
- No separate embedding config for OpenAI users (auto-configured)
- No 1.2GB model download for OpenAI users (API embeddings)
- No global vs local config confusion (global only, local inherits)
- No empty `[llm]` overriding global (not written to local)

---

## Workaround (for v0.3.2 testers)

The cleanest path on the current release:

```bash
# 1. Set env var (required because embedding only reads api_key_env, not api_key)
echo 'export OPENAI_API_KEY="sk-proj-your-key-here"' >> ~/.bashrc
source ~/.bashrc

# 2. Delete existing vault and re-init with provider flag
rm -rf .hebbs
hebbs init . --provider openai --model gpt-4o-mini --api-key-env OPENAI_API_KEY

# 3. Edit global config to add embedding provider
nano ~/.hebbs/config.toml
# Add this section:
# [embedding]
# provider = "openai"
# model = "text-embedding-3-small"
# api_key_env = "OPENAI_API_KEY"
# dimensions = 1536

# 4. Stop daemon (it loaded old config) and index
hebbs stop
hebbs index .
```
