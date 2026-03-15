#!/usr/bin/env bash
set -euo pipefail

# vault_ops.sh - Vault manipulation helpers for HEBBS e2e scenario tests.
# Source this file; do not execute directly.

HEBBS_BIN="${HEBBS_BIN:-hebbs}"

_WATCHER_PID=""

# ---------------------------------------------------------------------------
# Vault lifecycle
# ---------------------------------------------------------------------------

vault_init() {
  local vault_path="$1"
  "${HEBBS_BIN}" init --vault "${vault_path}"
}

vault_index() {
  local vault_path="$1"
  "${HEBBS_BIN}" index --vault "${vault_path}"
}

vault_watch_start() {
  local vault_path="$1"
  "${HEBBS_BIN}" watch --vault "${vault_path}" &
  _WATCHER_PID=$!
}

vault_watch_stop() {
  if [[ -n "${_WATCHER_PID}" ]] && kill -0 "${_WATCHER_PID}" 2>/dev/null; then
    kill -INT "${_WATCHER_PID}"
    wait "${_WATCHER_PID}" 2>/dev/null || true
    _WATCHER_PID=""
  fi
}

vault_status() {
  local vault_path="$1"
  "${HEBBS_BIN}" status --vault "${vault_path}"
}

vault_rebuild() {
  local vault_path="$1"
  "${HEBBS_BIN}" rebuild --vault "${vault_path}"
}

# ---------------------------------------------------------------------------
# File manipulation
# ---------------------------------------------------------------------------

create_md_file() {
  local vault_path="$1"
  local rel_path="$2"
  local content="$3"
  local full_path="${vault_path}/${rel_path}"
  mkdir -p "$(dirname "${full_path}")"
  printf '%s' "${content}" > "${full_path}"
}

modify_md_file() {
  local vault_path="$1"
  local rel_path="$2"
  local content="$3"
  local full_path="${vault_path}/${rel_path}"
  printf '%s' "${content}" > "${full_path}"
}

append_to_md_file() {
  local vault_path="$1"
  local rel_path="$2"
  local text="$3"
  local full_path="${vault_path}/${rel_path}"
  printf '%s' "${text}" >> "${full_path}"
}

delete_md_file() {
  local vault_path="$1"
  local rel_path="$2"
  rm -f "${vault_path}/${rel_path}"
}

rename_md_file() {
  local vault_path="$1"
  local old_rel="$2"
  local new_rel="$3"
  local new_full="${vault_path}/${new_rel}"
  mkdir -p "$(dirname "${new_full}")"
  mv "${vault_path}/${old_rel}" "${new_full}"
}

# ---------------------------------------------------------------------------
# Sync and manifest
# ---------------------------------------------------------------------------

wait_for_sync() {
  local vault_path="$1"
  local timeout_s="$2"
  local manifest="${vault_path}/.hebbs/manifest.json"
  local deadline=$(( $(date +%s) + timeout_s ))

  while (( $(date +%s) < deadline )); do
    if [[ -f "${manifest}" ]]; then
      local synced_count
      synced_count="$(jq '[.files[].sections[] | select(.state == "synced")] | length' "${manifest}" 2>/dev/null || echo "0")"
      # At least one section has been fully synced (phase 1 + phase 2 complete)
      if [[ "${synced_count}" -gt 0 ]]; then
        return 0
      fi
    fi
    sleep 0.2
  done
  return 1
}

get_manifest() {
  local vault_path="$1"
  cat "${vault_path}/.hebbs/manifest.json"
}

get_section_count() {
  local vault_path="$1"
  jq '[.files[].sections[]] | length' "${vault_path}/.hebbs/manifest.json"
}

# ---------------------------------------------------------------------------
# Temp vault helpers
# ---------------------------------------------------------------------------

create_temp_vault() {
  mktemp -d "${TMPDIR:-/tmp}/hebbs-e2e-vault.XXXXXX"
}

cleanup_vault() {
  local vault_path="$1"
  if [[ -n "${vault_path}" && -d "${vault_path}" ]]; then
    rm -rf "${vault_path}"
  fi
}

# ---------------------------------------------------------------------------
# Daemon lifecycle
# ---------------------------------------------------------------------------

_DAEMON_PID=""
_DAEMON_SOCK=""
_DAEMON_PID_FILE=""

daemon_start() {
  local idle_timeout="${1:-30}"
  local home_dir="${HOME:-$(eval echo ~)}"
  _DAEMON_SOCK="${home_dir}/.hebbs/daemon.sock"
  _DAEMON_PID_FILE="${home_dir}/.hebbs/daemon.pid"

  # Ensure ~/.hebbs/ exists
  mkdir -p "${home_dir}/.hebbs"

  # Clean up any stale socket/PID from a previous run
  rm -f "${_DAEMON_SOCK}" 2>/dev/null || true
  rm -f "${_DAEMON_PID_FILE}" 2>/dev/null || true

  # Start daemon in foreground mode (as background job), log to file
  local daemon_log="${home_dir}/.hebbs/daemon-test.log"
  "${HEBBS_BIN}" serve --foreground --idle-timeout "${idle_timeout}" > "${daemon_log}" 2>&1 &
  _DAEMON_PID=$!

  # Wait for socket to appear (up to 90s to account for first-run model download)
  local attempts=0
  local max_attempts=180  # 180 * 0.5s = 90s
  while [[ ! -S "${_DAEMON_SOCK}" ]] && (( attempts < max_attempts )); do
    # Check if daemon process died
    if ! kill -0 "${_DAEMON_PID}" 2>/dev/null; then
      echo "ERROR: daemon process died during startup"
      return 1
    fi
    sleep 0.5
    attempts=$((attempts + 1))
  done

  if [[ ! -S "${_DAEMON_SOCK}" ]]; then
    echo "ERROR: daemon socket did not appear within 90s"
    return 1
  fi
}

daemon_stop() {
  if [[ -n "${_DAEMON_PID}" ]] && kill -0 "${_DAEMON_PID}" 2>/dev/null; then
    kill -TERM "${_DAEMON_PID}" 2>/dev/null || true
    wait "${_DAEMON_PID}" 2>/dev/null || true
    _DAEMON_PID=""
  fi
  # Verify cleanup
  if [[ -n "${_DAEMON_SOCK}" && -S "${_DAEMON_SOCK}" ]]; then
    # Socket should be gone after clean shutdown
    sleep 0.2
  fi
}

daemon_ensure_running() {
  if [[ -n "${_DAEMON_PID}" ]] && kill -0 "${_DAEMON_PID}" 2>/dev/null; then
    return 0
  fi
  daemon_start "${1:-30}"
}

daemon_pid() {
  local home_dir="${HOME:-$(eval echo ~)}"
  local pid_file="${home_dir}/.hebbs/daemon.pid"
  if [[ -f "${pid_file}" ]]; then
    cat "${pid_file}"
  else
    echo ""
  fi
}

daemon_is_alive() {
  if [[ -n "${_DAEMON_PID}" ]] && kill -0 "${_DAEMON_PID}" 2>/dev/null; then
    return 0
  fi
  return 1
}

daemon_wait_for_exit() {
  local timeout_s="${1:-10}"
  local deadline=$(( $(date +%s) + timeout_s ))
  while (( $(date +%s) < deadline )); do
    if ! daemon_is_alive; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}
