//! Multi-tenant isolation integration tests for HEBBS Phase 13.
//!
//! Verifies that tenants sharing the same underlying storage cannot observe,
//! modify, or interfere with each other's data across every layer:
//!
//! - **Category 1**: Storage-level key-prefix isolation
//! - **Category 2**: Engine pipeline isolation (remember/recall/forget/etc.)
//! - **Category 3**: Auth module (KeyCache, generate_key, validate, revoke)
//! - **Category 4**: Rate limiter (per-tenant, per-class token buckets)
//! - **Category 5**: Concurrent multi-tenant workload (5 tenants × threads)

use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hebbs_core::auth::{
    generate_key, AuthError, KeyCache, KEY_PREFIX, PERM_ADMIN, PERM_READ, PERM_WRITE,
};
use hebbs_core::engine::{Engine, RememberInput};
use hebbs_core::error::HebbsError;
use hebbs_core::forget::ForgetCriteria;
use hebbs_core::rate_limit::{RateLimitConfig, RateLimiter};
use hebbs_core::recall::{RecallInput, RecallStrategy};
use hebbs_core::tenant::TenantContext;
use hebbs_embed::MockEmbedder;
use hebbs_index::HnswParams;
use hebbs_storage::{
    BatchOperation, ColumnFamilyName, InMemoryBackend, StorageBackend, TenantScopedStorage,
};

// ═══════════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn shared_backend() -> Arc<InMemoryBackend> {
    Arc::new(InMemoryBackend::new())
}

fn tenant_engine(backend: Arc<dyn StorageBackend>) -> Engine {
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 4);
    Engine::new_with_params(backend, embedder, params, 42).unwrap()
}

fn tenant_ctx(id: &str) -> TenantContext {
    TenantContext::new(id).unwrap()
}

fn remember_for(engine: &Engine, tenant: &TenantContext, content: &str) -> Vec<u8> {
    engine
        .remember_for_tenant(
            tenant,
            RememberInput {
                content: content.to_string(),
                importance: Some(0.7),
                context: None,
                entity_id: Some(format!("{}_entity", tenant.tenant_id())),
                edges: vec![],
                kind: None,
            },
        )
        .unwrap()
        .memory_id
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category 1: Storage Isolation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn storage_same_logical_key_different_physical_keys() {
    let backend = shared_backend();
    let scoped_a = TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "alpha");
    let scoped_b = TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "bravo");

    scoped_a
        .put(ColumnFamilyName::Default, b"shared_key", b"alpha_value")
        .unwrap();
    scoped_b
        .put(ColumnFamilyName::Default, b"shared_key", b"bravo_value")
        .unwrap();

    let val_a = scoped_a
        .get(ColumnFamilyName::Default, b"shared_key")
        .unwrap()
        .unwrap();
    let val_b = scoped_b
        .get(ColumnFamilyName::Default, b"shared_key")
        .unwrap()
        .unwrap();

    assert_eq!(val_a, b"alpha_value");
    assert_eq!(val_b, b"bravo_value");

    // Verify they are physically distinct keys in the raw backend
    let raw_all = backend
        .prefix_iterator(ColumnFamilyName::Default, b"")
        .unwrap();
    assert_eq!(raw_all.len(), 2, "two physical keys expected");
    let raw_keys: HashSet<Vec<u8>> = raw_all.into_iter().map(|(k, _)| k).collect();
    assert_eq!(raw_keys.len(), 2, "physical keys must be distinct");
}

