/* eslint-disable */
// AUTO-GENERATED FILE. DO NOT EDIT MANUALLY.
// Source: engine/schemas/*.schema.json (generated via schemars from Rust).

/**
 * One element of the flat stream array.
 */
export type StreamElement =
  | {
      /**
       * Stable token ID used for round-trip annotations.
       */
      id: string;
      language: string;
      /**
       * 0-based index of the confirmed line this word belongs to.
       */
      lineIndex: number;
      /**
       * Original text as it appears in the poem.
       */
      original: string;
      stressSource: StressSource;
      /**
       * 0-based index of the stressed syllable; `-1` if no stress.
       */
      stressedSyllable: number;
      syllableCount: number;
      syllables: IpaStreamSyllable[];
      type: "word";
      /**
       * 0-based position of the word within its line.
       */
      wordIndex: number;
      [k: string]: any | undefined;
    }
  | {
      type: "whitespace";
      [k: string]: any | undefined;
    }
  | {
      text: string;
      type: "punctuation";
      [k: string]: any | undefined;
    }
  | {
      lineIndex: number;
      type: "line_break";
      [k: string]: any | undefined;
    };
/**
 * Source / reliability of the stress assignment.
 */
export type StressSource = "dict" | "ml" | "manual";

/**
 * Top-level IPA Stream document.
 */
export interface IpaStream {
  metadata: IpaStreamMetadata;
  stream: StreamElement[];
  [k: string]: any | undefined;
}
export interface IpaStreamMetadata {
  confirmedLineCount: number;
  generatedAt: string;
  languagesPresent: string[];
  totalWordCount: number;
  version: string;
  [k: string]: any | undefined;
}
/**
 * One syllable inside a word token.
 */
export interface IpaStreamSyllable {
  /**
   * Grapheme characters aligned to this syllable.
   */
  grapheme: string;
  /**
   * Full IPA string of the syllable (e.g. `"ʃuk"`).
   */
  ipa: string;
  /**
   * Whether the syllable ends on a vowel (open syllable).
   */
  isOpen: boolean;
  /**
   * Whether this is the stressed syllable.
   */
  stressed: boolean;
  /**
   * Discrete phoneme symbols in order (e.g. `["ʃ","u","k"]`).
   */
  tokens: string[];
  [k: string]: any | undefined;
}

export type MolstarContactKind = "similarity_weak" | "rhyme_strong" | "pause_pattern";
/**
 * How the line ends relative to its last stressed syllable.
 */
export type Clausula = "masculine" | "feminine" | "dactylic" | "hyperdactylic";
/**
 * How a syllable's actual stress relates to the detected metre.
 */
export type DeviationType = "match" | "pyrrhic" | "spondee";

/**
 * Result of `analyze_stream`, shaped for the frontend round-trip protocol.
 *
 * Serialises to: ```json { "version": "1.1", "annotations": { "tok-001": { "rhymeGroup": "A", ... } }, "clusters": [ ... ] } ```
 */
export interface StreamAnalysisResult {
  /**
   * Analyzer name and version.
   */
  analyzer: AnalyzerInfo;
  /**
   * Per-word annotations keyed by word `id`.
   */
  annotations: {
    [k: string]: WordAnnotation | undefined;
  };
  /**
   * Sound clusters detected across the full stream.
   */
  clusters: Cluster[];
  /**
   * Per-phoneme echo opacity annotations.
   */
  echo: EchoAnnotation[];
  /**
   * Short metric glossary for fast frontend interpretation and UX hints.
   */
  metricGlossary: MetricGlossaryEntry[];
  /**
   * IPA analysis transcribed as amino-acid-like chain for Mol* rendering.
   */
  molstar: MolstarTranscription;
  /**
   * Prosodic pauses created by punctuation and/or line breaks.
   */
  pauses: PauseAnnotation[];
  /**
   * Fully expanded per-phoneme payload sorted by flat index.
   */
  phonemes: PhonemeLayer;
  /**
   * Response schema descriptor for contract-aware consumers.
   */
  responseSchema: ResponseSchemaInfo;
  /**
   * Connected rhyme groups derived from `rhyme_pairs` (can be larger than pairs).
   */
  rhyme_groups: RhymeGroup[];
  /**
   * All detected rhyme pairs with similarity scores (flexible grouping).
   */
  rhyme_pairs: RhymePair[];
  /**
   * Per-line rhythm analysis (stress pattern, clausula, confidence).
   */
  rhythm: LineRhythm[];
  /**
   * Cross-plane structural complexity report normalised to [0, 1].
   */
  structurality: StructuralityAnalysis;
  /**
   * IPA Stream format version this result was produced from.
   */
  version: string;
  [k: string]: any | undefined;
}
export interface AnalyzerInfo {
  name: string;
  version: string;
  [k: string]: any | undefined;
}
/**
 * Per-word annotation keyed by `id` from the input stream.
 */
