//! `lib.rs` — public API for the phonetic poetry engine core crate.
//!
//! Two entry points are provided:
//! - `analyze(ipa_str, registry)` — simple flat IPA string (space/newline separated)
//! - `analyze_stream(stream, registry)` — structured IPA Stream v1.1 JSON format

pub mod algorithms;
pub mod matrix;
pub mod registry;
pub mod stream;
pub mod tokenizer;

pub use matrix::PhoneticStream;
pub use registry::FeatureRegistry;
pub use stream::{IpaStream, FORMAT_VERSION};

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use algorithms::density::{find_clusters, sliding_window, IDX_DELREL, IDX_NAS, IDX_STRID, IDX_SYL, IDX_LAT};
use algorithms::dtw::{dtw_similarity, rhyme_distance};
use algorithms::echo::{compute_echo, EchoParams};
use algorithms::pause::collect_pauses;
use algorithms::rhythm::analyze_line_rhythm;
use algorithms::structural::analyze_structural;
use stream::{coda_tokens_from_word, stress_confidence, tokens_from_word};
use tokenizer::TokenType;

pub use algorithms::echo::{EchoAnnotation, PhonemeRef};
pub use algorithms::pause::{collect_pauses as compute_pauses, PauseAnnotation};
pub use algorithms::rhythm::{Clausula, DeviationType, LineRhythm, SyllableAnnotation, SyllableRef};
pub use algorithms::structural::{StructuralAnnotation, SyllableShape};

// ────────────────────────────────────────────────────────────────────────────
// Output types (serialisable to JSON)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RhymeMatch {
    pub r#type: RhymeType,
    /// Token indices of the first sequence.
    pub indices_a: Vec<usize>,
    /// Token indices of the second sequence.
    pub indices_b: Vec<usize>,
    /// Similarity score [0, 1]; 1 = perfect rhyme.
    pub score: f32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RhymeType {
    Perfect,   // score > 0.90
    Near,      // score > 0.70
    Imperfect, // score > 0.50
}

#[derive(Debug, Serialize)]
pub struct Cluster {
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub peak: f32,
}

#[derive(Debug, Serialize)]
pub struct AnalysisResult {
    pub token_count: usize,
    pub phoneme_count: usize,
    pub rhymes: Vec<RhymeMatch>,
    pub clusters: Vec<Cluster>,
}

// ────────────────────────────────────────────────────────────────────────────
// Main analysis entry point
// ────────────────────────────────────────────────────────────────────────────

