# TASK-28: Install and Init UX Issues

**Status:** Open
**Priority:** High
**Created:** 2026-03-26
**Found during:** v0.3.2 user testing on Linux (Ubuntu) and macOS (Intel)

---

## Issue 1: OpenAI provider should default to OpenAI embeddings

**Severity:** High (every OpenAI user hits this)

When a user selects OpenAI as their LLM provider during `hebbs init`, the embedding model still defaults to `embeddinggemma-300m` (local ONNX, 1.2GB download). This is confusing:

- User already provided an OpenAI key
- They didn't ask for a 1.2GB model download
- OpenAI's `text-embedding-3-small` would work immediately with the same key
- The local model adds unnecessary setup friction

**Expected behavior:** When `--provider openai` is set (or selected interactively), auto-configure:

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"   # same env var as [llm]
dimensions = 1536
```

Skip the 1.2GB model download entirely.

**Fix location:** `hebbs-vault/src/bin/hebbs.rs` in the init command handler. After LLM provider is configured, if provider is `openai`, set the embedding config to OpenAI defaults.

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

## Workaround (for current testers)

1. Set the env var:
```bash
export OPENAI_API_KEY="sk-proj-your-key-here"
```

2. Edit `~/.hebbs/config.toml`:
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

3. Stop daemon and rebuild:
```bash
hebbs stop
hebbs rebuild .
```
