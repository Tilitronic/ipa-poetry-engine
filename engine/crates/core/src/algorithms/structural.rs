//! `structural.rs` — Syllabic-structure analysis for structural rhyme detection.
//!
//! Computes the "shape fingerprint" of each word's rhyming coda (stressed
//! syllable onwards) in terms of:
//! - `onset`: consonants before the first vowel nucleus.
//! - `coda`:  consonants after the last vowel nucleus.
//!
//! Words that share the same `(onset, coda)` fingerprint form a **structural
//! rhyme group** even when the actual phonemes differ (e.g. "пласт" ~ "блиск").
//!
//! All annotations carry `word_id` for direct frontend mapping.

use std::collections::HashMap;
use serde::Serialize;

use crate::registry::FeatureRegistry;
use crate::stream::IpaStreamWord;

// Syllabic feature index — must match FEATURE_NAMES[0] = "syl" in registry.rs
const IDX_SYL: usize = 0;

// ────────────────────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────────────────────

/// Structural fingerprint of a rhyming coda segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyllableShape {
    /// Number of consonants before the first vowel nucleus.
    pub onset: u8,
    /// Number of consonants after the last vowel nucleus.
    pub coda: u8,
}

/// Structural annotation for one word; keyed by `word_id` for round-trip use.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralAnnotation {
    /// Stable token ID of the word (from IPA Stream).
    pub word_id: String,
    /// Syllabic shape of this word's rhyming coda.
    pub shape: SyllableShape,
    /// Group letter (A, B, C, …) shared by all words with the same shape,
    /// or `null` if the shape is unique.
    pub structural_rhyme_group: Option<String>,
}

// ────────────────────────────────────────────────────────────────────────────
// Private helpers
// ────────────────────────────────────────────────────────────────────────────

/// Return `true` if the phoneme symbol is a vowel according to the registry.
fn is_vowel(symbol: &str, registry: &FeatureRegistry) -> bool {
    registry
        .get(symbol)
        .map(|v| v[IDX_SYL] > 0.5)
        .unwrap_or(false)
}

/// Compute the (onset, coda) shape of a flat token sequence.
fn syllable_shape(tokens: &[String], registry: &FeatureRegistry) -> SyllableShape {
    let first_vowel = tokens.iter().position(|t| is_vowel(t, registry));
    let last_vowel  = tokens.iter().rposition(|t| is_vowel(t, registry));

    let onset = first_vowel.unwrap_or(tokens.len()) as u8;
    let coda  = match last_vowel {
        None     => 0,
        Some(lv) => tokens.len().saturating_sub(lv + 1) as u8,
    };

    SyllableShape { onset, coda }
}

/// Compute the structural shape of a word's rhyming coda (stressed syllable →
/// end of word).  Falls back to the whole word when no stress is marked.
fn word_coda_shape(word: &IpaStreamWord, registry: &FeatureRegistry) -> SyllableShape {
    let start = if word.stressed_syllable >= 0 {
        (word.stressed_syllable as usize).min(word.syllables.len().saturating_sub(1))
    } else {
        0
    };

    let coda_tokens: Vec<String> = word.syllables[start..]
        .iter()
        .flat_map(|s| s.tokens.iter().cloned())
        .collect();

    if coda_tokens.is_empty() {
        return SyllableShape { onset: 0, coda: 0 };
    }

    syllable_shape(&coda_tokens, registry)
}

// ────────────────────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────────────────────

