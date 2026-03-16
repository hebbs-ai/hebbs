//! Key encoding utilities for HEBBS column families.
//!
//! Key encoding determines sort order, scan efficiency, and prefix
//! iteration behavior. RocksDB iterates in byte order, so keys must
//! be byte-sortable in the order that queries need.
//!
//! ## Encoding principles
//!
//! - Fixed-length components (ULID, timestamps) use big-endian encoding
//!   so byte sort == numeric sort.
//! - Variable-length components (entity_id) are followed by `\xFF` separator
//!   which cannot appear in valid UTF-8 strings (max byte `0xF4`).
//! - Timestamps are u64 microseconds, big-endian, for chronological byte order.

/// Separator byte between variable-length key components.
/// `0xFF` is safe because entity IDs are UTF-8 (max byte `0xF4`).
pub const KEY_SEPARATOR: u8 = 0xFF;

/// Encode a ULID (128-bit) as a 16-byte big-endian key.
/// Since ULIDs are time-sortable, byte order == creation order.
///
/// Complexity: O(1).
#[inline]
pub fn encode_memory_key(ulid_bytes: &[u8]) -> Vec<u8> {
    debug_assert_eq!(ulid_bytes.len(), 16, "ULID must be exactly 16 bytes");
    ulid_bytes.to_vec()
}

/// Encode a temporal index key: `[entity_id][0xFF][timestamp_be_u64]`.
///
/// Enables:
/// - Prefix scan on `entity_id + 0xFF` → all memories for an entity, chronological.
/// - Range scan on `entity_id + 0xFF + start_ts .. entity_id + 0xFF + end_ts`.
/// - Both operations are O(log n + k) where k = result set size.
///
/// Complexity: O(len(entity_id)).
pub fn encode_temporal_key(entity_id: &str, timestamp_us: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(entity_id.len() + 1 + 8);
    key.extend_from_slice(entity_id.as_bytes());
    key.push(KEY_SEPARATOR);
    key.extend_from_slice(&timestamp_us.to_be_bytes());
    key
}

/// Build the prefix for scanning all temporal entries of an entity.
///
/// Complexity: O(len(entity_id)).
pub fn encode_temporal_prefix(entity_id: &str) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(entity_id.len() + 1);
    prefix.extend_from_slice(entity_id.as_bytes());
    prefix.push(KEY_SEPARATOR);
    prefix
}

/// Encode a graph index key:
/// `[source_memory_id 16B][edge_type u8][target_memory_id 16B]`.
///
/// Enables:
/// - Prefix scan on `source_memory_id` → all outgoing edges.
/// - Prefix scan on `source_memory_id + edge_type` → edges of a specific type.
/// - Both operations are O(log n + k).
///
/// Complexity: O(1).
pub fn encode_graph_key(source_id: &[u8], edge_type: u8, target_id: &[u8]) -> Vec<u8> {
    debug_assert_eq!(source_id.len(), 16, "source ULID must be 16 bytes");
    debug_assert_eq!(target_id.len(), 16, "target ULID must be 16 bytes");
    let mut key = Vec::with_capacity(33);
    key.extend_from_slice(source_id);
    key.push(edge_type);
    key.extend_from_slice(target_id);
    key
}

/// Encode a meta column family key from a string name.
///
/// Complexity: O(len(name)).
#[inline]
pub fn encode_meta_key(name: &str) -> Vec<u8> {
    name.as_bytes().to_vec()
}

/// Decode a u64 from 8 big-endian bytes.
///
/// Complexity: O(1).
#[inline]
pub fn decode_u64_be(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[..8]);
    u64::from_be_bytes(buf)
}

/// Encode a u64 as 8 big-endian bytes.
///
/// Complexity: O(1).
#[inline]
pub fn encode_u64_be(val: u64) -> [u8; 8] {
    val.to_be_bytes()
}

/// Prefix for pending contradiction keys in the Pending CF.
pub const PENDING_CONTRADICTION_PREFIX: &[u8] = b"ctr:";

/// Encode a pending contradiction key: `[ctr:][pending_id 16B]`.
///
/// Enables prefix scan on `ctr:` to retrieve all pending contradiction
/// candidates awaiting AI review.
///
/// Complexity: O(1).
#[inline]
pub fn encode_pending_contradiction_key(pending_id: &[u8; 16]) -> Vec<u8> {
    let mut key = Vec::with_capacity(PENDING_CONTRADICTION_PREFIX.len() + 16);
    key.extend_from_slice(PENDING_CONTRADICTION_PREFIX);
    key.extend_from_slice(pending_id);
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_key_preserves_bytes() {
        let ulid = [1u8; 16];
        assert_eq!(encode_memory_key(&ulid), ulid.to_vec());
    }

    #[test]
    fn temporal_key_format() {
        let key = encode_temporal_key("customer_42", 1_700_000_000_000_000);
        // entity_id bytes + separator + 8 timestamp bytes
        assert_eq!(key.len(), "customer_42".len() + 1 + 8);
        assert_eq!(key["customer_42".len()], KEY_SEPARATOR);
    }

    #[test]
    fn temporal_keys_sort_chronologically() {
        let k1 = encode_temporal_key("entity", 100);
        let k2 = encode_temporal_key("entity", 200);
        let k3 = encode_temporal_key("entity", 300);
        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    #[test]
    fn temporal_keys_isolate_entities() {
        let ka = encode_temporal_key("alice", 100);
        let kb = encode_temporal_key("bob", 100);
        let prefix_a = encode_temporal_prefix("alice");
        let prefix_b = encode_temporal_prefix("bob");
        assert!(ka.starts_with(&prefix_a));
        assert!(!ka.starts_with(&prefix_b));
        assert!(kb.starts_with(&prefix_b));
        assert!(!kb.starts_with(&prefix_a));
    }

    #[test]
    fn graph_key_format() {
        let source = [1u8; 16];
        let target = [2u8; 16];
        let edge_type = 3u8;
        let key = encode_graph_key(&source, edge_type, &target);
        assert_eq!(key.len(), 33);
        assert_eq!(&key[..16], &source);
        assert_eq!(key[16], edge_type);
        assert_eq!(&key[17..], &target);
    }

    #[test]
    fn u64_roundtrip() {
        let val = 0xDEAD_BEEF_CAFE_BABEu64;
        let bytes = encode_u64_be(val);
        assert_eq!(decode_u64_be(&bytes), val);
    }

    #[test]
    fn u64_big_endian_sorts_numerically() {
        let a = encode_u64_be(100);
        let b = encode_u64_be(200);
        let c = encode_u64_be(u64::MAX);
        assert!(a.as_slice() < b.as_slice());
        assert!(b.as_slice() < c.as_slice());
    }

    #[test]
    fn pending_contradiction_key_format() {
        let id = [0xABu8; 16];
        let key = encode_pending_contradiction_key(&id);
        assert_eq!(key.len(), PENDING_CONTRADICTION_PREFIX.len() + 16);
        assert!(key.starts_with(PENDING_CONTRADICTION_PREFIX));
        assert_eq!(&key[PENDING_CONTRADICTION_PREFIX.len()..], &id);
    }
}