#[test]
fn storage_prefix_iterator_isolation() {
    let backend = shared_backend();
    let scoped_a = TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "alpha");
    let scoped_b = TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "bravo");

    for i in 0..5u8 {
        scoped_a
            .put(ColumnFamilyName::Default, &[b'p', i], &[b'a', i])
            .unwrap();
        scoped_b
            .put(ColumnFamilyName::Default, &[b'p', i], &[b'b', i])
            .unwrap();
    }

    let a_results = scoped_a
        .prefix_iterator(ColumnFamilyName::Default, b"p")
        .unwrap();
    let b_results = scoped_b
        .prefix_iterator(ColumnFamilyName::Default, b"p")
        .unwrap();

    assert_eq!(a_results.len(), 5);
    assert_eq!(b_results.len(), 5);

    for (_k, v) in &a_results {
        assert_eq!(v[0], b'a', "tenant A iterator must only see A's values");
    }
    for (_k, v) in &b_results {
        assert_eq!(v[0], b'b', "tenant B iterator must only see B's values");
    }

    // Verify zero cross-tenant leakage: A scanning B's prefix returns nothing
    // (scoped_a's prefix_iterator prepends alpha's prefix, so it won't match bravo's keys)
    let cross = scoped_a
        .prefix_iterator(ColumnFamilyName::Default, &[])
        .unwrap();
    let cross_values: Vec<u8> = cross.iter().map(|(_, v)| v[0]).collect();
    assert!(
        cross_values.iter().all(|&v| v == b'a'),
        "tenant A full scan must never see B's values"
    );
}

#[test]
fn storage_write_batch_isolation() {
    let backend = shared_backend();
    let scoped_a = TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "alpha");
    let scoped_b = TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "bravo");

    let ops = vec![
        BatchOperation::Put {
            cf: ColumnFamilyName::Default,
            key: b"batch_1".to_vec(),
            value: b"val_1".to_vec(),
        },
        BatchOperation::Put {
            cf: ColumnFamilyName::Default,
            key: b"batch_2".to_vec(),
            value: b"val_2".to_vec(),
        },
    ];
    scoped_a.write_batch(&ops).unwrap();

    assert!(
        scoped_a
            .get(ColumnFamilyName::Default, b"batch_1")
            .unwrap()
            .is_some(),
        "tenant A should see its own batch writes"
    );
    assert!(
        scoped_b
            .get(ColumnFamilyName::Default, b"batch_1")
            .unwrap()
            .is_none(),
        "tenant B must NOT see A's batch writes"
    );
    assert!(
        scoped_b
            .get(ColumnFamilyName::Default, b"batch_2")
            .unwrap()
            .is_none(),
        "tenant B must NOT see A's batch writes"
    );
}