export interface WordAnnotation {
  /**
   * Line index of this word (0-based), mirrors `IpaStreamWord.line_index`.
   */
  lineIndex: number;
  /**
   * Rhyme group letter (A, B, C, …) or `null` if not detected.
   */
  rhymeGroup?: string | null;
  /**
   * DTW similarity score against the best-matching rhyme partner [0, 1].
   */
  rhymeScore?: number | null;
  /**
   * Confidence in the stress assignment (derived from `stressSource`).
   */
  stressConfidence: number;
  /**
   * Structural rhyme group (shared syllabic shape), or `null` if unique.
   */
  structuralRhymeGroup?: string | null;
  /**
   * Word index within its line (0-based), mirrors `IpaStreamWord.word_index`.
   */
  wordIndex: number;
  [k: string]: any | undefined;
}
export interface Cluster {
  end: number;
  kind: string;
  peak: number;
  start: number;
  [k: string]: any | undefined;
}
/**
 * Echo annotation for one phoneme.
 */
export interface EchoAnnotation {
  /**
   * Gap in phoneme-units to the nearest match (stream length when no match).
   */
  gap: number;
  /**
   * `flat_index` of the nearest similar phoneme, or `null` if none found.
   */
  nearestMatch?: number | null;
  /**
   * Visual opacity in `[alpha_min, 1.0]`.
   */
  opacity: number;
  /**
   * Identifies the phoneme in the original document.
   */
  source: PhonemeRef;
  [k: string]: any | undefined;
}
/**
 * Stable reference to one phoneme; the frontend uses this as a lookup key.
 */
export interface PhonemeRef {
  /**
   * Position in the flat phoneme-only stream (0-based).
   */
  flatIndex: number;
  /**
   * 0-based phoneme index within the syllable's `tokens` array.
   */
  phonemeIndex: number;
  /**
   * 0-based syllable index within the word.
   */
  syllableIndex: number;
  /**
   * Stable token ID of the containing word.
   */
  wordId: string;
  [k: string]: any | undefined;
}
export interface MetricGlossaryEntry {
  description: string;
  id: string;
  interpretation: string;
  source: string;
  [k: string]: any | undefined;
}
export interface MolstarTranscription {
  biophysicalModel: MolstarBiophysicalModel;
  chainId: string;
  contacts: MolstarContact[];
  fasta: string;
  formatVersion: string;
  interpretation: string[];
  ipaLines: string[];
  originalLines: string[];
  pdb: string;
  residueMap: MolstarResidueMapItem[];
  secondaryStructure: MolstarSecondaryElement[];
  sequence: string;
  wordSpans: MolstarWordSpan[];
  [k: string]: any | undefined;
}
export interface MolstarBiophysicalModel {
  backboneStep: number;
  contactEnergyUnit: string;
  distanceUnit: string;
  equations: string[];
  modelName: string;
  pausePatternMinRepeat: number;
  similarityContactCutoff: number;
  [k: string]: any | undefined;
}
export interface MolstarContact {
  decayLength: number;
  energy: number;
  equilibriumDistance: number;
  fromResidueIndex: number;
  kind: MolstarContactKind;
  note: string;
  springConstant: number;
  strength: number;
  toResidueIndex: number;
  [k: string]: any | undefined;
}
export interface MolstarResidueMapItem {
  aminoAcid: string;
  aminoAcidName: string;
  language: string;
  lineIndex: number;
  originalWord: string;
  residueIndex: number;
  source: PhonemeRef;
  syllableGrapheme: string;
  syllableIpa: string;
  symbol: string;
  wordIndex: number;
  [k: string]: any | undefined;
}
export interface MolstarSecondaryElement {
  endResidueIndex: number;
  kind: string;
  note: string;
  startResidueIndex: number;
  [k: string]: any | undefined;
}
export interface MolstarWordSpan {
  ipaWord: string;
  language: string;
  lineIndex: number;
  originalWord: string;
  residueEnd: number;
  residueStart: number;
  wordId: string;
  wordIndex: number;
  [k: string]: any | undefined;
}
/**
 * A prosodic pause detected in the stream.
 */
