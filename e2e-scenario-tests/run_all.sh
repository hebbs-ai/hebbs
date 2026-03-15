#!/usr/bin/env bash
set -euo pipefail

# HEBBS Vault E2E Scenario Test Runner
# Runs all (or selected) scenario tests and produces a summary.
#
# Usage:
#   ./run_all.sh                         # all scenarios
#   ./run_all.sh --scenario 02           # single scenario
#   ./run_all.sh --vault-size large      # override vault size
#   ./run_all.sh --verbose               # verbose output
#   ./run_all.sh --binary /path/to/bin   # custom binary path

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Require Bash 4+ for associative arrays (declare -A)
if (( BASH_VERSINFO[0] < 4 )); then
    echo "Error: Bash 4+ is required (found ${BASH_VERSION})"
    echo "On macOS: brew install bash && /opt/homebrew/bin/bash ./run_all.sh"
    exit 1
fi

# Parse arguments
SCENARIO_FILTER=""
VAULT_SIZE="medium"
VERBOSE=""
HEBBS_BIN="${HEBBS_BIN:-hebbs}"
TEST_MODE="${TEST_MODE:-all}"  # all, local, daemon

while [[ $# -gt 0 ]]; do
    case "$1" in
        --scenario)
            SCENARIO_FILTER="$2"
            shift 2
            ;;
        --vault-size)
            VAULT_SIZE="$2"
            shift 2
            ;;
        --verbose)
            VERBOSE="--verbose"
            shift
            ;;
        --binary)
            HEBBS_BIN="$2"
            shift 2
            ;;
        --mode)
            TEST_MODE="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: ./run_all.sh [--scenario NUM] [--vault-size SIZE] [--verbose] [--binary PATH] [--mode local|daemon|all]"
            exit 1
            ;;
    esac
done

export VAULT_SIZE
export HEBBS_BIN
export VERBOSE
export TEST_MODE

# Check prerequisites
if ! command -v "$HEBBS_BIN" &> /dev/null; then
    # Try to find it in the build directory
    CANDIDATE="$SCRIPT_DIR/../hebbs/target/release/hebbs"
    if [ -f "$CANDIDATE" ]; then
        HEBBS_BIN="$CANDIDATE"
        export HEBBS_BIN
        echo "Using binary: $HEBBS_BIN"
    else
        CANDIDATE="$SCRIPT_DIR/../hebbs/target/debug/hebbs"
        if [ -f "$CANDIDATE" ]; then
            HEBBS_BIN="$CANDIDATE"
            export HEBBS_BIN
            echo "Using binary: $HEBBS_BIN (debug build)"
        else
            echo "Error: hebbs binary not found."
            echo "Build it with: cd ../hebbs && cargo build -p hebbs-vault [--release]"
            echo "Or specify: ./run_all.sh --binary /path/to/hebbs"
            exit 1
        fi
    fi
fi

if ! command -v python3 &> /dev/null; then
    echo "Error: python3 is required for fixture generation"
    exit 1
fi

if ! command -v jq &> /dev/null; then
    echo "Error: jq is required for JSON assertions"
    exit 1
fi

# Create results directory
TIMESTAMP=$(date +%Y-%m-%d_%H-%M-%S)
RESULTS_DIR="$SCRIPT_DIR/results/$TIMESTAMP"
mkdir -p "$RESULTS_DIR"

echo "============================================"
echo "  HEBBS Vault E2E Scenario Tests"
echo "============================================"
echo "Binary:     $HEBBS_BIN"
echo "Vault size: $VAULT_SIZE"
echo "Mode:       $TEST_MODE"
echo "Results:    $RESULTS_DIR"
echo "============================================"
echo ""

# Collect scenarios
SCENARIOS_DIR="$SCRIPT_DIR/scenarios"
SCENARIOS_PASSED=0
SCENARIOS_FAILED=0
SCENARIOS_SKIPPED=0
SCENARIO_RESULTS=()

