# TASK-19: Release Readiness for v0.3.0

Target: coordinate release of all components at v0.3.0 for OpenClaw customer onboarding.

---

## Status: In Progress

## What Changed (0.2.0 -> 0.3.0)

**Vault system (new):**
- File-first markdown sync with daemon mode
- Multi-vault daemon with auto-start, proactive vault opening, file watchers
- Contradiction detection (heuristic + LLM classification)
- Memory Palace control panel (force-directed graph, search, sliders, timeline, config editor)
- Query audit log and stats API
- Vault lifecycle (init, index, watch, rebuild, status)

**Unified CLI:**
- Single `hebbs` binary replaces `hebbs-server`, `hebbs-cli`, and `hebbs-vault`
- Dual-mode engine (local embedded RocksDB or remote gRPC/REST client)
- Brain discovery (--vault flag, HEBBS_VAULT env, walk-up .hebbs/, ~/.hebbs/ fallback)
- Queries subcommand for query audit log

**Server fixes:**
- Empty kind_filter defaults to [Episode, Insight, Revision] instead of rejecting all
- Global reflect infers entity_id on insights when all source memories agree
- Entity-scoped temporal index scan + cosine ranking replaces global HNSW + post-filter
- source_memory_ids field (proto field 15) for Insight-kind memories

**TypeScript SDK (0.1.2 -> 0.3.0):**
- Subscription.listen(timeoutMs, maxPushes) convenience method
- sourceMemoryIds: Buffer[] on Memory type
- CONTRADICTS edge type and two-step reflect (ReflectPrepare + ReflectCommit)

**Python SDK (0.1.2 -> 0.3.0):**
- Subscription.listen(timeout, max_pushes) convenience method
- source_memory_ids: list[bytes] on Memory dataclass
- CONTRADICTS edge type and two-step reflect

---

## Release Infrastructure Fixes (Done)

| Fix | Files |
|-----|-------|
| Version bump to 0.3.0 | Cargo.toml, package.json, index.ts, pyproject.toml, __init__.py, cli.py (both demos) |
| install.sh unified binary | hebbs-deploy/scripts/install.sh (binary names, usage, systemd) |
| Dockerfile fix | hebbs/docker/Dockerfile (CMD serve --foreground, removed broken HEALTHCHECK) |
| systemd unit fix | hebbs-deploy/systemd/hebbs-server.service (ExecStart, service name) |
| RELEASE_GUIDE.md update | Version refs, changelog, version bump locations list |

## CI Fixes (Done)

| Fix | Root Cause |
|-----|------------|
| cargo fmt | Rust 1.94 reformatted 25 files (new stable rustfmt rules) |
| Dead code warning | `OpenVault.vault_root` field never read in vault_manager.rs |

## Remaining: Release Sequence

1. [x] Fix CI failures (fmt + dead code)
2. [ ] Push origin main
3. [ ] Subtree push all 8 repos (skip homebrew-tap, CI handles it)
4. [ ] Tag hebbs v0.3.0 -> wait for CI (binary + Docker + Homebrew)
5. [ ] Tag hebbs-typescript v0.3.0 -> wait for npm
6. [ ] Tag hebbs-python v0.3.0 -> wait for PyPI
7. [ ] Post-release verification (brew, install script, npm, pip)

## Subtree Push Status

All 9 subtrees have unpushed changes:
- hebbs: 51+ files, +16,879 lines
- hebbs-typescript: 10 files, +303 lines
- hebbs-python: 11 files, +467 lines
- hebbs-website: 13 files, +1,171 lines
- hebbs-blog: 3 files, +215 lines
- hebbs-docs: 15 files, +1,040 lines
- hebbs-deploy: 3 files, +27 lines
- hebbs-skill: 1 file, rewritten
- homebrew-tap: 1 file (auto-updated by release CI)

## Known Remaining Issues (Not Blocking Release)

- hebbs-deploy/README.md and runbooks still reference old binary names (hebbs-server, hebbs-cli)
- release.yml still creates backward-compat symlinks (hebbs-cli, hebbs-vault) marked for removal in v0.3.0
- Nightly workflow missing protoc install
