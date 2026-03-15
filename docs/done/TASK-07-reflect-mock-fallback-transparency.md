# TASK-07: Reflect Mock Fallback Transparency

Eliminate the silent mock-LLM fallback that pollutes the knowledge base with fake insights when no API key is configured. Make the system honest: tell users exactly what happened, and never store garbage as truth.

## Problem

When `hebbs reflect` runs without a valid LLM API key:

1. Server defaults to `proposal_provider: "openai"` / `proposal_model: "gpt-4o"` (config defaults).
2. `build_llm_provider_config` resolves `ProviderType::OpenAi` and checks `OPENAI_API_KEY` — which is `None`.
3. `create_provider` calls `OpenAiProvider::new`, which returns `Err("OpenAI provider requires api_key")`.
4. Server catches the error and **silently falls back to `MockLlmProvider`**, logging a warning: `"failed to create reflect proposal LLM provider, falling back to mock"`.
5. `MockLlmProvider` generates synthetic insights — for each cluster, it produces `"Consolidated insight about <first 5 words>"` with a hardcoded confidence of `0.85` and auto-accepts all candidates.
6. These mock insights are stored as real `MemoryKind::Insight` entries in the database.

**The user has no indication this happened unless they read server logs.** The CLI output shows `Insights created: 3` as if everything worked. This is arguably worse than returning nothing — it pollutes the knowledge base with low-quality mock data that will appear in future recall results and influence agent behavior.

## Design Principle

Tests should use mocks. Production should never silently degrade to mocks. If the system cannot perform a real operation, it must say so — loudly, at the call site, not buried in server logs.

## Changes Required

### 1. Proto (`hebbs/proto/hebbs.proto`)

Add a `warnings` field to `ReflectResponse` and `ReflectPrepareResponse`:

```protobuf
message ReflectResponse {
  uint64 insights_created = 1;
  uint64 clusters_found = 2;
  uint64 clusters_processed = 3;
  uint64 memories_processed = 4;
  repeated string warnings = 5;              // NEW
  bool used_mock_provider = 6;               // NEW
}

message ReflectPrepareResponse {
  string session_id = 1;
  uint64 memories_processed = 2;
  repeated ClusterPrompt clusters = 3;
  uint64 existing_insight_count = 4;
  repeated string warnings = 5;              // NEW
}
```

### 2. Server (`hebbs/crates/hebbs-server/src/server.rs`)

Replace the silent fallback. Two options depending on desired behavior:

**Option A (recommended): Fail loud, don't store garbage.**

When `create_provider` fails for proposal or validation, do NOT fall back to mock. Instead, store the failure state in the server and:

- `reflect` RPC returns `insights_created: 0` with `warnings: ["Reflect skipped: no LLM provider configured. Set OPENAI_API_KEY, ANTHROPIC_API_KEY, or configure an Ollama endpoint."]` and `used_mock_provider: false`.
- The server still starts successfully — reflect is a background capability, not a boot requirement.
- Clustering still runs (it's pure algorithm). The response includes `clusters_found` and `memories_processed` so the user knows there IS data to reflect on.

**Option B (backward-compatible): Fall back but be transparent.**

Keep the mock fallback but propagate the `used_mock_provider: true` flag and populate `warnings`. This preserves current behavior for users who expect *something* to happen but makes the degradation visible.

### 3. Core Engine (`hebbs/crates/hebbs-core/src/reflect.rs`)

Add a `warnings: Vec<String>` field to the reflect output type (`ReflectResult` or equivalent). The pipeline populates this when mock providers are detected.

If Option A: add a `provider_available: bool` check before running the proposal/validation stages. Skip LLM stages if false, return early with cluster stats and warnings.

### 4. CLI (`hebbs/crates/hebbs-cli/src/format.rs`)

Update `render_reflect_result` to display warnings:

```
$ hebbs reflect
⚠  Reflect ran with mock LLM provider (no real analysis).
   Set OPENAI_API_KEY for real insights, or use `reflect-prepare` + `reflect-commit` with your own LLM.

Insights created: 3 (mock-generated, not real analysis)
Clusters found:   5
Memories processed: 100
```

When `used_mock_provider` is true:

- Pretty format: print warning lines with `⚠` prefix before the stats. Annotate insight count with `(mock-generated)`.
- JSON format: include `"warnings"` array and `"used_mock_provider"` boolean in output.
- Raw format: include warnings in debug output.

### 5. REST API (`hebbs/crates/hebbs-server/src/rest.rs`)

Propagate `warnings` and `used_mock_provider` fields in the JSON response body for `POST /v1/reflect`.

### 6. SDK Propagation

Update response types in all SDKs:

- **Rust client** (`hebbs/crates/hebbs-client/`): Add `warnings: Vec<String>` and `used_mock_provider: bool` to `ReflectResponse` wrapper.
- **Python SDK** (`hebbs-python/src/hebbs/`): Add `warnings: list[str]` and `used_mock_provider: bool` to `ReflectResult` type.
- **TypeScript SDK** (`hebbs-typescript/src/`): Add `warnings: string[]` and `usedMockProvider: boolean` to `ReflectResult` interface.

### 7. Documentation (`hebbs-docs/`)

Update:

- `api/rest-endpoints.mdx` — document `warnings` and `used_mock_provider` fields on reflect response.
- `api/protobuf-schema.mdx` — new fields on `ReflectResponse`.
- `cli/commands.mdx` — document warning output behavior for `reflect`.
- `server/configuration.mdx` — add a section on LLM provider requirements for reflect, with clear instructions for each supported provider (OpenAI, Anthropic, Gemini, Ollama).

## Non-Goals

- Not changing the `MockLlmProvider` itself — it remains essential for unit and integration tests.
- Not adding a `--allow-mock` CLI flag. If users want mock behavior for local testing, they can set `proposal_provider: "mock"` explicitly in `hebbs.toml`. The silent fallback is the problem, not the mock provider's existence.
- Not blocking server startup when no LLM key is present. Reflect is optional; the server should boot and serve remember/recall/forget without an LLM.

## Validation

- [ ] `hebbs reflect` without any LLM key prints a clear warning and either skips insight generation (Option A) or annotates output as mock-generated (Option B)
- [ ] `ReflectResponse` proto includes `warnings` and `used_mock_provider` fields
- [ ] CLI pretty format shows `⚠` warning lines when mock provider is used
- [ ] CLI JSON format includes `warnings` array and `used_mock_provider` boolean
- [ ] REST endpoint includes warning fields in JSON response
- [ ] Server starts successfully without LLM keys — remember, recall, forget all work
- [ ] Explicit `proposal_provider: "mock"` in `hebbs.toml` does NOT trigger warnings (intentional mock use)
- [ ] TypeScript, Python, and Rust client SDKs expose warning fields on reflect result types
- [ ] Existing tests using `MockLlmProvider` continue to pass unchanged
