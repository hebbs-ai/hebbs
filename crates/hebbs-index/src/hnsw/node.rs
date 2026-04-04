use crate::error::{IndexError, Result};
use crate::hnsw::quantize::QuantizedVector;

/// In-memory representation of an HNSW graph node.
///
/// When quantization is enabled, the node stores an int8 quantized vector
/// for fast distance computation during graph traversal. The full-precision
/// f32 vector is stored only on disk (RocksDB Vectors CF) for reranking.
///
/// When quantization is disabled, the node stores the original f32 vector.
///
/// ## Memory layout per node (384-dim, quantized with rotation to 512-dim)
///
/// | Component | Bytes |
/// |-----------|-------|
/// | quantized vector (512 * 1 + 8) | 520 |
/// | neighbors layer 0 (M_max=32 * 16B) | 512 |
/// | neighbors upper layers (M=16 * 16B * ~0.36 avg) | ~92 |
/// | metadata (layer, tombstone, id) | ~24 |
/// | **Total** | **~1,148** |
///
/// Compare to f32 unquantized (384-dim): ~2,164 bytes. Approximately 1.9x reduction.
#[derive(Debug, Clone)]
pub struct HnswNode {
    pub memory_id: [u8; 16],
    /// Quantized vector for HNSW traversal (when quantization is enabled).
    /// When quantization is disabled, this is None and `vector` holds f32 data.
    pub quantized: Option<QuantizedVector>,
    /// Full-precision f32 vector (only present when quantization is disabled).
    pub vector: Option<Vec<f32>>,
    pub layer: u8,
    /// neighbors[l] contains the neighbor IDs at layer l.
    /// Length: layer + 1 (layers 0 through self.layer).
    pub neighbors: Vec<Vec<[u8; 16]>>,
    pub deleted: bool,
}

/// Serialization format version byte for quantized nodes.
const FORMAT_VERSION_QUANTIZED: u8 = 1;

/// Compact binary format for persisting HNSW nodes to the vectors CF.
///
/// Version 0 (legacy):
/// ```text
/// [layer: u8]
/// [vector: dimensions * 4 bytes, f32 little-endian]
/// [num_layers: u8]
/// [neighbor lists...]
/// ```
///
/// Version 1 (quantized):
/// ```text
/// [version: u8 = 1]
/// [layer: u8]
/// [f32_dims: u16 LE]
/// [f32 vector: f32_dims * 4 bytes]
/// [quant_dims: u16 LE]
/// [scale: f32 LE]
/// [zero_point: f32 LE]
/// [int8 data: quant_dims bytes]
/// [num_layers: u8]
/// [neighbor lists...]
/// ```
impl HnswNode {
    /// Serialize this node in legacy v0 format (f32 only, no version byte).
    ///
    /// Used when quantization is disabled.
    pub fn serialize_v0(&self) -> Vec<u8> {
        let vector = self
            .vector
            .as_ref()
            .expect("v0 serialize requires f32 vector");
        let neighbor_bytes: usize = self
            .neighbors
            .iter()
            .map(|layer| 2 + layer.len() * 16)
            .sum();
        let capacity = 1 + vector.len() * 4 + 1 + neighbor_bytes;
        let mut buf = Vec::with_capacity(capacity);

        buf.push(self.layer);

        for &val in vector {
            buf.extend_from_slice(&val.to_le_bytes());
        }

        self.serialize_neighbors(&mut buf);
        buf
    }

    /// Serialize this node in v1 format with both f32 and quantized data.
    ///
    /// The f32 vector is persisted for reranking and migration.
    /// The quantized vector is persisted for fast rebuild without re-quantization.
    pub fn serialize_v1(&self, f32_vector: &[f32]) -> Vec<u8> {
        let quantized = self
            .quantized
            .as_ref()
            .expect("v1 serialize requires quantized vector");
        let neighbor_bytes: usize = self
            .neighbors
            .iter()
            .map(|layer| 2 + layer.len() * 16)
            .sum();
        let f32_dims = f32_vector.len();
        let quant_dims = quantized.data.len();
        let capacity = 1 + 1 + 2 + f32_dims * 4 + 2 + 8 + quant_dims + 1 + neighbor_bytes;
        let mut buf = Vec::with_capacity(capacity);

        buf.push(FORMAT_VERSION_QUANTIZED);
        buf.push(self.layer);

        // f32 vector
        buf.extend_from_slice(&(f32_dims as u16).to_le_bytes());
        for &val in f32_vector {
            buf.extend_from_slice(&val.to_le_bytes());
        }

        // Quantized vector
        buf.extend_from_slice(&(quant_dims as u16).to_le_bytes());
        buf.extend_from_slice(&quantized.scale.to_le_bytes());
        buf.extend_from_slice(&quantized.zero_point.to_le_bytes());
        for &val in &quantized.data {
            buf.push(val as u8);
        }

        self.serialize_neighbors(&mut buf);
        buf
    }

