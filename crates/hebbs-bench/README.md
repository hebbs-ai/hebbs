# hebbs-bench

Standalone benchmark suite for the HEBBS cognitive memory engine. Measures latency, scalability, and resource consumption across all hot-path operations.

**Benchmarks must be run on dedicated hardware** — not in CI. Shared CI runners produce noisy, unreproducible results that cannot be meaningfully compared across runs.

## Building

```bash
cargo build -p hebbs-bench --release
```

The binary is placed at `target/release/hebbs-bench`.

## Tiers

Each benchmark supports three tiers that control dataset size and iteration count:

| Tier       | Memories   | Runs/Op  | Warmup | Typical Duration |
|------------|-----------|----------|--------|------------------|
| `quick`    | 10,000    | 1,000    | 100    | ~1 minute        |
| `standard` | 100,000   | 10,000   | 1,000  | ~30 minutes      |
| `full`     | 1,000,000 | 100,000  | 10,000 | ~2+ hours        |

Use `quick` for development iteration and smoke checks. Use `standard` or `full` for release-quality measurements on dedicated hardware.

## Benchmark Categories

### Latency

Measures p50/p95/p99/p999 latency for each operation: `remember`, `get`, `recall_similarity`, `recall_temporal`, `prime`, `revise`, `forget_single`, `count`.

```bash
hebbs-bench latency --tier quick
hebbs-bench latency --tier standard --output latency.json
```

### Scalability

Measures recall latency (similarity + temporal) at increasing memory counts to verify that performance degrades gracefully.

| Tier       | Scale Points                      |
|------------|-----------------------------------|
| `quick`    | 1K, 5K, 10K                      |
| `standard` | 1K, 10K, 50K, 100K               |
| `full`     | 1K, 10K, 100K, 500K, 1M          |

```bash
hebbs-bench scalability --tier standard --output scalability.json
```

### Resources

Measures disk usage, RSS, and bytes-per-memory at each scale point.

```bash
hebbs-bench resources --tier standard --output resources.json
```

### All

Runs latency, scalability, and resources sequentially.

```bash
hebbs-bench all --tier standard --output results.json
```

## Comparing Against a Baseline

Save a previous run as a baseline, then compare:

```bash
# Generate baseline
hebbs-bench latency --tier standard --output baseline.json

# After changes, compare
hebbs-bench latency --tier standard --output current.json --baseline baseline.json
```

The comparison table flags any operation with a p99 regression >10%.

## Options

| Flag           | Description                                    |
|----------------|------------------------------------------------|
| `--tier`       | `quick`, `standard`, or `full` (default: quick)|
| `--output`     | Write JSON report to file                      |
| `--baseline`   | Compare against a previous JSON report         |
| `--data-dir`   | Directory for temporary RocksDB data           |
| `--seed`       | RNG seed for reproducible datasets (default: 42)|

## Running a Pre-Release Benchmark

Before tagging a release, run the full suite on a dedicated machine and verify results against the baseline:

```bash
# Build release binary
cargo build -p hebbs-bench --release

# Run standard tier (suitable for most releases)
./target/release/hebbs-bench all \
    --tier standard \
    --output release-bench.json \
    --baseline benches/baseline.json

# For major releases, run full tier
./target/release/hebbs-bench all \
    --tier full \
    --output release-bench.json \
    --baseline benches/baseline.json
```

Check the output for regressions. A >10% p99 regression on any operation should block the release.

## Output Format

Reports are JSON with this structure:

```json
{
  "version": "0.1.0",
  "tier": "standard",
  "timestamp": "...",
  "system": { "os": "linux", "arch": "aarch64" },
  "results": {
    "latency": { "operations": [...] },
    "scalability": { "scale_points": [...] },
    "resources": { "measurements": [...] }
  }
}
```
