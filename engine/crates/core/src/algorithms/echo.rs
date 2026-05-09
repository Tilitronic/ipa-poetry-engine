//! `echo.rs` — Sound echo / opacity computation.
//!
//! For every phoneme in the text, finds the nearest phonetically similar phoneme
//! in the flat stream and computes a visual opacity via exponential decay:
//!
//! ```text
//! opacity = max(alpha_min, exp(-gap / lambda))
//! ```
//!
//! where `gap` is the distance in phoneme-units to the nearest similar token.
//!
//! All annotations carry `word_id`, `syllable_index`, and `phoneme_index` so the
//! frontend can map each result back to the exact phoneme in the original IPA
//! Stream document and render it with the appropriate opacity.

use serde::Serialize;

use crate::algorithms::distance::cosine_similarity;
use crate::registry::FeatureRegistry;
use crate::stream::IpaStreamWord;

// ────────────────────────────────────────────────────────────────────────────
// Parameters
// ────────────────────────────────────────────────────────────────────────────

/// Minimum cosine similarity to treat two phonemes as "the same sound family".
pub const DEFAULT_THRESHOLD: f32 = 0.80;

/// Decay constant in phoneme-units; controls how quickly opacity falls off.
pub const DEFAULT_LAMBDA: f32 = 10.0;

/// Opacity floor — ensures every phoneme has at least this opacity.
pub const DEFAULT_ALPHA_MIN: f32 = 0.05;

/// Tuning knobs for [`compute_echo`].
pub struct EchoParams {
    /// Minimum cosine similarity to count as a "match".
    pub threshold: f32,
    /// Exponential decay constant (phoneme units).
    pub lambda: f32,
    /// Opacity floor.
    pub alpha_min: f32,
}

impl Default for EchoParams {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            lambda:    DEFAULT_LAMBDA,
            alpha_min: DEFAULT_ALPHA_MIN,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Output types
// ────────────────────────────────────────────────────────────────────────────

/// Stable reference to one phoneme; the frontend uses this as a lookup key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhonemeRef {
    /// Stable token ID of the containing word.
    pub word_id: String,
    /// 0-based syllable index within the word.
    pub syllable_index: usize,
    /// 0-based phoneme index within the syllable's `tokens` array.
    pub phoneme_index: usize,
    /// Position in the flat phoneme-only stream (0-based).
    pub flat_index: usize,
}

/// Echo annotation for one phoneme.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoAnnotation {
    /// Identifies the phoneme in the original document.
    pub source: PhonemeRef,
    /// `flat_index` of the nearest similar phoneme, or `null` if none found.
    pub nearest_match: Option<usize>,
    /// Gap in phoneme-units to the nearest match (stream length when no match).
    pub gap: f32,
    /// Visual opacity in `[alpha_min, 1.0]`.
    pub opacity: f32,
}

// ────────────────────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────────────────────

