# Phase 12: Testing and Benchmark Suite -- Architecture Blueprint

## Status: COMPLETE

---

## Intent

Phase 12 exists because confidence is the precondition for everything that follows. Phase 13 (Production Hardening) demands proof that the engine does not corrupt data under adversarial conditions. Phase 17 (Edge Sync) demands proof that latency budgets hold at 10M memories with constrained hardware. Phase 15 (Deployment) demands CI artifacts -- release binaries, regression gates, reproducible benchmarks. Without Phase 12, every downstream phase is building on hope instead of evidence.

Phases 1 through 11 built the engine incrementally, with each phase carrying its own unit tests, integration tests, property tests, and Criterion microbenchmarks. The workspace currently holds 750+ tests distributed across 11 crates. What is missing is the *cross-cutting* layer: tests that exercise the system as an indivisible whole (not crate-by-crate), benchmarks that measure behavior at production scale (not 1K-record toy datasets), and infrastructure that enforces non-regression on every change (not "run it manually and eyeball the numbers").

This phase produces three artifacts: a workspace-level integration test suite, the `hebbs-bench` CLI binary, and a CI pipeline. Each serves a different audience: integration tests serve the developer (fast feedback on correctness), benchmarks serve the operator (quantifiable proof of performance), and CI serves the project (automated quality gates that never sleep).

The decisions made here -- what to measure, how to measure it, what thresholds to enforce, what the benchmark harness looks like -- become the credibility foundation of HEBBS. A benchmark suite that is hard to reproduce, easy to game, or silent about failure modes is worse than no benchmark suite at all. The bar is BenchmarksAndProofs.md, and the bar is non-negotiable.

---

## Scope Boundaries

### What Phase 12 delivers

- `hebbs-bench` crate: a standalone CLI binary producing latency, scalability, resource, and cognitive benchmark reports
- Workspace-level integration test suite in `tests/` exercising cross-crate scenarios that no single crate's tests can cover
- Crash recovery test harness: fault injection during writes, verifying data integrity after restart
- Scale validation: correctness and latency verified at 100K and 1M memory counts (addressing deferred items from Phases 3, 4, and 5)
- GitHub Actions CI pipeline: test, lint, benchmark regression gate, release binary builds
- Property-based tests for `hebbs-server` (deferred from Phase 8)
- Criterion benchmarks for `hebbs-server` gRPC and REST paths (deferred from Phase 8)
- Decay cursor crash recovery integration test (deferred from Phase 5)
- Labeled test dataset for cognitive benchmark evaluation (multi-path recall precision measurement)
- JSON + human-readable benchmark report format consumable by CI and documentation

### What Phase 12 explicitly does NOT deliver

