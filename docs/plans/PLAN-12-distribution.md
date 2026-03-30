# PLAN-12: Enterprise Distribution

Ship HEBBS Enterprise as a one-command install for customers. No source code, no build steps.

## Architecture

### Two binaries, one name

| Binary | Crate | Contains | Size | Who |
|--------|-------|----------|------|-----|
| `hebbs` (full) | hebbs-vault | Engine + daemon + local CLI + REST client | ~80MB | Open-source users |
| `hebbs` (cli) | hebbs-cli | REST client only, no engine | ~5MB | Enterprise customers |

Both binaries are named `hebbs`. Same commands, same docs. The CLI build has no RocksDB, no ONNX, no embeddings, no daemon. Just REST calls to a remote server.

When an enterprise customer runs `hebbs init .`, they get: "Local mode not available in this build. Connect to a server: hebbs login --endpoint <url>".

### Three distribution channels

**Homebrew:**
```sh
# Enterprise (CLI only, 5MB)
brew install hebbs-ai/tap/hebbs-cli

# Open-source (full engine, 80MB)
brew install hebbs-ai/tap/hebbs
```

Both install a binary named `hebbs`. Different formulae, different downloads.

**Curl install:**
```sh
# CLI only (default)
curl -sSf https://hebbs.ai/install | sh

# Full engine
curl -sSf https://hebbs.ai/install | sh -s -- --full
```

One install script, detects OS + arch, downloads the right binary to `/usr/local/bin/hebbs`.

**Docker (enterprise server):**
```sh
# Customer gets a compose file + token
echo "<token>" | docker login ghcr.io -u hebbs-customer --password-stdin
docker compose up -d
```

## What the customer gets

### Server setup (admin, once)

```
hebbs-enterprise/
  docker-compose.yml     # Pulls pre-built images from GHCR
  .env.example           # OPENAI_API_KEY + optional config
  README.md              # Step-by-step setup
```

```sh
cp .env.example .env
# Set OPENAI_API_KEY
docker compose up -d
# Open http://localhost:8080 -> onboarding wizard
```

### Developer setup (every team member)

```sh
# Install CLI
curl -sSf https://hebbs.ai/install | sh

# Connect
hebbs login --endpoint https://hebbs.acme.com

# Use
hebbs recall "password reset process"
hebbs push ./docs
hebbs remember "SOC2 audit passed" --importance 0.8
```

### SDK (Python)

```sh
pip install hebbs
```

```python
from hebbs import HebbsRestClient

async with HebbsRestClient("https://hebbs.acme.com", api_key="hb_...") as hb:
    results = await hb.recall("query")
    print(results.text)
```

## Build plan

### Phase 1: Rust CLI with REST transport

Add REST transport to `hebbs-cli` crate so the same binary works with remote servers.

**Files to modify:**
- `hebbs-cli/Cargo.toml` -- add `reqwest` with feature flags
- `hebbs-cli/src/transport/mod.rs` -- NEW: trait for transport abstraction
- `hebbs-cli/src/transport/grpc.rs` -- NEW: wrap existing gRPC client
- `hebbs-cli/src/transport/rest.rs` -- NEW: REST client using reqwest
- `hebbs-cli/src/connection.rs` -- refactor to use transport trait
- `hebbs-cli/src/commands.rs` -- no changes (uses trait)
- `hebbs-cli/src/config.rs` -- add `transport` field (auto-detect from endpoint)
- `hebbs-cli/src/error.rs` -- add HTTP status code mapping

**Feature flags:**
```toml
[features]
default = ["grpc", "rest"]
grpc = ["tonic", "hebbs-proto"]
rest = ["reqwest"]
```

**Two binary targets in Cargo.toml:**
```toml
[[bin]]
name = "hebbs"
path = "src/main.rs"
required-features = ["grpc"]  # full binary includes gRPC

[[bin]]
name = "hebbs-remote"
path = "src/main.rs"          # same entry point, different features
```

Building:
```sh
# Full binary (open-source)
cargo build --release -p hebbs-cli

# CLI only (enterprise)
cargo build --release -p hebbs-cli --no-default-features --features rest
```

**Transport auto-detection:**
- Endpoint port 6380 -> gRPC (local daemon)
- Endpoint port 8080 or 443 -> REST (remote server)
- Explicit `--transport grpc|rest` flag overrides

**Commands supported in REST mode:**
- login, recall, remember, prime, forget, status, push, workspaces, keys, dashboard
- NOT supported: subscribe (streaming), reflect/contradict (two-phase), REPL dot-commands (.connect, .disconnect)
- Unsupported commands in REST mode print: "This command requires local mode. Install the full binary: brew install hebbs-ai/tap/hebbs"

### Phase 2: Docker images to GHCR

Push pre-built images to private GitHub Container Registry.

**Images:**
- `ghcr.io/hebbs-ai/hebbs-engine:0.4.0` (Rust engine, ~150MB)
- `ghcr.io/hebbs-ai/hebbs-platform:0.4.0` (Node.js platform + dashboard, ~250MB)

