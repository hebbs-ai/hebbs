//! Query audit log: records every recall/prime operation for trust and debugging.
//!
//! Entries are stored in the `QueryLog` column family of the vault's RocksDB.
//! Key format: `{timestamp_us:016x}:{id:016x}` for time-ordered iteration.
//!
//! Complexity:
//! - `append`: O(1) single RocksDB put
//! - `list`: O(log n) seek + O(k) scan where k = entries in range
//! - `compact_old`: O(log n) seek + O(k) scan + O(k) deletes

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use hebbs_storage::{ColumnFamilyName, StorageBackend};

/// Monotonically increasing ID counter for query log entries.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A single query audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryLogEntry {
    /// Unique ID for this query event.
    pub id: u64,
    /// Microsecond timestamp (same clock as memory timestamps).
    pub timestamp_us: u64,
    /// Operation type.
    pub operation: QueryOperation,
    /// Caller identity.
    pub caller: String,
    /// The query text submitted (empty for prime without similarity_cue).
    pub query: String,
    /// Recall strategy used.
    pub strategy: Option<String>,
    /// Number of results requested (top_k).
    pub top_k: u32,
    /// Number of results returned.
    pub result_count: u32,
    /// IDs of memories returned (ordered by rank).
    pub result_memory_ids: Vec<String>,
    /// Top result's composite score.
    pub top_score: f32,
    /// Query latency in microseconds.
    pub latency_us: u64,
    /// Entity ID scope (if the query was scoped to an entity).
    pub entity_id: Option<String>,
    /// Vault path that was queried.
    pub vault_path: Option<String>,
}

/// The type of query operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryOperation {
    Recall,
    Prime,
}

/// Write a query log entry directly to a storage backend.
///
/// Used by the daemon dispatch where we have `&dyn StorageBackend` from the engine.
/// O(1) single RocksDB put.
pub fn append_to_storage(storage: &dyn StorageBackend, entry: &QueryLogEntry) -> Result<(), String> {
    let key = format!("{:016x}:{:016x}", entry.timestamp_us, entry.id);
    let value = serde_json::to_vec(entry)
        .map_err(|e| format!("failed to serialize query log entry: {}", e))?;
    storage
        .put(ColumnFamilyName::QueryLog, key.as_bytes(), &value)
        .map_err(|e| format!("failed to write query log entry: {}", e))?;
    Ok(())
}

/// Query log store backed by a RocksDB column family.
///
/// Thread-safe via the underlying StorageBackend (RocksDB serializes writes internally).
pub struct QueryLogStore {
    storage: Arc<dyn StorageBackend>,
}

