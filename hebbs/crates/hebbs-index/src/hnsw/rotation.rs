//! Walsh-Hadamard random rotation for quantization quality improvement.
//!
//! Embedding vectors often have uneven energy distribution across dimensions.
//! Quantizing directly loses more information in high-energy dimensions.
//! Random rotation spreads energy uniformly, making scalar quantization
//! near-optimal for any input distribution (data-oblivious).
//!
//! The rotation R = H * D where:
//! - D is a diagonal matrix with random +1/-1 entries (deterministic from seed)
//! - H is the Walsh-Hadamard matrix of size next_power_of_2(d)
//!
//! Since H is orthogonal, inner products are preserved: <Rv, Rw> = <v, w>.
//! Vectors are padded to the next power of 2 before rotation.
//!
//! Complexity: O(d log d) per rotation (vs O(d^2) for dense random matrix).

/// Randomized Hadamard rotation matrix, generated deterministically from a seed.
///
/// Pads vectors to `padded_dim` (next power of 2 >= original_dim),
/// applies random sign flips, then the Walsh-Hadamard transform.
#[derive(Debug, Clone)]
pub struct HadamardRotation {
    /// Random +1.0 or -1.0 per padded dimension.
    signs: Vec<f32>,
    /// Next power of 2 >= original_dim.
    pub padded_dim: usize,
    /// Original embedding dimensionality.
    pub original_dim: usize,
}

impl HadamardRotation {
    /// Create a new rotation for the given dimensionality, deterministic from seed.
    ///
    /// The seed should be stored persistently (Meta CF) so all vectors in an
    /// index use the same rotation.
    pub fn new(original_dim: usize, seed: u64) -> Self {
        let padded_dim = original_dim.next_power_of_two();

        use rand::Rng;
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let signs: Vec<f32> = (0..padded_dim)
            .map(|_| if rng.gen_bool(0.5) { 1.0 } else { -1.0 })
            .collect();

        Self {
            signs,
            padded_dim,
            original_dim,
        }
    }

    /// Rotate an f32 vector: pad to power-of-2, apply random signs, apply WHT.
    ///
    /// The output has `padded_dim` dimensions. L2 norm is scaled by sqrt(padded_dim)
    /// but this is consistent across all vectors so distance ordering is preserved.
    ///
    /// Complexity: O(d log d) where d = padded_dim.
    pub fn rotate(&self, v: &[f32]) -> Vec<f32> {
        debug_assert_eq!(
            v.len(),
            self.original_dim,
            "input must have original_dim={} dimensions, got {}",
            self.original_dim,
            v.len()
        );

        let mut buf = vec![0.0f32; self.padded_dim];
        // Copy and apply random sign flips (padded dims stay 0.0)
        for i in 0..self.original_dim {
            buf[i] = v[i] * self.signs[i];
        }

        walsh_hadamard_transform(&mut buf);

        // Normalize by 1/sqrt(padded_dim) to keep values in a reasonable range
        // for quantization. This preserves distance ordering.
        let norm_factor = 1.0 / (self.padded_dim as f32).sqrt();
        for x in &mut buf {
            *x *= norm_factor;
        }

        buf
    }
}

/// In-place Walsh-Hadamard transform (butterfly algorithm).
///
/// Input length must be a power of 2.
///
/// The WHT is its own inverse (up to scaling by n):
///   WHT(WHT(v)) = n * v
///
/// Complexity: O(d log d) with d/2 * log2(d) additions.
pub fn walsh_hadamard_transform(v: &mut [f32]) {
    let n = v.len();
    debug_assert!(n.is_power_of_two(), "WHT requires power-of-2 length");

    let mut h = 1;
    while h < n {
        for i in (0..n).step_by(h * 2) {
            for j in i..i + h {
                let x = v[j];
                let y = v[j + h];
                v[j] = x + y;
                v[j + h] = x - y;
            }
        }
        h *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wht_known_input() {
        // WHT of [1, 0, 0, 0] should be [1, 1, 1, 1]
        let mut v = vec![1.0, 0.0, 0.0, 0.0];
        walsh_hadamard_transform(&mut v);
        assert_eq!(v, vec![1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn wht_self_inverse() {
        // WHT(WHT(v)) = n * v
        let original = vec![1.0, 2.0, 3.0, 4.0];
        let mut v = original.clone();
        walsh_hadamard_transform(&mut v);
        walsh_hadamard_transform(&mut v);
        let n = original.len() as f32;
        for (a, b) in v.iter().zip(original.iter()) {
            assert!(
                (a - b * n).abs() < 1e-6,
                "WHT is not self-inverse: {a} != {} * {n}",
                b
            );
        }
    }

    #[test]
    fn rotation_preserves_inner_product() {
        use rand::Rng;
        use rand::SeedableRng;

        let dims = 384;
        let rotation = HadamardRotation::new(dims, 42);

        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let a: Vec<f32> = (0..dims).map(|_| rng.gen_range(-1.0..1.0)).collect();
        let b: Vec<f32> = (0..dims).map(|_| rng.gen_range(-1.0..1.0)).collect();

        let original_ip: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();

        let ra = rotation.rotate(&a);
        let rb = rotation.rotate(&b);
        let rotated_ip: f32 = ra.iter().zip(rb.iter()).map(|(x, y)| x * y).sum();

        let error = (original_ip - rotated_ip).abs();
        assert!(
            error < 1e-3,
            "rotation changed inner product by {error} (original={original_ip}, rotated={rotated_ip})"
        );
    }

    #[test]
    fn rotation_deterministic() {
        let dims = 384;
        let r1 = HadamardRotation::new(dims, 42);
        let r2 = HadamardRotation::new(dims, 42);

        let v = vec![1.0; dims];
        let a = r1.rotate(&v);
        let b = r2.rotate(&v);

        assert_eq!(a, b, "same seed must produce identical rotations");
    }

    #[test]
    fn rotation_spreads_energy() {
        // A vector with energy concentrated in one dimension should have
        // energy spread more evenly after rotation.
        let dims = 384;
        let rotation = HadamardRotation::new(dims, 42);

        let mut v = vec![0.0; dims];
        v[0] = 1.0; // all energy in first dimension

        let rotated = rotation.rotate(&v);

        // After rotation, energy should be spread across dimensions.
        // Variance of rotated values should be much lower than original.
        let mean: f32 = rotated.iter().sum::<f32>() / rotated.len() as f32;
        let variance: f32 =
            rotated.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / rotated.len() as f32;

        // Original variance of [1, 0, 0, ..., 0] padded to 512:
        // mean = 1/512, variance ~ 1/512 ~ 0.00195
        // Rotated should have similar or lower variance
        assert!(
            variance < 0.01,
            "energy not spread after rotation: variance = {variance}"
        );
    }

    #[test]
    fn padded_dim_is_power_of_two() {
        assert_eq!(HadamardRotation::new(384, 0).padded_dim, 512);
        assert_eq!(HadamardRotation::new(768, 0).padded_dim, 1024);
        assert_eq!(HadamardRotation::new(1536, 0).padded_dim, 2048);
        assert_eq!(HadamardRotation::new(256, 0).padded_dim, 256);
    }
}