export interface PauseAnnotation {
  /**
   * The `id` of the word immediately before this pause.
   */
  afterWordId: string;
  /**
   * Whether a line break immediately follows the punctuation (or the word).
   */
  hasLineBreak: boolean;
  /**
   * The punctuation character(s) that precede the pause, if any.
   */
  punctuation?: string | null;
  /**
   * Normalised pause strength in `[0.0, 1.0]`.
   */
  strength: number;
  [k: string]: any | undefined;
}
export interface PhonemeLayer {
  entries: PhonemeRecord[];
  featureSchema: PhonemeFeatureSchemaItem[];
  sortedBy: string;
  total: number;
  valueEncoding: PhonemeFeatureEncoding;
  [k: string]: any | undefined;
}
export interface PhonemeRecord {
  computedMetrics: PhonemeComputedMetrics;
  features: PhonemeFeatureValue[];
  naturalProfile: PhonemeNaturalProfile;
  source: PhonemeRef;
  symbol: string;
  vector: number[];
  [k: string]: any | undefined;
}
export interface PhonemeComputedMetrics {
  clusterContributions: PhonemeClusterContribution[];
  clusterMembershipCount: number;
  clusterPeakMax: number;
  echoGap: number;
  echoOpacity: number;
  isStressedSyllable: boolean;
  lineIndex: number;
  nearestMatchFlatIndex?: number | null;
  rhymeGroup?: string | null;
  stressWeight: number;
  structuralRhymeGroup?: string | null;
  wordIndex: number;
  [k: string]: any | undefined;
}
export interface PhonemeClusterContribution {
  kind: string;
  peak: number;
  [k: string]: any | undefined;
}
export interface PhonemeFeatureValue {
  description: string;
  key: string;
  sign: string;
  value: number;
  [k: string]: any | undefined;
}
export interface PhonemeNaturalProfile {
  backness: number;
  classHint: string;
  consonantality: number;
  coronality: number;
  highness: number;
  labiality: number;
  laterality: number;
  lengthness: number;
  nasality: number;
  openness: number;
  roundedness: number;
  sonority: number;
  stridency: number;
  tenseness: number;
  toneHeight: number;
  voicing: number;
  vowelness: number;
  [k: string]: any | undefined;
}
export interface PhonemeFeatureSchemaItem {
  description: string;
  key: string;
  [k: string]: any | undefined;
}
export interface PhonemeFeatureEncoding {
  negative: string;
  positive: string;
  unspecified: string;
  [k: string]: any | undefined;
}
export interface ResponseSchemaInfo {
  dialect: string;
  file: string;
  name: string;
  version: string;
  [k: string]: any | undefined;
}
/**
 * Connected rhyme component that can contain 2+ words.
 */