for scenario_script in "$SCENARIOS_DIR"/*.sh; do
    scenario_name=$(basename "$scenario_script" .sh)
    scenario_num="${scenario_name%%_*}"

    # Apply filter
    if [ -n "$SCENARIO_FILTER" ] && [ "$scenario_num" != "$SCENARIO_FILTER" ]; then
        continue
    fi

    # Apply mode filter: scenarios 17-20 are daemon tests, 01-16 are local tests
    scenario_num_int=$((10#$scenario_num))
    if [ "$TEST_MODE" = "local" ] && [ "$scenario_num_int" -ge 17 ]; then
        continue
    fi
    if [ "$TEST_MODE" = "daemon" ] && [ "$scenario_num_int" -lt 17 ]; then
        continue
    fi

    # Local tests (01-16) run without daemon; daemon tests (17-20) use daemon
    if [ "$scenario_num_int" -lt 17 ]; then
        export HEBBS_NO_DAEMON=1
    else
        unset HEBBS_NO_DAEMON
    fi

    echo "--- Running: $scenario_name ---"
    SCENARIO_START=$(date +%s%N)

    # Run scenario, capture exit code
    set +e
    OUTPUT=$( "$BASH" "$scenario_script" 2>&1 )
    EXIT_CODE=$?
    set -e

    SCENARIO_END=$(date +%s%N)
    DURATION_MS=$(( (SCENARIO_END - SCENARIO_START) / 1000000 ))

    # Save output
    echo "$OUTPUT" > "$RESULTS_DIR/${scenario_name}.log"

    if [ $EXIT_CODE -eq 0 ]; then
        echo "  PASSED ($DURATION_MS ms)"
        SCENARIOS_PASSED=$((SCENARIOS_PASSED + 1))
        SCENARIO_RESULTS+=("{\"name\":\"$scenario_name\",\"pass\":true,\"duration_ms\":$DURATION_MS}")
    else
        echo "  FAILED (exit code: $EXIT_CODE, $DURATION_MS ms)"
        echo "  Output: $(echo "$OUTPUT" | tail -5)"
        SCENARIOS_FAILED=$((SCENARIOS_FAILED + 1))
        SCENARIO_RESULTS+=("{\"name\":\"$scenario_name\",\"pass\":false,\"duration_ms\":$DURATION_MS,\"exit_code\":$EXIT_CODE}")
    fi
    echo ""
done

# Generate summary
TOTAL=$((SCENARIOS_PASSED + SCENARIOS_FAILED))
OVERALL_PASS="true"
if [ $SCENARIOS_FAILED -gt 0 ]; then
    OVERALL_PASS="false"
fi

# Build JSON array of results
RESULTS_JSON="["
for i in "${!SCENARIO_RESULTS[@]}"; do
    if [ $i -gt 0 ]; then
        RESULTS_JSON+=","
    fi
    RESULTS_JSON+="${SCENARIO_RESULTS[$i]}"
done
RESULTS_JSON+="]"

cat > "$RESULTS_DIR/summary.json" << ENDJSON
{
  "run_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "platform": "$(uname -s)-$(uname -m)",
  "vault_size": "$VAULT_SIZE",
  "binary": "$HEBBS_BIN",
  "overall_pass": $OVERALL_PASS,
  "scenarios_passed": $SCENARIOS_PASSED,
  "scenarios_failed": $SCENARIOS_FAILED,
  "scenarios_total": $TOTAL,
  "scenarios": $RESULTS_JSON
}
ENDJSON

echo "============================================"
echo "  Results Summary"
echo "============================================"
echo "  Passed:  $SCENARIOS_PASSED / $TOTAL"
echo "  Failed:  $SCENARIOS_FAILED / $TOTAL"
echo "  Results: $RESULTS_DIR/summary.json"
echo "============================================"

if [ $SCENARIOS_FAILED -gt 0 ]; then
    exit 1
fi
