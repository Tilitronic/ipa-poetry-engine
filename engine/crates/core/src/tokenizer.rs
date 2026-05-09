//! `tokenizer.rs` — converts an IPA string into a sequence of `PhoneticToken`s
//! using a **Longest Prefix Match** strategy.
//!
//! Supports multi-codepoint segments (e.g. `d͡ʒ`, `ɡʲː`) by scanning up to
//! `MAX_SYMBOL_CHARS` Unicode scalar values forward at each position.

use ndarray::Array1;

use crate::registry::FeatureRegistry;

// ────────────────────────────────────────────────────────────────────────────
// Constants
// ────────────────────────────────────────────────────────────────────────────

/// Maximum number of Unicode scalar values to try for a single segment.
const MAX_SYMBOL_CHARS: usize = 6;

/// Feature weights applied to each token (index into feature vector for `syl`
/// to identify vowels, applied to the scalar weight field).
const WEIGHT_VOWEL:     f32 = 1.0; // syllabic phoneme
const WEIGHT_CONSONANT: f32 = 0.8; // non-syllabic phoneme
const WEIGHT_BOUNDARY:  f32 = 0.0; // space / line break — no phonetic content

// ────────────────────────────────────────────────────────────────────────────
// Data models
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    Phoneme,
    WordBoundary, // space character
    LineBreak,    // '\n'
    Unknown,      // unrecognised symbol (preserved for diagnostics)
}

#[derive(Debug, Clone)]
pub struct PhoneticToken {
    /// The raw IPA segment (e.g. `"d͡ʒ"`, `"ʃ"`, `" "`).
    pub symbol: String,
    pub t_type: TokenType,
    /// 24-dimensional phonological feature vector (zeros for boundaries).
    pub features: Array1<f32>,
    /// Scalar salience weight.
    pub weight: f32,
}

// ────────────────────────────────────────────────────────────────────────────
// Tokenizer
// ────────────────────────────────────────────────────────────────────────────

/// Parse an IPA string into tokens using the given registry.
///
/// # Boundary characters
/// - `' '` (space) → `WordBoundary`
/// - `'\n'`         → `WordBoundary` (line breaks carry no phonological weight)
/// - `'#'`          → `WordBoundary` (common G2P separator)
///
/// Segments not found in the registry are emitted as `Unknown` tokens with a
/// zero vector so the rest of the pipeline can proceed.
pub fn tokenize(ipa: &str, registry: &FeatureRegistry) -> Vec<PhoneticToken> {
    let chars: Vec<char> = ipa.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        // ── Boundary characters ──────────────────────────────────────────
        if ch == ' ' || ch == '#' || ch == '\n' {
            tokens.push(boundary_token(ch.to_string(), TokenType::WordBoundary));
            i += 1;
            continue;
        }

        // ── Longest prefix match ─────────────────────────────────────────
        let max_len = MAX_SYMBOL_CHARS.min(chars.len() - i);
        let mut matched = false;

        for len in (1..=max_len).rev() {
            let candidate: String = chars[i..i + len].iter().collect();
            if let Some(vec) = registry.get(&candidate) {
                // syl feature is index 0; +1.0 means vowel
                let weight = if vec[0] == 1.0 { WEIGHT_VOWEL } else { WEIGHT_CONSONANT };
                tokens.push(PhoneticToken {
                    symbol:   candidate,
                    t_type:   TokenType::Phoneme,
                    features: vec.clone(),
                    weight,
                });
                i += len;
                matched = true;
                break;
            }
        }

        // ── Unrecognised single character ────────────────────────────────
        if !matched {
            tokens.push(PhoneticToken {
                symbol:   chars[i].to_string(),
                t_type:   TokenType::Unknown,
                features: Array1::zeros(crate::registry::FEATURE_NAMES.len()),
                weight:   0.0,
            });
            i += 1;
        }
    }

    tokens
}

