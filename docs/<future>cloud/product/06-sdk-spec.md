# HEBBS Cloud: SDK Specification

## Overview

The existing `hebbs-python` and `hebbs-typescript` SDKs are updated to support cloud mode via REST. No new SDK repos. The SDK auto-detects whether it's talking to a local daemon (gRPC) or the cloud (REST) based on the constructor arguments.

---

## Python SDK (`hebbs-python`)

### Installation

```sh
pip install hebbs
```

Same package as today. Cloud support is additive.

### Constructor

```python
from hebbs import Hebbs

# Cloud mode (new) — uses REST to api.hebbs.ai
hb = Hebbs(api_key="hb_live_sk_abc123")

# Cloud mode with region (skip global gateway hop)
hb = Hebbs(api_key="hb_live_sk_abc123", region="eu")

# Cloud mode with org-scoped key
hb = Hebbs(api_key="hb_live_ok_abc123", workspace="sales-agent")

# Local mode (existing, unchanged) — uses gRPC to local daemon
hb = Hebbs(endpoint="localhost:6380")
```

Detection logic:
- If `api_key` is provided → REST mode, target `api.hebbs.ai` (or regional endpoint)
- If `endpoint` is provided → gRPC mode, target that endpoint (existing behavior)
- If both → error

### Core methods

#### `recall()`

```python
result = hb.recall(
    cue="What are the user's display preferences?",
    entity_id="user_42",
    k=10,                    # optional, default auto-selected
    include_global=False,    # optional, search global brain too
)

# Result object
result.memories     # list of Memory objects
result.text         # pre-formatted string for prompt injection
result.strategy     # which strategy was used
result.query_time   # milliseconds

# Memory object
result.memories[0].id
result.memories[0].content
result.memories[0].importance
result.memories[0].score
result.memories[0].entity_id
result.memories[0].created_at      # datetime
result.memories[0].context         # dict

# Indexing status (always present)
result.indexing             # True if indexing is in progress
result.indexing_progress    # {"files_indexed": 18, "files_total": 34, "memories": 412}

# Most common usage — just inject into prompt
response = llm(f"Context:\n{result.text}\n\nUser: {message}")

# Handle indexing-in-progress gracefully
if result.indexing and not result.memories:
    progress = result.indexing_progress
    print(f"Indexing: {progress['files_indexed']}/{progress['files_total']} files")
```

#### `remember()`

```python
memory = hb.remember(
    content="Password reset is done via Settings > Security",
    entity_id="user_42",     # optional — groups memories by entity
    importance=0.7,          # optional, default 0.5
    context={"source": "conversation", "session": "sess_abc"},  # optional
    global_brain=False,      # optional, store in global brain
)

# Memory object
memory.id            # "01JABCDEF..."
memory.created_at    # datetime
```

#### `forget()`

```python
# By entity
hb.forget(entity_id="user_42")

# By memory IDs
hb.forget(ids=["01JAB1...", "01JAB2..."])

# By age
hb.forget(older_than_days=90)
```

#### `index()`

```python
# Upload and index a folder
status = hb.index("./docs")

# status.files         → 34
# status.memories      → 812
# status.status        → "complete"

# Upload and index individual files
hb.upload("./docs/new-policy.md")
```

#### `status()`

```python
info = hb.status()

# info.memories        → 3152
# info.files           → 34
# info.region          → "us"
# info.daemon          → "running"
# info.indexing        → {"in_progress": False, "files_indexed": 34, "files_total": 34}
```

#### `insights()`

Load aggregated knowledge about an entity. Built automatically by the reflect system — no explicit profile creation needed.

```python
profile = hb.insights(entity_id="user_42", min_confidence=0.7)

# Result object
profile.text               # pre-formatted string for prompt injection
profile.insights           # list of Insight objects

# Insight object
profile.insights[0].content           # "User prefers concise, step-by-step answers"
profile.insights[0].confidence        # 0.85
profile.insights[0].source_memories   # ["01JAB1...", "01JAB2...", "01JAB3..."]

# Use for personalization — inject into system prompt
system = f"About this user:\n{profile.text}\n\nContext:\n{memories.text}"
```

