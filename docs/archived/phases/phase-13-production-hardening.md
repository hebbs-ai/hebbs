# Phase 13: Hardening for Production -- Architecture Blueprint

## Status: PENDING

---

## Intent

Phase 13 is the gate between "it works" and "it is safe to run with real data." Everything before this phase was built with correctness, speed, and testability as the primary constraints. Phase 13 adds the three constraints that production demands: **isolation** (tenant A cannot see tenant B's data), **authorization** (only authenticated callers reach the engine), and **resilience** (the system survives sustained load, adversarial input, and operational mistakes without degradation).

The decisions made here are load-bearing for Phase 17 (Edge Sync) and Phase 15 (Deployment). If tenant isolation is not structural, Phase 17's fleet sync cannot guarantee data boundaries. If auth is not composable middleware, Phase 15's Kubernetes deployment cannot integrate with external identity providers. If rate limiting is not per-tenant, a single noisy tenant can starve the fleet.

This phase does not add new cognitive capabilities. It does not make recall smarter or reflection deeper. It makes the existing nine operations safe to expose to untrusted callers, multiple tenants, and sustained production traffic. The system that exits Phase 13 is the system that enters deployment.

---

## Scope Boundaries

### What Phase 13 delivers

- Authentication layer: API key validation for gRPC and HTTP, with middleware-based composition
- Optional mTLS for gRPC transport encryption and client certificate verification
- Multi-tenancy: tenant ID propagation through all nine operations, per-tenant storage isolation at the key-encoding level, per-tenant in-memory HNSW graphs
- Per-tenant rate limiting on all operations with configurable burst and sustained rates
- Configuration extensions: `[auth]`, `[tenancy]`, `[rate_limit]` sections in `hebbs.toml`
- Error taxonomy extensions: `Unauthorized`, `Forbidden`, `RateLimited`, `TenantNotFound` variants
- Proto schema extensions: `tenant_id` field in all request messages, auth metadata in headers
- 72-hour soak test execution using the Phase 12 `hebbs-bench` harness
- Security audit: input validation audit, dependency audit, `unsafe` audit, sensitive data handling review
- Data integrity hardening: checksum verification on deserialization, crash recovery chaos testing (1,000 iterations of the Phase 12 crash harness)
- Snapshot retention limits: configurable max predecessors per memory (deferred from Phase 5)
- `SetPolicy` RPC for runtime reflect policy management (deferred from Phase 8)
- REST Subscribe via SSE for HTTP-only environments (deferred from Phase 8)
- Config dump sensitive value redaction (deferred from Phase 8)
- Client SDK activation: `api_key()` and `tls_config()` on `HebbsClient` builder become functional (deferred from Phase 10)
- CLI auth flags: `--api-key` and `--tls-cert`/`--tls-key` become functional (deferred from Phase 9)
- Thread pool optimization for subscriptions if Phase 12 profiling shows thread creation overhead is significant (deferred from Phase 6)

### What Phase 13 explicitly does NOT deliver

- Edge mode or sync protocol (Phase 17)
- Per-tenant separate RocksDB instances (over-engineering for Phase 13 scale targets; key-prefix isolation is sufficient up to 10,000 tenants)
- External identity provider integration (OAuth2, OIDC, SAML) -- Phase 13 uses API keys; external IdP integration is a Phase 15 deployment concern
- Hot configuration reload (restart-to-reconfigure remains acceptable; hot reload adds complexity with marginal production value)
- Distributed rate limiting across multiple server instances (Phase 17 sync infrastructure is required; Phase 13 rate limiting is per-process)
- SIMD-optimized distance computation (only if Phase 12 profiling showed distance computation is <20% of recall latency; if >20%, it is an in-scope optimization)
- Horizontal scaling / sharding (Phase 15 -- Phase 13 hardens a single node)
- Standard `grpc.health.v1` protocol (can be added as a one-line service addition; custom health service remains primary)

These exclusions are deliberate. Phase 13 builds the production armor for a single node. Phase 17 extends it across devices. Phase 15 deploys it to production infrastructure.

---

## Architectural Decisions

### 1. Authentication Architecture

Authentication answers one question: "Who is this caller?" Phase 13 uses API keys -- opaque bearer tokens that identify a (tenant, permission set) pair. API keys are the right choice for a system-to-system API where the callers are agent frameworks, not humans in browsers. OAuth2/OIDC is over-engineered for this use case and is deferred to Phase 15 where Kubernetes deployments may require it.

**Key lifecycle:**

Every API key is a 256-bit cryptographically random token, encoded as a 43-character base64url string (no padding). The key is prefixed with `hb_` for visual identification in logs and config files (e.g., `hb_a1b2c3...`). The prefix is not part of the cryptographic material.

Keys are stored in the `meta` column family under the key prefix `auth:key:`. The stored value is a record containing: the SHA-256 hash of the key (never the plaintext), the associated `tenant_id`, a human-readable `name` (for identification in admin UIs), a `created_at` timestamp, an `expires_at` timestamp (optional, for time-bounded keys), and a `permissions` bitfield.

**Permission model:**

Phase 13 uses a coarse permission model with three levels, represented as a bitmask:

| Permission | Bit | Allows |
|------------|-----|--------|
| `read` | 0x01 | `recall`, `prime`, `subscribe`, `insights`, `get` |
| `write` | 0x02 | `remember`, `revise`, `forget` |
| `admin` | 0x04 | `reflect`, `reflect_policy`, tenant management, key management |

A key with `read + write` (0x03) is the typical agent key. A key with `admin` (0x07 -- all three) is for operators. Read-only keys (0x01) are for monitoring and analytics pipelines.

**Why not RBAC or ABAC:** Role-based and attribute-based access control add complexity that is not justified until multi-team, multi-environment deployments in Phase 15. The three-level bitmask covers the operational reality of Phase 13: agents read and write, operators manage. If a finer-grained model is needed later, the bitmask can be extended without breaking existing keys.

**Key rotation:**

Multiple keys can be active simultaneously for the same tenant. This enables zero-downtime rotation: create a new key, update clients, revoke the old key. Revocation is immediate -- the key record is deleted from the meta CF, and the in-memory key cache is invalidated.

**Key cache:**

On startup, all key records are loaded into an in-memory `HashMap<[u8; 32], KeyRecord>` (keyed by SHA-256 hash). This cache is wrapped in a `parking_lot::RwLock`. Lookups on the hot path take a read lock (zero contention under normal operation). Key creation and revocation take a write lock (rare, admin-only operations). The cache is bounded by tenant count -- at 10,000 tenants with 3 keys each, the cache is ~1MB. No eviction policy needed.

**Auth bypass:**

- Health check endpoints (`/v1/health/live`, `/v1/health/ready`, gRPC `HealthService`) are always unauthenticated. Kubernetes probes must work without credentials.
- The `--no-auth` CLI flag disables authentication entirely. This flag emits a warning at startup: "Authentication disabled. This server accepts unauthenticated requests." This is acceptable for local development and embedded mode (FFI, Python SDK). It is a deployment misconfiguration for production.
- Embedded mode (FFI, Python SDK) bypasses auth by default because there is no network boundary to protect. The engine is linked directly into the caller's process.

### 2. Middleware Composition: gRPC and REST

Authentication, tenant extraction, and rate limiting are implemented as composable middleware layers, not as logic inside service handlers. This is critical: handler code must not change when auth is added, and auth must compose with future middleware (tracing, compression, request deduplication) without handler awareness.

**gRPC middleware (tonic interceptor chain):**

tonic interceptors process requests before they reach the service handler. Phase 13 inserts three interceptors in order:

1. **AuthInterceptor**: Extracts the `authorization` metadata key from the gRPC request. Expects the format `Bearer hb_...`. Hashes the token with SHA-256. Looks up the hash in the key cache. If the key is not found, expired, or does not have the required permission for the requested method, the interceptor returns `Status::unauthenticated()` or `Status::permission_denied()`. If valid, the interceptor injects the resolved `tenant_id` and `permissions` into the request extensions (tonic's type-map attached to `Request`).

2. **TenantInterceptor**: Reads the `tenant_id` from request extensions (set by AuthInterceptor). If the request message also contains a `tenant_id` field, validates that it matches the key's tenant. If it does not match, returns `Status::permission_denied()` ("key not authorized for this tenant"). This prevents a key issued for tenant A from being used to access tenant B's data, even if the request message is crafted with tenant B's ID.

3. **RateLimitInterceptor**: Reads the `tenant_id` from request extensions. Checks the per-tenant rate limiter for the requested operation. If the limit is exceeded, returns `Status::resource_exhausted()` with a `retry-after` metadata value.

**REST middleware (axum layer chain):**

axum's tower-based middleware stack mirrors the gRPC chain. The `Authorization` HTTP header carries the same `Bearer hb_...` token. Middleware layers extract tenant context, check permissions, and enforce rate limits. The resolved `TenantContext` is inserted into axum's request extensions, accessible in handlers via an extractor.

**Why middleware, not handler logic:** Phase 8's handlers use the pattern `extract request → validate → spawn_blocking → engine call → response`. Inserting auth checks inside each handler would mean 9+ copy-paste sites, each a potential inconsistency. Middleware intercepts before the handler, so every endpoint is protected by construction. Adding a new endpoint in Phase 17 or 18 automatically inherits all middleware.

### 3. mTLS for gRPC

Mutual TLS provides transport-level encryption and client certificate verification. It is optional in Phase 13 -- the default remains plaintext for development and local deployment. mTLS is activated via the `[auth.tls]` configuration section.

**When mTLS is enabled:**

- tonic's `ServerTlsConfig` is configured with the server certificate, server private key, and client CA certificate.
- The server rejects connections from clients that do not present a certificate signed by the configured CA.
- The client certificate's Common Name (CN) or Subject Alternative Name (SAN) is not used for tenant identification in Phase 13. It provides transport-level mutual authentication only. Tenant identity still comes from the API key. This avoids the complexity of certificate-based identity management, which is a Phase 15 concern.

**Why not mandate TLS:** The single-binary deployment model (Principle 2) means many users run HEBBS on localhost or behind a reverse proxy that terminates TLS. Forcing TLS would break the "unpack and run" promise. mTLS is for deployments where the network between client and server is untrusted and the operator controls the PKI.

### 4. Multi-Tenancy Isolation Model

Multi-tenancy is the highest-stakes architectural decision in Phase 13. A mistake here -- either too weak (data leaks) or too strong (unmanageable overhead) -- is a rewrite.

**The isolation model: key-prefix scoping at the storage trait boundary.**

Guiding Principle 12 states: "Tenant isolation is structural, not logical. Multi-tenant deployments use separate RocksDB column family prefixes per tenant." The implementation interprets this as: all storage operations for a tenant are scoped by a key prefix derived from the tenant ID. The scoping is enforced at a layer between the engine and the storage backend, so that engine code cannot accidentally perform a cross-tenant operation.

**Why key-prefix, not separate CFs per tenant:**

| Approach | Per-tenant overhead | Max tenants | Isolation guarantee | Complexity |
|----------|-------------------|-------------|-------------------|------------|
| Key prefix in shared CFs | ~0 (just the prefix bytes) | Unlimited | Structural at storage wrapper | Low |
| Separate CFs per tenant (5 CFs × N) | ~2MB memtable + bloom filter per CF | ~200 (1,000 CFs is RocksDB soft limit) | Physical per CF | Medium |
| Separate RocksDB instance per tenant | ~50MB base + 256MB block cache per instance | ~50 (memory-bound) | Complete process isolation | High |

At Phase 13's target of supporting up to 10,000 tenants on a single node, only key-prefix scoping is viable. Separate CFs would require 50,000 column families; separate instances would require 500GB of RAM for block caches alone. Key-prefix scoping adds 1-32 bytes of overhead per key (the tenant ID length) and zero additional memory structures.

**The structural guarantee:** A `TenantScopedStorage` wrapper implements `StorageBackend`. It holds a reference to the underlying (unscoped) backend and a `tenant_id`. Every method prepends `[tenant_id_bytes][0xFF]` to every key before delegating to the inner backend. Every iterator is prefixed with the tenant scope, so it cannot see keys from other tenants. The separator byte `0xFF` cannot appear in valid UTF-8 tenant IDs (max UTF-8 byte is `0xF4`), guaranteeing prefix isolation. No engine code interacts with the underlying backend directly -- all access goes through the scoped wrapper.

**What this means for each column family:**

| CF | Current key format | Tenant-scoped key format |
|----|-------------------|------------------------|
| `default` | `[memory_id 16B]` | `[tenant_id][0xFF][memory_id 16B]` |
| `temporal` | `[entity_id][0xFF][timestamp_be 8B]` | `[tenant_id][0xFF][entity_id][0xFF][timestamp_be 8B]` |
| `vectors` | `[hnsw node data]` | `[tenant_id][0xFF][hnsw node data]` |
| `graph` | `[prefix 1B][source 16B][edge_type 1B][target 16B]` | `[tenant_id][0xFF][prefix 1B][source 16B][edge_type 1B][target 16B]` |
| `meta` | `[meta_key string]` | `[tenant_id][0xFF][meta_key string]` |

**System-level metadata** (schema version, global config) remains unscoped in the `meta` CF under a reserved prefix `_system:`. Tenant-scoped meta entries (decay cursor, reflect watermark, memory count) are scoped like all other tenant data.

### 5. Per-Tenant HNSW Graphs

The in-memory HNSW graph is currently a single structure shared across all data. Multi-tenancy requires per-tenant HNSW graphs because HNSW is a proximity graph -- if tenant A's vectors are in the same graph as tenant B's, a nearest-neighbor search for tenant A will traverse tenant B's nodes, violating isolation and wasting computation.

**Design: lazy-loaded per-tenant HNSW map.**

`IndexManager` currently holds a single `RwLock<HnswGraph>`. In Phase 13, this becomes a `RwLock<HashMap<TenantId, HnswGraph>>`. Each tenant's HNSW graph is loaded lazily on first access: when a tenant's first `remember()` or `recall()` arrives, the `IndexManager` checks if the tenant's graph exists in the map. If not, it rebuilds it from the `vectors` CF by scanning the tenant's scoped prefix. Subsequent operations use the in-memory graph directly.

**Memory implications:**

Each HNSW graph has a per-node overhead of approximately `(M_max * 2 + M * (layers - 1)) * 16 bytes` for neighbor lists, plus 16 bytes for the node ID and metadata. With `M=16`, `M_max=32`, and an average of 1.1 layers per node, this is approximately 600 bytes per node. At 100K memories per tenant, that is ~60MB per tenant HNSW graph in memory.

**Eviction policy:**

For deployments with many tenants where most are inactive, holding all HNSW graphs in memory is wasteful. Phase 13 introduces an LRU eviction policy: HNSW graphs that have not been accessed within a configurable window (default: 1 hour) are evicted from memory. The next access triggers a rebuild from the `vectors` CF. Rebuild cost is O(N * M * log N) where N is the tenant's memory count.

The eviction window and maximum loaded tenants are configurable via `[tenancy]` config. For single-tenant deployments, eviction is disabled (the sole tenant is always loaded).

**Thread safety:** The outer `RwLock` protects the tenant map itself (add/remove tenants). Each `HnswGraph` within the map retains its own `RwLock` for reader-writer separation during search and insert. This two-level locking avoids holding a global lock during HNSW search.

### 6. Tenant-Aware Engine Operations

Every public engine operation must accept a tenant context. This is the most pervasive change in Phase 13 -- it touches every operation's signature and call site.

**Design: `TenantContext` parameter, not a per-tenant engine instance.**

Creating a separate `Engine` instance per tenant would duplicate the embedder, the ULID generator, the subscription registry, and the background workers. Instead, a `TenantContext` struct is threaded through every operation. The `TenantContext` contains the `tenant_id` and is used to construct a `TenantScopedStorage` wrapper on each call.

**Call flow (after Phase 13):**

1. gRPC/REST request arrives with `Authorization` header.
2. Auth middleware validates the key, resolves the `tenant_id`, checks permissions.
3. Rate limit middleware checks the tenant's rate limiter.
4. Handler extracts `TenantContext` from request extensions.
5. Handler calls `engine.remember(tenant_ctx, input)` (or any operation).
6. Engine constructs a `TenantScopedStorage` wrapping the shared storage backend with the tenant's scope.
7. Engine calls `IndexManager` with the tenant scope, which selects the tenant's HNSW graph and uses the scoped storage for temporal and graph queries.
8. All reads and writes go through the scoped storage, guaranteeing isolation.

**Single-tenant mode:**

For deployments that do not need multi-tenancy (local development, embedded mode, single-agent systems), a `default` tenant is used implicitly. The `--no-auth` flag (which disables auth) also implicitly sets the tenant to `default`. The engine code path is identical -- there is no "multi-tenant vs. single-tenant" branch. Every deployment is multi-tenant; single-tenant deployments have exactly one tenant called `default`.

**Why not a global "tenant registry" with lifecycle management:** Phase 13 tenants are implicit -- they come into existence when the first key is created for them and the first memory is remembered. There is no `create_tenant` / `delete_tenant` API. Tenant deletion (purging all data) is a `forget` operation with a tenant-wide scope, followed by key revocation. Explicit lifecycle management is a Phase 15 deployment concern.

### 7. Rate Limiting Design

Rate limiting prevents a single tenant from monopolizing server resources and provides backpressure to misbehaving clients.

**Algorithm: token bucket per (tenant, operation_class).**

| Alternative | Burst handling | Fairness | Memory per tenant | Implementation complexity | Verdict |
|-------------|---------------|----------|-------------------|--------------------------|---------|
| Fixed window | Poor (boundary spike) | Poor | 16 bytes | Trivial | Unfair at window boundaries |
| Sliding window log | Excellent | Excellent | O(requests) | Medium | Unbounded memory per tenant |
| Sliding window counter | Good | Good | 32 bytes | Low | Good but two-window approximation |
| Token bucket | Excellent | Excellent | 24 bytes | Low | Best balance of fairness, memory, and burst handling |
| Leaky bucket | Good | Excellent | 24 bytes | Low | No burst tolerance |

Token bucket wins because it supports burst traffic (common for agent workloads: idle → sudden recall storm → idle) while maintaining a sustained rate limit. Each bucket stores three values: `tokens_remaining` (f64), `last_refill_timestamp` (u64), and `max_tokens` (f64). Refill is computed lazily on each check: `elapsed * rate` tokens are added (capped at `max_tokens`). Total memory per (tenant, operation_class) pair: 24 bytes. At 10,000 tenants × 3 operation classes = 720KB. Negligible.

**Operation classes:**

Rate limits are grouped into three classes, not per-operation, to keep configuration manageable:

| Class | Operations | Default sustained rate | Default burst |
|-------|-----------|----------------------|---------------|
| `write` | `remember`, `revise`, `forget` | 1,000 ops/sec | 5,000 |
| `read` | `recall`, `prime`, `subscribe`, `insights`, `get` | 5,000 ops/sec | 10,000 |
| `admin` | `reflect`, `reflect_policy`, key management | 10 ops/sec | 20 |

**Response signaling:**

When a request is rate-limited:
- gRPC: `Status::resource_exhausted()` with `retry-after-ms` metadata header indicating when the next token will be available.
- REST: HTTP 429 with `Retry-After` header (seconds) and `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` headers on every response (not just 429).

**Subscribe backpressure:** Subscribe streams are not rate-limited by the token bucket (they are long-lived streams, not discrete requests). Instead, the existing output buffer backpressure (Phase 6: drop-oldest when full) remains the mechanism. Phase 13 adds a per-tenant maximum concurrent subscription count (default: 10) enforced by the `SubscriptionRegistry`.

### 8. Error Taxonomy Extensions

Phase 13 introduces four new error variants. These are added to the existing `HebbsError` enum as non-exhaustive variants (per Phase 1's error design principle).

| Variant | When | gRPC Status | HTTP Status | Retryable |
|---------|------|------------|-------------|-----------|
| `Unauthorized` | Missing or invalid API key | `UNAUTHENTICATED` (16) | 401 | No (fix the key) |
| `Forbidden` | Valid key but insufficient permissions for this operation | `PERMISSION_DENIED` (7) | 403 | No (use a different key) |
| `RateLimited { retry_after_ms }` | Tenant's rate limit exceeded for this operation class | `RESOURCE_EXHAUSTED` (8) | 429 | Yes (after `retry_after_ms`) |
| `TenantNotFound { tenant_id }` | Operation references a tenant with no data | `NOT_FOUND` (5) | 404 | No |

**Error context:** Following Phase 1's principle that every error carries structured context, each variant includes:
- `Unauthorized`: which endpoint was called, whether the issue was a missing header, malformed token, expired key, or unknown key.
- `Forbidden`: which endpoint, which permission was required, which permissions the key actually has.
- `RateLimited`: which operation class, current rate, limit, and when the next token will be available.
- `TenantNotFound`: the tenant ID that was referenced.

### 9. Configuration Extensions

Phase 13 adds three new sections to `hebbs.toml`. Existing sections are unchanged -- backward compatibility is maintained (Principle: additive only).

**`[auth]` section:**

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Master switch. `false` is equivalent to `--no-auth`. |
| `keys_file` | Option<String> | None | Path to a separate file containing key definitions (for operators who do not want keys in the main config). |

**`[auth.tls]` section (optional, for mTLS):**

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Enable TLS on gRPC transport. |
| `cert_path` | String | required if enabled | Path to server certificate PEM file. |
| `key_path` | String | required if enabled | Path to server private key PEM file. |
| `client_ca_path` | Option<String> | None | Path to client CA certificate PEM file. If set, enables mutual TLS (clients must present a cert signed by this CA). |

**`[tenancy]` section:**

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `max_tenants` | u32 | 10,000 | Maximum number of distinct tenants. Prevents runaway tenant creation. |
| `max_memories_per_tenant` | u64 | 10,000,000 | Per-tenant memory count limit. Enforced on `remember()`. |
| `hnsw_eviction_secs` | u64 | 3,600 | Idle time before a tenant's HNSW graph is evicted from memory. 0 = never evict. |
| `max_loaded_hnsw` | u32 | 100 | Maximum number of tenant HNSW graphs held in memory simultaneously. LRU eviction when exceeded. |

**`[rate_limit]` section:**

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Master switch. |
| `write_rate` | f64 | 1000.0 | Sustained write operations per second per tenant. |
| `write_burst` | u32 | 5000 | Maximum burst capacity for writes. |
| `read_rate` | f64 | 5000.0 | Sustained read operations per second per tenant. |
| `read_burst` | u32 | 10000 | Maximum burst capacity for reads. |
| `admin_rate` | f64 | 10.0 | Sustained admin operations per second per tenant. |
| `admin_burst` | u32 | 20 | Maximum burst capacity for admin operations. |

**Environment variable overrides** follow the existing convention: `HEBBS_AUTH_ENABLED`, `HEBBS_TENANCY_MAX_TENANTS`, `HEBBS_RATE_LIMIT_WRITE_RATE`, etc.

**Sensitive value redaction:** `hebbs-server config-dump` now redacts fields annotated as sensitive. The `keys_file` path is shown but its contents are not. TLS key paths are shown but their contents are not. This resolves the deferred item from Phase 8.

### 10. Proto Schema Extensions

All request messages gain a `tenant_id` field. This field was reserved in Phase 8's design but not added to the proto schema. Phase 13 adds it.

**Design: tenant_id as a request-level field, not metadata.**

The `tenant_id` is a field in the protobuf request message, not a gRPC metadata key. This is deliberate: metadata is for transport-level concerns (auth tokens, tracing IDs). Tenant identity is a domain concept -- it scopes the operation's data. Keeping it in the message makes it visible in proto documentation, validated by proto schema tools, and explicitly passed by client code.

However, in practice, the `tenant_id` in the request message is **optional and secondary**. The primary source of truth for tenant identity is the API key (resolved by the auth middleware). The request message's `tenant_id` field is used only for validation: if present, it must match the key's tenant. If absent, the key's tenant is used. This prevents a misconfigured client from accidentally operating on the wrong tenant.

**SetPolicy RPC (deferred from Phase 8):**

The `ReflectService` gains a `SetPolicy` RPC that accepts a `ReflectPolicyRequest` with trigger configuration (threshold count, schedule interval, enabled flag). This is the runtime-settable alternative to the `[reflect]` TOML section. The RPC requires `admin` permission. Policy is stored per-tenant in the meta CF.

### 11. Soak Test Methodology

The 72-hour soak test is the definitive evidence that the system is production-ready. Phase 12 built the measurement harness; Phase 13 runs it.

**Test configuration:**

- Duration: 72 hours continuous
- Load profile: 100,000 operations per second sustained, mixed workload
- Workload mix: 40% remember, 30% recall (mixed strategies), 10% prime, 10% revise, 5% forget, 5% subscribe lifecycle
- Tenant count: 100 active tenants (simulating a realistic multi-tenant deployment)
- Memory count: starts at 0, grows continuously throughout the test
- Hardware: dedicated machine (not shared CI runner) with 8+ cores, 32GB+ RAM, NVMe storage

**Monitored metrics (sampled every 10 seconds):**

| Metric | Pass criterion |
|--------|---------------|
| `remember` p99 latency | < 5ms throughout (Principle 1 budget) |
| `recall` similarity p99 latency | < 10ms throughout (Principle 1 budget) |
| `recall` temporal p99 latency | < 5ms throughout (Principle 1 budget) |
| p99 latency drift | < 5% between hour 1 and hour 72 |
| RSS memory | Monotonically bounded (no leaks); growth rate < 1% per hour after initial ramp |
| Disk growth rate | Linear with memory count (no amplification beyond 2x write amp) |
| File descriptor count | Stable (no FD leaks) |
| Thread count | Stable (no thread leaks) |
| Error rate | < 0.001% (excluding rate-limited rejections) |
| RocksDB compaction backlog | Never exceeds 10 pending compactions |

**Failure response:** If any monitored metric violates its criterion, the soak test fails. The failure is investigated (is it a bug or a tuning issue?), fixed, and the 72-hour test is restarted from zero. Partial passes do not count.

### 12. Security Audit Framework

Phase 13 conducts a systematic security review, not a penetration test. The audit is a checklist-driven process that every Phase 13 deliverable must pass before the phase is considered complete.

**Audit categories:**

**A. Input validation completeness:**
- Every gRPC and REST handler validates every field before it reaches the engine.
- Content length limits enforced (64KB max, per Phase 1).
- UTF-8 validity verified on all string inputs.
- Context depth limits enforced (max nesting depth: 10 levels, max context serialized size: 16KB).
- Numeric parameter range checks (importance in [0.0, 1.0], top_k in [1, 1000], max_depth in [0, 10]).
- Tenant ID format validation (alphanumeric + hyphens, 1-128 characters).
- API key format validation (must start with `hb_`, correct length).

**B. Dependency audit:**
- `cargo audit` passes with zero unaddressed advisories.
- Dependency tree is reviewed for unnecessary transitive dependencies.
- Every direct dependency justifies its presence.
- `ort` pre-release status assessed -- if stable release is available, upgrade.

**C. Unsafe code audit:**
- Every `unsafe` block in the workspace has a written safety invariant comment.
- No `unsafe` blocks on paths reachable by external input without a documented safety proof.
- FFI boundary (`hebbs-ffi`) receives special scrutiny: every pointer dereference, every lifetime assumption.

**D. Sensitive data handling:**
- API keys are never logged (even at trace level).
- `config-dump` redacts sensitive fields.
- Memory content is not included in error messages (memory ID only).
- TLS private keys are read once and held in memory; file permissions are checked on load (warn if world-readable).

**E. Denial of service resistance:**
- Rate limiting is enabled by default.
- All bounded resource limits (Principle 4) are verified: max content length, max batch size, max subscriptions, max memories per tenant, max tenants.
- HNSW search is bounded by `ef_search` (no unbounded traversal).
- Graph traversal is bounded by `max_depth`.
- Reflect scope is bounded by `max_memories_per_reflect`.
- Subscribe bloom filter rebuild is bounded.

### 13. Data Integrity Hardening

**Checksum verification on deserialization:**

Currently, bitcode deserialization trusts the stored bytes. If RocksDB's internal checksums pass but the application-level data is corrupted (e.g., a bug in a future migration writes invalid bytes), deserialization may produce silently wrong data or a confusing deserialization error.

Phase 13 adds an optional CRC-32C checksum to serialized memory records. The checksum is appended as the last 4 bytes of the serialized payload. On deserialization, if the checksum bytes are present (detected by a version flag in the meta CF), they are verified before the bitcode decode. This adds ~2ns of overhead per read (CRC-32C is hardware-accelerated on modern CPUs via SSE 4.2).

**Why CRC-32C:** RocksDB already uses CRC-32C for block checksums. The CPU instruction is the same. No new dependency. 32 bits of checksum provides a 1-in-4-billion chance of undetected corruption, which is sufficient for an application-level defense-in-depth layer on top of RocksDB's own checksums.

**Why optional (version-flagged):** Existing data written before Phase 13 does not have checksums. The meta CF stores a `checksum_version` key. If absent or 0, checksums are not verified. If 1, CRC-32C is expected. New writes always include checksums. Old data is checksummed lazily on next `revise()` or during a background migration sweep.

**Crash recovery chaos testing:**

Phase 12 built the crash recovery primitive (fork, SIGKILL, verify integrity). Phase 13 runs it 1,000 times with randomized crash points across all write operations (remember, revise, forget, decay sweep, reflect consolidation). The test passes only if all 1,000 iterations result in a consistent database state after restart.

### 14. Snapshot Retention Limits

Deferred from Phase 5: a memory revised 1,000 times produces 1,000 predecessor snapshots. Phase 13 adds configurable retention.

**Configuration:**

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `max_snapshots_per_memory` | u32 | 100 | Maximum predecessor snapshots retained per memory. Oldest snapshots are pruned on next `revise()`. |

**Pruning behavior:** When a `revise()` call would exceed the limit, the oldest snapshots beyond the limit are deleted in the same `WriteBatch` as the new snapshot creation. The `RevisedFrom` graph edges pointing to pruned snapshots are also removed. This is a hard boundary, not a background sweep -- pruning happens synchronously on the revise call to prevent unbounded growth.

### 15. Subscribe via SSE (REST)

Deferred from Phase 8: subscribe streaming is available over gRPC but not HTTP. Phase 13 adds Server-Sent Events (SSE) for HTTP-only environments (browsers, edge runtimes without gRPC support).

**Endpoint:** `POST /v1/subscribe` with the subscribe configuration in the request body. The response is a `text/event-stream` with `Transfer-Encoding: chunked`. Each event is a JSON-serialized `SubscribePush`.

**Reconnection:** SSE has a built-in reconnection protocol via the `Last-Event-ID` header. Each push includes an `id` field (the push sequence number). On reconnect, the client sends `Last-Event-ID`, and the server replays pushes from the buffer (if still available). If the buffer has been evicted, the server sends a `reset` event and the client must start from scratch.

**Feed endpoint:** `POST /v1/subscribe/{subscription_id}/feed` sends text chunks to an active subscription. This mirrors the gRPC `Feed` RPC.

**Connection management:** SSE connections count against the per-tenant subscription limit. The connection is closed server-side if idle for longer than a configurable timeout (default: 5 minutes with no feed input).

### 16. Thread Pool Optimization for Subscriptions

Deferred from Phase 6: the thread-per-subscription model may not scale if profiling reveals thread creation overhead is significant.

**Decision criteria (from Phase 12 profiling):** If subscription creation latency is >1ms or if the system shows thread count instability under rapid subscribe/unsubscribe cycles, replace the thread-per-subscription model with a fixed-size thread pool.

**Thread pool design (if needed):** A `crossbeam-channel` work-stealing pool with `N` worker threads (default: `min(available_cores, 16)`). Subscriptions are dispatched to workers via a shared job queue. Each worker handles multiple subscriptions, checking each subscription's accumulator in a round-robin loop with a configurable poll interval.

**If profiling shows thread creation is not a bottleneck:** No change. Document the finding and close the deferred item.

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| Key-prefix isolation has a bug that allows cross-tenant reads via a malformed prefix scan | Critical -- data leak between tenants | `TenantScopedStorage` is a dedicated wrapper with exhaustive unit tests: every method verified to prepend/strip prefix correctly. Property-based tests with two tenants verify that no operation on tenant A ever returns data from tenant B. Integration test: populate two tenants, verify complete isolation across all 9 operations. |
| Per-tenant HNSW graphs consume excessive memory with many active tenants | High -- OOM kills, degraded performance | LRU eviction with configurable limits (`max_loaded_hnsw`). Monitoring via Prometheus gauge (`hnsw_loaded_tenants`). Memory usage per tenant is bounded by `max_memories_per_tenant`. Alert at 80% of configured memory ceiling. |
| API key cache becomes a bottleneck under high concurrency | Medium -- auth becomes the latency bottleneck | `parking_lot::RwLock` with reader preference ensures zero contention on the hot path (read-only lookups). Write lock is only taken during key creation/revocation (rare admin operations). Benchmark auth overhead in the soak test: must be < 1µs per request. |
| Token bucket rate limiter drift under high concurrency | Low -- rate limits are slightly inaccurate | Each bucket uses `AtomicU64` for the token count and `Instant` for the last refill. CAS loop for atomic token decrement. No mutex. Drift under contention is bounded by CAS retry time (~nanoseconds). |
| 72-hour soak test passes on test hardware but fails on production hardware | Medium -- false confidence | Document exact hardware specifications. Provide `hebbs-bench soak` subcommand so operators can run the soak test on their own hardware. Soak test configuration is committed to the repository. |
| Adding `tenant_id` to all proto messages breaks existing client SDKs | High -- backward compatibility violation | The field is optional (default empty string). Existing clients that do not send `tenant_id` work against a server running in `--no-auth` mode (default tenant). Clients that upgrade to Phase 13 SDKs send `tenant_id` explicitly. Proto field addition is always backward-compatible in protobuf. |
| mTLS configuration complexity discourages adoption | Low -- mTLS is optional | mTLS is off by default. Plaintext is the default for development. Documentation includes a step-by-step mTLS setup guide with `openssl` commands for generating test certificates. |
| Rate limit state is lost on server restart | Low -- limits reset, brief burst allowed | Acceptable trade-off. Rate limit state is ephemeral (in-memory only). Persisting rate limit state to disk adds write overhead on the hot path for marginal benefit. Token buckets refill quickly after restart. |
| Snapshot pruning on `revise()` adds latency to the write path | Medium -- revise latency increases | Pruning only occurs when the snapshot count exceeds the limit (rare for most memories). The pruning deletes are batched into the same `WriteBatch` as the revise write, so the cost is one additional `write_batch` entry per pruned snapshot. Benchmark revise latency with and without pruning. |

---

## Deliverables Checklist

Phase 13 is done when ALL of the following are true:

### Authentication

- [ ] API key generation utility: `hebbs-server key create --tenant <id> --permissions <read,write,admin> --name <label>`
- [ ] API key revocation: `hebbs-server key revoke --key-id <id>`
- [ ] API key listing: `hebbs-server key list --tenant <id>` (shows name, permissions, created_at, expires_at; never shows the key value)
- [ ] Key records stored in meta CF with SHA-256 hash (never plaintext)
- [ ] gRPC `AuthInterceptor` validates `authorization` metadata, resolves tenant, checks permissions
- [ ] REST auth middleware validates `Authorization: Bearer hb_...` header
- [ ] Health check endpoints bypass auth (always accessible)
- [ ] `--no-auth` flag disables authentication with startup warning
- [ ] Expired keys are rejected at validation time
- [ ] Multiple active keys per tenant supported (key rotation)
- [ ] Auth overhead < 1µs per request (benchmarked)
- [ ] Invalid/missing/expired key returns `Unauthorized` error with actionable message
- [ ] Insufficient permissions return `Forbidden` error with required vs. actual permissions

### mTLS

- [ ] `[auth.tls]` config section enables TLS on gRPC transport
- [ ] Server certificate and key loaded from PEM files
- [ ] Client CA certificate enables mutual TLS when configured
- [ ] Plaintext remains the default when `[auth.tls]` is absent
- [ ] TLS misconfiguration (missing files, invalid certs) produces clear startup error

### Multi-Tenancy

- [ ] `TenantScopedStorage` wrapper prepends tenant prefix to every key, strips on read
- [ ] All five CFs (default, temporal, vectors, graph, meta) are tenant-scoped
- [ ] System-level metadata uses `_system:` prefix, immune to tenant scoping
- [ ] Per-tenant HNSW graphs in `IndexManager` with lazy loading
- [ ] HNSW LRU eviction when `max_loaded_hnsw` is exceeded
- [ ] HNSW eviction after `hnsw_eviction_secs` of inactivity
- [ ] `max_memories_per_tenant` enforced on `remember()` with `CapacityExceeded` error
- [ ] `max_tenants` enforced on first operation for a new tenant
- [ ] Single-tenant mode: `default` tenant used implicitly when `--no-auth`
- [ ] Property-based test: two tenants, all 9 operations, zero cross-tenant data leakage
- [ ] Integration test: 10 tenants, concurrent operations, isolation verified

### Rate Limiting

- [ ] Token bucket implementation with lazy refill
- [ ] Per (tenant, operation_class) rate limiting
- [ ] Three operation classes: write, read, admin
- [ ] `[rate_limit]` config section with per-class rate and burst
- [ ] gRPC returns `RESOURCE_EXHAUSTED` with `retry-after-ms` metadata
- [ ] REST returns 429 with `Retry-After`, `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` headers
- [ ] Rate limit headers included on all REST responses (not just 429)
- [ ] Per-tenant concurrent subscription limit enforced
- [ ] Rate limiting disabled cleanly when `rate_limit.enabled = false`

### Error Taxonomy

- [ ] `Unauthorized` variant added to `HebbsError`
- [ ] `Forbidden` variant added to `HebbsError`
- [ ] `RateLimited { retry_after_ms }` variant added to `HebbsError`
- [ ] `TenantNotFound { tenant_id }` variant added to `HebbsError`
- [ ] All new variants carry structured context per Phase 1 error design principles
- [ ] gRPC status code mapping correct for all new variants
- [ ] HTTP status code mapping correct for all new variants

### Proto and API

- [ ] `tenant_id` field added to all request messages (optional, validated against key's tenant)
- [ ] `SetPolicy` RPC implemented in `ReflectService`
- [ ] REST `/v1/subscribe` SSE endpoint implemented
- [ ] REST `/v1/subscribe/{id}/feed` endpoint implemented
- [ ] Client SDK `api_key()` builder method functional
- [ ] Client SDK `tls_config()` builder method functional
- [ ] CLI `--api-key` flag functional
- [ ] CLI `--tls-cert` and `--tls-key` flags functional

### Configuration

- [ ] `[auth]` config section parsed and applied
- [ ] `[auth.tls]` config section parsed and applied
- [ ] `[tenancy]` config section parsed and applied
- [ ] `[rate_limit]` config section parsed and applied
- [ ] Environment variable overrides for all new config keys
- [ ] `config-dump` redacts sensitive values (key file contents, TLS key paths)
- [ ] Missing new sections use defaults (backward compatible with existing `hebbs.toml` files)

### Data Integrity

- [ ] CRC-32C checksum on serialized memory records (new writes)
- [ ] Checksum verification on deserialization (version-flagged, old data skipped)
- [ ] Crash recovery chaos test: 1,000 iterations, all pass
- [ ] Crash during `remember()` with tenant-scoped storage: integrity verified
- [ ] Crash during multi-tenant concurrent writes: isolation verified after restart

### Soak Test

- [ ] 72-hour soak test completed on dedicated hardware
- [ ] 100K ops/sec sustained throughout
- [ ] p99 latency drift < 5% between hour 1 and hour 72
- [ ] No memory leaks (RSS growth rate < 1% per hour after ramp)
- [ ] No file descriptor leaks
- [ ] No thread leaks
- [ ] Error rate < 0.001%
- [ ] Soak test configuration and results committed to repository

### Security Audit

- [ ] Input validation audit: all handlers, all fields, all limits
- [ ] `cargo audit` passes (zero unaddressed advisories)
- [ ] No `unwrap()` or `expect()` on external-input paths in new code
- [ ] No `unsafe` blocks in new code (or each justified with safety invariant)
- [ ] API keys never logged
- [ ] Memory content never in error messages
- [ ] TLS key file permissions warned if world-readable

### Deferred Items Resolved

- [ ] Snapshot retention limits: `max_snapshots_per_memory` config, pruning on `revise()`
- [ ] `SetPolicy` RPC: runtime reflect policy management
- [ ] REST Subscribe via SSE: `POST /v1/subscribe` with event stream
- [ ] Config dump redaction: sensitive values masked
- [ ] Client SDK auth activation: `api_key()` and `tls_config()` functional
- [ ] CLI auth flags: `--api-key`, `--tls-cert`, `--tls-key` functional
- [ ] Thread pool optimization for subscriptions: profiling result documented, optimization applied if warranted

### Code Quality

- [ ] Zero clippy warnings across the workspace
- [ ] `cargo fmt --check` passes
- [ ] `cargo audit` passes
- [ ] No `unwrap()` or `expect()` on external-input paths
- [ ] No `unsafe` blocks without written safety invariants
- [ ] All new public types have doc comments
- [ ] Benchmark regression gate passes (Phase 12 baseline not regressed)

---

## Interfaces Published to Future Phases

Phase 13 creates contracts that later phases depend on. These interfaces are stable after Phase 13 and should not change without a documented migration plan.

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| `TenantScopedStorage` wrapper and key-prefix encoding scheme | 17 (sync scoped per tenant), 15 (tenant-aware deployment) | Prefix format is immutable after Phase 13. Adding new CFs must follow the same prefix convention. |
| `TenantContext` struct and its propagation pattern | 17 (sync operations are tenant-scoped), 18/19 (SDKs pass tenant context), 15 (deployment tooling) | Fields are additive only (can add, never remove). |
| API key format (`hb_` prefix, 256-bit random, SHA-256 stored) | 17 (edge devices authenticate to hub), 18/19 (SDK clients use API keys), 15 (deployment scripts create keys) | Key format is immutable. New key types (e.g., device keys for Phase 17) use a different prefix (`hb_dev_`). |
| Permission model (read/write/admin bitmask) | 17 (edge sync needs write permission), 18/19 (SDKs document required permissions), 15 (deployment guides) | Bitmask is additive (new bits for new permissions, existing bits never change meaning). |
| Auth middleware composition pattern (interceptor chain) | 17 (adds sync-specific auth), 15 (external IdP integration adds a new interceptor) | New interceptors are appended, never replace existing ones. |
| Rate limit configuration schema | 15 (Helm chart templates reference rate limit keys) | Additive only (new operation classes, never remove existing ones). |
| `[auth]`, `[tenancy]`, `[rate_limit]` config sections | 17 (adds `[sync]` section), 15 (Helm/Terraform templates) | Section names and key semantics are stable. New keys with defaults for backward compatibility. |
| Per-tenant HNSW lazy loading and eviction | 17 (edge mode has a single tenant, no eviction needed), 15 (deployment sizing depends on eviction behavior) | Eviction policy is configurable. Behavior under eviction (rebuild from CF) is a stable contract. |
| SSE subscribe endpoint (`/v1/subscribe`) | 18 (TypeScript SDK uses SSE in browser environments) | Endpoint path, event format, and reconnection protocol are stable. |
| Soak test configuration and methodology | 15 (operators run soak tests on production hardware) | `hebbs-bench soak` subcommand interface is additive only. |