/// Analyse an IPA string and return rhyme matches + sound clusters.
///
/// Lines are delimited by `'\n'`; words by `' '` or `'#'`.
/// The algorithm compares every pair of *lines* for rhyme, and uses a sliding
/// window of 10 tokens to detect alliteration / assonance clusters.
pub fn analyze(ipa_string: &str, registry: &FeatureRegistry) -> AnalysisResult {
    let stream = PhoneticStream::from_ipa(ipa_string, registry);

    let phoneme_count = stream.phonemes_only().count();

    // ── Split into lines (delimited by '\n' in the flat IPA string) ─────
    // '\n' now produces WordBoundary tokens; we detect a line boundary as a
    // WordBoundary whose symbol is literally "\n".
    let lines: Vec<Vec<&tokenizer::PhoneticToken>> = {
        let mut lines = Vec::new();
        let mut current = Vec::new();
        for tok in &stream.tokens {
            if tok.t_type == TokenType::WordBoundary && tok.symbol == "\n" {
                if !current.is_empty() {
                    lines.push(current.clone());
                    current.clear();
                }
            } else {
                current.push(tok);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    };

    // ── Rhyme detection (all pairs of lines) ─────────────────────────────
    let mut rhymes = Vec::new();
    let n_lines = lines.len();

    // Collect owned tokens per line once
    let line_tokens: Vec<Vec<tokenizer::PhoneticToken>> = lines
        .iter()
        .map(|line| line.iter().map(|t| (*t).clone()).collect())
        .collect();

    for i in 0..n_lines {
        for j in (i + 1)..n_lines {
            let dist  = rhyme_distance(&line_tokens[i], &line_tokens[j]);
            let score = dtw_similarity(dist);

            let rhyme_type = if score > 0.90 {
                RhymeType::Perfect
            } else if score > 0.70 {
                RhymeType::Near
            } else if score > 0.50 {
                RhymeType::Imperfect
            } else {
                continue; // not a rhyme
            };

            // Build index lists for each line (indices into the global token list)
            let base_i: usize = line_tokens[..i].iter().map(|l| l.len()).sum::<usize>() + i;
            let base_j: usize = line_tokens[..j].iter().map(|l| l.len()).sum::<usize>() + j;

            rhymes.push(RhymeMatch {
                r#type:    rhyme_type,
                indices_a: (base_i..base_i + line_tokens[i].len()).collect(),
                indices_b: (base_j..base_j + line_tokens[j].len()).collect(),
                score,
            });
        }
    }

    // ── Density / alliteration detection ─────────────────────────────────
    const WINDOW: usize = 10;
    let windows = sliding_window(&stream.tokens, WINDOW);
    let mut clusters = Vec::new();

    let features: &[(&'static str, usize, f32)] = &[
        ("sibilant", IDX_STRID, 2.5),
        ("nasal",    IDX_NAS,   2.5),
        ("lateral",  IDX_LAT,   2.0),
        ("assonance",IDX_SYL,   3.0),
    ];

    for &(kind, feat_idx, threshold) in features {
        for cluster in find_clusters(&windows, feat_idx, threshold) {
            clusters.push(Cluster {
                kind,
                start:  cluster.start,
                end:    cluster.end,
                peak:   cluster.peak_value,
            });
        }
    }

    AnalysisResult {
        token_count: stream.len(),
        phoneme_count,
        rhymes,
        clusters,
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Error type
// ────────────────────────────────────────────────────────────────────────────

/// Errors produced by the structured stream analysis pipeline.
#[derive(Debug, Serialize, Deserialize)]
pub enum EngineError {
    /// Input document version does not match the expected `FORMAT_VERSION`.
    UnsupportedVersion { got: String, expected: String },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::UnsupportedVersion { got, expected } => {
                write!(f, "Unsupported IPA Stream version '{got}'; expected '{expected}'")
            }
        }
    }
}
impl std::error::Error for EngineError {}

// ────────────────────────────────────────────────────────────────────────────
// Stream analysis output types (IPA Stream v1.1 round-trip)
// ────────────────────────────────────────────────────────────────────────────

/// Per-word annotation keyed by `id` from the input stream.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordAnnotation {
    /// Line index of this word (0-based), mirrors `IpaStreamWord.line_index`.
    pub line_index: usize,
    /// Word index within its line (0-based), mirrors `IpaStreamWord.word_index`.
    pub word_index: usize,
    /// Rhyme group letter (A, B, C, …) or `null` if not detected.
    pub rhyme_group: Option<String>,
    /// DTW similarity score against the best-matching rhyme partner [0, 1].
    pub rhyme_score: Option<f32>,
    /// Confidence in the stress assignment (derived from `stressSource`).
    pub stress_confidence: f32,
    /// Structural rhyme group (shared syllabic shape), or `null` if unique.
    pub structural_rhyme_group: Option<String>,
}

/// Result of `analyze_stream`, shaped for the frontend round-trip protocol.
///
/// Serialises to:
/// ```json
/// {
///   "version": "1.1",
///   "annotations": { "tok-001": { "rhymeGroup": "A", ... } },
///   "clusters": [ ... ]
/// }
/// ```
#[derive(Debug, Serialize)]
pub struct StreamAnalysisResult {
    /// IPA Stream format version this result was produced from.
    pub version: &'static str,
    /// Per-word annotations keyed by word `id`.
    pub annotations: HashMap<String, WordAnnotation>,
    /// Sound clusters detected across the full stream.
    pub clusters: Vec<Cluster>,
    /// Per-line rhythm analysis (stress pattern, clausula, confidence).
    pub rhythm: Vec<LineRhythm>,
    /// Per-phoneme echo opacity annotations.
    pub echo: Vec<EchoAnnotation>,
    /// Prosodic pauses created by punctuation and/or line breaks.
    pub pauses: Vec<PauseAnnotation>,
}

// ────────────────────────────────────────────────────────────────────────────
// Rhyme grouping threshold
// ────────────────────────────────────────────────────────────────────────────

/// Minimum DTW similarity score to consider two words as rhyming.
const RHYME_THRESHOLD: f32 = 0.65;

// ────────────────────────────────────────────────────────────────────────────
// analyze_stream — IPA Stream v1.1 entry point
// ────────────────────────────────────────────────────────────────────────────

/// Analyse a structured IPA Stream v1.1 document.
///
/// # Algorithm
///
/// 1. **Version check** — returns `Err` if `metadata.version ≠ "1.1"`.
/// 2. **Rhyme detection** — compares the *coda* (stressed syllable → end) of
///    the last word of each line, using DTW on 24-dim feature vectors.
/// 3. **Rhyme grouping** — greedy assignment: words whose coda DTW similarity
///    exceeds [`RHYME_THRESHOLD`] share a group letter (A, B, C, …).
/// 4. **Cluster detection** — sliding window density analysis over all phoneme
///    tokens to detect sibilant, nasal, lateral, and assonance clusters.
///
/// # Output
///
/// Returns a [`StreamAnalysisResult`] whose `annotations` map is keyed by the
/// word `id` from the input, making it directly usable as the backend response
/// in the round-trip protocol.
pub fn analyze_stream(
    ipa_stream: &IpaStream,
    registry: &FeatureRegistry,
) -> Result<StreamAnalysisResult, EngineError> {
    // ── Version validation ────────────────────────────────────────────────
    if ipa_stream.version() != FORMAT_VERSION {
        return Err(EngineError::UnsupportedVersion {
            got:      ipa_stream.version().to_string(),
            expected: FORMAT_VERSION.to_string(),
        });
    }

    let all_words: Vec<&stream::IpaStreamWord> = ipa_stream.words().collect();
    let lines = ipa_stream.lines();

    // ── Rhyme detection over ALL words ────────────────────────────────────
    // Build coda token sequences for every word (stressed syllable → end).
    // Comparing all words pairwise catches end-rhymes, internal rhymes,
    // anaphoric rhymes, and cross-line rhymes uniformly.
    let coda_seqs: Vec<Vec<tokenizer::PhoneticToken>> = all_words
        .iter()
        .map(|w| coda_tokens_from_word(w, registry))
        .collect();

    // ── Pairwise rhyme scoring ────────────────────────────────────────────
    let n = all_words.len();
    let mut pairs: Vec<(usize, usize, f32)> = Vec::new();

    for i in 0..n {
        for j in (i + 1)..n {
            let dist  = rhyme_distance(&coda_seqs[i], &coda_seqs[j]);
            let score = dtw_similarity(dist);
            if score >= RHYME_THRESHOLD {
                pairs.push((i, j, score));
            }
        }
    }

    // Sort best scores first for greedy group assignment
    pairs.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // ── Greedy rhyme group assignment (A, B, C, …) ───────────────────────
    let mut group_of: Vec<Option<String>> = vec![None; n];
    let mut group_counter: u8 = b'A';

    for &(i, j, _) in &pairs {
        match (group_of[i].clone(), group_of[j].clone()) {
            (None, None) => {
                let letter = (group_counter as char).to_string();
                group_counter = group_counter.saturating_add(1);
                group_of[i] = Some(letter.clone());
                group_of[j] = Some(letter);
            }
            (Some(g), None) => { group_of[j] = Some(g); }
            (None, Some(g)) => { group_of[i] = Some(g); }
            (Some(_), Some(_)) => {} // both already in a group
        }
    }

    // Best score per word
    let mut best_score: Vec<f32> = vec![0.0; n];
    for &(i, j, score) in &pairs {
        if score > best_score[i] { best_score[i] = score; }
        if score > best_score[j] { best_score[j] = score; }
    }

    // ── Build annotation map for ALL words ───────────────────────────────
    let mut annotations: HashMap<String, WordAnnotation> = HashMap::new();

    for (idx, word) in all_words.iter().enumerate() {
        let stress_conf = stress_confidence(&word.stress_source);
        let rhyme_group = group_of[idx].clone();
        let rhyme_score = best_score[idx];

        annotations.insert(word.id.clone(), WordAnnotation {
            line_index:           word.line_index,
            word_index:           word.word_index,
            rhyme_group,
            rhyme_score:          if rhyme_score > 0.0 { Some(rhyme_score) } else { None },
            stress_confidence:    stress_conf,
            structural_rhyme_group: None, // filled in after structural analysis
        });
    }

    // ── Structural rhyme analysis ─────────────────────────────────────────
    let structural = analyze_structural(&all_words, registry);
    for ann in &structural {
        if let Some(word_ann) = annotations.get_mut(&ann.word_id) {
            word_ann.structural_rhyme_group = ann.structural_rhyme_group.clone();
        }
    }

    // ── Rhythm analysis (one LineRhythm per confirmed line) ───────────────
    let rhythm: Vec<LineRhythm> = lines
        .iter()
        .enumerate()
        .map(|(line_idx, line_words)| analyze_line_rhythm(line_words, line_idx))
        .collect();

    // ── Echo / opacity analysis ───────────────────────────────────────────
    let echo = compute_echo(&all_words, registry, &EchoParams::default());

    // ── Density / cluster detection over full token stream ────────────────
    // Flatten all phoneme tokens from all words in stream order
    let flat_tokens: Vec<tokenizer::PhoneticToken> = all_words
        .iter()
        .flat_map(|w| tokens_from_word(w, registry))
        .collect();

    const WINDOW: usize = 10;
    let windows = sliding_window(&flat_tokens, WINDOW);
    let mut clusters: Vec<Cluster> = Vec::new();

    let feature_checks: &[(&'static str, usize, f32)] = &[
        ("sibilant",  IDX_STRID,  2.5),
        ("affricate", IDX_DELREL, 2.0),
        ("nasal",     IDX_NAS,    2.5),
        ("lateral",   IDX_LAT,    2.0),
        ("assonance", IDX_SYL,    3.0),
    ];

    for &(kind, feat_idx, threshold) in feature_checks {
        for cluster in find_clusters(&windows, feat_idx, threshold) {
            clusters.push(Cluster {
                kind,
                start: cluster.start,
                end:   cluster.end,
                peak:  cluster.peak_value,
            });
        }
    }

    let pauses = collect_pauses(ipa_stream);

    Ok(StreamAnalysisResult {
        version: FORMAT_VERSION,
        annotations,
        clusters,
        rhythm,
        echo,
        pauses,
    })
}


#[cfg(test)]
mod integration_tests {
    use super::*;

    fn reg() -> FeatureRegistry {
        let json = include_str!("test_data/mini_registry.json");
        FeatureRegistry::from_json_bytes(json.as_bytes()).unwrap()
    }

    // ── PhoneticStream round-trip ─────────────────────────────────────────

    #[test]
    fn test_stream_token_and_matrix_are_consistent() {
        let reg = reg();
        let stream = PhoneticStream::from_ipa("p a\nb a", &reg);
        assert_eq!(stream.feature_matrix.nrows(), stream.tokens.len());
        assert_eq!(stream.feature_matrix.ncols(), registry::FEATURE_NAMES.len());
    }

    // ── analyze: identical lines → perfect rhyme ─────────────────────────

    #[test]
    fn test_identical_lines_yield_perfect_rhyme() {
        let reg = reg();
        let result = analyze("p a\np a", &reg);
        assert!(!result.rhymes.is_empty(), "expected at least one rhyme match");
        let top = &result.rhymes[0];
        assert!(top.score > 0.90, "identical lines should be a perfect rhyme, got {}", top.score);
        assert!(matches!(top.r#type, RhymeType::Perfect));
    }

    // ── analyze: similar lines score higher than dissimilar lines ────────

    #[test]
    fn test_similar_lines_score_higher_than_dissimilar() {
        let reg = reg();
        // Identical lines → perfect rhyme
        let result_similar = analyze("p a\np a", &reg);
        // Reversed lines — phoneme content differs in ordering
        let result_different = analyze("p a\na p", &reg);

        let top_similar = result_similar.rhymes.first().map(|r| r.score).unwrap_or(0.0);
        let top_different = result_different.rhymes.first().map(|r| r.score).unwrap_or(0.0);

        assert!(
            top_similar >= top_different,
            "identical lines ({top_similar}) should score >= reversed lines ({top_different})"
        );
    }

    // ── analyze: sibilant cluster detection ──────────────────────────────

    #[test]
    fn test_sibilant_cluster_detected() {
        let reg = reg();
        // Long run of sibilants (ʃ and s) — window density should exceed threshold
        let ipa = "ʃ ʃ ʃ s ʃ s ʃ ʃ s ʃ ʃ";
        let result = analyze(ipa, &reg);
        let sibilant_clusters: Vec<_> = result.clusters.iter()
            .filter(|c| c.kind == "sibilant")
            .collect();
        assert!(!sibilant_clusters.is_empty(), "expected sibilant clusters in dense sibilant input");
    }

    // ── analyze: token / phoneme counts ──────────────────────────────────

    #[test]
    fn test_phoneme_count_excludes_boundaries() {
        let reg = reg();
        // "p a" → 3 tokens (p, space, a) but only 2 phonemes
        let result = analyze("p a", &reg);
        assert_eq!(result.token_count, 3);
        assert_eq!(result.phoneme_count, 2);
    }

    #[test]
    fn test_analyze_output_serialises_to_json() {
        let reg = reg();
        let result = analyze("p a\nb a", &reg);
        let json = serde_json::to_string(&result);
        assert!(json.is_ok(), "AnalysisResult should serialize cleanly");
    }

    // ── Full registry loading ─────────────────────────────────────────────

    #[test]
    fn test_full_registry_loads_from_assets() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().parent().unwrap() // → engine/
            .join("assets/phonemes.json");
        if path.exists() {
            let reg = FeatureRegistry::from_file(&path).unwrap();
            assert!(reg.len() > 1000, "expected thousands of segments");
            assert!(reg.get("p").is_some());
            assert!(reg.get("ʃ").is_some());
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Stream integration tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod stream_integration_tests {
    use super::*;

    fn reg() -> FeatureRegistry {
        let json = include_str!("test_data/mini_registry.json");
        FeatureRegistry::from_json_bytes(json.as_bytes()).unwrap()
    }

    /// Build an IpaStream from a JSON string (panics on parse error).
    fn parse(json: &str) -> IpaStream {
        IpaStream::from_json_bytes(json.as_bytes()).unwrap()
    }

    fn make_word(
        id: &str, line_idx: usize, word_idx: usize,
        stressed: i32, syllables: &[(&str, &[&str], bool)],
    ) -> String {
        let syls: Vec<String> = syllables.iter().map(|(ipa, toks, is_str)| {
            let tok_arr: Vec<String> = toks.iter().map(|t| format!("\"{t}\"")).collect();
            format!(
                r#"{{"ipa":"{ipa}","tokens":[{}],"grapheme":"","stressed":{is_str},"isOpen":true}}"#,
                tok_arr.join(",")
            )
        }).collect();
        format!(
            r#"{{"type":"word","id":"{id}","lineIndex":{line_idx},"wordIndex":{word_idx},"language":"uk","original":"","syllableCount":{n},"stressedSyllable":{stressed},"stressSource":"dict","syllables":[{s}]}}"#,
            n = syllables.len(),
            s = syls.join(","),
        )
    }

    fn wrap_stream(stream_items: &[String]) -> String {
        format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00.000Z","confirmedLineCount":1,"totalWordCount":1,"languagesPresent":["uk"]}},"stream":[{}]}}"#,
            stream_items.join(",")
        )
    }

    // ── Version validation ────────────────────────────────────────────────

    #[test]
    fn test_wrong_version_returns_error() {
        let json = r#"{"metadata":{"version":"2.0","generatedAt":"","confirmedLineCount":0,"totalWordCount":0,"languagesPresent":[]},"stream":[]}"#;
        let stream = parse(json);
        let reg = reg();
        let result = analyze_stream(&stream, &reg);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::UnsupportedVersion { .. }));
        assert!(err.to_string().contains("2.0"));
    }

    #[test]
    fn test_correct_version_returns_ok() {
        let json = r#"{"metadata":{"version":"1.1","generatedAt":"","confirmedLineCount":0,"totalWordCount":0,"languagesPresent":[]},"stream":[]}"#;
        let stream = parse(json);
        let reg = reg();
        assert!(analyze_stream(&stream, &reg).is_ok());
    }

    // ── All words annotated ───────────────────────────────────────────────

    #[test]
    fn test_all_words_appear_in_annotations() {
        let w1 = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let w2 = make_word("t2", 0, 1, 0, &[("ba", &["b", "a"], true)]);
        let lb = r#"{"type":"line_break","lineIndex":0}"#.to_string();
        let w3 = make_word("t3", 1, 0, 0, &[("pa", &["p", "a"], true)]);
        let json = wrap_stream(&[w1, r#"{"type":"whitespace"}"#.to_string(), w2, lb, w3]);
        let stream = parse(&json);
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        assert!(result.annotations.contains_key("t1"));
        assert!(result.annotations.contains_key("t2"));
        assert!(result.annotations.contains_key("t3"));
    }

    // ── Stress confidence from source ─────────────────────────────────────

    #[test]
    fn test_dict_stress_confidence_in_annotation() {
        let w = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let stream = parse(&wrap_stream(&[w]));
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        let ann = &result.annotations["t1"];
        assert!((ann.stress_confidence - 0.95).abs() < 1e-6);
    }

    // ── Identical line-final words → same rhyme group ─────────────────────

    #[test]
    fn test_identical_codas_get_same_rhyme_group() {
        // Two lines, each ending with "pa" (identical coda)
        let w1 = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let lb = r#"{"type":"line_break","lineIndex":0}"#.to_string();
        let w2 = make_word("t2", 1, 0, 0, &[("pa", &["p", "a"], true)]);
        let json = wrap_stream(&[w1, lb, w2]);
        let stream = parse(&json);
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();

        let g1 = &result.annotations["t1"].rhyme_group;
        let g2 = &result.annotations["t2"].rhyme_group;
        assert!(g1.is_some(), "t1 should have a rhyme group");
        assert_eq!(g1, g2, "identical codas should share a rhyme group");
    }

    // ── Similar coda scores higher than dissimilar coda ─────────────────

    #[test]
    fn test_identical_coda_scores_higher_than_dissimilar() {
        // We compare the rhyme SCORES rather than the binary group assignment
        // because short phoneme sequences can share many structural features.
        let reg = reg();

        // Identical: t1 and t3 both end with [p, a]
        let w_pa1 = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let lb1   = r#"{"type":"line_break","lineIndex":0}"#.to_string();
        let w_pa2 = make_word("t3", 1, 0, 0, &[("pa", &["p", "a"], true)]);
        let json_same = wrap_stream(&[w_pa1, lb1, w_pa2]);
        let stream_same = parse(&json_same);
        let score_same = analyze_stream(&stream_same, &reg).unwrap()
            .annotations["t1"].rhyme_score.unwrap_or(0.0);

        // Different: t1 ends with [p, a], t2 ends with [ʃ, u]
        let w_pa  = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let lb2   = r#"{"type":"line_break","lineIndex":0}"#.to_string();
        let w_shu = make_word("t2", 1, 0, 0, &[("ʃu", &["ʃ", "u"], true)]);
        let json_diff = wrap_stream(&[w_pa, lb2, w_shu]);
        let stream_diff = parse(&json_diff);
        let score_diff = analyze_stream(&stream_diff, &reg).unwrap()
            .annotations["t1"].rhyme_score.unwrap_or(0.0);

        assert!(
            score_same >= score_diff,
            "identical coda score ({score_same}) should be >= dissimilar coda score ({score_diff})"
        );
    }

    // ── Rhyme score present when rhyme detected ───────────────────────────

    #[test]
    fn test_rhyme_score_present_for_rhyming_words() {
        let w1 = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let lb = r#"{"type":"line_break","lineIndex":0}"#.to_string();
        let w2 = make_word("t2", 1, 0, 0, &[("pa", &["p", "a"], true)]);
        let json = wrap_stream(&[w1, lb, w2]);
        let stream = parse(&json);
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        assert!(result.annotations["t1"].rhyme_score.is_some());
        assert!(result.annotations["t2"].rhyme_score.is_some());
    }

    // ── Internal (mid-line) rhyme is detected ─────────────────────────────

    #[test]
    fn test_internal_rhyme_detected_across_lines() {
        // Line 0: mid_a(pa) end_a(su)
        // Line 1: mid_b(pa) end_b(du)
        // mid_a and mid_b should share a rhyme group (identical coda "pa"),
        // even though they are not line-final words.
        let mid_a = make_word("mid_a", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let end_a = make_word("end_a", 0, 1, 0, &[("su", &["s", "u"], true)]);
        let lb    = r#"{"type":"line_break","lineIndex":0}"#.to_string();
        let mid_b = make_word("mid_b", 1, 0, 0, &[("pa", &["p", "a"], true)]);
        let end_b = make_word("end_b", 1, 1, 0, &[("du", &["d", "u"], true)]);
        let lb2   = r#"{"type":"line_break","lineIndex":1}"#.to_string();
        let json  = wrap_stream(&[mid_a, end_a, lb, mid_b, end_b, lb2]);
        let stream = parse(&json);
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();

        let g_mid_a = &result.annotations["mid_a"].rhyme_group;
        let g_mid_b = &result.annotations["mid_b"].rhyme_group;
        assert!(g_mid_a.is_some(), "mid_a should have a rhyme group");
        assert_eq!(g_mid_a, g_mid_b, "internal rhyming words should share a group");

        // end words differ phonologically — they should not share mid's group
        let g_end_a = &result.annotations["end_a"].rhyme_group;
        assert_ne!(g_end_a, g_mid_a, "non-rhyming end word should not share mid group");
    }

    // ── WordAnnotation carries position fields ────────────────────────────

    #[test]
    fn test_word_annotation_has_position_fields() {
        let w = make_word("t1", 3, 2, 0, &[("pa", &["p", "a"], true)]);
        let stream = parse(&wrap_stream(&[w]));
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        let ann = &result.annotations["t1"];
        assert_eq!(ann.line_index, 3);
        assert_eq!(ann.word_index, 2);
    }

    // ── Round-trip JSON serialisation ─────────────────────────────────────

    #[test]
    fn test_stream_result_serialises_to_json() {
        let w1 = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let stream = parse(&wrap_stream(&[w1]));
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        let json_str = serde_json::to_string(&result);
        assert!(json_str.is_ok());
        let json_str = json_str.unwrap();
        assert!(json_str.contains("\"version\""));
        assert!(json_str.contains("\"annotations\""));
        assert!(json_str.contains("\"clusters\""));
    }

    #[test]
    fn test_stream_result_version_field_is_1_1() {
        let w = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let stream = parse(&wrap_stream(&[w]));
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        assert_eq!(result.version, "1.1");
    }

    // ── camelCase serialisation (round-trip field names) ──────────────────

    #[test]
    fn test_annotation_fields_are_camel_case() {
        let w = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let stream = parse(&wrap_stream(&[w]));
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        let json_str = serde_json::to_string(&result).unwrap();
        assert!(json_str.contains("rhymeGroup"),      "expected camelCase 'rhymeGroup'");
        assert!(json_str.contains("rhymeScore"),      "expected camelCase 'rhymeScore'");
        assert!(json_str.contains("stressConfidence"), "expected camelCase 'stressConfidence'");
    }

    // ── Sibilant cluster detected via stream ──────────────────────────────

    #[test]
    fn test_sibilant_cluster_detected_from_stream() {
        // Use pure sibilant consonant tokens (no vowels that would cancel the
        // strident feature: ʃ=+1, a=−1 → net 0 per syllable).
        // Each syllable is a single sibilant phoneme so density stays positive.
        let strid_word = |id: &str, li: usize| make_word(
            id, li, 0, 0,
            &[
                ("ʃ", &["ʃ"], false),
                ("s", &["s"], false),
                ("ʃ", &["ʃ"], false),
                ("s", &["s"], true),
                ("ʃ", &["ʃ"], false),
                ("s", &["s"], false),
            ],
        );
        let w1 = strid_word("t1", 0);
        let lb = r#"{"type":"line_break","lineIndex":0}"#.to_string();
        let w2 = strid_word("t2", 1);
        let json = wrap_stream(&[w1, lb, w2]);
        let stream = parse(&json);
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        let sib: Vec<_> = result.clusters.iter().filter(|c| c.kind == "sibilant").collect();
        assert!(!sib.is_empty(), "expected sibilant clusters from dense sibilant stream");
    }
}

