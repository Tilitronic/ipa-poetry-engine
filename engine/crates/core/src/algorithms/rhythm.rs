//! `rhythm.rs` — Syllabic stress-pattern analysis.
//!
//! Analyses the accentual (stress) pattern of each confirmed line:
//! - Detects the dominant metre **period** (2 = binary, 3 = ternary) and phase.
//! - Classifies every syllable as `Match`, `Pyrrhic`, or `Spondee`.
//! - Computes a **confidence** score [0, 1] — how regularly the pattern repeats.
//! - Identifies the **clausula** type (masculine / feminine / dactylic).
//!
//! All annotations carry `word_id` + `syllable_index` so the frontend can map
//! them back to individual syllables in the original IPA Stream document.

use schemars::JsonSchema;
use serde::Serialize;

use crate::stream::IpaStreamWord;

// ────────────────────────────────────────────────────────────────────────────
// Public types (all Serialize → appear verbatim in StreamAnalysisResult JSON)
// ────────────────────────────────────────────────────────────────────────────

/// Stable reference to one syllable; the frontend uses this as a lookup key.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyllableRef {
    /// Stable token ID of the containing word (from IPA Stream `id` field).
    pub word_id: String,
    /// 0-based index of this syllable within the word.
    pub syllable_index: usize,
    /// 0-based position of this syllable within the line's flat syllable sequence.
    pub line_position: usize,
}

/// How a syllable's actual stress relates to the detected metre.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviationType {
    /// Actual and expected stress agree.
    Match,
    /// Expected stressed position was unstressed (skipped stress).
    Pyrrhic,
    /// Unexpected stress on a weak position.
    Spondee,
}

/// Stress annotation for one syllable.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyllableAnnotation {
    /// Identifies the syllable for frontend mapping.
    pub syllable_ref: SyllableRef,
    /// Actual stress value: `1.0` = stressed, `0.0` = unstressed.
    pub stress: f32,
    /// Expected stress under the detected metre.
    pub expected: f32,
    /// Classification relative to the metre.
    pub deviation: DeviationType,
}

/// How the line ends relative to its last stressed syllable.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Clausula {
    /// Last stress on the final syllable.
    Masculine,
    /// Last stress on the penultimate syllable.
    Feminine,
    /// Last stress on the antepenultimate syllable.
    Dactylic,
    /// Last stress four or more syllables from the end.
    Hyperdactylic,
}

/// Full rhythm analysis result for one confirmed line.
///
/// Serialises with camelCase field names for the round-trip protocol.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineRhythm {
    /// 0-based index of the confirmed line in the poem.
    pub line_index: usize,
    /// Dominant metre period: `2` (binary) or `3` (ternary).
    pub period: u8,
    /// Phase offset: the position within the period where stress is expected.
    pub phase: u8,
    /// Clausula type (how the line ends).
    pub clausula: Clausula,
    /// Regularity score [0, 1]; 1.0 = every syllable matches the pattern.
    pub confidence: f32,
    /// Per-syllable annotations in line order (all syllables, including unstressed).
    pub syllables: Vec<SyllableAnnotation>,
    /// Total syllable count across all words in the line.
    pub syllable_count: usize,
}

// ────────────────────────────────────────────────────────────────────────────
// Private helpers
// ────────────────────────────────────────────────────────────────────────────

/// Detect the clausula type from a flat stress vector.
fn detect_clausula(stress: &[f32]) -> Clausula {
    match stress.iter().rposition(|&s| s > 0.5) {
        None => Clausula::Masculine, // no stress at all → treat as masculine
        Some(last) => {
            let tail = stress.len().saturating_sub(1).saturating_sub(last);
            match tail {
                0 => Clausula::Masculine,
                1 => Clausula::Feminine,
                2 => Clausula::Dactylic,
                _ => Clausula::Hyperdactylic,
            }
        }
    }
}

