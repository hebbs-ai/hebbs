#!/usr/bin/env bash
set -euo pipefail

# Scenario 23: Panel Serving from Daemon (Milestone 6)
# Tests that the daemon serves the Memory Palace control panel:
#   1. Daemon starts and panel HTTP port is listening
#   2. Panel serves static files (index.html, app.js)
#   3. Panel API returns vault status
#   4. Panel API returns graph data after storing memories
#   5. Panel API lists vaults from vaults.json
#   6. Daemon still serves Unix socket queries alongside panel

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/assertions.sh"
source "$SCRIPT_DIR/../lib/metrics.sh"
source "$SCRIPT_DIR/../lib/vault_ops.sh"

scenario_name="23_daemon_panel_serving"
echo "=== Scenario 23: Panel Serving from Daemon ==="

# --- Setup ---
FAKE_HOME=$(mktemp -d "/tmp/hb-panel.XXXXXX")
export HOME="$FAKE_HOME"
PROJECT_DIR="${FAKE_HOME}/test-project"
mkdir -p "$PROJECT_DIR"

# Pick a random high port to avoid conflicts
PANEL_PORT=$((20000 + RANDOM % 10000))

cleanup() {
  daemon_stop 2>/dev/null || true
  rm -rf "$FAKE_HOME"
}
trap cleanup EXIT

# Initialize vaults
"${HEBBS_BIN}" init "$FAKE_HOME" 2>/dev/null
"${HEBBS_BIN}" init "$PROJECT_DIR" 2>/dev/null

# Start daemon with panel on our test port
_DAEMON_SOCK="${FAKE_HOME}/.hebbs/daemon.sock"
_DAEMON_PID_FILE="${FAKE_HOME}/.hebbs/daemon.pid"
mkdir -p "${FAKE_HOME}/.hebbs"
rm -f "${_DAEMON_SOCK}" 2>/dev/null || true
rm -f "${_DAEMON_PID_FILE}" 2>/dev/null || true

DAEMON_LOG="${FAKE_HOME}/.hebbs/daemon-test.log"
"${HEBBS_BIN}" serve --foreground --idle-timeout 120 --panel-port "$PANEL_PORT" > "$DAEMON_LOG" 2>&1 &
_DAEMON_PID=$!

# Wait for socket
attempts=0
max_attempts=180
while [[ ! -S "${_DAEMON_SOCK}" ]] && (( attempts < max_attempts )); do
  if ! kill -0 "${_DAEMON_PID}" 2>/dev/null; then
    echo "ERROR: daemon died during startup"
    cat "$DAEMON_LOG" 2>/dev/null || true
    exit 1
  fi
  sleep 0.5
  attempts=$((attempts + 1))
done

if [[ ! -S "${_DAEMON_SOCK}" ]]; then
  echo "ERROR: daemon socket did not appear"
  exit 1
fi

# Wait for HTTP panel to be ready (up to 30s after socket)
# The panel starts after the daemon opens the global vault, which may take a moment.
attempts=0
while (( attempts < 60 )); do
  HTTP_CHECK=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PANEL_PORT}/" 2>/dev/null || echo "000")
  if [[ "$HTTP_CHECK" == "200" ]]; then
    break
  fi
  sleep 0.5
  attempts=$((attempts + 1))
done

echo "  Daemon started with panel on port $PANEL_PORT"

# =========================================================================
# 1. Panel HTTP port is listening
# =========================================================================
echo ""
echo "--- 1. Panel HTTP port is listening ---"

HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PANEL_PORT}/" 2>/dev/null)
assert_eq "panel_http_reachable" "$HTTP_CODE" "200"

echo "  Panel HTTP responding (status $HTTP_CODE)"

# =========================================================================
# 2. Panel serves static files
# =========================================================================
echo ""
echo "--- 2. Static files served ---"

INDEX_HTML=$(curl -s "http://127.0.0.1:${PANEL_PORT}/" 2>/dev/null)
INDEX_FILE=$(mktemp)
echo "$INDEX_HTML" > "$INDEX_FILE"
assert_contains "index_has_title" "$INDEX_FILE" "Memory Palace"
rm -f "$INDEX_FILE"

APP_JS_CODE=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PANEL_PORT}/static/app.js" 2>/dev/null || echo "000")
assert_eq "app_js_served" "$APP_JS_CODE" "200"

