#!/usr/bin/env bash
set -euo pipefail

# metrics.sh - Timing and counting helpers for HEBBS e2e scenario tests.
# Source this file; do not execute directly.

declare -A _TIMER_START
declare -A _TIMER_ELAPSED
declare -A _COUNTERS
declare -A _METRICS_VALUES
declare -A _METRICS_UNITS

# ---------------------------------------------------------------------------
# Timers (millisecond precision)
# ---------------------------------------------------------------------------

_now_ms() {
  if [[ "$(uname)" == "Darwin" ]]; then
    # macOS: use perl for millisecond precision
    perl -MTime::HiRes=gettimeofday -e '($s,$us)=gettimeofday; printf "%d\n", $s*1000 + int($us/1000)'
  else
    date +%s%3N
  fi
}

timer_start() {
  local name="$1"
  _TIMER_START["${name}"]="$(_now_ms)"
}

timer_stop() {
  local name="$1"
  local end
  end="$(_now_ms)"
  local start="${_TIMER_START["${name}"]:-${end}}"
  _TIMER_ELAPSED["${name}"]=$(( end - start ))
}

timer_get() {
  local name="$1"
  echo "${_TIMER_ELAPSED["${name}"]:-0}"
}

# ---------------------------------------------------------------------------
# Counters
# ---------------------------------------------------------------------------

count_init() {
  local name="$1"
  _COUNTERS["${name}"]=0
}

count_inc() {
  local name="$1"
  local cur="${_COUNTERS["${name}"]:-0}"
  _COUNTERS["${name}"]=$(( cur + 1 ))
}

count_get() {
  local name="$1"
  echo "${_COUNTERS["${name}"]:-0}"
}

# ---------------------------------------------------------------------------
# Metric logging
# ---------------------------------------------------------------------------

log_metric() {
  local name="$1"
  local value="$2"
  local unit="${3:-}"
  _METRICS_VALUES["${name}"]="${value}"
  _METRICS_UNITS["${name}"]="${unit}"
  echo "  METRIC  ${name}=${value}${unit:+ ${unit}}"
}

save_metrics() {
  local output_file="$1"
  local first=true

  echo "{" > "${output_file}"

  # Merge timers into metrics
  for key in "${!_TIMER_ELAPSED[@]}"; do
    if [[ -z "${_METRICS_VALUES["${key}"]+x}" ]]; then
      _METRICS_VALUES["${key}"]="${_TIMER_ELAPSED["${key}"]}"
      _METRICS_UNITS["${key}"]="ms"
    fi
  done

  # Merge counters into metrics
  for key in "${!_COUNTERS[@]}"; do
    if [[ -z "${_METRICS_VALUES["${key}"]+x}" ]]; then
      _METRICS_VALUES["${key}"]="${_COUNTERS["${key}"]}"
      _METRICS_UNITS["${key}"]=""
    fi
  done

  # Write all metrics as JSON
  for key in "${!_METRICS_VALUES[@]}"; do
    if ${first}; then
      first=false
    else
      echo "," >> "${output_file}"
    fi
    local val="${_METRICS_VALUES["${key}"]}"
    local unit="${_METRICS_UNITS["${key}"]:-}"
    # Emit value as number if it looks numeric, otherwise as string
    if [[ "${val}" =~ ^-?[0-9]+(\.[0-9]+)?$ ]]; then
      printf '  "%s": {"value": %s, "unit": "%s"}' "${key}" "${val}" "${unit}" >> "${output_file}"
    else
      printf '  "%s": {"value": "%s", "unit": "%s"}' "${key}" "${val}" "${unit}" >> "${output_file}"
    fi
  done

  echo "" >> "${output_file}"
  echo "}" >> "${output_file}"
}
