#!/bin/bash
set -e

VAULT_DIR="/data/vault"
HEBBS_DIR="$VAULT_DIR/.hebbs"
DAEMON_DIR="/data/daemon"

# Create workspaces directory (writable by both platform and engine)
mkdir -p /data/workspaces
chmod 777 /data/workspaces

# Symlink ~/.hebbs to shared volume so platform can access daemon socket
mkdir -p "$DAEMON_DIR"
rm -rf /home/hebbs/.hebbs
ln -sf "$DAEMON_DIR" /home/hebbs/.hebbs

# Initialize vault if it doesn't exist
if [ ! -d "$HEBBS_DIR" ]; then
    echo "Initializing vault at $VAULT_DIR..."
    mkdir -p "$VAULT_DIR"
    hebbs init "$VAULT_DIR" \
        --provider "${HEBBS_LLM_PROVIDER:-openai}" \
        --model "${HEBBS_LLM_MODEL:-gpt-4o-mini}" \
        --key "${HEBBS_LLM_API_KEY}" \
        --local
    echo "Vault initialized. Stopping init daemon..."
    hebbs stop 2>/dev/null || true
    sleep 1
    rm -f "$DAEMON_DIR/daemon.pid" 2>/dev/null || true
fi

# cd to vault dir so `serve` picks up .hebbs/config.toml for embedding config
cd "$VAULT_DIR"

echo "Starting HEBBS daemon (panel on port ${HEBBS_PANEL_PORT:-6381})..."
exec hebbs serve --foreground \
    --panel-port "${HEBBS_PANEL_PORT:-6381}" \
    --idle-timeout 0 \
    --initial-vault "$VAULT_DIR"