CSS_CODE=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PANEL_PORT}/static/panel.css" 2>/dev/null || echo "000")
assert_eq "css_served" "$CSS_CODE" "200"

echo "  Static files served correctly"

# =========================================================================
# 3. Panel API returns vault status
# =========================================================================
echo ""
echo "--- 3. Panel API vault status ---"

STATUS_JSON=$(curl -s "http://127.0.0.1:${PANEL_PORT}/api/panel/status" 2>/dev/null)
STATUS_FILE=$(mktemp)
echo "$STATUS_JSON" > "$STATUS_FILE"
assert_contains "status_has_vault_path" "$STATUS_FILE" "vault_path"
rm -f "$STATUS_FILE"

echo "  Status API working"

# =========================================================================
# 4. Store memories and verify graph + recall APIs
# =========================================================================
echo ""
echo "--- 4. Store memories and verify APIs ---"

# Store memories via daemon (Unix socket)
"${HEBBS_BIN}" remember "The deployment uses Kubernetes with Helm charts" \
  --importance 0.8 --entity-id infra --format json --global 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "Database backups run every 6 hours to S3" \
  --importance 0.7 --entity-id infra --format json --global 2>/dev/null > /dev/null
"${HEBBS_BIN}" remember "API rate limiting is set to 100 requests per minute" \
  --importance 0.9 --entity-id infra --format json --global 2>/dev/null > /dev/null

# Graph API returns valid response (nodes come from manifest, not remember API)
GRAPH_JSON=$(curl -s "http://127.0.0.1:${PANEL_PORT}/api/panel/graph" 2>/dev/null)
GRAPH_HAS_NODES=$(echo "$GRAPH_JSON" | jq 'has("nodes")' 2>/dev/null || echo "false")
assert_eq "graph_api_valid" "$GRAPH_HAS_NODES" "true"

# Recall API works through the panel (same engine as daemon)
RECALL_JSON=$(curl -s -X POST "http://127.0.0.1:${PANEL_PORT}/api/panel/recall" \
  -H "Content-Type: application/json" \
  -d '{"query":"Kubernetes deployment","top_k":5}' 2>/dev/null)
RECALL_COUNT=$(echo "$RECALL_JSON" | jq '.results | length' 2>/dev/null || echo "0")
assert_gt "panel_recall_works" "$RECALL_COUNT" "0"

echo "  Graph and recall APIs working ($RECALL_COUNT recall results)"

# =========================================================================
# 5. Vault listing API
# =========================================================================
echo ""
echo "--- 5. Vault listing API ---"

VAULTS_JSON=$(curl -s "http://127.0.0.1:${PANEL_PORT}/api/panel/vaults" 2>/dev/null)
VAULTS_COUNT=$(echo "$VAULTS_JSON" | jq 'length' 2>/dev/null || echo "0")

assert_gt "vaults_listed" "$VAULTS_COUNT" "0"

echo "  Vaults API returned $VAULTS_COUNT vault(s)"

# =========================================================================
# 6. Daemon still serves Unix socket alongside panel
# =========================================================================
echo ""
echo "--- 6. Unix socket still works alongside panel ---"

RECALL_OUT=$("${HEBBS_BIN}" recall "Kubernetes deployment" \
  --strategy similarity --top-k 5 --format json --global 2>/dev/null)
RECALL_COUNT=$(echo "$RECALL_OUT" | jq 'length')
assert_gt "socket_recall_works" "$RECALL_COUNT" "0"

echo "  Unix socket queries work alongside HTTP panel"

# =========================================================================
# 7. Dashboard API
# =========================================================================
echo ""
echo "--- 7. Dashboard API ---"

DASHBOARD_CODE=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PANEL_PORT}/api/panel/dashboard" 2>/dev/null || echo "000")
assert_eq "dashboard_api_works" "$DASHBOARD_CODE" "200"

echo "  Dashboard API working"

# =========================================================================
# Summary
# =========================================================================

log_metric "panel_port" "$PANEL_PORT" "port"
log_metric "vaults_listed" "$VAULTS_COUNT" "count"

save_metrics "$SCRIPT_DIR/../results/${scenario_name}.json"

echo ""
summary
