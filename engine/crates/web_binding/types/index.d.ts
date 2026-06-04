/**
 * ipa-poetry-engine — TypeScript type definitions
 *
 * Input:  IPA Stream v1.1  (produced by the G2P frontend pipeline)
 * Output: StreamAnalysisResult  (produced by this WASM module)
 *
 * All field names use camelCase (matching Rust `#[serde(rename_all = "camelCase")]`).
 *
 * Round-trip mapping guarantee:
 *   Every output annotation references input elements exclusively by stable IDs:
 *   - WordAnnotation        → keyed by IpaStreamWord.id
 *   - SyllableRef.wordId    → IpaStreamWord.id  +  .syllableIndex
 *   - PhonemeRef.wordId     → IpaStreamWord.id  +  .syllableIndex  +  .phonemeIndex
 *   - PauseAnnotation.afterWordId → IpaStreamWord.id of the preceding word
 */

// ═══════════════════════════════════════════════════════════════════════════
// INPUT — IPA Stream v1.1
// ═══════════════════════════════════════════════════════════════════════════

/** Document-level metadata block. */
export interface IpaStreamMetadata {
  /** Always "1.1" for documents compatible with this engine. */
  version: "1.1";
  /** ISO 8601 timestamp when the document was generated. */
  generatedAt: string;
  /** Number of confirmed poetic lines (= unique lineIndex values on Word elements). */
  confirmedLineCount: number;
  /** Total number of word tokens in the document. */
  totalWordCount: number;
  /** BCP-47 language codes of languages present (e.g. ["uk", "en"]). */
  languagesPresent: string[];
}

/** One syllable within a word. */
export interface IpaStreamSyllable {
  /** IPA transcription of this syllable (e.g. "zɑ"). */
  ipa: string;
  /**
   * Ordered list of IPA token strings that make up this syllable.
   * Each token is a single phoneme or diacritic cluster as in phonemes.json.
   * These are the units passed to the feature registry for look-up.
   */
  tokens: string[];
  /** Source grapheme string for this syllable (may be empty for synthetic tokens). */
  grapheme: string;
  /** Whether this syllable carries the primary lexical stress. */
  stressed: boolean;
  /** Whether the syllable ends in a vowel (open syllable). */
  isOpen: boolean;
}

/** Source of the stress assignment for a word. */
export type StressSource = "dict" | "ml" | "manual";

/** A lexical word token in the stream. */
export interface IpaStreamWord {
  type: "word";
  /** Stable opaque identifier (e.g. "tok_14_t1kht"). Used as primary key in output. */
  id: string;
  /** 0-based index of the confirmed poetic line this word belongs to. */
  lineIndex: number;
  /** 0-based position of this word within its line. */
  wordIndex: number;
  /** BCP-47 language code. */
  language: string;
  /** Original surface form (before G2P). */
  original: string;
  /** Number of syllables. */
  syllableCount: number;
  /**
   * 0-based index of the stressed syllable, or -1 if the word carries no
   * primary stress (e.g. clitics, prepositions).
   */
  stressedSyllable: number;
  /** How the stress assignment was determined. */
  stressSource: StressSource;
  /** Syllable sequence in pronunciation order. */
  syllables: IpaStreamSyllable[];
}

/** Whitespace between two stream elements. Carries no phonetic content. */
export interface IpaStreamWhitespace {
  type: "whitespace";
}

/** Punctuation mark. Triggers a prosodic pause in the output. */
export interface IpaStreamPunctuation {
  type: "punctuation";
  /**
   * Normalised punctuation text.
   * Common values: "," | "." | "?" | "!" | ";" | ":" | "—" | "–"
   */
  text: string;
}

/**
 * Structural line break.
 * `lineIndex` marks the line that just ended (matches Word.lineIndex values on
 * preceding words). Analytically invisible — do NOT use this to split lines;
 * use Word.lineIndex instead.
 */
export interface IpaStreamLineBreak {
  type: "line_break";
  lineIndex: number;
}

/** Discriminated union of all possible stream elements. */
export type StreamElement =
  | IpaStreamWord
  | IpaStreamWhitespace
  | IpaStreamPunctuation
  | IpaStreamLineBreak;

/** Top-level IPA Stream v1.1 document — the only accepted input format. */
export interface IpaStream {
  metadata: IpaStreamMetadata;
  stream: StreamElement[];
}

// ═══════════════════════════════════════════════════════════════════════════
// OUTPUT — StreamAnalysisResult
// ═══════════════════════════════════════════════════════════════════════════

// ── Rhyme & word annotations ─────────────────────────────────────────────