fn boundary_token(symbol: String, t_type: TokenType) -> PhoneticToken {
    PhoneticToken {
        symbol,
        t_type,
        features: Array1::zeros(crate::registry::FEATURE_NAMES.len()),
        weight:   WEIGHT_BOUNDARY,
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::FeatureRegistry;

    /// Minimal registry with a few segments for deterministic tests.
    fn make_registry() -> FeatureRegistry {
        // We embed a tiny JSON that covers the symbols used in tests.
        let json = include_str!("test_data/mini_registry.json");
        FeatureRegistry::from_json_bytes(json.as_bytes()).unwrap()
    }

    // ── Boundary handling ────────────────────────────────────────────────

    #[test]
    fn test_space_becomes_word_boundary() {
        let reg = make_registry();
        let tokens = tokenize(" ", &reg);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].t_type, TokenType::WordBoundary);
        assert_eq!(tokens[0].weight, 0.0);
    }

    #[test]
    fn test_hash_becomes_word_boundary() {
        let reg = make_registry();
        let tokens = tokenize("#", &reg);
        assert_eq!(tokens[0].t_type, TokenType::WordBoundary);
    }

    #[test]
    fn test_newline_becomes_word_boundary() {
        let reg = make_registry();
        let tokens = tokenize("\n", &reg);
        assert_eq!(tokens[0].t_type, TokenType::WordBoundary);
    }

    // ── Single-symbol matching ───────────────────────────────────────────

    #[test]
    fn test_single_consonant_p() {
        let reg = make_registry();
        let tokens = tokenize("p", &reg);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].symbol, "p");
        assert_eq!(tokens[0].t_type, TokenType::Phoneme);
        assert_eq!(tokens[0].weight, 0.8); // consonant
    }

    #[test]
    fn test_vowel_gets_higher_weight() {
        let reg = make_registry();
        let tokens = tokenize("a", &reg);
        assert_eq!(tokens[0].weight, 1.0); // vowel
    }

    // ── Multi-character segment matching ─────────────────────────────────

    #[test]
    fn test_multi_char_segment_matched_as_one_token() {
        let reg = make_registry();
        // "d͡ʒ" is 3 Unicode chars but one segment
        let tokens = tokenize("d͡ʒ", &reg);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].symbol, "d͡ʒ");
        assert_eq!(tokens[0].t_type, TokenType::Phoneme);
    }

    #[test]
    fn test_longest_match_preferred_over_shorter() {
        let reg = make_registry();
        // "d" exists but "d͡ʒ" should win when the full sequence is present
        let tokens = tokenize("d͡ʒ", &reg);
        assert_eq!(tokens[0].symbol, "d͡ʒ");
    }

    // ── Sequences ────────────────────────────────────────────────────────

    #[test]
    fn test_sequence_with_boundary() {
        let reg = make_registry();
        let tokens = tokenize("p a", &reg);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].symbol, "p");
        assert_eq!(tokens[1].t_type, TokenType::WordBoundary);
        assert_eq!(tokens[2].symbol, "a");
    }

    #[test]
    fn test_complex_sequence() {
        let reg = make_registry();
        // "d͡ʒ" + "a" + space + "p"
        let tokens = tokenize("d͡ʒa p", &reg);
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].symbol, "d͡ʒ");
        assert_eq!(tokens[1].symbol, "a");
        assert_eq!(tokens[2].t_type, TokenType::WordBoundary);
        assert_eq!(tokens[3].symbol, "p");
    }

    // ── Unknown symbol handling ──────────────────────────────────────────

    #[test]
    fn test_unknown_symbol_emitted_as_unknown_token() {
        let reg = make_registry();
        let tokens = tokenize("X", &reg);
        assert_eq!(tokens[0].t_type, TokenType::Unknown);
        assert_eq!(tokens[0].weight, 0.0);
        // Feature vector should be all zeros
        assert!(tokens[0].features.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_unknown_symbol_does_not_stop_parsing() {
        let reg = make_registry();
        let tokens = tokenize("pXa", &reg);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].symbol, "p");
        assert_eq!(tokens[1].t_type, TokenType::Unknown);
        assert_eq!(tokens[2].symbol, "a");
    }

    // ── Feature vector sanity checks ─────────────────────────────────────

    #[test]
    fn test_phoneme_feature_vector_length_is_24() {
        let reg = make_registry();
        let tokens = tokenize("p", &reg);
        assert_eq!(tokens[0].features.len(), 24);
    }

    #[test]
    fn test_boundary_feature_vector_is_zeros() {
        let reg = make_registry();
        let tokens = tokenize(" ", &reg);
        assert!(tokens[0].features.iter().all(|&v| v == 0.0));
    }
}
