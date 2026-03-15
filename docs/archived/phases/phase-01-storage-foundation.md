# Phase 1: Storage Foundation -- Architecture Blueprint

## Status: ✅ COMPLETE

All deliverables met. 69 tests passing (30 unit, 18 storage, 12 integration, 9 property-based). Criterion benchmarks established. Zero clippy warnings, zero fmt issues, zero `unsafe`, zero `unwrap()` on external paths.

---

## Intent

Phase 1 establishes the bedrock that every subsequent phase builds on. The decisions made here -- serialization format, key encoding scheme, column family layout, error taxonomy, trait boundaries -- will be load-bearing for the next 16 phases. Changing them later is a rewrite, not a refactor.

This phase produces two crates: `hebbs-storage` and `hebbs-core`. By the end, a caller can `remember()` a memory and retrieve it by ID. Nothing more. But the interfaces, data layouts, and abstractions must be designed to support everything that follows: three index types, four recall strategies, decay, sync, multi-tenancy, and edge deployment.

---

## Scope Boundaries

### What Phase 1 delivers

- Cargo workspace with `hebbs-storage` and `hebbs-core` crates
- The canonical `Memory` data model with serialization
- RocksDB integration with column family strategy
- Key encoding scheme for all column families (even those unused until later phases)
- `remember()` operation: write a memory, get back an ID
- `get()` by memory ID
- `delete()` by memory ID
- Iterator by entity prefix
- Error taxonomy for the full project (not just Phase 1)
- Criterion benchmark baseline for raw write/read throughput
- Property-based tests for serialization round-trips

### What Phase 1 explicitly does NOT deliver

- Embeddings (Phase 2)
- Any index updates on write (Phase 3)
- Any form of `recall()` (Phase 4)
- Networking, gRPC, HTTP (Phase 8)
- Configuration file loading (Phase 8)
- Multi-tenancy (Phase 12)

These are listed because the temptation to "just add a little bit" is strong. Resist. Phase 1 is about getting the foundation right, not getting features out.

---

## Architectural Decisions

### 1. Workspace Structure

The workspace starts with two crates but must be structured to accept ten.

```
hebbs/
  Cargo.toml            (workspace definition)
  crates/
    hebbs-core/
      Cargo.toml
      src/
        lib.rs
    hebbs-storage/
      Cargo.toml
      src/
        lib.rs
```

**Dependency direction is strictly one-way:** `hebbs-core` depends on `hebbs-storage`. Never the reverse. `hebbs-storage` knows nothing about memories, importance, or cognitive concepts. It is a key-value storage abstraction that happens to use RocksDB underneath.

**Why this separation matters:** In Phase 9, `hebbs-ffi` will link `hebbs-core` for embedded mode. In Phase 8, `hebbs-server` will wire `hebbs-core` to network handlers. If `hebbs-core` directly instantiates RocksDB, these downstream crates cannot swap storage implementations for testing or alternative deployments. The storage trait boundary is the escape hatch.

### 2. The Memory Data Model

The `Memory` struct is the single most important type in the system. Every other module reads it, writes it, queries it, or transforms it. Its design must accommodate all 17 phases.

**Fields and their rationale:**

