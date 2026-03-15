# TASK-04: Tenant & Key Management + E2E Isolation Tests

## Status: Planned

## Problem

HEBBS supports multi-tenancy at the engine level (`tenant_id` provides hard storage isolation), but the server currently has no API for creating tenants or generating additional API keys. On startup with auth enabled, a single bootstrap admin key is generated for the `"default"` tenant. There is no way to:

1. Create a new tenant
2. Generate an API key bound to a specific tenant
3. List/revoke keys for a tenant (beyond the in-memory `KeyCache` methods that exist but are unexposed)

This gap blocks:

- **E2E testing of tenant isolation** — tests need two API keys bound to different tenants, but the server only emits one bootstrap key.
- **Production multi-tenant deployments** — a SaaS platform embedding HEBBS cannot onboard multiple customers without manually manipulating storage.
- **Experience demo with auth** — the demo currently relies on no-auth mode to use per-session `tenant_id` via the proto field.

---

## Current State

### What exists

- `hebbs-core/src/auth.rs` — `generate_key(tenant_id, name, permissions, expires_at)` creates a key record. `KeyCache` has `insert()`, `revoke()`, `list_for_tenant()`.
- `hebbs-core/src/tenant.rs` — `TenantContext` with validation (alphanumeric + hyphens/underscores, 1-128 chars).
- `hebbs-server/src/server.rs` — Bootstrap key generation on first startup (tenant=`"default"`, permissions=read+write+admin).
- `hebbs-server/src/middleware.rs` — `resolve_tenant()` honors proto `tenant_id` when auth yields default tenant (added in this session).
- All gRPC handlers use `resolve_tenant()` to allow client-specified tenant when auth is disabled.

### What is missing

- **gRPC/REST endpoints** for key management (CreateKey, RevokeKey, ListKeys).
- **Proto definitions** for key management messages.
- **CLI commands** for tenant/key operations.
- **SDK methods** for key management in Python, TypeScript, and Rust SDKs.
- **TypeScript E2E tests** for tenant isolation (blocked by single-key limitation).

---

## Deliverables

### 1. Key Management API

Add to `hebbs.proto`:

```protobuf
service AdminService {
  rpc CreateKey(CreateKeyRequest) returns (CreateKeyResponse);
  rpc RevokeKey(RevokeKeyRequest) returns (RevokeKeyResponse);
  rpc ListKeys(ListKeysRequest) returns (ListKeysResponse);
}

message CreateKeyRequest {
  string tenant_id = 1;
  string name = 2;
  uint32 permissions = 3;       // bitmask: 1=read, 2=write, 4=admin
  optional uint64 expires_at = 4;
}

message CreateKeyResponse {
  string raw_key = 1;           // shown once, never stored
  string tenant_id = 2;
  string name = 3;
  uint64 created_at = 4;
}

message RevokeKeyRequest {
  string key_prefix = 1;        // first 12 chars of the key for identification
}

message RevokeKeyResponse {
  bool revoked = 1;
}

message ListKeysRequest {
  optional string tenant_id = 1;
}

message ListKeysResponse {
  repeated KeyInfo keys = 1;
}

message KeyInfo {
  string tenant_id = 1;
  string name = 2;
  uint32 permissions = 3;
  uint64 created_at = 4;
  optional uint64 expires_at = 5;
}
```

### 2. Server Implementation

- `grpc/admin_service.rs` — implement the three RPCs, requiring `PERM_ADMIN`.
- `rest.rs` — add `/v1/keys` REST endpoints (POST create, DELETE revoke, GET list).
- All key management operations require the admin permission.

### 3. CLI Commands

```
hebbs-cli key create --tenant <id> --name <label> [--permissions read,write,admin] [--expires <duration>]
hebbs-cli key list [--tenant <id>]
hebbs-cli key revoke --prefix <key-prefix>
```

### 4. SDK Methods (all three SDKs)

```python
# Python
client.create_key(tenant_id="acme", name="prod-key", permissions=["read", "write"])
client.list_keys(tenant_id="acme")
client.revoke_key(key_prefix="hb_EsmBJRiW")
```

### 5. TypeScript E2E Tests — Tenant & Entity Isolation

Add a new section to `hebbs-typescript/tests/e2e/e2e.test.ts`:

#### Tenant isolation tests (require admin key to create two tenant-scoped keys)

| Test | What it validates |
|------|-------------------|
| `tenant: remember in A, recall from B -> 0 results` | Hard memory isolation between tenants |
| `tenant: get by ID across tenants -> NotFound` | Get is tenant-scoped |
| `tenant: forget does not cross tenants` | Forget-by-entity respects tenant boundary |
| `tenant: reflect/insights are tenant-scoped` | Reflect pipeline is isolated |

Test setup: use admin key to `createKey(tenant_id="e2e-tenant-a")` and `createKey(tenant_id="e2e-tenant-b")`, then run isolation assertions with those keys.

#### Entity scoping tests (work with any single key)

| Test | What it validates |
|------|-------------------|
| `entity: recall scoped by entity_id` | Temporal recall is entity-scoped; similarity is cross-entity |
| `entity: prime scoped by entity_id` | Prime returns only the target entity's memories |
| `entity: forget by entity_id is selective` | Forget-by-entity removes only that entity, others survive |

Entity tests are unblocked today and can be added immediately.

---

## Cross-Component Propagation

Per the consistency rules, this change touches:

1. **Proto** — new `AdminService` + messages
2. **hebbs-proto** — regenerate
3. **Server gRPC** — new `admin_service.rs`
4. **Server REST** — new `/v1/keys` routes
5. **Rust client SDK** — add key management methods
6. **CLI** — add `key create/list/revoke` commands
7. **Python SDK (gRPC)** — add key management methods
8. **TypeScript SDK (gRPC)** — add key management methods
9. **FFI/PyO3** — expose if needed
10. **hebbs-docs** — API reference, CLI commands, SDK references, server config, key-concepts

---

## Priority

Medium. The entity scoping tests (3 tests) can be added immediately without this work. The tenant isolation tests (4 tests) and production multi-tenancy are blocked until key management is implemented.

---

## Immediate Action (no blockers)

Add the 3 entity scoping E2E tests to `hebbs-typescript/tests/e2e/e2e.test.ts` now — they work with the existing single API key and validate `entity_id` behavior across recall, prime, and forget.
