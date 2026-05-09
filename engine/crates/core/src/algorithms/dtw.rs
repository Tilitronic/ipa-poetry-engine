//! `dtw.rs` — Dynamic Time Warping for comparing phoneme sequences of
//! potentially different lengths (supports imperfect / compound rhymes).
//!
//! The algorithm warps two sequences of feature vectors in "time" to find the
//! minimum-cost alignment, ignoring boundary tokens (WordBoundary / LineBreak).
//!
//! The returned score is normalised to [0, 1]:  0 = identical, 1 = maximally
//! different.

use ndarray::ArrayView1;

use crate::algorithms::distance::cosine_distance;
use crate::tokenizer::{PhoneticToken, TokenType};

// ────────────────────────────────────────────────────────────────────────────
// DTW core
// ────────────────────────────────────────────────────────────────────────────

/// Compute the normalised DTW distance between two feature-vector sequences.
///
/// Both slices must be non-empty.  Returns a value in [0, 1].
pub fn dtw_distance(
    seq_a: &[ArrayView1<f32>],
    seq_b: &[ArrayView1<f32>],
) -> f32 {
    let n = seq_a.len();
    let m = seq_b.len();

    if n == 0 || m == 0 {
        return 1.0; // nothing to compare
    }

    // Cost matrix; initialised to f32::MAX (∞).
    let mut cost = vec![vec![f32::MAX; m]; n];

    cost[0][0] = cosine_distance(seq_a[0], seq_b[0]);

    // First row
    for j in 1..m {
        cost[0][j] = cost[0][j - 1] + cosine_distance(seq_a[0], seq_b[j]);
    }
    // First column
    for i in 1..n {
        cost[i][0] = cost[i - 1][0] + cosine_distance(seq_a[i], seq_b[0]);
    }
    // Fill
    for i in 1..n {
        for j in 1..m {
            let local = cosine_distance(seq_a[i], seq_b[j]);
            let prev = cost[i - 1][j]
                .min(cost[i][j - 1])
                .min(cost[i - 1][j - 1]);
            cost[i][j] = local + prev;
        }
    }

    // Normalise by path length (n + m) to make score independent of sequence
    // length.
    let raw = cost[n - 1][m - 1];
    let norm = raw / (n + m) as f32;
    norm.clamp(0.0, 1.0)
}

// ────────────────────────────────────────────────────────────────────────────
// High-level helper: compare two token slices, skipping boundaries
// ────────────────────────────────────────────────────────────────────────────

/// Compare two sequences of `PhoneticToken`, ignoring boundary tokens.
///
/// Returns a normalised distance in [0, 1].  Lower = more similar (rhymes).
pub fn rhyme_distance(tokens_a: &[PhoneticToken], tokens_b: &[PhoneticToken]) -> f32 {
    let vecs_a: Vec<ArrayView1<f32>> = tokens_a
        .iter()
        .filter(|t| t.t_type == TokenType::Phoneme)
        .map(|t| t.features.view())
        .collect();

    let vecs_b: Vec<ArrayView1<f32>> = tokens_b
        .iter()
        .filter(|t| t.t_type == TokenType::Phoneme)
        .map(|t| t.features.view())
        .collect();

    dtw_distance(&vecs_a, &vecs_b)
}

