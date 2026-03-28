# HEBBS Cloud: Data Residency and Compliance

## Principle

Customer data (documents, memories, embeddings, conversation content) never leaves the region the customer chose when creating the workspace. This is enforced architecturally, not by policy.

---

## What lives where

### Central platform (US)

**Contains only metadata:**

| Data | Example | Sensitive? |
|---|---|---|
| Org name | "Acme Corp" | No |
| Workspace name | "sales-agent" | No |
| Region assignment | "eu" | No |
| API key hash | SHA-256 hash | No (hash only) |
| Member emails | "alice@acme.com" | Low (PII, but operational) |
| Usage counts | "45,230 recalls in March" | No |
| Billing info | Stripe customer ID | No (Stripe handles PII) |
| Tune scores | "baseline 64%, current 86%" | No |

**Never contains:**
- Document content
- Memory content
- Embeddings
- Entity IDs (these are customer-defined and may contain PII)
- Query text (cues)
- Conversation data

### Regional infrastructure (US, EU)

**Contains all customer data:**

| Data | Storage | Encrypted at rest |
|---|---|---|
| Uploaded documents | Tenant volume (`/data/docs/`) | Yes (cloud KMS) |
| Memories (content, metadata) | RocksDB (`/data/.hebbs/`) | Yes (cloud KMS) |
| Embeddings (float32 vectors) | RocksDB vectors CF | Yes (cloud KMS) |
| Graph edges | RocksDB graph CF | Yes (cloud KMS) |
| Insights | RocksDB insights CF | Yes (cloud KMS) |
| Query audit log | Regional gateway store | Yes (cloud KMS) |

**Also in region:**
- OpenAI API calls originate from the region (embedding + extraction)
- Tune runner executes in the region
- Memory Palace data served from the region

---

## Region availability

### Launch (v1)

| Region | Location | ID |
|---|---|---|
| US | us-east-1 (Virginia) | `us` |
| EU | eu-west-1 (Ireland) | `eu` |

### Future

| Region | Location | ID | When |
|---|---|---|---|
| APAC | ap-southeast-1 (Singapore) | `apac` | When customer demand requires |
| UK | eu-west-2 (London) | `uk` | Post-Brexit regulatory need |
| AU | ap-southeast-2 (Sydney) | `au` | When customer demand requires |

Adding a region requires:
1. Deploy regional gateway to new cloud region
2. Deploy Kubernetes cluster for workspace containers
3. Register region in platform's region_registry table
4. No platform code changes

---

## Data flow audit

### Recall request from EU workspace

```
1. Customer agent → api.hebbs.ai (US)
   Payload: API key + cue text + entity_id

   ⚠️ Cue text transits through US briefly during routing

2. Global gateway → eu.api.hebbs.ai (EU)
   Payload forwarded, not stored in US

3. Regional gateway → workspace container (EU)
   All processing happens in EU:
   - Embedding API call to OpenAI (from EU)
   - HNSW search (local RocksDB in EU)
   - Response constructed in EU

4. Response returns through gateways to customer
```

**The transit issue:** When a customer calls `api.hebbs.ai`, the cue text briefly passes through the US global gateway before being forwarded to EU. It's not stored, logged, or processed — just forwarded.

**Mitigation for strict compliance:**
- Customer calls `eu.api.hebbs.ai` directly (skip US entirely)
- SDK supports this via `region` parameter: `Hebbs(api_key="...", region="eu")`
- The global gateway is a convenience, not a requirement

### File upload

```
1. Customer → api.hebbs.ai/v1/upload (US gateway)
   File bytes transit through US

2. Gateway forwards to eu.api.hebbs.ai/v1/upload
   Regional gateway writes file to EU volume

3. Daemon indexes in EU
   All processing in EU
```

Same transit issue. Same mitigation: upload to `eu.api.hebbs.ai` directly.

### For strict data residency requirements

```python
# Data never leaves EU
hb = Hebbs(api_key="hb_live_sk_abc123", region="eu")
# All requests go directly to eu.api.hebbs.ai
# Nothing transits through US
```

---

## GDPR considerations

### Data subject rights

| Right | Implementation |
|---|---|
| Right to access | `GET /v1/memories?entity_id=user_42` — returns all memories for a user |
| Right to erasure | `POST /v1/forget {"entity_id": "user_42"}` — GDPR-compliant deletion |
| Right to portability | `POST /v1/workspaces/:slug/export` — full workspace export |
| Right to rectification | `PUT /v1/memories/:id` — revise memory content |

### Deletion guarantees

When `forget()` is called:
1. Memory is immediately removed from all indexes (HNSW, temporal, graph)
2. RocksDB tombstone is written (data becomes inaccessible)
3. RocksDB compaction physically removes data (within hours)
4. No backup retention of deleted memories (volumes are snapshotted, but forget operations are replayed on restore)

### Data processing agreement (DPA)

Enterprise customers get a DPA covering:
- HEBBS as a data processor
- Sub-processor list (OpenAI for embedding/extraction, cloud provider for infrastructure)
- Data location guarantees
- Breach notification procedures
- Audit rights

### OpenAI as sub-processor

OpenAI processes customer text during:
- Embedding generation (text → vector)
- Proposition extraction (text → atomic facts)
- Reflection (clustering → insight text)

OpenAI's data usage policy: API inputs are not used for training (as of their current terms). This should be documented in the DPA and monitored for changes.

**Future mitigation:** Replace OpenAI with self-hosted embedding models (e.g., running text-embedding-3-small equivalent on our infrastructure). This eliminates the sub-processor dependency. Not needed at launch but architecturally possible — the embedding provider is configurable per workspace.

---

## Compliance certifications (roadmap)

| Certification | Target | Status |
|---|---|---|
| SOC 2 Type I | Enterprise launch | Not started |
| SOC 2 Type II | 6 months post-launch | Not started |
| GDPR compliance | Launch | Architecturally ready |
| HIPAA | Enterprise demand | Not planned (requires BAA, encryption changes) |
| ISO 27001 | 12 months post-launch | Not started |

---

## Workspace migration between regions

Not supported at launch. A workspace's region is immutable after creation.

**Workaround for customers who need to move:**
1. Export workspace: `hb export --output ./backup/`
2. Create new workspace in target region: `hb workspaces create sales-agent-v2 --region eu`
3. Import: upload backup to new workspace
4. Update API key in their agent code
5. Delete old workspace

**Future:** Automated migration via platform command, with zero-downtime switchover.
