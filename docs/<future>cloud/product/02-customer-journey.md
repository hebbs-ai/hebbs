# HEBBS Cloud: Customer Journey

## Persona

**Alex** — a backend engineer at a startup that built an AI customer support agent. The agent answers questions from product docs, but it's stateless. Every conversation starts fresh. Alex wants the agent to learn from conversations, remember user preferences, and stay current with the docs.

---

## Stage 1: Discovery and Sign-up (5 minutes)

Alex finds HEBBS via a blog post / GitHub / word of mouth.

1. Visits `hebbs.ai`
2. Clicks "Get Started"
3. Signs up with GitHub OAuth (or email + password)
4. An **org** is auto-created from their GitHub org/username
5. Prompted: **"Name your first workspace:"** → Alex types `support-agent`
6. Region defaults to US (changeable)
7. An **API key** is generated and displayed once

```
Welcome to HEBBS Cloud.

Workspace: support-agent (us)
Your API key: hb_live_sk_abc123def456...

Save this — you won't see it again.
```

Alex copies the key. Done. The workspace has a name that means something from day one.

---

## Stage 2: Push documents (5 minutes)

Alex has a `docs/` folder with product documentation in markdown.

**Option A: CLI**

```sh
# Install
brew install hebbs-ai/tap/hb
# or: curl -sSf https://hebbs.ai/install-cloud | sh

# Login
hb login
# Opens browser → authorizes → token saved to ~/.hb/config
# Current workspace: support-agent (us)

# Push docs
hb push ./docs
# Pushing to workspace: support-agent (us)
# Uploading 34 files (245 KB)...
# Indexing... done. 34 files, 812 memories created.
```

If Alex had created multiple workspaces (e.g., via the dashboard) before using the CLI, the CLI would prompt:

```sh
hb push ./docs
# You have multiple workspaces. Which one?
#   1. support-agent (us)
#   2. sales-agent (eu)
# Select [1-2]: 1
# Pushing to workspace: support-agent (us)
# Uploading 34 files...
```

**Option B: SDK**

```python
pip install hebbs

from hebbs import Hebbs

# API key is scoped to a workspace — no ambiguity
hb = Hebbs(api_key="hb_live_sk_abc123def456")
hb.index("./docs")
# Pushing to workspace: support-agent (us)
# 34 files, 812 memories created.
```

Behind the scenes:
- Files are uploaded to the regional gateway
- Gateway writes them to the workspace container's volume
- The existing daemon detects new files and starts indexing
- Embedding via OpenAI text-embedding-3-small (our key)
- LLM extraction via gpt-4o-mini (our key)
- Propositions, entities, and graph edges are created

**Keeping docs in sync:** `push` is a one-time upload. As Alex edits docs, they can keep the workspace up to date:

```sh
# Smart sync — only uploads changed files
hb sync ./docs

# Or start a file watcher that auto-pushes on every save
hb sync ./docs --watch
```

The watcher is a lightweight local process — it watches the folder and pushes changes to the cloud. All indexing happens server-side. See [07-cli-spec.md](07-cli-spec.md) for details.

**Important:** File upload is fast, but indexing takes time (embedding + LLM extraction). For 34 files, indexing may take 1-5 minutes depending on size. The push command returns after upload, and indexing continues in the background.

If Alex tries to recall before indexing is complete:

```sh
hb recall "password reset"

# ⏳ Indexing in progress: 18/34 files indexed, 412 memories so far.
#    Showing results from indexed content only.
#
#   1. [0.89] Password Reset Documentation
#      Users can reset passwords via Settings > Security...
```

If no files have been indexed yet:

```sh
hb recall "password reset"

# ⏳ First-time indexing in progress: 0/34 files indexed.
#    No memories available yet. Check status:
#    hb status
#    Or view live progress on the dashboard:
#    hb dashboard
```

Alex can check indexing progress at any time:

```sh
hb status

#   Workspace:       support-agent (us)
#   Status:          indexing
#   Files uploaded:  34
#   Files indexed:   18/34
#   Memories:        412 (growing)
#   Currently:       embedding sections from policies/data-retention.md
#   Dashboard:       https://app.hebbs.ai/workspaces/support-agent/panel
```