| Field | Type | Why |
|-------|------|-----|
| `memory_id` | ULID (128-bit) | Globally unique without coordination. Sortable by time. Critical for edge sync in Phase 13 where two offline devices generate IDs independently. UUIDv7 is an alternative but ULID has wider Rust ecosystem support and identical properties. |
| `content` | String | The raw experience text. Bounded by max length (default 64KB, configurable). |
| `importance` | f32 | 0.0 to 1.0. Drives decay scoring, recall ranking, and conflict resolution in sync. Not optional -- defaults to 0.5 if unset, because cognition requires prioritization (Principle 3). |
| `context` | Structured map (String -> Value) | Arbitrary metadata: entity_id, tags, stage, signal type. Stored as serialized bytes, not as separate RocksDB keys. Queried only after deserialization. Not indexed directly -- the index layer (Phase 3) extracts what it needs. |
| `entity_id` | Option<String> | Extracted from context for temporal index key prefix. Optional because not all memories belong to an entity. When present, enables O(log n) temporal range scans in Phase 3. |
| `embedding` | Option<Vec<f32>> | None in Phase 1. Populated by Phase 2. The field exists from day one because adding it later changes the serialization format and requires a migration. |
| `created_at` | u64 (microseconds since epoch) | Microsecond precision. Stored as u64 rather than a datetime struct for compact serialization, deterministic ordering, and zero-cost comparisons. |
| `updated_at` | u64 | Set equal to `created_at` on initial write. Updated by `revise()` in Phase 5. |
| `last_accessed_at` | u64 | Updated on every `recall()` hit in Phase 4. Drives decay scoring in Phase 5. |
| `access_count` | u64 | Incremented on every recall hit. Part of the reinforcement signal: `decay_score = importance * 2^(-age/half_life) * log(1 + access_count)`. |
| `decay_score` | f32 | Computed, not stored long-term. Recalculated on read from `importance`, `created_at`, `last_accessed_at`, and `access_count`. Storing it would create a stale-cache problem -- the score changes with time even without writes. However, a cached `decay_score` is stored for the background decay sweep to avoid recalculating for every memory. |
| `kind` | Enum (Episode, Insight, Revision) | Distinguishes raw episodes from consolidated insights (Phase 7) and revised versions (Phase 5). The enum exists from Phase 1 but only `Episode` is used initially. Future variants do not require schema migration. |
| `device_id` | Option<String> | For edge sync (Phase 13). Which device created this memory. None for cloud-only deployments. Exists from Phase 1 to avoid schema migration. |
| `logical_clock` | u64 | For conflict resolution in sync (Phase 13). Monotonically increasing per device. Default 0 in Phase 1. |

**Design decision: include future fields now vs add later.** Including them now adds ~40 bytes per memory of overhead (mostly Options that serialize as single zero bytes). Adding them later requires a serialization migration that touches every stored memory. At 10M memories, a migration is a multi-hour offline operation. The overhead is negligible. Include them.

### 3. Serialization Format

**Decision: bitcode (gamma-encoded lengths, bitwise packing)**

| Candidate | Size (typical Memory) | Serialize speed | Deserialize speed | Schema evolution | Verdict |
|-----------|----------------------|----------------|-------------------|-----------------|---------|
| JSON | ~800 bytes | Slow | Slow | Flexible | Too large, too slow for hot path |
| MessagePack | ~350 bytes | Fast | Fast | Limited | Good but no schema evolution |
| bincode v2 | ~280 bytes | Fast | Fast | Limited (append-only) | Good performance but larger output than bitcode |
| bitcode | ~200-220 bytes | Fastest | Fastest | Limited (append-only) | Smallest output, fastest performance, compresses well under LZ4/Zstd |
| Protocol Buffers | ~320 bytes | Fast | Fast | Excellent | Would add protobuf dependency to storage crate; reserved for network layer |
| FlatBuffers | ~300 bytes | Zero-copy read | Fastest read | Good | Complex API, limited Rust ergonomics |

**Why bitcode:** At 10M memories, the difference between 200 and 350 bytes per record is 1.5GB of disk. bitcode produces ~20-30% smaller output than bincode through gamma-encoded lengths and bitwise packing, while maintaining comparable or better serialization speed. The smaller payloads compress better under RocksDB's LZ4/Zstd block compression, improving block cache hit rates and reducing SST file sizes. Encoding is infallible for valid Rust types, eliminating an error path on writes. bitcode is the clear winner for an embedded engine where the network format (protobuf) is separate from the storage format.

**Schema evolution strategy:** bitcode does not support removing or reordering fields. New fields are appended with `Option<T>` wrapping. Old data missing new fields deserializes with `None`. This is sufficient because HEBBS controls both the writer and reader -- there is no third-party compatibility concern for the on-disk format.

**Migration escape hatch:** If a breaking schema change is ever required, HEBBS will support a `hebbs-server migrate` command that reads all records in the old format and rewrites them. This is an offline operation and a last resort.

### 4. RocksDB Column Family Strategy

RocksDB column families are logically separate key-value namespaces that share the same WAL but have independent LSM trees, compaction settings, and bloom filters. They are the physical isolation mechanism.

