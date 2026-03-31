# Memory-First CRM — Built on Hebbs

## Thesis

Traditional CRMs are record-first (structured fields, pipelines, manual entry). Modern AI CRMs (Attio, Clay, Folk) add AI summaries but are still built on postgres/elasticsearch. A memory-first CRM flips the model: the system remembers everything and learns from it. Reps just talk to their AI, and the CRM builds itself.

## Distribution Model: CRM-as-a-Skill

The customer brings their own LLM subscription (Claude Pro, ChatGPT Plus, etc.). The CRM ships as a skill/plugin/MCP server. The LLM is the UI. Hebbs is the memory layer underneath.

```
Customer's AI subscription (Claude Pro, ChatGPT Plus, etc.)
            |
    Skill / Plugin / MCP Server
            |
    Hebbs Engine (cloud or local)
            |
    Customer's deal memory
```

### Why This Works Economically

- LLM inference cost: customer pays (they already have a subscription)
- UI development: zero (the LLM is the UI)
- Onboarding: "install this skill, start talking"
- Your cost structure: just Hebbs engine hosting

---

## How Hebbs Operations Map to CRM

| Hebbs Op | CRM Function |
|----------|-------------|
| `remember` | Auto-log every call, email, meeting, note — no manual entry |
| `recall(similarity)` | "Find me deals similar to this one" |
| `recall(temporal)` | "Show me everything that happened with Acme, in order" |
| `recall(causal)` | "What led to us losing the Initech deal?" — full causal chain |
| `recall(analogical)` | "We're selling to a fintech startup — what worked with similar companies?" |
| `prime` | Before a call, auto-load everything relevant about this prospect |
| `subscribe` | "Alert me if any deal shows the same stall pattern as Q3 losses" |
| `reflect` | Auto-generate: "Your top objection-handling patterns this quarter" |
| `insights` | "What do all our closed-won enterprise deals have in common?" |
| `revise` | Contact changed roles — update with lineage ("was VP Eng, now CTO") |
| `forget` | Prospect requests data deletion — real deletion, not soft-delete |
| `contradiction` | "Rep says champion is bought in, but last 3 emails show declining engagement" |

---

## Interaction Layers

### 1. Conversational (Primary — via Skill)

Rep talks to their AI. The skill handles CRM operations automatically.

**Before a call:**
> "Brief me on Acme Corp"
> -> prime loads full context: last interactions, open objections, champion status, similar deals, risk signals

**After a call:**
> "Sarah said they're evaluating two other vendors and need a security review before procurement. Timeline moved to Q3."
> -> Auto-remembers with entity scoping, revises timeline, flags contradiction if forecast says Q2

**Strategic questions:**
> "Why do we keep losing fintech deals after demo?"
> -> Causal recall across all fintech deals -> surfaces pattern

### 2. Auto-Capture (Passive)

| Source | How | What Gets Remembered |
|--------|-----|---------------------|
| Email (Gmail/Outlook) | OAuth integration | Sent/received, commitments, objections, next steps |
| Calendar | API sync | Meetings, attendees, frequency patterns |
| Call recordings | Gong/Fireflies webhook | Transcripts, key moments auto-extracted |
| Slack/Teams | Bot integration | Deal-related messages |
| Web activity | Tracking pixel | Prospect opened proposal, visited pricing page |

### 3. Feed (Push via Subscribe)

Morning briefing:
> "3 deals need attention: Acme (champion silent 8 days), Initech (procurement engaged), Globex (contradiction: marked Commit but no meeting since Mar 15)"

Real-time alerts:
> "Sarah at Acme viewed your proposal for the 4th time. Similar behavior preceded closing in 3 past deals."

### 4. Dashboard (Lightweight — For Managers)

Not the primary interface, but needed for pipeline visibility:
- Timeline view (temporal reconstruction of any deal)
- Insight cards (consolidation outputs)
- Contradiction board (pipeline inconsistencies)
- Pattern explorer
- Deal board (stages inferred from memory, not manually set)

