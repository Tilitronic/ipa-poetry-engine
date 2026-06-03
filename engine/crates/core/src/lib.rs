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

use std::collections::{HashMap, HashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use crate::registry::{FEATURE_DESCRIPTIONS, FEATURE_NAMES};

use algorithms::density::{find_clusters, sliding_window, IDX_DELREL, IDX_NAS, IDX_STRID, IDX_SYL, IDX_LAT};
use algorithms::dtw::{dtw_similarity, rhyme_distance};
use algorithms::echo::{compute_echo, EchoParams, DEFAULT_ALPHA_MIN};
use algorithms::pause::collect_pauses;
use algorithms::rhythm::analyze_line_rhythm;
use algorithms::structural::analyze_structural;
use algorithms::structurality::compute_structurality;
use stream::{stress_confidence, tokens_from_word};
use tokenizer::TokenType;

pub use algorithms::echo::{EchoAnnotation, PhonemeRef};
pub use algorithms::pause::{collect_pauses as compute_pauses, PauseAnnotation};
pub use algorithms::rhythm::{Clausula, DeviationType, LineRhythm, SyllableAnnotation, SyllableRef};
pub use algorithms::structural::{StructuralAnnotation, SyllableShape};
pub use algorithms::structurality::{StructuralityAnalysis, StructuralityComponent, StructuralityWeights};

// ────────────────────────────────────────────────────────────────────────────
// Output types (serialisable to JSON)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct RhymeMatch {
    pub r#type: RhymeType,
    /// Token indices of the first sequence.
    pub indices_a: Vec<usize>,
    /// Token indices of the second sequence.
    pub indices_b: Vec<usize>,
    /// Similarity score [0, 1]; 1 = perfect rhyme.
    pub score: f32,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RhymeType {
    Perfect,   // score > 0.90
    Near,      // score > 0.70
    Imperfect, // score > 0.50
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Cluster {
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub peak: f32,
}

#[derive(Debug, Serialize, JsonSchema)]
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

/// Detected rhyme match between two words with position and strength metadata.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RhymePair {
    /// ID of the first word in the pair.
    pub word_id_a: String,
    /// ID of the second word in the pair.
    pub word_id_b: String,
    /// DTW phonetic similarity score [0, 1], higher = stronger rhyme.
    pub similarity: f32,
    /// Length of the best matching phoneme subsequence.
    pub match_length: usize,
    /// Start position (phoneme index) of match in word A.
    pub position_a: usize,
    /// Start position (phoneme index) of match in word B.
    pub position_b: usize,
    /// IPA symbol sequence of the matched region in word A.
    pub sequence_a: Vec<String>,
    /// IPA symbol sequence of the matched region in word B.
    pub sequence_b: Vec<String>,
    /// Weighted score: similarity × sqrt(match_length).
    pub weighted_score: f32,
    /// Strength tier for quick filtering: "strong" | "medium" | "weak".
    pub strength_tier: String,
}

/// Per-word annotation keyed by `id` from the input stream.
#[derive(Debug, Serialize, JsonSchema)]
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

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhonemeFeatureSchemaItem {
    pub key: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhonemeFeatureEncoding {
    pub positive: &'static str,
    pub negative: &'static str,
    pub unspecified: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhonemeFeatureValue {
    pub key: &'static str,
    pub description: &'static str,
    pub value: f32,
    pub sign: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhonemeNaturalProfile {
    pub class_hint: &'static str,
    pub vowelness: f32,
    pub consonantality: f32,
    pub sonority: f32,
    pub voicing: f32,
    pub labiality: f32,
    pub coronality: f32,
    pub nasality: f32,
    pub stridency: f32,
    pub laterality: f32,
    pub highness: f32,
    pub openness: f32,
    pub backness: f32,
    pub roundedness: f32,
    pub tenseness: f32,
    pub lengthness: f32,
    pub tone_height: f32,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhonemeClusterContribution {
    pub kind: &'static str,
    pub peak: f32,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhonemeComputedMetrics {
    pub line_index: usize,
    pub word_index: usize,
    pub is_stressed_syllable: bool,
    pub stress_weight: f32,
    pub rhyme_group: Option<String>,
    pub structural_rhyme_group: Option<String>,
    pub nearest_match_flat_index: Option<usize>,
    pub echo_gap: f32,
    pub echo_opacity: f32,
    pub cluster_membership_count: usize,
    pub cluster_peak_max: f32,
    pub cluster_contributions: Vec<PhonemeClusterContribution>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhonemeRecord {
    pub source: PhonemeRef,
    pub symbol: String,
    pub vector: Vec<f32>,
    pub features: Vec<PhonemeFeatureValue>,
    pub natural_profile: PhonemeNaturalProfile,
    pub computed_metrics: PhonemeComputedMetrics,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MolstarContactKind {
    SimilarityWeak,
    RhymeStrong,
    PausePattern,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MolstarSecondaryElement {
    pub kind: &'static str,
    pub start_residue_index: usize,
    pub end_residue_index: usize,
    pub note: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MolstarContact {
    pub kind: MolstarContactKind,
    pub from_residue_index: usize,
    pub to_residue_index: usize,
    pub strength: f32,
    pub equilibrium_distance: f32,
    pub decay_length: f32,
    pub spring_constant: f32,
    pub energy: f32,
    pub note: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MolstarResidueMapItem {
    pub residue_index: usize,
    pub amino_acid: char,
    pub amino_acid_name: &'static str,
    pub source: PhonemeRef,
    pub symbol: String,
    pub line_index: usize,
    pub word_index: usize,
    pub language: String,
    pub original_word: String,
    pub syllable_ipa: String,
    pub syllable_grapheme: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MolstarWordSpan {
    pub word_id: String,
    pub line_index: usize,
    pub word_index: usize,
    pub language: String,
    pub original_word: String,
    pub ipa_word: String,
    pub residue_start: usize,
    pub residue_end: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MolstarBiophysicalModel {
    pub model_name: &'static str,
    pub backbone_step: f32,
    pub contact_energy_unit: &'static str,
    pub distance_unit: &'static str,
    pub similarity_contact_cutoff: f32,
    pub pause_pattern_min_repeat: usize,
    pub equations: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MolstarTranscription {
    pub format_version: &'static str,
    pub chain_id: char,
    pub sequence: String,
    pub fasta: String,
    pub pdb: String,
    pub secondary_structure: Vec<MolstarSecondaryElement>,
    pub contacts: Vec<MolstarContact>,
    pub residue_map: Vec<MolstarResidueMapItem>,
    pub word_spans: Vec<MolstarWordSpan>,
    pub ipa_lines: Vec<String>,
    pub original_lines: Vec<String>,
    pub biophysical_model: MolstarBiophysicalModel,
    pub interpretation: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzerInfo {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResponseSchemaInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub dialect: &'static str,
    pub file: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhonemeLayer {
    pub total: usize,
    pub sorted_by: &'static str,
    pub feature_schema: Vec<PhonemeFeatureSchemaItem>,
    pub value_encoding: PhonemeFeatureEncoding,
    pub entries: Vec<PhonemeRecord>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricGlossaryEntry {
    pub id: &'static str,
    pub source: &'static str,
    pub description: &'static str,
    pub interpretation: &'static str,
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
#[derive(Debug, Serialize, JsonSchema)]
pub struct StreamAnalysisResult {
    /// IPA Stream format version this result was produced from.
    pub version: &'static str,
    /// Analyzer name and version.
    pub analyzer: AnalyzerInfo,
    /// Response schema descriptor for contract-aware consumers.
    #[serde(rename = "responseSchema")]
    pub response_schema: ResponseSchemaInfo,
    /// Per-word annotations keyed by word `id`.
    pub annotations: HashMap<String, WordAnnotation>,
    /// All detected rhyme pairs with similarity scores (flexible grouping).
    pub rhyme_pairs: Vec<RhymePair>,
    /// Sound clusters detected across the full stream.
    pub clusters: Vec<Cluster>,
    /// Per-line rhythm analysis (stress pattern, clausula, confidence).
    pub rhythm: Vec<LineRhythm>,
    /// Per-phoneme echo opacity annotations.
    pub echo: Vec<EchoAnnotation>,
    /// Prosodic pauses created by punctuation and/or line breaks.
    pub pauses: Vec<PauseAnnotation>,
    /// Fully expanded per-phoneme payload sorted by flat index.
    pub phonemes: PhonemeLayer,
    /// IPA analysis transcribed as amino-acid-like chain for Mol* rendering.
    pub molstar: MolstarTranscription,
    /// Cross-plane structural complexity report normalised to [0, 1].
    pub structurality: StructuralityAnalysis,
    /// Short metric glossary for fast frontend interpretation and UX hints.
    #[serde(rename = "metricGlossary")]
    pub metric_glossary: Vec<MetricGlossaryEntry>,
}

fn metric_glossary() -> Vec<MetricGlossaryEntry> {
    vec![
        MetricGlossaryEntry {
            id: "rhyme_pairs.similarity",
            source: "rhyme_pairs[]",
            description: "DTW similarity of two phoneme sequences in [0,1].",
            interpretation: "Higher means stronger rhyme; useful for threshold filtering.",
        },
        MetricGlossaryEntry {
            id: "rhyme_pairs.weighted_score",
            source: "rhyme_pairs[]",
            description: "Similarity weighted by match length: similarity * sqrt(matchLength).",
            interpretation: "Ranks longer and cleaner matches above short accidental matches.",
        },
        MetricGlossaryEntry {
            id: "annotations.rhymeScore",
            source: "annotations[wordId]",
            description: "Best rhyme similarity for a word against any detected partner.",
            interpretation: "Use for per-word confidence, sorting, or heatmap intensity.",
        },
        MetricGlossaryEntry {
            id: "clusters.peak",
            source: "clusters[]",
            description: "Peak density value of the feature-specific sliding window.",
            interpretation: "Higher means stronger local concentration of that sound class.",
        },
        MetricGlossaryEntry {
            id: "echo.opacity",
            source: "echo[]",
            description: "Opacity derived from nearest similar-phoneme distance.",
            interpretation: "Higher means denser local repetition; lower means isolated sound.",
        },
        MetricGlossaryEntry {
            id: "phonemes.entries[].computedMetrics.stressWeight",
            source: "phonemes.entries[]",
            description: "Stress-aware phoneme weight used by downstream scoring.",
            interpretation: "Higher values indicate stressed or rhythmically salient positions.",
        },
        MetricGlossaryEntry {
            id: "phonemes.entries[].computedMetrics.clusterPeakMax",
            source: "phonemes.entries[]",
            description: "Maximum cluster peak among all clusters containing the phoneme.",
            interpretation: "Useful for filtering phonemes that belong to strong sound patches.",
        },
        MetricGlossaryEntry {
            id: "structurality.score",
            source: "structurality",
            description: "Global structural complexity score normalised to [0,1].",
            interpretation: "Higher means richer cross-level organisation across rhythm, rhyme and pause planes.",
        },
    ]
}

// ────────────────────────────────────────────────────────────────────────────
// Rhyme grouping threshold
// ────────────────────────────────────────────────────────────────────────────

/// Minimum DTW similarity score to consider two words as rhyming.
/// Lowered to 0.4 to enable flexible frontend grouping (e.g., "дон-зон" ~60%).
const RHYME_THRESHOLD: f32 = 0.40;

/// Minimum phoneme count in n-gram subsequence to qualify as rhyme candidate.
const MIN_NGRAM_LENGTH: usize = 2;

/// Maximum phoneme count in n-gram subsequence (performance limit).
const MAX_NGRAM_LENGTH: usize = 15;

// ────────────────────────────────────────────────────────────────────────────
// N-gram rhyme matching — find similar phoneme subsequences across full text
// ────────────────────────────────────────────────────────────────────────────

/// N-gram phoneme subsequence with position metadata.
#[derive(Debug, Clone)]
struct PhonemeNGram {
    /// Start position in flat phoneme sequence.
    start_flat_index: usize,
    /// Length of the n-gram (number of phonemes).
    length: usize,
    /// Feature vectors of phonemes in this n-gram.
    features: Vec<ndarray::Array1<f32>>,
    /// IPA symbols for debugging/export.
    symbols: Vec<String>,
    /// Word IDs that this n-gram spans (for cross-word rhymes).
    word_ids: Vec<String>,
}

/// Find all n-gram rhyme matches across the full flattened phoneme sequence.
/// 
/// Algorithm:
/// 1. Generate all n-grams (length 3-15) from flat phoneme stream
/// 2. Compare all pairs via DTW
/// 3. Map back to source words and compute weighted scores
/// 4. Return sorted by weighted_score (similarity × sqrt(length))
fn find_ngram_rhyme_matches(
    flat_tokens: &[tokenizer::PhoneticToken],
    flat_context: &[FlatPhonemeContext],
    _all_words: &[&stream::IpaStreamWord],
) -> Vec<RhymePair> {

    // ── Step 1: Generate all n-grams from flat phoneme sequence ──────────
    let mut ngrams: Vec<PhonemeNGram> = Vec::new();
    
    for length in MIN_NGRAM_LENGTH..=MAX_NGRAM_LENGTH {
        for start in 0..flat_tokens.len().saturating_sub(length - 1) {
            let end = start + length;
            
            let features: Vec<ndarray::Array1<f32>> = flat_tokens[start..end]
                .iter()
                .filter(|t| t.t_type == tokenizer::TokenType::Phoneme)
                .map(|t| t.features.clone())
                .collect();
            
            if features.len() != length {
                continue; // Skip if contains boundary tokens
            }
            
            let symbols: Vec<String> = flat_context[start..end]
                .iter()
                .map(|ctx| ctx.symbol.clone())
                .collect();
            
            let mut word_ids_set = std::collections::HashSet::new();
            for ctx in &flat_context[start..end] {
                word_ids_set.insert(ctx.word_id.clone());
            }
            let word_ids: Vec<String> = word_ids_set.into_iter().collect();
            
            ngrams.push(PhonemeNGram {
                start_flat_index: start,
                length,
                features,
                symbols,
                word_ids,
            });
        }
    }

    // ── Step 2: Compare all n-gram pairs via DTW ─────────────────────────
    let mut matches: Vec<RhymePair> = Vec::new();
    
    for i in 0..ngrams.len() {
        for j in (i + 1)..ngrams.len() {
            let ng_a = &ngrams[i];
            let ng_b = &ngrams[j];
            
            // Skip if same length but too different positions (optimization)
            if ng_a.length == ng_b.length && ng_a.start_flat_index.abs_diff(ng_b.start_flat_index) < ng_a.length {
                continue; // Overlapping or adjacent — not a rhyme
            }
            
            // Skip if no shared words (ensure cross-word or inter-word matches)
            let same_word = ng_a.word_ids.iter().any(|id| ng_b.word_ids.contains(id));
            if same_word && ng_a.word_ids.len() == 1 && ng_b.word_ids.len() == 1 {
                continue; // Both within same single word — not interesting
            }
            
            // Compute DTW similarity
            let features_a: Vec<ndarray::ArrayView1<f32>> = ng_a.features.iter().map(|f| f.view()).collect();
            let features_b: Vec<ndarray::ArrayView1<f32>> = ng_b.features.iter().map(|f| f.view()).collect();
            
            let distance = algorithms::dtw::dtw_distance(&features_a, &features_b);
            let similarity = algorithms::dtw::dtw_similarity(distance);
            
            if similarity < RHYME_THRESHOLD {
                continue;
            }
            
            // Compute weighted score: similarity × sqrt(avg_length)
            let avg_length = (ng_a.length + ng_b.length) as f32 / 2.0;
            let weighted_score = similarity * avg_length.sqrt();
            
            // Determine strength tier
            let strength_tier = if weighted_score >= 2.5 {
                "strong"
            } else if weighted_score >= 1.5 {
                "medium"
            } else {
                "weak"
            }.to_string();
            
            // Map to primary word IDs (use first word if spans multiple)
            let word_id_a = ng_a.word_ids.first().cloned().unwrap_or_default();
            let word_id_b = ng_b.word_ids.first().cloned().unwrap_or_default();
            
            matches.push(RhymePair {
                word_id_a,
                word_id_b,
                similarity,
                match_length: ((ng_a.length + ng_b.length) / 2),
                position_a: ng_a.start_flat_index,
                position_b: ng_b.start_flat_index,
                sequence_a: ng_a.symbols.clone(),
                sequence_b: ng_b.symbols.clone(),
                weighted_score,
                strength_tier,
            });
        }
    }
    
    // ── Step 3: Sort by weighted score (strongest first) ─────────────────
    matches.sort_by(|a, b| b.weighted_score.partial_cmp(&a.weighted_score).unwrap_or(std::cmp::Ordering::Equal));
    
    matches
}

#[derive(Debug)]
struct FlatPhonemeContext {
    source: PhonemeRef,
    symbol: String,
    line_index: usize,
    word_index: usize,
    word_id: String,
    language: String,
    original_word: String,
    syllable_ipa: String,
    syllable_grapheme: String,
    is_stressed_syllable: bool,
}
const ANALYZER_NAME: &str = "phonetic-poetry-engine";
const ANALYZER_VERSION: &str = env!("CARGO_PKG_VERSION");
const RESPONSE_SCHEMA_NAME: &str = "StreamAnalysisResult";
const RESPONSE_SCHEMA_VERSION: &str = "1.0";
const RESPONSE_SCHEMA_DIALECT: &str = "http://json-schema.org/draft-07/schema#";
const RESPONSE_SCHEMA_FILE: &str = "stream_analysis.response.schema.json";

fn tri_state_to_unit_interval(value: f32) -> f32 {
    ((value + 1.0) / 2.0).clamp(0.0, 1.0)
}

fn value_sign(value: f32) -> &'static str {
    if value > 0.0 {
        "+"
    } else if value < 0.0 {
        "-"
    } else {
        "0"
    }
}

fn class_hint(features: &[f32]) -> &'static str {
    let syl = features[0];
    let son = features[1];
    let cons = features[2];

    if syl > 0.0 {
        "vowel_like"
    } else if cons > 0.0 && son < 0.0 {
        "obstruent_like"
    } else if cons > 0.0 {
        "consonant_like"
    } else if son > 0.0 {
        "sonorant_like"
    } else {
        "unspecified_or_nonsegmental"
    }
}

fn amino_acid_for_vector(features: &[f32]) -> (char, &'static str) {
    const AA: [(char, &str); 20] = [
        ('A', "ALA"), ('R', "ARG"), ('N', "ASN"), ('D', "ASP"), ('C', "CYS"),
        ('Q', "GLN"), ('E', "GLU"), ('G', "GLY"), ('H', "HIS"), ('I', "ILE"),
        ('L', "LEU"), ('K', "LYS"), ('M', "MET"), ('F', "PHE"), ('P', "PRO"),
        ('S', "SER"), ('T', "THR"), ('W', "TRP"), ('Y', "TYR"), ('V', "VAL"),
    ];

    let score = features
        .iter()
        .enumerate()
        .fold(0i32, |acc, (i, &v)| acc + ((v * 100.0) as i32) * ((i as i32 % 7) + 1));
    let idx = score.rem_euclid(AA.len() as i32) as usize;
    AA[idx]
}

fn build_pdb_atom_line(
    atom_serial: usize,
    residue_name: &str,
    chain_id: char,
    residue_seq: usize,
    x: f32,
    y: f32,
    z: f32,
) -> String {
    format!(
        "ATOM  {atom_serial:>5}  CA  {residue_name:>3} {chain_id}{residue_seq:>4}    {x:>8.3}{y:>8.3}{z:>8.3}  1.00 20.00           C\n"
    )
}

fn stddev(values: &[f32]) -> f32 {
    if values.len() <= 1 {
        return 0.0;
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let var = values
        .iter()
        .map(|v| {
            let d = v - mean;
            d * d
        })
        .sum::<f32>()
        / values.len() as f32;
    var.sqrt()
}

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

    // ── Flatten all phoneme tokens from all words (needed for n-gram rhyme) ──
    let mut flat_tokens: Vec<tokenizer::PhoneticToken> = Vec::new();
    let mut flat_token_lines: Vec<usize> = Vec::new();
    let mut flat_context: Vec<FlatPhonemeContext> = Vec::new();

    for word in &all_words {
        let word_tokens = tokens_from_word(word, registry);
        flat_token_lines.extend(std::iter::repeat(word.line_index).take(word_tokens.len()));
        flat_tokens.extend(word_tokens);

        for (syl_idx, syllable) in word.syllables.iter().enumerate() {
            let is_stressed_syllable = word.stressed_syllable >= 0
                && syl_idx == word.stressed_syllable as usize;
            for (phoneme_idx, symbol) in syllable.tokens.iter().enumerate() {
                flat_context.push(FlatPhonemeContext {
                    source: PhonemeRef {
                        word_id: word.id.clone(),
                        syllable_index: syl_idx,
                        phoneme_index: phoneme_idx,
                        flat_index: flat_context.len(),
                    },
                    symbol: symbol.clone(),
                    line_index: word.line_index,
                    word_index: word.word_index,
                    word_id: word.id.clone(),
                    language: word.language.clone(),
                    original_word: word.original.clone(),
                    syllable_ipa: syllable.ipa.clone(),
                    syllable_grapheme: syllable.grapheme.clone(),
                    is_stressed_syllable,
                });
            }
        }
    }

    debug_assert_eq!(flat_context.len(), flat_tokens.len());

    // ── N-gram rhyme detection over full flattened phoneme sequence ──────
    // Find similar phoneme subsequences (length 3-15) using sliding window
    // across the entire text, ignoring word boundaries. This catches:
    // - Cross-word rhymes (кар-то-ма зо-на → "тома-зо")
    // - Internal rhymes (ор-ка ↔ он-ка)
    // - Anagrammatic rhymes (пужд ↔ ждуб)
    // - Variable-length rhymes (кар-то ↔ кр-то)
    let rhyme_pairs_output = find_ngram_rhyme_matches(&flat_tokens, &flat_context, &all_words);

    // ── Greedy rhyme group assignment from n-gram pairs ──────────────────
    let n = all_words.len();
    let mut group_of: Vec<Option<String>> = vec![None; n];
    let mut group_counter: u8 = b'A';
    let mut best_score: Vec<f32> = vec![0.0; n];

    // Build index: word_id → word_index
    let word_id_to_idx: std::collections::HashMap<&str, usize> = all_words
        .iter()
        .enumerate()
        .map(|(idx, w)| (w.id.as_str(), idx))
        .collect();

    for pair in &rhyme_pairs_output {
        let i = word_id_to_idx.get(pair.word_id_a.as_str()).copied();
        let j = word_id_to_idx.get(pair.word_id_b.as_str()).copied();

        if let (Some(i), Some(j)) = (i, j) {
            // Update best scores
            if pair.similarity > best_score[i] {
                best_score[i] = pair.similarity;
            }
            if pair.similarity > best_score[j] {
                best_score[j] = pair.similarity;
            }

            // Greedy group assignment
            match (group_of[i].clone(), group_of[j].clone()) {
                (None, None) => {
                    let letter = (group_counter as char).to_string();
                    group_counter = group_counter.saturating_add(1);
                    group_of[i] = Some(letter.clone());
                    group_of[j] = Some(letter);
                }
                (Some(g), None) => {
                    group_of[j] = Some(g);
                }
                (None, Some(g)) => {
                    group_of[i] = Some(g);
                }
                (Some(_), Some(_)) => {} // Both already grouped
            }
        }
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
    // Use already-flattened phoneme tokens for cluster detection.
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

    let mut clusters_by_flat_idx: Vec<HashMap<&'static str, f32>> =
        (0..flat_tokens.len()).map(|_| HashMap::new()).collect();
    for cluster in &clusters {
        let start = cluster.start.min(flat_tokens.len());
        let end = cluster.end.min(flat_tokens.len());
        for idx in start..end {
            let bucket = &mut clusters_by_flat_idx[idx];
            let current = bucket.entry(cluster.kind).or_insert(0.0);
            if cluster.peak > *current {
                *current = cluster.peak;
            }
        }
    }

    let echo_by_flat_idx: HashMap<usize, &EchoAnnotation> = echo
        .iter()
        .map(|item| (item.source.flat_index, item))
        .collect();

    let feature_schema: Vec<PhonemeFeatureSchemaItem> = FEATURE_NAMES
        .iter()
        .zip(FEATURE_DESCRIPTIONS.iter())
        .map(|(&key, &description)| PhonemeFeatureSchemaItem { key, description })
        .collect();

    let phoneme_entries: Vec<PhonemeRecord> = flat_tokens
        .iter()
        .enumerate()
        .map(|(idx, token)| {
            let context = &flat_context[idx];
            let vector = token.features.to_vec();
            let features: Vec<PhonemeFeatureValue> = FEATURE_NAMES
                .iter()
                .zip(FEATURE_DESCRIPTIONS.iter())
                .zip(vector.iter())
                .map(|((&key, &description), &value)| PhonemeFeatureValue {
                    key,
                    description,
                    value,
                    sign: value_sign(value),
                })
                .collect();

            let natural_profile = PhonemeNaturalProfile {
                class_hint: class_hint(&vector),
                vowelness: tri_state_to_unit_interval(vector[0]),
                consonantality: tri_state_to_unit_interval(vector[2]),
                sonority: tri_state_to_unit_interval(vector[1]),
                voicing: tri_state_to_unit_interval(vector[8]),
                labiality: tri_state_to_unit_interval(vector[14]),
                coronality: tri_state_to_unit_interval(vector[12]),
                nasality: tri_state_to_unit_interval(vector[6]),
                stridency: tri_state_to_unit_interval(vector[7]),
                laterality: tri_state_to_unit_interval(vector[5]),
                highness: tri_state_to_unit_interval(vector[15]),
                openness: tri_state_to_unit_interval(vector[16]),
                backness: tri_state_to_unit_interval(vector[17]),
                roundedness: tri_state_to_unit_interval(vector[18]),
                tenseness: tri_state_to_unit_interval(vector[20]),
                lengthness: tri_state_to_unit_interval(vector[21]),
                tone_height: 0.5 * tri_state_to_unit_interval(vector[22])
                    + 0.5 * tri_state_to_unit_interval(vector[23]),
            };

            let mut cluster_contributions: Vec<PhonemeClusterContribution> = clusters_by_flat_idx[idx]
                .iter()
                .map(|(&kind, &peak)| PhonemeClusterContribution { kind, peak })
                .collect();
            cluster_contributions.sort_by(|a, b| a.kind.cmp(b.kind));
            let cluster_peak_max = cluster_contributions
                .iter()
                .map(|c| c.peak)
                .fold(0.0, f32::max);

            let (nearest_match_flat_index, echo_gap, echo_opacity) =
                if let Some(echo_item) = echo_by_flat_idx.get(&idx) {
                    (
                        echo_item.nearest_match,
                        echo_item.gap,
                        echo_item.opacity,
                    )
                } else {
                    (None, flat_tokens.len() as f32, DEFAULT_ALPHA_MIN)
                };

            let word_ann = annotations.get(&context.word_id);
            let computed_metrics = PhonemeComputedMetrics {
                line_index: context.line_index,
                word_index: context.word_index,
                is_stressed_syllable: context.is_stressed_syllable,
                stress_weight: token.weight,
                rhyme_group: word_ann.and_then(|a| a.rhyme_group.clone()),
                structural_rhyme_group: word_ann.and_then(|a| a.structural_rhyme_group.clone()),
                nearest_match_flat_index,
                echo_gap,
                echo_opacity,
                cluster_membership_count: cluster_contributions.len(),
                cluster_peak_max,
                cluster_contributions,
            };

            PhonemeRecord {
                source: context.source.clone(),
                symbol: context.symbol.clone(),
                vector,
                features,
                natural_profile,
                computed_metrics,
            }
        })
        .collect();

    let phonemes = PhonemeLayer {
        total: phoneme_entries.len(),
        sorted_by: "source.flatIndex asc",
        feature_schema,
        value_encoding: PhonemeFeatureEncoding {
            positive: "positively specified (present)",
            negative: "negatively specified (absent)",
            unspecified: "unspecified / not applicable",
        },
        entries: phoneme_entries,
    };

    let pauses = collect_pauses(ipa_stream);

    let mut deviation_by_syllable: HashMap<(String, usize), DeviationType> = HashMap::new();
    for line in &rhythm {
        for syl in &line.syllables {
            deviation_by_syllable.insert(
                (
                    syl.syllable_ref.word_id.clone(),
                    syl.syllable_ref.syllable_index,
                ),
                syl.deviation.clone(),
            );
        }
    }

    let mut sequence = String::with_capacity(flat_tokens.len());
    let mut residue_map: Vec<MolstarResidueMapItem> = Vec::with_capacity(flat_tokens.len());
    let mut pdb_lines: Vec<String> = Vec::with_capacity(flat_tokens.len() + 16);
    let mut sec_codes: Vec<char> = Vec::with_capacity(flat_tokens.len());
    let mut word_spans_map: HashMap<String, MolstarWordSpan> = HashMap::new();

    for (idx, token) in flat_tokens.iter().enumerate() {
        let residue_index = idx + 1;
        let context = &flat_context[idx];
        let vector = token.features.to_vec();
        let (aa, aa_name) = amino_acid_for_vector(&vector);
        sequence.push(aa);

        residue_map.push(MolstarResidueMapItem {
            residue_index,
            amino_acid: aa,
            amino_acid_name: aa_name,
            source: context.source.clone(),
            symbol: context.symbol.clone(),
            line_index: context.line_index,
            word_index: context.word_index,
            language: context.language.clone(),
            original_word: context.original_word.clone(),
            syllable_ipa: context.syllable_ipa.clone(),
            syllable_grapheme: context.syllable_grapheme.clone(),
        });

        let span_entry = word_spans_map
            .entry(context.word_id.clone())
            .or_insert_with(|| MolstarWordSpan {
                word_id: context.word_id.clone(),
                line_index: context.line_index,
                word_index: context.word_index,
                language: context.language.clone(),
                original_word: context.original_word.clone(),
                ipa_word: String::new(),
                residue_start: residue_index,
                residue_end: residue_index,
            });
        span_entry.residue_end = residue_index;
        span_entry.ipa_word.push_str(&context.symbol);

        let deviation = deviation_by_syllable
            .get(&(context.word_id.clone(), context.source.syllable_index));
        let sec_code = if context.is_stressed_syllable {
            'H'
        } else if matches!(deviation, Some(DeviationType::Spondee | DeviationType::Pyrrhic)) {
            'E'
        } else {
            'C'
        };
        sec_codes.push(sec_code);

        let t = idx as f32;
        let (x, y, z) = match sec_code {
            'H' => (
                1.6 * (t * 0.72).cos(),
                1.6 * (t * 0.72).sin(),
                t * 1.5,
            ),
            'E' => (
                t * 1.2,
                if idx % 2 == 0 { 1.3 } else { -1.3 },
                t * 0.3,
            ),
            _ => (t * 1.35, 0.0, 0.0),
        };

        pdb_lines.push(build_pdb_atom_line(
            residue_index,
            aa_name,
            'A',
            residue_index,
            x,
            y,
            z,
        ));
    }

    let mut secondary_structure: Vec<MolstarSecondaryElement> = Vec::new();
    if !sec_codes.is_empty() {
        let mut run_start = 0usize;
        for idx in 1..=sec_codes.len() {
            if idx == sec_codes.len() || sec_codes[idx] != sec_codes[run_start] {
                let code = sec_codes[run_start];
                let (kind, note) = match code {
                    'H' => ("helix", "rhythmic stress scaffolding"),
                    'E' => ("sheet", "rhythmic deviation zig-zag"),
                    _ => ("coil", "neutral flow segment"),
                };
                secondary_structure.push(MolstarSecondaryElement {
                    kind,
                    start_residue_index: run_start + 1,
                    end_residue_index: idx,
                    note,
                });
                run_start = idx;
            }
        }
    }

    let mut word_last_residue: HashMap<String, usize> = HashMap::new();
    for (word_id, span) in &word_spans_map {
        word_last_residue.insert(word_id.clone(), span.residue_end);
    }

    let mut word_spans: Vec<MolstarWordSpan> = word_spans_map.into_values().collect();
    word_spans.sort_by(|a, b| {
        a.line_index
            .cmp(&b.line_index)
            .then(a.word_index.cmp(&b.word_index))
    });

    let mut ipa_lines_map: HashMap<usize, Vec<String>> = HashMap::new();
    let mut original_lines_map: HashMap<usize, Vec<String>> = HashMap::new();
    for span in &word_spans {
        ipa_lines_map
            .entry(span.line_index)
            .or_default()
            .push(span.ipa_word.clone());
        original_lines_map
            .entry(span.line_index)
            .or_default()
            .push(span.original_word.clone());
    }
    let max_line_idx = ipa_lines_map
        .keys()
        .chain(original_lines_map.keys())
        .copied()
        .max()
        .unwrap_or(0);
    let ipa_lines: Vec<String> = if ipa_lines_map.is_empty() && original_lines_map.is_empty() {
        Vec::new()
    } else {
        (0..=max_line_idx)
            .map(|idx| ipa_lines_map.get(&idx).map(|v| v.join(" ")).unwrap_or_default())
            .collect()
    };
    let original_lines: Vec<String> = if ipa_lines_map.is_empty() && original_lines_map.is_empty() {
        Vec::new()
    } else {
        (0..=max_line_idx)
            .map(|idx| original_lines_map.get(&idx).map(|v| v.join(" ")).unwrap_or_default())
            .collect()
    };

    let residue_line_index: HashMap<usize, usize> = word_spans
        .iter()
        .map(|span| (span.residue_end, span.line_index))
        .collect();

    let mut contact_dedupe: HashSet<(usize, usize, &'static str)> = HashSet::new();
    let mut contacts: Vec<MolstarContact> = Vec::new();

    for ann in &echo {
        let from_idx = ann.source.flat_index + 1;
        if let Some(nearest) = ann.nearest_match {
            if ann.gap <= 6.0 {
                let to_idx = nearest + 1;
                let (a, b) = if from_idx < to_idx { (from_idx, to_idx) } else { (to_idx, from_idx) };
                if contact_dedupe.insert((a, b, "similarity")) {
                    let strength = (ann.opacity * (1.0 - ann.gap / 8.0)).clamp(0.05, 0.45);
                    let equilibrium_distance = 3.6 + ann.gap * 0.35;
                    let decay_length = 2.0 + ann.gap * 0.4;
                    let spring_constant = (0.5 + 2.5 * strength).clamp(0.5, 2.0);
                    let energy = -(strength * 0.6);
                    contacts.push(MolstarContact {
                        kind: MolstarContactKind::SimilarityWeak,
                        from_residue_index: a,
                        to_residue_index: b,
                        strength,
                        equilibrium_distance,
                        decay_length,
                        spring_constant,
                        energy,
                        note: "short-range phoneme similarity".to_string(),
                    });
                }
            }
        }
    }

    let mut rhyme_groups: HashMap<String, Vec<(usize, f32)>> = HashMap::new();
    for (word_id, ann) in &annotations {
        if let Some(group) = &ann.rhyme_group {
            if let Some(&res_idx) = word_last_residue.get(word_id) {
                rhyme_groups
                    .entry(group.clone())
                    .or_default()
                    .push((res_idx, ann.rhyme_score.unwrap_or(0.7)));
            }
        }
    }

    for (group, members) in &rhyme_groups {
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let (a, sa) = members[i];
                let (b, sb) = members[j];
                let (from_idx, to_idx) = if a < b { (a, b) } else { (b, a) };
                if contact_dedupe.insert((from_idx, to_idx, "rhyme")) {
                    let strength = (0.65 + 0.35 * ((sa + sb) * 0.5)).clamp(0.65, 1.0);
                    let line_a = residue_line_index.get(&a).copied().unwrap_or(0) as i32;
                    let line_b = residue_line_index.get(&b).copied().unwrap_or(0) as i32;
                    let line_distance = (line_a - line_b).abs() as f32;
                    let equilibrium_distance = 7.0 + line_distance * 0.8;
                    let decay_length = 10.0;
                    let spring_constant = (4.0 + 4.0 * strength).clamp(4.0, 8.0);
                    let energy = -(1.2 * strength);
                    contacts.push(MolstarContact {
                        kind: MolstarContactKind::RhymeStrong,
                        from_residue_index: from_idx,
                        to_residue_index: to_idx,
                        strength,
                        equilibrium_distance,
                        decay_length,
                        spring_constant,
                        energy,
                        note: format!("rhyme-group {group}"),
                    });
                }
            }
        }
    }

    let mut pause_pattern_groups: HashMap<String, Vec<(usize, f32)>> = HashMap::new();
    for pause in &pauses {
        if let Some(&res_idx) = word_last_residue.get(&pause.after_word_id) {
            let key = match (&pause.punctuation, pause.has_line_break) {
                (Some(p), true) => format!("{p}+lb"),
                (Some(p), false) => p.clone(),
                (None, true) => "line_break".to_string(),
                (None, false) => continue,
            };
            pause_pattern_groups
                .entry(key)
                .or_default()
                .push((res_idx, pause.strength));
        }
    }

    for (pattern, mut members) in pause_pattern_groups {
        if members.len() < 3 {
            continue;
        }
        members.sort_by_key(|(idx, _)| *idx);
        let spacings: Vec<f32> = members
            .windows(2)
            .map(|w| w[1].0.saturating_sub(w[0].0) as f32)
            .collect();
        let spacing_std = stddev(&spacings);
        let regularity = (1.0 / (1.0 + spacing_std)).clamp(0.0, 1.0);
        if regularity < 0.35 {
            continue;
        }
        for pair in members.windows(2) {
            let (a_idx, a_strength) = pair[0];
            let (b_idx, b_strength) = pair[1];
            if b_idx.saturating_sub(a_idx) > 12 {
                continue;
            }
            let (from_idx, to_idx) = if a_idx < b_idx { (a_idx, b_idx) } else { (b_idx, a_idx) };
            if contact_dedupe.insert((from_idx, to_idx, "pause")) {
                let strength = (0.08 + 0.25 * ((a_strength + b_strength) * 0.5) * regularity)
                    .clamp(0.08, 0.35);
                let equilibrium_distance = 4.5 + (to_idx.saturating_sub(from_idx) as f32) * 0.25;
                let decay_length = 3.5;
                let spring_constant = (0.4 + 1.2 * strength).clamp(0.4, 1.0);
                let energy = -(0.25 * strength);
                contacts.push(MolstarContact {
                    kind: MolstarContactKind::PausePattern,
                    from_residue_index: from_idx,
                    to_residue_index: to_idx,
                    strength,
                    equilibrium_distance,
                    decay_length,
                    spring_constant,
                    energy,
                    note: format!("pause-pattern {pattern} (regularity={regularity:.2})"),
                });
            }
        }
    }

    contacts.sort_by(|a, b| {
        a.from_residue_index
            .cmp(&b.from_residue_index)
            .then(a.to_residue_index.cmp(&b.to_residue_index))
    });

    let fasta = format!(">IPA_PHONEME_CHAIN_A\n{sequence}\n");

    let mut pdb = String::new();
    pdb.push_str("HEADER    IPA PHONEME TO MOLSTAR TRANSCRIPTION\n");
    pdb.push_str("TITLE     PHONETIC ANALYSIS AS PSEUDO PROTEIN CHAIN\n");
    for line in &pdb_lines {
        pdb.push_str(line);
    }
    pdb.push_str("TER\nEND\n");

    let molstar = MolstarTranscription {
        format_version: "1.0",
        chain_id: 'A',
        sequence,
        fasta,
        pdb,
        secondary_structure,
        contacts,
        residue_map,
        word_spans,
        ipa_lines,
        original_lines,
        biophysical_model: MolstarBiophysicalModel {
            model_name: "ipa_biophysical_proxy_v2",
            backbone_step: 3.8,
            contact_energy_unit: "a.u.",
            distance_unit: "angstrom",
            similarity_contact_cutoff: 6.0,
            pause_pattern_min_repeat: 3,
            equations: vec![
                "E_similarity = -0.6 * S".to_string(),
                "E_rhyme = -1.2 * S".to_string(),
                "E_pause = -0.25 * S".to_string(),
                "S_similarity = opacity * max(0, 1 - gap/8)".to_string(),
                "S_rhyme = clamp(0.65 + 0.35 * mean(rhymeScorePair), 0.65, 1.0)".to_string(),
                "S_pause = clamp(0.08 + 0.25 * mean(pauseStrengthPair) * regularity, 0.08, 0.35)".to_string(),
            ],
        },
        interpretation: vec![
            "rhythm: mapped to secondary structure bands (helix/sheet/coil)".to_string(),
            "echo similarity: weak short-range contacts from nearest similar phonemes".to_string(),
            "rhyme: strong tertiary contacts between rhyme-group terminal residues".to_string(),
            "pause patterns: weak local contacts when punctuation/line-break patterns repeat".to_string(),
        ],
    };

    let structurality = compute_structurality(
        &all_words,
        &annotations,
        &rhythm,
        &echo,
        &pauses,
        &clusters,
        &flat_token_lines,
    );

    Ok(StreamAnalysisResult {
        version: FORMAT_VERSION,
        analyzer: AnalyzerInfo {
            name: ANALYZER_NAME,
            version: ANALYZER_VERSION,
        },
        response_schema: ResponseSchemaInfo {
            name: RESPONSE_SCHEMA_NAME,
            version: RESPONSE_SCHEMA_VERSION,
            dialect: RESPONSE_SCHEMA_DIALECT,
            file: RESPONSE_SCHEMA_FILE,
        },
        annotations,
        rhyme_pairs: rhyme_pairs_output,
        clusters,
        rhythm,
        echo,
        pauses,
        phonemes,
        molstar,
        structurality,
        metric_glossary: metric_glossary(),
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
    fn test_ngram_rhyme_detection() {
        // Test: N-gram sliding window detects similar phoneme subsequences.
        // 
        // Line 0: word_a(pa-ta-su) [6 phonemes]
        // Line 1: word_b(ka-su)    [4 phonemes]
        //
        // Expected: "asu" subsequence in word_a matches "asu" in word_b (after ka).
        //           Both words get rhyme groups based on n-gram similarity.
        let word_a = make_word("word_a", 0, 0, 0, &[("pa", &["p", "a"], false), ("ta", &["t", "a"], true), ("su", &["s", "u"], false)]);
        let lb    = r#"{"type":"line_break","lineIndex":0}"#.to_string();
        let word_b = make_word("word_b", 1, 0, 0, &[("ka", &["k", "a"], true), ("su", &["s", "u"], false)]);
        let lb2   = r#"{"type":"line_break","lineIndex":1}"#.to_string();
        let json  = wrap_stream(&[word_a, lb, word_b, lb2]);
        let stream = parse(&json);
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();

        // Both words should have rhyme groups (n-gram match found)
        let g_a = &result.annotations["word_a"].rhyme_group;
        let g_b = &result.annotations["word_b"].rhyme_group;
        assert!(g_a.is_some(), "word_a should have a rhyme group");
        assert!(g_b.is_some(), "word_b should have a rhyme group");
        assert_eq!(g_a, g_b, "words with matching n-grams should share a group");

        // Check rhyme_pairs contains at least one pair
        let pair_exists = result.rhyme_pairs.iter().any(|p| 
            (p.word_id_a == "word_a" && p.word_id_b == "word_b") ||
            (p.word_id_a == "word_b" && p.word_id_a == "word_a")
        );
        assert!(pair_exists, "rhyme_pairs should contain word_a-word_b match");

        // Verify match length is at least 2 (substring match)
        let max_length = result.rhyme_pairs.iter()
            .filter(|p| 
                (p.word_id_a == "word_a" && p.word_id_b == "word_b") ||
                (p.word_id_a == "word_b" && p.word_id_b == "word_a")
            )
            .map(|p| p.match_length)
            .max()
            .unwrap_or(0);
        assert!(max_length >= 2, "n-gram match should be at least 2 phonemes (got {})", max_length);
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
        assert!(json_str.contains("\"analyzer\""));
        assert!(json_str.contains("\"responseSchema\""));
        assert!(json_str.contains("\"annotations\""));
        assert!(json_str.contains("\"clusters\""));
        assert!(json_str.contains("\"phonemes\""));
        assert!(json_str.contains("\"structurality\""));
    }

    #[test]
    fn test_stream_result_version_field_is_1_1() {
        let w = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let stream = parse(&wrap_stream(&[w]));
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        assert_eq!(result.version, "1.1");
        assert_eq!(result.analyzer.name, "phonetic-poetry-engine");
        assert_eq!(result.analyzer.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(result.response_schema.name, "StreamAnalysisResult");
        assert_eq!(result.response_schema.file, "stream_analysis.response.schema.json");
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

    #[test]
    fn test_structurality_scores_are_in_unit_interval() {
        let w1 = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let lb = r#"{"type":"line_break","lineIndex":0}"#.to_string();
        let w2 = make_word("t2", 1, 0, 0, &[("pa", &["p", "a"], true)]);
        let stream = parse(&wrap_stream(&[w1, lb, w2]));
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        let s = result.structurality;

        for value in [
            s.rhythm.score,
            s.local_phoneme_patterning.score,
            s.sound_sequence_patterning.score,
            s.pause_patterning.score,
            s.cross_level_coupling.score,
            s.global,
        ] {
            assert!((0.0..=1.0).contains(&value), "score {value} should be within [0,1]");
        }
    }

    #[test]
    fn test_structurality_weights_sum_to_one() {
        let w = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let stream = parse(&wrap_stream(&[w]));
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();
        let weights = result.structurality.weights;
        let sum = weights.rhythm
            + weights.local_phoneme_patterning
            + weights.sound_sequence_patterning
            + weights.pause_patterning
            + weights.cross_level_coupling;
        assert!((sum - 1.0).abs() < 1e-6, "weights should sum to 1, got {sum}");
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

    #[test]
    fn test_phoneme_payload_is_complete_and_sorted() {
        let w1 = make_word("t1", 0, 0, 0, &[("pa", &["p", "a"], true)]);
        let w2 = make_word("t2", 0, 1, 0, &[("su", &["s", "u"], true)]);
        let stream = parse(&wrap_stream(&[w1, r#"{"type":"whitespace"}"#.to_string(), w2]));
        let reg = reg();
        let result = analyze_stream(&stream, &reg).unwrap();

        assert_eq!(result.phonemes.total, result.phonemes.entries.len());
        assert_eq!(result.phonemes.sorted_by, "source.flatIndex asc");
        assert_eq!(result.phonemes.feature_schema.len(), registry::FEATURE_NAMES.len());
        assert_eq!(result.phonemes.entries.len(), 4);

        for (idx, item) in result.phonemes.entries.iter().enumerate() {
            assert_eq!(item.source.flat_index, idx);
            assert_eq!(item.vector.len(), registry::FEATURE_NAMES.len());
            assert_eq!(item.features.len(), registry::FEATURE_NAMES.len());
        }

        assert_eq!(result.molstar.chain_id, 'A');
        assert_eq!(result.molstar.sequence.len(), result.phonemes.entries.len());
        assert!(result.molstar.pdb.contains("ATOM"));
        assert!(!result.molstar.residue_map.is_empty());
        assert_eq!(result.molstar.word_spans.len(), 2);
        assert_eq!(result.molstar.original_lines, vec![" ".to_string()]);
        assert!(!result.molstar.ipa_lines.is_empty());
        assert_eq!(result.molstar.word_spans[0].word_id, "t1");
        assert_eq!(result.molstar.word_spans[0].ipa_word, "pa");

        let first = &result.molstar.residue_map[0];
        assert_eq!(first.original_word, "");
        assert_eq!(first.symbol, "p");
        assert_eq!(first.source.word_id, "t1");
        assert!(result.molstar.biophysical_model.model_name.contains("ipa_biophysical_proxy"));
    }
}

