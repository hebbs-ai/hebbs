#!/bin/sh
# HEBBS Enterprise Server Installer
# Usage: curl -sSf https://hebbs.ai/server | OPENAI_API_KEY=sk-... sh
#
# Environment variables:
#   OPENAI_API_KEY    (required) OpenAI API key
#   HEBBS_PORT        Port for dashboard and API (default: 8080)
#   HEBBS_DIR         Installation directory (default: ./hebbs)
#   HEBBS_VERSION     Image version (default: 0.4.0)
#   HEBBS_LLM_MODEL   LLM model (default: gpt-4o-mini)
#   HEBBS_EMBED_MODEL  Embedding model (default: text-embedding-3-small)

set -eu

VERSION="${HEBBS_VERSION:-0.4.0}"
PORT="${HEBBS_PORT:-8080}"
DIR="${HEBBS_DIR:-./hebbs}"
LLM_MODEL="${HEBBS_LLM_MODEL:-gpt-4o-mini}"
EMBED_MODEL="${HEBBS_EMBED_MODEL:-text-embedding-3-small}"

# ── Formatting ───────────────────────────────────────────────────────────────

bold=""
reset=""
green=""
red=""
yellow=""
if [ -t 1 ]; then
    bold="\033[1m"
    reset="\033[0m"
    green="\033[32m"
    red="\033[31m"
    yellow="\033[33m"
fi

info()  { printf "${bold}${green}  ▸${reset} %s\n" "$1"; }
warn()  { printf "${bold}${yellow}  ▸${reset} %s\n" "$1"; }
err()   { printf "${bold}${red}  ✗${reset} %s\n" "$1" >&2; }
die()   { err "$1"; exit 1; }

# ── Banner ───────────────────────────────────────────────────────────────────

printf "\n"
printf "${bold}  HEBBS Enterprise Installer${reset}\n"
printf "\n"

# ── Preflight checks ────────────────────────────────────────────────────────

info "Checking prerequisites..."

if ! command -v docker >/dev/null 2>&1; then
    die "Docker is not installed. Install it from https://docs.docker.com/engine/install/"
fi

if ! docker info >/dev/null 2>&1; then
    die "Docker daemon is not running. Start it with: sudo systemctl start docker"
fi

if ! docker compose version >/dev/null 2>&1; then
    die "Docker Compose v2 is required. Update Docker or install the compose plugin."
fi

if ! command -v curl >/dev/null 2>&1; then
    die "curl is required but not found."
fi

DOCKER_VERSION=$(docker version --format '{{.Server.Version}}' 2>/dev/null || echo "unknown")
COMPOSE_VERSION=$(docker compose version --short 2>/dev/null || echo "unknown")
info "Docker ${DOCKER_VERSION}, Compose ${COMPOSE_VERSION}"

# ── Resolve OpenAI API key ──────────────────────────────────────────────────

if [ -z "${OPENAI_API_KEY:-}" ]; then
    if [ -t 0 ]; then
        printf "  Enter your OpenAI API key: "
        read -r OPENAI_API_KEY
        if [ -z "$OPENAI_API_KEY" ]; then
            die "OpenAI API key is required."
        fi
    else
        die "OPENAI_API_KEY is required. Usage: curl -sSf https://hebbs.ai/server | OPENAI_API_KEY=sk-... sh"
    fi
fi

