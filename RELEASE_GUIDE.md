# HEBBS Release Instructions

How to cut a release for the unified CLI, TypeScript SDK, and Python SDK.

---

## Current Versions

| Component | Version | Repo |
|-----------|---------|------|
| Unified CLI (`hebbs`) | `0.2.0` | `hebbs-ai/hebbs` |
| TypeScript SDK (`@hebbs/sdk`) | `0.1.1` | `hebbs-ai/hebbs-typescript` |
| Python SDK (`hebbs`) | `0.1.0` | `hebbs-ai/hebbs-python` |

---

## How Releases Work

All three repos are **tag-triggered**. Push a `v*` tag and CI builds, tests, and publishes automatically.

- **Unified CLI** (`release.yml`): builds the single `hebbs` binary for linux-x86_64, linux-aarch64, macos-arm64 + Docker image. Creates GitHub Release with assets + checksums. Updates Homebrew tap. The `hebbs` binary replaces the old `hebbs-server`, `hebbs-cli`, and `hebbs-vault` binaries (see TASK-17).
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
hebbs version  # should show the new version
```

---

## Pre-Release Checklist

### 1. Sync versions across all three repos

All three should have the same version number for a coordinated release. Decide on the version (e.g., `0.2.0`) and update:

**Server** — `hebbs/Cargo.toml` (workspace root, one line):
```toml
version = "0.2.0"
```
This propagates to all 13 crates via `[workspace.package]`.

**TypeScript SDK** — `hebbs-typescript/package.json`:
```json
"version": "0.2.0"
```

**Python SDK** — `hebbs-python/pyproject.toml`:
```toml
version = "0.2.0"
```

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

Expected: TypeScript 27/27, Python 28/28.

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

## Tagging (What to Do Now)

Tag order matters: **server first**, then SDKs. The SDKs depend on the server's proto/API, not the other way around.

### Step 1 -- Tag the unified CLI

```sh
cd hebbs

# Confirm you're on main and clean
git status
git log --oneline -5

# Tag and push
git tag v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

Watch `Actions / Release` on GitHub. Wait for all three matrix builds (linux-x86_64, linux-aarch64, macos-arm64) + Docker + **Homebrew tap update** to go green before proceeding. The release now produces a single `hebbs` binary that handles both local (embedded engine) and remote (gRPC/REST client) modes.

### Step 2 — Tag the TypeScript SDK

```sh
cd hebbs-typescript
git tag v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

Watch `Actions / CI & Publish` publish job. Verify on npmjs: `https://www.npmjs.com/package/@hebbs/sdk`.

### Step 3 — Tag the Python SDK

```sh
cd hebbs-python
git tag v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

Watch `Actions / Publish to PyPI`. Verify on PyPI: `https://pypi.org/project/hebbs/`.

---

## Making a Release "Stable" (for Install Script)

The install script resolves the latest version via:
```
https://api.github.com/repos/hebbs-ai/hebbs/releases/latest
```

GitHub's "latest" release is the **most recent non-prerelease, non-draft** release. As long as you don't mark the release as a prerelease, it will automatically become "latest" and the install script will pick it up.

**To mark as stable:** When creating the release (done automatically by `release.yml` via `softprops/action-gh-release`), do NOT pass `prerelease: true`. The current workflow doesn't, so releases are stable by default.

**To pin a specific version on the install script** (optional, for testing):
```sh
HEBBS_VERSION=v0.2.0 curl -sSf https://hebbs.ai/install | sh
```

---

## Commit Messages for This Release

Per AGENTS.md conventions:

```
chore(release): bump server to v0.2.0
chore(release): bump typescript sdk to v0.2.0
chore(release): bump python sdk to v0.2.0
```

---

## What Changed Since Last Release (0.1.x to 0.2.0)

Changes made in this session that should go into the release notes:

**Unified CLI (TASK-17: One Brain, Unified Engine):**
- `feat(cli): unified hebbs binary replaces hebbs-server, hebbs-cli, and hebbs-vault`
- `feat(cli): dual-mode engine -- local (embedded RocksDB) or remote (gRPC/REST client)`
- `feat(cli): brain discovery -- --vault flag, HEBBS_VAULT env, walk-up .hebbs/, ~/.hebbs/ fallback`
- `feat(cli): all commands in one binary -- init, index, watch, rebuild, remember, recall, forget, prime, reflect, insights, inspect, export, and more`
- `feat(cli): remote mode delegates to hebbs-cli library for identical output formatting`

**Server fixes:**
- `fix(subscribe): empty kind_filter now defaults to [Episode, Insight, Revision] instead of rejecting all memories`
- `fix(reflect): global reflect now infers entity_id on insights when all source memories agree`
- `fix(prime): replaced global HNSW + entity post-filter with entity-scoped temporal index scan + cosine ranking`
- `feat(lineage): added source_memory_ids field to Memory proto (field 15) for Insight-kind memories`

**TypeScript SDK:**
- `feat(subscribe): add Subscription.listen(timeoutMs, maxPushes) convenience method`
- `feat(memory): expose sourceMemoryIds: Buffer[] on Memory type`
- `fix(test): replace hand-rolled Symbol.asyncIterator race with sub.listen()`

**Python SDK:**
- `feat(subscribe): add Subscription.listen(timeout, max_pushes) convenience method`
- `feat(memory): expose source_memory_ids: list[bytes] on Memory dataclass`
- `fix(proto): regenerated stubs with source_memory_ids field 15`

---

## Post-Release Verification

After all three tags are live:

```sh
# Verify Homebrew tap picks up new version
brew update
brew install hebbs-ai/tap/hebbs
hebbs version  # should show 0.2.0

# Verify install script picks up new version
curl -sSf https://hebbs.ai/install | sh --dry-run  # if dry-run is supported
# or just:
HEBBS_VERSION=v0.2.0 curl -sSf https://hebbs.ai/install | sh

# Verify npm
npm info @hebbs/sdk version  # should show 0.2.0

# Verify PyPI
pip index versions hebbs     # should list 0.2.0
```
