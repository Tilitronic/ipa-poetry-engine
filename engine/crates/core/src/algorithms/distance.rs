//! `distance.rs` — pairwise phonological distance metrics.
//!
//! Two metrics are provided:
//! - **Cosine similarity** — angle-based, range [-1, 1], 1 = identical.
//! - **Hamming distance** — count of differing (non-zero, non-matching)
//!   feature dimensions, normalised to [0, 1].

use ndarray::ArrayView1;

// ────────────────────────────────────────────────────────────────────────────
// Cosine similarity
// ────────────────────────────────────────────────────────────────────────────

/// Compute cosine similarity between two feature vectors.
///
/// Returns `0.0` when either vector is the zero vector (e.g. a boundary token).
pub fn cosine_similarity(a: ArrayView1<f32>, b: ArrayView1<f32>) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "vectors must have equal length");

    let dot: f32    = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

/// Convert cosine similarity to a distance in [0, 1].
/// distance = (1 - similarity) / 2
#[inline]
pub fn cosine_distance(a: ArrayView1<f32>, b: ArrayView1<f32>) -> f32 {
    (1.0 - cosine_similarity(a, b)) / 2.0
}

// ────────────────────────────────────────────────────────────────────────────
// Hamming distance (normalised)
// ────────────────────────────────────────────────────────────────────────────

/// Normalised Hamming distance between two feature vectors.
///
/// Positions where **both** values are `0.0` (unspecified) are treated as
/// equal (not a difference).  Returns a value in [0, 1].
pub fn hamming_distance(a: ArrayView1<f32>, b: ArrayView1<f32>) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let n = a.len() as f32;
    if n == 0.0 {
        return 0.0;
    }
    let diff_count = a.iter()
        .zip(b.iter())
        .filter(|(&x, &y)| (x - y).abs() > 1e-6)
        .count() as f32;
    diff_count / n
}

// ────────────────────────────────────────────────────────────────────────────
// Weighted Euclidean distance
// ────────────────────────────────────────────────────────────────────────────

/// Default perceptual weights for the 24 Panphon features (FEATURE_NAMES order).
///
/// Higher weight = more perceptually salient when comparing phoneme pairs.
/// Order: syl son cons cont delrel lat nas strid voi sg cg ant cor distr lab
///        hi lo back round velaric tense long hitone hireg
pub const DEFAULT_FEATURE_WEIGHTS: [f32; 24] = [
    2.0, // syl     — syllabic (fundamental consonant/vowel split)
    1.5, // son     — sonorant
    1.5, // cons    — consonantal
    1.0, // cont    — continuant
    0.8, // delrel  — delayed release
    1.2, // lat     — lateral
    2.0, // nas     — nasal (very salient perceptually)
    2.0, // strid   — strident / sibilant (very salient)
    1.8, // voi     — voiced
    1.0, // sg      — spread glottis
    1.0, // cg      — constricted glottis
    1.0, // ant     — anterior
    1.2, // cor     — coronal
    0.7, // distr   — distributed (subtle)
    1.2, // lab     — labial
    1.2, // hi      — high
    1.2, // lo      — low
    1.0, // back    — back
    1.2, // round   — round
    0.8, // velaric — velaric airstream
    0.8, // tense   — tense
    0.8, // long    — long
    0.5, // hitone  — high tone (less relevant for non-tonal languages)
    0.5, // hireg   — high register
];

