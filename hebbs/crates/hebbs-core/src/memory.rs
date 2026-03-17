use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// CRC-32C (Castagnoli) lookup table for polynomial 0x1EDC6F41.
const CRC32C_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0x82F6_3B78; // reflected polynomial
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};

/// Compute CRC-32C (Castagnoli) checksum using a lookup table.
fn crc32c(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = CRC32C_TABLE[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Version marker byte: plain (legacy) format without checksum.
const FORMAT_PLAIN: u8 = 0x00;
/// Version marker byte: checksummed format with trailing CRC-32C.
const FORMAT_CHECKSUMMED: u8 = 0x01;

/// The kind of memory — distinguishes raw episodes from consolidated
/// insights and revised versions. Only `Episode` is used in Phase 1;
/// the enum exists from day one so future variants don't require schema migration.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub enum MemoryKind {
    #[default]
    Episode,
    Insight,
    Revision,
    /// Whole-file or LLM-summarized document memory (Layer 1).
    Document,
    /// Atomic fact extracted by LLM (Layer 2).
    Proposition,
}

/// The canonical HEBBS memory record.
///
/// This is the single most important type in the system. Every subsequent
/// module reads, writes, queries, or transforms it. Its layout is designed
/// to accommodate all 17 phases without breaking serialization.
///
/// ## Serialization
///
/// Serialized with bitcode (gamma-encoded lengths, bitwise packing).
/// Produces ~20-30% smaller output than bincode while maintaining
/// comparable speed. Highly compressible under LZ4/Zstd (RocksDB's
/// compression layers), reducing SST file sizes and improving block
/// cache hit rates.
///
/// Schema evolution is append-only: new fields are added as `Option<T>`
/// at the end. Removing or reordering fields is a breaking change
/// requiring offline migration.
///
/// ## Context field
///
/// The `context` field is stored as pre-serialized JSON bytes within the
/// bitcode payload. This avoids requiring `serde_json::Value` to implement
/// bitcode traits while preserving the structured-metadata semantics.
///
/// ## Field sizing
///
/// At ~200-220 bytes per record (without embedding), 10M memories ≈ 2.0-2.2 GB.
/// The `embedding` field adds `D * 4` bytes when populated (1,536 bytes
/// for 384-dim, 6,144 bytes for 1536-dim).
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct Memory {
    /// Globally unique, sortable identifier (ULID, 128-bit).
    /// Time-sortable without coordination — critical for edge sync.
    pub memory_id: Vec<u8>,

    /// The raw experience text. Bounded by max length (default 64 KB).
    pub content: String,

    /// Importance score in `[0.0, 1.0]`. Drives decay scoring, recall
    /// ranking, and conflict resolution in sync. Defaults to 0.5.
    pub importance: f32,

    /// Arbitrary structured metadata stored as JSON bytes.
    /// Deserialized on demand via `Memory::context()`.
    pub context_bytes: Vec<u8>,

    /// Extracted from context for temporal index key prefix. `None` if
    /// the memory is not scoped to an entity.
    pub entity_id: Option<String>,

    /// Vector embedding. `None` in Phase 1 — populated by Phase 2.
    /// Included from day one to avoid serialization migration.
    pub embedding: Option<Vec<f32>>,

    /// Microseconds since Unix epoch. Compact, deterministic ordering.
    pub created_at: u64,

    /// Set equal to `created_at` on initial write; updated by `revise()`.
    pub updated_at: u64,

    /// Updated on every `recall()` hit. Drives decay scoring.
    pub last_accessed_at: u64,

    /// Incremented on every recall hit.
    /// Reinforcement signal: `decay_score = importance * 2^(-age/half_life) * log(1 + access_count)`.
    pub access_count: u64,

    /// Cached decay score for background sweep efficiency.
    /// Recalculated on read from importance, timestamps, and access_count.
    pub decay_score: f32,

    /// Distinguishes episodes, insights, and revisions.
    pub kind: MemoryKind,

    /// Which device created this memory (edge sync, Phase 13).
    pub device_id: Option<String>,

    /// Monotonically increasing per-device clock for conflict resolution.
    pub logical_clock: u64,

    /// Associative embedding that evolves via Hebbian updates as edges are created.
    /// Initially equals the content embedding. `None` for legacy records.
    pub associative_embedding: Option<Vec<f32>>,
}

/// Maximum content length in bytes (64 KB).
pub const MAX_CONTENT_LENGTH: usize = 64 * 1024;

/// Maximum serialized context size in bytes (16 KB).
pub const MAX_CONTEXT_SIZE: usize = 16 * 1024;

/// Default importance when not specified by the caller.
pub const DEFAULT_IMPORTANCE: f32 = 0.5;

impl Memory {
    /// Deserialize the context map from the stored JSON bytes.
    ///
    /// Returns an empty map if the context bytes are empty.
    pub fn context(&self) -> Result<HashMap<String, serde_json::Value>, String> {
        if self.context_bytes.is_empty() {
            return Ok(HashMap::new());
        }
        serde_json::from_slice(&self.context_bytes)
            .map_err(|e| format!("failed to deserialize context: {}", e))
    }

    /// Serialize a context map to JSON bytes for storage.
    pub fn serialize_context(
        context: &HashMap<String, serde_json::Value>,
    ) -> Result<Vec<u8>, String> {
        if context.is_empty() {
            return Ok(Vec::new());
        }
        serde_json::to_vec(context).map_err(|e| format!("failed to serialize context: {}", e))
    }

    /// Serialize this memory to bitcode bytes (plain format, no checksum).
    ///
    /// Uses bitcode with gamma-encoded lengths and bitwise packing
    /// for compact output that compresses well under LZ4/Zstd.
    ///
    /// This is infallible — bitcode encoding cannot fail for valid Rust types.
    pub fn to_bytes(&self) -> Vec<u8> {
        bitcode::encode(self)
    }

    /// Serialize this memory to bitcode bytes with a CRC-32C integrity checksum.
    ///
    /// Wire format: `[0x01][bitcode payload...][CRC-32C LE 4B]`
    ///
    /// The version marker (`0x01`) distinguishes this from plain format,
    /// enabling backward-compatible deserialization in [`from_bytes`].
    pub fn to_bytes_checksummed(&self) -> Vec<u8> {
        let payload = bitcode::encode(self);
        let checksum = crc32c(&payload);
        let mut out = Vec::with_capacity(1 + payload.len() + 4);
        out.push(FORMAT_CHECKSUMMED);
        out.extend_from_slice(&payload);
        out.extend_from_slice(&checksum.to_le_bytes());
        out
    }

    /// Deserialize a memory from bitcode bytes.
    ///
    /// Handles both formats transparently:
    /// - `0x01` prefix: checksummed — verifies CRC-32C before deserializing.
    /// - `0x00` prefix or any other: plain bitcode (backward compat).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("deserialization failed: empty input".to_string());
        }

        if bytes[0] == FORMAT_CHECKSUMMED {
            // Minimum: 1 (marker) + 1 (payload) + 4 (checksum)
            if bytes.len() < 6 {
                return Err("deserialization failed: checksummed data too short".to_string());
            }
            let payload = &bytes[1..bytes.len() - 4];
            let stored_checksum = u32::from_le_bytes(bytes[bytes.len() - 4..].try_into().unwrap());
            let computed = crc32c(payload);
            if stored_checksum != computed {
                return Err("CRC-32C checksum mismatch".to_string());
            }
            bitcode::decode(payload).map_err(|e| format!("deserialization failed: {}", e))
        } else if bytes[0] == FORMAT_PLAIN {
            bitcode::decode(&bytes[1..]).map_err(|e| format!("deserialization failed: {}", e))
        } else {
            // Legacy data without any version marker — decode as raw bitcode
            bitcode::decode(bytes).map_err(|e| format!("deserialization failed: {}", e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_memory() -> Memory {
        let mut context = HashMap::new();
        context.insert(
            "stage".to_string(),
            serde_json::Value::String("discovery".to_string()),
        );
        let context_bytes = Memory::serialize_context(&context).unwrap();

        Memory {
            memory_id: vec![0u8; 16],
            content: "Customer expressed urgency about Q4 deadline".to_string(),
            importance: 0.8,
            context_bytes,
            entity_id: Some("customer_123".to_string()),
            embedding: None,
            created_at: 1_700_000_000_000_000,
            updated_at: 1_700_000_000_000_000,
            last_accessed_at: 1_700_000_000_000_000,
            access_count: 0,
            decay_score: 0.8,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        }
    }

    #[test]
    fn serialization_roundtrip() {
        let mem = sample_memory();
        let bytes = mem.to_bytes();
        let restored = Memory::from_bytes(&bytes).unwrap();
        assert_eq!(mem, restored);
    }

    #[test]
    fn context_roundtrip() {
        let mem = sample_memory();
        let ctx = mem.context().unwrap();
        assert_eq!(ctx["stage"], serde_json::json!("discovery"));
    }

    #[test]
    fn serialization_compact() {
        let mem = sample_memory();
        let bytes = mem.to_bytes();
        assert!(
            bytes.len() < 500,
            "serialized size {} exceeds 500 byte expectation",
            bytes.len()
        );
    }

    #[test]
    fn roundtrip_with_embedding() {
        let mut mem = sample_memory();
        mem.embedding = Some(vec![0.1; 384]);
        let bytes = mem.to_bytes();
        let restored = Memory::from_bytes(&bytes).unwrap();
        assert_eq!(mem.embedding, restored.embedding);
    }

    #[test]
    fn roundtrip_with_all_options_none() {
        let mem = Memory {
            memory_id: vec![1u8; 16],
            content: "minimal".to_string(),
            importance: 0.5,
            context_bytes: Vec::new(),
            entity_id: None,
            embedding: None,
            created_at: 0,
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 0,
            decay_score: 0.5,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };
        let bytes = mem.to_bytes();
        let restored = Memory::from_bytes(&bytes).unwrap();
        assert_eq!(mem, restored);
    }

    #[test]
    fn checksummed_roundtrip() {
        let mem = sample_memory();
        let bytes = mem.to_bytes_checksummed();
        assert_eq!(bytes[0], 0x01, "first byte should be checksummed marker");
        let restored = Memory::from_bytes(&bytes).unwrap();
        assert_eq!(mem, restored);
    }

    #[test]
    fn checksummed_detects_corruption() {
        let mem = sample_memory();
        let mut bytes = mem.to_bytes_checksummed();
        // Flip a bit in the payload (not the marker or checksum)
        bytes[5] ^= 0x01;
        let err = Memory::from_bytes(&bytes).unwrap_err();
        assert_eq!(err, "CRC-32C checksum mismatch");
    }

    #[test]
    fn backward_compat_plain_bytes() {
        let mem = sample_memory();
        let plain_bytes = mem.to_bytes();
        // Plain bytes produced by to_bytes() have no version marker —
        // from_bytes() must still deserialize them correctly.
        let restored = Memory::from_bytes(&plain_bytes).unwrap();
        assert_eq!(mem, restored);
    }

    #[test]
    fn checksummed_with_embedding() {
        let mut mem = sample_memory();
        mem.embedding = Some(vec![0.42; 384]);
        let bytes = mem.to_bytes_checksummed();
        let restored = Memory::from_bytes(&bytes).unwrap();
        assert_eq!(mem, restored);
    }

    #[test]
    fn empty_context_returns_empty_map() {
        let mem = Memory {
            memory_id: vec![0u8; 16],
            content: "no context".to_string(),
            importance: 0.5,
            context_bytes: Vec::new(),
            entity_id: None,
            embedding: None,
            created_at: 0,
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 0,
            decay_score: 0.5,
            kind: MemoryKind::Episode,
            device_id: None,
            logical_clock: 0,
            associative_embedding: None,
        };
        assert!(mem.context().unwrap().is_empty());
    }
}
