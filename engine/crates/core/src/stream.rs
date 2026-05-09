//! `stream.rs` — IPA Stream v1.1 format deserialization and helpers.
//!
//! Implements the full IPA Stream specification (v1.1) which is the canonical
//! input format produced by the frontend G2P pipeline and consumed by this
//! engine.
//!
//! # Format overview
//!
//! ```json
//! {
//!   "metadata": { "version": "1.1", ... },
//!   "stream": [
//!     { "type": "word", "id": "tok-001", "syllables": [...], ... },
//!     { "type": "whitespace" },
//!     { "type": "line_break", "lineIndex": 0 },
//!     ...
//!   ]
//! }
//! ```

use ndarray::Array1;
use serde::Deserialize;

use crate::registry::{FeatureRegistry, FEATURE_NAMES};
use crate::tokenizer::{PhoneticToken, TokenType};

// ────────────────────────────────────────────────────────────────────────────
// Version constant
// ────────────────────────────────────────────────────────────────────────────

pub const FORMAT_VERSION: &str = "1.1";

// ────────────────────────────────────────────────────────────────────────────
// Serde structs — mirror the JSON schema exactly
// ────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug, Clone)]
pub struct IpaStreamMetadata {
    pub version: String,
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    #[serde(rename = "confirmedLineCount")]
    pub confirmed_line_count: usize,
    #[serde(rename = "totalWordCount")]
    pub total_word_count: usize,
    #[serde(rename = "languagesPresent")]
    pub languages_present: Vec<String>,
}

/// One syllable inside a word token.
#[derive(Deserialize, Debug, Clone)]
pub struct IpaStreamSyllable {
    /// Full IPA string of the syllable (e.g. `"ʃuk"`).
    pub ipa: String,
    /// Discrete phoneme symbols in order (e.g. `["ʃ","u","k"]`).
    pub tokens: Vec<String>,
    /// Grapheme characters aligned to this syllable.
    pub grapheme: String,
    /// Whether this is the stressed syllable.
    pub stressed: bool,
    /// Whether the syllable ends on a vowel (open syllable).
    #[serde(rename = "isOpen")]
    pub is_open: bool,
}

/// Source / reliability of the stress assignment.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StressSource {
    /// High-reliability dictionary lookup.
    Dict,
    /// Medium-reliability ML predictor (OOV words).
    Ml,
    /// Manually confirmed by user — absolute reliability.
    Manual,
}

/// A word element in the IPA stream.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IpaStreamWord {
    /// Stable token ID used for round-trip annotations.
    pub id: String,
    /// 0-based index of the confirmed line this word belongs to.
    pub line_index: usize,
    /// 0-based position of the word within its line.
    pub word_index: usize,
    pub language: String,
    /// Original text as it appears in the poem.
    pub original: String,
    pub syllable_count: usize,
    /// 0-based index of the stressed syllable; `-1` if no stress.
    pub stressed_syllable: i32,
    pub stress_source: StressSource,
    pub syllables: Vec<IpaStreamSyllable>,
}

/// One element of the flat stream array.
#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamElement {
    Word(IpaStreamWord),
    Whitespace,
    /// Punctuation token (comma, period, dash, etc.).
    /// `text` is already normalised by the frontend (e.g. en-dash → em-dash).
    Punctuation { text: String },
    LineBreak {
        #[serde(rename = "lineIndex")]
        line_index: usize,
    },
}

/// Top-level IPA Stream document.
#[derive(Deserialize, Debug)]
pub struct IpaStream {
    pub metadata: IpaStreamMetadata,
    pub stream: Vec<StreamElement>,
}

impl IpaStream {
    /// Parse from raw JSON bytes.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// Version string from metadata (should be `"1.1"`).
    #[inline]
    pub fn version(&self) -> &str {
        &self.metadata.version
    }

    /// Iterate over all word elements in stream order.
    pub fn words(&self) -> impl Iterator<Item = &IpaStreamWord> {
        self.stream.iter().filter_map(|e| {
            if let StreamElement::Word(w) = e { Some(w) } else { None }
        })
    }