    fn serialize_neighbors(&self, buf: &mut Vec<u8>) {
        let num_layers = self.neighbors.len() as u8;
        buf.push(num_layers);

        for layer_neighbors in &self.neighbors {
            let count = layer_neighbors.len() as u16;
            buf.extend_from_slice(&count.to_le_bytes());
            for neighbor_id in layer_neighbors {
                buf.extend_from_slice(neighbor_id);
            }
        }
    }

    /// Legacy serialize for backward compatibility (delegates to v0 or v1).
    pub fn serialize(&self) -> Vec<u8> {
        if self.quantized.is_some() {
            // When quantized, we need the f32 vector for persistence.
            // If we don't have it, fall back to v0 with dequantized data.
            if let Some(ref vector) = self.vector {
                self.serialize_v1(vector)
            } else {
                // Dequantize for persistence (lossy, only during migration)
                let f32_vec = crate::hnsw::quantize::dequantize(self.quantized.as_ref().unwrap());
                self.serialize_v1(&f32_vec)
            }
        } else {
            self.serialize_v0()
        }
    }

    /// Deserialize a node, auto-detecting format version.
    ///
    /// If the data starts with version byte 1, parse v1 format.
    /// Otherwise, assume legacy v0 format.
    pub fn deserialize(memory_id: [u8; 16], data: &[u8], dimensions: usize) -> Result<Self> {
        if data.is_empty() {
            return Err(IndexError::Serialization {
                message: "empty HNSW node data".to_string(),
            });
        }

        // Detect format: v1 starts with byte 0x01, v0 starts with the layer byte.
        // Since layer values are small (typically 0-6), and version 1 = 0x01,
        // we need a heuristic. Check if interpreting as v0 gives a valid size.
        //
        // v0 minimum size: 1 (layer) + dimensions*4 (vector) + 1 (num_layers)
        // v1 minimum size: 1 (version) + 1 (layer) + 2 (f32_dims) + f32_dims*4 + ...
        //
        // Reliable detection: try v1 first (check version byte), fall back to v0.
        if data[0] == FORMAT_VERSION_QUANTIZED {
            Self::deserialize_v1(memory_id, data)
        } else {
            Self::deserialize_v0(memory_id, data, dimensions)
        }
    }

    /// Deserialize legacy v0 format (f32 vector, no version byte).
    fn deserialize_v0(memory_id: [u8; 16], data: &[u8], dimensions: usize) -> Result<Self> {
        let vector_bytes = dimensions * 4;
        let min_size = 1 + vector_bytes + 1;
        if data.len() < min_size {
            return Err(IndexError::Serialization {
                message: format!(
                    "HNSW node data too short: {} bytes, need at least {}",
                    data.len(),
                    min_size
                ),
            });
        }

        let mut pos = 0;

        let layer = data[pos];
        pos += 1;

        let mut vector = Vec::with_capacity(dimensions);
        for _ in 0..dimensions {
            if pos + 4 > data.len() {
                return Err(IndexError::Serialization {
                    message: "truncated vector data in HNSW node".to_string(),
                });
            }
            let val = f32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            vector.push(val);
            pos += 4;
        }

        let neighbors = Self::deserialize_neighbors(data, &mut pos)?;

        Ok(HnswNode {
            memory_id,
            quantized: None,
            vector: Some(vector),
            layer,
            neighbors,
            deleted: false,
        })
    }

