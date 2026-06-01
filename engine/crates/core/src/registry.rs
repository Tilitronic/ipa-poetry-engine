//! `registry.rs` — loads `phonemes.json` (Panphon format) into an in-memory
//! lookup: IPA symbol → 24-dimensional feature vector.
//!
//! Feature values in the JSON are "+", "-", or "0", which we map to
//! +1.0, -1.0, 0.0 respectively.

use std::collections::HashMap;
use std::path::Path;

use ndarray::Array1;
use serde::Deserialize;

// ────────────────────────────────────────────────────────────────────────────
// JSON schema (mirrors `phonemes.json` produced by parse_phonemes.py)
// ────────────────────────────────────────────────────────────────────────────

/// One feature entry inside `"features"` object.
#[derive(Deserialize, Debug)]
struct RawFeature {
    value: String, // "+", "-", or "0"
}

/// One phoneme entry inside `"phonemes"` array.
#[derive(Deserialize, Debug)]
struct RawPhoneme {
    ipa: String,
    features: HashMap<String, RawFeature>,
}

/// Top-level JSON object.
#[derive(Deserialize, Debug)]
struct RawDatabase {
    phonemes: Vec<RawPhoneme>,
}

// ────────────────────────────────────────────────────────────────────────────
// Canonical feature order (same as Panphon CSV column order)
// ────────────────────────────────────────────────────────────────────────────

pub const FEATURE_NAMES: [&str; 24] = [
    "syl", "son", "cons", "cont", "delrel", "lat", "nas", "strid",
    "voi", "sg", "cg", "ant", "cor", "distr", "lab", "hi", "lo",
    "back", "round", "velaric", "tense", "long", "hitone", "hireg",
];

pub const FEATURE_DESCRIPTIONS: [&str; 24] = [
    "syllabic",
    "sonorant",
    "consonantal",
    "continuant",
    "delayed release",
    "lateral",
    "nasal",
    "strident",
    "voice",
    "spread glottis",
    "constricted glottis",
    "anterior",
    "coronal",
    "distributed",
    "labial",
    "high",
    "low",
    "back",
    "round",
    "velaric",
    "tense",
    "long",
    "high tone",
    "high register",
];