**Column families created in Phase 1 (even if unused until later):**

| Column Family | Purpose | Key Format | Value Format | First Used |
|---------------|---------|-----------|-------------|-----------|
| `default` | Memory records | `memory_id` (16 bytes, ULID) | Serialized `Memory` (bitcode) | Phase 1 |
| `temporal` | Temporal index | `entity_id + \xff + timestamp_be` (variable + 8 bytes) | `memory_id` (16 bytes) | Phase 3 |
| `vectors` | HNSW graph structure | Implementation-specific | HNSW node data | Phase 3 |
| `graph` | Causal/relational edges | `source_id + edge_type + target_id` | Edge metadata (bitcode) | Phase 3 |
| `meta` | System metadata | String keys | Various | Phase 1 |

**Why create all column families in Phase 1:** RocksDB requires column families to exist before they can be written to. Adding a column family to an existing database requires opening the DB with the new family list, which is a restart. More importantly, the column family list is part of the `DB::open` call -- if the code in Phase 3 adds column families that Phase 1 did not create, every existing database requires a migration step. Creating them empty costs nothing.

**The `meta` column family:** Stores system state that is not a memory: last reflect timestamp, decay sweep cursor, sync watermarks, schema version. Key examples: `schema_version` (u32), `memory_count` (u64), `last_decay_sweep` (u64). This keeps system bookkeeping separate from user data.

### 5. Key Encoding Scheme

Key encoding determines sort order, scan efficiency, and prefix iteration behavior. Getting this wrong is a full data rewrite.

**Principles:**

- Keys must be byte-sortable in the order that queries need. RocksDB iterates in byte order.
- Variable-length components (entity_id, strings) must be followed by a separator byte that cannot appear in the component itself. Use `\xff` as separator since entity IDs are UTF-8 (max byte value `0xF4` in valid UTF-8).
- Fixed-length components (ULID, timestamps) use big-endian encoding so byte sort = numeric sort.
- Timestamps are stored as big-endian u64 microseconds so that byte-order iteration yields chronological order.

**Default column family keys:**

Memory records are keyed by their 16-byte ULID in raw binary form. Since ULIDs are time-sortable, iterating the default column family yields memories in creation order. This is a free property that costs nothing to maintain.

**Temporal column family keys (designed now, populated in Phase 3):**

```
[entity_id bytes][0xFF][timestamp_be_u64] -> [memory_id 16 bytes]
```

This encoding enables:
- Prefix scan on `entity_id + \xff` to get all memories for an entity, in chronological order
- Range scan on `entity_id + \xff + start_ts .. entity_id + \xff + end_ts` for time-windowed queries
- Both operations are O(log n + k) where k is the result set size

**Graph column family keys (designed now, populated in Phase 3):**

```
[source_memory_id 16 bytes][edge_type u8][target_memory_id 16 bytes] -> [edge_metadata]
```

This encoding enables:
- Prefix scan on `source_memory_id` to find all outgoing edges from a memory
- Prefix scan on `source_memory_id + edge_type` to find all edges of a specific type
- Both operations are O(log n + k)

For reverse lookups (find all memories that point TO a given memory), a secondary index is needed: same key format but with source and target swapped in a separate key prefix.

### 6. Storage Trait Design

`hebbs-storage` exposes a trait, not a concrete RocksDB type. This is critical for:

- **Testing:** Unit tests in `hebbs-core` use an in-memory implementation, not RocksDB. Tests run in milliseconds, not seconds. No temp directory cleanup.
- **Embedded mode:** Phase 9 FFI can use the RocksDB implementation directly.
- **Future flexibility:** If a use case requires SQLite or a custom LSM, the trait boundary is the swap point.

**The trait covers:**

- `put(cf, key, value) -> Result<()>` -- single key-value write
- `get(cf, key) -> Result<Option<Vec<u8>>>` -- single key-value read
- `delete(cf, key) -> Result<()>` -- single key-value delete
- `write_batch(operations) -> Result<()>` -- atomic multi-key write (maps to RocksDB WriteBatch)
- `prefix_iterator(cf, prefix) -> Iterator` -- scan by key prefix
- `range_iterator(cf, start, end) -> Iterator` -- scan by key range
- `compact(cf) -> Result<()>` -- trigger compaction (for `forget` cleanup)

