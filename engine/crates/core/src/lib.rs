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
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use crate::registry::{FEATURE_DESCRIPTIONS, FEATURE_NAMES};

use algorithms::density::{find_clusters, sliding_window, IDX_DELREL, IDX_NAS, IDX_STRID, IDX_SYL, IDX_LAT};
use algorithms::distance::cosine_similarity;
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

/// Connected rhyme component that can contain 2+ words.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RhymeGroup {
    /// Rhyme group letter (A, B, C, ...).
    pub group_id: String,
    /// Word IDs that belong to this rhyme group.
    pub word_ids: Vec<String>,
    /// Number of rhyme pair edges inside this connected group.
    pub pair_count: usize,
    /// Mean DTW similarity across group's pair edges.
    pub average_similarity: f32,
    /// Maximum DTW similarity across group's pair edges.
    pub max_similarity: f32,
    /// Mean weighted score across group's pair edges.
    pub average_weighted_score: f32,
    /// Maximum weighted score across group's pair edges.
    pub max_weighted_score: f32,
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
    /// Connected rhyme groups derived from `rhyme_pairs` (can be larger than pairs).
    pub rhyme_groups: Vec<RhymeGroup>,
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
            id: "rhyme_groups.average_similarity",
            source: "rhyme_groups[]",
            description: "Mean DTW similarity of all pair edges within one rhyme group.",
            interpretation: "Higher means group members are consistently similar, not just linked by one weak bridge.",
        },
        MetricGlossaryEntry {
            id: "rhyme_groups.max_similarity",
            source: "rhyme_groups[]",
            description: "Maximum DTW similarity among pair edges inside a rhyme group.",
            interpretation: "Use to find each group's strongest anchor rhyme.",
        },
        MetricGlossaryEntry {
            id: "rhyme_groups.average_weighted_score",
            source: "rhyme_groups[]",
            description: "Mean weighted score (similarity * sqrt(matchLength)) across group edges.",
            interpretation: "Balances phonetic similarity with match length for stable ranking.",
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

#[derive(Debug)]
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            let root = self.find(self.parent[x]);
            self.parent[x] = root;
        }
        self.parent[x]
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }

        if self.rank[ra] < self.rank[rb] {
            self.parent[ra] = rb;
        } else if self.rank[ra] > self.rank[rb] {
            self.parent[rb] = ra;
        } else {
            self.parent[rb] = ra;
            self.rank[ra] = self.rank[ra].saturating_add(1);
        }
    }
}

