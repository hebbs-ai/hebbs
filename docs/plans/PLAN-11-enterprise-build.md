# PLAN-11: Enterprise Build

Build the HEBBS Enterprise product on an AWS Ubuntu machine. Port 8080 open, no domain until everything is tested.

**Spec:** [docs/enterprise/product/](../enterprise/product/README.md)
**Target:** Onboard enterprise customers.
**Approach:** Build → test on the machine → move to next phase. Each phase has a concrete test you can run from your laptop against `http://<machine-ip>:8080`.

**Key decision:** Use `docker-compose.yml` from the start. Phase 1 has engine only. Later phases update the compose file. Each phase test starts clean — `docker compose down -v` to wipe volumes, then `docker compose up` fresh. This ensures we're testing the real deployment experience, not accumulated state.

**Clean slate per phase:**
```sh
docker compose down -v    # stop + remove volumes
docker compose up -d      # fresh start
```

**Prerequisites on the Ubuntu machine:**
- Docker Engine 24+ and Docker Compose v2
- Git (to clone repos)
- OpenAI API key
- Port 8080 open in security group

---

## Phase 1: Engine in Docker Compose

**Goal:** `hebbs-server` runs via docker-compose on the Ubuntu machine. REST API + Memory Palace accessible on port 8080 (mapped from engine's 6381).

**Build:**

- Clone the engine repo on the machine
- Write Dockerfile for `hebbs-server` (OpenAI only, no ONNX). See [09-deployment.md](../enterprise/product/09-deployment.md).
- Write `docker-compose.yml` with engine service only (port 8080 → 6381)
- Write `.env` with `OPENAI_API_KEY`
- Build image, `docker compose up`
- Run `hebbs init` inside container to create first vault

**`docker-compose.yml` (Phase 1):**
```yaml
services:
  engine:
    build: ./hebbs
    ports:
      - "8080:6381"      # engine directly on 8080 for now
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

volumes:
  hebbs-data:
```

**Test from laptop (clean start, remote, via port 8080):**

```sh
# On the machine: clean slate
docker compose down -v && docker compose up -d
# Wait for health check to pass

# From laptop:
# Health check
curl http://<machine-ip>:8080/v1/health/live

# Remember something
curl -X POST http://<machine-ip>:8080/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"content": "Test memory", "importance": 0.5}'

# Recall it
curl -X POST http://<machine-ip>:8080/v1/recall \
  -H "Content-Type: application/json" \
  -d '{"cue": "test"}'

# Open Memory Palace in browser
# http://<machine-ip>:8080
```

**Pass criteria:**

- [ ] Health endpoint returns 200
- [ ] Remember returns a memory_id
- [ ] Recall returns the memory we just stored
- [ ] Memory Palace loads in browser and shows the memory

---

## Phase 2: File indexing in Docker

**Goal:** Push markdown files into the container volume. Daemon auto-indexes them. Recall returns content from indexed files.

**Build:**

- Clone demo data: `git clone git@github.com:hebbs-ai/demos.git` on the Ubuntu machine
- Copy `enterprise-legal-mini` files into the container's data volume
- Or: mount the cloned directory as a host volume
- Run `hebbs index` inside container (or let daemon auto-detect)

**Build:**

```sh
# On the Ubuntu machine — clone demo data
git clone git@github.com:hebbs-ai/demos.git ~/hebbs-demos

# Copy test files into the running container's data volume
docker cp ~/hebbs-demos/enterprise-legal-mini/. $(docker compose ps -q engine):/data/docs/

# Or: add a bind mount in docker-compose.yml for testing:
#   volumes:
#     - ~/hebbs-demos/enterprise-legal-mini:/data/docs:ro
```

**Test (clean start, then load files):**

```sh
# On the machine: clean slate
docker compose down -v && docker compose up -d
# Wait for engine healthy

# Copy demo files into container
docker cp ~/hebbs-demos/enterprise-legal-mini/. $(docker compose ps -q engine):/data/docs/

# Wait for daemon to index (check: docker compose logs -f engine | grep -i index)

# Check index status
curl http://<machine-ip>:8080/v1/health

# Recall from indexed content
curl -X POST http://<machine-ip>:8080/v1/recall \
  -H "Content-Type: application/json" \
  -d '{"cue": "ransomware coverage"}'

# Verify propositions were extracted (not just embeddings)
curl http://<machine-ip>:8080/v1/entities
```

**Pass criteria:**

- Daemon detects new files and indexes automatically
- Recall returns relevant results from indexed files
- Entities were extracted (entities endpoint returns data)
- Memory Palace shows indexed memories as graph nodes

---

## Phase 3: Platform — API proxy + auth

**Goal:** `hebbs-platform` sits in front of the engine. Validates API keys, proxies data-plane requests to engine. Port 8080 exposed to host.

**Build:**

- Create `hebbs-platform` project (language TBD — Rust, Go, or Python)
- API key validation (store hashed keys in SQLite)
- Proxy all `/v1/memories`, `/v1/recall`, `/v1/prime`, `/v1/forget`, `/v1/upload`, `/v1/health` to engine on port 6381
- File upload endpoint: receive multipart → write to engine's volume → daemon indexes
- Bootstrap: on first start, generate admin API key, print to stdout
- **Update the existing `docker-compose.yml`:** add platform service, move port 8080 from engine to platform, engine becomes internal-only

**docker-compose.yml update (Phase 3):**
```yaml
services:
  platform:
    build: ./hebbs-platform
    ports:
      - "8080:8080"              # platform takes over port 8080
    environment:
      - HEBBS_ENGINE_URL=http://engine:6381
      - HEBBS_ENGINE_GRPC=engine:6380
    volumes:
      - hebbs-data:/data
    depends_on:
      engine:
        condition: service_healthy
    restart: unless-stopped

  engine:
    build: ./hebbs
    # ports removed — no longer exposed to host, only reachable via platform
    environment:
      ...same as Phase 1...
    volumes:
      - hebbs-data:/data
    healthcheck:
      ...same as Phase 1...
    restart: unless-stopped
```

`docker compose up` — same port 8080, now served by platform instead of engine directly. No reinstall needed.

**Test (clean start with platform + engine):**

```sh
# On the machine: clean slate with updated docker-compose.yml (now includes platform)
docker compose down -v && docker compose build && docker compose up -d
# Platform starts, waits for engine health, prints bootstrap API key to logs
# Grab the key: docker compose logs platform | grep "API key"

# From laptop:
# Health
curl http://<machine-ip>:8080/v1/system/health

# Recall WITHOUT api key → 401
curl -X POST http://<machine-ip>:8080/v1/recall \
  -d '{"cue": "test"}'
# Expected: 401 Unauthorized

# Recall WITH api key → 200
curl -X POST http://<machine-ip>:8080/v1/recall \
  -H "Authorization: Bearer hb_live_sk_<bootstrap-key>" \
  -H "Content-Type: application/json" \
  -d '{"cue": "ransomware coverage"}'
# Expected: results from indexed files

# Remember via platform
curl -X POST http://<machine-ip>:8080/v1/memories \
  -H "Authorization: Bearer hb_live_sk_<bootstrap-key>" \
  -H "Content-Type: application/json" \
  -d '{"content": "Test via platform", "importance": 0.7}'

# Upload a file via platform
curl -X POST http://<machine-ip>:8080/v1/upload \
  -H "Authorization: Bearer hb_live_sk_<bootstrap-key>" \
  -F "files=@test-doc.md"

# Wait, then recall from uploaded file
curl -X POST http://<machine-ip>:8080/v1/recall \
  -H "Authorization: Bearer hb_live_sk_<bootstrap-key>" \
  -H "Content-Type: application/json" \
  -d '{"cue": "content from the uploaded file"}'

# Prime
curl -X POST http://<machine-ip>:8080/v1/prime \
  -H "Authorization: Bearer hb_live_sk_<bootstrap-key>" \
  -H "Content-Type: application/json" \
  -d '{"entity_id": "test-entity"}'
```

**Pass criteria:**

- Requests without API key → 401
- Requests with valid API key → proxied to engine, correct results
- File upload → file lands in engine volume → daemon indexes → recallable
- `docker compose up` starts both containers, platform waits for engine health
- Bootstrap API key printed on first start

---

## Phase 4: Platform — workspace management

**Goal:** Multiple workspaces. Each workspace has its own vault, own API keys. Workspace-scoped API keys only access their workspace.

**Build:**

- Workspace CRUD API: `POST /v1/workspaces`, `GET /v1/workspaces`, `DELETE /v1/workspaces/:slug`
- Each workspace → separate vault in engine (via `hebbs init`)
- API key scoping: key is tied to a workspace
- Workspace creation returns new API key
- File uploads scoped to workspace
- Recall scoped to workspace

**Test (clean start, create workspaces from scratch):**

```sh
# On the machine: clean slate
docker compose down -v && docker compose build && docker compose up -d
# Grab bootstrap admin key from logs

# From laptop:
ADMIN_KEY="hb_admin_sk_<bootstrap>"

# Create workspace
curl -X POST http://<machine-ip>:8080/v1/workspaces \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "support-agent"}'
# Returns: workspace + api_key

SUPPORT_KEY="hb_live_sk_<returned-key>"

# Create second workspace
curl -X POST http://<machine-ip>:8080/v1/workspaces \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "sales-agent"}'

SALES_KEY="hb_live_sk_<returned-key>"

# Remember in support workspace
curl -X POST http://<machine-ip>:8080/v1/memories \
  -H "Authorization: Bearer $SUPPORT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"content": "Support fact: password reset via Settings", "importance": 0.7}'

# Remember in sales workspace
curl -X POST http://<machine-ip>:8080/v1/memories \
  -H "Authorization: Bearer $SALES_KEY" \
  -H "Content-Type: application/json" \
  -d '{"content": "Sales fact: enterprise plan is $10k/year", "importance": 0.7}'

# Recall from support → should NOT see sales fact
curl -X POST http://<machine-ip>:8080/v1/recall \
  -H "Authorization: Bearer $SUPPORT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"cue": "enterprise plan pricing"}'
# Expected: empty or irrelevant (not the sales fact)

# Recall from sales → should see sales fact
curl -X POST http://<machine-ip>:8080/v1/recall \
  -H "Authorization: Bearer $SALES_KEY" \
  -H "Content-Type: application/json" \
  -d '{"cue": "enterprise plan pricing"}'
# Expected: "Sales fact: enterprise plan is $10k/year"

# List workspaces
curl http://<machine-ip>:8080/v1/workspaces \
  -H "Authorization: Bearer $ADMIN_KEY"
```

**Pass criteria:**

- Create workspace returns workspace + API key
- Workspace-scoped key can only access its own workspace
- Memories are isolated between workspaces (recall in one doesn't return other's data)
- List workspaces shows all workspaces with stats
- File upload is workspace-scoped

---

## Phase 5: Platform — dashboard UI

**Goal:** Web dashboard at `http://<machine-ip>:8080`. Onboarding wizard, workspace list, workspace detail, Memory Palace link, API key management, config.

**Build:**

- See [04-platform-services.md](../enterprise/product/04-platform-services.md) for full spec
- Onboarding wizard (first-time only): create admin account, name workspace, configure OpenAI, test connection
- Dashboard home: workspace list with memory count, file count, status
- Workspace detail: stats, indexing status, Memory Palace link
- API key management: create, revoke, list
- Config page: OpenAI settings, concurrency, decay, reflection
- Account management: create developer accounts, assign roles

**Test (clean start, full onboarding):**

```sh
# On the machine: clean slate
docker compose down -v && docker compose build && docker compose up -d
```

```
From laptop browser:

1. Open http://<machine-ip>:8080
   → Should show onboarding wizard (first time)

2. Complete onboarding:
   - Create admin account (email + password)
   - Name workspace: "test-workspace"
   - OpenAI key already configured via .env
   - Test connection → green checkmark
   → Dashboard loads

3. Dashboard shows:
   - test-workspace with memory count, file count
   - System health: green
   - OpenAI: connected

4. Click into workspace:
   - Memory count, entity list, indexing status
   - "Open Memory Palace" → loads Memory Palace graph
   - API keys section → create a new key, copy it

5. Create second workspace from dashboard
   → Appears in list

6. Settings page:
   - OpenAI config visible
   - Concurrency setting visible and changeable
   - Decay half-life visible
```

**Pass criteria:**

- Onboarding wizard completes successfully on first visit
- Dashboard shows workspace list with correct stats
- Memory Palace accessible per workspace via dashboard link
- API keys can be created and revoked from dashboard
- Config settings are readable and writable
- Second visit skips onboarding, goes straight to dashboard

---

## Phase 6: `hb` CLI — remote client

**Goal:** The `hb` CLI works from a laptop, talking to the HEBBS server over port 8080.

**Build:**

- See [07-cli-spec.md](../enterprise/product/07-cli-spec.md) for full spec
- Commands: login, push, sync, recall, remember, forget, prime, workspaces, status, dashboard
- Talks REST to the platform on port 8080
- Stores endpoint + token in `~/.hb/config`
- Multi-workspace: prompt if multiple exist and none selected

**Test (against running server — clean slate optional, or reuse Phase 5 state):**

```sh
# From laptop:
# Login
hb login --endpoint http://<machine-ip>:8080
# → authenticates, saves config

# Status
hb status
# → shows workspace name, memory count, indexing status

# Push docs
hb push ./test-docs
# → "Pushing to workspace: test-workspace"
# → uploads files, shows progress

# Wait for indexing
hb status
# → "Files indexed: 5/5"

# Recall
hb recall "password reset"
# → shows results with scores

# Remember
hb remember "CLI test fact" --entity-id test-entity --importance 0.7
# → returns memory_id

# Prime
hb prime test-entity
# → returns all memories for test-entity

# Sync
hb sync ./test-docs --watch
# → watches for changes, auto-pushes
# → edit a file, verify it re-uploads

# Workspaces
hb workspaces list
# → shows all workspaces
hb workspaces create another-workspace
# → creates, returns API key
hb workspaces switch another-workspace
# → switches context

# Dashboard
hb dashboard
# → opens browser to http://<machine-ip>:8080/workspaces/...
```

**Pass criteria:**

- Login stores endpoint + credentials
- Push uploads files and triggers indexing
- Recall returns results from indexed content
- Remember stores and is immediately recallable
- Prime returns all memories for entity
- Sync --watch detects file changes and pushes
- Workspace create/switch works
- Multi-workspace prompt works when no current set
- All output shows workspace name

---

## Phase 7: Python SDK — REST transport

**Goal:** Python SDK connects to the HEBBS server via REST, all methods work.

**Build:**

- See [06-sdk-spec.md](../enterprise/product/06-sdk-spec.md) for full spec
- Add REST transport to `hebbs-python`
- `endpoint` param on constructor
- Methods: recall, remember, prime, revise, forget, insights, index, status
- `.text` property on recall/prime/insights results
- Indexing status on recall results

**Test (against running server — clean slate optional, or reuse existing state):**

```python
from hebbs import Hebbs

hb = Hebbs(api_key="hb_live_sk_...", endpoint="http://<machine-ip>:8080")

# Status
info = hb.status()
print(f"Memories: {info.memories}, Files: {info.files}")

# Recall
result = hb.recall("password reset", entity_id="user_42")
print(f"Results: {len(result.memories)}")
print(f"Text: {result.text}")
print(f"Indexing: {result.indexing}")

# Remember
mem = hb.remember("SDK test fact", entity_id="user_42", importance=0.7)
print(f"Stored: {mem.id}")

# Prime
ctx = hb.prime(entity_id="user_42")
print(f"Context: {ctx.text}")

# Revise
hb.revise(memory_id=mem.id, content="SDK test fact (revised)", importance=0.8)

# Insights
profile = hb.insights(entity_id="user_42")
print(f"Profile: {profile.text}")

# Index (push files)
hb.index("./test-docs")

# Forget
hb.forget(entity_id="test-cleanup")
```

**Pass criteria:**

- Constructor with endpoint connects to remote server
- recall() returns memories with .text and .indexing
- remember() stores and returns memory_id
- prime() returns all memories for entity
- revise() updates memory with revision chain
- insights() returns entity profile
- index() uploads files and triggers indexing
- forget() deletes memories
- Auth errors return proper exceptions

---

## Phase 8: End-to-end customer journey

**Goal:** Walk through the entire [customer journey](../enterprise/product/02-customer-journey.md) on the Ubuntu machine. Everything works together.

**Test sequence (MUST start from clean slate — this is the real customer experience):**

```
0. On the machine: docker compose down -v && docker compose build && docker compose up -d
1. From laptop: open http://<machine-ip>:8080 (nothing cached, nothing pre-existing)
2. Onboarding wizard appears (first-time experience)
3. Create admin, name workspace "support-agent", verify OpenAI
4. From laptop: hb login --endpoint http://<machine-ip>:8080
5. hb push ~/hebbs-demos/enterprise-legal-mini
6. hb status → indexing in progress
7. hb recall "ransomware coverage" → partial results or indexing message
8. Wait for indexing → hb status shows complete
9. hb recall "ransomware coverage" → full results
10. Open dashboard in browser → workspace shows memory count, entities
11. Click Memory Palace → graph with nodes and edges
12. From Python SDK:
    - hb.recall("vendor risk") → results
    - hb.remember("SOC2 audit passed", entity_id="compliance", importance=0.8)
    - hb.prime(entity_id="compliance") → returns the memory
    - hb.insights(entity_id="compliance") → profile (may need reflection to run first)
13. Create second workspace "sales-agent" from dashboard
14. hb workspaces switch sales-agent
15. hb push ./different-docs
16. Verify isolation: recall in support doesn't see sales data
17. Tune: load tune skill in Claude, run tune process against the server
    - Agent uses hb.recall() to test queries
    - Agent uses hb.remember() to store retrieval instructions
    - Agent exports rules file
18. Dashboard shows both workspaces, correct stats, Memory Palace works for both
```

**Pass criteria:**

- Full journey completes without errors
- Indexing status visible throughout
- Workspace isolation confirmed
- Memory Palace works for each workspace
- SDK + CLI + Dashboard all work against the same server
- Tune skill works using regular recall/remember (no special tune API)
- Revise works (update a memory, verify revision chain)
- Prime loads retrieval instructions stored during tuning

---

## Phase order and dependencies

```
Phase 1 (engine in docker-compose, port 8080 direct)
  └→ Phase 2 (file indexing, same compose)
       └→ Phase 3 (add platform to compose, platform takes port 8080, engine goes internal)
            └→ Phase 4 (workspaces)
                 └→ Phase 5 (dashboard UI)
  Phase 6 (hb CLI) — can start after Phase 3
  Phase 7 (Python SDK) — can start after Phase 3
  Phase 8 (end-to-end) — after all above
```

Phases 6 and 7 can run in parallel with Phases 4-5. The dashboard UI and CLI/SDK are independent — they both talk to the same platform API.

**docker-compose.yml evolves across phases:**
- Phase 1-2: engine only, port 8080 → engine:6381
- Phase 3+: platform added, port 8080 → platform:8080, engine internal only

**Every phase test starts with `docker compose down -v && docker compose up -d`** — clean slate. This ensures each phase works from zero, not from accumulated state. The final Phase 8 is the real customer experience: fresh deploy → onboard → everything works.