    /// Deserialize v1 format (f32 + quantized vectors).
    fn deserialize_v1(memory_id: [u8; 16], data: &[u8]) -> Result<Self> {
        if data.len() < 6 {
            return Err(IndexError::Serialization {
                message: "v1 HNSW node data too short".to_string(),
            });
        }

        let mut pos = 0;

        let _version = data[pos];
        pos += 1;

        let layer = data[pos];
        pos += 1;

        // f32 vector
        let f32_dims = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        let mut f32_vector = Vec::with_capacity(f32_dims);
        for _ in 0..f32_dims {
            if pos + 4 > data.len() {
                return Err(IndexError::Serialization {
                    message: "truncated f32 vector in v1 HNSW node".to_string(),
                });
            }
            let val = f32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            f32_vector.push(val);
            pos += 4;
        }

        // Quantized vector
        if pos + 2 > data.len() {
            return Err(IndexError::Serialization {
                message: "truncated quant dims in v1 HNSW node".to_string(),
            });
        }
        let quant_dims = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        if pos + 8 + quant_dims > data.len() {
            return Err(IndexError::Serialization {
                message: "truncated quantized data in v1 HNSW node".to_string(),
            });
        }
        let scale = f32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;
        let zero_point =
            f32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;

        let mut quant_data = Vec::with_capacity(quant_dims);
        for _ in 0..quant_dims {
            quant_data.push(data[pos] as i8);
            pos += 1;
        }

        let neighbors = Self::deserialize_neighbors(data, &mut pos)?;

        Ok(HnswNode {
            memory_id,
            quantized: Some(QuantizedVector {
                data: quant_data,
                scale,
                zero_point,
            }),
            vector: Some(f32_vector),
            layer,
            neighbors,
            deleted: false,
        })
    }

    fn deserialize_neighbors(data: &[u8], pos: &mut usize) -> Result<Vec<Vec<[u8; 16]>>> {
        if *pos >= data.len() {
            return Err(IndexError::Serialization {
                message: "missing num_layers in HNSW node".to_string(),
            });
        }
        let num_layers = data[*pos] as usize;
        *pos += 1;

        let mut neighbors = Vec::with_capacity(num_layers);
        for l in 0..num_layers {
            if *pos + 2 > data.len() {
                return Err(IndexError::Serialization {
                    message: format!("truncated neighbor count at layer {}", l),
                });
            }
            let count = u16::from_le_bytes([data[*pos], data[*pos + 1]]) as usize;
            *pos += 2;

            let needed = count * 16;
            if *pos + needed > data.len() {
                return Err(IndexError::Serialization {
                    message: format!(
                        "truncated neighbor IDs at layer {}: need {} bytes, have {}",
                        l,
                        needed,
                        data.len() - *pos
                    ),
                });
            }

            let mut layer_neighbors = Vec::with_capacity(count);
            for _ in 0..count {
                let mut id = [0u8; 16];
                id.copy_from_slice(&data[*pos..*pos + 16]);
                layer_neighbors.push(id);
                *pos += 16;
            }
            neighbors.push(layer_neighbors);
        }

        Ok(neighbors)
    }

