# HEBBS Enterprise: Deployment

## Overview

The enterprise deployment is a single Docker Compose package that runs on the customer's machine. Everything is included. The customer provides a machine and an OpenAI API key. We do the rest.

---

## Requirements

### Customer provides

| Requirement | Minimum | Recommended |
|---|---|---|
| Machine | VM or bare metal with Docker | Dedicated VM |
| OS | Linux (any distro with Docker) | Ubuntu 22.04+ |
| CPU | 2 vCPUs | 4 vCPUs |
| RAM | 2 GB | 4 GB |
| Disk | 10 GB | 50 GB (grows with doc volume) |
| Docker | Docker Engine 24+ | Latest |
| Docker Compose | v2.20+ | Latest |
| Network | Outbound HTTPS (for OpenAI API) | + inbound on port 8080 for team access |
| OpenAI API key | Active account with API access | |

### Optional

| Requirement | Purpose |
|---|---|
| Domain name | `hebbs.customer.com` instead of IP:port |
| TLS certificate | HTTPS for the dashboard and API |
| Static IP | Stable endpoint for SDK/CLI clients |

---

## Docker Compose package

### What's included

```
hebbs-enterprise/
├── docker-compose.yml       # Orchestrates both containers
├── .env.example             # Template — customer copies to .env
├── README.md                # Quick start instructions
└── tls/                     # Optional: place TLS certs here
    ├── cert.pem
    └── key.pem
```

### `docker-compose.yml`

```yaml
version: "3.8"

services:
  platform:
    image: ghcr.io/hebbs-ai/hebbs-platform:latest
    ports:
      - "${HEBBS_PORT:-8080}:8080"
    environment:
      - HEBBS_ENGINE_URL=http://engine:6381
      - HEBBS_ENGINE_GRPC=engine:6380
      - HEBBS_CENTRAL_URL=${HEBBS_CENTRAL_URL:-https://central.hebbs.ai}
      - HEBBS_DEPLOYMENT_ID=${HEBBS_DEPLOYMENT_ID}
      - HEBBS_HEARTBEAT_ENABLED=${HEBBS_HEARTBEAT_ENABLED:-true}
      - HEBBS_TLS_CERT=/tls/cert.pem
      - HEBBS_TLS_KEY=/tls/key.pem
    volumes:
      - hebbs-data:/data
      - ./tls:/tls:ro
    depends_on:
      engine:
        condition: service_healthy
    restart: unless-stopped

  engine:
    image: ghcr.io/hebbs-ai/hebbs-server:latest
    environment:
      - HEBBS_LLM_PROVIDER=openai
      - HEBBS_LLM_MODEL=${HEBBS_LLM_MODEL:-gpt-4o-mini}
      - HEBBS_LLM_API_KEY=${OPENAI_API_KEY}
      - HEBBS_EMBED_PROVIDER=openai
      - HEBBS_EMBED_MODEL=${HEBBS_EMBED_MODEL:-text-embedding-3-small}
      - HEBBS_EMBED_API_KEY=${OPENAI_API_KEY}
      - HEBBS_EMBED_DIMENSIONS=1536
      - HEBBS_BIND_REST=0.0.0.0:6381
      - HEBBS_BIND_GRPC=0.0.0.0:6380
      - HEBBS_DATA_DIR=/data
    volumes:
      - hebbs-data:/data
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:6381/v1/health/live"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 10s
    restart: unless-stopped
    # Not exposed to host — only reachable via platform

volumes:
  hebbs-data:
    driver: local
```

### `.env.example`

```sh
# Required
OPENAI_API_KEY=sk-proj-your-key-here

# Deployment identity (we provide this)
HEBBS_DEPLOYMENT_ID=acme-corp-001

# Optional: change the port (default 8080)
HEBBS_PORT=8080

# Optional: LLM model overrides
HEBBS_LLM_MODEL=gpt-4o-mini
HEBBS_EMBED_MODEL=text-embedding-3-small

# Optional: disable heartbeat to our central dashboard
HEBBS_HEARTBEAT_ENABLED=true

# Optional: central dashboard URL (default: https://central.hebbs.ai)
HEBBS_CENTRAL_URL=https://central.hebbs.ai
```

---

## Deployment steps

### 1. We prepare

Before the deployment session:
- Generate a `HEBBS_DEPLOYMENT_ID` for the customer
- Prepare the Docker Compose package
- Get the customer's OpenAI API key (or they enter it during onboarding)

### 2. Deploy

```sh
# On customer's machine
mkdir /opt/hebbs && cd /opt/hebbs

# We provide the package (download, USB, git clone — whatever works)
cp -r hebbs-enterprise/* .

# Customer creates .env from template
cp .env.example .env
# Edit .env: set OPENAI_API_KEY and HEBBS_DEPLOYMENT_ID

# Start everything
docker compose up -d

# Verify
docker compose ps
#   NAME       STATUS    PORTS
#   platform   Up        0.0.0.0:8080->8080/tcp
#   engine     Up (healthy)

# Check health
curl http://localhost:8080/v1/system/health
# {"status": "ok", "version": "0.3.3", "engine": "healthy"}
```

### 3. First access

Customer opens `http://<machine-ip>:8080` (or `https://hebbs.customer.com` if domain configured).

Onboarding wizard runs (see [04-platform-services.md](04-platform-services.md)):
- Create admin account
- Name first workspace
- Confirm OpenAI connection
- Get API key + endpoint

### 4. Networking

#### Option A: No domain (IP access only)

The machine is accessed by IP. Open port 8080 on the firewall.

```
Team members connect to: http://<machine-ip>:8080
SDK endpoint:            http://<machine-ip>:8080
CLI:                     hb login --endpoint http://<machine-ip>:8080
```

