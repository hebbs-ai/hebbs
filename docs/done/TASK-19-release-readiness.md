# TASK-19: Release Readiness for v0.3.0

Target: coordinate release of all components at v0.3.0 for OpenClaw customer onboarding.

---

## Status: Released - Awaiting Verification

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

## Additional Fixes (Done)

| Fix | Files |
|-----|-------|
| Remove backward-compat symlinks | hebbs/.github/workflows/release.yml (binary + Homebrew) |
| Add protoc to nightly CI | hebbs/.github/workflows/nightly.yml (full-test + bench-standard) |
| Fix old binary refs in deploy | hebbs-deploy/README.md, runbooks/operations.md, examples/*.toml |

## Release Sequence

1. [x] Fix CI failures (fmt + dead code)
2. [x] Fix remaining issues (symlinks, protoc, deploy docs)
3. [x] Push origin main
4. [x] Subtree push all 8 repos (skip homebrew-tap, CI handles it)
5. [x] Tag hebbs v0.3.0 (triggers binary + Docker + Homebrew CI)
6. [x] Tag hebbs-typescript v0.3.0 (triggers npm publish)
7. [x] Tag hebbs-python v0.3.0 (triggers PyPI publish)
8. [ ] Post-release verification (see test plan below)

## Subtree Push Status (Completed 2026-03-15)

- hebbs -> hebbs-ai/hebbs: pushed (04d0900)
- hebbs-typescript -> hebbs-ai/hebbs-typescript: pushed (ec905a0)
- hebbs-python -> hebbs-ai/hebbs-python: pushed (72731b3)
- hebbs-website -> hebbs-ai/hebbs-website: up to date
- hebbs-blog -> hebbs-ai/hebbs-blog: up to date
- hebbs-docs -> hebbs-ai/hebbs-docs: up to date
- hebbs-deploy -> hebbs-ai/hebbs-deploy: pushed (3a64673)
- hebbs-skill -> hebbs-ai/hebbs-skill: up to date
- homebrew-tap: auto-updated by release CI

---

## Post-Release Verification Test Plan

### Phase 1: CI Green
- [ ] hebbs-ai/hebbs CI workflow passes on main
- [ ] hebbs-ai/hebbs Release workflow completes (build-binaries, build-docker, docker-smoke-test, create-release, update-homebrew)
- [ ] hebbs-ai/hebbs-typescript CI & Publish workflow passes (build-and-test + publish)
- [ ] hebbs-ai/hebbs-python Publish to PyPI workflow passes

### Phase 2: Release Artifacts
- [ ] GitHub Release exists at github.com/hebbs-ai/hebbs/releases/tag/v0.3.0
- [ ] Release has 3 tarballs: hebbs-linux-x86_64.tar.gz, hebbs-linux-aarch64.tar.gz, hebbs-macos-arm64.tar.gz
- [ ] Release has checksums.txt
- [ ] Each tarball contains `hebbs` and `hebbs-bench` binaries (no hebbs-cli, no hebbs-vault symlinks)
- [ ] Docker image exists: `docker pull ghcr.io/hebbs-ai/hebbs:0.3.0`
- [ ] Docker tags: 0.3.0, 0.3, latest, and SHA tag

### Phase 3: Install Methods
- [ ] `curl -sSf https://hebbs.ai/install | sh` installs v0.3.0
- [ ] `HEBBS_VERSION=v0.3.0 curl -sSf https://hebbs.ai/install | sh` works
- [ ] Installed binary: `hebbs --version` prints 0.3.0
- [ ] `brew tap hebbs-ai/tap && brew install hebbs` installs v0.3.0
- [ ] `brew info hebbs` shows version 0.3.0
- [ ] `npm install hebbs@0.3.0` succeeds
- [ ] `npm info hebbs version` shows 0.3.0
- [ ] `pip install hebbs==0.3.0` succeeds
- [ ] `pip show hebbs` shows version 0.3.0

### Phase 4: Version Consistency
- [ ] `hebbs --version` -> 0.3.0
- [ ] Node: `require('hebbs').VERSION` -> '0.3.0'
- [ ] Python: `import hebbs; hebbs.__version__` -> '0.3.0'
- [ ] Docker: `docker run ghcr.io/hebbs-ai/hebbs:0.3.0 --version` -> 0.3.0

### Phase 5: Docker Smoke Test
- [ ] `docker run -d -p 6381:6381 -p 50051:50051 -e HEBBS_AUTH_ENABLED=false ghcr.io/hebbs-ai/hebbs:0.3.0`
- [ ] `curl http://localhost:6381/v1/health/live` returns OK
- [ ] `curl http://localhost:6381/v1/health/ready` returns OK
- [ ] `curl http://localhost:6381/v1/metrics` returns Prometheus metrics
- [ ] Container runs CMD `serve --foreground` (no old `start` subcommand)

### Phase 6: Local E2E (macOS)
- [ ] `hebbs init ~/test-vault` creates .hebbs/ directory
- [ ] `hebbs remember "test memory" --vault ~/test-vault` stores successfully
- [ ] `hebbs recall "test" --vault ~/test-vault` returns the memory
- [ ] `hebbs panel --vault ~/test-vault` opens Memory Palace on localhost
- [ ] `hebbs status --vault ~/test-vault` shows engine health
- [ ] Clean up: `rm -rf ~/test-vault`

### Phase 7: SDK E2E (against Docker server)
- [ ] TypeScript: connect to localhost:50051, remember + recall round-trip
- [ ] TypeScript: subscription.listen() receives push
- [ ] TypeScript: sourceMemoryIds populated on Insight memories
- [ ] Python: connect to localhost:50051, remember + recall round-trip
- [ ] Python: subscription.listen() receives push
- [ ] Python: source_memory_ids populated on Insight memories
- [ ] Both SDKs: CONTRADICTS edge type works with two-step reflect

### Phase 8: Cleanup
- [ ] Stop and remove Docker container
- [ ] Remove test vault directory
- [ ] Verify no leftover processes (`ps aux | grep hebbs`)
