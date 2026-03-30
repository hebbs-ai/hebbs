# HEBBS Enterprise

Cognitive memory engine for AI agents. Deploy on your infrastructure in minutes.

## Quick Start

### 1. Authenticate with our registry

```sh
echo "<your-token>" | docker login ghcr.io -u hebbs-customer --password-stdin
```

### 2. Configure

```sh
cp .env.example .env
# Edit .env: set your OPENAI_API_KEY
```

### 3. Start

```sh
docker compose -f docker-compose.prod.yml up -d
```

### 4. Open the dashboard

```
http://your-server:8080
```

The onboarding wizard runs on first visit. Create your admin account and first workspace.

## Developer Setup

### CLI

```sh
curl -sSf https://hebbs.ai/install | sh
hebbs login --endpoint http://your-server:8080 --api-key <your-workspace-key>
hebbs recall "your query"
hebbs push ./docs
```

### Python SDK

```sh
pip install hebbs
```

```python
from hebbs.rest_client import HebbsRestClient

async with HebbsRestClient("http://your-server:8080", api_key="hb_live_sk_...") as hb:
    results = await hb.recall("your query")
    print(results.text)
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | (required) | Your OpenAI API key |
| `HEBBS_PORT` | 8080 | Dashboard + API port |
| `HEBBS_LLM_MODEL` | gpt-4o-mini | LLM model for extraction |
| `HEBBS_EMBED_MODEL` | text-embedding-3-small | Embedding model |

## Operations

```sh
# View logs
docker compose -f docker-compose.prod.yml logs -f

# Upgrade
docker compose -f docker-compose.prod.yml pull
docker compose -f docker-compose.prod.yml up -d

# Backup
docker compose -f docker-compose.prod.yml stop
tar czf hebbs-backup-$(date +%Y%m%d).tar.gz -C /var/lib/docker/volumes/hebbs-enterprise_hebbs-data/_data .
docker compose -f docker-compose.prod.yml start
```

## Support

https://hebbs.ai