### 5. API (For Power Users)

Direct Hebbs operations for custom workflows.

---

## Platform Support

| Platform | Delivery Mechanism |
|----------|-------------------|
| Claude Desktop / Code | MCP Server (`npx @hebbs/crm-server`) |
| Claude Code | Skill (SKILL.md) |
| ChatGPT | Custom GPT with Actions pointing to Hebbs REST API |
| Any LLM (OpenAI API, Anthropic API, Ollama) | Tool/function definitions |

---

## The SKILL.md Is the Product

CRM application logic lives in a well-crafted system prompt:

```markdown
## When to REMEMBER (auto-detect in conversation):
- Customer mentioned a competitor -> importance: 0.9
- Timeline/budget discussed -> importance: 0.8
- Objection raised -> importance: 0.9
- Next steps agreed -> importance: 0.7
- Stakeholder mentioned -> importance: 0.6

## When to RECALL:
- "brief me" / "prep me" -> prime(entity_id)
- "why" about a deal -> recall(strategy: causal)
- "what worked" -> recall(strategy: analogical)
- "what happened" -> recall(strategy: temporal)

## When to ALERT (check on conversation start):
- Deals with no activity >7 days -> surface risk
- Contradictions detected -> surface immediately
- Similar deals closing -> surface opportunity

## Entity mapping:
- Each company = entity_id (e.g., "acme-corp")
- Each contact = tagged within memories
- Each deal = entity_id (e.g., "acme-corp/deal-2026-q2")
```

---

## Differentiation vs. Incumbents

| Capability | Salesforce / HubSpot | AI CRMs (Attio, Clay) | Memory-First CRM |
|---|---|---|---|
| Data entry | Manual | Semi-auto | Zero (auto-remember) |
| Search | Field/keyword | Semantic similarity | 4 strategies (similarity + temporal + causal + analogical) |
| Deal history | Activity log | AI summary | Temporal reconstruction with importance weighting |
| Win/loss analysis | Manual post-mortem | AI summary | Causal chain traversal across all deals |
| Pattern transfer | Reports/dashboards | Basic similarity | Analogical recall across domains |
| Org learning | None | None | Auto-consolidation into playbooks |
| Risk detection | Rule-based alerts | AI scoring | Contradiction detection + subscribe patterns |
| Privacy/deletion | Soft delete | Soft delete | Real deletion with lineage |
| Latency | 100s of ms | Depends on LLM | <10ms recall |

---

## Pricing

| Tier | What | Price |
|------|------|-------|
| Solo | Hebbs cloud instance + skill | $29/mo |
| Team | Shared workspace, 10 reps | $199/mo |
| Business | Multi-workspace, connectors, SSO | $499/mo |
| Self-hosted | Docker + license key | $999/mo |

Revenue math: 50-person sales team on Team/Business = meaningful ARR per customer. 100 customers = $5-6M ARR.

---

## What Needs Building

| Component | Effort | Already Exists? |
|-----------|--------|-----------------|
| Hebbs engine | Done | Yes |
| CRM entity schema | Small | Extend existing entity_id system |
| SKILL.md / system prompt | Small | hebbs-skill exists, extend for CRM |
| MCP server wrapper | Small | Standard MCP pattern |
| ChatGPT actions wrapper | Small | REST API already exists |
| Email/calendar connectors | Medium | New — optional for v1 |
| Cloud hosting / multi-tenant | Medium | hebbs-enterprise exists |
| Billing/auth | Medium | hebbs-platform has auth |

**V1 scope:** Skill + hosted Hebbs engine + CRM system prompt. Email connectors and dashboard come later as upsells.

---

## Install Experience (Target)

```bash
# Install
npx @hebbs/crm init

# Connect to Claude
claude mcp add hebbs-crm npx @hebbs/crm-server

# Use
"Hey Claude, I just had a great call with Acme..."
```

Three commands to a working CRM.