**Production docker-compose.yml:**
```yaml
services:
  platform:
    image: ghcr.io/hebbs-ai/hebbs-platform:0.4.0
    ports:
      - "${HEBBS_PORT:-8080}:8080"
    environment:
      - HEBBS_ENGINE_URL=http://engine:6381
      - HEBBS_ENGINE_SOCKET=/data/daemon/daemon.sock
      - HEBBS_DEFAULT_VAULT=/data/vault
      - HEBBS_DB_PATH=/data/platform.db
      - HEBBS_LLM_PROVIDER=openai
      - HEBBS_LLM_MODEL=${HEBBS_LLM_MODEL:-gpt-4o-mini}
      - HEBBS_LLM_API_KEY=${OPENAI_API_KEY}
      - HEBBS_WORKSPACES_DIR=/data/workspaces
    volumes:
      - hebbs-data:/data
    depends_on:
      engine:
        condition: service_healthy
    restart: unless-stopped

  engine:
    image: ghcr.io/hebbs-ai/hebbs-engine:0.4.0
    environment:
      - HEBBS_LLM_PROVIDER=openai
      - HEBBS_LLM_MODEL=${HEBBS_LLM_MODEL:-gpt-4o-mini}
      - HEBBS_LLM_API_KEY=${OPENAI_API_KEY}
      - HEBBS_EMBED_PROVIDER=openai
      - HEBBS_EMBED_MODEL=${HEBBS_EMBED_MODEL:-text-embedding-3-small}
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
```

**.env.example:**
```sh
# Required
OPENAI_API_KEY=sk-proj-your-key-here

# Optional
HEBBS_PORT=8080
HEBBS_LLM_MODEL=gpt-4o-mini
HEBBS_EMBED_MODEL=text-embedding-3-small
```

### Phase 3: Install script

**https://hebbs.ai/install** (shell script):
```sh
#!/bin/sh
set -e
FULL=false
for arg in "$@"; do [ "$arg" = "--full" ] && FULL=true; done

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
case "$ARCH" in x86_64) ARCH="x86_64" ;; arm64|aarch64) ARCH="aarch64" ;; esac
case "$OS" in linux) TARGET="${ARCH}-unknown-linux-gnu" ;; darwin) TARGET="${ARCH}-apple-darwin" ;; esac

if [ "$FULL" = true ]; then
  BINARY="hebbs-${TARGET}"
else
  BINARY="hebbs-cli-${TARGET}"
fi

VERSION=$(curl -sL https://api.github.com/repos/hebbs-ai/hebbs/releases/latest | grep tag_name | cut -d'"' -f4)
URL="https://github.com/hebbs-ai/hebbs/releases/download/${VERSION}/${BINARY}"

curl -sL "$URL" -o /usr/local/bin/hebbs
chmod +x /usr/local/bin/hebbs
echo "Installed hebbs ${VERSION}"
```

### Phase 4: Homebrew tap

Update `homebrew-tap` repo with two formulae:

**Formula: hebbs.rb** (full engine)
```ruby
class Hebbs < Formula
  desc "Cognitive memory engine for AI agents"
  version "0.4.0"
  if Hardware::CPU.intel?
    url "https://github.com/hebbs-ai/hebbs/releases/download/v0.4.0/hebbs-x86_64-apple-darwin"
  else
    url "https://github.com/hebbs-ai/hebbs/releases/download/v0.4.0/hebbs-aarch64-apple-darwin"
  end
  def install
    bin.install "hebbs"
  end
end
```

**Formula: hebbs-cli.rb** (REST client only)
```ruby
class HebbsCli < Formula
  desc "HEBBS CLI client for enterprise servers"
  version "0.4.0"
  if Hardware::CPU.intel?
    url "https://github.com/hebbs-ai/hebbs-cli/releases/download/v0.4.0/hebbs-cli-x86_64-apple-darwin"
  else
    url "https://github.com/hebbs-ai/hebbs-cli/releases/download/v0.4.0/hebbs-cli-aarch64-apple-darwin"
  end
  def install
    bin.install "hebbs-cli" => "hebbs"
  end
end
```

### Phase 5: Python SDK on PyPI

Update `hebbs-python/pyproject.toml`:
```toml
[project.optional-dependencies]
rest = ["aiohttp>=3.9"]
```

```sh
pip install hebbs[rest]
```

### Phase 6: CI/CD (GitHub Actions)

**On tag push (v0.4.0):**
1. Build Rust binaries for 4 platforms (full + cli = 8 binaries)
2. Build Docker images (engine + platform)
3. Push binaries to GitHub Releases
4. Push Docker images to GHCR
5. Update Homebrew formulae
6. Publish Python SDK to PyPI

### Phase 7: Customer README

One-page getting started:

```
HEBBS Enterprise: Quick Start

1. Server Setup
   docker login ghcr.io -u hebbs-customer -p <your-token>
   curl -sL https://install.hebbs.ai/server | sh
   cd hebbs-enterprise
   echo "OPENAI_API_KEY=sk-..." > .env
   docker compose up -d
   Open http://your-server:8080

2. Developer Setup
   curl -sSf https://hebbs.ai/install | sh
   hebbs login --endpoint https://your-server:8080
   hebbs recall "your query"

3. Python SDK
   pip install hebbs[rest]

4. Support
   https://hebbs.ai/support
```

## Delete

- `hb/` directory (TypeScript CLI, replaced by Rust hebbs-cli with REST)

## Order of execution

| # | Task | Depends on | Effort |
|---|------|-----------|--------|
| 1 | Add REST transport to hebbs-cli | None | 1 session |
| 2 | Build + push Docker images to GHCR | M1 (GHCR token) | 30 min |
| 3 | Create production docker-compose.yml + .env.example + README | Phase 2 | 30 min |
| 4 | Create install script | Phase 1 | 15 min |
| 5 | Update Homebrew tap | Phase 1 | 15 min |
| 6 | Update Python SDK pyproject.toml | None | 15 min |
| 7 | Set up GitHub Actions CI | Phases 1-5 | 1 session |
| 8 | Delete TypeScript hb/ | Phase 1 verified | 5 min |

## Manual steps (you)

| # | Task |
|---|------|
| M1 | Create GHCR access token with `write:packages` |
| M2 | Set up install.hebbs.ai (static host for install script + compose file) |
| M3 | Per customer: generate read token, send one-pager |