`insights()` is the way to "know" an entity. It returns what the daemon's reflection system has learned from accumulated memories — preferences, patterns, corrections, common topics. Think of it as an auto-generated entity profile.

### Async support

```python
from hebbs import AsyncHebbs

hb = AsyncHebbs(api_key="hb_live_sk_abc123")

memories = await hb.recall("query", entity_id="user_42")
await hb.remember("fact", entity_id="user_42")
profile = await hb.insights(entity_id="user_42")
```

### Error handling

```python
from hebbs import HebbsError, QuotaExceeded, RateLimited, WorkspaceSuspended

try:
    hb.remember("fact")
except QuotaExceeded as e:
    print(f"Hit limit: {e.limit} memories")
except RateLimited as e:
    print(f"Retry after {e.retry_after}s")
except WorkspaceSuspended:
    print("Workspace is suspended")
except HebbsError as e:
    print(f"Error: {e.code} - {e.message}")
```

---

## TypeScript SDK (`hebbs-typescript`)

### Installation

```sh
npm install @hebbs/sdk
```

Same package as today. Cloud support is additive.

### Constructor

```typescript
import { Hebbs } from '@hebbs/sdk';

// Cloud mode
const hb = new Hebbs({ apiKey: 'hb_live_sk_abc123' });

// Cloud mode with region
const hb = new Hebbs({ apiKey: 'hb_live_sk_abc123', region: 'eu' });

// Cloud mode with org key
const hb = new Hebbs({ apiKey: 'hb_live_ok_abc123', workspace: 'sales-agent' });

// Local mode (existing)
const hb = new Hebbs({ endpoint: 'localhost:6380' });
```

### Core methods

```typescript
// Recall
const result = await hb.recall({
  cue: "What are the user's display preferences?",
  entityId: 'user_42',
  k: 10,
});

result.memories;   // Memory[]
result.text;       // string, for prompt injection
result.strategy;   // string
result.queryTime;  // number (ms)
result.indexing;   // boolean
result.indexingProgress; // { filesIndexed, filesTotal, memories }

// Remember
const memory = await hb.remember({
  content: 'Password reset is via Settings > Security',
  entityId: 'user_42',
  importance: 0.7,
});

memory.id;         // string
memory.createdAt;  // Date

// Forget
await hb.forget({ entityId: 'user_42' });
await hb.forget({ ids: ['01JAB1...'] });
await hb.forget({ olderThanDays: 90 });

// Index
const status = await hb.index('./docs');

// Status
const info = await hb.status();

// Insights (entity profiling)
const profile = await hb.insights({ entityId: 'user_42' });
profile.text;      // string, for prompt injection
profile.insights;  // Insight[]
```

### Error handling

```typescript
import { HebbsError, QuotaExceeded, RateLimited } from '@hebbs/sdk';

try {
  await hb.remember({ content: 'fact' });
} catch (e) {
  if (e instanceof QuotaExceeded) {
    console.log(`Limit: ${e.limit}`);
  } else if (e instanceof RateLimited) {
    console.log(`Retry after ${e.retryAfter}s`);
  }
}
```

---

## Best practices: storing memories

`remember()` embeds and stores content as-is. It does NOT extract propositions or entities via LLM (unlike file indexing, which does). This means **how you write the content matters**.

The customer's agent is an LLM. It should extract atomic facts before calling `remember()`.

### Bad: storing raw conversation

```python
# One blob — hard to recall precisely
hb.remember(
    f"User asked: {message}\nAgent said: {response}",
    entity_id="user_42",
)
```

Problems:
- The entire exchange is one memory, searchable only by vector similarity to the whole blob
- "How do I reset my password?" might not match well against a blob that also contains greetings, follow-up questions, and agent preamble
- No atomic facts extracted — the insight "password reset is via Settings > Security" is buried

### Good: storing atomic facts

```python
# Each fact is independently searchable
hb.remember(
    "Password reset is done via Settings > Security > Reset Password",
    entity_id="user_42",
    importance=0.7,
)
hb.remember(
    "User prefers step-by-step instructions over summaries",
    entity_id="user_42",
    importance=0.6,
)
```

