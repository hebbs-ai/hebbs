# HEBBS Release Instructions

How to cut a release for the unified CLI, TypeScript SDK, and Python SDK.

---

## Current Versions

| Component | Version | Repo |
|-----------|---------|------|
| Unified CLI (`hebbs`) | `0.3.0` | `hebbs-ai/hebbs` |
| TypeScript SDK (`@hebbs/sdk`) | `0.3.0` | `hebbs-ai/hebbs-typescript` |
| Python SDK (`hebbs`) | `0.3.0` | `hebbs-ai/hebbs-python` |

---

## How Releases Work

All three repos are **tag-triggered**. Push a `v*` tag and CI builds, tests, and publishes automatically.

- **Unified CLI** (`release.yml`): builds the single `hebbs` binary for linux-x86_64, linux-aarch64, macos-arm64 + Docker image. Creates GitHub Release with assets + checksums. Updates Homebrew tap.
- **TypeScript SDK** (`ci.yml` publish job): runs build + tests then `npm publish --provenance` to npmjs.
- **Python SDK** (`publish.yml`): builds sdist + wheel then publishes to PyPI via trusted publishing (no API key needed, uses OIDC).

The install script at `hebbs-deploy/scripts/install.sh` already reads `releases/latest` from GitHub. Once you tag a stable release, `curl https://hebbs.ai/install | sh` will automatically pick it up.

---

## Installation Methods

After a release, users can install HEBBS via:

### Homebrew (macOS / Linux)

```sh
brew install hebbs-ai/tap/hebbs
```

This installs the unified `hebbs` binary. The Homebrew formula is automatically updated by the `update-homebrew` job in `release.yml` (see below).

### Install script (Linux / macOS)

```sh
curl -sSf https://hebbs.ai/install | sh
```

### npm (TypeScript SDK)

```sh
npm install @hebbs/sdk
```

### pip (Python SDK)

```sh
pip install hebbs
```

---

## Homebrew Tap