    /// Group words into confirmed lines, preserving word order within each line.
    ///
    /// Uses the `line_index` field on each word element as the authoritative
    /// grouping key.  The `line_break` stream elements are **not** used here —
    /// they are purely visual separators with no analytical weight.
    pub fn lines(&self) -> Vec<Vec<&IpaStreamWord>> {
        use std::collections::BTreeMap;
        let mut by_line: BTreeMap<usize, Vec<&IpaStreamWord>> = BTreeMap::new();
        for word in self.words() {
            by_line.entry(word.line_index).or_default().push(word);
        }
        by_line.into_values().collect()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Stress confidence helper
// ────────────────────────────────────────────────────────────────────────────

/// Map a stress source to a numeric confidence value in [0, 1].
pub fn stress_confidence(source: &StressSource) -> f32 {
    match source {
        StressSource::Dict   => 0.95,
        StressSource::Ml     => 0.75,
        StressSource::Manual => 1.00,
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Phoneme token extraction
// ────────────────────────────────────────────────────────────────────────────

/// Feature-vector weights based on stress × syllabicity.
///
/// | Stressed | Syllabic | Weight |
/// |----------|----------|--------|
/// | yes      | yes      | 1.5    |
/// | yes      | no       | 1.0    |
/// | no       | yes      | 0.8    |
/// | no       | no       | 0.6    |
fn phoneme_weight(is_stressed: bool, features: &Array1<f32>) -> f32 {
    let is_syllabic = features[0] == 1.0; // syl feature at index 0
    match (is_stressed, is_syllabic) {
        (true,  true)  => 1.5,
        (true,  false) => 1.0,
        (false, true)  => 0.8,
        (false, false) => 0.6,
    }
}

/// Build all `PhoneticToken`s from a word, with stress-aware weights.
///
/// Unknown IPA symbols (not in the registry) are emitted as `Unknown` tokens
/// with a zero vector so downstream algorithms can proceed.
pub fn tokens_from_word(word: &IpaStreamWord, registry: &FeatureRegistry) -> Vec<PhoneticToken> {
    let mut tokens = Vec::new();

    for (syl_idx, syllable) in word.syllables.iter().enumerate() {
        let is_stressed = word.stressed_syllable >= 0
            && syl_idx == word.stressed_syllable as usize;

        for ipa_symbol in &syllable.tokens {
            if let Some(features) = registry.get(ipa_symbol) {
                let weight = phoneme_weight(is_stressed, features);
                tokens.push(PhoneticToken {
                    symbol:   ipa_symbol.clone(),
                    t_type:   TokenType::Phoneme,
                    features: features.clone(),
                    weight,
                });
            } else {
                tokens.push(PhoneticToken {
                    symbol:   ipa_symbol.clone(),
                    t_type:   TokenType::Unknown,
                    features: Array1::zeros(FEATURE_NAMES.len()),
                    weight:   0.0,
                });
            }
        }
    }

    tokens
}

/// Extract only the **coda** of a word — phonemes from the stressed syllable
/// to the end of the word.
///
/// This is the phonetically relevant portion for rhyme matching.
/// If the word has no stress (`stressed_syllable == -1`), the full word is
/// returned.
pub fn coda_tokens_from_word(
    word: &IpaStreamWord,
    registry: &FeatureRegistry,
) -> Vec<PhoneticToken> {
    let coda_start = if word.stressed_syllable >= 0 {
        word.stressed_syllable as usize
    } else {
        0 // no stress → whole word is the coda
    };

    let mut tokens = Vec::new();

    for (syl_idx, syllable) in word.syllables.iter().enumerate() {
        if syl_idx < coda_start {
            continue;
        }
        let is_stressed = syl_idx == coda_start && word.stressed_syllable >= 0;

        for ipa_symbol in &syllable.tokens {
            if let Some(features) = registry.get(ipa_symbol) {
                let weight = phoneme_weight(is_stressed, features);
                tokens.push(PhoneticToken {
                    symbol:   ipa_symbol.clone(),
                    t_type:   TokenType::Phoneme,
                    features: features.clone(),
                    weight,
                });
            } else {
                tokens.push(PhoneticToken {
                    symbol:   ipa_symbol.clone(),
                    t_type:   TokenType::Unknown,
                    features: Array1::zeros(FEATURE_NAMES.len()),
                    weight:   0.0,
                });
            }
        }
    }

    tokens
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::FeatureRegistry;

    fn reg() -> FeatureRegistry {
        let json = include_str!("test_data/mini_registry.json");
        FeatureRegistry::from_json_bytes(json.as_bytes()).unwrap()
    }

    /// The canonical example from the IPA Stream v1.1 specification.
    fn spec_example_json() -> &'static str {
        r#"{
          "metadata": {
            "version": "1.1",
            "generatedAt": "2026-05-08T14:22:00.000Z",
            "confirmedLineCount": 2,
            "totalWordCount": 3,
            "languagesPresent": ["uk"]
          },
          "stream": [
            {
              "type": "word",
              "id": "tok-001",
              "lineIndex": 0,
              "wordIndex": 0,
              "language": "uk",
              "original": "башук",
              "syllableCount": 2,
              "stressedSyllable": 1,
              "stressSource": "dict",
              "syllables": [
                { "ipa": "ba", "tokens": ["b", "a"], "grapheme": "ба", "stressed": false, "isOpen": true },
                { "ipa": "ʃuk", "tokens": ["ʃ", "u", "k"], "grapheme": "шук", "stressed": true, "isOpen": false }
              ]
            },
            { "type": "whitespace" },
            {
              "type": "word",
              "id": "tok-002",
              "lineIndex": 0,
              "wordIndex": 1,
              "language": "uk",
              "original": "капуш",
              "syllableCount": 2,
              "stressedSyllable": 1,
              "stressSource": "dict",
              "syllables": [
                { "ipa": "ka", "tokens": ["k", "a"], "grapheme": "ка", "stressed": false, "isOpen": true },
                { "ipa": "puʃ", "tokens": ["p", "u", "ʃ"], "grapheme": "пуш", "stressed": true, "isOpen": false }
              ]
            },
            { "type": "line_break", "lineIndex": 0 },
            {
              "type": "word",
              "id": "tok-003",
              "lineIndex": 1,
              "wordIndex": 0,
              "language": "uk",
              "original": "недоріка",
              "syllableCount": 4,
              "stressedSyllable": 2,
              "stressSource": "dict",
              "syllables": [
                { "ipa": "ne", "tokens": ["b", "a"], "grapheme": "не", "stressed": false, "isOpen": true },
                { "ipa": "do", "tokens": ["d", "a"], "grapheme": "до", "stressed": false, "isOpen": true },
                { "ipa": "ʃi", "tokens": ["ʃ", "u"], "grapheme": "рі", "stressed": true,  "isOpen": true },
                { "ipa": "ka", "tokens": ["k", "a"], "grapheme": "ка", "stressed": false, "isOpen": true }
              ]
            }
          ]
        }"#
    }

    // ── Parsing ──────────────────────────────────────────────────────────

    #[test]
    fn test_parse_spec_example() {
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        assert_eq!(stream.metadata.version, "1.1");
        assert_eq!(stream.metadata.confirmed_line_count, 2);
        assert_eq!(stream.metadata.total_word_count, 3);
        assert_eq!(stream.metadata.languages_present, vec!["uk"]);
    }

    #[test]
    fn test_stream_element_count() {
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        // 3 words + 1 whitespace + 1 line_break = 5
        assert_eq!(stream.stream.len(), 5);
    }

    #[test]
    fn test_word_fields_parsed_correctly() {
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let word = match &stream.stream[0] {
            StreamElement::Word(w) => w,
            _ => panic!("expected word"),
        };
        assert_eq!(word.id, "tok-001");
        assert_eq!(word.line_index, 0);
        assert_eq!(word.word_index, 0);
        assert_eq!(word.original, "башук");
        assert_eq!(word.syllable_count, 2);
        assert_eq!(word.stressed_syllable, 1);
        assert_eq!(word.stress_source, StressSource::Dict);
    }

    #[test]
    fn test_whitespace_element_parsed() {
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        assert!(matches!(stream.stream[1], StreamElement::Whitespace));
    }

    #[test]
    fn test_line_break_element_parsed() {
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        assert!(matches!(
            stream.stream[3],
            StreamElement::LineBreak { line_index: 0 }
        ));
    }

    #[test]
    fn test_syllable_fields_parsed() {
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let word = match &stream.stream[0] {
            StreamElement::Word(w) => w,
            _ => panic!("expected word"),
        };
        assert_eq!(word.syllables.len(), 2);
        assert_eq!(word.syllables[0].ipa, "ba");
        assert!(!word.syllables[0].stressed);
        assert!(word.syllables[0].is_open);
        assert!(word.syllables[1].stressed);
        assert!(!word.syllables[1].is_open);
    }

    #[test]
    fn test_invalid_json_returns_error() {
        assert!(IpaStream::from_json_bytes(b"not json").is_err());
    }

    // ── lines() ──────────────────────────────────────────────────────────

    #[test]
    fn test_lines_groups_words_correctly() {
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let lines = stream.lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 2); // башук + капуш
        assert_eq!(lines[1].len(), 1); // недоріка
    }

    #[test]
    fn test_lines_preserves_word_order() {
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let lines = stream.lines();
        assert_eq!(lines[0][0].id, "tok-001");
        assert_eq!(lines[0][1].id, "tok-002");
        assert_eq!(lines[1][0].id, "tok-003");
    }

    // ── words() ──────────────────────────────────────────────────────────

    #[test]
    fn test_words_iterator_returns_all_words() {
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let words: Vec<_> = stream.words().collect();
        assert_eq!(words.len(), 3);
        assert_eq!(words[0].id, "tok-001");
        assert_eq!(words[2].id, "tok-003");
    }

    // ── stress_confidence ────────────────────────────────────────────────

    #[test]
    fn test_stress_confidence_dict_is_high() {
        assert!((stress_confidence(&StressSource::Dict) - 0.95).abs() < 1e-6);
    }

    #[test]
    fn test_stress_confidence_ml_is_medium() {
        assert!((stress_confidence(&StressSource::Ml) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn test_stress_confidence_manual_is_one() {
        assert!((stress_confidence(&StressSource::Manual) - 1.0).abs() < 1e-6);
    }

    // ── tokens_from_word ─────────────────────────────────────────────────

    #[test]
    fn test_tokens_from_word_returns_all_phonemes() {
        let reg = reg();
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let word = stream.words().next().unwrap(); // "башук" → [b,a,ʃ,u,k]
        let tokens = tokens_from_word(word, &reg);
        // b,a (syl 0) + ʃ,u,k (syl 1) = 5 phonemes
        assert_eq!(tokens.len(), 5);
    }

    #[test]
    fn test_stressed_vowel_gets_weight_1_5() {
        let reg = reg();
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let word = stream.words().next().unwrap(); // stressedSyllable=1
        let tokens = tokens_from_word(word, &reg);
        // Token index 3 = "u" in stressed syllable (syllabic)
        assert_eq!(tokens[3].symbol, "u");
        assert_eq!(tokens[3].weight, 1.5);
    }

    #[test]
    fn test_stressed_consonant_gets_weight_1_0() {
        let reg = reg();
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let word = stream.words().next().unwrap(); // stressedSyllable=1 → ʃ,u,k
        let tokens = tokens_from_word(word, &reg);
        // Token index 2 = "ʃ" in stressed syllable (consonant)
        assert_eq!(tokens[2].symbol, "ʃ");
        assert_eq!(tokens[2].weight, 1.0);
    }

    #[test]
    fn test_unstressed_vowel_gets_weight_0_8() {
        let reg = reg();
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let word = stream.words().next().unwrap(); // syl[0]=[b,a] unstressed
        let tokens = tokens_from_word(word, &reg);
        // Token index 1 = "a" in unstressed syllable (syllabic)
        assert_eq!(tokens[1].symbol, "a");
        assert_eq!(tokens[1].weight, 0.8);
    }

    #[test]
    fn test_unstressed_consonant_gets_weight_0_6() {
        let reg = reg();
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let word = stream.words().next().unwrap(); // syl[0]=[b,a] unstressed
        let tokens = tokens_from_word(word, &reg);
        // Token index 0 = "b" in unstressed syllable (consonant)
        assert_eq!(tokens[0].symbol, "b");
        assert_eq!(tokens[0].weight, 0.6);
    }

    // ── coda_tokens_from_word ────────────────────────────────────────────

    #[test]
    fn test_coda_starts_at_stressed_syllable() {
        let reg = reg();
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let word = stream.words().next().unwrap(); // "башук" stressed_syllable=1
        let coda = coda_tokens_from_word(word, &reg);
        // Coda should be only syl[1]: ʃ,u,k
        assert_eq!(coda.len(), 3);
        assert_eq!(coda[0].symbol, "ʃ");
        assert_eq!(coda[1].symbol, "u");
        assert_eq!(coda[2].symbol, "k");
    }

    #[test]
    fn test_coda_no_stress_returns_full_word() {
        let reg = reg();
        // Manually construct a word with stressed_syllable = -1
        let json = r#"{
          "metadata": {"version":"1.1","generatedAt":"2026-01-01T00:00:00.000Z","confirmedLineCount":1,"totalWordCount":1,"languagesPresent":["uk"]},
          "stream": [{
            "type": "word", "id": "t1", "lineIndex": 0, "wordIndex": 0,
            "language": "uk", "original": "і",
            "syllableCount": 1, "stressedSyllable": -1, "stressSource": "dict",
            "syllables": [
              { "ipa": "a", "tokens": ["a"], "grapheme": "і", "stressed": false, "isOpen": true }
            ]
          }]
        }"#;
        let stream = IpaStream::from_json_bytes(json.as_bytes()).unwrap();
        let word = stream.words().next().unwrap();
        let coda = coda_tokens_from_word(word, &reg);
        assert_eq!(coda.len(), 1); // whole word
    }

    #[test]
    fn test_coda_stressed_vowel_weight_is_1_5() {
        let reg = reg();
        let stream = IpaStream::from_json_bytes(spec_example_json().as_bytes()).unwrap();
        let word = stream.words().next().unwrap();
        let coda = coda_tokens_from_word(word, &reg);
        // "u" is the vowel of the stressed syllable
        let u_tok = coda.iter().find(|t| t.symbol == "u").unwrap();
        assert_eq!(u_tok.weight, 1.5);
    }

    // ── Punctuation element ───────────────────────────────────────────────

    #[test]
    fn test_punctuation_element_parsed() {
        let json = r#"{
          "metadata": {"version":"1.1","generatedAt":"2026-01-01T00:00:00.000Z","confirmedLineCount":1,"totalWordCount":1,"languagesPresent":["uk"]},
          "stream": [
            {"type":"punctuation","text":"—"},
            {"type":"whitespace"},
            {"type":"punctuation","text":"!"}
          ]
        }"#;
        let stream = IpaStream::from_json_bytes(json.as_bytes()).unwrap();
        assert_eq!(stream.stream.len(), 3);
        assert!(matches!(&stream.stream[0], StreamElement::Punctuation { text } if text == "—"));
        assert!(matches!(&stream.stream[2], StreamElement::Punctuation { text } if text == "!"));
    }

    #[test]
    fn test_punctuation_does_not_appear_in_words_or_lines() {
        let json = r#"{
          "metadata": {"version":"1.1","generatedAt":"2026-01-01T00:00:00.000Z","confirmedLineCount":1,"totalWordCount":2,"languagesPresent":["uk"]},
          "stream": [
            {"type":"word","id":"t1","lineIndex":0,"wordIndex":0,"language":"uk","original":"ой","syllableCount":1,"stressedSyllable":0,"stressSource":"dict",
             "syllables":[{"ipa":"oj","tokens":["a"],"grapheme":"ой","stressed":true,"isOpen":true}]},
            {"type":"punctuation","text":","},
            {"type":"whitespace"},
            {"type":"word","id":"t2","lineIndex":0,"wordIndex":1,"language":"uk","original":"ти","syllableCount":1,"stressedSyllable":0,"stressSource":"dict",
             "syllables":[{"ipa":"tɪ","tokens":["a"],"grapheme":"ти","stressed":true,"isOpen":true}]}
          ]
        }"#;
        let stream = IpaStream::from_json_bytes(json.as_bytes()).unwrap();
        let words: Vec<_> = stream.words().collect();
        assert_eq!(words.len(), 2);
        let lines = stream.lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 2);
    }
}
