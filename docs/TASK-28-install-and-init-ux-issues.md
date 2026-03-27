# TASK-28: Install and Init UX Issues

**Status:** Done
**Priority:** High
**Created:** 2026-03-26
**Found during:** v0.3.2 user testing on Linux (Ubuntu) and macOS (Intel)

---

## Issue 1: Default embedding provider should be OpenAI, not local gemma

**Severity:** High (every user hits this)
**Status:** FIXED

When `--provider openai` is used during init, embedding now auto-configures to `text-embedding-3-small` (1536 dims) with the same API key. No local model download. Implemented via `EmbeddingConfig::inherit_from_llm()` in `config.rs`, called from the init handler in `hebbs.rs`.

For non-OpenAI providers (Anthropic, Gemini, Ollama), embedding defaults to local gemma as before.

---

## Issue 2: macOS Intel (x86_64) not supported

**Severity:** Medium (affects older Macs)
**Status:** FIXED (binary available on next release)

Added `x86_64-apple-darwin` to the release workflow matrix (`macos-13` runner), updated the Homebrew formula template with an Intel Mac block, and removed the rejection in the install script. The Intel binary will be built and published on the next tagged release (v0.3.3+).

Changes:
- `hebbs/.github/workflows/release.yml`: added matrix entry, download/checksum, formula template block, sed replacement
- `hebbs-deploy/scripts/install.sh`: changed `die` to `ARTIFACT="hebbs-macos-x86_64"`
- `homebrew-tap/Formula/hebbs.rb`: added `elsif Hardware::CPU.intel?` block (placeholder SHA until next release)

---

## Issue 3: Interactive init prompt for api_key_env is confusing

**Severity:** High (user pasted actual key instead of env var name)
**Status:** FIXED

The interactive prompt now says `API key:` instead of `API key env var [OPENAI_API_KEY]:`. Users paste the actual key directly. It is stored as `api_key` in config. The canonical env var name (e.g. `OPENAI_API_KEY`) is auto-set as `api_key_env` without asking.

---

## Issue 4: Embedding model download is slow and unexpected

**Severity:** Low (one-time, but bad first impression)
**Status:** FIXED

When embedding provider is an API provider (openai), the local model download is skipped entirely. The init output says: "Using API embeddings (openai/text-embedding-3-small). No local model needed."

---

## Issue 5: Embedding config ignores `api_key`, only reads `api_key_env`

**Severity:** High (blocks anyone using `api_key` for embeddings)
**Status:** FIXED

Added `api_key: Option<String>` field to `EmbeddingConfig` with a `resolved_api_key()` method that checks `api_key` first, then falls back to `api_key_env` env lookup, then falls back to `LlmConfig.resolved_api_key()`. Both `setup_engine()` and daemon config population now use this resolution chain.

---

## Issue 6: Empty local `[llm]` overrides global config instead of inheriting

**Severity:** High (breaks the "configure once" promise)
**Status:** FIXED

Added `skip_serializing_if = "EmbeddingConfig::is_default"` on `VaultConfig.embedding` (same pattern as `LlmConfig::is_empty`). Default embedding config is no longer written to local `.hebbs/config.toml`. When LLM + embedding are configured via flags, they are saved to global `~/.hebbs/config.toml` only. Local config inherits from global.

---

## Additional fix found during testing

**`--provider openai` without `--model` saved `model = ""` to config.**

Added default model per provider: `gpt-4o-mini` (openai), `claude-haiku-4-5-20251001` (anthropic), `gemini-2.0-flash` (gemini), `gemma3:1b` (ollama).

---

## Changes made (files)

### Code
- `hebbs/crates/hebbs-vault/src/config.rs`: `api_key` field, `resolved_api_key()`, `is_default()`, `inherit_from_llm()`, `skip_serializing_if` on embedding
- `hebbs/crates/hebbs-vault/src/lib.rs`: `init_with_llm()` accepts `Option<EmbeddingConfig>`
- `hebbs/crates/hebbs-vault/src/bin/hebbs.rs`: wizard prompt, `--key` alias, auto-embedding, default models, key resolution in `setup_engine()` and daemon config

### Docs
- `hebbs/SETUP.md`: updated init examples, embedding section, optional fields
- `hebbs/skills/hebbs/SKILL.md`: updated init examples with `--key`
- `hebbs-skill/SKILL.md`: updated init examples with `--key`
- `hebbs-docs/src/content/docs/getting-started/quickstart.mdx`: updated init examples
- `hebbs-docs/src/content/docs/vault/configuration.mdx`: added embedding provider/api_key/api_key_env fields
- `AGENTS.md`: added Testing Rules section

---

## What's still pending

1. **Config round-trip tests broken locally:** `test_config_round_trip` and `test_config_load_missing_file` fail on any machine with `~/.hebbs/config.toml` because `VaultConfig::load()` merges with real global config. Tests should set up controlled global config fixtures or mock the global path. Not a production issue.
2. **Intel Mac binary not yet published:** The CI/formula/install script changes are in place but the actual binary won't exist until the next tagged release (v0.3.3+).