info "OpenAI API key: ${OPENAI_API_KEY%"${OPENAI_API_KEY#sk-????}"}..."

# ── Create install directory ─────────────────────────────────────────────────

FULL_DIR=$(mkdir -p "$DIR" && cd "$DIR" && pwd)

if [ -f "$FULL_DIR/docker-compose.yml" ]; then
    warn "Existing installation found at $FULL_DIR"
    warn "Updating configuration and restarting..."
fi

info "Installing to $FULL_DIR"

# ── Write docker-compose.yml ─────────────────────────────────────────────────

cat > "$FULL_DIR/docker-compose.yml" << COMPOSEOF
services:
  platform:
    image: ghcr.io/hebbs-ai/hebbs-platform:${VERSION}
    ports:
      - "\${HEBBS_PORT:-8080}:8080"
    environment:
      - HEBBS_ENGINE_URL=http://engine:6381
      - HEBBS_ENGINE_SOCKET=/data/daemon/daemon.sock
      - HEBBS_DEFAULT_VAULT=/data/vault
      - HEBBS_DB_PATH=/data/platform.db
      - HEBBS_LLM_PROVIDER=openai
      - HEBBS_LLM_MODEL=\${HEBBS_LLM_MODEL:-gpt-4o-mini}
      - HEBBS_LLM_API_KEY=\${OPENAI_API_KEY}
      - HEBBS_WORKSPACES_DIR=/data/workspaces
    volumes:
      - hebbs-data:/data
    depends_on:
      engine:
        condition: service_healthy
    restart: unless-stopped

  engine:
    image: ghcr.io/hebbs-ai/hebbs-engine:${VERSION}
    environment:
      - HEBBS_LLM_PROVIDER=openai
      - HEBBS_LLM_MODEL=\${HEBBS_LLM_MODEL:-gpt-4o-mini}
      - HEBBS_LLM_API_KEY=\${OPENAI_API_KEY}
      - HEBBS_EMBED_PROVIDER=openai
      - HEBBS_EMBED_MODEL=\${HEBBS_EMBED_MODEL:-text-embedding-3-small}
      - HEBBS_EMBED_DIMENSIONS=1536
      - HEBBS_PANEL_PORT=6381
      - HEBBS_PANEL_BIND_ADDRESS=0.0.0.0
    volumes:
      - hebbs-data:/data
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:6381/api/panel/health"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 30s
    restart: unless-stopped

volumes:
  hebbs-data:
COMPOSEOF

info "docker-compose.yml written"

# ── Write .env ───────────────────────────────────────────────────────────────

cat > "$FULL_DIR/.env" << ENVOF
OPENAI_API_KEY=${OPENAI_API_KEY}
HEBBS_PORT=${PORT}
HEBBS_LLM_MODEL=${LLM_MODEL}
HEBBS_EMBED_MODEL=${EMBED_MODEL}
ENVOF

chmod 600 "$FULL_DIR/.env"
info ".env written (mode 600)"

# ── Pull and start ───────────────────────────────────────────────────────────

cd "$FULL_DIR"

info "Pulling images..."
docker compose pull --quiet 2>/dev/null || docker compose pull

info "Starting services..."
docker compose up -d

# ── Wait for health ──────────────────────────────────────────────────────────

info "Waiting for HEBBS to become healthy..."

HEALTH_URL="http://localhost:${PORT}/health"
TIMEOUT=120
elapsed=0

while [ "$elapsed" -lt "$TIMEOUT" ]; do
    if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
        break
    fi
    sleep 2
    elapsed=$((elapsed + 2))
    printf "."
done

if [ "$elapsed" -ge "$TIMEOUT" ]; then
    printf "\n"
    err "Timed out after ${TIMEOUT}s. Check logs with: cd $FULL_DIR && docker compose logs"
    exit 1
fi

printf "\n"

# ── Success ──────────────────────────────────────────────────────────────────

printf "\n"
printf "${bold}${green}  ✓ HEBBS Enterprise ${VERSION} is running!${reset}\n"
printf "\n"
printf "    Dashboard:  ${bold}http://localhost:${PORT}${reset}\n"
printf "    API:        http://localhost:${PORT}/v1\n"
printf "    Directory:  ${FULL_DIR}\n"
printf "\n"
printf "  ${bold}Next steps:${reset}\n"
printf "    1. Open http://localhost:${PORT} to create your admin account\n"
printf "    2. Install the CLI:\n"
printf "       curl -sSf https://hebbs.ai/install | sh\n"
printf "    3. Connect the CLI:\n"
printf "       hebbs login --endpoint http://localhost:${PORT} --api-key <your-key>\n"
printf "\n"
printf "  ${bold}Manage:${reset}\n"
printf "    cd ${FULL_DIR}\n"
printf "    docker compose logs -f                              # view logs\n"
printf "    docker compose down                                 # stop\n"
printf "    docker compose pull && docker compose up -d         # upgrade\n"
printf "\n"