/**
 * Detected rhyme pair between two words with similarity score.
 * Returned as an array in StreamAnalysisResult.rhymePairs.
 *
 * Use these pairs for flexible client-side grouping:
 * - Filter by `similarity >= threshold` to adjust rhyme detection sensitivity
 * - Group by connected components to create custom rhyme schemes
 * - Color-code by similarity strength for visual rhyme density heatmaps
 *
 * Example: "дон" ↔ "зон" might have similarity ~0.6 (60% phoneme match).
 * You can set threshold at 0.5 to group them, or 0.7 to separate them.
 */
export interface RhymePair {
  /** ID of the first word in the pair (from IpaStreamWord.id). */
  wordIdA: string;
  /** ID of the second word in the pair (from IpaStreamWord.id). */
  wordIdB: string;
  /** DTW phonetic similarity score [0, 1], higher = stronger rhyme. */
  similarity: number;
  /** Length of the coda sequence for word A (number of phonemes). */
  codaLengthA: number;
  /** Length of the coda sequence for word B (number of phonemes). */
  codaLengthB: number;
}

/**
 * Per-word annotation.
 * Returned as a map keyed by IpaStreamWord.id.
 *
 * Mapping: annotations[word.id] → WordAnnotation
 */
export interface WordAnnotation {
  /** Mirrors IpaStreamWord.lineIndex — for direct ABBA-pattern detection. */
  lineIndex: number;
  /** Mirrors IpaStreamWord.wordIndex — for column alignment in the UI. */
  wordIndex: number;
  /**
   * Rhyme group label (A, B, C, …) shared by all words whose coda phoneme
   * sequence is phonologically similar (DTW score ≥ threshold).
   * Covers end-rhymes, internal rhymes, and anaphoric rhymes uniformly.
   * `null` if no rhyming partner was found.
   */
  rhymeGroup: string | null;
  /**
   * Best DTW similarity score [0, 1] against any rhyming partner.
   * `null` when rhymeGroup is null.
   */
  rhymeScore: number | null;
  /**
   * Confidence in the stress assignment [0, 1].
   * "dict" → 0.95 | "manual" → 1.0 | "ml" → 0.75
   */
  stressConfidence: number;
  /**
   * Structural rhyme group: words that share the same syllabic shape
   * (onset consonant count × coda consonant count) from the stressed syllable
   * onwards. Independent of actual phoneme identity. `null` if shape is unique.
   */
  structuralRhymeGroup: string | null;
}

// ── Sound clusters ────────────────────────────────────────────────────────

/** Type of phonological cluster detected by the sliding-window density algorithm. */
export type ClusterKind =
  | "sibilant" // strident feature — /s z ʃ ʒ/ family
  | "affricate" // delayed-release feature — /tʃ dʒ ts dz/ family
  | "nasal" // nasal feature — /m n ŋ/ family
  | "lateral" // lateral feature — /l lʲ/ family
  | "assonance"; // syllabic feature — vowel density

/**
 * A zone of phonological density detected in the flat phoneme stream.
 * Positions are indices into the flat phoneme token array (all phonemes from
 * all words in stream order, boundaries excluded).
 */
export interface Cluster {
  /** Type of acoustic feature driving this cluster. */
  kind: ClusterKind;
  /** Index of the first phoneme in the cluster (inclusive). */
  start: number;
  /** Index of the last phoneme in the cluster (inclusive). */
  end: number;
  /** Peak density value within the sliding window that formed this cluster. */
  peak: number;
}

// ── Rhythm ───────────────────────────────────────────────────────────────

/** Stable reference to a syllable in the input document. */
export interface SyllableRef {
  /** IpaStreamWord.id of the containing word. */
  wordId: string;
  /** 0-based index into IpaStreamWord.syllables. */
  syllableIndex: number;
  /** 0-based position within the line's flat syllable sequence. */
  linePosition: number;
}

/** How a syllable's actual stress relates to the detected metre. */
export type DeviationType =
  | "match" // actual stress agrees with the expected metrical position
  | "pyrrhic" // expected stress was absent (skipped)
  | "spondee"; // unexpected stress on a weak position

/** Stress annotation for one syllable. */
export interface SyllableAnnotation {
  /** Locates this syllable in the input document. */
  syllableRef: SyllableRef;
  /** Actual stress weight: 1.0 = stressed, 0.0 = unstressed. */
  stress: number;
  /** Expected stress under the detected metre (0.0 or 1.0). */
  expected: number;
  /** Classification relative to the metre. */
  deviation: DeviationType;
}