impl QueryLogStore {
    /// Create a new query log store wrapping the given storage backend.
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
    }

    /// Append a query log entry. Returns the assigned entry ID.
    ///
    /// O(1) single RocksDB put.
    pub fn append(&self, entry: &QueryLogEntry) -> Result<u64, String> {
        let key = format!("{:016x}:{:016x}", entry.timestamp_us, entry.id);
        let value = serde_json::to_vec(entry)
            .map_err(|e| format!("failed to serialize query log entry: {}", e))?;

        self.storage
            .put(ColumnFamilyName::QueryLog, key.as_bytes(), &value)
            .map_err(|e| format!("failed to write query log entry: {}", e))?;

        Ok(entry.id)
    }

    /// List query log entries, newest first.
    ///
    /// O(log n) seek + O(k) scan where k = entries returned.
    pub fn list(&self, params: &QueryLogListParams) -> Result<Vec<QueryLogEntry>, String> {
        // Use prefix iterator with empty prefix to get all entries,
        // or range iterator for time-bounded queries.
        let entries = if params.since_us.is_some() || params.until_us.is_some() {
            let start = format!("{:016x}:", params.since_us.unwrap_or(0));
            let end = format!("{:016x}:", params.until_us.unwrap_or(u64::MAX));
            self.storage
                .range_iterator(ColumnFamilyName::QueryLog, start.as_bytes(), end.as_bytes())
                .map_err(|e| format!("failed to read query log: {}", e))?
        } else {
            self.storage
                .prefix_iterator(ColumnFamilyName::QueryLog, b"")
                .map_err(|e| format!("failed to read query log: {}", e))?
        };

        let mut results: Vec<QueryLogEntry> = entries
            .iter()
            .filter_map(|(_, value)| serde_json::from_slice(value).ok())
            .collect();

        // Apply filters
        if let Some(ref caller) = params.caller {
            results.retain(|e| &e.caller == caller);
        }
        if let Some(ref operation) = params.operation {
            results.retain(|e| &e.operation == operation);
        }
        if let Some(ref query_contains) = params.query_contains {
            let q = query_contains.to_lowercase();
            results.retain(|e| e.query.to_lowercase().contains(&q));
        }
        if let Some(min_latency) = params.min_latency_us {
            results.retain(|e| e.latency_us >= min_latency);
        }

        // Reverse for newest-first
        results.reverse();

        // Apply offset and limit
        let offset = params.offset.unwrap_or(0) as usize;
        let limit = params.limit.unwrap_or(50).min(500) as usize;
        let results: Vec<QueryLogEntry> = results.into_iter().skip(offset).take(limit).collect();

        Ok(results)
    }

    /// Get a single query log entry by ID.
    pub fn get(&self, id: u64) -> Result<Option<QueryLogEntry>, String> {
        // Scan all entries and find by ID (acceptable for bounded log).
        let entries = self
            .storage
            .prefix_iterator(ColumnFamilyName::QueryLog, b"")
            .map_err(|e| format!("failed to read query log: {}", e))?;

        for (_, value) in &entries {
            if let Ok(entry) = serde_json::from_slice::<QueryLogEntry>(value) {
                if entry.id == id {
                    return Ok(Some(entry));
                }
            }
        }

        Ok(None)
    }

    /// Delete entries older than `max_age_us` or exceeding `max_entries`.
    ///
    /// O(log n) seek + O(k) scan + O(d) deletes where d = entries removed.
    pub fn compact_old(&self, max_entries: u64, max_age_us: u64) -> Result<usize, String> {
        let entries = self
            .storage
            .prefix_iterator(ColumnFamilyName::QueryLog, b"")
            .map_err(|e| format!("failed to read query log for compaction: {}", e))?;

        let now_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        let cutoff_us = now_us.saturating_sub(max_age_us);
        let total = entries.len();
        let mut to_delete = Vec::new();

        // Entries are in key order (oldest first).
        for (i, (key, value)) in entries.iter().enumerate() {
            let should_delete = if let Ok(entry) = serde_json::from_slice::<QueryLogEntry>(value) {
                // Delete if too old
                entry.timestamp_us < cutoff_us
                    // Or if we have too many entries (keep newest max_entries)
                    || (total as u64 > max_entries && i < (total - max_entries as usize))
            } else {
                true // Delete corrupt entries
            };

            if should_delete {
                to_delete.push(key.clone());
            }
        }

        let deleted = to_delete.len();
        for key in &to_delete {
            self.storage
                .delete(ColumnFamilyName::QueryLog, key)
                .map_err(|e| format!("failed to delete query log entry: {}", e))?;
        }

        Ok(deleted)
    }

    /// Get aggregate statistics for the query log.
    pub fn stats(&self, since_us: Option<u64>) -> Result<QueryLogStats, String> {
        let entries = if let Some(since) = since_us {
            let start = format!("{:016x}:", since);
            let end = format!("{:016x}:", u64::MAX);
            self.storage
                .range_iterator(ColumnFamilyName::QueryLog, start.as_bytes(), end.as_bytes())
                .map_err(|e| format!("failed to read query log: {}", e))?
        } else {
            self.storage
                .prefix_iterator(ColumnFamilyName::QueryLog, b"")
                .map_err(|e| format!("failed to read query log: {}", e))?
        };

        let mut total_queries: u64 = 0;
        let mut total_latency: u64 = 0;
        let mut max_latency: u64 = 0;
        let mut by_caller: std::collections::HashMap<String, (u64, u64)> =
            std::collections::HashMap::new();
        let mut latencies: Vec<u64> = Vec::new();

        for (_, value) in &entries {
            if let Ok(entry) = serde_json::from_slice::<QueryLogEntry>(value) {
                total_queries += 1;
                total_latency += entry.latency_us;
                if entry.latency_us > max_latency {
                    max_latency = entry.latency_us;
                }
                latencies.push(entry.latency_us);

                let counter = by_caller.entry(entry.caller.clone()).or_insert((0, 0));
                counter.0 += 1;
                counter.1 += entry.latency_us;
            }
        }

        latencies.sort_unstable();
        let p99_latency_us = if latencies.is_empty() {
            0
        } else {
            let idx = ((latencies.len() as f64) * 0.99).ceil() as usize;
            latencies[idx.min(latencies.len() - 1)]
        };

        let avg_latency_us = if total_queries > 0 {
            total_latency / total_queries
        } else {
            0
        };

        let callers: Vec<CallerStats> = by_caller
            .into_iter()
            .map(|(name, (count, lat))| CallerStats {
                caller: name,
                count,
                avg_latency_us: if count > 0 { lat / count } else { 0 },
            })
            .collect();

        Ok(QueryLogStats {
            total_queries,
            avg_latency_us,
            p99_latency_us,
            max_latency_us: max_latency,
            by_caller: callers,
        })
    }
}

