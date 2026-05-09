//! `density.rs` — Sliding-window phonological density analysis.
//!
//! Used to detect alliterations, assonance clusters and other "sound patches"
//! within an IPA stream.
//!
//! The window sums the feature vectors of all phoneme tokens inside it.
//! By inspecting specific feature dimensions in the summed vector, the caller
//! can detect which phonological property dominates a region of the poem.

use crate::tokenizer::{PhoneticToken, TokenType};

// Feature index constants (must match FEATURE_NAMES order in registry.rs)
pub const IDX_SYL:    usize = 0;  // syllabic  (vowels)
pub const IDX_DELREL: usize = 4;  // delayed release (affricates: t͡ʃ, d͡ʒ, d͡z …)
pub const IDX_LAT:    usize = 5;  // lateral
pub const IDX_NAS:    usize = 6;  // nasal
pub const IDX_STRID:  usize = 7;  // strident  (sibilants + strident affricates)
pub const IDX_VOI:    usize = 8;  // voice

// ────────────────────────────────────────────────────────────────────────────
// DensityWindow result
// ────────────────────────────────────────────────────────────────────────────

/// Result of a single window position.
#[derive(Debug, Clone)]
pub struct WindowResult {
    /// Index of the first token in this window.
    pub start: usize,
    /// Index one past the last token.
    pub end: usize,
    /// Summed feature vector over all *phoneme* tokens in the window.
    /// Length equals number of features (24).
    pub density: Vec<f32>,
}