Benefits:
- Each fact matches precisely when recalled
- Importance is set per fact (corrections are 0.9, preferences 0.7, transient notes 0.3)
- Entity grouping enables temporal and entity-scoped recall
- The reflect system clusters these facts into insights (entity profiles)

### Pattern: let your agent extract facts

```python
def extract_and_store(message: str, response: str, entity_id: str):
    """Let the agent LLM extract key facts from a conversation turn."""
    facts = call_llm(
        system="Extract 1-3 key facts from this exchange. Return as JSON array "
               "with {content, importance} objects. importance: 0.9 for corrections, "
               "0.7 for decisions/preferences, 0.5 for general facts, 0.3 for transient.",
        user=f"User: {message}\nAgent: {response}",
    )
    for fact in facts:
        hb.remember(
            content=fact["content"],
            entity_id=entity_id,
            importance=fact["importance"],
        )
```

This is the recommended pattern. The agent already has an LLM — use it to extract before storing.

### Entity ID usage

`entity_id` groups related memories. It's not just for users — use it for any entity you want to track:

```python
# Users
hb.remember("Prefers dark mode", entity_id="user_42", importance=0.7)

# Topics
hb.remember("SOC2 audit passed March 2026", entity_id="compliance", importance=0.8)

# Products / vendors
hb.remember("Cloudvault had 2hr outage on 2026-03-15", entity_id="cloudvault", importance=0.7)

# Architecture decisions
hb.remember("Chose Postgres over MySQL for ACID guarantees", entity_id="architecture", importance=0.8)
```

Each entity_id builds its own profile via the reflect system. `hb.insights(entity_id="cloudvault")` returns aggregated knowledge about Cloudvault — automatically.

---

## SDK design principles

1. **`entity_id` is the real concept.** Entities can be users, topics, products, vendors, or anything worth grouping memories around. The SDK uses the same terminology as the engine — no aliases, no confusion.

2. **Strategy auto-selection by default.** Cloud customers should never think about strategies or weights. The SDK omits these from the request, and the server picks the right approach. Power users can override.

3. **`.text` for easy prompt injection.** The most common pattern is: recall → inject into prompt. The `.text` property gives a ready-to-use string. No formatting code needed. Available on both `recall()` and `insights()` results.

4. **Same package, two modes.** Installing `hebbs` gives you both local and cloud support. The constructor determines the mode. This means self-hosted customers can migrate to cloud (or vice versa) by changing one line.

5. **Minimal surface area.** Six methods cover 95% of use cases: `recall`, `remember`, `forget`, `index`, `status`, `insights`. Everything else (revise, edges, subscribe, prime) is available but not prominently documented for cloud users.

6. **Remember stores, not extracts.** `remember()` embeds and stores — no LLM proposition extraction. The customer's agent should extract atomic facts before calling `remember()`. This is by design: the agent knows what's important, the storage layer shouldn't guess.

---

## Changes to existing SDK repos

### `hebbs-python`

| Change | Scope |
|---|---|
| Add REST transport (requests/httpx) alongside existing gRPC | New module, ~200 LOC |
| Add `api_key` and `region` constructor params | Constructor update |
| Add `index()` and `upload()` methods | New methods, ~100 LOC |
| Add `.text` property on RecallResult and InsightsResult | Small addition |
| Add `indexing` and `indexing_progress` on RecallResult | Small addition |
| Add cloud error types | New exceptions |
| Auto-detect mode (cloud vs local) | Constructor logic |

### `hebbs-typescript`

| Change | Scope |
|---|---|
| Add REST transport (fetch) alongside existing gRPC | New module, ~200 LOC |
| Add `apiKey` and `region` constructor params | Constructor update |
| Add `index()` and `upload()` methods | New methods, ~100 LOC |
| Add `.text` property on RecallResult and InsightsResult | Small addition |
| Add `indexing` and `indexingProgress` on RecallResult | Small addition |
| Add cloud error types | New error classes |
| Auto-detect mode (cloud vs local) | Constructor logic |

Total new code per SDK: ~500 LOC. Existing gRPC functionality untouched.