**What the trait does NOT expose:** RocksDB-specific concepts like snapshots, transactions, merge operators, or compaction filters. These are implementation details of the RocksDB backend. If a future backend does not support them, the trait should not promise them.

**Iterator design:** Iterators return `(key: Vec<u8>, value: Vec<u8>)` pairs. The storage layer does not deserialize. Deserialization is the responsibility of the caller (`hebbs-core`). This keeps the storage layer type-agnostic and avoids pulling the `Memory` type into `hebbs-storage`.

### 7. Error Taxonomy

Errors are defined once, project-wide, in Phase 1. Every subsequent phase adds variants to this taxonomy but does not create parallel error types.

**Error categories:**

| Category | When | Retryable | Examples |
|----------|------|-----------|---------|
| `StorageError` | RocksDB I/O failure | Sometimes | Disk full, corruption detected, WAL write failure |
| `NotFound` | Requested memory does not exist | No | Get by invalid ID, revise non-existent memory |
| `InvalidInput` | Caller provided bad data | No | Content exceeds max length, invalid UTF-8, importance out of range, malformed context |
| `SerializationError` | Data cannot be serialized or deserialized | No | Corrupt stored data, incompatible schema version |
| `CapacityExceeded` | A bounded resource hit its limit | Sometimes | Max memories per tenant, max batch size |
| `Internal` | Bug in HEBBS itself | No | Invariant violation, unreachable state |

**Error design principles:**

- Every error carries structured context: which operation, which memory_id (if applicable), which column family, what the limit was and what the actual value was.
- Error messages are actionable. A human reading the error should know what to do next.
- Errors implement `std::error::Error` via `thiserror`. No custom Display implementations -- `thiserror`'s derive handles it.
- Errors are non-exhaustive (`#[non_exhaustive]`) so that adding new variants in future phases is not a breaking change for downstream crates.

### 8. RocksDB Tuning (Phase 1 Defaults)

RocksDB has hundreds of tuning knobs. Phase 1 sets conservative defaults that prioritize correctness and reasonable performance. Aggressive tuning happens in Phase 11 (benchmarks) and Phase 12 (production hardening).

**Write path:**

- WAL enabled (durability before performance)
- `sync` on write: disabled by default (fsync on every write is too expensive for the hot path). Instead, WAL is fsynced periodically (every 1 second or every 1000 writes, whichever comes first). This means up to 1 second of data loss on power failure, which is acceptable for a memory engine where the source data (agent interactions) can be replayed.
- `write_buffer_size`: 64MB (default). Larger buffers reduce write amplification but increase memory usage. 64MB is a safe starting point.

**Read path:**

- Bloom filters enabled on all column families (10 bits per key). Reduces point-lookup disk reads from O(levels) to O(1) for keys that do not exist.
- Block cache: 256MB shared across all column families. LRU eviction. This is the single largest memory consumer and must be tunable via configuration.

**Compaction:**

- Level compaction (not universal). Level compaction has more predictable space amplification and is better suited for workloads with both reads and writes.
- Max background compaction threads: 2 (conservative, avoids saturating I/O on edge devices).
- Rate limiter: 100MB/s write rate limit during compaction. Prevents compaction from starving the read path.

**Data directory:**

- Configurable via `data_dir` parameter passed to the storage constructor.
- Default: `./hebbs-data` (relative to working directory).
- WAL is in the same directory as SST files (simplifies backup/restore).

### 9. The `remember()` Operation -- Design

`remember()` is the first operation implemented. Its design sets the pattern for all nine operations.

**Input contract:**

- `content`: required, must be valid UTF-8, max 64KB
- `importance`: optional, defaults to 0.5, must be in [0.0, 1.0]
- `context`: optional, defaults to empty map, max serialized size 16KB
- `entity_id`: optional, extracted from context if present

**Processing steps:**

