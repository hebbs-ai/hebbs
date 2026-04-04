//! Scalar int8 quantization for HNSW vectors.
//!
//! Per-vector affine quantization maps f32 values to [-128, 127]:
//!   q[i] = clamp(round((v[i] - zero_point) / scale), -128, 127)
//!   v_approx[i] = q[i] as f32 * scale + zero_point
//!
//! Memory per 384-dim vector: 384 bytes (int8) + 8 bytes (scale + zero_point)
//! = 392 bytes vs 1,536 bytes for f32. Approximately 3.9x reduction.
//!
//! With Walsh-Hadamard rotation (padded to 512-dim): 512 + 8 = 520 bytes,
//! still ~3x reduction.

/// Quantized representation of a vector using int8 scalar quantization.
///
/// Stores per-vector scale and zero_point for dequantization.
/// The quantization is affine: `original ~= data[i] * scale + zero_point`.
#[derive(Debug, Clone)]
pub struct QuantizedVector {
    /// Int8 quantized values, one per dimension.
    pub data: Vec<i8>,
    /// Scale factor: (max - min) / 255.
    pub scale: f32,
    /// Zero point: min value of the original vector.
    pub zero_point: f32,
}

/// Quantize an f32 vector to int8 using per-vector min/max scaling.
///
/// Maps the value range [min, max] to [-128, 127].
///
/// Complexity: O(d) where d = vector dimensionality.
pub fn quantize(v: &[f32]) -> QuantizedVector {
    if v.is_empty() {
        return QuantizedVector {
            data: Vec::new(),
            scale: 0.0,
            zero_point: 0.0,
        };
    }

    let mut min_val = f32::INFINITY;
    let mut max_val = f32::NEG_INFINITY;
    for &x in v {
        if x < min_val {
            min_val = x;
        }
        if x > max_val {
            max_val = x;
        }
    }

    let range = max_val - min_val;
    let scale = if range > 0.0 { range / 255.0 } else { 1.0 };
    let zero_point = min_val;

    let data: Vec<i8> = v
        .iter()
        .map(|&x| {
            let q = ((x - zero_point) / scale) - 128.0;
            q.round().clamp(-128.0, 127.0) as i8
        })
        .collect();

    QuantizedVector {
        data,
        scale,
        zero_point,
    }
}

/// Dequantize an int8 vector back to approximate f32 values.
///
/// Complexity: O(d).
pub fn dequantize(q: &QuantizedVector) -> Vec<f32> {
    q.data
        .iter()
        .map(|&x| (x as f32 + 128.0) * q.scale + q.zero_point)
        .collect()
}

/// Compute inner product distance between two quantized vectors.
///
/// Uses dequantize-on-the-fly: each int8 value is converted to f32 before
/// the multiply-accumulate. This prioritizes correctness over speed;
/// SIMD-optimized int8 dot product is a future optimization (Phase D).
///
/// distance = 1.0 - sum(dequant(a[i]) * dequant(b[i]))
///
/// Complexity: O(d).
#[inline]
pub fn quantized_inner_product_distance(a: &QuantizedVector, b: &QuantizedVector) -> f32 {
    debug_assert_eq!(
        a.data.len(),
        b.data.len(),
        "quantized vectors must have equal dimensionality"
    );
    let mut sum = 0.0f32;
    for i in 0..a.data.len() {
        let a_val = (a.data[i] as f32 + 128.0) * a.scale + a.zero_point;
        let b_val = (b.data[i] as f32 + 128.0) * b.scale + b.zero_point;
        sum += a_val * b_val;
    }
    1.0 - sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalized_vec(dims: usize, seed: u64) -> Vec<f32> {
        use rand::Rng;
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut v: Vec<f32> = (0..dims).map(|_| rng.gen_range(-1.0..1.0)).collect();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }

    #[test]
    fn roundtrip_error_bounded() {
        let v = normalized_vec(384, 42);
        let q = quantize(&v);
        let restored = dequantize(&q);

        let max_error: f32 = v
            .iter()
            .zip(restored.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);

        // For L2-normalized vectors in [-1, 1], scale ~= 2/255 ~= 0.0078
        // Max quantization error should be <= scale/2 ~= 0.004
        assert!(
            max_error < 0.01,
            "max roundtrip error {max_error} exceeds 0.01"
        );
    }

    #[test]
    fn distance_preservation() {
        let a = normalized_vec(384, 1);
        let b = normalized_vec(384, 2);

        let exact_dist = crate::hnsw::distance::inner_product_distance(&a, &b);
        let qa = quantize(&a);
        let qb = quantize(&b);
        let approx_dist = quantized_inner_product_distance(&qa, &qb);

        let relative_error = (exact_dist - approx_dist).abs() / exact_dist.max(0.001);
        assert!(
            relative_error < 0.05,
            "relative distance error {relative_error:.4} exceeds 5% (exact={exact_dist:.4}, approx={approx_dist:.4})"
        );
    }

    #[test]
    fn identical_vectors_near_zero_distance() {
        let v = normalized_vec(384, 99);
        let q = quantize(&v);
        let dist = quantized_inner_product_distance(&q, &q);
        assert!(dist.abs() < 0.05, "self-distance {dist} should be near 0");
    }

    #[test]
    fn empty_vector() {
        let q = quantize(&[]);
        assert!(q.data.is_empty());
        assert_eq!(dequantize(&q).len(), 0);
    }

    #[test]
    fn constant_vector() {
        let v = vec![0.5; 10];
        let q = quantize(&v);
        let restored = dequantize(&q);
        for &x in &restored {
            assert!((x - 0.5).abs() < 0.01, "constant value not preserved: {x}");
        }
    }
}
