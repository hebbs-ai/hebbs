use std::sync::Arc;

use crate::error::{ColumnFamilyName, Result};
use crate::traits::{BatchOperation, StorageBackend};

/// Separator byte between tenant prefix and logical key.
/// 0xFF cannot appear in valid UTF-8 (max byte is 0xF4), guaranteeing
/// no tenant_id prefix can collide with another tenant's keys.
const TENANT_SEPARATOR: u8 = 0xFF;

/// A storage backend wrapper that transparently scopes all operations
/// to a specific tenant by prepending a key prefix.
///
/// This provides structural tenant isolation at the storage layer:
/// a bug in query logic cannot leak data across tenants because the
/// prefix is enforced on every read and write operation.
///
/// Keys seen by callers are "logical" (unscoped). Keys stored in the
/// underlying backend are "physical" (tenant-prefixed).
pub struct TenantScopedStorage {
    inner: Arc<dyn StorageBackend>,
    prefix: Vec<u8>,
    prefix_len: usize,
}

impl TenantScopedStorage {
    pub fn new(inner: Arc<dyn StorageBackend>, tenant_id: &str) -> Self {
        let mut prefix = Vec::with_capacity(tenant_id.len() + 1);
        prefix.extend_from_slice(tenant_id.as_bytes());
        prefix.push(TENANT_SEPARATOR);
        let prefix_len = prefix.len();
        Self {
            inner,
            prefix,
            prefix_len,
        }
    }

    fn scope_key(&self, key: &[u8]) -> Vec<u8> {
        let mut scoped = Vec::with_capacity(self.prefix_len + key.len());
        scoped.extend_from_slice(&self.prefix);
        scoped.extend_from_slice(key);
        scoped
    }

    fn unscope_key(&self, physical_key: &[u8]) -> Vec<u8> {
        if physical_key.len() >= self.prefix_len && physical_key.starts_with(&self.prefix) {
            physical_key[self.prefix_len..].to_vec()
        } else {
            physical_key.to_vec()
        }
    }

    /// Returns a reference to the underlying (unscoped) storage backend.
    pub fn inner(&self) -> &dyn StorageBackend {
        self.inner.as_ref()
    }
}

impl std::fmt::Debug for TenantScopedStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenantScopedStorage")
            .field("prefix_len", &self.prefix_len)
            .finish()
    }
}

impl StorageBackend for TenantScopedStorage {
    fn put(&self, cf: ColumnFamilyName, key: &[u8], value: &[u8]) -> Result<()> {
        let scoped = self.scope_key(key);
        self.inner.put(cf, &scoped, value)
    }

    fn get(&self, cf: ColumnFamilyName, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let scoped = self.scope_key(key);
        self.inner.get(cf, &scoped)
    }

    fn delete(&self, cf: ColumnFamilyName, key: &[u8]) -> Result<()> {
        let scoped = self.scope_key(key);
        self.inner.delete(cf, &scoped)
    }

    fn write_batch(&self, operations: &[BatchOperation]) -> Result<()> {
        let scoped_ops: Vec<BatchOperation> = operations
            .iter()
            .map(|op| match op {
                BatchOperation::Put { cf, key, value } => BatchOperation::Put {
                    cf: *cf,
                    key: self.scope_key(key),
                    value: value.clone(),
                },
                BatchOperation::Delete { cf, key } => BatchOperation::Delete {
                    cf: *cf,
                    key: self.scope_key(key),
                },
            })
            .collect();
        self.inner.write_batch(&scoped_ops)
    }

    fn prefix_iterator(
        &self,
        cf: ColumnFamilyName,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let scoped_prefix = self.scope_key(prefix);
        let results = self.inner.prefix_iterator(cf, &scoped_prefix)?;
        Ok(results
            .into_iter()
            .map(|(k, v)| (self.unscope_key(&k), v))
            .collect())
    }

    fn range_iterator(
        &self,
        cf: ColumnFamilyName,
        start: &[u8],
        end: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let scoped_start = self.scope_key(start);
        let scoped_end = self.scope_key(end);
        let results = self.inner.range_iterator(cf, &scoped_start, &scoped_end)?;
        Ok(results
            .into_iter()
            .map(|(k, v)| (self.unscope_key(&k), v))
            .collect())
    }