/// Convert a single feature value string to `f32`.
fn parse_value(v: &str) -> f32 {
    match v {
        "+" => 1.0,
        "-" => -1.0,
        _   => 0.0, // "0" or anything unexpected
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────────────────────

/// Loaded once at startup; maps every IPA segment to its feature vector.
pub struct FeatureRegistry {
    pub vectors: HashMap<String, Array1<f32>>,
}

impl FeatureRegistry {
    /// Build a registry from raw JSON bytes.
    pub fn from_json_bytes(json: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let db: RawDatabase = serde_json::from_slice(json)?;
        let mut vectors = HashMap::with_capacity(db.phonemes.len());

        for phoneme in db.phonemes {
            let mut vec = Array1::<f32>::zeros(FEATURE_NAMES.len());
            for (i, name) in FEATURE_NAMES.iter().enumerate() {
                if let Some(feat) = phoneme.features.get(*name) {
                    vec[i] = parse_value(&feat.value);
                }
            }
            vectors.insert(phoneme.ipa, vec);
        }

        Ok(Self { vectors })
    }

    /// Convenience: load directly from a file path.
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let bytes = std::fs::read(path)?;
        Self::from_json_bytes(&bytes)
    }

    /// Look up a feature vector for an IPA segment.
    #[inline]
    pub fn get(&self, symbol: &str) -> Option<&Array1<f32>> {
        self.vectors.get(symbol)
    }

    /// Total number of known segments.
    #[inline]
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal inline JSON that mimics our phonemes.json format.
    fn minimal_json() -> &'static [u8] {
        br#"{
            "metadata": {},
            "phonemes": [
                {
                    "ipa": "p",
                    "is_base": true,
                    "features": {
                        "syl":     { "value": "-", "label": "negative", "description": "syllabic" },
                        "son":     { "value": "-", "label": "negative", "description": "sonorant" },
                        "cons":    { "value": "+", "label": "positive", "description": "consonantal" },
                        "cont":    { "value": "-", "label": "negative", "description": "continuant" },
                        "delrel":  { "value": "-", "label": "negative", "description": "delayed release" },
                        "lat":     { "value": "-", "label": "negative", "description": "lateral" },
                        "nas":     { "value": "-", "label": "negative", "description": "nasal" },
                        "strid":   { "value": "-", "label": "negative", "description": "strident" },
                        "voi":     { "value": "-", "label": "negative", "description": "voice" },
                        "sg":      { "value": "-", "label": "negative", "description": "spread glottis" },
                        "cg":      { "value": "-", "label": "negative", "description": "constricted glottis" },
                        "ant":     { "value": "+", "label": "positive", "description": "anterior" },
                        "cor":     { "value": "-", "label": "negative", "description": "coronal" },
                        "distr":   { "value": "-", "label": "negative", "description": "distributed" },
                        "lab":     { "value": "+", "label": "positive", "description": "labial" },
                        "hi":      { "value": "-", "label": "negative", "description": "high" },
                        "lo":      { "value": "-", "label": "negative", "description": "low" },
                        "back":    { "value": "-", "label": "negative", "description": "back" },
                        "round":   { "value": "-", "label": "negative", "description": "round" },
                        "velaric": { "value": "-", "label": "negative", "description": "velaric" },
                        "tense":   { "value": "0", "label": "unspecified", "description": "tense" },
                        "long":    { "value": "-", "label": "negative", "description": "long" },
                        "hitone":  { "value": "0", "label": "unspecified", "description": "high tone" },
                        "hireg":   { "value": "0", "label": "unspecified", "description": "high register" }
                    }
                },
                {
                    "ipa": "b",
                    "is_base": true,
                    "features": {
                        "syl":     { "value": "-", "label": "negative", "description": "syllabic" },
                        "son":     { "value": "-", "label": "negative", "description": "sonorant" },
                        "cons":    { "value": "+", "label": "positive", "description": "consonantal" },
                        "cont":    { "value": "-", "label": "negative", "description": "continuant" },
                        "delrel":  { "value": "-", "label": "negative", "description": "delayed release" },
                        "lat":     { "value": "-", "label": "negative", "description": "lateral" },
                        "nas":     { "value": "-", "label": "negative", "description": "nasal" },
                        "strid":   { "value": "-", "label": "negative", "description": "strident" },
                        "voi":     { "value": "+", "label": "positive", "description": "voice" },
                        "sg":      { "value": "-", "label": "negative", "description": "spread glottis" },
                        "cg":      { "value": "-", "label": "negative", "description": "constricted glottis" },
                        "ant":     { "value": "+", "label": "positive", "description": "anterior" },
                        "cor":     { "value": "-", "label": "negative", "description": "coronal" },
                        "distr":   { "value": "-", "label": "negative", "description": "distributed" },
                        "lab":     { "value": "+", "label": "positive", "description": "labial" },
                        "hi":      { "value": "-", "label": "negative", "description": "high" },
                        "lo":      { "value": "-", "label": "negative", "description": "low" },
                        "back":    { "value": "-", "label": "negative", "description": "back" },
                        "round":   { "value": "-", "label": "negative", "description": "round" },
                        "velaric": { "value": "-", "label": "negative", "description": "velaric" },
                        "tense":   { "value": "0", "label": "unspecified", "description": "tense" },
                        "long":    { "value": "-", "label": "negative", "description": "long" },
                        "hitone":  { "value": "0", "label": "unspecified", "description": "high tone" },
                        "hireg":   { "value": "0", "label": "unspecified", "description": "high register" }
                    }
                }
            ]
        }"#
    }

    #[test]
    fn test_registry_loads_correct_count() {
        let reg = FeatureRegistry::from_json_bytes(minimal_json()).unwrap();
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn test_known_symbol_returns_vector() {
        let reg = FeatureRegistry::from_json_bytes(minimal_json()).unwrap();
        assert!(reg.get("p").is_some());
        assert!(reg.get("b").is_some());
    }

    #[test]
    fn test_unknown_symbol_returns_none() {
        let reg = FeatureRegistry::from_json_bytes(minimal_json()).unwrap();
        assert!(reg.get("z").is_none());
    }

    #[test]
    fn test_vector_length_is_24() {
        let reg = FeatureRegistry::from_json_bytes(minimal_json()).unwrap();
        let v = reg.get("p").unwrap();
        assert_eq!(v.len(), 24);
    }

    #[test]
    fn test_feature_value_encoding() {
        let reg = FeatureRegistry::from_json_bytes(minimal_json()).unwrap();
        let p = reg.get("p").unwrap();
        let b = reg.get("b").unwrap();

        // syl index=0 → "-" → -1.0 for both
        assert_eq!(p[0], -1.0);
        // cons index=2 → "+" → 1.0
        assert_eq!(p[2], 1.0);
        // voi index=8 → "+" → 1.0 for b, "-" → -1.0 for p
        assert_eq!(b[8], 1.0);
        assert_eq!(p[8], -1.0);
        // tense index=20 → "0" → 0.0
        assert_eq!(p[20], 0.0);
    }

    #[test]
    fn test_p_and_b_differ_only_in_voice() {
        let reg = FeatureRegistry::from_json_bytes(minimal_json()).unwrap();
        let p = reg.get("p").unwrap();
        let b = reg.get("b").unwrap();
        // They should be identical except at index 8 (voi)
        for i in 0..24 {
            if i == 8 {
                assert_ne!(p[i], b[i], "voi feature should differ");
            } else {
                assert_eq!(p[i], b[i], "feature {i} should match");
            }
        }
    }

    #[test]
    fn test_invalid_json_returns_error() {
        let result = FeatureRegistry::from_json_bytes(b"not json");
        assert!(result.is_err());
    }
}
