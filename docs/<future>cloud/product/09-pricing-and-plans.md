# HEBBS Cloud: Pricing and Plans

## Principles

1. **Free tier must deliver value.** Not a crippled demo — a real workspace with enough capacity to power a small agent. Converts through usage, not frustration.
2. **Simple dimensions.** Customers should understand what they're paying for without a spreadsheet. Three levers: workspaces, memories, recalls.
3. **No surprise bills.** Hard limits, not overages. When you hit a limit, operations return 402 — they don't generate a $10,000 invoice.
4. **Upgrade trigger is growth, not gates.** The features that make HEBBS valuable (recall strategies, reflection, contradiction detection) are available on every plan. You upgrade because you need more capacity or more workspaces.

---

## Plans

| | Free | Pro | Enterprise |
|---|---|---|---|
| **Price** | $0 | $49/mo | Custom |
| **Workspaces** | 1 | 10 | Unlimited |
| **Memories per workspace** | 5,000 | 100,000 | Unlimited |
| **Recall calls / month** | 10,000 | 1,000,000 | Unlimited |
| **Remember calls / month** | 5,000 | 500,000 | Unlimited |
| **File storage** | 50 MB | 5 GB | Unlimited |
| **Team members** | 1 | 10 | Unlimited |
| **Regions** | US only | US + EU | All |
| **Auto-tune** | No | Yes | Yes |
| **Memory Palace dashboard** | Yes | Yes | Yes |
| **Reflection + contradiction** | Yes | Yes | Yes |
| **Data export** | No | Yes | Yes |
| **Support** | Community (GitHub) | Email (48h SLA) | Dedicated (4h SLA) |
| **SSO / SAML** | No | No | Yes |
| **Audit logs** | No | No | Yes |
| **SLA** | None | 99.5% | 99.9% |
| **Self-hosted option** | No | No | Yes |

---

## Plan sizing rationale

### Free tier

**5,000 memories** is enough for:
- ~25 indexed documents (generates ~200 memories per doc on average)
- ~500 conversation turns stored
- Enough to see HEBBS working, build a POC, demo internally

**10,000 recalls/month** is enough for:
- ~330 recalls/day
- A small agent handling ~165 conversations/day (2 recalls per turn)

**Why it works:** The customer builds a working prototype, shows it to their team, and needs more capacity. That's the upgrade trigger — not a missing feature.

### Pro tier

**100,000 memories** is enough for:
- ~500 documents indexed
- ~10,000 conversation turns stored
- A production agent for a mid-size company

**1,000,000 recalls/month** is enough for:
- ~33,000 recalls/day
- An agent handling ~16,500 conversations/day
- Well beyond what most early-stage SaaS products need

**10 workspaces** covers:
- Support agent, sales agent, internal KB, staging/test copies
- Most companies don't need more than 5

### Enterprise

Custom pricing based on:
- Number of workspaces
- Memory volume
- Recall volume
- Required SLA
- Region requirements
- Self-hosted vs cloud

---

## What's NOT gated by plan

These features are available on every plan including Free:

- All 4 recall strategies (similarity, temporal, causal, analogical)
- Composite scoring with tunable weights
- Automatic reflection and insight generation
- Automatic contradiction detection
- Memory Palace dashboard
- Entity grouping and graph edges
- Decay and reinforcement
- Global brain (one per org)

This is intentional. The core intelligence is the product's differentiator. Gating it would weaken the free tier and make conversion harder.

---

## Metering

### What's metered

| Metric | How counted | Plan limit applies to |
|---|---|---|
| Memories | Current count in workspace (gauge) | Max concurrent memories |
| Recall calls | Each POST /v1/recall = 1 call | Monthly total |
| Remember calls | Each POST /v1/memories = 1 call | Monthly total |
| File storage | Cumulative bytes of uploaded files | Total storage |
| Team members | Current count in org | Max concurrent members |
| Workspaces | Current count in org | Max concurrent workspaces |

### What's NOT metered (our cost, built into pricing)

| Cost item | Approximate cost | Notes |
|---|---|---|
| OpenAI embedding | ~$0.02 / 1M tokens | ~$0.10/month per active workspace |
| OpenAI extraction (gpt-4o-mini) | ~$0.15 / 1M input tokens | ~$0.50/month per active workspace |
| Auto-tune cycles | ~$0.05 per cycle | ~$0.20/month per workspace |
| RocksDB storage | Cloud volume cost | ~$0.50/month per workspace |
| Container compute | ~0.5 vCPU, 512MB | ~$10/month per active container |

**Platform cost per active workspace:** ~$12/month
**Pro plan revenue per workspace:** $49/10 = $4.90 (if all 10 used)

This means the Pro plan is profitable only if the average customer uses 3 or fewer workspaces actively. At 10 active workspaces, the margin is negative.

**Mitigation:**
- Most customers use 2-3 workspaces (support + production)
- Suspended workspaces (no activity for 30d) cost near-zero (container stopped, only volume cost)
- Enterprise pricing accounts for high workspace counts

### Limit enforcement

| When limit is hit | Behavior |
|---|---|
| Memory limit | `remember()` returns 402. Recall still works. |
| Recall rate limit | `recall()` returns 429 with retry-after header. |
| Remember rate limit | `remember()` returns 429 with retry-after header. |
| File storage limit | `upload()` returns 402. Existing files accessible. |
| Workspace limit | Workspace creation returns 402. Existing workspaces unaffected. |
| Member limit | Invite returns 402. Existing members unaffected. |

**No silent degradation.** When you hit a limit, you get a clear error with the limit value and upgrade instructions. The agent can handle this gracefully.

---

## Billing mechanics

### Payment

- Free: no payment method required
- Pro: credit card or billing via Stripe
- Enterprise: invoiced, NET 30

### Billing cycle

- Monthly, billed on the anniversary of sign-up
- Usage resets at the start of each billing period (recall count, remember count)
- Storage and memory count are gauges, not cumulative (no reset needed)

### Upgrade

- Immediate effect
- Prorated charge for remaining days in current period

### Downgrade

- Takes effect at next billing period
- If over new plan limits: workspace becomes read-only (recall works, remember blocked) until under limits
- Customer can delete memories or workspaces to get under limits

### Cancellation

- Free tier: workspaces suspended after 90 days of inactivity, deleted after 180 days
- Pro/Enterprise: workspaces remain accessible until end of paid period, then follow free tier rules

---

## Future pricing considerations

### Usage-based add-ons (post-launch)

| Add-on | Price | For |
|---|---|---|
| Additional workspaces | $10/mo each | Beyond plan limit |
| Additional memories | $5/mo per 50K | Beyond plan limit |
| Additional recalls | $5/mo per 500K | Beyond plan limit |
| Priority support | $99/mo | Faster SLA for Pro |
| Additional regions | $20/mo each | Beyond 2 for Pro |

### Volume discounts (Enterprise)

| Workspaces | Discount |
|---|---|
| 10-50 | 20% |
| 50-200 | 35% |
| 200+ | Custom |
