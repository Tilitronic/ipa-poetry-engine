//! `matrix.rs` — assembles a `PhoneticStream` from a tokenized IPA string.
//!
//! The stream stores all tokens *plus* a 2-D `Array2<f32>` (N × 24) that
//! stacks all feature vectors so that slicing and mathematical operations
//! on contiguous memory are fast.

use ndarray::Array2;

use crate::registry::FeatureRegistry;
use crate::tokenizer::{tokenize, PhoneticToken};

// ────────────────────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────────────────────

/// The fully-parsed representation of a poem / IPA string.
pub struct PhoneticStream {
    /// Ordered list of all tokens (phonemes + boundaries).
    pub tokens: Vec<PhoneticToken>,
    /// N × 24 matrix; row `i` is `tokens[i].features`.
    pub feature_matrix: Array2<f32>,
}

impl PhoneticStream {
    /// Parse `ipa_string` using `registry` and build the stream.
    pub fn from_ipa(ipa_string: &str, registry: &FeatureRegistry) -> Self {
        let tokens = tokenize(ipa_string, registry);
        let n_tokens = tokens.len();
        let n_features = crate::registry::FEATURE_NAMES.len();

        let mut matrix = Array2::<f32>::zeros((n_tokens, n_features));
        for (i, tok) in tokens.iter().enumerate() {
            matrix.row_mut(i).assign(&tok.features);
        }

        Self { tokens, feature_matrix: matrix }
    }

    /// Number of tokens in the stream.
    #[inline]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Return only the phoneme tokens (skip boundaries / unknowns).
    pub fn phonemes_only(&self) -> impl Iterator<Item = &PhoneticToken> {
        use crate::tokenizer::TokenType;
        self.tokens.iter().filter(|t| t.t_type == TokenType::Phoneme)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::FeatureRegistry;
    use crate::tokenizer::TokenType;

    fn make_registry() -> FeatureRegistry {
        let json = include_str!("test_data/mini_registry.json");
        FeatureRegistry::from_json_bytes(json.as_bytes()).unwrap()
    }

    // ── Shape ────────────────────────────────────────────────────────────

    #[test]
    fn test_matrix_row_count_equals_token_count() {
        let reg = make_registry();
        let stream = PhoneticStream::from_ipa("p a", &reg);
        // "p", " ", "a" → 3 tokens
        assert_eq!(stream.feature_matrix.nrows(), 3);
    }

    #[test]
    fn test_matrix_column_count_is_24() {
        let reg = make_registry();
        let stream = PhoneticStream::from_ipa("p", &reg);
        assert_eq!(stream.feature_matrix.ncols(), 24);
    }

    #[test]
    fn test_len_matches_token_vec() {
        let reg = make_registry();
        let stream = PhoneticStream::from_ipa("p a", &reg);
        assert_eq!(stream.len(), stream.tokens.len());
    }

    // ── Content ──────────────────────────────────────────────────────────

    #[test]
    fn test_matrix_row_matches_token_features() {
        let reg = make_registry();
        let stream = PhoneticStream::from_ipa("p", &reg);
        let row: Vec<f32> = stream.feature_matrix.row(0).to_vec();
        let tok: Vec<f32> = stream.tokens[0].features.to_vec();
        assert_eq!(row, tok);
    }

    #[test]
    fn test_boundary_row_is_zeros() {
        let reg = make_registry();
        let stream = PhoneticStream::from_ipa("p a", &reg);
        // index 1 = space token
        let row = stream.feature_matrix.row(1);
        assert!(row.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_multi_char_segment_in_matrix() {
        let reg = make_registry();
        let stream = PhoneticStream::from_ipa("d͡ʒ", &reg);
        assert_eq!(stream.len(), 1);
        assert_eq!(stream.tokens[0].symbol, "d͡ʒ");
        // strid feature (index 7) should be +1.0 for d͡ʒ
        assert_eq!(stream.feature_matrix[(0, 7)], 1.0);
    }

    // ── phonemes_only ────────────────────────────────────────────────────

    #[test]
    fn test_phonemes_only_skips_boundaries() {
        let reg = make_registry();
        let stream = PhoneticStream::from_ipa("p a", &reg);
        let phonemes: Vec<_> = stream.phonemes_only().collect();
        assert_eq!(phonemes.len(), 2);
        assert!(phonemes.iter().all(|t| t.t_type == TokenType::Phoneme));
    }

    #[test]
    fn test_empty_input_produces_empty_stream() {
        let reg = make_registry();
        let stream = PhoneticStream::from_ipa("", &reg);
        assert!(stream.is_empty());
        assert_eq!(stream.feature_matrix.nrows(), 0);
    }
}
