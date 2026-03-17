use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use hebbs_storage::{ColumnFamilyName, StorageBackend};

use crate::error::{IndexError, Result};

// --- Key prefix bytes for collision avoidance ---
// Forward and reverse keys occupy disjoint ranges in the graph CF.
// 0xF0-0xFF are reserved for graph index key types, safe because
// ULID timestamps in the next century produce first bytes in 0x00-0x02.

/// Prefix byte for forward edges: source → target.
const FORWARD_PREFIX: u8 = 0xF0;
/// Prefix byte for reverse edges: target → source (for backward traversal).
const REVERSE_PREFIX: u8 = 0xF1;

/// Edge types in the HEBBS graph index.
///
/// Byte assignments are immutable after Phase 3 (append-only).
/// New types can be added in later phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EdgeType {
    CausedBy = 0x01,
    RelatedTo = 0x02,
    FollowedBy = 0x03,
    /// Reserved for Phase 5 (revise).
    RevisedFrom = 0x04,
    /// Reserved for Phase 7 (reflect).
    InsightFrom = 0x05,
    /// Contradiction detection edge.
    Contradicts = 0x06,
    /// Memory references an entity.
    HasEntity = 0x07,
    /// Directed relationship between two entities.
    EntityRelation = 0x08,
    /// Proposition extracted from a document.
    PropositionOf = 0x09,
}

impl EdgeType {
    pub fn from_byte(byte: u8) -> Result<Self> {
        match byte {
            0x01 => Ok(EdgeType::CausedBy),
            0x02 => Ok(EdgeType::RelatedTo),
            0x03 => Ok(EdgeType::FollowedBy),
            0x04 => Ok(EdgeType::RevisedFrom),
            0x05 => Ok(EdgeType::InsightFrom),
            0x06 => Ok(EdgeType::Contradicts),
            0x07 => Ok(EdgeType::HasEntity),
            0x08 => Ok(EdgeType::EntityRelation),
            0x09 => Ok(EdgeType::PropositionOf),
            _ => Err(IndexError::InvalidEdgeType { value: byte }),
        }
    }

    pub fn as_byte(self) -> u8 {
        self as u8
    }

    /// Edge types that Phase 3 can create during remember().
    pub fn phase3_types() -> &'static [EdgeType] {
        &[
            EdgeType::CausedBy,
            EdgeType::RelatedTo,
            EdgeType::FollowedBy,
        ]
    }
}

/// Lightweight edge metadata stored as the value in graph CF entries.
///
/// Format: `[confidence: f32 LE][timestamp: u64 LE]` = 12 bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeMetadata {
    pub confidence: f32,
    pub timestamp_us: u64,
}

impl EdgeMetadata {
    pub fn new(confidence: f32, timestamp_us: u64) -> Self {
        Self {
            confidence,
            timestamp_us,
        }
    }

    /// Serialize to compact binary format (12 bytes).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(12);
        buf.extend_from_slice(&self.confidence.to_le_bytes());
        buf.extend_from_slice(&self.timestamp_us.to_le_bytes());
        buf
    }

    /// Deserialize from binary format.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(IndexError::Serialization {
                message: format!("edge metadata too short: {} bytes, need 12", data.len()),
            });
        }
        let confidence = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let timestamp_us = u64::from_le_bytes([
            data[4], data[5], data[6], data[7], data[8], data[9], data[10], data[11],
        ]);
        Ok(Self {
            confidence,
            timestamp_us,
        })
    }
}

/// A traversal result entry.
#[derive(Debug, Clone)]
pub struct TraversalEntry {
    pub memory_id: [u8; 16],
    pub depth: usize,
    pub edge_type: EdgeType,
}