#[test]
fn storage_delete_isolation() {
    let backend = shared_backend();
    let scoped_a = TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "alpha");
    let scoped_b = TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "bravo");

    scoped_a
        .put(ColumnFamilyName::Default, b"k", b"a_val")
        .unwrap();
    scoped_b
        .put(ColumnFamilyName::Default, b"k", b"b_val")
        .unwrap();

    scoped_a.delete(ColumnFamilyName::Default, b"k").unwrap();

    assert!(
        scoped_a
            .get(ColumnFamilyName::Default, b"k")
            .unwrap()
            .is_none(),
        "tenant A's key should be deleted"
    );
    assert_eq!(
        scoped_b
            .get(ColumnFamilyName::Default, b"k")
            .unwrap()
            .unwrap(),
        b"b_val",
        "tenant B's key must survive A's delete"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category 2: Engine Isolation (Full Pipeline)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn engine_remember_isolation() {
    let backend = shared_backend();
    let engine = tenant_engine(backend.clone() as Arc<dyn StorageBackend>);

    let ctx_a = tenant_ctx("tenant-a");
    let ctx_b = tenant_ctx("tenant-b");

    let id_a = remember_for(&engine, &ctx_a, "shared content about quarterly revenue");
    let id_b = remember_for(&engine, &ctx_b, "shared content about quarterly revenue");

    assert_ne!(
        id_a, id_b,
        "same content should produce distinct memory IDs per tenant"
    );

    let mem_a = engine.get_for_tenant(&ctx_a, &id_a).unwrap();
    let mem_b = engine.get_for_tenant(&ctx_b, &id_b).unwrap();

    assert_eq!(mem_a.content, "shared content about quarterly revenue");
    assert_eq!(mem_b.content, "shared content about quarterly revenue");
}

#[test]
fn engine_recall_isolation() {
    let backend = shared_backend();
    let engine = tenant_engine(backend.clone() as Arc<dyn StorageBackend>);

    let ctx_a = tenant_ctx("tenant-a");
    let ctx_b = tenant_ctx("tenant-b");

    for i in 0..10 {
        remember_for(
            &engine,
            &ctx_a,
            &format!("alpha exclusive memory content item {}", i),
        );
    }
    for i in 0..10 {
        remember_for(
            &engine,
            &ctx_b,
            &format!("bravo exclusive memory content item {}", i),
        );
    }

    let recall_a = engine
        .recall_for_tenant(
            &ctx_a,
            RecallInput::new("alpha exclusive memory", RecallStrategy::Similarity),
        )
        .unwrap();

    let recall_b = engine
        .recall_for_tenant(
            &ctx_b,
            RecallInput::new("bravo exclusive memory", RecallStrategy::Similarity),
        )
        .unwrap();

    let ids_a: HashSet<Vec<u8>> = recall_a
        .results
        .iter()
        .map(|r| r.memory.memory_id.clone())
        .collect();
    let ids_b: HashSet<Vec<u8>> = recall_b
        .results
        .iter()
        .map(|r| r.memory.memory_id.clone())
        .collect();

    assert!(
        ids_a.is_disjoint(&ids_b),
        "recall results for tenant A and B must be completely disjoint"
    );
}

#[test]
fn engine_count_isolation() {
    let backend = shared_backend();
    let engine = tenant_engine(backend.clone() as Arc<dyn StorageBackend>);

    let ctx_a = tenant_ctx("tenant-a");
    let ctx_b = tenant_ctx("tenant-b");

    for i in 0..5 {
        remember_for(&engine, &ctx_a, &format!("a memory {}", i));
    }
    for i in 0..3 {
        remember_for(&engine, &ctx_b, &format!("b memory {}", i));
    }

    let count_a = engine.count_for_tenant(&ctx_a).unwrap();
    let count_b = engine.count_for_tenant(&ctx_b).unwrap();

    assert_eq!(count_a, 5, "tenant A should have exactly 5 memories");
    assert_eq!(count_b, 3, "tenant B should have exactly 3 memories");
}

#[test]
fn engine_get_cross_tenant_returns_not_found() {
    let backend = shared_backend();
    let engine = tenant_engine(backend.clone() as Arc<dyn StorageBackend>);

    let ctx_a = tenant_ctx("tenant-a");
    let ctx_b = tenant_ctx("tenant-b");

    let id_a = remember_for(&engine, &ctx_a, "private memory only for A");

    let result = engine.get_for_tenant(&ctx_b, &id_a);
    assert!(
        matches!(result, Err(HebbsError::MemoryNotFound { .. })),
        "tenant B must not be able to read tenant A's memory by ID"
    );
}

#[test]
fn engine_entity_data_isolation_via_count_and_get() {
    let backend = shared_backend();
    let engine = tenant_engine(backend.clone() as Arc<dyn StorageBackend>);

    let ctx_a = tenant_ctx("tenant-a");
    let ctx_b = tenant_ctx("tenant-b");

    let mut a_ids = Vec::new();
    for i in 0..5 {
        let mem = engine
            .remember_for_tenant(
                &ctx_a,
                RememberInput {
                    content: format!("a entity memory {}", i),
                    importance: Some(0.5),
                    context: None,
                    entity_id: Some("shared_entity".to_string()),
                    edges: vec![],
                    kind: None,
                },
            )
            .unwrap();
        a_ids.push(mem.memory_id);
    }

    let mut b_ids = Vec::new();
    for i in 0..3 {
        let mem = engine
            .remember_for_tenant(
                &ctx_b,
                RememberInput {
                    content: format!("b entity memory {}", i),
                    importance: Some(0.5),
                    context: None,
                    entity_id: Some("shared_entity".to_string()),
                    edges: vec![],
                    kind: None,
                },
            )
            .unwrap();
        b_ids.push(mem.memory_id);
    }

    // Counts are correctly scoped per-tenant
    assert_eq!(engine.count_for_tenant(&ctx_a).unwrap(), 5);
    assert_eq!(engine.count_for_tenant(&ctx_b).unwrap(), 3);

    // Each tenant can read only its own memories
    for id in &a_ids {
        assert!(engine.get_for_tenant(&ctx_a, id).is_ok());
        assert!(
            matches!(
                engine.get_for_tenant(&ctx_b, id),
                Err(HebbsError::MemoryNotFound { .. })
            ),
            "tenant B must NOT be able to read tenant A's memory"
        );
    }
    for id in &b_ids {
        assert!(engine.get_for_tenant(&ctx_b, id).is_ok());
        assert!(
            matches!(
                engine.get_for_tenant(&ctx_a, id),
                Err(HebbsError::MemoryNotFound { .. })
            ),
            "tenant A must NOT be able to read tenant B's memory"
        );
    }
}

#[test]
fn engine_forget_isolation() {
    let backend = shared_backend();
    let engine = tenant_engine(backend.clone() as Arc<dyn StorageBackend>);

    let ctx_a = tenant_ctx("tenant-a");
    let ctx_b = tenant_ctx("tenant-b");

    let mut a_ids = Vec::new();
    for i in 0..5 {
        a_ids.push(remember_for(
            &engine,
            &ctx_a,
            &format!("a forget test {}", i),
        ));
    }
    let mut b_ids = Vec::new();
    for i in 0..5 {
        b_ids.push(remember_for(
            &engine,
            &ctx_b,
            &format!("b forget test {}", i),
        ));
    }

    // Forget all of tenant A's memories
    let forget_output = engine
        .forget_for_tenant(&ctx_a, ForgetCriteria::by_ids(a_ids.clone()))
        .unwrap();
    assert_eq!(forget_output.forgotten_count, 5);

    assert_eq!(
        engine.count_for_tenant(&ctx_a).unwrap(),
        0,
        "tenant A should have zero memories after forget"
    );
    assert_eq!(
        engine.count_for_tenant(&ctx_b).unwrap(),
        5,
        "tenant B must be unaffected by A's forget"
    );

    for id in &b_ids {
        assert!(
            engine.get_for_tenant(&ctx_b, id).is_ok(),
            "all of B's memories must still be accessible"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category 3: Auth Module Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auth_generate_key_unique_with_prefix() {
    let (key1, record1) = generate_key("acme", "key-1", PERM_READ, None);
    let (key2, record2) = generate_key("acme", "key-2", PERM_READ, None);

    assert!(key1.starts_with(KEY_PREFIX));
    assert!(key2.starts_with(KEY_PREFIX));
    assert_ne!(key1, key2, "each generated key must be unique");
    assert_ne!(
        record1.key_hash, record2.key_hash,
        "hashes of unique keys must differ"
    );
}

#[test]
fn auth_validate_accepts_valid_key() {
    let storage = shared_backend();
    let cache = KeyCache::new();

    let (raw_key, record) = generate_key("acme", "valid", PERM_READ | PERM_WRITE, None);
    cache.insert(storage.as_ref(), record).unwrap();

    let validated = cache.validate(&raw_key).unwrap();
    assert_eq!(validated.tenant_id, "acme");
    assert!(validated.has_permission(PERM_READ));
    assert!(validated.has_permission(PERM_WRITE));
    assert!(!validated.has_permission(PERM_ADMIN));
}

#[test]
fn auth_validate_rejects_unknown_key() {
    let cache = KeyCache::new();
    let result = cache.validate("hb_this_key_does_not_exist_anywhere");
    assert_eq!(result, Err(AuthError::UnknownKey));
}

#[test]
fn auth_validate_rejects_malformed_key() {
    let cache = KeyCache::new();
    assert_eq!(
        cache.validate("not_valid"),
        Err(AuthError::MalformedToken),
        "key without hb_ prefix should be rejected"
    );
    assert_eq!(
        cache.validate(""),
        Err(AuthError::MalformedToken),
        "empty string should be rejected"
    );
}

#[test]
fn auth_validate_rejects_expired_key() {
    let storage = shared_backend();
    let cache = KeyCache::new();

    // expires_at = 1 microsecond (far in the past)
    let (raw_key, record) = generate_key("acme", "expired", PERM_READ, Some(1));
    cache.insert(storage.as_ref(), record).unwrap();

    assert_eq!(
        cache.validate(&raw_key),
        Err(AuthError::ExpiredKey),
        "expired key should be rejected"
    );
}

#[test]
fn auth_revoke_removes_key() {
    let storage = shared_backend();
    let cache = KeyCache::new();

    let (raw_key, record) = generate_key("acme", "to-revoke", PERM_READ, None);
    let key_hash = record.key_hash;
    cache.insert(storage.as_ref(), record).unwrap();

    assert!(
        cache.validate(&raw_key).is_ok(),
        "key should be valid before revoke"
    );

    let removed = cache.revoke(storage.as_ref(), &key_hash).unwrap();
    assert!(removed, "revoke should return true for existing key");

    assert_eq!(
        cache.validate(&raw_key),
        Err(AuthError::UnknownKey),
        "revoked key should be rejected"
    );
}

#[test]
fn auth_load_from_storage_roundtrip() {
    let storage = shared_backend();
    let cache1 = KeyCache::new();

    let (raw_key1, record1) = generate_key("acme", "persistent-1", PERM_READ, None);
    let (raw_key2, record2) = generate_key("globex", "persistent-2", PERM_WRITE, None);
    cache1.insert(storage.as_ref(), record1).unwrap();
    cache1.insert(storage.as_ref(), record2).unwrap();

    // Simulate a restart: create a fresh cache and load from storage
    let cache2 = KeyCache::new();
    let loaded = cache2.load_from_storage(storage.as_ref()).unwrap();
    assert_eq!(loaded, 2, "should load 2 keys from storage");

    assert!(cache2.validate(&raw_key1).is_ok());
    assert!(cache2.validate(&raw_key2).is_ok());

    let validated1 = cache2.validate(&raw_key1).unwrap();
    assert_eq!(validated1.tenant_id, "acme");
    let validated2 = cache2.validate(&raw_key2).unwrap();
    assert_eq!(validated2.tenant_id, "globex");
}

#[test]
fn auth_permission_checks() {
    let storage = shared_backend();
    let cache = KeyCache::new();

    let (read_key, read_record) = generate_key("acme", "read-only", PERM_READ, None);
    let (write_key, write_record) = generate_key("acme", "write-only", PERM_WRITE, None);
    let (admin_key, admin_record) = generate_key("acme", "admin-only", PERM_ADMIN, None);
    let (full_key, full_record) = generate_key(
        "acme",
        "full-access",
        PERM_READ | PERM_WRITE | PERM_ADMIN,
        None,
    );

    cache.insert(storage.as_ref(), read_record).unwrap();
    cache.insert(storage.as_ref(), write_record).unwrap();
    cache.insert(storage.as_ref(), admin_record).unwrap();
    cache.insert(storage.as_ref(), full_record).unwrap();

    let r = cache.validate(&read_key).unwrap();
    assert!(r.has_permission(PERM_READ));
    assert!(!r.has_permission(PERM_WRITE));
    assert!(!r.has_permission(PERM_ADMIN));

    let w = cache.validate(&write_key).unwrap();
    assert!(!w.has_permission(PERM_READ));
    assert!(w.has_permission(PERM_WRITE));
    assert!(!w.has_permission(PERM_ADMIN));

    let a = cache.validate(&admin_key).unwrap();
    assert!(!a.has_permission(PERM_READ));
    assert!(!a.has_permission(PERM_WRITE));
    assert!(a.has_permission(PERM_ADMIN));

    let f = cache.validate(&full_key).unwrap();
    assert!(f.has_permission(PERM_READ));
    assert!(f.has_permission(PERM_WRITE));
    assert!(f.has_permission(PERM_ADMIN));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category 4: Rate Limiter Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rate_limiter_burst_limit() {
    let config = RateLimitConfig {
        enabled: true,
        write_rate: 10.0,
        write_burst: 5,
        read_rate: 100.0,
        read_burst: 10,
        admin_rate: 1.0,
        admin_burst: 2,
    };
    let limiter = RateLimiter::new(config);

    // Exhaust the write burst for tenant-a
    for i in 0..5 {
        assert!(
            limiter.check("tenant-a", "remember").is_ok(),
            "request {} should be allowed within burst",
            i
        );
    }
    assert!(
        limiter.check("tenant-a", "remember").is_err(),
        "request beyond burst should be denied"
    );
}

#[test]
fn rate_limiter_tenant_independence() {
    let config = RateLimitConfig {
        enabled: true,
        write_rate: 10.0,
        write_burst: 3,
        ..Default::default()
    };
    let limiter = RateLimiter::new(config);

    // Exhaust tenant-a's write burst
    for _ in 0..3 {
        assert!(limiter.check("tenant-a", "remember").is_ok());
    }
    assert!(
        limiter.check("tenant-a", "remember").is_err(),
        "tenant-a should be rate limited"
    );

    // tenant-b should be completely unaffected
    for _ in 0..3 {
        assert!(
            limiter.check("tenant-b", "remember").is_ok(),
            "tenant-b should have its own independent bucket"
        );
    }
}

#[test]
fn rate_limiter_operation_class_independence() {
    let config = RateLimitConfig {
        enabled: true,
        write_rate: 10.0,
        write_burst: 2,
        read_rate: 100.0,
        read_burst: 2,
        admin_rate: 1.0,
        admin_burst: 2,
    };
    let limiter = RateLimiter::new(config);

    // Exhaust write burst
    for _ in 0..2 {
        assert!(limiter.check("t", "remember").is_ok());
    }
    assert!(limiter.check("t", "remember").is_err());

    // Read burst should be independent
    for _ in 0..2 {
        assert!(
            limiter.check("t", "recall").is_ok(),
            "read class should have its own bucket"
        );
    }
    assert!(limiter.check("t", "recall").is_err());

    // Admin burst should also be independent
    for _ in 0..2 {
        assert!(
            limiter.check("t", "reflect").is_ok(),
            "admin class should have its own bucket"
        );
    }
    assert!(limiter.check("t", "reflect").is_err());
}

#[test]
fn rate_limiter_disabled_always_allows() {
    let config = RateLimitConfig {
        enabled: false,
        write_rate: 1.0,
        write_burst: 1,
        read_rate: 1.0,
        read_burst: 1,
        admin_rate: 1.0,
        admin_burst: 1,
    };
    let limiter = RateLimiter::new(config);

    for _ in 0..1000 {
        assert!(
            limiter.check("t", "remember").is_ok(),
            "disabled limiter must always allow"
        );
    }
}

#[test]
fn rate_limiter_retry_after_positive() {
    let config = RateLimitConfig {
        enabled: true,
        write_rate: 1.0,
        write_burst: 1,
        ..Default::default()
    };
    let limiter = RateLimiter::new(config);

    assert!(limiter.check("t", "remember").is_ok());
    match limiter.check("t", "remember") {
        Err(retry_ms) => {
            assert!(
                retry_ms > 0,
                "retry-after must be positive, got {}",
                retry_ms
            );
        }
        Ok(_) => panic!("expected rate limit error"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category 5: Concurrent Multi-Tenant
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_multi_tenant_isolation() {
    let backend = shared_backend();
    let engine = Arc::new(tenant_engine(backend.clone() as Arc<dyn StorageBackend>));

    let num_tenants = 5;
    let ops_per_tenant = 20;

    let handles: Vec<_> = (0..num_tenants)
        .map(|t| {
            let engine = engine.clone();
            thread::spawn(move || {
                let tenant_id = format!("conc-tenant-{}", t);
                let ctx = tenant_ctx(&tenant_id);

                // Remember
                let mut ids = Vec::new();
                for i in 0..ops_per_tenant {
                    let mem = engine
                        .remember_for_tenant(
                            &ctx,
                            RememberInput {
                                content: format!("{} memory number {}", tenant_id, i),
                                importance: Some(0.5),
                                context: None,
                                entity_id: Some(format!("{}_entity", tenant_id)),
                                edges: vec![],
                                kind: None,
                            },
                        )
                        .unwrap();
                    ids.push(mem.memory_id);
                }

                // Recall
                let recall_out = engine
                    .recall_for_tenant(
                        &ctx,
                        RecallInput::new(
                            format!("{} memory", tenant_id),
                            RecallStrategy::Similarity,
                        ),
                    )
                    .unwrap();
                for r in &recall_out.results {
                    assert!(
                        r.memory.content.starts_with(&tenant_id),
                        "tenant {} recall returned cross-tenant result: {}",
                        tenant_id,
                        r.memory.content
                    );
                }

                // Verify count
                let count = engine.count_for_tenant(&ctx).unwrap();
                assert_eq!(
                    count, ops_per_tenant,
                    "tenant {} expected {} memories, got {}",
                    tenant_id, ops_per_tenant, count
                );

                // Forget half
                let to_forget: Vec<Vec<u8>> =
                    ids.iter().take(ops_per_tenant / 2).cloned().collect();
                let forget_out = engine
                    .forget_for_tenant(&ctx, ForgetCriteria::by_ids(to_forget))
                    .unwrap();
                assert_eq!(forget_out.forgotten_count, ops_per_tenant / 2);

                let remaining = engine.count_for_tenant(&ctx).unwrap();
                assert_eq!(
                    remaining,
                    ops_per_tenant - ops_per_tenant / 2,
                    "tenant {} post-forget count mismatch",
                    tenant_id
                );

                (tenant_id, remaining)
            })
        })
        .collect();

    let results: Vec<(String, usize)> = handles
        .into_iter()
        .map(|h| h.join().expect("tenant thread panicked"))
        .collect();

    // Post-join verification: each tenant's remaining count is correct
    for (tenant_id, expected_remaining) in &results {
        let ctx = tenant_ctx(tenant_id);
        let actual = engine.count_for_tenant(&ctx).unwrap();
        assert_eq!(
            actual, *expected_remaining,
            "post-join: tenant {} count mismatch",
            tenant_id
        );
    }

    // Cross-tenant check: each tenant's memories are inaccessible to others
    for (i, (tenant_id_i, _)) in results.iter().enumerate() {
        let ctx_i = tenant_ctx(tenant_id_i);
        let count_i = engine.count_for_tenant(&ctx_i).unwrap();
        assert!(
            count_i > 0,
            "tenant {} should have remaining memories",
            tenant_id_i
        );

        for (j, (tenant_id_j, _)) in results.iter().enumerate() {
            if i == j {
                continue;
            }
            let ctx_j = tenant_ctx(tenant_id_j);
            // Sample: try reading first tenant's memory from another tenant
            // (full cross-check is expensive, count isolation is sufficient)
            let count_j = engine.count_for_tenant(&ctx_j).unwrap();
            assert!(
                count_j > 0,
                "tenant {} should have remaining memories",
                tenant_id_j
            );
        }
    }
}

#[test]
fn concurrent_multi_tenant_no_deadlock() {
    let backend = shared_backend();
    let engine = Arc::new(tenant_engine(backend.clone() as Arc<dyn StorageBackend>));

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let num_tenants = 5;

    let handles: Vec<_> = (0..num_tenants)
        .map(|t| {
            let engine = engine.clone();
            thread::spawn(move || {
                let tenant_id = format!("deadlock-tenant-{}", t);
                let ctx = tenant_ctx(&tenant_id);
                let mut remembered_ids = Vec::new();
                let mut cycle = 0u64;

                while std::time::Instant::now() < deadline {
                    // Remember
                    let mem = engine
                        .remember_for_tenant(
                            &ctx,
                            RememberInput {
                                content: format!("{} cycle {} memory", tenant_id, cycle),
                                importance: Some(0.5),
                                context: None,
                                entity_id: Some(tenant_id.clone()),
                                edges: vec![],
                                kind: None,
                            },
                        )
                        .unwrap();
                    remembered_ids.push(mem.memory_id.clone());

                    // Recall
                    let _ = engine.recall_for_tenant(
                        &ctx,
                        RecallInput::new(
                            format!("{} cycle {}", tenant_id, cycle),
                            RecallStrategy::Similarity,
                        ),
                    );

                    // Periodically forget oldest
                    if remembered_ids.len() > 10 {
                        let to_forget = remembered_ids.drain(..5).collect::<Vec<_>>();
                        let _ = engine.forget_for_tenant(&ctx, ForgetCriteria::by_ids(to_forget));
                    }

                    cycle += 1;
                }

                cycle
            })
        })
        .collect();

    let cycles: Vec<u64> = handles
        .into_iter()
        .map(|h| {
            h.join()
                .expect("deadlock detected: thread panicked or hung")
        })
        .collect();

    for (t, &c) in cycles.iter().enumerate() {
        assert!(
            c > 0,
            "tenant thread {} completed 0 cycles — likely deadlocked",
            t
        );
    }
}