1. Validate all inputs. Reject invalid input with `InvalidInput` error before any I/O.
2. Generate ULID for `memory_id`.
3. Construct `Memory` struct with all fields set (timestamps = now, access_count = 0, embedding = None, decay_score = importance, kind = Episode).
4. Serialize the `Memory` struct to bytes via bitcode.
5. Write to the `default` column family with `memory_id` as key.
6. If `entity_id` is present, write the temporal index entry. (In Phase 1, this is a no-op placeholder -- the temporal column family exists but index writes are deferred to Phase 3 where all three indexes are updated atomically in a WriteBatch.)
7. Return the `memory_id`.

**What `remember()` does NOT do in Phase 1:**

- Generate embeddings (Phase 2)
- Update any indexes (Phase 3 -- all three atomically)
- Emit metrics (added incrementally, but the hook points are designed now)
- Validate against tenant context (Phase 12)

**Why the index writes are deferred:** Principle 11 requires atomic multi-index updates. Writing to the temporal index without also writing to vector and graph indexes creates a partial index state. Phase 3 introduces WriteBatch-based atomic writes across all three indexes. Phase 1 writes only to the `default` column family.

### 10. Concurrency Model

Even in Phase 1, the concurrency model must be correct because retrofitting it later is a rewrite.

**RocksDB concurrency:** RocksDB supports concurrent reads and writes from multiple threads. Point lookups and iterators do not block each other. Writes are serialized through the WAL but this is internal to RocksDB and transparent to the caller.

**HEBBS concurrency (Phase 1):**

- All public functions on the core engine take `&self` (shared reference), not `&mut self`.
- The RocksDB handle is wrapped in `Arc<DB>` (not `Mutex<DB>`). RocksDB is internally thread-safe.
- No application-level locks on the read path. Reads go directly to RocksDB.
- Writes do not need application-level locks because RocksDB serializes them internally.

**This design enables (in future phases):**

- Phase 4: Multiple concurrent `recall()` calls without contention
- Phase 6: `subscribe()` readers do not block `remember()` writers
- Phase 8: gRPC handler spawns a tokio task per request, all sharing the same engine

### 11. Testing Strategy

**Layer 1: Unit tests (in-crate)**

- `hebbs-storage`: Test the storage trait implementation against both the RocksDB backend and an in-memory backend. Verify: put/get round-trip, delete removes, prefix scan returns correct results in correct order, range scan respects bounds, write batch is atomic (all succeed or all fail).
- `hebbs-core`: Test `remember()` against the in-memory storage backend. Verify: generated ULID is valid, timestamps are set correctly, importance defaults to 0.5, content length validation rejects oversized input, context serialization round-trips.

**Layer 2: Property-based tests**

- Serialization round-trip: for any valid `Memory` struct, `deserialize(serialize(memory)) == memory`. Use `proptest` to generate arbitrary `Memory` instances.
- Key ordering: for any two ULIDs where `a < b`, the byte representation of `a` sorts before `b` in RocksDB iteration order.
- Entity prefix isolation: memories with different entity_ids never appear in each other's prefix scans.

**Layer 3: Integration tests**

- Full round-trip through RocksDB (not in-memory): write 1,000 memories, restart the process, read them all back, verify all data intact.
- Concurrent writes: 10 threads each writing 1,000 memories simultaneously. All 10,000 memories must be readable afterward with no corruption.
- Disk full behavior: fill the temp directory, attempt a `remember()`, verify the error is `StorageError` (not a panic), verify the database is still readable after space is freed.

**Layer 4: Criterion benchmarks**

- `remember()` single write: measure p50/p99 latency for a single memory write with 200 bytes of content.
- `remember()` batch: measure throughput for 10,000 sequential writes.
- `get()` single read: measure p50/p99 latency for a point lookup by memory_id.
- `get()` miss: measure latency for a lookup of a non-existent memory_id (should be fast due to bloom filter).
- Serialization: measure serialize + deserialize time for a typical Memory struct in isolation.

