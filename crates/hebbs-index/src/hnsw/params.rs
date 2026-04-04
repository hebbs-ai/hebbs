/// HNSW index configuration parameters.
///
/// These are set at construction time and cannot be changed without
/// rebuilding the index. See Phase 3 architecture document for rationale.
#[derive(Debug, Clone)]
pub struct HnswParams {
    /// Max connections per node per layer (except layer 0).
    /// Default: 16. Higher M = better recall, more memory, slower insert.
    pub m: usize,

    /// Max connections at layer 0. Default: 2 * M = 32.
    /// Layer 0 is the densest and most-queried layer.
    pub m_max: usize,

    /// Controls insert quality. Default: 200.
    /// Higher = better graph quality but slower inserts.
    pub ef_construction: usize,

    /// Controls search quality. Default: 100.
    /// Higher = better recall but slower queries.
    /// Can be overridden per-query in Phase 4.
    pub ef_search: usize,

    /// Vector dimensionality. Must match the embedding model output.
    pub dimensions: usize,

    /// Level multiplier: 1.0 / ln(M).
    /// Used for random layer assignment.
    pub ml: f64,

    /// Enable int8 scalar quantization for HNSW traversal vectors.
    /// Full-precision f32 vectors are kept on disk for reranking.
    /// Default: true.
    pub use_quantization: bool,

    /// Enable Walsh-Hadamard random rotation before quantization.
    /// Spreads energy across dimensions, reducing quantization error.
    /// Only effective when use_quantization is true.
    /// Default: true.
    pub use_rotation: bool,
}

impl HnswParams {
    /// Create parameters with sensible defaults for the given dimensionality.
    ///
    /// Defaults tuned for 384-dim vectors at up to 10M memories:
    /// - M = 16, M_max = 32
    /// - ef_construction = 200
    /// - ef_search = 100
    /// - ml = 1/ln(16) ≈ 0.3607
    pub fn new(dimensions: usize) -> Self {
        let m = 16;
        Self {
            m,
            m_max: 2 * m,
            ef_construction: 200,
            ef_search: 100,
            dimensions,
            ml: 1.0 / (m as f64).ln(),
            use_quantization: true,
            use_rotation: true,
        }
    }

    /// Create parameters with custom M value.
    pub fn with_m(dimensions: usize, m: usize) -> Self {
        Self {
            m,
            m_max: 2 * m,
            ef_construction: 200,
            ef_search: 100,
            dimensions,
            ml: 1.0 / (m as f64).ln(),
            use_quantization: true,
            use_rotation: true,
        }
    }

    /// Maximum number of neighbors allowed at a given layer.
    /// Layer 0 uses `m_max`, higher layers use `m`.
    #[inline]
    pub fn max_neighbors(&self, layer: usize) -> usize {
        if layer == 0 {
            self.m_max
        } else {
            self.m
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_params() {
        let params = HnswParams::new(384);
        assert_eq!(params.m, 16);
        assert_eq!(params.m_max, 32);
        assert_eq!(params.ef_construction, 200);
        assert_eq!(params.ef_search, 100);
        assert_eq!(params.dimensions, 384);
        assert!((params.ml - 1.0 / 16.0f64.ln()).abs() < 1e-10);
    }

    #[test]
    fn max_neighbors_layer_0() {
        let params = HnswParams::new(384);
        assert_eq!(params.max_neighbors(0), 32);
    }

    #[test]
    fn max_neighbors_higher_layers() {
        let params = HnswParams::new(384);
        assert_eq!(params.max_neighbors(1), 16);
        assert_eq!(params.max_neighbors(5), 16);
    }
}