The Homebrew formula lives in the external repo [`hebbs-ai/homebrew-tap`](https://github.com/hebbs-ai/homebrew-tap).

### How it's updated automatically

The `update-homebrew` job in `hebbs/.github/workflows/release.yml`:

1. Runs after `create-release` when a `v*` tag is pushed.
2. Downloads release artifacts and computes SHA256 checksums.
3. Checks out `hebbs-ai/homebrew-tap` using `HOMEBREW_TAP_TOKEN`.
4. Generates `Formula/hebbs.rb` with the new version, download URLs, and checksums.
5. Commits and pushes the updated formula.

Supported platforms in the formula:
- macOS ARM64 (`darwin-arm64`)
- Linux x86_64 (`linux-x86_64`)
- Linux aarch64 (`linux-aarch64`)

### Required secret

| Repo | Secret | Used for |
|------|--------|----------|
| `hebbs-ai/hebbs` | `HOMEBREW_TAP_TOKEN` | Push updated formula to `hebbs-ai/homebrew-tap` |

### Manual verification after release

```sh
brew update
brew install hebbs-ai/tap/hebbs
hebbs version  # should show 0.3.0
```

---

## Pre-Release Checklist

### 1. Sync versions across all three repos

All three should have the same version number for a coordinated release.

**Engine** (6 places to update):
- `hebbs/Cargo.toml` (workspace root): `version = "X.Y.Z"` (propagates to all crates)
- `hebbs-typescript/package.json`: `"version": "X.Y.Z"`
- `hebbs-typescript/src/index.ts`: `export const VERSION = 'X.Y.Z'`
- `hebbs-python/pyproject.toml`: `version = "X.Y.Z"`
- `hebbs-python/src/hebbs/__init__.py`: `__version__ = "X.Y.Z"`
- `hebbs-python/demo/cli.py`: `version="X.Y.Z"` in click version_option

### 2. Verify E2E passes on all three

```sh
# Server integration tests
cd hebbs && cargo test --workspace

# TypeScript E2E (needs live server + OpenAI key)
cd hebbs-typescript
HEBBS_API_KEY="hb_..." OPENAI_API_KEY="sk-..." npm run test:e2e

# Python E2E (needs live server + OpenAI key)
cd hebbs-python
HEBBS_API_KEY="hb_..." OPENAI_API_KEY="sk-..." uv run python -m pytest tests/test_e2e_python_sdk.py -v
```

### 3. Check cargo audit

```sh
cd hebbs && cargo audit
```

Fix any HIGH/CRITICAL advisories before tagging. MEDIUM can be assessed case-by-case.

### 4. Verify GitHub secrets are set

| Repo | Secret | Used for |
|------|--------|----------|
| `hebbs-ai/hebbs` | `GITHUB_TOKEN` (auto) | Create GitHub Release, push Docker to GHCR |
| `hebbs-ai/hebbs` | `HOMEBREW_TAP_TOKEN` | Push formula to `hebbs-ai/homebrew-tap` |
| `hebbs-ai/hebbs-typescript` | `NPM_TOKEN` | Publish to npmjs |
| `hebbs-ai/hebbs-python` | PyPI trusted publishing (OIDC, no token) | Publish to PyPI |

Check the Python SDK repo has a PyPI "trusted publisher" configured at `pypi.org/manage/project/hebbs/settings/publishing/` pointing to `hebbs-ai/hebbs-python`, workflow `publish.yml`, environment `pypi`.

---

## Tagging

Tag order matters: **engine first**, then SDKs. The SDKs depend on the engine's proto/API, not the other way around.

### Step 1: Push subtrees

Before tagging, all subtrees must be pushed to their upstream repos:

```sh
# Check which subtrees have changes
for p in hebbs hebbs-typescript hebbs-python hebbs-website hebbs-blog hebbs-docs hebbs-deploy hebbs-skill homebrew-tap; do
  t="last-push/$p"
  if git rev-parse "$t" >/dev/null 2>&1; then
    s=$(git diff --shortstat "$t" -- "$p/")
    if [ -n "$s" ]; then
      echo "CHANGED  $p  $s"
    else
      echo "clean    $p"
    fi
  else
    echo "NO TAG   $p"
  fi
done
```

Push each changed subtree and update tags.

### Step 2: Tag the unified CLI

```sh
# In the hebbs-ai/hebbs upstream repo (or via subtree)
git tag v0.3.0 -m "Release v0.3.0"
git push origin v0.3.0
```

Watch `Actions / Release` on GitHub. Wait for all three matrix builds (linux-x86_64, linux-aarch64, macos-arm64) + Docker + Homebrew tap update to go green before proceeding.

### Step 3: Tag the TypeScript SDK

```sh
git tag v0.3.0 -m "Release v0.3.0"
git push origin v0.3.0
```

Watch `Actions / CI & Publish` publish job. Verify on npmjs: `https://www.npmjs.com/package/@hebbs/sdk`.

### Step 4: Tag the Python SDK

```sh
git tag v0.3.0 -m "Release v0.3.0"
git push origin v0.3.0
```

Watch `Actions / Publish to PyPI`. Verify on PyPI: `https://pypi.org/project/hebbs/`.

---

## Making a Release "Stable" (for Install Script)

The install script resolves the latest version via:
```
https://api.github.com/repos/hebbs-ai/hebbs/releases/latest
```

GitHub's "latest" release is the **most recent non-prerelease, non-draft** release. As long as you don't mark the release as a prerelease, it will automatically become "latest" and the install script will pick it up.

**To pin a specific version on the install script** (optional, for testing):
```sh
HEBBS_VERSION=v0.3.0 curl -sSf https://hebbs.ai/install | sh
```

---

## What Changed Since Last Release (0.2.0 to 0.3.0)

**Vault system (new):**
- `feat(vault): file-first markdown sync with daemon mode`
- `feat(vault): multi-vault daemon with auto-start, proactive vault opening, file watchers`
- `feat(vault): contradiction detection (heuristic + LLM classification)`
- `feat(vault): Memory Palace control panel (force-directed graph, search, sliders, timeline, config editor)`
- `feat(vault): query audit log and stats API`
- `feat(vault): vault lifecycle (init, index, watch, rebuild, status)`

**Unified CLI:**
- `feat(cli): unified hebbs binary replaces hebbs-server, hebbs-cli, and hebbs-vault`
- `feat(cli): dual-mode engine (local embedded RocksDB or remote gRPC/REST client)`
- `feat(cli): brain discovery (--vault flag, HEBBS_VAULT env, walk-up .hebbs/, ~/.hebbs/ fallback)`
- `feat(cli): queries subcommand for query audit log`

**Server fixes:**
- `fix(subscribe): empty kind_filter defaults to [Episode, Insight, Revision] instead of rejecting all`
- `fix(reflect): global reflect infers entity_id on insights when all source memories agree`
- `fix(prime): entity-scoped temporal index scan + cosine ranking replaces global HNSW + post-filter`
- `feat(lineage): source_memory_ids field (proto field 15) for Insight-kind memories`

**TypeScript SDK:**
- `feat(subscribe): Subscription.listen(timeoutMs, maxPushes) convenience method`
- `feat(memory): sourceMemoryIds: Buffer[] on Memory type`
- `feat(sdk): CONTRADICTS edge type and two-step reflect (ReflectPrepare + ReflectCommit)`

**Python SDK:**
- `feat(subscribe): Subscription.listen(timeout, max_pushes) convenience method`
- `feat(memory): source_memory_ids: list[bytes] on Memory dataclass`
- `feat(sdk): CONTRADICTS edge type and two-step reflect`

**Infrastructure:**
- `fix(docker): Dockerfile CMD corrected to serve --foreground`
- `fix(install): install.sh updated for unified hebbs binary`
- `fix(install): systemd unit updated for hebbs serve --foreground`

---

## Post-Release Verification

After all three tags are live:

```sh
# Verify Homebrew tap picks up new version
brew update
brew install hebbs-ai/tap/hebbs
hebbs version  # should show 0.3.0

# Verify install script picks up new version
HEBBS_VERSION=v0.3.0 curl -sSf https://hebbs.ai/install | sh

# Verify npm
npm info @hebbs/sdk version  # should show 0.3.0

# Verify PyPI
pip index versions hebbs     # should list 0.3.0
```