/// Weighted Euclidean distance between two feature vectors.
///
/// `weights` must have the same length as `a` and `b`.
/// Returns an unnormalised distance ≥ 0.
pub fn weighted_euclidean_distance(
    a: ArrayView1<f32>,
    b: ArrayView1<f32>,
    weights: &[f32],
) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "vectors must have equal length");
    debug_assert_eq!(a.len(), weights.len(), "weights must match vector length");

    a.iter()
        .zip(b.iter())
        .zip(weights.iter())
        .map(|((&x, &y), &w)| w * (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

/// Weighted Euclidean distance using [`DEFAULT_FEATURE_WEIGHTS`].
#[inline]
pub fn weighted_euclidean_distance_default(
    a: ArrayView1<f32>,
    b: ArrayView1<f32>,
) -> f32 {
    weighted_euclidean_distance(a, b, &DEFAULT_FEATURE_WEIGHTS)
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    // ── Cosine similarity ────────────────────────────────────────────────

    #[test]
    fn test_cosine_identical_vectors_is_one() {
        let v = array![1.0_f32, -1.0, 0.0, 1.0];
        let sim = cosine_similarity(v.view(), v.view());
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_opposite_vectors_is_minus_one() {
        let a = array![1.0_f32, 1.0];
        let b = array![-1.0_f32, -1.0];
        let sim = cosine_similarity(a.view(), b.view());
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_orthogonal_vectors_is_zero() {
        let a = array![1.0_f32, 0.0];
        let b = array![0.0_f32, 1.0];
        let sim = cosine_similarity(a.view(), b.view());
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_zero_vector_returns_zero() {
        let zero = array![0.0_f32, 0.0, 0.0];
        let v    = array![1.0_f32, -1.0, 1.0];
        assert_eq!(cosine_similarity(zero.view(), v.view()), 0.0);
        assert_eq!(cosine_similarity(v.view(), zero.view()), 0.0);
    }

    #[test]
    fn test_cosine_p_and_b_are_very_similar() {
        // p and b differ only in voice (+1 vs -1 at one dimension).
        // All other 23 dimensions identical → high cosine.
        let mut p = vec![-1.0_f32; 24];
        p[2]  =  1.0; // cons +
        p[11] =  1.0; // ant  +
        p[14] =  1.0; // lab  +
        let mut b = p.clone();
        b[8] = 1.0; // voi + (was -1.0 for p)

        let pa = ndarray::Array1::from(p);
        let ba = ndarray::Array1::from(b);
        let sim = cosine_similarity(pa.view(), ba.view());
        assert!(sim > 0.85, "p and b should be very similar, got {sim}");
    }

    // ── Cosine distance ──────────────────────────────────────────────────

    #[test]
    fn test_cosine_distance_identical_is_zero() {
        let v = array![1.0_f32, -1.0, 1.0];
        assert!((cosine_distance(v.view(), v.view())).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_distance_in_zero_one_range() {
        let a = array![1.0_f32, -1.0, 0.0];
        let b = array![-1.0_f32, 0.0, 1.0];
        let d = cosine_distance(a.view(), b.view());
        assert!((0.0..=1.0).contains(&d));
    }

    // ── Hamming distance ─────────────────────────────────────────────────

    #[test]
    fn test_hamming_identical_vectors_is_zero() {
        let v = array![1.0_f32, -1.0, 0.0];
        assert_eq!(hamming_distance(v.view(), v.view()), 0.0);
    }

    #[test]
    fn test_hamming_all_different_is_one() {
        let a = array![1.0_f32,  1.0, 1.0];
        let b = array![-1.0_f32, -1.0, -1.0];
        assert!((hamming_distance(a.view(), b.view()) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_hamming_half_different_is_half() {
        let a = array![1.0_f32, 1.0, -1.0, -1.0];
        let b = array![1.0_f32, 1.0,  1.0,  1.0];
        let d = hamming_distance(a.view(), b.view());
        assert!((d - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_hamming_both_zero_treated_as_equal() {
        let a = array![0.0_f32, 1.0];
        let b = array![0.0_f32, 1.0];
        assert_eq!(hamming_distance(a.view(), b.view()), 0.0);
    }

    // ── Weighted Euclidean ───────────────────────────────────────────────

    #[test]
    fn test_weighted_euclidean_identical_is_zero() {
        let a = array![1.0_f32, -1.0, 0.0, 1.0];
        let weights = [1.0_f32; 4];
        assert_eq!(weighted_euclidean_distance(a.view(), a.view(), &weights), 0.0);
    }

    #[test]
    fn test_weighted_euclidean_symmetric() {
        let a = array![1.0_f32, 0.0, -1.0];
        let b = array![-1.0_f32, 0.0, 1.0];
        let w = [1.0_f32, 2.0, 3.0];
        let d_ab = weighted_euclidean_distance(a.view(), b.view(), &w);
        let d_ba = weighted_euclidean_distance(b.view(), a.view(), &w);
        assert!((d_ab - d_ba).abs() < 1e-6);
    }

    #[test]
    fn test_weight_scales_distance() {
        // difference only in dimension 0
        let a = array![1.0_f32, 0.0];
        let b = array![-1.0_f32, 0.0];
        let w1 = [1.0_f32, 1.0];
        let w2 = [4.0_f32, 1.0]; // 4× weight on differing dim
        let d1 = weighted_euclidean_distance(a.view(), b.view(), &w1);
        let d2 = weighted_euclidean_distance(a.view(), b.view(), &w2);
        // d2 should be exactly 4× * d1 (sqrt(4*4) vs sqrt(1*4))
        assert!((d2 - 2.0 * d1).abs() < 1e-5, "d1={d1} d2={d2}");
    }

    #[test]
    fn test_default_weights_has_24_elements() {
        assert_eq!(DEFAULT_FEATURE_WEIGHTS.len(), 24);
    }

    #[test]
    fn test_weighted_euclidean_default_differs_from_uniform() {
        // Two vectors that differ only on the `nas` (index 6) dimension.
        // Default weight for nas is 2.0 vs uniform 1.0 — result should differ.
        let mut a = [0.0_f32; 24];
        let mut b = [0.0_f32; 24];
        a[6] = 1.0;  // nas = +1
        b[6] = -1.0; // nas = -1
        let a_arr = ndarray::Array1::from(a.to_vec());
        let b_arr = ndarray::Array1::from(b.to_vec());
        let uniform = [1.0_f32; 24];
        let d_uniform = weighted_euclidean_distance(a_arr.view(), b_arr.view(), &uniform);
        let d_default = weighted_euclidean_distance_default(a_arr.view(), b_arr.view());
        assert!(d_default > d_uniform, "default weight for nas should increase distance");
    }
}