- 72-hour soak test execution (Phase 13 -- requires production hardening first; Phase 12 builds the *harness*, Phase 13 *runs* it)
- PyPI wheel publishing to the public registry (Phase 12 builds the CI workflow; actual publishing is gated on the team's release decision)
- SIMD-optimized distance computation (Phase 12 *profiles* to determine if it is needed; implementation is a separate optimization task post-profiling)
- Snapshot retention limits (deferred from Phase 5 -- this is a feature, not a test; tracked for Phase 13)
- Security audit or penetration testing (Phase 13)
- Chaos testing at production scale (Phase 13 -- Phase 12 provides the crash recovery *primitive*, Phase 13 runs it 1,000 times)
- Competitive benchmarks against Qdrant/pgvector/Redis (Phase 16 Documentation -- requires controlled environment and reproducible methodology)

These boundaries exist because Phase 12 is an *infrastructure* phase, not a *feature* phase. The temptation is to start fixing things the benchmarks reveal. Resist. Phase 12's job is to build the measurement apparatus and establish the evidence base. Fixes belong in Phase 13 or are tracked as optimization work.

---

## Architectural Decisions

### 1. Integration Test Architecture: Layered Test Pyramid

The workspace already has ~750 tests, but they are stratified by crate. A `hebbs-core` integration test exercises the engine with a `MockEmbedder` and in-memory storage. A `hebbs-server` integration test exercises a gRPC endpoint. Neither exercises the *deployed system path*: binary startup → config loading → RocksDB initialization → ONNX model load → gRPC accept → engine call → response serialization → client deserialization.

Phase 12 adds the fourth test layer: **system tests**.

| Layer | Location | What it proves | Runs in CI | Speed |
|-------|----------|---------------|------------|-------|
| Unit tests | `src/**/*.rs` (per crate) | Individual function correctness | Every PR | Seconds |
| Property tests | `tests/property_tests.rs` (per crate) | Invariant preservation over random inputs | Every PR | Seconds |
| Crate integration tests | `tests/integration_tests.rs` (per crate) | Cross-module correctness within a crate | Every PR | Seconds–minutes |
| **System tests** (Phase 12) | `tests/` (workspace root) | End-to-end correctness across crate boundaries, through real I/O, at realistic scale | Every PR (subset), nightly (full) | Minutes |

System tests use the *real* RocksDB backend, the *real* `MockEmbedder` (not in-memory storage), and exercise the full `Engine` pipeline including index updates, decay, and reflection. A subset also starts `hebbs-server` as a child process and exercises it through the `hebbs-client` SDK, validating the entire network path.

**Why MockEmbedder, not OnnxEmbedder, for system tests:** OnnxEmbedder requires a 33MB model download and ~3ms per embedding. System tests at 100K memories would spend 5 minutes on embedding alone. MockEmbedder produces deterministic 384-dim vectors from content hashes. This preserves the semantic property that different content produces different embeddings while keeping test runtime bounded. OnnxEmbedder-specific tests exist in `hebbs-embed` and are sufficient for embedding correctness. The system tests validate *system behavior*, not embedding quality.

### 2. System Test Categories

Six categories, each targeting a distinct failure class that crate-level tests cannot catch.

**Category A: Full Lifecycle Tests**

Exercise the complete operational sequence: `remember` → `recall` (all four strategies) → `revise` → `recall` (verify update) → `reflect` → `insights` → `forget` → `recall` (verify removal). These tests are the system's heartbeat. If any lifecycle test fails, something fundamental is broken.

Variant: lifecycle with `subscribe` active throughout, verifying that subscription pushes arrive at correct points during the lifecycle.

**Category B: Concurrency Tests**

100 parallel threads performing mixed operations: 50 remembering, 25 recalling, 10 revising, 10 forgetting, 5 subscribing. Verify: no panics, no deadlocks (timeout-based detection), no data corruption (every remembered memory is retrievable, every forgotten memory is gone, every revised memory reflects its latest content). Run duration: configurable, default 10 seconds.

These tests catch lock-ordering bugs, WriteBatch atomicity violations under contention, and HNSW graph corruption during concurrent insert+delete.

**Category C: Crash Recovery Tests**

The hardest tests to get right and the most valuable. Approach:

Fork a child process that performs a write-heavy workload. At a random point, send SIGKILL (not SIGTERM -- no graceful shutdown). Restart a new engine instance on the same data directory. Verify:
- The database opens without error
- Every memory that was acknowledged (returned from `remember()`) is retrievable
- No partial index states: if a memory is in the default CF, it must also be in temporal, vector, and graph CFs
- HNSW rebuild from the vectors CF produces a searchable graph
- Decay cursor in meta CF is valid (deferred from Phase 5)

Repeat for three crash scenarios: crash during `remember()`, crash during `forget()` (mid-batch), crash during decay sweep.

**Category D: Scale Validation Tests**

Verify correctness and latency at 100K and 1M memory counts. These are *not* benchmarks (that is `hebbs-bench`). They are correctness checks: recall@10 > 85% at 100K (deferred from Phase 3), temporal ordering correct at 1M, graph traversal bounded at 100K with dense edge graphs.

Marked `#[ignore]` for normal CI runs. Executed in nightly CI or explicitly via `cargo test --ignored`.

**Category E: Server Round-Trip Tests**

Start `hebbs-server` as a subprocess. Connect via `hebbs-client`. Exercise all nine operations end-to-end through the network layer. Verify: protobuf serialization round-trips preserve all fields, gRPC status codes are correct for error cases, health check reflects actual engine state.

Distinct from `hebbs-server`'s own integration tests which use in-process test servers. Category E validates the actual binary, actual port binding, actual config loading.

**Category F: Edge Case Tests**

The collection of degenerate inputs and boundary conditions that span multiple crates:
- Recall on empty database (every strategy, including multi-strategy)
- Forget non-existent memory (must be no-op, not error)
- Revise non-existent memory (must be NotFound)
- Remember with maximum content length (64KB), verify recall works
- Remember with empty content (must be rejected)
- 1000 concurrent `subscribe` attempts against the 100-subscription limit
- Reflect with fewer than the minimum cluster threshold memories
- Recall with `top_k = 0`, `top_k = 1`, `top_k = 1000`
- Graph traversal at `max_depth = 0` (seed only), `max_depth = 10`

### 3. The `hebbs-bench` CLI: Design Philosophy

`hebbs-bench` is a *public benchmark tool*, not an internal development utility. Third parties will use it to reproduce every number in BenchmarksAndProofs.md. This shapes every design decision.

**Principles:**
- **Reproducible:** Same hardware + same dataset + same parameters = same numbers (within statistical noise). Seed all randomness. Pin dataset generation.
- **Self-contained:** One binary, one command. No external scripts, no pre-generated datasets, no "first run this Python script." Dataset generation is built into the tool.
- **Honest:** Report p50, p95, p99, p999, min, max, and standard deviation. Never report only averages. Include system context in the report: CPU model, core count, RAM, OS, Rust version, HEBBS version (git SHA).
- **Comparable:** Output a machine-parseable JSON report alongside the human-readable summary. CI uses the JSON report for regression detection. The documentation site renders it.
- **Progressive:** Benchmarks run in tiers. Tier 1 (quick, ~2 minutes) runs in CI on every PR. Tier 2 (standard, ~15 minutes) runs nightly. Tier 3 (full, ~2 hours) runs before releases.

**The binary interface:**

`hebbs-bench` accepts a subcommand for each benchmark category, plus `all` to run everything at a given tier.

Subcommands: `latency`, `scalability`, `resources`, `cognitive`, `all`.

Flags: `--tier <quick|standard|full>`, `--scale <N>` (override memory count), `--output <path.json>`, `--baseline <path.json>` (compare against previous run), `--data-dir <path>` (scratch directory), `--threads <N>` (concurrency level for throughput tests).

### 4. Benchmark Categories and Metrics

**4a: Latency Benchmarks**

Measure p50/p95/p99/p999 for each hot-path operation at a fixed scale. The scale varies by tier:

| Tier | Memory Count | Runs per Operation | Warmup Runs |
|------|-------------|-------------------|-------------|
| Quick | 10K | 1,000 | 100 |
| Standard | 100K | 10,000 | 1,000 |
| Full | 1M | 100,000 | 10,000 |

Operations measured:
- `remember` (single, with 200B content + structured context)
- `remember` (batch, 100 memories per batch)
- `recall` similarity (top-10)
- `recall` temporal (last 100, single entity)
- `recall` causal (depth 3, moderate edge density)
- `recall` analogical (top-10, re-ranked)
- `recall` multi-strategy (similarity + temporal)
- `prime` (single entity, 50 memories)
- `revise` (content update, triggers re-embedding and re-indexing)
- `forget` (single ID)
- `forget` (by entity, ~100 memories)
- `subscribe` pipeline (text chunk → push latency, end-to-end)

Each operation is measured in isolation (dedicated phase with controlled background load) and under contention (mixed workload running concurrently).

Target values are from GuidingPrinciples.md, Section 1, Latency Budgets. These are hard contracts.

**4b: Scalability Benchmarks**

Measure how latency changes as memory count grows. Populate the database to each scale point, then measure recall latency.

| Scale | `recall` similarity p99 target | `recall` temporal p99 target |
|-------|-------------------------------|------------------------------|
| 100K | 3ms | 0.6ms |
| 1M | 5ms | 0.8ms |
| 10M | 8ms | 1.2ms |

10M is Tier 3 only (requires ~10GB RAM, ~40GB disk, ~30 minutes to populate).

The key insight: this benchmark validates the *logarithmic scaling* claim. HNSW search complexity is O(log N) with respect to graph size. If latency scales linearly, the HNSW implementation has a bug (likely in layer selection or neighbor pruning). The benchmark should plot the latency curve and flag any deviation from logarithmic growth.

**4c: Resource Benchmarks**

Measure RAM and disk consumption at each scale point. Compare against BenchmarksAndProofs.md targets:

| Metric | Target |
|--------|--------|
| RAM per 10M memories | < 5GB |
| Disk per 10M memories | < 20GB |
| Average bytes per memory (on disk) | < 2KB |

Methodology: measure RSS (Resident Set Size) via `/proc/self/statm` on Linux, `mach_task_basic_info` on macOS. Measure disk via directory size of the RocksDB data directory after compaction.

**4d: Cognitive Benchmarks**

This is what separates HEBBS benchmarks from a generic database benchmark suite. Cognitive benchmarks measure whether the memory model -- multi-path recall, reflection, decay -- actually improves retrieval quality over similarity-only search.

**Labeled test dataset:** A curated dataset of ~1,000 memories with ground-truth annotations. Each memory is tagged with:
- Correct temporal predecessors/successors (for temporal recall ground truth)
- Correct causal relationships (for causal recall ground truth)
- Cross-domain analogies (for analogical recall ground truth)
- Expected clusters and insight summaries (for reflection ground truth)

The dataset is deterministic (generated from a seed, not hand-curated) to ensure reproducibility. It models three domains: a sales conversation history, a technical troubleshooting log, and a project management timeline. Each domain has 300+ memories with realistic temporal structure, causal chains, and cross-domain parallels.

Cognitive metrics measured:
- **Multi-path recall precision vs. similarity-only:** For each ground-truth query, compare precision@10 using similarity-only vs. the appropriate specialized strategy vs. multi-strategy. Target deltas from BenchmarksAndProofs.md: temporal +68%, causal +63%, analogical +43%.
- **Reflection compression ratio:** Run reflect on each domain. Measure episodes-to-insights ratio. Target: 1,000 episodes → 30-50 insights.
- **Decay impact:** Populate 1M memories. Measure recall precision with and without decay. Target: +23% precision improvement with decay active (BenchmarksAndProofs.md: 61% → 84%).

Cognitive benchmarks are Tier 2 and Tier 3 only. They require a MockLlmProvider for deterministic reflection output.

### 5. Benchmark Report Format

Every `hebbs-bench` run produces two outputs:

**Human-readable summary** (stdout): A formatted table matching the layout of BenchmarksAndProofs.md. Color-coded: green for meeting target, yellow for within 10% of target, red for missing target.

**Machine-readable report** (JSON file): Contains every measurement, system metadata, and comparison deltas if a baseline was provided.

Report structure:

```
{
  "version": "0.1.0",
  "git_sha": "abc123",
  "timestamp": "2026-03-03T...",
  "system": {
    "cpu": "Apple M3 Max",
    "cores": 14,
    "ram_gb": 36,
    "os": "macOS 15.3",
    "rust_version": "1.82.0"
  },
  "tier": "standard",
  "results": {
    "latency": { ... per-operation p50/p95/p99/p999/min/max/stddev ... },
    "scalability": { ... per-scale-point latencies ... },
    "resources": { ... RAM and disk at each scale ... },
    "cognitive": { ... precision metrics with/without each strategy ... }
  },
  "comparison": {
    "baseline_sha": "def456",
    "regressions": [ ... operations where p99 regressed >10% ... ],
    "improvements": [ ... operations where p99 improved >10% ... ]
  }
}
```

### 6. CI Pipeline Architecture

The CI pipeline runs on GitHub Actions. Three workflow files, triggered by different events.

**Workflow 1: `ci.yml` -- On Every PR and Push to Main**

Jobs (in dependency order where noted, otherwise parallel):

| Job | What | Timeout | Runs On |
|-----|------|---------|---------|
| `check` | `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings` | 10 min | ubuntu-latest |
| `test` | `cargo test --workspace` (all unit, property, crate integration tests) | 20 min | ubuntu-latest |
| `test-python` | `maturin develop --release` + `pytest crates/hebbs-python/tests/` | 15 min | ubuntu-latest |
| `bench-gate` | Run `hebbs-bench latency --tier quick`, compare against committed baseline, fail if any p99 regresses >10% | 15 min | ubuntu-latest (dedicated runner for consistent numbers) |
| `audit` | `cargo audit` (security advisory check) | 5 min | ubuntu-latest |

`bench-gate` is the critical innovation. It turns performance into a first-class CI signal. The baseline JSON file is committed to the repository (at `benches/baseline.json`). When `bench-gate` runs, it executes `hebbs-bench latency --tier quick --baseline benches/baseline.json` and exits non-zero if any regression exceeds the threshold. New baselines are committed when performance intentionally changes (architecture change, algorithm swap) with an explanatory commit message.

**Workflow 2: `nightly.yml` -- Scheduled, Once Per Day on Main**

| Job | What | Timeout |
|-----|------|---------|
| `full-test` | `cargo test --workspace` + `cargo test --workspace --ignored` (includes scale validation and crash recovery) | 60 min |
| `bench-standard` | `hebbs-bench all --tier standard` | 30 min |
| `bench-python` | Python SDK benchmarks (embedded mode latency) | 15 min |

Nightly results are uploaded as CI artifacts. A trend dashboard (Phase 15 Grafana or a simple static page) renders latency over time to detect gradual regressions that stay under the 10% threshold individually but accumulate.

**Workflow 3: `release.yml` -- On Git Tag Push**

| Job | What | Timeout |
|-----|------|---------|
| `bench-full` | `hebbs-bench all --tier full` | 3 hours |
| `build-linux-x86_64` | Cross-compile `hebbs-server` and `hebbs-cli` for `x86_64-unknown-linux-musl` (static binary) | 30 min |
| `build-linux-aarch64` | Cross-compile for `aarch64-unknown-linux-musl` | 30 min |
| `build-macos-arm64` | Native build for `aarch64-apple-darwin` | 20 min |
| `build-python-wheels` | `maturin build --release` for Linux x86_64, Linux aarch64, macOS arm64 | 30 min |
| `create-release` | Upload binaries and benchmark report to GitHub Release | 5 min |

Release binaries are statically linked (musl on Linux) so that the "unpack and run" promise from Principle 2 is honored.

### 7. Benchmark Harness Implementation Strategy

`hebbs-bench` is a standalone binary crate, not a library. It depends on `hebbs-core`, `hebbs-embed` (for `MockEmbedder`), and `hebbs-storage` (for `RocksDbStorage`). It does *not* depend on `hebbs-server`, `hebbs-client`, or `hebbs-proto` -- it exercises the engine directly, not through the network. Network-path benchmarks are a separate concern for Phase 13 soak testing.

**Dataset generation:** `hebbs-bench` generates its own test data deterministically from a seed. Each memory has realistic content (template-generated sentences with variable entity names, timestamps, and domain terms), structured context (3-5 key-value pairs), and importance scores distributed across the [0.1, 1.0] range. Edge relationships follow a power-law distribution: most memories have 0-2 edges, a few have 10+. This models real agent behavior.

**Measurement methodology:** Each operation is measured using `std::time::Instant` (monotonic clock). Warmup runs are discarded. Measurement runs are collected into a sorted vector. Percentiles are computed by index: p50 = values[n/2], p99 = values[n * 99 / 100]. No external statistics library needed for percentile computation.

**Isolation:** Each benchmark category gets a fresh RocksDB instance in a temporary directory. No shared state between benchmarks. The temp directory is cleaned up after each benchmark unless `--keep-data` is specified.

**Memory measurement:** RSS is sampled before and after population, and before and after benchmark runs. The delta during population gives the per-memory overhead. The delta during benchmark runs reveals any memory leaks in the hot path.

### 8. Addressing Deferred Items from Phases 1-11

Phase 12 is the designated home for several items explicitly deferred during earlier phases. Each is addressed as a specific deliverable.

| Deferred Item | Origin Phase | Phase 12 Resolution |
|---------------|-------------|-------------------|
| Recall@10 at 100K and 1M scale | Phase 3 | Scale validation test (Category D) + `hebbs-bench scalability` |
| Recall latency at 100K and 1M | Phase 4 | `hebbs-bench latency --tier standard/full` |
| Decay cursor crash recovery test | Phase 5 | Crash recovery test (Category C) -- kill during decay sweep, verify cursor integrity |
| Criterion benchmarks for `hebbs-server` | Phase 8 | New Criterion benchmark group in `hebbs-server/benches/` covering gRPC and REST round-trip latency |
| Property-based tests for `hebbs-server` | Phase 8 | Proptest generators for protobuf request types, round-trip invariants |
| PyPI CI wheel builds | Phase 11 | `release.yml` workflow -- `maturin build` for target platforms |

Items explicitly *not* resolved in Phase 12 (tracked for later):
- SIMD distance computation: Phase 12 profiles to determine if it matters. If `hebbs-bench` shows distance computation is >20% of recall latency, file it as a Phase 13 optimization task. If it is <20%, it is not worth the `unsafe` surface area.
- Snapshot retention limits: Feature work, not testing. Tracked for Phase 13.

### 9. Criterion Benchmark Completion

Phases 1 through 7 each added Criterion benchmarks for their operations. Phase 8 deferred its benchmarks to Phase 12. The remaining Criterion benchmark gaps:

| Crate | Existing Benchmarks | Phase 12 Additions |
|-------|-------------------|-------------------|
| `hebbs-server` | None | gRPC round-trip (remember, recall, forget), REST round-trip (remember, recall, forget), config parsing, protobuf ser/de |
| `hebbs-client` | None | Client-side latency overhead (connection setup, request building, response parsing) |
| `hebbs-ffi` | None | FFI call overhead (hebbs_remember, hebbs_recall, hebbs_close), JSON parse/serialize at FFI boundary |
| `hebbs-python` | None | PyO3 bridge overhead (Python→Rust→Python round-trip), deferred to Python-specific benchmark script outside Criterion |

Criterion benchmarks are microbenchmarks. They measure the *cost of a code path*, not the *behavior of the system*. `hebbs-bench` measures system behavior. Both are needed; they answer different questions.

### 10. Test Data Management

System tests and benchmarks need test data. Three approaches, used for different purposes:

**Approach A: Generated in-process (system tests, benchmarks).** A deterministic data generator produces `RememberInput` structs from a seed. No files on disk. No checked-in test fixtures. The generator is a shared utility in a `test-support` module (not a published crate -- it lives in `tests/support/` or as a `[dev-dependency]` internal crate).

**Approach B: Golden files (property tests, serialization compatibility).** Serialized `Memory` structs from each phase are checked into `tests/golden/`. Deserialization tests verify that the current code can still read data written by previous phases. This catches schema evolution regressions.

**Approach C: Labeled dataset (cognitive benchmarks).** A JSON file checked into `benches/datasets/` containing the curated 1,000-memory dataset with ground-truth annotations. Generated once by a script (also checked in), reproducible from a seed. Updated only when the cognitive benchmark methodology changes.

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| Benchmark numbers vary across CI runner hardware, causing false regressions | High -- developers lose trust in the gate, start ignoring it | Pin CI benchmarks to a specific runner type (self-hosted or dedicated GitHub Actions runner class). Use relative regression (% change from baseline) not absolute thresholds. Baseline is captured on the same runner class. Allow 5% noise margin before flagging. |
| Crash recovery tests are flaky due to timing sensitivity in SIGKILL delivery | Medium -- intermittent CI failures | Use a deterministic write count (not time-based) before kill. Fork child with a pipe; parent reads "ready" from child, then kills. No races. Retry the test once on failure before marking it as a true failure. |
| `hebbs-bench` at Tier 3 (1M+ memories) requires more RAM/disk than CI runners provide | Medium -- full benchmarks cannot run in CI | Tier 3 runs only on release workflow with a larger runner (self-hosted or GitHub Actions `large` runner). Tier 1 and Tier 2 are sized for standard 7GB RAM runners. Document minimum hardware for Tier 3 in the README. |
| Cognitive benchmark precision numbers depend on MockEmbedder's hash-based vectors, which do not model true semantic similarity | High -- published precision numbers may not reflect real-world performance | Document clearly that cognitive benchmarks measure *strategy effectiveness* (temporal vs. similarity, causal vs. similarity), not absolute precision. The relative delta is valid regardless of embedding quality. Absolute precision requires OnnxEmbedder and is measured separately in a manual evaluation process. |
| Baseline JSON file in the repository creates merge conflicts when multiple PRs run benchmarks | Low -- annoying but not blocking | Baseline is updated only on main branch by a dedicated CI step after merge. PRs compare against the committed baseline but do not update it. A bot or CI step updates the baseline after merge if the new numbers are intentionally different. |
| RocksDB compilation in CI is slow (~5 minutes), bloating CI times | Medium -- slow feedback loop | Cache the RocksDB build artifact using GitHub Actions cache with a key based on `rust-rocksdb` version + compiler version. Expect ~10x speedup on cache hits. Use `sccache` for broader Rust build caching. |
| Python wheel builds across platforms require cross-compilation toolchains | Medium -- complex CI setup | Use `maturin`'s official GitHub Action (`PyO3/maturin-action`) which handles manylinux containers and cross-platform builds. Follow the pattern used by `pydantic-core`, `polars`, and `tiktoken` for multi-platform wheel CI. |

---

## Deliverables Checklist

Phase 12 is done when ALL of the following are true:

### `hebbs-bench` CLI

- [ ] `hebbs-bench` crate exists in `crates/hebbs-bench/` as a workspace member
- [ ] `hebbs-bench latency` measures all hot-path operations at configurable scale
- [ ] `hebbs-bench scalability` measures recall latency at 100K, 1M, and 10M scale points
- [ ] `hebbs-bench resources` measures RAM and disk consumption at each scale point
- [ ] `hebbs-bench cognitive` measures multi-path recall precision against labeled dataset
- [ ] `hebbs-bench all --tier quick` completes in under 3 minutes on Apple Silicon
- [ ] `hebbs-bench all --tier standard` completes in under 20 minutes
- [ ] `--output <path.json>` produces a valid JSON report with system metadata, all measurements, and percentile breakdowns
- [ ] `--baseline <path.json>` compares against a previous run and reports regressions/improvements with percentage deltas
- [ ] All randomness is seeded and deterministic given the same seed
- [ ] Human-readable stdout output matches BenchmarksAndProofs.md table format

### System Integration Tests

- [ ] Category A: Full lifecycle test passes (remember → recall all strategies → revise → reflect → insights → forget)
- [ ] Category A: Lifecycle with active subscription passes
- [ ] Category B: 100-thread concurrent mixed workload runs for 10 seconds with zero corruption
- [ ] Category C: Crash during `remember()` -- data integrity verified after restart
- [ ] Category C: Crash during `forget()` -- data integrity verified after restart
- [ ] Category C: Crash during decay sweep -- cursor integrity verified after restart (Phase 5 deferred item)
- [ ] Category D: Recall@10 > 85% at 100K memories (Phase 3 deferred item)
- [ ] Category D: Recall@10 > 85% at 1M memories (Phase 3 deferred item, `#[ignore]`)
- [ ] Category D: Temporal ordering correct at 1M memories (`#[ignore]`)
- [ ] Category E: Server round-trip tests pass for all nine operations via `hebbs-client`
- [ ] Category E: Server health check reflects actual engine state
- [ ] Category F: All edge case tests pass (empty DB, non-existent targets, boundary inputs, subscription limit)
- [ ] No test uses `sleep` for synchronization (use condition variables, channels, or retry loops with timeout)

### Deferred Item Resolution

- [ ] Criterion benchmarks added for `hebbs-server` gRPC and REST paths
- [ ] Property-based tests added for `hebbs-server` protobuf round-trips
- [ ] Decay cursor crash recovery integration test passes
- [ ] Scale validation tests at 100K confirm recall@10 > 85%
- [ ] `release.yml` workflow builds Python wheels for Linux x86_64, Linux aarch64, macOS arm64

### CI Pipeline

- [ ] `ci.yml` runs on every PR: fmt check, clippy, workspace tests, Python tests, bench gate, cargo audit
- [ ] `bench-gate` job fails the PR if any p99 latency regresses >10% from committed baseline
- [ ] `nightly.yml` runs `#[ignore]` tests and standard-tier benchmarks
- [ ] `release.yml` builds static Linux binaries (musl), macOS binary, Python wheels, and uploads to GitHub Release
- [ ] `benches/baseline.json` is committed and maintained
- [ ] RocksDB build is cached in CI (cache hit reduces build time by >50%)
- [ ] CI workflow total time for `ci.yml` is under 20 minutes on cache hit

### Code Quality

- [ ] Zero clippy warnings across the workspace (including new test and benchmark code)
- [ ] No `unwrap()` or `expect()` in `hebbs-bench` on any path that could fail due to user input or system state
- [ ] `hebbs-bench --help` documents every subcommand, flag, and tier
- [ ] `cargo audit` passes with no unaddressed advisories

### Golden File and Dataset

- [ ] Serialization golden files checked in for Memory structs from each schema version
- [ ] Cognitive benchmark labeled dataset checked in at `benches/datasets/cognitive_benchmark.json`
- [ ] Dataset generation script checked in and reproducible from seed

---

## Interfaces Published to Future Phases

Phase 12 creates infrastructure that later phases depend on. These interfaces are stable after Phase 12.

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| `hebbs-bench` CLI interface (subcommands, flags, exit codes) | 13 (soak test harness), 15 (deployment validation), 16 (documentation benchmarks) | Additive only (new subcommands/flags, never remove or rename) |
| JSON report schema | 13 (soak test analysis), 15 (Grafana dashboards), 16 (docs site rendering) | Versioned, backward-compatible (new fields, never remove) |
| CI workflow structure (`ci.yml`, `nightly.yml`, `release.yml`) | 13 (adds soak test job), 17 (adds edge-mode build targets), 18/19 (adds SDK CI) | Extensible (new jobs, never remove existing gates) |
| `benches/baseline.json` format and location | All future phases (every PR is gated on it) | Schema-versioned, tooling reads it |
| System test harness and `tests/support/` utilities | 13 (chaos tests reuse crash recovery harness), 17 (sync tests reuse concurrency harness) | Internal API, but crash recovery and data generation utilities are reused directly |
| Release binary artifact names and structure | 15 (Docker, Helm reference the binary names), 16 (docs link to releases) | Stable naming convention once established |
| Labeled cognitive dataset format | 16 (documentation cites benchmark numbers from this dataset) | Versioned, changes require re-running benchmarks |
