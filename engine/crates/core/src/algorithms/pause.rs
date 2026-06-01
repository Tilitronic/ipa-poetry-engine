//! `pause.rs` — prosodic pause detection from IPA Stream v1.1.
//!
//! Pauses are prosodic boundaries created by:
//! - **Punctuation** (`,` `;` `:` `.` `?` `!` `—` `–` …)
//! - **Line breaks** (structural verse boundaries)
//! - **Both** at once (most common in Ukrainian verse: `слово,\n`)
//!
//! Each [`PauseAnnotation`] records the word that precedes the pause and a
//! normalised `strength` in `[0.0, 1.0]`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use crate::stream::{IpaStream, StreamElement};

// ────────────────────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────────────────────

/// Strength of a prosodic pause (0 = no pause, 1 = full stop).
///
/// | Source                     | Strength |
/// |----------------------------|----------|
/// | bare line break            | 0.35     |
/// | `,`                        | 0.25     |
/// | `;` / `:`                  | 0.50     |
/// | `—` / `–`                 | 0.60     |
/// | `.` / `?` / `!`           | 0.75     |
/// | other punctuation          | 0.40     |
/// | any punct **+ line break** | +0.15    |
fn punct_base_strength(text: &str) -> f32 {
    match text {
        ","                 => 0.25,
        ";" | ":"           => 0.50,
        "." | "?" | "!"    => 0.75,
        "—" | "–" | "‒"   => 0.60,
        _                   => 0.40,
    }
}

/// Compute pause strength from punctuation text and whether a line break follows.
pub fn pause_strength(punct: Option<&str>, has_line_break: bool) -> f32 {
    match (punct, has_line_break) {
        (Some(p), true)  => (punct_base_strength(p) + 0.15).min(1.0),
        (Some(p), false) => punct_base_strength(p),
        (None,    true)  => 0.35,
        (None,    false) => 0.0,
    }
}

/// A prosodic pause detected in the stream.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PauseAnnotation {
    /// The `id` of the word immediately before this pause.
    pub after_word_id: String,

    /// The punctuation character(s) that precede the pause, if any.
    pub punctuation: Option<String>,

    /// Whether a line break immediately follows the punctuation (or the word).
    pub has_line_break: bool,

    /// Normalised pause strength in `[0.0, 1.0]`.
    pub strength: f32,
}

// ────────────────────────────────────────────────────────────────────────────
// Core algorithm
// ────────────────────────────────────────────────────────────────────────────

