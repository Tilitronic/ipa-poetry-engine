"""
Parse all Panphon phonemes with their phonological features into JSON.

Feature values:
  "+"  => [+feature]   (positively specified)
  "-"  => [-feature]   (negatively specified)
  "0"  => [0feature]   (unspecified / not applicable)
"""

import json
import os
import pandas as pd
import panphon

# ------------------------------------------------------------------
# Locate panphon data files
# ------------------------------------------------------------------
DATA_DIR = os.path.join(os.path.dirname(panphon.__file__), "data")
IPA_ALL_CSV = os.path.join(DATA_DIR, "ipa_all.csv")
IPA_BASES_CSV = os.path.join(DATA_DIR, "ipa_bases.csv")

# ------------------------------------------------------------------
# Feature column descriptions (for documentation inside each record)
# ------------------------------------------------------------------
FEATURE_DESCRIPTIONS = {
    "syl":     "syllabic",
    "son":     "sonorant",
    "cons":    "consonantal",
    "cont":    "continuant",
    "delrel":  "delayed release",
    "lat":     "lateral",
    "nas":     "nasal",
    "strid":   "strident",
    "voi":     "voice",
    "sg":      "spread glottis",
    "cg":      "constricted glottis",
    "ant":     "anterior",
    "cor":     "coronal",
    "distr":   "distributed",
    "lab":     "labial",
    "hi":      "high",
    "lo":      "low",
    "back":    "back",
    "round":   "round",
    "velaric": "velaric",
    "tense":   "tense",
    "long":    "long",
    "hitone":  "high tone",
    "hireg":   "high register",
}


def value_label(v: str) -> str:
    """Convert raw cell value to a readable label."""
    return {"+": "positive", "-": "negative", "0": "unspecified"}.get(str(v), str(v))


def parse_ipa_csv(path: str) -> list[dict]:
    """Read one of the IPA CSVs and return a list of phoneme dicts."""
    df = pd.read_csv(path, encoding="utf-8", dtype=str)
    df.fillna("0", inplace=True)

    feature_cols = [c for c in df.columns if c != "ipa"]
    records = []

    for _, row in df.iterrows():
        ipa_symbol = row["ipa"]
        features = {}
        for feat in feature_cols:
            raw = str(row[feat]).strip()
            features[feat] = {
                "value": raw,
                "label": value_label(raw),
                "description": FEATURE_DESCRIPTIONS.get(feat, feat),
            }

        records.append(
            {
                "ipa": ipa_symbol,
                "features": features,
            }
        )

    return records


# ------------------------------------------------------------------
# Build combined dataset: bases + all derived forms
# ------------------------------------------------------------------
bases = parse_ipa_csv(IPA_BASES_CSV)
all_segments = parse_ipa_csv(IPA_ALL_CSV)

# Index base phonemes for quick lookup
base_ipa_set = {r["ipa"] for r in bases}

# Mark whether each segment is a base phoneme or a derived form
for seg in all_segments:
    seg["is_base"] = seg["ipa"] in base_ipa_set

output = {
    "metadata": {
        "source": "Panphon 0.22.2",
        "total_segments": len(all_segments),
        "total_base_phonemes": len(bases),
        "feature_schema": {
            feat: desc for feat, desc in FEATURE_DESCRIPTIONS.items()
        },
        "value_encoding": {
            "+": "positively specified (present)",
            "-": "negatively specified (absent)",
            "0": "unspecified / not applicable",
        },
    },
    "phonemes": all_segments,
}

# ------------------------------------------------------------------
# Write JSON
# ------------------------------------------------------------------
OUTPUT_PATH = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "IPA", "phonemes.json"
)
os.makedirs(os.path.dirname(OUTPUT_PATH), exist_ok=True)

with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
    json.dump(output, f, ensure_ascii=False, indent=2)

print(f"Done. {len(all_segments)} segments written to {OUTPUT_PATH}")