/// Graph index operating on the `graph` column family.
///
/// Stores directed, typed edges between memories with bidirectional
/// key encoding for efficient forward and backward traversal.
///
/// ## Key encoding
///
/// | Direction | Format |
/// |-----------|--------|
/// | Forward | `[0xF0][source_id 16B][edge_type 1B][target_id 16B]` |
/// | Reverse | `[0xF1][target_id 16B][edge_type 1B][source_id 16B]` |
///
/// Both entries are always written together in the same WriteBatch.
///
/// ## Query complexity
///
/// | Operation | Complexity |
/// |-----------|-----------|
/// | Outgoing edges from M | O(log n + k) |
/// | Incoming edges to M | O(log n + k) |
/// | Bounded traversal depth D | O(branching_factor^D) |
pub struct GraphIndex {
    storage: Arc<dyn StorageBackend>,
}

impl GraphIndex {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
    }

    /// Encode a forward edge key: `[0xF0][source 16B][edge_type 1B][target 16B]`.
    /// Total: 34 bytes.
    #[inline]
    pub fn encode_forward_key(
        source_id: &[u8; 16],
        edge_type: EdgeType,
        target_id: &[u8; 16],
    ) -> Vec<u8> {
        let mut key = Vec::with_capacity(34);
        key.push(FORWARD_PREFIX);
        key.extend_from_slice(source_id);
        key.push(edge_type.as_byte());
        key.extend_from_slice(target_id);
        key
    }

    /// Encode a reverse edge key: `[0xF1][target 16B][edge_type 1B][source 16B]`.
    /// Total: 34 bytes.
    #[inline]
    pub fn encode_reverse_key(
        source_id: &[u8; 16],
        edge_type: EdgeType,
        target_id: &[u8; 16],
    ) -> Vec<u8> {
        let mut key = Vec::with_capacity(34);
        key.push(REVERSE_PREFIX);
        key.extend_from_slice(target_id);
        key.push(edge_type.as_byte());
        key.extend_from_slice(source_id);
        key
    }

    /// Build the prefix for scanning all outgoing edges from a memory.
    /// `[0xF0][memory_id 16B]` = 17 bytes.
    #[inline]
    pub fn encode_forward_prefix(memory_id: &[u8; 16]) -> Vec<u8> {
        let mut prefix = Vec::with_capacity(17);
        prefix.push(FORWARD_PREFIX);
        prefix.extend_from_slice(memory_id);
        prefix
    }

    /// Build the prefix for scanning outgoing edges of a specific type.
    /// `[0xF0][memory_id 16B][edge_type 1B]` = 18 bytes.
    #[inline]
    pub fn encode_forward_type_prefix(memory_id: &[u8; 16], edge_type: EdgeType) -> Vec<u8> {
        let mut prefix = Vec::with_capacity(18);
        prefix.push(FORWARD_PREFIX);
        prefix.extend_from_slice(memory_id);
        prefix.push(edge_type.as_byte());
        prefix
    }

    /// Build the prefix for scanning all incoming edges to a memory.
    /// `[0xF1][memory_id 16B]` = 17 bytes.
    #[inline]
    pub fn encode_reverse_prefix(memory_id: &[u8; 16]) -> Vec<u8> {
        let mut prefix = Vec::with_capacity(17);
        prefix.push(REVERSE_PREFIX);
        prefix.extend_from_slice(memory_id);
        prefix
    }

    /// Decode a forward key into (source_id, edge_type, target_id).
    pub fn decode_forward_key(key: &[u8]) -> Result<([u8; 16], EdgeType, [u8; 16])> {
        if key.len() != 34 || key[0] != FORWARD_PREFIX {
            return Err(IndexError::Serialization {
                message: format!(
                    "invalid forward key: len={}, prefix=0x{:02X}",
                    key.len(),
                    key.first().copied().unwrap_or(0)
                ),
            });
        }
        let mut source = [0u8; 16];
        source.copy_from_slice(&key[1..17]);
        let edge_type = EdgeType::from_byte(key[17])?;
        let mut target = [0u8; 16];
        target.copy_from_slice(&key[18..34]);
        Ok((source, edge_type, target))
    }

    /// Decode a reverse key into (target_id, edge_type, source_id).
    pub fn decode_reverse_key(key: &[u8]) -> Result<([u8; 16], EdgeType, [u8; 16])> {
        if key.len() != 34 || key[0] != REVERSE_PREFIX {
            return Err(IndexError::Serialization {
                message: format!(
                    "invalid reverse key: len={}, prefix=0x{:02X}",
                    key.len(),
                    key.first().copied().unwrap_or(0)
                ),
            });
        }
        let mut target = [0u8; 16];
        target.copy_from_slice(&key[1..17]);
        let edge_type = EdgeType::from_byte(key[17])?;
        let mut source = [0u8; 16];
        source.copy_from_slice(&key[18..34]);
        Ok((target, edge_type, source))
    }

    /// Get all outgoing edges from a memory.
    ///
    /// Complexity: O(log n + k) where k = number of outgoing edges.
    pub fn outgoing_edges(
        &self,
        memory_id: &[u8; 16],
    ) -> Result<Vec<(EdgeType, [u8; 16], EdgeMetadata)>> {
        let prefix = Self::encode_forward_prefix(memory_id);
        let entries = self
            .storage
            .prefix_iterator(ColumnFamilyName::Graph, &prefix)?;

        let mut edges = Vec::new();
        for (key, value) in entries {
            if key.len() == 34 {
                let (_, edge_type, target) = Self::decode_forward_key(&key)?;
                let metadata = EdgeMetadata::from_bytes(&value)?;
                edges.push((edge_type, target, metadata));
            }
        }
        Ok(edges)
    }

    /// Get outgoing edges of a specific type from a memory.
    ///
    /// Complexity: O(log n + k).
    pub fn outgoing_edges_of_type(
        &self,
        memory_id: &[u8; 16],
        edge_type: EdgeType,
    ) -> Result<Vec<([u8; 16], EdgeMetadata)>> {
        let prefix = Self::encode_forward_type_prefix(memory_id, edge_type);
        let entries = self
            .storage
            .prefix_iterator(ColumnFamilyName::Graph, &prefix)?;

        let mut edges = Vec::new();
        for (key, value) in entries {
            if key.len() == 34 {
                let (_, _, target) = Self::decode_forward_key(&key)?;
                let metadata = EdgeMetadata::from_bytes(&value)?;
                edges.push((target, metadata));
            }
        }
        Ok(edges)
    }

    /// Get all incoming edges to a memory.
    ///
    /// Complexity: O(log n + k).
    pub fn incoming_edges(
        &self,
        memory_id: &[u8; 16],
    ) -> Result<Vec<(EdgeType, [u8; 16], EdgeMetadata)>> {
        let prefix = Self::encode_reverse_prefix(memory_id);
        let entries = self
            .storage
            .prefix_iterator(ColumnFamilyName::Graph, &prefix)?;

        let mut edges = Vec::new();
        for (key, value) in entries {
            if key.len() == 34 {
                let (_, edge_type, source) = Self::decode_reverse_key(&key)?;
                let metadata = EdgeMetadata::from_bytes(&value)?;
                edges.push((edge_type, source, metadata));
            }
        }
        Ok(edges)
    }

    /// Bounded BFS traversal from a seed memory following specified edge types.
    ///
    /// Returns all reachable memories up to `max_depth` hops, with cycle detection.
    /// The seed memory itself is NOT included in results.
    ///
    /// `max_depth` is bounded (default 10, from Principle 4).
    /// Returns partial results with `truncated` flag if the bound is hit.
    ///
    /// Complexity: O(branching_factor^max_depth).
    pub fn traverse(
        &self,
        seed_id: &[u8; 16],
        edge_types: &[EdgeType],
        max_depth: usize,
        max_results: usize,
    ) -> Result<(Vec<TraversalEntry>, bool)> {
        let mut visited: HashSet<[u8; 16]> = HashSet::new();
        visited.insert(*seed_id);

        let mut queue: VecDeque<([u8; 16], usize)> = VecDeque::new();
        queue.push_back((*seed_id, 0));

        let mut results: Vec<TraversalEntry> = Vec::new();
        let mut truncated = false;

        while let Some((current_id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            for &edge_type in edge_types {
                let prefix = Self::encode_forward_type_prefix(&current_id, edge_type);
                let entries = self
                    .storage
                    .prefix_iterator(ColumnFamilyName::Graph, &prefix)?;

                for (key, _) in entries {
                    if key.len() != 34 {
                        continue;
                    }

                    let (_, _, target) = Self::decode_forward_key(&key)?;

                    if !visited.insert(target) {
                        continue;
                    }

                    if results.len() >= max_results {
                        truncated = true;
                        return Ok((results, truncated));
                    }

                    results.push(TraversalEntry {
                        memory_id: target,
                        depth: depth + 1,
                        edge_type,
                    });

                    queue.push_back((target, depth + 1));
                }
            }
        }

        Ok((results, truncated))
    }

    /// Collect all edge keys (forward + reverse) touching a memory.
    /// Used by delete to build the WriteBatch for atomic edge removal.
    ///
    /// Complexity: O(log n + degree).
    pub fn collect_edge_keys_for_delete(&self, memory_id: &[u8; 16]) -> Result<Vec<Vec<u8>>> {
        let mut keys_to_delete = Vec::new();

        // Forward edges from this memory: [0xF0][memory_id]...
        let fwd_prefix = Self::encode_forward_prefix(memory_id);
        let fwd_entries = self
            .storage
            .prefix_iterator(ColumnFamilyName::Graph, &fwd_prefix)?;

        for (key, _) in &fwd_entries {
            if key.len() == 34 {
                // Delete the forward key
                keys_to_delete.push(key.clone());
                // Also delete the corresponding reverse key
                let (source, edge_type, target) = Self::decode_forward_key(key)?;
                let rev_key = Self::encode_reverse_key(&source, edge_type, &target);
                keys_to_delete.push(rev_key);
            }
        }

        // Reverse edges to this memory: [0xF1][memory_id]...
        let rev_prefix = Self::encode_reverse_prefix(memory_id);
        let rev_entries = self
            .storage
            .prefix_iterator(ColumnFamilyName::Graph, &rev_prefix)?;

        for (key, _) in &rev_entries {
            if key.len() == 34 {
                // Delete the reverse key
                keys_to_delete.push(key.clone());
                // Also delete the corresponding forward key
                let (target, edge_type, source) = Self::decode_reverse_key(key)?;
                let fwd_key = Self::encode_forward_key(&source, edge_type, &target);
                keys_to_delete.push(fwd_key);
            }
        }

        // Deduplicate (a forward key's reverse counterpart may appear twice)
        keys_to_delete.sort();
        keys_to_delete.dedup();

        Ok(keys_to_delete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hebbs_storage::{BatchOperation, InMemoryBackend};

    fn test_index() -> GraphIndex {
        GraphIndex::new(Arc::new(InMemoryBackend::new()))
    }

    fn make_id(val: u8) -> [u8; 16] {
        [val; 16]
    }

    fn insert_edge(
        index: &GraphIndex,
        source: &[u8; 16],
        edge_type: EdgeType,
        target: &[u8; 16],
        confidence: f32,
        ts: u64,
    ) {
        let fwd_key = GraphIndex::encode_forward_key(source, edge_type, target);
        let rev_key = GraphIndex::encode_reverse_key(source, edge_type, target);
        let meta = EdgeMetadata::new(confidence, ts).to_bytes();

        index
            .storage
            .write_batch(&[
                BatchOperation::Put {
                    cf: ColumnFamilyName::Graph,
                    key: fwd_key,
                    value: meta.clone(),
                },
                BatchOperation::Put {
                    cf: ColumnFamilyName::Graph,
                    key: rev_key,
                    value: meta,
                },
            ])
            .unwrap();
    }

    #[test]
    fn forward_key_encoding() {
        let source = make_id(1);
        let target = make_id(2);
        let key = GraphIndex::encode_forward_key(&source, EdgeType::CausedBy, &target);
        assert_eq!(key.len(), 34);
        assert_eq!(key[0], FORWARD_PREFIX);
        assert_eq!(&key[1..17], &source);
        assert_eq!(key[17], EdgeType::CausedBy.as_byte());
        assert_eq!(&key[18..34], &target);
    }

    #[test]
    fn reverse_key_encoding() {
        let source = make_id(1);
        let target = make_id(2);
        let key = GraphIndex::encode_reverse_key(&source, EdgeType::CausedBy, &target);
        assert_eq!(key.len(), 34);
        assert_eq!(key[0], REVERSE_PREFIX);
        assert_eq!(&key[1..17], &target); // target comes first in reverse key
        assert_eq!(key[17], EdgeType::CausedBy.as_byte());
        assert_eq!(&key[18..34], &source);
    }

    #[test]
    fn forward_reverse_keys_in_disjoint_ranges() {
        let id = make_id(1);
        let fwd = GraphIndex::encode_forward_prefix(&id);
        let rev = GraphIndex::encode_reverse_prefix(&id);
        assert_ne!(fwd[0], rev[0]);
    }

    #[test]
    fn edge_metadata_roundtrip() {
        let meta = EdgeMetadata::new(0.95, 1_700_000_000_000_000);
        let bytes = meta.to_bytes();
        let restored = EdgeMetadata::from_bytes(&bytes).unwrap();
        assert!((meta.confidence - restored.confidence).abs() < 1e-6);
        assert_eq!(meta.timestamp_us, restored.timestamp_us);
    }

    #[test]
    fn edge_type_roundtrip() {
        for &et in &[
            EdgeType::CausedBy,
            EdgeType::RelatedTo,
            EdgeType::FollowedBy,
            EdgeType::RevisedFrom,
            EdgeType::InsightFrom,
            EdgeType::Contradicts,
        ] {
            let byte = et.as_byte();
            let restored = EdgeType::from_byte(byte).unwrap();
            assert_eq!(et, restored);
        }
    }

    #[test]
    fn invalid_edge_type_rejected() {
        let result = EdgeType::from_byte(0xFF);
        assert!(matches!(result, Err(IndexError::InvalidEdgeType { .. })));
    }

    #[test]
    fn outgoing_edges() {
        let index = test_index();
        let a = make_id(1);
        let b = make_id(2);
        let c = make_id(3);

        insert_edge(&index, &a, EdgeType::CausedBy, &b, 1.0, 100);
        insert_edge(&index, &a, EdgeType::RelatedTo, &c, 0.8, 200);

        let edges = index.outgoing_edges(&a).unwrap();
        assert_eq!(edges.len(), 2);

        let types: HashSet<EdgeType> = edges.iter().map(|(et, _, _)| *et).collect();
        assert!(types.contains(&EdgeType::CausedBy));
        assert!(types.contains(&EdgeType::RelatedTo));
    }

    #[test]
    fn incoming_edges() {
        let index = test_index();
        let a = make_id(1);
        let b = make_id(2);

        insert_edge(&index, &a, EdgeType::CausedBy, &b, 1.0, 100);

        let incoming = index.incoming_edges(&b).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].0, EdgeType::CausedBy);
        assert_eq!(incoming[0].1, a);
    }

    #[test]
    fn outgoing_edges_of_type() {
        let index = test_index();
        let a = make_id(1);
        let b = make_id(2);
        let c = make_id(3);

        insert_edge(&index, &a, EdgeType::CausedBy, &b, 1.0, 100);
        insert_edge(&index, &a, EdgeType::RelatedTo, &c, 0.8, 200);

        let caused = index
            .outgoing_edges_of_type(&a, EdgeType::CausedBy)
            .unwrap();
        assert_eq!(caused.len(), 1);
        assert_eq!(caused[0].0, b);
    }

    #[test]
    fn traverse_bounded_depth() {
        let index = test_index();

        // Build a chain: 1 -> 2 -> 3 -> 4 -> 5
        for i in 1..5u8 {
            insert_edge(
                &index,
                &make_id(i),
                EdgeType::FollowedBy,
                &make_id(i + 1),
                1.0,
                i as u64 * 100,
            );
        }

        // Traverse from 1 with max_depth=2
        let (results, truncated) = index
            .traverse(&make_id(1), &[EdgeType::FollowedBy], 2, 100)
            .unwrap();

        assert!(!truncated);
        assert_eq!(results.len(), 2); // nodes 2 and 3
        assert_eq!(results[0].memory_id, make_id(2));
        assert_eq!(results[0].depth, 1);
        assert_eq!(results[1].memory_id, make_id(3));
        assert_eq!(results[1].depth, 2);
    }

    #[test]
    fn traverse_cycle_detection() {
        let index = test_index();

        // Build a cycle: 1 -> 2 -> 3 -> 1
        insert_edge(
            &index,
            &make_id(1),
            EdgeType::FollowedBy,
            &make_id(2),
            1.0,
            100,
        );
        insert_edge(
            &index,
            &make_id(2),
            EdgeType::FollowedBy,
            &make_id(3),
            1.0,
            200,
        );
        insert_edge(
            &index,
            &make_id(3),
            EdgeType::FollowedBy,
            &make_id(1),
            1.0,
            300,
        );

        let (results, truncated) = index
            .traverse(&make_id(1), &[EdgeType::FollowedBy], 10, 100)
            .unwrap();

        assert!(!truncated);
        assert_eq!(results.len(), 2); // nodes 2 and 3 (1 is the seed, not revisited)
    }

    #[test]
    fn traverse_depth_zero_returns_empty() {
        let index = test_index();
        insert_edge(
            &index,
            &make_id(1),
            EdgeType::FollowedBy,
            &make_id(2),
            1.0,
            100,
        );

        let (results, _) = index
            .traverse(&make_id(1), &[EdgeType::FollowedBy], 0, 100)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn traverse_max_results_truncation() {
        let index = test_index();

        // Build a star: 1 -> {2, 3, 4, 5, 6}
        for i in 2..7u8 {
            insert_edge(
                &index,
                &make_id(1),
                EdgeType::RelatedTo,
                &make_id(i),
                1.0,
                100,
            );
        }

        let (results, truncated) = index
            .traverse(&make_id(1), &[EdgeType::RelatedTo], 1, 3)
            .unwrap();

        assert!(truncated);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn collect_edge_keys_for_delete() {
        let index = test_index();
        let a = make_id(1);
        let b = make_id(2);
        let c = make_id(3);

        insert_edge(&index, &a, EdgeType::CausedBy, &b, 1.0, 100);
        insert_edge(&index, &c, EdgeType::RelatedTo, &a, 0.5, 200);

        let keys = index.collect_edge_keys_for_delete(&a).unwrap();
        // a->b forward + a->b reverse + c->a forward + c->a reverse = 4
        assert_eq!(keys.len(), 4);
    }

    #[test]
    fn no_edges_returns_empty() {
        let index = test_index();
        let edges = index.outgoing_edges(&make_id(99)).unwrap();
        assert!(edges.is_empty());
    }

    #[test]
    fn forward_key_decode_roundtrip() {
        let source = make_id(1);
        let target = make_id(2);
        let key = GraphIndex::encode_forward_key(&source, EdgeType::FollowedBy, &target);
        let (dec_source, dec_type, dec_target) = GraphIndex::decode_forward_key(&key).unwrap();
        assert_eq!(dec_source, source);
        assert_eq!(dec_type, EdgeType::FollowedBy);
        assert_eq!(dec_target, target);
    }

    #[test]
    fn reverse_key_decode_roundtrip() {
        let source = make_id(1);
        let target = make_id(2);
        let key = GraphIndex::encode_reverse_key(&source, EdgeType::CausedBy, &target);
        let (dec_target, dec_type, dec_source) = GraphIndex::decode_reverse_key(&key).unwrap();
        assert_eq!(dec_source, source);
        assert_eq!(dec_type, EdgeType::CausedBy);
        assert_eq!(dec_target, target);
    }
}
