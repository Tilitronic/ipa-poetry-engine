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
  | "sibilant"   // strident feature — /s z ʃ ʒ/ family
  | "affricate"  // delayed-release feature — /tʃ dʒ ts dz/ family
  | "nasal"      // nasal feature — /m n ŋ/ family
  | "lateral"    // lateral feature — /l lʲ/ family
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
  | "match"    // actual stress agrees with the expected metrical position
  | "pyrrhic"  // expected stress was absent (skipped)
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
  | "masculine"      // last stress on the final syllable
  | "feminine"       // last stress on the penultimate syllable
  | "dactylic"       // last stress on the antepenultimate syllable
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
  /**
   * Per-word annotations.
   * Key: IpaStreamWord.id  →  Value: WordAnnotation
   */
  annotations: Record<string, WordAnnotation>;
  /** Sound clusters detected across the full phoneme stream. */
  clusters: Cluster[];
  /** Per-line rhythm analysis (one entry per confirmed poetic line). */
  rhythm: LineRhythm[];
  /** Per-phoneme echo opacity (alliteration / assonance density). */
  echo: EchoAnnotation[];
  /** Prosodic pauses created by punctuation and/or line breaks. */
  pauses: PauseAnnotation[];
}

// ═══════════════════════════════════════════════════════════════════════════
// WASM module API
// ═══════════════════════════════════════════════════════════════════════════

/**
 * Load the phoneme feature registry. Must be called once before `analyze()`.
 *
 * @param phonemesJson - Full content of `phonemes.json` (~1 MB, 6367 segments).
 * @throws If JSON is malformed or if called more than once.
 *
 * @example
 * const phonemesJson = await fetch("/assets/phonemes.json").then(r => r.text());
 * init(phonemesJson);
 */
export function init(phonemesJson: string): void;

/**
 * Analyse an IPA Stream v1.1 document.
 *
 * @param streamJson - JSON-serialised {@link IpaStream}.
 * @returns JSON-serialised {@link StreamAnalysisResult}.
 * @throws If `init()` has not been called, or if the stream JSON is invalid.
 *
 * @example
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
 * Returns the IPA Stream format version supported by this build.
 * @returns `"1.1"`
 */
export function version(): string;