/** How the line ends relative to its last stressed syllable. */
export type Clausula =
  | "masculine" // last stress on the final syllable
  | "feminine" // last stress on the penultimate syllable
  | "dactylic" // last stress on the antepenultimate syllable
  | "hyperdactylic"; // last stress four+ syllables from the end

/** Full rhythm analysis for one confirmed poetic line. */
export interface LineRhythm {
  /** 0-based index matching IpaStreamWord.lineIndex values. */
  lineIndex: number;
  /** Dominant metre period: 2 (binary/iambic/trochaic) or 3 (ternary). */
  period: 2 | 3;
  /** Phase offset within the period where stress is expected (0-based). */
  phase: number;
  /** Ending pattern of the line. */
  clausula: Clausula;
  /**
   * Regularity score [0, 1].
   * 1.0 = every syllable perfectly matches the pattern.
   * Typical values: iambic Ukrainian ≈ 0.75–0.90.
   */
  confidence: number;
  /** Per-syllable annotations in line order. */
  syllables: SyllableAnnotation[];
  /** Total syllable count across all words in this line. */
  syllableCount: number;
}

// ── Echo / alliteration opacity ───────────────────────────────────────────

/** Stable reference to a phoneme in the input document. */
export interface PhonemeRef {
  /** IpaStreamWord.id of the containing word. */
  wordId: string;
  /** 0-based index into IpaStreamWord.syllables. */
  syllableIndex: number;
  /** 0-based index into IpaStreamSyllable.tokens. */
  phonemeIndex: number;
  /** Position in the flat phoneme-only stream (word boundaries excluded). */
  flatIndex: number;
}

/**
 * Echo opacity annotation for one phoneme.
 *
 * Opacity follows: `opacity = max(αMin, exp(-gap / λ))`
 * where gap is the phoneme-distance to the nearest similar phoneme.
 */
export interface EchoAnnotation {
  /** Identifies this phoneme in the input document. */
  source: PhonemeRef;
  /**
   * flatIndex of the nearest phoneme with cosine similarity ≥ 0.80.
   * `null` if no match was found within the stream.
   */
  nearestMatch: number | null;
  /**
   * Distance to the nearest match in phoneme units.
   * Equals stream length when no match exists (drives opacity to αMin).
   */
  gap: number;
  /**
   * Visual opacity in [0.05, 1.0].
   * Higher = phoneme is part of a denser sound cluster.
   */
  opacity: number;
}

// ── Full per-phoneme payload ─────────────────────────────────────────────

/** Canonical phonological feature metadata item (Panphon-compatible). */
export interface PhonemeFeatureSchemaItem {
  /** Feature key (e.g. "syl", "cons", "lab"). */
  key: string;
  /** Human-readable description (e.g. "syllabic"). */
  description: string;
}

/** Tri-state feature encoding legend used in `phonemes.features[].sign`. */
export interface PhonemeFeatureEncoding {
  positive: string;
  negative: string;
  unspecified: string;
}

/** One named feature value for a phoneme token. */
export interface PhonemeFeatureValue {
  key: string;
  description: string;
  /** Numeric encoded value from the feature vector (-1, 0, +1). */
  value: number;
  /** Sign label corresponding to `value`: "+" | "-" | "0". */
  sign: "+" | "-" | "0";
}

/** Derived natural (intrinsic) properties normalised to [0, 1]. */
export interface PhonemeNaturalProfile {
  classHint: string;
  vowelness: number;
  consonantality: number;
  sonority: number;
  voicing: number;
  labiality: number;
  coronality: number;
  nasality: number;
  stridency: number;
  laterality: number;
  highness: number;
  openness: number;
  backness: number;
  roundedness: number;
  tenseness: number;
  lengthness: number;
  toneHeight: number;
}

/** Contribution of one detected cluster kind to a concrete phoneme. */
export interface PhonemeClusterContribution {
  kind: ClusterKind;
  peak: number;
}

/** Computed (contextual) metrics attached to one phoneme. */
export interface PhonemeComputedMetrics {
  lineIndex: number;
  wordIndex: number;
  isStressedSyllable: boolean;
  stressWeight: number;
  rhymeGroup: string | null;
  structuralRhymeGroup: string | null;
  nearestMatchFlatIndex: number | null;
  echoGap: number;
  echoOpacity: number;
  clusterMembershipCount: number;
  clusterPeakMax: number;
  clusterContributions: PhonemeClusterContribution[];
}