export interface RhymeGroup {
  /**
   * Mean DTW similarity across group's pair edges.
   */
  averageSimilarity: number;
  /**
   * Mean weighted score across group's pair edges.
   */
  averageWeightedScore: number;
  /**
   * Rhyme group letter (A, B, C, ...).
   */
  groupId: string;
  /**
   * Maximum DTW similarity across group's pair edges.
   */
  maxSimilarity: number;
  /**
   * Maximum weighted score across group's pair edges.
   */
  maxWeightedScore: number;
  /**
   * Number of rhyme pair edges inside this connected group.
   */
  pairCount: number;
  /**
   * Word IDs that belong to this rhyme group.
   */
  wordIds: string[];
  [k: string]: any | undefined;
}
/**
 * Detected rhyme match between two words with position and strength metadata.
 */
export interface RhymePair {
  /**
   * Length of the best matching phoneme subsequence.
   */
  matchLength: number;
  /**
   * Start position (phoneme index) of match in word A.
   */
  positionA: number;
  /**
   * Start position (phoneme index) of match in word B.
   */
  positionB: number;
  /**
   * IPA symbol sequence of the matched region in word A.
   */
  sequenceA: string[];
  /**
   * IPA symbol sequence of the matched region in word B.
   */
  sequenceB: string[];
  /**
   * DTW phonetic similarity score [0, 1], higher = stronger rhyme.
   */
  similarity: number;
  /**
   * Strength tier for quick filtering: "strong" | "medium" | "weak".
   */
  strengthTier: string;
  /**
   * Weighted score: similarity × sqrt(match_length).
   */
  weightedScore: number;
  /**
   * ID of the first word in the pair.
   */
  wordIdA: string;
  /**
   * ID of the second word in the pair.
   */
  wordIdB: string;
  [k: string]: any | undefined;
}
/**
 * Full rhythm analysis result for one confirmed line.
 *
 * Serialises with camelCase field names for the round-trip protocol.
 */
export interface LineRhythm {
  /**
   * Clausula type (how the line ends).
   */
  clausula: Clausula;
  /**
   * Regularity score [0, 1]; 1.0 = every syllable matches the pattern.
   */
  confidence: number;
  /**
   * 0-based index of the confirmed line in the poem.
   */
  lineIndex: number;
  /**
   * Dominant metre period: `2` (binary) or `3` (ternary).
   */
  period: number;
  /**
   * Phase offset: the position within the period where stress is expected.
   */
  phase: number;
  /**
   * Total syllable count across all words in the line.
   */
  syllableCount: number;
  /**
   * Per-syllable annotations in line order (all syllables, including unstressed).
   */
  syllables: SyllableAnnotation[];
  [k: string]: any | undefined;
}
/**
 * Stress annotation for one syllable.
 */
export interface SyllableAnnotation {
  /**
   * Classification relative to the metre.
   */
  deviation: DeviationType;
  /**
   * Expected stress under the detected metre.
   */
  expected: number;
  /**
   * Actual stress value: `1.0` = stressed, `0.0` = unstressed.
   */
  stress: number;
  /**
   * Identifies the syllable for frontend mapping.
   */
  syllableRef: SyllableRef;
  [k: string]: any | undefined;
}
/**
 * Stable reference to one syllable; the frontend uses this as a lookup key.
 */
export interface SyllableRef {
  /**
   * 0-based position of this syllable within the line's flat syllable sequence.
   */
  linePosition: number;
  /**
   * 0-based index of this syllable within the word.
   */
  syllableIndex: number;
  /**
   * Stable token ID of the containing word (from IPA Stream `id` field).
   */
  wordId: string;
  [k: string]: any | undefined;
}
export interface StructuralityAnalysis {
  crossLevelCoupling: StructuralityComponent;
  global: number;
  interdependencyModel: string;
  localPhonemePatterning: StructuralityComponent;
  pausePatterning: StructuralityComponent;
  rhythm: StructuralityComponent;
  soundSequencePatterning: StructuralityComponent;
  weights: StructuralityWeights;
  [k: string]: any | undefined;
}
export interface StructuralityComponent {
  baseline: number;
  rawSignal: number;
  score: number;
  [k: string]: any | undefined;
}
export interface StructuralityWeights {
  crossLevelCoupling: number;
  localPhonemePatterning: number;
  pausePatterning: number;
  rhythm: number;
  soundSequencePatterning: number;
  [k: string]: any | undefined;
}