    fn compact(&self, cf: ColumnFamilyName) -> Result<()> {
        self.inner.compact(cf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryBackend;

    fn setup() -> (Arc<InMemoryBackend>, TenantScopedStorage) {
        let backend = Arc::new(InMemoryBackend::new());
        let scoped =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_a");
        (backend, scoped)
    }

    #[test]
    fn put_get_round_trip() {
        let (_backend, scoped) = setup();
        scoped
            .put(ColumnFamilyName::Default, b"key1", b"value1")
            .unwrap();
        let val = scoped
            .get(ColumnFamilyName::Default, b"key1")
            .unwrap()
            .unwrap();
        assert_eq!(val, b"value1");
    }

    #[test]
    fn tenant_isolation() {
        let backend = Arc::new(InMemoryBackend::new());
        let tenant_a =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_a");
        let tenant_b =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_b");

        tenant_a
            .put(ColumnFamilyName::Default, b"shared_key", b"value_a")
            .unwrap();
        tenant_b
            .put(ColumnFamilyName::Default, b"shared_key", b"value_b")
            .unwrap();

        let val_a = tenant_a
            .get(ColumnFamilyName::Default, b"shared_key")
            .unwrap()
            .unwrap();
        let val_b = tenant_b
            .get(ColumnFamilyName::Default, b"shared_key")
            .unwrap()
            .unwrap();

        assert_eq!(val_a, b"value_a");
        assert_eq!(val_b, b"value_b");
    }

    #[test]
    fn delete_isolation() {
        let backend = Arc::new(InMemoryBackend::new());
        let tenant_a =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_a");
        let tenant_b =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_b");

        tenant_a
            .put(ColumnFamilyName::Default, b"key", b"a")
            .unwrap();
        tenant_b
            .put(ColumnFamilyName::Default, b"key", b"b")
            .unwrap();

        tenant_a.delete(ColumnFamilyName::Default, b"key").unwrap();

        assert!(tenant_a
            .get(ColumnFamilyName::Default, b"key")
            .unwrap()
            .is_none());
        assert_eq!(
            tenant_b
                .get(ColumnFamilyName::Default, b"key")
                .unwrap()
                .unwrap(),
            b"b"
        );
    }

    #[test]
    fn prefix_iterator_isolation() {
        let backend = Arc::new(InMemoryBackend::new());
        let tenant_a =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_a");
        let tenant_b =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_b");

        for i in 0..5u8 {
            tenant_a
                .put(ColumnFamilyName::Default, &[b'p', i], &[b'a', i])
                .unwrap();
            tenant_b
                .put(ColumnFamilyName::Default, &[b'p', i], &[b'b', i])
                .unwrap();
        }

        let a_results = tenant_a
            .prefix_iterator(ColumnFamilyName::Default, &[b'p'])
            .unwrap();
        let b_results = tenant_b
            .prefix_iterator(ColumnFamilyName::Default, &[b'p'])
            .unwrap();

        assert_eq!(a_results.len(), 5);
        assert_eq!(b_results.len(), 5);

        for (k, v) in &a_results {
            assert_eq!(k[0], b'p');
            assert_eq!(v[0], b'a');
        }
        for (k, v) in &b_results {
            assert_eq!(k[0], b'p');
            assert_eq!(v[0], b'b');
        }
    }

    #[test]
    fn write_batch_isolation() {
        let backend = Arc::new(InMemoryBackend::new());
        let tenant_a =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_a");
        let tenant_b =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_b");

        let ops = vec![
            BatchOperation::Put {
                cf: ColumnFamilyName::Default,
                key: b"k1".to_vec(),
                value: b"v1".to_vec(),
            },
            BatchOperation::Put {
                cf: ColumnFamilyName::Default,
                key: b"k2".to_vec(),
                value: b"v2".to_vec(),
            },
        ];

        tenant_a.write_batch(&ops).unwrap();

        assert!(tenant_a
            .get(ColumnFamilyName::Default, b"k1")
            .unwrap()
            .is_some());
        assert!(tenant_b
            .get(ColumnFamilyName::Default, b"k1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn range_iterator_isolation() {
        let backend = Arc::new(InMemoryBackend::new());
        let tenant_a =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_a");
        let tenant_b =
            TenantScopedStorage::new(backend.clone() as Arc<dyn StorageBackend>, "tenant_b");

        for i in 0..10u8 {
            tenant_a
                .put(ColumnFamilyName::Default, &[i], &[b'a', i])
                .unwrap();
            tenant_b
                .put(ColumnFamilyName::Default, &[i], &[b'b', i])
                .unwrap();
        }

        let a_results = tenant_a
            .range_iterator(ColumnFamilyName::Default, &[3], &[7])
            .unwrap();
        assert_eq!(a_results.len(), 4); // keys 3,4,5,6
        for (_k, v) in &a_results {
            assert_eq!(v[0], b'a');
        }
    }
}