/// Scan the stream and emit a [`PauseAnnotation`] for every prosodic boundary.
///
/// Traversal rules (state machine over [`StreamElement`]):
///
/// * On `Word` — save it as `last_word_id`.  If a pending punctuation was
///   accumulated since the previous word **without** a line break, emit that
///   punctuation-only pause before advancing.
/// * On `Punctuation` — accumulate `pending_punct`.
/// * On `LineBreak` — emit a pause for the current `last_word_id` (with
///   `pending_punct` if any) and reset state.
/// * `Whitespace` — ignored.
///
/// After the stream ends, any trailing punctuation without a following line
/// break is emitted as a final pause (e.g. terminal `.`).
pub fn collect_pauses(stream: &IpaStream) -> Vec<PauseAnnotation> {
    let mut pauses: Vec<PauseAnnotation> = Vec::new();
    let mut last_word_id: Option<String> = None;
    let mut pending_punct: Option<String> = None;

    for elem in &stream.stream {
        match elem {
            StreamElement::Word(w) => {
                // A new word starts. If we had punctuation pending (mid-line),
                // emit it now — it was a pause between the previous and this word.
                if let (Some(wid), Some(punct)) = (last_word_id.take(), pending_punct.take()) {
                    let strength = pause_strength(Some(&punct), false);
                    pauses.push(PauseAnnotation {
                        after_word_id: wid,
                        punctuation: Some(punct),
                        has_line_break: false,
                        strength,
                    });
                }
                // last_word_id is already None after .take() above in either branch
                last_word_id = Some(w.id.clone());
            }

            StreamElement::Punctuation { text } => {
                // If there's already a pending punct (shouldn't normally happen),
                // flush it as a mid-line pause.
                if let (Some(wid), Some(old)) = (last_word_id.clone(), pending_punct.take()) {
                    let strength = pause_strength(Some(&old), false);
                    pauses.push(PauseAnnotation {
                        after_word_id: wid,
                        punctuation: Some(old),
                        has_line_break: false,
                        strength,
                    });
                }
                pending_punct = Some(text.clone());
            }

            StreamElement::LineBreak { .. } => {
                // Emit pause for the preceding word (with or without punctuation).
                if let Some(wid) = last_word_id.take() {
                    let punct_ref = pending_punct.as_deref();
                    let strength = pause_strength(punct_ref, true);
                    pauses.push(PauseAnnotation {
                        after_word_id: wid,
                        punctuation: pending_punct.take(),
                        has_line_break: true,
                        strength,
                    });
                }
                // If no last_word_id (e.g. opening line_break), drop everything.
                pending_punct = None;
            }

            StreamElement::Whitespace => {}
        }
    }

    // Trailing punctuation at end of stream (e.g. terminal `.`).
    if let (Some(wid), Some(punct)) = (last_word_id, pending_punct) {
        let strength = pause_strength(Some(&punct), false);
        pauses.push(PauseAnnotation {
            after_word_id: wid,
            punctuation: Some(punct),
            has_line_break: false,
            strength,
        });
    }

    pauses
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::IpaStream;

    fn stream(json: &str) -> IpaStream {
        IpaStream::from_json_bytes(json.as_bytes()).unwrap()
    }

    fn simple_word(id: &str, line_index: usize) -> String {
        format!(r#"{{"type":"word","id":"{id}","lineIndex":{line_index},"wordIndex":0,"language":"ua","original":"x","syllableCount":1,"stressedSyllable":0,"stressSource":"manual","syllables":[{{"ipa":"a","tokens":["a"],"grapheme":"a","stressed":true,"isOpen":true}}]}}"#)
    }

    // ── No pauses ────────────────────────────────────────────────────────────

    #[test]
    fn test_no_pauses_in_plain_words() {
        // w1 → whitespace → w2 → line_break
        // Only a bare line_break pause after w2 (strength 0.35). No pause between words.
        let json = format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00Z","confirmedLineCount":1,"totalWordCount":2,"languagesPresent":["ua"]}},"stream":[{w1},{ws},{w2},{lb}]}}"#,
            w1 = simple_word("w1", 0),
            ws = r#"{"type":"whitespace"}"#,
            w2 = simple_word("w2", 0),
            lb = r#"{"type":"line_break","lineIndex":0}"#,
        );
        let s = stream(&json);
        let pauses = collect_pauses(&s);
        assert_eq!(pauses.len(), 1);
        assert_eq!(pauses[0].after_word_id, "w2");
        assert_eq!(pauses[0].punctuation, None);
        assert!(pauses[0].has_line_break);
        assert!((pauses[0].strength - 0.35).abs() < 1e-5);
    }

    // ── Comma mid-line ───────────────────────────────────────────────────────

    #[test]
    fn test_comma_midline_pause() {
        // w1 , w2 → pause after w1 with comma, strength 0.25
        let json = format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00Z","confirmedLineCount":1,"totalWordCount":2,"languagesPresent":["ua"]}},"stream":[{w1},{p},{ws},{w2},{lb}]}}"#,
            w1 = simple_word("w1", 0),
            p  = r#"{"type":"punctuation","text":","}"#,
            ws = r#"{"type":"whitespace"}"#,
            w2 = simple_word("w2", 0),
            lb = r#"{"type":"line_break","lineIndex":0}"#,
        );
        let s = stream(&json);
        let pauses = collect_pauses(&s);
        // pause after w1 (comma, no line_break) + pause after w2 (line_break)
        assert_eq!(pauses.len(), 2);
        let p0 = &pauses[0];
        assert_eq!(p0.after_word_id, "w1");
        assert_eq!(p0.punctuation.as_deref(), Some(","));
        assert!(!p0.has_line_break);
        assert!((p0.strength - 0.25).abs() < 1e-5);
    }

    // ── Comma + line break ────────────────────────────────────────────────────

    #[test]
    fn test_comma_plus_line_break() {
        // w1 , \n  → strength 0.25 + 0.15 = 0.40
        let json = format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00Z","confirmedLineCount":1,"totalWordCount":1,"languagesPresent":["ua"]}},"stream":[{w1},{p},{lb}]}}"#,
            w1 = simple_word("w1", 0),
            p  = r#"{"type":"punctuation","text":","}"#,
            lb = r#"{"type":"line_break","lineIndex":0}"#,
        );
        let s = stream(&json);
        let pauses = collect_pauses(&s);
        assert_eq!(pauses.len(), 1);
        assert_eq!(pauses[0].after_word_id, "w1");
        assert_eq!(pauses[0].punctuation.as_deref(), Some(","));
        assert!(pauses[0].has_line_break);
        assert!((pauses[0].strength - 0.40).abs() < 1e-5);
    }

    // ── Em-dash + line break ──────────────────────────────────────────────────

    #[test]
    fn test_em_dash_plus_line_break() {
        // w1 — \n  → strength 0.60 + 0.15 = 0.75
        let json = format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00Z","confirmedLineCount":1,"totalWordCount":1,"languagesPresent":["ua"]}},"stream":[{w1},{p},{lb}]}}"#,
            w1 = simple_word("w1", 0),
            p  = r#"{"type":"punctuation","text":"—"}"#,
            lb = r#"{"type":"line_break","lineIndex":0}"#,
        );
        let s = stream(&json);
        let pauses = collect_pauses(&s);
        assert_eq!(pauses.len(), 1);
        assert!(pauses[0].has_line_break);
        assert_eq!(pauses[0].punctuation.as_deref(), Some("—"));
        assert!((pauses[0].strength - 0.75).abs() < 1e-5);
    }

    // ── Period at end of stream ───────────────────────────────────────────────

    #[test]
    fn test_terminal_period_no_line_break() {
        // w1 .   (no line_break after)
        let json = format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00Z","confirmedLineCount":1,"totalWordCount":1,"languagesPresent":["ua"]}},"stream":[{w1},{p}]}}"#,
            w1 = simple_word("w1", 0),
            p  = r#"{"type":"punctuation","text":"."}"#,
        );
        let s = stream(&json);
        let pauses = collect_pauses(&s);
        assert_eq!(pauses.len(), 1);
        assert_eq!(pauses[0].punctuation.as_deref(), Some("."));
        assert!(!pauses[0].has_line_break);
        assert!((pauses[0].strength - 0.75).abs() < 1e-5);
    }

    // ── Bare line break (no punctuation) ────────────────────────────────────

    #[test]
    fn test_bare_line_break_strength() {
        let json = format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00Z","confirmedLineCount":1,"totalWordCount":1,"languagesPresent":["ua"]}},"stream":[{w1},{lb}]}}"#,
            w1 = simple_word("w1", 0),
            lb = r#"{"type":"line_break","lineIndex":0}"#,
        );
        let s = stream(&json);
        let pauses = collect_pauses(&s);
        assert_eq!(pauses.len(), 1);
        assert_eq!(pauses[0].punctuation, None);
        assert!(pauses[0].has_line_break);
        assert!((pauses[0].strength - 0.35).abs() < 1e-5);
    }

    // ── Period + line break capped at 1.0 ────────────────────────────────────

    #[test]
    fn test_period_plus_line_break_capped() {
        // . + line_break → 0.75 + 0.15 = 0.90 (not capped)
        let json = format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00Z","confirmedLineCount":1,"totalWordCount":1,"languagesPresent":["ua"]}},"stream":[{w1},{p},{lb}]}}"#,
            w1 = simple_word("w1", 0),
            p  = r#"{"type":"punctuation","text":"."}"#,
            lb = r#"{"type":"line_break","lineIndex":0}"#,
        );
        let s = stream(&json);
        let pauses = collect_pauses(&s);
        assert!((pauses[0].strength - 0.90).abs() < 1e-5);
    }

    // ── Multiple pauses in sequence ──────────────────────────────────────────

    #[test]
    fn test_multiple_pauses_across_lines() {
        // w1 , \n  w2 — \n
        let json = format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00Z","confirmedLineCount":2,"totalWordCount":2,"languagesPresent":["ua"]}},"stream":[{w1},{c},{lb0},{w2},{d},{lb1}]}}"#,
            w1  = simple_word("w1", 0),
            c   = r#"{"type":"punctuation","text":","}"#,
            lb0 = r#"{"type":"line_break","lineIndex":0}"#,
            w2  = simple_word("w2", 1),
            d   = r#"{"type":"punctuation","text":"—"}"#,
            lb1 = r#"{"type":"line_break","lineIndex":1}"#,
        );
        let s = stream(&json);
        let pauses = collect_pauses(&s);
        assert_eq!(pauses.len(), 2);
        assert_eq!(pauses[0].after_word_id, "w1");
        assert!((pauses[0].strength - 0.40).abs() < 1e-5); // comma + line_break
        assert_eq!(pauses[1].after_word_id, "w2");
        assert!((pauses[1].strength - 0.75).abs() < 1e-5); // dash + line_break
    }

    // ── pause_strength helper ────────────────────────────────────────────────

    #[test]
    fn test_pause_strength_helper() {
        assert!((pause_strength(Some(","),  false) - 0.25).abs() < 1e-5);
        assert!((pause_strength(Some(";"),  false) - 0.50).abs() < 1e-5);
        assert!((pause_strength(Some("—"), false) - 0.60).abs() < 1e-5);
        assert!((pause_strength(Some("."),  false) - 0.75).abs() < 1e-5);
        assert!((pause_strength(None,       true)  - 0.35).abs() < 1e-5);
        assert!((pause_strength(Some(","),  true)  - 0.40).abs() < 1e-5);
        assert!((pause_strength(Some("."),  true)  - 0.90).abs() < 1e-5);
        assert!((pause_strength(None,       false) - 0.00).abs() < 1e-5);
    }
}