These benchmarks establish the Phase 1 baseline that all future phases must not regress.

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| bitcode schema evolution is too limited for future field changes | High -- would require data migration | Include all known future fields as `Option<T>` in the initial struct. New fields are always appended with Option wrapping. |
| RocksDB Rust binding (`rust-rocksdb`) has FFI instability | Medium -- could cause segfaults | Pin the exact RocksDB version. Run under ASAN/MSAN in CI. Evaluate `rocksdb` crate vs `tikv-client`'s `tirocks` binding. |
| Key encoding scheme is wrong for a future query pattern | High -- full data rewrite | Design all key encodings in Phase 1 (including for unused column families) and validate with thought experiments against every known query pattern from Phases 3-7. |
| ULID generation is not monotonic under high concurrency | Medium -- could cause ordering issues | Use a ULID generator with monotonic mode (increments the random component if the same millisecond is hit). Verify with a concurrent generation test. |
| In-memory storage mock diverges from RocksDB behavior | Medium -- tests pass but production fails | Run the full integration test suite against both backends. Any behavioral difference is a bug in the mock. |

---

## Deliverables Checklist

Phase 1 is done when ALL of the following are true:

- [x] Cargo workspace compiles with `cargo build` (zero warnings, zero clippy lints)
- [x] `hebbs-storage` exposes a trait with put/get/delete/write_batch/prefix_iterator/range_iterator
- [x] `hebbs-storage` has a RocksDB implementation and an in-memory implementation
- [x] RocksDB opens with all five column families (default, temporal, vectors, graph, meta)
- [x] Schema version is written to `meta` column family on first open
- [x] `hebbs-core` exposes `remember()` that validates input, generates ULID, serializes, persists
- [x] `hebbs-core` exposes `get()` that retrieves and deserializes a memory by ID
- [x] `hebbs-core` exposes `delete()` that removes a memory by ID
- [x] Error types cover all six categories with structured context
- [x] No `unwrap()` or `expect()` on any path reachable by external input
- [x] No `unsafe` blocks
- [x] Property-based tests pass for serialization round-trips and key ordering
- [x] Integration test: 1,000 memories survive process restart
- [x] Integration test: 10-thread concurrent write produces zero corruption
- [x] Criterion benchmarks establish baseline for write/read latency
- [x] `cargo audit` passes (no CVEs)
- [x] `cargo clippy` passes with zero warnings
- [x] `cargo fmt --check` passes

### Implementation Notes

**Serialization choice:** bitcode with gamma-encoded lengths and bitwise packing. `serde_json::Value` does not implement bitcode's `Encode`/`Decode` traits, so the `context` field is stored as pre-serialized JSON bytes (`Vec<u8>`) within the bitcode payload. Deserialized on demand via `Memory::context()`. Encoding is infallible (`to_bytes()` returns `Vec<u8>` directly, not `Result`), eliminating an error path on the write hot path. This avoids trait bound issues while preserving the structured-metadata semantics documented above.

**ULID generation:** Uses `ulid::Generator` (monotonic mode) wrapped in `parking_lot::Mutex`. This guarantees strict ordering of IDs within the same process even at sub-millisecond speeds, addressing the risk register item about ULID monotonicity under high concurrency.

**Benchmark baseline (in-memory backend, release build on Apple Silicon):**

| Operation | Latency |
|-----------|---------|
| `remember()` single (200B content) | 384 ns |
| `get()` point lookup | 114 ns |
| `get()` miss | 102 ns |
| `serialize_memory` | 43 ns |
| `deserialize_memory` | 107 ns |
| `remember()` batch × 10,000 | 3.04 ms |

---

## Interfaces Published to Future Phases

Phase 1 creates contracts that later phases depend on. These interfaces are stable after Phase 1 and should not change without a documented migration plan.

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| `Memory` struct and its serialization format | All phases | Append-only evolution (new Option fields only) |
| Storage trait (put/get/delete/write_batch/iterators) | 3, 5, 7, 12, 13 | Additive only (new methods, never change signatures) |
| Column family names and key encoding schemes | 3, 5, 7, 12 | Immutable after Phase 1 |
| ULID generation and byte-ordering guarantee | 4, 5, 13 | Immutable |
| Error taxonomy categories | All phases | Non-exhaustive, additive only |
| `remember()` input validation rules | 8 (server), 10 (Python SDK) | Additive only (can tighten, never loosen) |
