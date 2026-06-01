# Mol\* Biophysical Transcription Model (IPA -> Pseudo-Protein)

## Purpose

The engine exposes a `molstar` payload inside `StreamAnalysisResult` so the frontend can render a structure in Mol\* and keep a strict, explicit mapping back to IPA tokens and source text.

Top-level location in response:

- `result.molstar`

## Why PDB/Mol\* works here

Mol\* supports loading structure files through URL parameters, including `pdb` and `mmcif` formats.

Relevant viewer docs:

- `structure-url`
- `structure-url-format` (`pdb`, `mmcif`, ...)
- `mvs-url` / `mvs-data` for optional MolViewSpec scenes

For this pipeline we emit a pseudo-protein `pdb` string directly, so a consumer can load it as a standard structure and then apply custom coloring/interaction overlays.

## Alignment Guarantees (Mol\* <-> IPA <-> Text)

The payload contains multiple alignment layers:

1. `molstar.residueMap[]`

- One item per pseudo-residue (one residue per phoneme).
- Contains:
  - `source.wordId`, `source.syllableIndex`, `source.phonemeIndex`, `source.flatIndex`
  - `symbol` (IPA token)
  - `originalWord`, `language`
  - `syllableIpa`, `syllableGrapheme`
  - `lineIndex`, `wordIndex`

2. `molstar.wordSpans[]`

- Residue interval per original word token.
- `residueStart..residueEnd` allows quick bidirectional highlighting.

3. `molstar.ipaLines[]` and `molstar.originalLines[]`

- Line-level textual projections for UI overlays.

Because `residueMap` includes both IPA and original text context, the frontend does not need to reconstruct mapping heuristically.

## Pseudo-Biophysical Model

Model id: `ipa_biophysical_proxy_v2`

The model is a proxy force interpretation that translates phonological structure into geometry and non-covalent-style contacts.

### 1) Backbone and secondary structure

- Each phoneme becomes one pseudo-residue (CA atom only).
- Backbone step is fixed (`backboneStep`, in Angstrom).
- Rhythm controls local geometry class:
  - stressed syllable -> `helix`
  - metrical deviation (`spondee`/`pyrrhic`) -> `sheet`
  - otherwise -> `coil`

This is encoded in:

- `molstar.secondaryStructure[]`

### 2) Contacts and energies

Contacts are in `molstar.contacts[]` with explicit parameters:

- `strength`
- `equilibriumDistance`
- `decayLength`
- `springConstant`
- `energy`
- `kind` in:
  - `similarity_weak`
  - `rhyme_strong`
  - `pause_pattern`

#### Similarity weak contacts

Short-range only (gap-limited), derived from echo opacity + distance.

#### Rhyme strong contacts

Long-range tertiary-like couplings between rhyme-group anchors (word-final residues).

#### Pause pattern contacts

Weak contacts enabled only when pause patterns repeat regularly (min repeats + spacing regularity gate).

### 3) Biophysical metadata

`molstar.biophysicalModel` provides:

- units
- thresholds
- minimal repeat criteria
- equation strings used by this version

This makes the model auditable and versioned for UI/analytics.

## Frontend Integration Pattern

1. Load structure in Mol\* from generated `pdb` text.
2. Build residue-index lookup from `residueMap`.
3. On text hover (word/syllable/phoneme), map to residue interval via `wordSpans` / `source` and highlight in Mol\*.
4. On Mol\* residue hover, map back through `residueMap` to IPA and original text.
5. Render contact overlays by `kind` and `strength`.

## Notes

- The model is intentionally a structural proxy, not a physical simulation.
- Parameters are chosen for stable visual behavior and interpretability.
- Future versions can swap equations while preserving alignment contracts.