/** Full payload for one phoneme in stream order. */
export interface PhonemeRecord {
  source: PhonemeRef;
  symbol: string;
  /** Raw 24-dim vector in canonical feature order. */
  vector: number[];
  /** Named feature values (same order as `phonemes.featureSchema`). */
  features: PhonemeFeatureValue[];
  naturalProfile: PhonemeNaturalProfile;
  computedMetrics: PhonemeComputedMetrics;
}

/** Full phoneme layer sorted by `source.flatIndex`. */
export interface PhonemeLayer {
  total: number;
  sortedBy: "source.flatIndex asc" | string;
  featureSchema: PhonemeFeatureSchemaItem[];
  valueEncoding: PhonemeFeatureEncoding;
  entries: PhonemeRecord[];
}

// ── Prosodic pauses ───────────────────────────────────────────────────────

/**
 * A prosodic boundary detected from punctuation and/or line breaks.
 *
 * Strength reference:
 *   bare line break   → 0.35
 *   ","               → 0.25     "," + line break → 0.40
 *   ";" / ":"         → 0.50     ";" + line break → 0.65
 *   "—" / "–"        → 0.60     "—" + line break → 0.75
 *   "." / "?" / "!"  → 0.75     "." + line break → 0.90
 */
export interface PauseAnnotation {
  /** IpaStreamWord.id of the word immediately before this pause. */
  afterWordId: string;
  /** The punctuation character(s), or null for a bare line break. */
  punctuation: string | null;
  /** Whether a structural line break immediately follows the punctuation. */
  hasLineBreak: boolean;
  /** Normalised pause strength in [0, 1]. */
  strength: number;
}

// ── Structural complexity / global structurality ─────────────────────────

/**
 * One normalised structurality component.
 *
 * Interpretation:
 * - `rawSignal`  : directly measured signal in [0, 1]
 * - `baseline`   : heuristic null-model floor below which the signal is treated
 *                  as indistinguishable from ordinary distributional noise
 * - `score`      : baseline-corrected structural complexity in [0, 1]
 */
export interface StructuralityComponent {
  /** Raw measured signal before baseline correction. */
  rawSignal: number;
  /** Null-model floor for this plane. */
  baseline: number;
  /** Baseline-corrected structural complexity coefficient in [0, 1]. */
  score: number;
}

/**
 * Weights used to build the global structurality score.
 *
 * The weights sum to 1.0.
 */
export interface StructuralityWeights {
  /** Rhythmic regularity and metrical consistency. */
  rhythm: number;
  /** Local phoneme-level patterning: echo density + cluster density. */
  localPhonemePatterning: number;
  /** Larger sound-sequence patterning: rhyme and structural rhyme. */
  soundSequencePatterning: number;
  /** Pause regularity and pause-load organisation. */
  pausePatterning: number;
  /** Coupling between line-level signals from multiple planes. */
  crossLevelCoupling: number;
}

/**
 * Cross-plane structural complexity report.
 *
 * The engine analyses several planes independently, converts each one into a
 * baseline-corrected coefficient in [0, 1], and then aggregates them into a
 * global structurality score.
 *
 * `crossLevelCoupling` is not computed with ANOVA.
 * Instead, the engine builds one line-level signal per plane and measures
 * pairwise agreement between those continuous signals using a hybrid of:
 * - level agreement: `1 - mean(abs(x_i - y_i))`
 * - shape agreement: positive Pearson correlation when variance exists
 *
 * This is better suited than ANOVA because the problem is not “do categorical
 * groups have different means?”, but “do multiple continuous structural signals
 * rise and fall together across the poem?”.
 */
export interface StructuralityAnalysis {
  /** Rhythmic structural complexity. */
  rhythm: StructuralityComponent;
  /** Local phoneme-level patterning structural complexity. */
  localPhonemePatterning: StructuralityComponent;
  /** Sound-sequence structural complexity. */
  soundSequencePatterning: StructuralityComponent;
  /** Pause structural complexity. */
  pausePatterning: StructuralityComponent;
  /** Coupling / interdependence of the structural planes. */
  crossLevelCoupling: StructuralityComponent;
  /**
   * Weighted global structurality score in [0, 1].
   * 0 = no structure above null floor, 1 = maximally loaded structure.
   */
  global: number;
  /** Explicit weighting scheme used for the global score. */
  weights: StructuralityWeights;
  /** Name of the interdependency model used by this build. */
  interdependencyModel: "pairwise_line_agreement_v1" | string;
}

/** Analyzer identity and build version metadata. */
export interface AnalyzerInfo {
  name: string;
  version: string;
}

/** Response schema descriptor for contract-aware consumers. */
export interface ResponseSchemaInfo {
  name: string;
  version: string;
  dialect: string;
  file: string;
}