    /// Extract only the f32 vector from serialized data (for reranking).
    ///
    /// Skips neighbor data for efficiency.
    pub fn deserialize_f32_only(data: &[u8], dimensions: usize) -> Result<Vec<f32>> {
        if data.is_empty() {
            return Err(IndexError::Serialization {
                message: "empty HNSW node data".to_string(),
            });
        }

        if data[0] == FORMAT_VERSION_QUANTIZED {
            // v1: skip version + layer, read f32_dims, read f32 vector
            if data.len() < 4 {
                return Err(IndexError::Serialization {
                    message: "v1 data too short for f32 extraction".to_string(),
                });
            }
            let f32_dims = u16::from_le_bytes([data[2], data[3]]) as usize;
            let start = 4;
            let end = start + f32_dims * 4;
            if end > data.len() {
                return Err(IndexError::Serialization {
                    message: "truncated f32 vector in v1 data".to_string(),
                });
            }
            let mut vector = Vec::with_capacity(f32_dims);
            for i in 0..f32_dims {
                let off = start + i * 4;
                let val =
                    f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
                vector.push(val);
            }
            Ok(vector)
        } else {
            // v0: skip layer byte, read dimensions * f32
            let start = 1;
            let end = start + dimensions * 4;
            if end > data.len() {
                return Err(IndexError::Serialization {
                    message: "v0 data too short for f32 extraction".to_string(),
                });
            }
            let mut vector = Vec::with_capacity(dimensions);
            for i in 0..dimensions {
                let off = start + i * 4;
                let val =
                    f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
                vector.push(val);
            }
            Ok(vector)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node_v0(dims: usize) -> HnswNode {
        let mut vector = Vec::with_capacity(dims);
        for i in 0..dims {
            vector.push(i as f32 * 0.001);
        }

        let id_a = [0xAA; 16];
        let id_b = [0xBB; 16];
        let id_c = [0xCC; 16];

        HnswNode {
            memory_id: [0x01; 16],
            quantized: None,
            vector: Some(vector),
            layer: 2,
            neighbors: vec![
                vec![id_a, id_b, id_c], // layer 0
                vec![id_a, id_b],       // layer 1
                vec![id_a],             // layer 2
            ],
            deleted: false,
        }
    }

    fn sample_node_v1(dims: usize) -> (HnswNode, Vec<f32>) {
        let mut f32_vector = Vec::with_capacity(dims);
        for i in 0..dims {
            f32_vector.push(i as f32 * 0.001);
        }

        let quantized = crate::hnsw::quantize::quantize(&f32_vector);

        let id_a = [0xAA; 16];
        let id_b = [0xBB; 16];

        let node = HnswNode {
            memory_id: [0x02; 16],
            quantized: Some(quantized),
            vector: Some(f32_vector.clone()),
            layer: 1,
            neighbors: vec![
                vec![id_a, id_b], // layer 0
                vec![id_a],       // layer 1
            ],
            deleted: false,
        };

        (node, f32_vector)
    }

    #[test]
    fn serialization_roundtrip_v0() {
        let node = sample_node_v0(384);
        let data = node.serialize_v0();
        let restored = HnswNode::deserialize(node.memory_id, &data, 384).unwrap();

        assert_eq!(restored.memory_id, node.memory_id);
        assert_eq!(restored.layer, node.layer);
        assert_eq!(restored.vector, node.vector);
        assert!(restored.quantized.is_none());
        assert_eq!(restored.neighbors.len(), node.neighbors.len());
        for (l, (orig, rest)) in node
            .neighbors
            .iter()
            .zip(restored.neighbors.iter())
            .enumerate()
        {
            assert_eq!(orig, rest, "neighbors differ at layer {}", l);
        }
    }

    #[test]
    fn serialization_roundtrip_v1() {
        let (node, f32_vec) = sample_node_v1(384);
        let data = node.serialize_v1(&f32_vec);
        let restored = HnswNode::deserialize(node.memory_id, &data, 384).unwrap();

        assert_eq!(restored.memory_id, node.memory_id);
        assert_eq!(restored.layer, node.layer);
        assert!(restored.quantized.is_some());
        assert!(restored.vector.is_some());

        let orig_q = node.quantized.as_ref().unwrap();
        let rest_q = restored.quantized.as_ref().unwrap();
        assert_eq!(orig_q.data, rest_q.data);
        assert_eq!(orig_q.scale.to_bits(), rest_q.scale.to_bits());
        assert_eq!(orig_q.zero_point.to_bits(), rest_q.zero_point.to_bits());

        assert_eq!(restored.vector.as_ref().unwrap(), &f32_vec);
        assert_eq!(restored.neighbors.len(), node.neighbors.len());
    }

    #[test]
    fn deserialize_f32_only_v0() {
        let node = sample_node_v0(10);
        let data = node.serialize_v0();
        let f32_vec = HnswNode::deserialize_f32_only(&data, 10).unwrap();
        assert_eq!(f32_vec, *node.vector.as_ref().unwrap());
    }

    #[test]
    fn deserialize_f32_only_v1() {
        let (node, f32_vec) = sample_node_v1(10);
        let data = node.serialize_v1(&f32_vec);
        let restored = HnswNode::deserialize_f32_only(&data, 10).unwrap();
        assert_eq!(restored, f32_vec);
    }

    #[test]
    fn serialization_roundtrip_empty_neighbors() {
        let node = HnswNode {
            memory_id: [0x42; 16],
            quantized: None,
            vector: Some(vec![1.0, 2.0, 3.0]),
            layer: 0,
            neighbors: vec![vec![]],
            deleted: false,
        };
        let data = node.serialize_v0();
        let restored = HnswNode::deserialize(node.memory_id, &data, 3).unwrap();
        assert_eq!(restored.neighbors, vec![Vec::<[u8; 16]>::new()]);
    }

    #[test]
    fn deserialize_truncated_data_fails() {
        let result = HnswNode::deserialize([0; 16], &[0x00], 384);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_wrong_dimensions_fails() {
        let node = HnswNode {
            memory_id: [0; 16],
            quantized: None,
            vector: Some(vec![1.0; 10]),
            layer: 0,
            neighbors: vec![vec![]],
            deleted: false,
        };
        let data = node.serialize_v0();
        let result = HnswNode::deserialize([0; 16], &data, 384);
        assert!(result.is_err());
    }
}