/// Convert a normalised DTW distance to a similarity score in [0, 1].
#[inline]
pub fn dtw_similarity(distance: f32) -> f32 {
    1.0 - distance.clamp(0.0, 1.0)
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn v(vals: &[f32]) -> ndarray::Array1<f32> {
        ndarray::Array1::from(vals.to_vec())
    }

    // ── dtw_distance ─────────────────────────────────────────────────────

    #[test]
    fn test_identical_sequences_return_zero() {
        let a = v(&[1.0, -1.0, 0.0]);
        let seq = vec![a.view()];
        assert!((dtw_distance(&seq, &seq) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_single_element_sequences() {
        let a = v(&[1.0, -1.0, 1.0]);
        let b = v(&[-1.0, 1.0, -1.0]);
        let sa = vec![a.view()];
        let sb = vec![b.view()];
        let d = dtw_distance(&sa, &sb);
        // max cosine_distance possible is 1.0, normalised by (1+1)=2 → 0.5
        assert!((0.0..=0.5).contains(&d), "got {d}");
    }

    #[test]
    fn test_different_length_sequences() {
        let a = v(&[1.0, -1.0, 0.0]);
        let b = v(&[1.0, -1.0, 0.0]);
        let c = v(&[1.0, -1.0, 0.0]);
        let seq_long  = vec![a.view(), b.view(), c.view()];
        let seq_short = vec![a.view()];
        // Should not panic and should return a value in [0, 1]
        let d = dtw_distance(&seq_long, &seq_short);
        assert!((0.0..=1.0).contains(&d));
    }

    #[test]
    fn test_empty_sequence_a_returns_one() {
        let a = v(&[1.0]);
        let seq_a: Vec<ArrayView1<f32>> = vec![];
        let seq_b = vec![a.view()];
        assert_eq!(dtw_distance(&seq_a, &seq_b), 1.0);
    }

    #[test]
    fn test_empty_sequence_b_returns_one() {
        let a = v(&[1.0]);
        let seq_a = vec![a.view()];
        let seq_b: Vec<ArrayView1<f32>> = vec![];
        assert_eq!(dtw_distance(&seq_a, &seq_b), 1.0);
    }

    #[test]
    fn test_similar_sequences_lower_distance_than_dissimilar() {
        // Sequence "similar": same values
        let x = v(&[1.0, -1.0, 1.0, -1.0]);
        let y = v(&[1.0, -1.0, 1.0, -1.0]);
        // Sequence "dissimilar": inverted
        let z = v(&[-1.0, 1.0, -1.0, 1.0]);

        let similar_dist    = dtw_distance(&[x.view()], &[y.view()]);
        let dissimilar_dist = dtw_distance(&[x.view()], &[z.view()]);
        assert!(similar_dist < dissimilar_dist,
            "similar={similar_dist}, dissimilar={dissimilar_dist}");
    }

    #[test]
    fn test_output_in_zero_one_range() {
        let a = v(&[1.0, 0.0, -1.0]);
        let b = v(&[-1.0, 0.0, 1.0]);
        let d = dtw_distance(&[a.view(), a.view()], &[b.view()]);
        assert!((0.0..=1.0).contains(&d));
    }

    // ── dtw_similarity ───────────────────────────────────────────────────

    #[test]
    fn test_similarity_is_one_minus_distance() {
        let d = 0.3;
        assert!((dtw_similarity(d) - 0.7).abs() < 1e-6);
    }

    // ── rhyme_distance ───────────────────────────────────────────────────

    #[test]
    fn test_rhyme_distance_ignores_boundaries() {
        use crate::tokenizer::{PhoneticToken, TokenType};
        use ndarray::Array1;

        let phoneme = |sym: &str, feats: Vec<f32>| PhoneticToken {
            symbol:   sym.to_string(),
            t_type:   TokenType::Phoneme,
            features: Array1::from(feats),
            weight:   0.8,
        };
        let boundary = PhoneticToken {
            symbol:   " ".into(),
            t_type:   TokenType::WordBoundary,
            features: Array1::zeros(3),
            weight:   0.0,
        };

        let feats = vec![1.0_f32, -1.0, 1.0];
        let tok_a = phoneme("a", feats.clone());
        let tok_b = phoneme("b", feats.clone());

        // With boundary in the middle — should equal distance without it
        let seq_with_boundary    = vec![tok_a.clone(), boundary.clone(), tok_b.clone()];
        let seq_without_boundary = vec![tok_a.clone(), tok_b.clone()];

        let d_with    = rhyme_distance(&seq_with_boundary, &seq_without_boundary);
        let d_without = rhyme_distance(&seq_without_boundary, &seq_without_boundary);
        // Both should be 0 (identical phoneme content)
        assert!(d_with.abs() < 1e-5, "got {d_with}");
        assert!(d_without.abs() < 1e-5, "got {d_without}");
    }
}