**Firewall rules needed:**

| Direction | Port | Protocol | Purpose |
|---|---|---|---|
| Inbound | 8080 | TCP | Dashboard + API (for team members, agent servers) |
| Outbound | 443 | TCP | OpenAI API calls |
| Outbound | 443 | TCP | Heartbeat to central.hebbs.ai (optional) |

Engine ports (6380, 6381) are internal to Docker — never exposed to the host.

**Security note:** Without TLS, API keys and data travel in plaintext. Acceptable for internal networks / VPNs. For anything over the internet, use Option B.

#### Option B: Domain with nginx + certbot (recommended)

The customer points a domain (e.g., `hebbs.acme.com`) at the machine and uses nginx as a reverse proxy with Let's Encrypt TLS.

**Step 1: DNS**

Point `hebbs.acme.com` → machine's public IP (A record).

**Step 2: Install nginx**

```sh
# Ubuntu/Debian
sudo apt update && sudo apt install -y nginx
```

**Step 3: Configure nginx**

```sh
sudo tee /etc/nginx/sites-available/hebbs <<'EOF'
server {
    listen 80;
    server_name hebbs.acme.com;

    # Redirect HTTP → HTTPS (certbot will handle this after setup)
    location / {
        return 301 https://$host$request_uri;
    }
}

server {
    listen 443 ssl http2;
    server_name hebbs.acme.com;

    # TLS certs (certbot fills these in)
    ssl_certificate /etc/letsencrypt/live/hebbs.acme.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/hebbs.acme.com/privkey.pem;

    # Proxy everything to hebbs-platform
    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSocket support (for Memory Palace real-time updates)
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }

    # File upload size (for pushing docs)
    client_max_body_size 100M;
}
EOF

sudo ln -s /etc/nginx/sites-available/hebbs /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
```

**Step 4: Install certbot and get TLS certificate**

```sh
sudo apt install -y certbot python3-certbot-nginx
sudo certbot --nginx -d hebbs.acme.com

# Certbot will:
# 1. Verify domain ownership
# 2. Issue Let's Encrypt certificate
# 3. Update nginx config with cert paths
# 4. Set up auto-renewal (cron)
```

**Step 5: Verify**

```sh
curl https://hebbs.acme.com/v1/system/health
# {"status": "ok", "version": "0.3.3", "engine": "healthy"}
```

**Firewall rules with domain:**

| Direction | Port | Protocol | Purpose |
|---|---|---|---|
| Inbound | 80 | TCP | HTTP → HTTPS redirect (certbot renewal) |
| Inbound | 443 | TCP | HTTPS — dashboard + API |
| Outbound | 443 | TCP | OpenAI API calls |
| Outbound | 443 | TCP | Heartbeat to central.hebbs.ai (optional) |

Port 8080 can be restricted to localhost only (`127.0.0.1:8080`) since nginx handles external traffic.

**Update docker-compose.yml to bind only to localhost:**

```yaml
services:
  platform:
    ports:
      - "127.0.0.1:8080:8080"   # Only accessible via nginx, not directly
```

**Certificate renewal** is automatic via certbot's cron/systemd timer. Verify with:

```sh
sudo certbot renew --dry-run
```

#### Option C: Customer's existing reverse proxy / load balancer

If the customer already has infrastructure (AWS ALB, Cloudflare, corporate proxy), they just point it at port 8080. They handle TLS their way. We don't need to configure anything extra.

```
Customer's proxy (TLS termination)
  → forwards to machine:8080
  → hebbs-platform handles the rest
```

---

## Operations

### Logs

```sh
# All logs
docker compose logs -f

# Engine only
docker compose logs -f engine

# Platform only
docker compose logs -f platform
```

### Backup

```sh
# Stop (optional, for consistency)
docker compose stop

# Backup the data volume
tar czf hebbs-backup-$(date +%Y%m%d).tar.gz -C /var/lib/docker/volumes/hebbs_hebbs-data/_data .

# Start
docker compose start
```

Or use Docker volume backup tools. The key directory is the `hebbs-data` volume.

### Restore

```sh
docker compose stop
# Clear existing data
rm -rf /var/lib/docker/volumes/hebbs_hebbs-data/_data/*
# Restore
tar xzf hebbs-backup-20260328.tar.gz -C /var/lib/docker/volumes/hebbs_hebbs-data/_data/
docker compose start
```

### Upgrade

```sh
# Pull new images
docker compose pull

# Restart with new images (zero downtime for short restarts)
docker compose up -d

# Verify
curl http://localhost:8080/v1/system/health
```

RocksDB recovers from WAL on restart. No data migration needed for minor versions.

### Resource monitoring

```sh
# Container resource usage
docker stats

#   NAME       CPU    MEM     NET I/O     BLOCK I/O
#   engine     2.1%   340MB   1.2MB/s     500KB/s
#   platform   0.3%   120MB   200KB/s     10KB/s
```

---

## Scaling

### When to scale up

| Signal | Current | Upgrade to |
|---|---|---|
| Recall latency > 200ms consistently | 2 vCPU, 2GB | 4 vCPU, 4GB |
| Disk > 80% | 10GB | 50GB+ |
| Memory > 80% | 2GB | 4GB+ (RocksDB is memory-mapped) |
| >100k memories per workspace | Standard | Consider dedicated disk IOPS |

### Horizontal scaling

Not supported in enterprise single-node deployment. For customers who outgrow a single machine, the path is:
- Vertical scaling first (bigger machine)
- Then: SaaS migration or custom multi-node deployment (future)

---

## Firewall summary

See "Networking" section above for detailed port requirements per option.

Engine ports (6380, 6381) are **always internal to Docker** — never exposed to the host network regardless of networking option.