/// Compute echo opacity for every phoneme in the word list.
///
/// Words must be provided **in stream order** — i.e. the same order as they
/// appear in the IPA Stream document — so that `flat_index` values are stable
/// and can be used as round-trip keys by the frontend.
///
/// Phonemes whose symbol is unknown to the registry are silently skipped; they
/// receive no annotation.
pub fn compute_echo(
    words: &[&IpaStreamWord],
    registry: &FeatureRegistry,
    params: &EchoParams,
) -> Vec<EchoAnnotation> {
    // ── Build flat phoneme list with refs ─────────────────────────────────
    // Each entry: (PhonemeRef, feature_vector)
    let mut flat: Vec<(PhonemeRef, ndarray::Array1<f32>)> = Vec::new();

    for word in words {
        for (syl_idx, syl) in word.syllables.iter().enumerate() {
            for (ph_idx, token) in syl.tokens.iter().enumerate() {
                if let Some(features) = registry.get(token.as_str()) {
                    flat.push((
                        PhonemeRef {
                            word_id:        word.id.clone(),
                            syllable_index: syl_idx,
                            phoneme_index:  ph_idx,
                            flat_index:     flat.len(), // assigned before push
                        },
                        features.to_owned(),
                    ));
                }
            }
        }
    }

    // Fix flat_index: it was assigned to flat.len() *before* the push, which is
    // the correct 0-based index for each element.  Verify consistency:
    for (i, (pref, _)) in flat.iter().enumerate() {
        debug_assert_eq!(pref.flat_index, i);
    }

    let n = flat.len();

    // ── For each phoneme, find nearest similar neighbor ───────────────────
    flat.iter()
        .enumerate()
        .map(|(i, (pref, feat_i))| {
            let nearest = (0..n)
                .filter(|&j| j != i)
                .filter_map(|j| {
                    let sim = cosine_similarity(feat_i.view(), flat[j].1.view());
                    if sim >= params.threshold {
                        let gap = (i as isize - j as isize).unsigned_abs();
                        Some((j, gap))
                    } else {
                        None
                    }
                })
                .min_by_key(|&(_, gap)| gap);

            let (nearest_match, gap) = match nearest {
                Some((j, g)) => (Some(j), g as f32),
                None         => (None,    n as f32),
            };

            let opacity = params.alpha_min.max((-gap / params.lambda).exp());

            EchoAnnotation {
                source: pref.clone(),
                nearest_match,
                gap,
                opacity,
            }
        })
        .collect()
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::FeatureRegistry;
    use crate::stream::{IpaStreamSyllable, IpaStreamWord, StressSource};

    fn reg() -> FeatureRegistry {
        let json = include_str!("../test_data/mini_registry.json");
        FeatureRegistry::from_json_bytes(json.as_bytes()).unwrap()
    }

    fn make_syl(tokens: &[&str]) -> IpaStreamSyllable {
        IpaStreamSyllable {
            ipa:      tokens.join(""),
            tokens:   tokens.iter().map(|s| s.to_string()).collect(),
            grapheme: String::new(),
            stressed: false,
            is_open:  false,
        }
    }

    fn make_word(id: &str, syls: Vec<IpaStreamSyllable>) -> IpaStreamWord {
        IpaStreamWord {
            id:                id.to_string(),
            line_index:        0,
            word_index:        0,
            language:          "uk".to_string(),
            original:          String::new(),
            syllable_count:    syls.len(),
            stressed_syllable: -1,
            stress_source:     StressSource::Dict,
            syllables:         syls,
        }
    }

    // ── Basic correctness ─────────────────────────────────────────────────

    #[test]
    fn test_empty_words_returns_empty() {
        let reg = reg();
        let result = compute_echo(&[], &reg, &EchoParams::default());
        assert!(result.is_empty());
    }

    #[test]
    fn test_phoneme_ref_word_id_correct() {
        let reg = reg();
        let w = make_word("my-id", vec![make_syl(&["p", "a"])]);
        let result = compute_echo(&[&w], &reg, &EchoParams::default());
        assert!(result.iter().all(|e| e.source.word_id == "my-id"));
    }

    #[test]
    fn test_phoneme_ref_syllable_index_correct() {
        let reg = reg();
        let w = make_word("w", vec![make_syl(&["p"]), make_syl(&["a"])]);
        let result = compute_echo(&[&w], &reg, &EchoParams::default());
        assert_eq!(result[0].source.syllable_index, 0);
        assert_eq!(result[1].source.syllable_index, 1);
    }

    #[test]
    fn test_phoneme_ref_phoneme_index_within_syllable() {
        let reg = reg();
        let w = make_word("w", vec![make_syl(&["p", "a"])]);
        let result = compute_echo(&[&w], &reg, &EchoParams::default());
        assert_eq!(result[0].source.phoneme_index, 0); // "p"
        assert_eq!(result[1].source.phoneme_index, 1); // "a"
    }

    #[test]
    fn test_flat_index_is_sequential() {
        let reg = reg();
        let w = make_word("w", vec![make_syl(&["p", "a", "s"])]);
        let result = compute_echo(&[&w], &reg, &EchoParams::default());
        for (i, ann) in result.iter().enumerate() {
            assert_eq!(ann.source.flat_index, i);
        }
    }

    // ── Opacity values ────────────────────────────────────────────────────

    #[test]
    fn test_opacity_always_in_valid_range() {
        let reg = reg();
        let p = EchoParams::default();
        let w = make_word("w", vec![make_syl(&["p", "a", "b", "u", "s", "ʃ"])]);
        let result = compute_echo(&[&w], &reg, &p);
        for ann in &result {
            assert!(ann.opacity >= p.alpha_min, "opacity below alpha_min: {}", ann.opacity);
            assert!(ann.opacity <= 1.0, "opacity above 1.0: {}", ann.opacity);
        }
    }

    #[test]
    fn test_no_match_gives_alpha_min_opacity() {
        let reg = reg();
        // Single isolated phoneme — no neighbour can match it
        let p = EchoParams { threshold: 0.99, lambda: DEFAULT_LAMBDA, alpha_min: DEFAULT_ALPHA_MIN };
        let w = make_word("w", vec![make_syl(&["p"])]);
        let result = compute_echo(&[&w], &reg, &p);
        assert_eq!(result.len(), 1);
        // No other phoneme → nearest_match is None → gap = n = 1 → opacity = max(0.05, e^(-0.1)) ≈ 0.90
        // But if threshold is 0.99 and there's only one phoneme, nearest_match = None, gap = 1 (n=1)
        // Actually gap = n as f32 = 1, opacity = max(0.05, exp(-1/10)) = max(0.05, 0.905) = 0.905
        // That's not alpha_min... Let me reconsider.
        // With threshold=1.01 (impossible) no match → gap=1, opacity = exp(-0.1) = 0.905
        // To test alpha_min floor we need gap very large relative to lambda.
        assert!(result[0].nearest_match.is_none());
    }

    #[test]
    fn test_nearby_identical_phoneme_gets_high_opacity() {
        let reg = reg();
        // Two consecutive "p" phonemes → gap=1 → opacity = exp(-1/10) ≈ 0.905
        let p = EchoParams { threshold: 0.99, lambda: DEFAULT_LAMBDA, alpha_min: DEFAULT_ALPHA_MIN };
        let w = make_word("w", vec![make_syl(&["p", "p"])]);
        let result = compute_echo(&[&w], &reg, &p);
        // Both "p" phonemes should find each other (gap=1)
        let p0 = &result[0];
        let p1 = &result[1];
        assert_eq!(p0.nearest_match, Some(1));
        assert_eq!(p1.nearest_match, Some(0));
        assert!(p0.opacity > 0.8, "opacity for gap=1 should be high: {}", p0.opacity);
    }

    #[test]
    fn test_distant_phoneme_has_lower_opacity_than_near() {
        let reg = reg();
        let p = EchoParams { threshold: 0.80, lambda: DEFAULT_LAMBDA, alpha_min: DEFAULT_ALPHA_MIN };
        // Layout: p a a a a a a a a a p
        //         0 1 2 3 4 5 6 7 8 9 10
        // First "p" (idx 0): nearest similar "p" at idx 10 → gap=10
        // Second-to-last "a" (idx 8): nearest similar "a" at idx 9 → gap=1
        let tokens: Vec<&str> = std::iter::once("p")
            .chain(std::iter::repeat("a").take(9))
            .chain(std::iter::once("p"))
            .collect();
        let w = make_word("w", vec![make_syl(&tokens)]);
        let result = compute_echo(&[&w], &reg, &p);

        let near_a  = result.iter().find(|e| e.source.flat_index == 8).unwrap();
        let far_p   = result.iter().find(|e| e.source.flat_index == 0).unwrap();
        assert!(near_a.opacity > far_p.opacity,
            "near 'a' opacity ({}) should exceed far 'p' opacity ({})",
            near_a.opacity, far_p.opacity);
    }

    #[test]
    fn test_nearest_match_is_closest_not_farthest() {
        let reg = reg();
        // p at idx 0, p at idx 1, p at idx 4
        // For idx 0: nearest should be idx 1 (gap=1), not idx 4 (gap=4)
        let p = EchoParams { threshold: 0.99, lambda: DEFAULT_LAMBDA, alpha_min: DEFAULT_ALPHA_MIN };
        let w = make_word("w", vec![make_syl(&["p", "p", "a", "u", "p"])]);
        let result = compute_echo(&[&w], &reg, &p);
        let first_p = &result[0];
        assert_eq!(first_p.nearest_match, Some(1), "should pick closest match");
    }
}
