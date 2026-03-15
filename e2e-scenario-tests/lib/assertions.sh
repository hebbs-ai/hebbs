#!/usr/bin/env bash
set -euo pipefail

# assertions.sh - Shared assertion functions for HEBBS e2e scenario tests.
# Source this file; do not execute directly.
# Each assertion increments PASS_COUNT or FAIL_COUNT and logs the result.

PASS_COUNT=0
FAIL_COUNT=0
CHECKS=()

# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

_record_pass() {
  local name="$1"
  PASS_COUNT=$((PASS_COUNT + 1))
  CHECKS+=("PASS: ${name}")
  echo "  PASS  ${name}"
}

_record_fail() {
  local name="$1"
  local detail="${2:-}"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  if [[ -n "${detail}" ]]; then
    CHECKS+=("FAIL: ${name} -- ${detail}")
    echo "  FAIL  ${name} -- ${detail}"
  else
    CHECKS+=("FAIL: ${name}")
    echo "  FAIL  ${name}"
  fi
}

# ---------------------------------------------------------------------------
# General purpose
# ---------------------------------------------------------------------------

assert_pass() {
  local name="$1"
  local condition_result="$2"
  if [[ "${condition_result}" == "0" || "${condition_result}" == "true" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "condition returned ${condition_result}"
  fi
}

assert_eq() {
  local name="$1"
  local actual="$2"
  local expected="$3"
  if [[ "${actual}" == "${expected}" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "expected '${expected}', got '${actual}'"
  fi
}

assert_gt() {
  local name="$1"
  local actual="$2"
  local expected="$3"
  if (( actual > expected )); then
    _record_pass "${name}"
  else
    _record_fail "${name}" "expected ${actual} > ${expected}"
  fi
}

assert_lt() {
  local name="$1"
  local actual="$2"
  local expected="$3"
  if (( actual < expected )); then
    _record_pass "${name}"
  else
    _record_fail "${name}" "expected ${actual} < ${expected}"
  fi
}

# ---------------------------------------------------------------------------
# Filesystem
# ---------------------------------------------------------------------------

assert_file_exists() {
  local name="$1"
  local path="$2"
  if [[ -f "${path}" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "file not found: ${path}"
  fi
}

assert_file_not_exists() {
  local name="$1"
  local path="$2"
  if [[ ! -f "${path}" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "file should not exist: ${path}"
  fi
}

assert_path_exists() {
  local name="$1"
  local path="$2"
  if [[ -e "${path}" || -S "${path}" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "path not found: ${path}"
  fi
}

assert_dir_exists() {
  local name="$1"
  local path="$2"
  if [[ -d "${path}" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "directory not found: ${path}"
  fi
}

assert_contains() {
  local name="$1"
  local file_path="$2"
  local substring="$3"
  if [[ ! -f "${file_path}" ]]; then
    _record_fail "${name}" "file not found: ${file_path}"
    return
  fi
  if grep -qF "${substring}" "${file_path}"; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "file '${file_path}' does not contain '${substring}'"
  fi
}

# ---------------------------------------------------------------------------
# String
# ---------------------------------------------------------------------------

assert_contains_str() {
  local name="$1"
  local haystack="$2"
  local needle="$3"
  if echo "${haystack}" | grep -qF "${needle}"; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "string does not contain '${needle}'"
  fi
}

assert_not_contains_str() {
  local name="$1"
  local haystack="$2"
  local needle="$3"
  if echo "${haystack}" | grep -qF "${needle}"; then
    _record_fail "${name}" "string should not contain '${needle}'"
  else
    _record_pass "${name}"
  fi
}

# ---------------------------------------------------------------------------
# Manifest (JSON via jq)
# ---------------------------------------------------------------------------

_manifest_path() {
  echo "${1}/.hebbs/manifest.json"
}

assert_manifest_has_file() {
  local name="$1"
  local vault_path="$2"
  local rel_path="$3"
  local manifest
  manifest="$(_manifest_path "${vault_path}")"
  if [[ ! -f "${manifest}" ]]; then
    _record_fail "${name}" "manifest not found: ${manifest}"
    return
  fi
  if jq -e --arg p "${rel_path}" '.files[$p]' "${manifest}" >/dev/null 2>&1; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "manifest missing file '${rel_path}'"
  fi
}

assert_manifest_file_count() {
  local name="$1"
  local vault_path="$2"
  local expected="$3"
  local manifest
  manifest="$(_manifest_path "${vault_path}")"
  if [[ ! -f "${manifest}" ]]; then
    _record_fail "${name}" "manifest not found: ${manifest}"
    return
  fi
  local actual
  actual="$(jq '.files | length' "${manifest}")"
  if [[ "${actual}" == "${expected}" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "expected ${expected} files, got ${actual}"
  fi
}

assert_all_sections_synced() {
  local name="$1"
  local vault_path="$2"
  local manifest
  manifest="$(_manifest_path "${vault_path}")"
  if [[ ! -f "${manifest}" ]]; then
    _record_fail "${name}" "manifest not found: ${manifest}"
    return
  fi
  local bad_count
  bad_count="$(jq '[.files[].sections[] | select(.state != "synced")] | length' "${manifest}")"
  if [[ "${bad_count}" == "0" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "${bad_count} section(s) not in synced state"
  fi
}

assert_section_count() {
  local name="$1"
  local vault_path="$2"
  local expected="$3"
  local manifest
  manifest="$(_manifest_path "${vault_path}")"
  if [[ ! -f "${manifest}" ]]; then
    _record_fail "${name}" "manifest not found: ${manifest}"
    return
  fi
  local actual
  actual="$(jq '[.files[].sections[]] | length' "${manifest}")"
  if [[ "${actual}" == "${expected}" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "expected ${expected} sections, got ${actual}"
  fi
}

assert_section_state_count() {
  local name="$1"
  local vault_path="$2"
  local state="$3"
  local expected="$4"
  local manifest
  manifest="$(_manifest_path "${vault_path}")"
  if [[ ! -f "${manifest}" ]]; then
    _record_fail "${name}" "manifest not found: ${manifest}"
    return
  fi
  local actual
  actual="$(jq --arg s "${state}" '[.files[].sections[] | select(.state == $s)] | length' "${manifest}")"
  if [[ "${actual}" == "${expected}" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "expected ${expected} sections in state '${state}', got ${actual}"
  fi
}

# ---------------------------------------------------------------------------
# JSON field
# ---------------------------------------------------------------------------

assert_json_field() {
  local name="$1"
  local json_file="$2"
  local jq_expr="$3"
  local expected="$4"
  if [[ ! -f "${json_file}" ]]; then
    _record_fail "${name}" "JSON file not found: ${json_file}"
    return
  fi
  local actual
  actual="$(jq -r "${jq_expr}" "${json_file}")"
  if [[ "${actual}" == "${expected}" ]]; then
    _record_pass "${name}"
  else
    _record_fail "${name}" "jq '${jq_expr}' returned '${actual}', expected '${expected}'"
  fi
}

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

summary() {
  echo ""
  echo "=============================="
  echo "  Results: ${PASS_COUNT} passed, ${FAIL_COUNT} failed"
  echo "=============================="
  if (( FAIL_COUNT > 0 )); then
    echo ""
    echo "Failures:"
    for c in "${CHECKS[@]}"; do
      if [[ "${c}" == FAIL:* ]]; then
        echo "  ${c}"
      fi
    done
    return 1
  fi
  return 0
}