/// Fraction of syllables whose actual stress matches the given period + phase.
fn period_confidence(stress: &[f32], period: u8, phase: u8) -> f32 {
    let n = stress.len();
    if n == 0 {
        return 0.0;
    }
    let matches = stress
        .iter()
        .enumerate()
        .filter(|&(i, &s)| {
            let expected_stressed = (i % period as usize) == phase as usize;
            (s > 0.5) == expected_stressed
        })
        .count();
    matches as f32 / n as f32
}

/// Try all (period, phase) combinations; return the one with highest confidence.
fn detect_period(stress: &[f32]) -> (u8, u8, f32) {
    let mut best = (2u8, 0u8, 0.0_f32);
    for period in [2u8, 3u8] {
        for phase in 0..period {
            let conf = period_confidence(stress, period, phase);
            if conf > best.2 {
                best = (period, phase, conf);
            }
        }
    }
    best
}

// ────────────────────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────────────────────

/// Analyse the rhythmic stress pattern of one line.
///
/// `line_words` must contain all words of the line **in order**.
/// Returns a [`LineRhythm`] whose `syllables` vec can be used directly by
/// the frontend to colour-code each syllable by stress deviation.
pub fn analyze_line_rhythm(line_words: &[&IpaStreamWord], line_index: usize) -> LineRhythm {
    // ── Flatten syllables from all words ─────────────────────────────────
    let mut flat: Vec<(SyllableRef, f32)> = Vec::new();
    for word in line_words {
        for (syl_idx, syl) in word.syllables.iter().enumerate() {
            flat.push((
                SyllableRef {
                    word_id:        word.id.clone(),
                    syllable_index: syl_idx,
                    line_position:  flat.len(),
                },
                if syl.stressed { 1.0 } else { 0.0 },
            ));
        }
    }

    let n = flat.len();
    if n == 0 {
        return LineRhythm {
            line_index,
            period:         2,
            phase:          0,
            clausula:       Clausula::Masculine,
            confidence:     0.0,
            syllables:      vec![],
            syllable_count: 0,
        };
    }

    // ── Detect metre ──────────────────────────────────────────────────────
    let stress_vec: Vec<f32> = flat.iter().map(|(_, s)| *s).collect();
    let (period, phase, confidence) = detect_period(&stress_vec);
    let clausula = detect_clausula(&stress_vec);

    // ── Annotate each syllable ────────────────────────────────────────────
    let syllables = flat
        .into_iter()
        .map(|(sref, actual)| {
            let expected = if (sref.line_position % period as usize) == phase as usize {
                1.0
            } else {
                0.0
            };
            let deviation = match (actual > 0.5, expected > 0.5) {
                (true, true) | (false, false) => DeviationType::Match,
                (false, true)                 => DeviationType::Pyrrhic,
                (true, false)                 => DeviationType::Spondee,
            };
            SyllableAnnotation { syllable_ref: sref, stress: actual, expected, deviation }
        })
        .collect();

    LineRhythm { line_index, period, phase, clausula, confidence, syllables, syllable_count: n }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{IpaStreamSyllable, IpaStreamWord, StressSource};

    // ── Test helpers ──────────────────────────────────────────────────────

    fn make_syl(stressed: bool) -> IpaStreamSyllable {
        IpaStreamSyllable {
            ipa:      String::new(),
            tokens:   vec![],
            grapheme: String::new(),
            stressed,
            is_open:  false,
        }
    }

    fn make_word(id: &str, stressed_syl: i32, syls: Vec<IpaStreamSyllable>) -> IpaStreamWord {
        IpaStreamWord {
            id:               id.to_string(),
            line_index:       0,
            word_index:       0,
            language:         "uk".to_string(),
            original:         String::new(),
            syllable_count:   syls.len(),
            stressed_syllable: stressed_syl,
            stress_source:    StressSource::Dict,
            syllables:        syls,
        }
    }

    // Build a line from a stress pattern string like "010101"
    fn line_from_pattern(pattern: &str) -> Vec<IpaStreamWord> {
        pattern
            .char_indices()
            .map(|(i, c)| {
                let stressed = c == '1';
                make_word(&format!("w{i}"), if stressed { 0 } else { -1 }, vec![make_syl(stressed)])
            })
            .collect()
    }

    fn refs(words: &[IpaStreamWord]) -> Vec<&IpaStreamWord> {
        words.iter().collect()
    }

    // ── Period / phase detection ──────────────────────────────────────────

    #[test]
    fn test_iambic_pattern_detected() {
        // 0 1 0 1 0 1 → period=2, phase=1
        let words = line_from_pattern("010101");
        let result = analyze_line_rhythm(&refs(&words), 0);
        assert_eq!(result.period, 2);
        assert_eq!(result.phase, 1);
    }

    #[test]
    fn test_trochaic_pattern_detected() {
        // 1 0 1 0 1 0 → period=2, phase=0
        let words = line_from_pattern("101010");
        let result = analyze_line_rhythm(&refs(&words), 0);
        assert_eq!(result.period, 2);
        assert_eq!(result.phase, 0);
    }

    #[test]
    fn test_ternary_anapest_detected() {
        // 0 0 1 0 0 1 → period=3, phase=2
        let words = line_from_pattern("001001");
        let result = analyze_line_rhythm(&refs(&words), 0);
        assert_eq!(result.period, 3);
        assert_eq!(result.phase, 2);
    }

    #[test]
    fn test_ternary_dactyl_detected() {
        // 1 0 0 1 0 0 → period=3, phase=0
        let words = line_from_pattern("100100");
        let result = analyze_line_rhythm(&refs(&words), 0);
        assert_eq!(result.period, 3);
        assert_eq!(result.phase, 0);
    }

    // ── Confidence ────────────────────────────────────────────────────────

    #[test]
    fn test_perfect_iambic_confidence_is_one() {
        let words = line_from_pattern("010101");
        let result = analyze_line_rhythm(&refs(&words), 0);
        assert!((result.confidence - 1.0).abs() < 1e-6, "got {}", result.confidence);
    }

    #[test]
    fn test_pyrrhic_lowers_confidence() {
        // 0 1 0 0 0 1 — one pyrrhic at position 3 (expected 1, got 0)
        let words = line_from_pattern("010001");
        let result = analyze_line_rhythm(&refs(&words), 0);
        assert!(result.confidence < 1.0, "confidence should drop below 1.0");
        assert!(result.confidence > 0.0, "should still detect some pattern");
    }

    // ── Deviation types ───────────────────────────────────────────────────

    #[test]
    fn test_match_syllables_annotated() {
        let words = line_from_pattern("0101");
        let result = analyze_line_rhythm(&refs(&words), 0);
        // All four syllables should match the iambic pattern
        for syl in &result.syllables {
            assert_eq!(syl.deviation, DeviationType::Match, "pos={}", syl.syllable_ref.line_position);
        }
    }

    #[test]
    fn test_pyrrhic_deviation_detected() {
        // period=2, phase=1 → stress expected at positions 1, 3
        // Pattern 010001 → position 3 expected=1 actual=0 → Pyrrhic
        let words = line_from_pattern("010001");
        let result = analyze_line_rhythm(&refs(&words), 0);
        let pyrrhics: Vec<_> = result
            .syllables
            .iter()
            .filter(|s| s.deviation == DeviationType::Pyrrhic)
            .collect();
        assert!(!pyrrhics.is_empty(), "expected at least one pyrrhic");
    }

    #[test]
    fn test_spondee_deviation_detected() {
        // period=2, phase=1 (iambic) → position 0 expected=0 actual=1 → Spondee
        let words = line_from_pattern("110101");
        let result = analyze_line_rhythm(&refs(&words), 0);
        let spondees: Vec<_> = result
            .syllables
            .iter()
            .filter(|s| s.deviation == DeviationType::Spondee)
            .collect();
        assert!(!spondees.is_empty(), "expected at least one spondee");
    }

    // ── Clausula ──────────────────────────────────────────────────────────

    #[test]
    fn test_clausula_masculine() {
        // 0101 → last stress at position 3 (tail=0)
        let words = line_from_pattern("0101");
        let result = analyze_line_rhythm(&refs(&words), 0);
        assert_eq!(result.clausula, Clausula::Masculine);
    }

    #[test]
    fn test_clausula_feminine() {
        // 01010 → last stress at position 3 (tail=1)
        let words = line_from_pattern("01010");
        let result = analyze_line_rhythm(&refs(&words), 0);
        assert_eq!(result.clausula, Clausula::Feminine);
    }

    #[test]
    fn test_clausula_dactylic() {
        // 010100 → last stress at position 3 (tail=2)
        let words = line_from_pattern("010100");
        let result = analyze_line_rhythm(&refs(&words), 0);
        assert_eq!(result.clausula, Clausula::Dactylic);
    }

    // ── SyllableRef mapping ───────────────────────────────────────────────

    #[test]
    fn test_syllable_ref_word_id_preserved() {
        // Two 2-syllable words → 4 syllables total
        let w1 = make_word("tok-1", 1, vec![make_syl(false), make_syl(true)]);
        let w2 = make_word("tok-2", 1, vec![make_syl(false), make_syl(true)]);
        let line: Vec<&IpaStreamWord> = vec![&w1, &w2];
        let result = analyze_line_rhythm(&line, 0);

        // First two syllables belong to tok-1
        assert_eq!(result.syllables[0].syllable_ref.word_id, "tok-1");
        assert_eq!(result.syllables[1].syllable_ref.word_id, "tok-1");
        // Last two syllables belong to tok-2
        assert_eq!(result.syllables[2].syllable_ref.word_id, "tok-2");
        assert_eq!(result.syllables[3].syllable_ref.word_id, "tok-2");
    }

    #[test]
    fn test_syllable_ref_syllable_index_correct() {
        let w = make_word("tok-1", 1, vec![make_syl(false), make_syl(true)]);
        let result = analyze_line_rhythm(&[&w], 0);
        assert_eq!(result.syllables[0].syllable_ref.syllable_index, 0);
        assert_eq!(result.syllables[1].syllable_ref.syllable_index, 1);
    }

    #[test]
    fn test_syllable_ref_line_position_sequential() {
        let w1 = make_word("a", 0, vec![make_syl(true), make_syl(false)]);
        let w2 = make_word("b", 0, vec![make_syl(true), make_syl(false)]);
        let result = analyze_line_rhythm(&[&w1, &w2], 0);
        for (i, syl) in result.syllables.iter().enumerate() {
            assert_eq!(syl.syllable_ref.line_position, i);
        }
    }

    #[test]
    fn test_syllable_count_matches_input() {
        let w = make_word("w", 1, vec![make_syl(false), make_syl(true), make_syl(false)]);
        let result = analyze_line_rhythm(&[&w], 0);
        assert_eq!(result.syllable_count, 3);
        assert_eq!(result.syllables.len(), 3);
    }

    // ── Edge cases ────────────────────────────────────────────────────────

    #[test]
    fn test_empty_line_returns_zero_confidence() {
        let result = analyze_line_rhythm(&[], 0);
        assert_eq!(result.confidence, 0.0);
        assert_eq!(result.syllable_count, 0);
    }

    #[test]
    fn test_single_syllable_line() {
        let w = make_word("w", 0, vec![make_syl(true)]);
        let result = analyze_line_rhythm(&[&w], 0);
        assert_eq!(result.syllable_count, 1);
        assert_eq!(result.syllables.len(), 1);
    }

    #[test]
    fn test_line_index_preserved() {
        let w = make_word("w", 0, vec![make_syl(true)]);
        let result = analyze_line_rhythm(&[&w], 7);
        assert_eq!(result.line_index, 7);
    }
}