/** Short explanation of a numeric metric exposed by the analysis payload. */
export interface MetricGlossaryEntry {
  /** Stable metric id, e.g. "rhyme_pairs.similarity". */
  id: string;
  /** JSON path-like source location where this metric appears. */
  source: string;
  /** What the metric measures. */
  description: string;
  /** How to read or use the metric in filtering/grouping UX. */
  interpretation: string;
}

// ── Top-level result ──────────────────────────────────────────────────────

/**
 * Full analysis result returned by `analyze()`.
 *
 * All arrays are in document order (matching stream element order).
 * The `annotations` map uses IpaStreamWord.id as key — always O(1) lookup.
 */
export interface StreamAnalysisResult {
  /** IPA Stream format version this result was produced from. Always "1.1". */
  version: "1.1";
  /** Analyzer name and version metadata. */
  analyzer: AnalyzerInfo;
  /** Response schema descriptor metadata. */
  responseSchema: ResponseSchemaInfo;
  /**
   * Per-word annotations.
   * Key: IpaStreamWord.id  →  Value: WordAnnotation
   */
  annotations: Record<string, WordAnnotation>;
  /**
   * All detected rhyme pairs with similarity scores (flexible grouping).
   * Use this for client-side threshold tuning and custom rhyme scheme visualization.
   * Pairs are ordered by descending similarity (strongest rhymes first).
   */
  rhyme_pairs: RhymePair[];
  /** Sound clusters detected across the full phoneme stream. */
  clusters: Cluster[];
  /** Per-line rhythm analysis (one entry per confirmed poetic line). */
  rhythm: LineRhythm[];
  /** Per-phoneme echo opacity (alliteration / assonance density). */
  echo: EchoAnnotation[];
  /** Prosodic pauses created by punctuation and/or line breaks. */
  pauses: PauseAnnotation[];
  /** Full per-phoneme payload: vectors, intrinsic traits, computed metrics. */
  phonemes: PhonemeLayer;
  /** Multi-plane structural complexity report. */
  structurality: StructuralityAnalysis;
  /** Short metric glossary for UI hints and quick interpretation. */
  metricGlossary: MetricGlossaryEntry[];
}

// ═══════════════════════════════════════════════════════════════════════════
// WASM module API
// ═══════════════════════════════════════════════════════════════════════════

/**
 * Analyse an IPA Stream v1.1 document.
 *
 * The phoneme registry is embedded in the WASM binary and initialised
 * automatically on first call — no separate `init()` step required.
 *
 * @param streamJson - JSON-serialised {@link IpaStream}.
 * @returns JSON-serialised {@link StreamAnalysisResult}.
 * @throws If the stream JSON is invalid or the format version is not `"1.1"`.
 *
 * @example
 * import init_wasm, { analyze } from "ipa-poetry-engine";
 * await init_wasm();
 * const result: StreamAnalysisResult = JSON.parse(analyze(JSON.stringify(stream)));
 *
 * // ABBA pattern detection:
 * const byLine = Object.values(result.annotations)
 *   .reduce((acc, ann) => {
 *     if (!acc[ann.lineIndex]) acc[ann.lineIndex] = [];
 *     acc[ann.lineIndex].push(ann);
 *     return acc;
 *   }, {} as Record<number, WordAnnotation[]>);
 *
 * // Sort each line by wordIndex, then read rhymeGroup of the last word:
 * const endRhymes = Object.entries(byLine).map(([lineIdx, words]) => {
 *   const last = words.sort((a, b) => a.wordIndex - b.wordIndex).at(-1)!;
 *   return { lineIdx: Number(lineIdx), rhymeGroup: last.rhymeGroup };
 * });
 * // endRhymes → [{ lineIdx: 0, rhymeGroup: "A" }, { lineIdx: 1, rhymeGroup: "B" }, ...]
 */
export function analyze(streamJson: string): string;

/**
 * Returns the FNV-1a (64-bit) hex digest of the embedded IPA phoneme database.
 *
 * Identical for every binary compiled from the same `phonemes.json`.
 * Use this to verify that multiple consumers share the exact same IPA library
 * and will produce mathematically compatible feature vectors.
 *
 * @returns 16-character lowercase hex string, e.g. `"a3f1c8e2b4d70591"`
 *
 * @example
 * // Assert shared library across worker instances:
 * const hash = phonemeDbHash();
 * console.assert(hash === expectedHash, `IPA library mismatch: ${hash}`);
 */
export function phonemeDbHash(): string;

/**
 * Returns the IPA Stream format version supported by this build.
 * @returns `"1.1"`
 */
export function version(): string;
