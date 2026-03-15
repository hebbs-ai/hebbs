use std::collections::BTreeMap;

use parking_lot::RwLock;

use crate::error::{ColumnFamilyName, Result};
use crate::traits::{BatchOperation, StorageBackend};

/// In-memory storage backend for testing.
///
/// Uses a `BTreeMap` per column family so that iteration order matches
/// RocksDB's byte-sorted order. This is critical: tests that pass against
/// the in-memory backend must also pass against RocksDB. Any behavioral
/// divergence is a bug in this mock.
///
/// Thread-safety: each column family has its own `RwLock<BTreeMap>`.
/// Reads never block reads; writes block only the affected CF.
pub struct InMemoryBackend {
    default: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
    temporal: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
    vectors: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
    graph: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
    meta: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
    vectors_associative: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
    query_log: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            default: RwLock::new(BTreeMap::new()),
            temporal: RwLock::new(BTreeMap::new()),
            vectors: RwLock::new(BTreeMap::new()),
            graph: RwLock::new(BTreeMap::new()),
            meta: RwLock::new(BTreeMap::new()),
            vectors_associative: RwLock::new(BTreeMap::new()),
            query_log: RwLock::new(BTreeMap::new()),
        }
    }

    fn get_cf(&self, cf: ColumnFamilyName) -> &RwLock<BTreeMap<Vec<u8>, Vec<u8>>> {
        match cf {
            ColumnFamilyName::Default => &self.default,
            ColumnFamilyName::Temporal => &self.temporal,
            ColumnFamilyName::Vectors => &self.vectors,
            ColumnFamilyName::Graph => &self.graph,
            ColumnFamilyName::Meta => &self.meta,
            ColumnFamilyName::VectorsAssociative => &self.vectors_associative,
            ColumnFamilyName::QueryLog => &self.query_log,
        }
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for InMemoryBackend {
    fn put(&self, cf: ColumnFamilyName, key: &[u8], value: &[u8]) -> Result<()> {
        let mut map = self.get_cf(cf).write();
        map.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn get(&self, cf: ColumnFamilyName, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let map = self.get_cf(cf).read();
        Ok(map.get(key).cloned())
    }

    fn delete(&self, cf: ColumnFamilyName, key: &[u8]) -> Result<()> {
        let mut map = self.get_cf(cf).write();
        map.remove(key);
        Ok(())
    }

    /// Atomic batch write: collect all mutations, then apply.
    /// If any operation would fail, none are applied.
    fn write_batch(&self, operations: &[BatchOperation]) -> Result<()> {
        // Phase 1: validate and collect mutations grouped by CF.
        // Phase 2: acquire write locks (in deterministic CF order to avoid deadlock)
        // and apply all mutations.

        // Group ops by CF to minimise lock acquisitions.
        struct CfMutations {
            puts: Vec<(Vec<u8>, Vec<u8>)>,
            deletes: Vec<Vec<u8>>,
        }
        let mut per_cf: [Option<CfMutations>; 6] = Default::default();

        for op in operations {
            let (_cf, idx) = match op {
                BatchOperation::Put { cf, .. } => (*cf, *cf as usize),
                BatchOperation::Delete { cf, .. } => (*cf, *cf as usize),
            };
            let mutations = per_cf[idx].get_or_insert_with(|| CfMutations {
                puts: Vec::new(),
                deletes: Vec::new(),
            });
            match op {
                BatchOperation::Put { key, value, .. } => {
                    mutations.puts.push((key.clone(), value.clone()));
                }
                BatchOperation::Delete { key, .. } => {
                    mutations.deletes.push(key.clone());
                }
            }
        }

        // Apply all mutations under write locks.
        for (idx, mutations) in per_cf.iter().enumerate() {
            if let Some(m) = mutations {
                let cf = match idx {
                    0 => ColumnFamilyName::Default,
                    1 => ColumnFamilyName::Temporal,
                    2 => ColumnFamilyName::Vectors,
                    3 => ColumnFamilyName::Graph,
                    4 => ColumnFamilyName::Meta,
                    5 => ColumnFamilyName::VectorsAssociative,
                    _ => unreachable!(),
                };
                let mut map = self.get_cf(cf).write();
                for (k, v) in &m.puts {
                    map.insert(k.clone(), v.clone());
                }
                for k in &m.deletes {
                    map.remove(k);
                }
            }
        }

        Ok(())
    }

    fn prefix_iterator(
        &self,
        cf: ColumnFamilyName,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let map = self.get_cf(cf).read();
        let results: Vec<(Vec<u8>, Vec<u8>)> = map
            .range::<Vec<u8>, _>(prefix.to_vec()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(results)
    }

    fn range_iterator(
        &self,
        cf: ColumnFamilyName,
        start: &[u8],
        end: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let map = self.get_cf(cf).read();
        let results: Vec<(Vec<u8>, Vec<u8>)> = map
            .range::<Vec<u8>, _>(start.to_vec()..end.to_vec())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(results)
    }

    fn compact(&self, _cf: ColumnFamilyName) -> Result<()> {
        // No-op for in-memory backend.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_roundtrip() {
        let backend = InMemoryBackend::new();
        backend
            .put(ColumnFamilyName::Default, b"key1", b"value1")
            .unwrap();
        let val = backend.get(ColumnFamilyName::Default, b"key1").unwrap();
        assert_eq!(val, Some(b"value1".to_vec()));
    }

    #[test]
    fn get_missing_returns_none() {
        let backend = InMemoryBackend::new();
        let val = backend
            .get(ColumnFamilyName::Default, b"nonexistent")
            .unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn delete_removes_key() {
        let backend = InMemoryBackend::new();
        backend
            .put(ColumnFamilyName::Default, b"key1", b"value1")
            .unwrap();
        backend.delete(ColumnFamilyName::Default, b"key1").unwrap();
        let val = backend.get(ColumnFamilyName::Default, b"key1").unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn delete_nonexistent_succeeds() {
        let backend = InMemoryBackend::new();
        backend.delete(ColumnFamilyName::Default, b"nope").unwrap();
    }

    #[test]
    fn write_batch_atomic() {
        let backend = InMemoryBackend::new();
        let ops = vec![
            BatchOperation::Put {
                cf: ColumnFamilyName::Default,
                key: b"a".to_vec(),
                value: b"1".to_vec(),
            },
            BatchOperation::Put {
                cf: ColumnFamilyName::Default,
                key: b"b".to_vec(),
                value: b"2".to_vec(),
            },
            BatchOperation::Put {
                cf: ColumnFamilyName::Meta,
                key: b"version".to_vec(),
                value: b"1".to_vec(),
            },
        ];
        backend.write_batch(&ops).unwrap();
        assert_eq!(
            backend.get(ColumnFamilyName::Default, b"a").unwrap(),
            Some(b"1".to_vec())
        );
        assert_eq!(
            backend.get(ColumnFamilyName::Default, b"b").unwrap(),
            Some(b"2".to_vec())
        );
        assert_eq!(
            backend.get(ColumnFamilyName::Meta, b"version").unwrap(),
            Some(b"1".to_vec())
        );
    }

    #[test]
    fn prefix_iterator_sorted_and_bounded() {
        let backend = InMemoryBackend::new();
        backend
            .put(ColumnFamilyName::Default, b"abc_1", b"v1")
            .unwrap();
        backend
            .put(ColumnFamilyName::Default, b"abc_2", b"v2")
            .unwrap();
        backend
            .put(ColumnFamilyName::Default, b"abd_1", b"v3")
            .unwrap();
        backend
            .put(ColumnFamilyName::Default, b"xyz_1", b"v4")
            .unwrap();

        let results = backend
            .prefix_iterator(ColumnFamilyName::Default, b"abc")
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, b"abc_1");
        assert_eq!(results[1].0, b"abc_2");
    }

    #[test]
    fn range_iterator_half_open() {
        let backend = InMemoryBackend::new();
        for i in 0u8..10 {
            backend
                .put(ColumnFamilyName::Default, &[i], &[i * 10])
                .unwrap();
        }
        let results = backend
            .range_iterator(ColumnFamilyName::Default, &[3], &[7])
            .unwrap();
        assert_eq!(results.len(), 4); // 3, 4, 5, 6
        assert_eq!(results[0].0, vec![3]);
        assert_eq!(results[3].0, vec![6]);
    }

    #[test]
    fn column_families_are_isolated() {
        let backend = InMemoryBackend::new();
        backend
            .put(ColumnFamilyName::Default, b"key", b"default_val")
            .unwrap();
        backend
            .put(ColumnFamilyName::Meta, b"key", b"meta_val")
            .unwrap();

        assert_eq!(
            backend.get(ColumnFamilyName::Default, b"key").unwrap(),
            Some(b"default_val".to_vec())
        );
        assert_eq!(
            backend.get(ColumnFamilyName::Meta, b"key").unwrap(),
            Some(b"meta_val".to_vec())
        );
        assert_eq!(
            backend.get(ColumnFamilyName::Temporal, b"key").unwrap(),
            None
        );
    }
}