impl WindowResult {
    /// Return the density value at a specific feature index.
    #[inline]
    pub fn feature(&self, idx: usize) -> f32 {
        self.density.get(idx).copied().unwrap_or(0.0)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Sliding window
// ────────────────────────────────────────────────────────────────────────────

/// Run a sliding window of `window_size` tokens over `tokens`.
///
/// Returns one `WindowResult` per window position.  Boundary tokens
/// (WordBoundary, LineBreak) contribute **zero** to the density sum but
/// advance the window position as normal.
pub fn sliding_window(tokens: &[PhoneticToken], window_size: usize) -> Vec<WindowResult> {
    if window_size == 0 || tokens.len() < window_size {
        return vec![];
    }

    let n_features = tokens
        .first()
        .map(|t| t.features.len())
        .unwrap_or(0);

    // Pre-compute initial window sum
    let mut density = vec![0.0_f32; n_features];
    for tok in &tokens[..window_size] {
        if tok.t_type == TokenType::Phoneme {
            for (d, f) in density.iter_mut().zip(tok.features.iter()) {
                *d += f;
            }
        }
    }

    let mut results = Vec::with_capacity(tokens.len() - window_size + 1);

    for start in 0..=(tokens.len() - window_size) {
        results.push(WindowResult {
            start,
            end: start + window_size,
            density: density.clone(),
        });

        // Slide: remove token at `start`, add token at `start + window_size`
        if start + window_size < tokens.len() {
            let leaving = &tokens[start];
            let entering = &tokens[start + window_size];

            if leaving.t_type == TokenType::Phoneme {
                for (d, f) in density.iter_mut().zip(leaving.features.iter()) {
                    *d -= f;
                }
            }
            if entering.t_type == TokenType::Phoneme {
                for (d, f) in density.iter_mut().zip(entering.features.iter()) {
                    *d += f;
                }
            }
        }
    }

    results
}

// ────────────────────────────────────────────────────────────────────────────
// Alliteration / assonance detection
// ────────────────────────────────────────────────────────────────────────────

/// A detected sound cluster.
#[derive(Debug, Clone)]
pub struct SoundCluster {
    /// Start token index of the window with peak density.
    pub start: usize,
    /// End token index (exclusive).
    pub end: usize,
    /// The feature dimension that triggered detection.
    pub feature_idx: usize,
    /// Peak density value at that feature.
    pub peak_value: f32,
}

/// Find windows where `feature_idx` density exceeds `threshold`.
///
/// Overlapping windows with the same peak are merged into a single cluster
/// by keeping the span with the maximum value.
pub fn find_clusters(
    windows: &[WindowResult],
    feature_idx: usize,
    threshold: f32,
) -> Vec<SoundCluster> {
    let mut clusters: Vec<SoundCluster> = Vec::new();

    for w in windows {
        let val = w.feature(feature_idx);
        if val >= threshold {
            if let Some(last) = clusters.last_mut() {
                // Merge if windows overlap
                if w.start < last.end {
                    last.end = w.end;
                    if val > last.peak_value {
                        last.peak_value = val;
                    }
                    continue;
                }
            }
            clusters.push(SoundCluster {
                start: w.start,
                end: w.end,
                feature_idx,
                peak_value: val,
            });
        }
    }

    clusters
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::{PhoneticToken, TokenType};
    use ndarray::Array1;

    // ── Helpers ──────────────────────────────────────────────────────────

    fn phoneme(symbol: &str, features: Vec<f32>) -> PhoneticToken {
        PhoneticToken {
            symbol:   symbol.to_string(),
            t_type:   TokenType::Phoneme,
            features: Array1::from(features),
            weight:   0.8,
        }
    }

    fn boundary() -> PhoneticToken {
        PhoneticToken {
            symbol:   " ".into(),
            t_type:   TokenType::WordBoundary,
            features: Array1::zeros(24),
            weight:   0.0,
        }
    }

    /// Build a feature vector with +1.0 at index `active` and 0.0 elsewhere.
    fn feat_at(active: usize) -> Vec<f32> {
        let mut v = vec![0.0_f32; 24];
        v[active] = 1.0;
        v
    }

    // ── sliding_window ───────────────────────────────────────────────────

    #[test]
    fn test_window_count_with_window_size_1() {
        let tokens = vec![phoneme("a", feat_at(0)), phoneme("b", feat_at(1))];
        let results = sliding_window(&tokens, 1);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_window_count_with_window_size_2() {
        let tokens = vec![
            phoneme("a", feat_at(0)),
            phoneme("b", feat_at(1)),
            phoneme("c", feat_at(2)),
        ];
        let results = sliding_window(&tokens, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_window_larger_than_tokens_returns_empty() {
        let tokens = vec![phoneme("a", feat_at(0))];
        let results = sliding_window(&tokens, 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_zero_window_size_returns_empty() {
        let tokens = vec![phoneme("a", feat_at(0))];
        let results = sliding_window(&tokens, 0);
        assert!(results.is_empty());
    }

    #[test]
    fn test_density_sums_phoneme_features() {
        // Two phonemes each contributing 1.0 at index 7 (strid)
        let tok_a = phoneme("s", {
            let mut v = vec![0.0_f32; 24];
            v[IDX_STRID] = 1.0;
            v
        });
        let tok_b = phoneme("ʃ", {
            let mut v = vec![0.0_f32; 24];
            v[IDX_STRID] = 1.0;
            v
        });
        let tokens = vec![tok_a, tok_b];
        let results = sliding_window(&tokens, 2);
        assert_eq!(results[0].feature(IDX_STRID), 2.0);
    }

    #[test]
    fn test_boundary_does_not_contribute_to_density() {
        let tokens = vec![
            phoneme("s", { let mut v = vec![0.0_f32; 24]; v[IDX_STRID] = 1.0; v }),
            boundary(),
        ];
        let results = sliding_window(&tokens, 2);
        // boundary should contribute 0
        assert_eq!(results[0].feature(IDX_STRID), 1.0);
    }

    #[test]
    fn test_sliding_window_advances_correctly() {
        // Window size 2 over [s, ʃ, a]:
        // window[0] = {s, ʃ} → strid density = 2.0
        // window[1] = {ʃ, a} → strid density = 1.0 (a has strid=0)
        let strid_feat = || { let mut v = vec![0.0_f32; 24]; v[IDX_STRID] = 1.0; v };
        let no_strid   = || vec![0.0_f32; 24];

        let tokens = vec![
            phoneme("s", strid_feat()),
            phoneme("ʃ", strid_feat()),
            phoneme("a", no_strid()),
        ];
        let results = sliding_window(&tokens, 2);
        assert_eq!(results[0].feature(IDX_STRID), 2.0);
        assert_eq!(results[1].feature(IDX_STRID), 1.0);
    }

    #[test]
    fn test_window_start_and_end_indices() {
        let tokens = vec![phoneme("a", feat_at(0)), phoneme("b", feat_at(1)), phoneme("c", feat_at(2))];
        let results = sliding_window(&tokens, 2);
        assert_eq!(results[0].start, 0);
        assert_eq!(results[0].end,   2);
        assert_eq!(results[1].start, 1);
        assert_eq!(results[1].end,   3);
    }

    // ── find_clusters ────────────────────────────────────────────────────

    #[test]
    fn test_no_clusters_when_below_threshold() {
        let strid = || { let mut v = vec![0.0_f32; 24]; v[IDX_STRID] = 0.5; v };
        let tokens: Vec<_> = (0..5).map(|_| phoneme("s", strid())).collect();
        let windows = sliding_window(&tokens, 3);
        let clusters = find_clusters(&windows, IDX_STRID, 10.0); // threshold too high
        assert!(clusters.is_empty());
    }

    #[test]
    fn test_cluster_detected_above_threshold() {
        let strid = || { let mut v = vec![0.0_f32; 24]; v[IDX_STRID] = 1.0; v };
        let tokens: Vec<_> = (0..5).map(|_| phoneme("s", strid())).collect();
        let windows = sliding_window(&tokens, 3);
        // Window density = 3.0 per window; threshold = 2.5
        let clusters = find_clusters(&windows, IDX_STRID, 2.5);
        assert!(!clusters.is_empty());
    }

    #[test]
    fn test_overlapping_windows_merged_into_one_cluster() {
        let strid = || { let mut v = vec![0.0_f32; 24]; v[IDX_STRID] = 1.0; v };
        let tokens: Vec<_> = (0..6).map(|_| phoneme("s", strid())).collect();
        let windows = sliding_window(&tokens, 3);
        // All windows exceed threshold → should merge to a single cluster
        let clusters = find_clusters(&windows, IDX_STRID, 2.5);
        assert_eq!(clusters.len(), 1);
    }

    #[test]
    fn test_affricate_cluster_detected_via_delrel() {
        // Affricates (d͡ʒ, t͡ʃ) have delrel=+1; a dense run should form a cluster.
        let aff = || { let mut v = vec![0.0_f32; 24]; v[IDX_DELREL] = 1.0; v };
        let tokens: Vec<_> = (0..6).map(|_| phoneme("d", aff())).collect();
        let windows = sliding_window(&tokens, 3);
        let clusters = find_clusters(&windows, IDX_DELREL, 2.0);
        assert!(!clusters.is_empty(), "dense affricate run should form a cluster");
        assert_eq!(clusters[0].peak_value, 3.0); // all 3 phonemes in window are affricates
    }

    #[test]
    fn test_non_affricate_does_not_trigger_affricate_cluster() {
        // Plain fricatives have delrel=0; should not trigger affricate cluster.
        let fric = || { let mut v = vec![0.0_f32; 24]; v[IDX_STRID] = 1.0; v }; // strid only
        let tokens: Vec<_> = (0..6).map(|_| phoneme("s", fric())).collect();
        let windows = sliding_window(&tokens, 3);
        let clusters = find_clusters(&windows, IDX_DELREL, 2.0);
        assert!(clusters.is_empty(), "pure fricatives should not form affricate cluster");
    }
}
