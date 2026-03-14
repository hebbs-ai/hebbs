#!/usr/bin/env bash
set -euo pipefail

# vault_ops.sh - Vault manipulation helpers for HEBBS e2e scenario tests.
# Source this file; do not execute directly.

HEBBS_BIN="${HEBBS_BIN:-hebbs-vault}"

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
      local bad_count
      bad_count="$(jq '[.files[].sections[] | select(.state != "synced")] | length' "${manifest}" 2>/dev/null || echo "-1")"
      if [[ "${bad_count}" == "0" ]]; then
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