/// Parameters for listing query log entries.
#[derive(Debug, Default, Deserialize)]
pub struct QueryLogListParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub caller: Option<String>,
    pub operation: Option<QueryOperation>,
    pub since_us: Option<u64>,
    pub until_us: Option<u64>,
    pub query_contains: Option<String>,
    pub min_latency_us: Option<u64>,
}

/// Aggregate statistics for the query log.
#[derive(Debug, Serialize)]
pub struct QueryLogStats {
    pub total_queries: u64,
    pub avg_latency_us: u64,
    pub p99_latency_us: u64,
    pub max_latency_us: u64,
    pub by_caller: Vec<CallerStats>,
}

/// Per-caller statistics.
#[derive(Debug, Serialize)]
pub struct CallerStats {
    pub caller: String,
    pub count: u64,
    pub avg_latency_us: u64,
}

/// Generate a new unique query log entry ID.
pub fn next_entry_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Get the current timestamp in microseconds.
pub fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Build a QueryLogEntry from recall results. Helper for daemon/panel.
pub fn build_recall_entry(
    caller: &str,
    cue: &str,
    strategy: Option<&str>,
    top_k: u32,
    entity_id: Option<&str>,
    result_count: u32,
    result_memory_ids: Vec<String>,
    top_score: f32,
    latency_us: u64,
    vault_path: Option<&str>,
) -> QueryLogEntry {
    QueryLogEntry {
        id: next_entry_id(),
        timestamp_us: now_us(),
        operation: QueryOperation::Recall,
        caller: caller.to_string(),
        query: cue.to_string(),
        strategy: strategy.map(|s| s.to_string()),
        top_k,
        result_count,
        result_memory_ids,
        top_score,
        latency_us,
        entity_id: entity_id.map(|s| s.to_string()),
        vault_path: vault_path.map(|s| s.to_string()),
    }
}

