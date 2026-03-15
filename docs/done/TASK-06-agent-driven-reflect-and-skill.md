# TASK-06: Agent-Driven Reflect & Universal Skill

Split the monolithic `reflect` operation into two new CLI/gRPC endpoints — `reflect-prepare` and `reflect-commit` — so any AI agent can drive reflection using its own LLM. Ship a universal SKILL.md that teaches agents to use HEBBS via CLI.

## Motivation

The current `reflect` pipeline couples clustering (pure algorithm) with LLM calls (proposal + validation) inside the HEBBS server. This requires users to configure LLM API keys on the server, which fails for:

- Agents using OAuth-based auth (OpenAI Codex, GitHub Copilot, Anthropic setup-token)
- Agents running through local proxies (Copilot Proxy, Ollama via VS Code)
- Agents on frameworks where the agent IS the LLM and already has inference access

The fix: HEBBS does the math (cluster, embed, index, store). The agent does the thinking (propose insights, validate them). Two clean CLI commands bridge the gap.

## Architecture

```
Agent                          HEBBS Server
  │                                │
  ├─ hebbs reflect-prepare ──────► │ gather memories + cluster (K-means)
  │◄──── clusters + prompts ───────┤ return cluster data + proposal prompts
  │                                │
  │  (agent reasons about clusters │
  │   using its own LLM / itself)  │
  │                                │
  ├─ hebbs reflect-commit ───────► │ embed insights + index + store
  │◄──── confirmation ─────────────┤ return insights_created count
  │                                │
```

The existing `hebbs reflect` command remains unchanged for users who configure server-side LLM keys.

## Changes Required

### 1. Proto (`hebbs/proto/hebbs.proto`)

Add two new RPCs to `ReflectService`:

```protobuf
service ReflectService {
  rpc Reflect(ReflectRequest) returns (ReflectResponse);
  rpc GetInsights(GetInsightsRequest) returns (GetInsightsResponse);
  rpc ReflectPrepare(ReflectPrepareRequest) returns (ReflectPrepareResponse);   // NEW
  rpc ReflectCommit(ReflectCommitRequest) returns (ReflectCommitResponse);     // NEW
}
```

New messages:

```protobuf
message ReflectPrepareRequest {
  ReflectScope scope = 1;
  optional string tenant_id = 2;
}

message ClusterPrompt {
  uint32 cluster_id = 1;
  uint32 member_count = 2;
  string proposal_system_prompt = 3;
  string proposal_user_prompt = 4;
  repeated string memory_ids = 5;        // hex-encoded source memory IDs
  string validation_context = 6;         // JSON: source memories + existing insights for validation
}

message ReflectPrepareResponse {
  string session_id = 1;                 // opaque handle for reflect-commit
  uint64 memories_processed = 2;
  repeated ClusterPrompt clusters = 3;
  uint64 existing_insight_count = 4;
}

message ProducedInsightInput {
  string content = 1;
  float confidence = 2;
  repeated string source_memory_ids = 3; // hex-encoded, must be subset of cluster memory_ids
  repeated string tags = 4;
  optional uint32 cluster_id = 5;
}

message ReflectCommitRequest {
  string session_id = 1;
  repeated ProducedInsightInput insights = 2;
  optional string tenant_id = 3;
}

message ReflectCommitResponse {
  uint64 insights_created = 1;
}
```

### 2. Core Engine (`hebbs/crates/hebbs-core/src/reflect.rs`)

Refactor `run_reflect_shared` into composable stages:

- **`reflect_prepare()`**: Calls `scope_memories()` + `load_existing_insights()` + `cluster_embeddings()` + `build_proposal_prompt()` + `build_validation_prompt()` for each cluster. Returns cluster data, prompts, and a session handle.
- **`reflect_commit()`**: Takes `ProducedInsightInput[]` + session handle. Calls `store_insight()` for each, updates cursor. Session handle ensures source memory IDs are validated against the original cluster.

Add a lightweight `ReflectSessionStore` (in-memory `HashMap<String, ReflectSession>` with TTL expiry) to hold prepare output between the two calls. The session stores cluster membership and source memory references for validation during commit.

### 3. Server (`hebbs/crates/hebbs-server/src/grpc/reflect_service.rs`)

Add handlers for the two new RPCs. Wire them to the refactored core functions. No LLM providers needed for either — these are pure storage + algorithm operations.

### 4. REST API (`hebbs/crates/hebbs-server/src/rest.rs`)

Add REST endpoints:

- `POST /v1/reflect/prepare` → `ReflectPrepareRequest`/`ReflectPrepareResponse`
- `POST /v1/reflect/commit` → `ReflectCommitRequest`/`ReflectCommitResponse`

### 5. CLI (`hebbs/crates/hebbs-cli/`)

Add two new commands:

- `hebbs reflect-prepare [--entity-id <id>] [--since-us <ts>]` — outputs JSON with clusters and prompts
- `hebbs reflect-commit --session-id <id> --insights <json>` — commits insights, outputs count

Both output JSON by default for easy agent consumption.

### 6. SDK Propagation

After proto changes, update:

- `hebbs-typescript/src/services/reflect.ts` — add `reflectPrepare()` and `reflectCommit()` methods
- `hebbs-python/src/hebbs/services/reflect.py` — add equivalent methods
- `hebbs/crates/hebbs-client/` — add Rust client methods

### 7. SKILL.md (`hebbs/skills/hebbs/SKILL.md`)

Create a universal agent skill file (AgentSkills-compatible) that teaches any agent how to use HEBBS via CLI. Covers:

- `remember` — when and how to store memories (importance guidelines, entity scoping)
- `recall` — all 4 strategies with guidance on when to use each
- `reflect-prepare` + `reflect-commit` — the two-step reflect flow
- `insights` — retrieving consolidated knowledge
- `forget` — memory deletion
- `prime` — context priming for entities

This skill works with OpenClaw, NanoClaw, Claude Code, Cursor, Codex, or any agent that can run shell commands.

### 8. Documentation (`hebbs-docs/`)

Update:

- `api/rest-endpoints.mdx` — new `/v1/reflect/prepare` and `/v1/reflect/commit`
- `api/protobuf-schema.mdx` — new messages and RPCs
- `cli/commands.mdx` — new `reflect-prepare` and `reflect-commit` commands
- New page: `guides/agent-skill.mdx` — how to use the HEBBS skill with any agent framework

## Non-Goals

- The existing `hebbs reflect` command is NOT removed or changed. It continues to work for server-side LLM reflection.
- No changes to the clustering algorithm or reflection prompts. The same prompts are exposed to the agent.
- No OpenClaw plugin. The skill + CLI approach replaces the plugin architecture for agent integration.

## Validation

- [ ] `hebbs reflect-prepare --entity-id test` returns valid JSON with clusters and prompts
- [ ] `hebbs reflect-commit --session-id <id> --insights '[...]'` stores insights correctly
- [ ] Stored insights have `MemoryKind::Insight`, proper `InsightFrom` edges, and appear in `hebbs insights`
- [ ] Session expires after TTL (default 10 minutes) — commit with expired session returns clear error
- [ ] Source memory ID validation: commit rejects IDs not in the original cluster
- [ ] Existing `hebbs reflect` still works unchanged with server-side LLM keys
- [ ] SKILL.md loads correctly in OpenClaw (`openclaw skills list` shows it)
- [ ] TypeScript and Python SDKs expose `reflectPrepare()` and `reflectCommit()`