fn compute_rhyme_grouping(
    all_words: &[&stream::IpaStreamWord],
    rhyme_pairs: &[RhymePair],
) -> (Vec<Option<String>>, Vec<f32>, Vec<RhymeGroup>) {
    let n = all_words.len();
    let mut best_score: Vec<f32> = vec![0.0; n];
    let mut involved: Vec<bool> = vec![false; n];

    // Build index: word_id -> word_index
    let word_id_to_idx: HashMap<&str, usize> = all_words
        .iter()
        .enumerate()
        .map(|(idx, w)| (w.id.as_str(), idx))
        .collect();

    let mut uf = UnionFind::new(n);

    for pair in rhyme_pairs {
        let i = word_id_to_idx.get(pair.word_id_a.as_str()).copied();
        let j = word_id_to_idx.get(pair.word_id_b.as_str()).copied();

        if let (Some(i), Some(j)) = (i, j) {
            involved[i] = true;
            involved[j] = true;

            if pair.similarity > best_score[i] {
                best_score[i] = pair.similarity;
            }
            if pair.similarity > best_score[j] {
                best_score[j] = pair.similarity;
            }

            uf.union(i, j);
        }
    }

    let mut root_to_group: HashMap<usize, String> = HashMap::new();
    let mut group_of: Vec<Option<String>> = vec![None; n];
    let mut group_counter: u8 = b'A';

    for idx in 0..n {
        if !involved[idx] {
            continue;
        }

        let root = uf.find(idx);
        let group = root_to_group.entry(root).or_insert_with(|| {
            let letter = (group_counter as char).to_string();
            group_counter = group_counter.saturating_add(1);
            letter
        });
        group_of[idx] = Some(group.clone());
    }

    #[derive(Default)]
    struct GroupStats {
        pair_count: usize,
        similarity_sum: f32,
        similarity_max: f32,
        weighted_sum: f32,
        weighted_max: f32,
    }

    let mut stats_by_group: HashMap<String, GroupStats> = HashMap::new();
    for pair in rhyme_pairs {
        let i = word_id_to_idx.get(pair.word_id_a.as_str()).copied();
        let j = word_id_to_idx.get(pair.word_id_b.as_str()).copied();
        if let (Some(i), Some(j)) = (i, j) {
            let gi = group_of[i].as_ref();
            let gj = group_of[j].as_ref();
            if let (Some(gi), Some(gj)) = (gi, gj) {
                if gi == gj {
                    let stats = stats_by_group.entry(gi.clone()).or_default();
                    stats.pair_count += 1;
                    stats.similarity_sum += pair.similarity;
                    stats.similarity_max = stats.similarity_max.max(pair.similarity);
                    stats.weighted_sum += pair.weighted_score;
                    stats.weighted_max = stats.weighted_max.max(pair.weighted_score);
                }
            }
        }
    }

    let mut grouped_word_ids: HashMap<String, Vec<String>> = HashMap::new();
    for (idx, maybe_group) in group_of.iter().enumerate() {
        if let Some(group) = maybe_group {
            grouped_word_ids
                .entry(group.clone())
                .or_default()
                .push(all_words[idx].id.clone());
        }
    }

    let mut rhyme_groups: Vec<RhymeGroup> = grouped_word_ids
        .into_iter()
        .map(|(group_id, mut word_ids)| {
            word_ids.sort();
            let stats = stats_by_group.remove(&group_id).unwrap_or_default();
            let denom = if stats.pair_count == 0 { 1.0 } else { stats.pair_count as f32 };
            RhymeGroup {
                group_id,
                word_ids,
                pair_count: stats.pair_count,
                average_similarity: stats.similarity_sum / denom,
                max_similarity: stats.similarity_max,
                average_weighted_score: stats.weighted_sum / denom,
                max_weighted_score: stats.weighted_max,
            }
        })
        .collect();
    rhyme_groups.sort_by(|a, b| a.group_id.cmp(&b.group_id));

    (group_of, best_score, rhyme_groups)
}

// ────────────────────────────────────────────────────────────────────────────
// Rhyme grouping threshold
// ────────────────────────────────────────────────────────────────────────────

/// Minimum average cosine similarity for a phoneme alignment to be kept
/// as a rhyme candidate.
const RHYME_THRESHOLD: f32 = 0.40;

/// Minimum phoneme count in an alignment to qualify as rhyme candidate.
const MIN_ALIGNMENT_LENGTH: usize = 2;



// ────────────────────────────────────────────────────────────────────────────
// Self-self Smith-Waterman local alignment — find similar phoneme subsequences
// across the full flattened phoneme sequence, ignoring word boundaries.
// ────────────────────────────────────────────────────────────────────────────

