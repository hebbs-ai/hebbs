# HEBBS Enterprise: Getting Started

Welcome to HEBBS, the cognitive memory engine for AI agents. This guide walks you through deploying HEBBS on your infrastructure and getting your first recall working.

## Quick Start (one line)

```sh
curl -sSf https://hebbs.ai/server | OPENAI_API_KEY=sk-your-key sh
```

This installs and starts HEBBS Enterprise. Open `http://your-server:8080` when it finishes.

**Prerequisites:** Linux (Ubuntu 22.04+) or macOS, Docker Engine 24+, 4 vCPUs / 4 GB RAM, port 8080 open.

**Optional overrides:** `HEBBS_PORT=9090`, `HEBBS_DIR=/opt/hebbs`, `HEBBS_LLM_MODEL=gpt-4o`, `HEBBS_EMBED_MODEL=text-embedding-3-large`.

Skip to [Step 5](#step-5-open-the-dashboard) after the installer finishes.

---

## Manual Setup

If you prefer to set things up manually, follow these steps instead.

### Prerequisites

- A Linux machine (Ubuntu 22.04+ recommended) with Docker Engine 24+ and Docker Compose v2
- 4 vCPUs, 4 GB RAM minimum
- An OpenAI API key with access to gpt-4o-mini and text-embedding-3-small
- Port 8080 open for your team to access the dashboard and API

### Step 1: Download the deployment package

```sh
mkdir hebbs && cd hebbs

curl -sL -o docker-compose.yml https://raw.githubusercontent.com/hebbs-ai/hebbs-enterprise/main/docker-compose.yml

curl -sL -o .env.example https://raw.githubusercontent.com/hebbs-ai/hebbs-enterprise/main/.env.example
```

### Step 2: Configure your environment

```sh
cp .env.example .env
```

Edit `.env` and set your OpenAI API key:

```
OPENAI_API_KEY=sk-proj-your-actual-key-here
```

### Step 3: Start HEBBS

```sh
docker compose up -d
```

This pulls two images (~200MB each) and starts the engine and platform. First startup takes about 60 seconds while the engine initializes.

### Step 4: Verify

```sh
docker compose ps
```

You should see:

```
NAME       STATUS          PORTS
engine     Up (healthy)    6381/tcp
platform   Up              0.0.0.0:8080->8080/tcp
```

## Step 5: Open the dashboard

Open your browser and go to:

```
http://your-server-ip:8080
```

The onboarding wizard appears on first visit:

1. **Create your admin account**: enter your email and a password
2. **Name your first workspace**: e.g., "support-agent" or "legal-docs"
3. **Verify connection**: confirms OpenAI is connected and the engine is healthy
4. **Save your API key**: the wizard generates a workspace API key. Copy and save it. This key is shown only once.

Click "Go to Dashboard" to complete setup.

## Step 6: Upload your documents

From the dashboard:

1. Click your workspace
2. Go to the **Files** tab
3. Click **Upload Files** and select your markdown, text, or PDF files
4. The progress bar shows indexing status. Each file takes 30-60 seconds depending on size.

Or from the command line (see Step 8 for CLI setup):

```sh
hebbs push ./your-docs-folder
```

## Step 7: Search your data

From the dashboard:

1. Go to the **Search** tab
2. Type a natural language query and click **Go**
3. Results show with relevance scores and source file information

Or from the command line:

```sh
hebbs recall "your search query"
```

Or from your Python agent:

```python
results = await hb.recall("your search query")
print(results.text)
```

## Step 8: Install the CLI (for your developers)

Each developer on your team can install the HEBBS CLI on their laptop:

```sh
curl -sSf https://hebbs.ai/install | sh
```

Then connect to your server:

```sh
hebbs login --endpoint http://your-server-ip:8080 --api-key <your-workspace-key>
```

Test the connection:

```sh
hebbs status
```

Common CLI commands:

```sh
hebbs recall "your question"           # Search memories
hebbs remember "important fact"        # Store a memory
hebbs push ./docs                      # Upload documents
hebbs prime customer-42                # Load all context for an entity
```

## Step 9: Integrate the Python SDK (for your AI agents)

```sh
pip install hebbs[rest]
```

```python
import asyncio
from hebbs.rest_client import HebbsRestClient

async def main():
    async with HebbsRestClient(
        "http://your-server-ip:8080",
        api_key="hb_live_sk_your-key-here"
    ) as hb:
        # Store a memory
        await hb.remember(
            "Customer prefers email over phone",
            entity_id="customer-42",
            importance=0.8
        )

        # Search
        results = await hb.recall("customer contact preference")
        print(results.text)

        # Load all context for an entity
        context = await hb.prime("customer-42")
        print(context.text)

asyncio.run(main())
```

## Step 9b: Integrate the TypeScript SDK (alternative)

```sh
npm install @hebbs/sdk
```

```typescript
import { HebbsRestClient } from '@hebbs/sdk';

const hb = new HebbsRestClient("http://your-server-ip:8080", {
  apiKey: "hb_live_sk_your-key-here"
});

// Store a memory
await hb.remember("Customer prefers email over phone", {
  entityId: "customer-42",
  importance: 0.8
});

// Search
const results = await hb.recall("customer contact preference");
console.log(results.text);

// Load all context for an entity
const context = await hb.prime("customer-42");
console.log(context.text);

await hb.close();
```

## Step 10: Add team members

From the dashboard:

1. Click **Team** in the top navigation
2. Click **+ Add Member**
3. Enter their email, set a password, choose role (Admin or Developer)
4. Assign them to one or more workspaces
5. Share their credentials. They log in at `http://your-server-ip:8080`

Developers can see their assigned workspaces, upload files, and search. They cannot access Settings or manage other team members.

## Step 11: Create additional workspaces (optional)

Each workspace is an isolated memory vault. Use separate workspaces for different teams or use cases:

1. Click **+ New Workspace** on the dashboard home
2. Name it (e.g., "sales-agent", "legal-research")
3. Save the generated API key
4. Assign team members to the workspace

Data is completely isolated between workspaces. A search in one workspace never returns results from another.

## Common Operations

### View logs

```sh
docker compose logs -f          # All services
docker compose logs -f engine   # Engine only
docker compose logs -f platform # Platform only
```

### Check health

```sh
curl http://your-server-ip:8080/v1/system/health
```

### Upgrade to a new version

```sh
docker compose pull
docker compose up -d
```

### Backup

```sh
docker compose stop
tar czf hebbs-backup-$(date +%Y%m%d).tar.gz \
  -C /var/lib/docker/volumes/hebbs_hebbs-data/_data .
docker compose start
```

### Restore from backup

```sh
docker compose stop
tar xzf hebbs-backup-YYYYMMDD.tar.gz \
  -C /var/lib/docker/volumes/hebbs_hebbs-data/_data/
docker compose start
```

## Troubleshooting

**"Cannot connect" when using CLI or SDK:**
- Verify port 8080 is open in your firewall / security group
- Check that both containers are running: `docker compose ps`
- Verify the engine is healthy: `docker compose logs engine | tail -5`

**"Invalid or revoked API key":**
- API keys are workspace-scoped. Make sure you are using the key for the correct workspace.
- Admin keys (starting with `hb_admin_sk_`) can access all workspaces.
- Workspace keys (starting with `hb_live_sk_`) can only access their assigned workspace.

**Indexing seems stuck:**
- Check engine logs: `docker compose logs -f engine`
- Indexing depends on OpenAI API. Verify your API key has sufficient quota.
- Each file takes 30-60 seconds (embedding + LLM extraction).

**"RocksDB lock" error:**
- This should not happen with v0.4.0+. If it does, restart the engine: `docker compose restart engine`

## Support

For help, feature requests, or to report issues: https://hebbs.ai
