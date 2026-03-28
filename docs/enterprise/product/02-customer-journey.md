# HEBBS Enterprise: Customer Journey

## Persona

**Acme Corp** — a company that built an AI customer support agent. The agent answers questions from product docs but is stateless. They want it to learn from conversations, remember user preferences, and stay current with their knowledge base. Data must stay on their infrastructure.

---

## Stage 1: We deploy on their machine (1 hour)

We schedule a deployment session. The customer provides a machine (VM, bare metal, cloud instance — anything that runs Docker).

**We run:**

```sh
# SSH into their machine (or they share a screen)
docker compose up -d
```

The Docker package includes everything:
- hebbs-server (engine, daemon, indexing, recall, reflection)
- hebbs-platform (dashboard, workspace management, API keys, config)
- Persistent volume for data

**We configure:**
- Customer's OpenAI API key (they provide it)
- Domain / TLS if they want external access (optional — can start with localhost)
- Admin account for the dashboard

**We set up networking** (see [09-deployment.md](09-deployment.md#4-networking)):
- With domain: nginx reverse proxy + certbot for TLS → `https://hebbs.acme.com`
- Without domain: open port 8080 → `http://<machine-ip>:8080`

**Customer opens the dashboard:**

```
https://hebbs.acme.com    (or http://<machine-ip>:8080 if no domain yet)
```

They see the onboarding wizard:

```
Welcome to HEBBS.

Step 1: Name your first workspace
  [support-agent]

Step 2: Your OpenAI API key
  [sk-proj-...]    (already configured during deployment)

Step 3: You're ready.
  Push your docs, then wire your agent.

  Workspace: support-agent
  API key: hb_live_sk_abc123...
  Endpoint: https://hebbs.acme.com
```

---

## Stage 2: Push documents (10 minutes)

The customer's engineer installs the CLI on their laptop and pushes docs to the HEBBS server.

```sh
# Install CLI
brew install hebbs-ai/tap/hb
# or: curl -sSf https://hebbs.ai/install-hb | sh

# Login to their HEBBS server
hb login --endpoint https://hebbs.acme.com
# Opens browser → authenticates → token saved
# Current workspace: support-agent

# Push docs
hb push ./docs
# Pushing to workspace: support-agent
# Uploading 34 files (245 KB)...
# Indexing will proceed in background.
```

If indexing is still in progress:

```sh
hb status
#   Workspace:       support-agent
#   Status:          indexing
#   Files uploaded:  34
#   Files indexed:   18/34
#   Memories:        412 (growing)
#   Currently:       embedding sections from policies/data-retention.md
#   Dashboard:       https://hebbs.acme.com/workspaces/support-agent/panel
```

Once complete, test recall:

```sh
hb recall "password reset process"
#   Workspace: support-agent
#
#   1. [0.92] Password Reset Documentation
#      Users can reset passwords via Settings > Security > Reset Password.
#      (importance: 0.7, source: docs/security.md)
#
#   2. [0.78] Account Recovery Guide
#      If password reset email is not received, check spam folder...
#      (importance: 0.5, source: docs/account-recovery.md)
```

Set up continuous sync so docs stay up to date:

```sh
# Smart sync — only uploads changed files
hb sync ./docs

# Or start a file watcher
hb sync ./docs --watch
```

---

## Stage 3: Wire into the agent (10 minutes)

The customer's engineer adds three calls to their agent code:

```python
from hebbs import Hebbs

# API key is scoped to a workspace (support-agent in this case)
# The server knows which workspace from the key — no workspace param needed
hb = Hebbs(api_key="hb_live_sk_abc123", endpoint="https://hebbs.acme.com")

def handle_message(entity_id: str, message: str) -> str:
    # PROFILE — load what we know about this entity
    profile = hb.insights(entity_id=entity_id)

    # RECALL — load relevant context for this query
    memories = hb.recall(message, entity_id=entity_id)

    # Agent generates response with profile + memory context
    response = call_llm(
        system=f"""You are a support agent.

About this user:
{profile.text}

Relevant context:
{memories.text}""",
        user=message,
    )

    # REMEMBER — store atomic facts, not raw conversation
    facts = extract_facts(message, response)
    for fact in facts:
        hb.remember(
            content=fact.content,
            entity_id=entity_id,
            importance=fact.importance,
        )

    return response
```

Three calls: `insights()` to know the entity, `recall()` to get context, `remember()` to store what was learned.

**Indexing-in-progress handling:**

```python
memories = hb.recall("password reset", entity_id=entity_id)

if memories.indexing and not memories.memories:
    # First-time indexing not complete yet
    return "I'm still learning your docs, give me a few minutes."
```

---

## Stage 4: Conversations accumulate (Week 1-2)

The agent is deployed. Users start chatting.

What happens automatically (no action from customer):
- Each conversation turn stores atomic facts via `remember()`
- Each query triggers `recall()` — returns relevant docs + past conversations
- The daemon runs reflection in the background — clusters related memories into insights
- Contradiction detection runs — flags conflicting info between docs and conversations
- Decay runs — stale, unreinforced memories fade
- Entity profiles build up via the insights system

The customer opens the Memory Palace dashboard to see:
- Graph view of all memories and relationships
- Entity profiles (user_42: 87 memories, billing: 203 memories)
- Contradictions flagged (doc says X, conversations show Y)
- Auto-generated insights
- Indexing status, file list, memory counts

---

## Stage 5: Tune recall with their agent (Month 1)

The customer's engineer uses their AI agent (Claude, GPT, etc.) to tune retrieval:

The agent loads the tune skill (`hebbs-skill/tune/SKILL.md`) and uses regular HEBBS commands — no special tune CLI:

```
Engineer: "Tune my HEBBS workspace. We're in legal/compliance."

Agent: I'll profile your domain. What do you typically search for?

Engineer: "Specific clauses, coverage limits, compliance gaps across vendors."

Agent: Got it — Legal/Compliance profile. Let me store that.
  [calls: hb.remember("CLIENT-PROFILE: ...", entity_id="retrieval-instructions", importance=0.9)]

  Now I'll generate test queries and run baseline...
  [calls: hb.recall() for each eval query, scores keyword matches]
  Baseline: 58%. 3 zero-hit queries.

  Tuning — trying different k, cue expansion, strategies...
  [calls: hb.recall() with variations for each failing query]
  Tuned: 85% (+27pp). 0 zero-hits.

  Storing retrieval instructions...
  [calls: hb.remember("RETRIEVAL-INSTRUCTION: ...", entity_id="retrieval-instructions", importance=0.9)]

  Exporting rules...
  [writes .hebbs/retrieval-rules.md]

  Done. Load retrieval-rules.md into your agent's context for optimal recall.
```

The agent used only `recall`, `remember`, and file writing. HEBBS has no idea tuning happened — it just served regular API calls.

---

## Stage 6: Create more workspaces (Month 2+)

The company wants memory for their sales agent too — different docs, different conversations.

Via dashboard: Create workspace → "sales-agent" → generates new API key.

Or via CLI:

```sh
hb workspaces create sales-agent
# Workspace created: sales-agent
# API key: hb_live_sk_sales_789...

hb workspaces switch sales-agent
hb push ./sales-playbook
```

Two agents, two brains, isolated. Support memories don't leak into sales.

---

## Stage 7: Team grows

More engineers connect to the same HEBBS server from their machines:

```sh
# Another engineer on their laptop
hb login --endpoint https://hebbs.acme.com
hb push ./new-docs
hb recall "test query"
```

Roles via dashboard:
- **Admin**: create/delete workspaces, manage API keys, view all workspaces
- **Developer**: push docs, recall, remember, view assigned workspaces

---

## Stage 8: Ongoing — we monitor from our side

From our central dashboard, we see:

```
HEBBS Central Dashboard

Deployments:
  Acme Corp     hebbs.acme.com      ✓ healthy    3 workspaces   12,450 memories   v0.3.3
  Beta Inc      hebbs.beta.io       ✓ healthy    1 workspace    2,100 memories    v0.3.3
  Gamma Ltd     hebbs.gamma.com     ⚠ outdated   2 workspaces   8,300 memories    v0.3.1
```

We see health, workspace count, memory count, version. We push updates. We don't see their data.

---

## Journey summary

| Stage | Time | Who does it | What happens |
|---|---|---|---|
| Deploy | 1 hour | Us (on their machine) | Docker up, configure, onboard |
| Push docs | 10 min | Customer engineer | CLI push, verify recall |
| Wire agent | 10 min | Customer engineer | SDK: recall + remember + insights |
| Accumulate | Weeks 1-2 | Automatic | Reflect, contradict, decay, entity profiles |
| Tune | Month 1 | Customer's agent | CLI tune workflow |
| Expand | Month 2 | Customer | More workspaces, more agents |
| Grow team | Ongoing | Customer | More engineers connect |
| Monitor | Ongoing | Us | Central dashboard, health, updates |