/// Build a QueryLogEntry from prime results. Helper for daemon/panel.
pub fn build_prime_entry(
    caller: &str,
    entity_id: &str,
    similarity_cue: Option<&str>,
    max_memories: u32,
    result_count: u32,
    result_memory_ids: Vec<String>,
    top_score: f32,
    latency_us: u64,
    vault_path: Option<&str>,
) -> QueryLogEntry {
    QueryLogEntry {
        id: next_entry_id(),
        timestamp_us: now_us(),
        operation: QueryOperation::Prime,
        caller: caller.to_string(),
        query: similarity_cue.unwrap_or("").to_string(),
        strategy: None,
        top_k: max_memories,
        result_count,
        result_memory_ids,
        top_score,
        latency_us,
        entity_id: Some(entity_id.to_string()),
        vault_path: vault_path.map(|s| s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_store() -> QueryLogStore {
        let storage = Arc::new(hebbs_storage::InMemoryBackend::new());
        QueryLogStore::new(storage)
    }

    #[test]
    fn test_append_and_list() {
        let store = make_test_store();
        let entry = build_recall_entry(
            "cli", "test query", Some("similarity"), 10, None, 3,
            vec!["a".into(), "b".into(), "c".into()], 0.95, 3200, None,
        );
        store.append(&entry).unwrap();

        let results = store.list(&QueryLogListParams::default()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].query, "test query");
        assert_eq!(results[0].caller, "cli");
        assert_eq!(results[0].result_count, 3);
    }

    #[test]
    fn test_list_newest_first() {
        let store = make_test_store();

        let mut e1 = build_recall_entry(
            "cli", "first", Some("similarity"), 10, None, 1, vec![], 0.5, 100, None,
        );
        e1.timestamp_us = 1000;
        store.append(&e1).unwrap();

        let mut e2 = build_recall_entry(
            "cli", "second", Some("similarity"), 10, None, 1, vec![], 0.5, 100, None,
        );
        e2.timestamp_us = 2000;
        store.append(&e2).unwrap();

        let results = store.list(&QueryLogListParams::default()).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].query, "second"); // newest first
        assert_eq!(results[1].query, "first");
    }

    #[test]
    fn test_filter_by_caller() {
        let store = make_test_store();
        store.append(&build_recall_entry(
            "cli", "q1", Some("similarity"), 10, None, 1, vec![], 0.5, 100, None,
        )).unwrap();
        store.append(&build_recall_entry(
            "hebbs-panel", "q2", Some("similarity"), 10, None, 1, vec![], 0.5, 100, None,
        )).unwrap();

        let params = QueryLogListParams {
            caller: Some("cli".to_string()),
            ..Default::default()
        };
        let results = store.list(&params).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].caller, "cli");
    }

    #[test]
    fn test_filter_by_operation() {
        let store = make_test_store();
        store.append(&build_recall_entry(
            "cli", "recall", Some("similarity"), 10, None, 1, vec![], 0.5, 100, None,
        )).unwrap();
        store.append(&build_prime_entry(
            "cli", "user_prefs", None, 20, 5, vec![], 0.5, 200, None,
        )).unwrap();

        let params = QueryLogListParams {
            operation: Some(QueryOperation::Prime),
            ..Default::default()
        };
        let results = store.list(&params).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].operation, QueryOperation::Prime);
    }

    #[test]
    fn test_compact_old_by_count() {
        let store = make_test_store();
        for i in 0..10 {
            let mut entry = build_recall_entry(
                "cli", &format!("q{}", i), Some("similarity"), 10, None, 1, vec![], 0.5, 100, None,
            );
            entry.timestamp_us = (i + 1) * 1000;
            store.append(&entry).unwrap();
        }

        let deleted = store.compact_old(5, u64::MAX).unwrap();
        assert_eq!(deleted, 5);

        let remaining = store.list(&QueryLogListParams::default()).unwrap();
        assert_eq!(remaining.len(), 5);
    }

    #[test]
    fn test_stats() {
        let store = make_test_store();
        store.append(&build_recall_entry(
            "cli", "q1", Some("similarity"), 10, None, 3, vec![], 0.9, 1000, None,
        )).unwrap();
        store.append(&build_recall_entry(
            "hebbs-panel", "q2", Some("similarity"), 10, None, 5, vec![], 0.8, 3000, None,
        )).unwrap();
        store.append(&build_recall_entry(
            "cli", "q3", Some("similarity"), 10, None, 2, vec![], 0.7, 2000, None,
        )).unwrap();

        let stats = store.stats(None).unwrap();
        assert_eq!(stats.total_queries, 3);
        assert_eq!(stats.avg_latency_us, 2000);
        assert_eq!(stats.max_latency_us, 3000);
        assert_eq!(stats.by_caller.len(), 2);
    }

    #[test]
    fn test_get_by_id() {
        let store = make_test_store();
        let entry = build_recall_entry(
            "cli", "findme", Some("similarity"), 10, None, 1, vec![], 0.5, 100, None,
        );
        let id = entry.id;
        store.append(&entry).unwrap();

        let found = store.get(id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().query, "findme");

        let not_found = store.get(99999).unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn test_limit_and_offset() {
        let store = make_test_store();
        for i in 0..20 {
            let mut entry = build_recall_entry(
                "cli", &format!("q{}", i), Some("similarity"), 10, None, 1, vec![], 0.5, 100, None,
            );
            entry.timestamp_us = (i + 1) * 1000;
            store.append(&entry).unwrap();
        }

        let params = QueryLogListParams {
            limit: Some(5),
            offset: Some(2),
            ..Default::default()
        };
        let results = store.list(&params).unwrap();
        assert_eq!(results.len(), 5);
        // Newest first, so offset 2 skips the 2 newest
        assert_eq!(results[0].query, "q17");
    }
}