/// Compute structural rhyme annotations for a list of words.
///
/// Words that share the same `(onset, coda)` fingerprint in their rhyming
/// coda receive the same `structural_rhyme_group` letter.  Unique shapes
/// receive `None`.
///
/// Group labels are assigned in first-occurrence order (A, B, C, …) and are
/// deterministic for a given input ordering.
pub fn analyze_structural(
    words: &[&IpaStreamWord],
    registry: &FeatureRegistry,
) -> Vec<StructuralAnnotation> {
    // ── Compute shape per word ────────────────────────────────────────────
    let shapes: Vec<(&IpaStreamWord, SyllableShape)> = words
        .iter()
        .map(|w| (*w, word_coda_shape(w, registry)))
        .collect();

    // ── Count shape frequencies ───────────────────────────────────────────
    let mut shape_count: HashMap<&SyllableShape, usize> = HashMap::new();
    for (_, shape) in &shapes {
        *shape_count.entry(shape).or_insert(0) += 1;
    }

    // ── Assign group labels in first-occurrence order ─────────────────────
    let mut shape_to_group: HashMap<SyllableShape, String> = HashMap::new();
    let mut label = b'A';

    for (_, shape) in &shapes {
        if shape_count[shape] >= 2 && !shape_to_group.contains_key(shape) {
            shape_to_group.insert(shape.clone(), (label as char).to_string());
            label = label.saturating_add(1);
        }
    }

    // ── Build annotation vec ──────────────────────────────────────────────
    shapes
        .into_iter()
        .map(|(word, shape)| {
            let structural_rhyme_group = shape_to_group.get(&shape).cloned();
            StructuralAnnotation { word_id: word.id.clone(), shape, structural_rhyme_group }
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

    fn make_syl(tokens: &[&str], stressed: bool) -> IpaStreamSyllable {
        let last_is_vowel = tokens.last().map(|&t| matches!(t, "a" | "u")).unwrap_or(false);
        IpaStreamSyllable {
            ipa:      tokens.join(""),
            tokens:   tokens.iter().map(|s| s.to_string()).collect(),
            grapheme: String::new(),
            stressed,
            is_open:  last_is_vowel,
        }
    }

    fn make_word(id: &str, stressed_syl: i32, syls: Vec<IpaStreamSyllable>) -> IpaStreamWord {
        IpaStreamWord {
            id:                id.to_string(),
            line_index:        0,
            word_index:        0,
            language:          "uk".to_string(),
            original:          String::new(),
            syllable_count:    syls.len(),
            stressed_syllable: stressed_syl,
            stress_source:     StressSource::Dict,
            syllables:         syls,
        }
    }

    // ── SyllableShape computation ─────────────────────────────────────────

    #[test]
    fn test_cv_syllable_has_onset_1_coda_0() {
        // "pa" → consonant then vowel → onset=1, coda=0
        let reg = reg();
        let shape = syllable_shape(
            &["p".to_string(), "a".to_string()],
            &reg,
        );
        assert_eq!(shape, SyllableShape { onset: 1, coda: 0 });
    }

    #[test]
    fn test_cvc_syllable_has_onset_1_coda_1() {
        // "puʃ" → p(C) u(V) ʃ(C) → onset=1, coda=1
        let reg = reg();
        let shape = syllable_shape(
            &["p".to_string(), "u".to_string(), "ʃ".to_string()],
            &reg,
        );
        assert_eq!(shape, SyllableShape { onset: 1, coda: 1 });
    }

    #[test]
    fn test_open_syllable_has_zero_coda() {
        // "ba" → onset=1, coda=0
        let reg = reg();
        let shape = syllable_shape(
            &["b".to_string(), "a".to_string()],
            &reg,
        );
        assert_eq!(shape.coda, 0);
    }

    #[test]
    fn test_pure_vowel_has_onset_0() {
        // "a" → no consonant before vowel → onset=0
        let reg = reg();
        let shape = syllable_shape(&["a".to_string()], &reg);
        assert_eq!(shape.onset, 0);
    }

    // ── Group assignment ──────────────────────────────────────────────────

    #[test]
    fn test_cv_words_share_group() {
        let reg = reg();
        // Both words end with a CV syllable (stressed) → same shape → same group
        let w1 = make_word("t1", 0, vec![make_syl(&["p", "a"], true)]);
        let w2 = make_word("t2", 0, vec![make_syl(&["b", "a"], true)]);
        let result = analyze_structural(&[&w1, &w2], &reg);
        let g1 = &result[0].structural_rhyme_group;
        let g2 = &result[1].structural_rhyme_group;
        assert!(g1.is_some(), "t1 should have a structural group");
        assert_eq!(g1, g2, "same shape should share group");
    }

    #[test]
    fn test_cv_and_cvc_get_different_groups() {
        let reg = reg();
        // Need 2 words of each shape so groups are assigned
        let w1 = make_word("t1", 0, vec![make_syl(&["p", "a"],      true)]); // CV
        let w2 = make_word("t2", 0, vec![make_syl(&["p", "u", "ʃ"], true)]); // CVC
        let w3 = make_word("t3", 0, vec![make_syl(&["b", "a"],      true)]); // CV (same as t1)
        let w4 = make_word("t4", 0, vec![make_syl(&["b", "u", "ʃ"], true)]); // CVC (same as t2)
        let result = analyze_structural(&[&w1, &w2, &w3, &w4], &reg);

        let g = |id: &str| result.iter().find(|a| a.word_id == id)
            .unwrap().structural_rhyme_group.clone();
        assert!(g("t1").is_some(), "CV words should have a group");
        assert!(g("t2").is_some(), "CVC words should have a group");
        assert_ne!(g("t1"), g("t2"), "CV and CVC should have different groups");
    }

    #[test]
    fn test_single_word_gets_no_group() {
        let reg = reg();
        let w = make_word("t1", 0, vec![make_syl(&["p", "a"], true)]);
        let result = analyze_structural(&[&w], &reg);
        assert!(result[0].structural_rhyme_group.is_none(), "unique shape should have no group");
    }

    #[test]
    fn test_group_labels_start_at_a() {
        let reg = reg();
        let w1 = make_word("t1", 0, vec![make_syl(&["p", "a"], true)]);
        let w2 = make_word("t2", 0, vec![make_syl(&["b", "a"], true)]);
        let result = analyze_structural(&[&w1, &w2], &reg);
        assert_eq!(
            result[0].structural_rhyme_group.as_deref(), Some("A"),
            "first group should be A"
        );
    }

    #[test]
    fn test_three_shapes_three_groups() {
        let reg = reg();
        // Shape 1 (CV): t1, t4
        // Shape 2 (CVC): t2, t5
        // Shape 3 (V): t3, t6
        let cv  = |id: &str| make_word(id, 0, vec![make_syl(&["p", "a"],       true)]);
        let cvc = |id: &str| make_word(id, 0, vec![make_syl(&["p", "u", "ʃ"],  true)]);
        let v   = |id: &str| make_word(id, 0, vec![make_syl(&["a"],            true)]);
        let words = [cv("t1"), cvc("t2"), v("t3"), cv("t4"), cvc("t5"), v("t6")];
        let refs: Vec<&IpaStreamWord> = words.iter().collect();
        let result = analyze_structural(&refs, &reg);

        let g = |id: &str| result.iter().find(|a| a.word_id == id)
            .unwrap().structural_rhyme_group.clone();

        assert_eq!(g("t1"), g("t4"));
        assert_eq!(g("t2"), g("t5"));
        assert_eq!(g("t3"), g("t6"));
        assert_ne!(g("t1"), g("t2"));
        assert_ne!(g("t2"), g("t3"));
    }

    #[test]
    fn test_structural_uses_stressed_syllable_onwards() {
        let reg = reg();
        // Word with 2 syllables: [ba (unstressed), puʃ (stressed)]
        // stressed_syllable=1 → coda shape computed from "puʃ" alone → CVC shape
        let w = make_word("t1", 1, vec![
            make_syl(&["b", "a"],       false),
            make_syl(&["p", "u", "ʃ"], true),
        ]);
        // Another word: single syllable "puʃ" → same CVC shape
        let w2 = make_word("t2", 0, vec![make_syl(&["p", "u", "ʃ"], true)]);
        let result = analyze_structural(&[&w, &w2], &reg);
        assert_eq!(
            result[0].structural_rhyme_group, result[1].structural_rhyme_group,
            "coda shapes should match when stressed syllable onwards is identical"
        );
    }

    #[test]
    fn test_word_id_preserved_in_annotation() {
        let reg = reg();
        let w = make_word("my-unique-id", 0, vec![make_syl(&["a"], true)]);
        let result = analyze_structural(&[&w], &reg);
        assert_eq!(result[0].word_id, "my-unique-id");
    }

    #[test]
    fn test_empty_words_returns_empty() {
        let reg = reg();
        let result = analyze_structural(&[], &reg);
        assert!(result.is_empty());
    }

    #[test]
    fn test_shape_field_serialises_to_camel_case() {
        let shape = SyllableShape { onset: 1, coda: 2 };
        let json = serde_json::to_string(&shape).unwrap();
        assert!(json.contains("\"onset\""));
        assert!(json.contains("\"coda\""));
    }
}