/// Find all similar phoneme subsequence pairs across the full flattened
/// phoneme sequence using Smith-Waterman local alignment applied to a
/// single sequence against itself.
///
/// Algorithm:
/// 1. Compute an N×N DP matrix where cell (i,j) stores the best local
///    alignment score ending at positions i, j (i < j, skip diagonal).
///    Score per position = 2·cosine_similarity(feats[i], feats[j]) - 1,
///    giving positive contribution when cos_sim > 0.5.
/// 2. Extract alignments at their end points (where the next diagonal
///    cell resets to zero or sequence boundary is reached).
/// 3. Trace back to find the start, compute average similarity from the
///    cumulative DP score, and emit a RhymePair per alignment.
/// 4. Sort by weighted_score = avg_similarity × √length.
///
/// Complexity: O(N²) in phoneme tokens (vs O(M²) in n-grams previously).
///
/// Bounds: worst-case output is N×(N-1)/2 RhymePairs, one per possible
/// alignment end. In practice, the cos_sim > 0.5 filter keeps this to
/// a few thousand for typical poems.
fn find_ngram_rhyme_matches(
    flat_tokens: &[tokenizer::PhoneticToken],
    flat_context: &[FlatPhonemeContext],
    _all_words: &[&stream::IpaStreamWord],
) -> Vec<RhymePair> {
    let n = flat_tokens.len();
    if n < MIN_ALIGNMENT_LENGTH {
        return Vec::new();
    }

    // ── Step 1: Compute Smith-Waterman DP matrix ───────────────────────
    // dp[i][j] = cumulative score of the best local alignment ending at
    // positions i, j in the flat token sequence.
    // Only upper triangle (i < j) is filled — the diagonal is skipped.
    let mut dp = vec![vec![0.0f32; n]; n];

    for i in 0..n {
        for j in (i + 1)..n {
            let cos_sim = cosine_similarity(
                flat_tokens[i].features.view(),
                flat_tokens[j].features.view(),
            );
            let score = 2.0 * cos_sim - 1.0;

            let prev = if i > 0 && j > 0 { dp[i - 1][j - 1] } else { 0.0 };
            dp[i][j] = 0.0f32.max(prev + score);
        }
    }

    // ── Step 2: Extract alignments at their end points ─────────────────
    let mut matches: Vec<RhymePair> = Vec::new();

    for i in 0..n {
        for j in (i + 1)..n {
            let val = dp[i][j];
            if val <= 0.0 { continue; }

            // An alignment ends when the next diagonal cell resets to zero
            // (or we're at the sequence boundary).
            let at_end = i + 1 >= n || j + 1 >= n || dp[i + 1][j + 1] == 0.0;
            if !at_end { continue; }

            // Trace back to find the start of this alignment
            let (mut si, mut sj) = (i, j);
            let mut len = 1usize;

            while si > 0 && sj > 0 && dp[si - 1][sj - 1] > 0.0 {
                si -= 1;
                sj -= 1;
                len += 1;
            }

            if len < MIN_ALIGNMENT_LENGTH { continue; }

            // Average similarity derived from cumulative DP score:
            //   dp[i][j] = Σ(2·cos - 1) = 2·Σ(cos) - len
            //   ⇒ avg_sim = Σ(cos) / len = (dp + len) / (2·len)
            let avg_sim = 0.5 + val / (2.0 * len as f32);
            if avg_sim < RHYME_THRESHOLD { continue; }

            let weighted_score = avg_sim * (len as f32).sqrt();

            let strength_tier = if weighted_score >= 2.5 {
                "strong"
            } else if weighted_score >= 1.5 {
                "medium"
            } else {
                "weak"
            }.to_string();

            matches.push(RhymePair {
                word_id_a: flat_context[si].word_id.clone(),
                word_id_b: flat_context[sj].word_id.clone(),
                similarity: avg_sim,
                match_length: len,
                position_a: si,
                position_b: sj,
                sequence_a: flat_context[si..=i].iter().map(|c| c.symbol.clone()).collect(),
                sequence_b: flat_context[sj..=j].iter().map(|c| c.symbol.clone()).collect(),
                weighted_score,
                strength_tier,
            });
        }
    }

    // ── Step 3: Sort by weighted score descending ──────────────────────
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
    is_stressed_syllable: bool,
}
const ANALYZER_NAME: &str = "phonetic-poetry-engine";
const ANALYZER_VERSION: &str = env!("CARGO_PKG_VERSION");
const RESPONSE_SCHEMA_NAME: &str = "StreamAnalysisResult";
const RESPONSE_SCHEMA_VERSION: &str = "1.1";
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
                    is_stressed_syllable,
                });
            }
        }
    }

    debug_assert_eq!(flat_context.len(), flat_tokens.len());

    // ── Smith-Waterman local alignment over full phoneme sequence ────────
    // Find all pairs of similar phoneme subsequences via self-self DP,
    // ignoring word boundaries. This catches:
    // - Cross-word rhymes (кар-то-ма зо-на → "тома-зо")
    // - Internal rhymes (ор-ка ↔ он-ка)
    // - Anagrammatic rhymes (пужд ↔ ждуб)
    // - Variable-length rhymes (кар-то ↔ кр-то)
    let rhyme_pairs_output = find_ngram_rhyme_matches(&flat_tokens, &flat_context, &all_words);

    // ── Rhyme grouping from connected components over pair graph ─────────
    let (group_of, best_score, rhyme_groups_output) =
        compute_rhyme_grouping(&all_words, &rhyme_pairs_output);


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
        rhyme_groups: rhyme_groups_output,
        clusters,
        rhythm,
        echo,
        pauses,
        phonemes,
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

    #[test]
    fn test_rhyme_grouping_merges_connected_pairs() {
        let words = vec![
            stream::IpaStreamWord {
                id: "w1".to_string(),
                line_index: 0,
                word_index: 0,
                language: "uk".to_string(),
                original: "w1".to_string(),
                syllable_count: 1,
                stressed_syllable: 0,
                stress_source: stream::StressSource::Ml,
                syllables: Vec::new(),
            },
            stream::IpaStreamWord {
                id: "w2".to_string(),
                line_index: 0,
                word_index: 1,
                language: "uk".to_string(),
                original: "w2".to_string(),
                syllable_count: 1,
                stressed_syllable: 0,
                stress_source: stream::StressSource::Ml,
                syllables: Vec::new(),
            },
            stream::IpaStreamWord {
                id: "w3".to_string(),
                line_index: 0,
                word_index: 2,
                language: "uk".to_string(),
                original: "w3".to_string(),
                syllable_count: 1,
                stressed_syllable: 0,
                stress_source: stream::StressSource::Ml,
                syllables: Vec::new(),
            },
        ];

        let make_pair = |a: &str, b: &str| RhymePair {
            word_id_a: a.to_string(),
            word_id_b: b.to_string(),
            similarity: 0.8,
            match_length: 3,
            position_a: 0,
            position_b: 0,
            sequence_a: vec!["a".to_string()],
            sequence_b: vec!["a".to_string()],
            weighted_score: 0.8,
            strength_tier: "strong".to_string(),
        };

        // Chain graph: w1-w2 and w2-w3 must produce one 3-word rhyme group.
        let pairs = vec![make_pair("w1", "w2"), make_pair("w2", "w3")];
        let word_refs: Vec<&stream::IpaStreamWord> = words.iter().collect();
        let (group_of, _best, groups) = compute_rhyme_grouping(&word_refs, &pairs);

        assert_eq!(group_of[0], group_of[1]);
        assert_eq!(group_of[1], group_of[2]);
        assert_eq!(groups.len(), 1, "expected one connected rhyme group");
        assert_eq!(groups[0].word_ids.len(), 3, "group should contain 3 words");
        assert_eq!(groups[0].pair_count, 2, "group should include both pair edges");
        assert!((groups[0].average_similarity - 0.8).abs() < 1e-6);
        assert!((groups[0].max_similarity - 0.8).abs() < 1e-6);
        assert!((groups[0].average_weighted_score - 0.8).abs() < 1e-6);
        assert!((groups[0].max_weighted_score - 0.8).abs() < 1e-6);
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
        assert_eq!(result.response_schema.version, "1.1");
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

        let first = &result.phonemes.entries[0];
        assert_eq!(first.source.word_id, "t1");
        assert_eq!(first.symbol, "p");
    }

    // ── Stress test: moderate poem with ~30 words ──────────────────────
    // Reproduces the WASM-unreachable crash from O(n²) n-gram pair comparisons.
    // Kept small to complete in debug mode.

    #[test]
    fn test_moderate_poem_does_not_crash_with_ngrams() {
        let reg = reg();

        // Build a 30-word / 10-line stream JSON.
        let mut items: Vec<String> = Vec::new();
        let mut word_id = 0usize;
        for line_idx in 0..10 {
            let word_count = (line_idx % 3) + 2; // 2..4 words per line
            for w in 0..word_count {
                let syl_count = (w % 2) + 1; // 1..2 syllables per word
                let mut syllables = Vec::new();
                for s in 0..syl_count {
                    let phoneme = if s == 0 { "k" } else { "a" };
                    syllables.push(format!(
                        r#"{{"ipa":"{phoneme}","tokens":["{phoneme}"],"grapheme":"","stressed":false,"isOpen":true}}"#,
                    ));
                }
                let syll_list = syllables.join(",");
                items.push(format!(
                    r#"{{"type":"word","id":"w{word_id}","lineIndex":{line_idx},"wordIndex":{w},"language":"uk","original":"","syllableCount":{syl_count},"stressedSyllable":0,"stressSource":"dict","syllables":[{syll_list}]}}"#
                ));
                word_id += 1;
            }
            items.push(format!(r#"{{"type":"line_break","lineIndex":{line_idx}}}"#));
        }

        let total_words = word_id;
        let json = format!(
            r#"{{"metadata":{{"version":"1.1","generatedAt":"2026-01-01T00:00:00.000Z","confirmedLineCount":10,"totalWordCount":{total_words},"languagesPresent":["uk"]}},"stream":[{}]}}"#,
            items.join(",")
        );

        let stream = IpaStream::from_json_bytes(json.as_bytes()).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = analyze_stream(&stream, &reg);
        }));
        assert!(result.is_ok(), "moderate poem with n-gram rhyme detection should not panic (OOM / unreachable)");
    }
}