Once indexing completes, Alex tests recall:

```sh
hb recall "password reset process"

#   Workspace: support-agent (us)
#
#   1. [0.92] Password Reset Documentation
#      Users can reset passwords via Settings > Security > Reset Password.
#      The reset link expires after 24 hours.
#      (importance: 0.7, source: docs/security.md)
#
#   2. [0.78] Account Recovery Guide
#      If password reset email is not received, check spam folder...
#      (importance: 0.5, source: docs/account-recovery.md)
```

Alex sees relevant results. The docs are working. Now they can wire the agent with confidence.

---

## Stage 3: Wire into the agent (10 minutes)

Alex adds three calls to their existing agent code:

```python
from hebbs import Hebbs

hb = Hebbs(api_key="hb_live_sk_abc123def456")

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

    # REMEMBER — store atomic facts, not the raw conversation
    # The agent is an LLM — it extracts key facts before storing
    facts = extract_facts(message, response)  # your LLM call
    for fact in facts:
        hb.remember(
            content=fact.content,
            entity_id=entity_id,
            importance=fact.importance,
        )

    return response
```

Three calls: `insights()` to know the entity, `recall()` to get context, `remember()` to store what was learned.

**Why store atomic facts, not conversation blobs:**

```python
# Bad — one blob, hard to recall precisely later
hb.remember(
    f"User asked: {message}\nResolution: {response}",
    entity_id=entity_id,
)

# Good — each fact is independently searchable
hb.remember("Password reset is via Settings > Security", entity_id=entity_id, importance=0.7)
hb.remember("User prefers step-by-step instructions", entity_id=entity_id, importance=0.6)
```

`remember()` embeds and stores — it doesn't extract propositions via LLM. The customer's agent IS an LLM. It should extract the important facts before storing. This makes future recall precise: "how do I reset my password?" matches the atomic fact directly, not a conversation dump.

**Entity profiling via `insights()`:**

The HEBBS daemon automatically clusters memories per entity and generates insights via the reflect system. When Alex's agent calls `hb.insights(entity_id="user_42")`, it gets aggregated knowledge:

```
- User prefers concise, step-by-step answers
- Frequently asks about billing and password management
- Power user — knows the API, references docs
- Corrected the agent twice about timezone handling
```

This builds up automatically over time as conversations accumulate. No explicit profile creation needed.

**`entity_id` is not just for users.** It groups any related memories:

```python
hb.remember("SOC2 audit passed March 2026", entity_id="compliance", importance=0.8)
hb.remember("Cloudvault had 2hr outage on 2026-03-15", entity_id="cloudvault", importance=0.7)
hb.remember("User prefers dark mode", entity_id="user_42", importance=0.6)
```

**Indexing-in-progress handling in the SDK:**

The `recall()` response includes indexing status so the agent can handle it gracefully:

```python
memories = hb.recall("password reset", entity_id=entity_id)

memories.indexing          # True if indexing is still in progress
memories.indexing_progress # {"files_indexed": 18, "files_total": 34, "memories": 412}
memories.memories          # results from whatever has been indexed so far (may be empty)
```

The agent can choose to inform the user: "I'm still learning your docs, give me a few minutes." Or just use whatever partial results are available.

---

## Stage 4: Conversations accumulate (Week 1-2)

Alex deploys. Users start chatting with the agent.

What happens automatically (no action from Alex):
- Each conversation turn generates a `remember()` call → memory stored
- Each query triggers `recall()` → relevant docs + past conversations returned
- The daemon runs reflection in the background → clusters related memories into insights
- Contradiction detection runs → flags conflicting info between docs and conversations
- Decay runs → stale, unreinforced memories fade

The agent starts exhibiting intelligence:
- "Last time you asked about this, the solution was X" (conversational memory)
- Answers improve because past successful resolutions surface alongside docs
- Entity profiles build up (user_42 always asks about billing)

---

## Stage 5: Alex checks the dashboard (Week 2)

Alex opens the Memory Palace dashboard:

```sh
hb dashboard
# Opens https://app.hebbs.ai/workspaces/support-agent/panel
```

Sees:
- **Graph view:** memories as nodes, edges as relationships, red dashed lines for contradictions
- **Stats:** 812 doc memories + 2,340 conversation memories = 3,152 total
- **Top entities:** user_42 (87 memories), user_15 (64 memories), billing (203 memories)
- **Contradictions:** 3 flagged (doc says "24-hour reset" but conversations show it's instant)
- **Insights:** 12 auto-generated (e.g., "Most billing questions are about invoice format, not charges")

Alex updates the docs to fix the contradictions. Pushes again:

```sh
hb push ./docs
# Uploading 2 changed files... re-indexed. 4 memories updated.
```

---

## Stage 6: Create a second workspace (Month 1)

The support agent works well. Alex's company wants to add memory to their sales agent too — different docs, different conversations, different tuning.

```sh
hb workspaces create sales-agent --region eu
# Workspace created: sales-agent (eu)
# API key: hb_live_sk_sales_789...

hb workspaces switch sales-agent
hb push ./sales-playbook
```

```python
# In the sales agent code
sales_hb = Hebbs(api_key="hb_live_sk_sales_789...")
```

Two agents, two brains, isolated. The support agent's memories don't leak into sales.

---

## Stage 7: Team grows (Month 2+)

Alex invites teammates to the org:

```sh
hb members invite alice@company.com --role admin
hb members invite bob@company.com --role developer
```

Roles:
- **Owner**: billing, delete org
- **Admin**: create/delete workspaces, manage members, manage API keys
- **Developer**: push docs, recall, remember, view dashboard

---

## Stage 8: Tune recall with their own agent (Month 1-2)

Alex notices some queries returning weak results. They open Claude Code (or whatever agent they use) and run the tune flow:

```
Alex: "Tune my HEBBS workspace. We're in legal/compliance."

Claude: I'll profile your domain and generate evals. What's hardest to find?

Alex: "Cross-vendor compliance gaps and contradicting coverage limits."

Claude: Got it. Let me set up the profile and generate test queries.

  [runs: hb tune profile --domain legal-compliance ...]
  [generates 20 evals from the indexed content]

  Here are 20 eval queries. Review these — want to add or change any?

Alex: "Looks good, run it."

Claude:
  [runs: hb tune baseline]
  Baseline: 58%. 3 zero-hit queries.

  [runs: hb tune run]
  Tuned: 85% (+27pp). 0 zero-hits.

  [runs: hb tune store]
  [runs: hb tune export --output ./retrieval-rules.md]

  Done. Load retrieval-rules.md into your agent's context for optimal recall.
```

Alex's own agent drove the entire process. HEBBS just served recall/remember calls. The intelligence was in the agent, the data was in HEBBS.

```sh
hb tune status
# Workspace: default
# Profile: legal-compliance
# Evals: 20 queries
# Baseline: 58%
# Tuned: 85% (+27pp)
# Rules stored: 8 master rules
```

---

## Stage 9: Enterprise needs (Month 6+)

Company grows. Needs:
- Self-hosted deployment (data sovereignty)
- SSO (SAML/OIDC)
- Audit logs
- SLA

Alex migrates the sales-agent workspace to self-hosted:

```sh
# Export workspace data
hb workspaces export sales-agent --output ./sales-backup

# On their own infrastructure
hebbs init ./sales-agent
hebbs import ./sales-backup
# Same data, same memories, now self-hosted
```

The open-source `hebbs` binary runs the same engine. No vendor lock-in.

---

## Journey summary

| Stage | Time | Customer action | HEBBS does |
|---|---|---|---|
| Sign up | 2 min | Create account | Generate org, workspace, API key |
| Push docs | 5 min | `hb push ./docs` | Upload, index, embed, extract |
| Wire agent | 10 min | Add `recall()` + `remember()` | Serve context, store memories |
| Accumulate | Weeks 1-2 | Nothing — users chat | Index conversations, reflect, detect contradictions |
| Dashboard | Week 2 | Open Memory Palace | Visualize brain, show insights |
| Expand | Month 1 | Create workspace | Provision workspace, isolated brain |
| Team | Month 2 | Invite members | Role-based access |
| Tune | Month 1-2 | Agent drives tune via CLI/SDK | Serve recall/remember, store tuned rules |
| Enterprise | Month 6 | Upgrade or self-host | Export/import, same engine |
