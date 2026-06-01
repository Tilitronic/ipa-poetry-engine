use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Serialize;

use crate::algorithms::echo::{EchoAnnotation, DEFAULT_ALPHA_MIN};
use crate::algorithms::pause::PauseAnnotation;
use crate::algorithms::rhythm::LineRhythm;
use crate::stream::IpaStreamWord;
use crate::{Cluster, WordAnnotation};

const RHYTHM_WEIGHT: f32 = 0.25;
const LOCAL_PHONEME_WEIGHT: f32 = 0.125;
const SOUND_SEQUENCE_WEIGHT: f32 = 0.375;
const PAUSE_WEIGHT: f32 = 0.125;
const COUPLING_WEIGHT: f32 = 0.125;

const RHYTHM_BASELINE: f32 = 0.50;
const LOCAL_PHONEME_BASELINE: f32 = 0.18;
const SOUND_SEQUENCE_BASELINE: f32 = 0.10;
const PAUSE_BASELINE: f32 = 0.20;
const COUPLING_BASELINE: f32 = 0.25;

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructuralityComponent {
    pub raw_signal: f32,
    pub baseline: f32,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructuralityWeights {
    pub rhythm: f32,
    pub local_phoneme_patterning: f32,
    pub sound_sequence_patterning: f32,
    pub pause_patterning: f32,
    pub cross_level_coupling: f32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructuralityAnalysis {
    pub rhythm: StructuralityComponent,
    pub local_phoneme_patterning: StructuralityComponent,
    pub sound_sequence_patterning: StructuralityComponent,
    pub pause_patterning: StructuralityComponent,
    pub cross_level_coupling: StructuralityComponent,
    pub global: f32,
    pub weights: StructuralityWeights,
    pub interdependency_model: &'static str,
}

pub fn compute_structurality(
    words: &[&IpaStreamWord],
    annotations: &HashMap<String, WordAnnotation>,
    rhythm: &[LineRhythm],
    echo: &[EchoAnnotation],
    pauses: &[PauseAnnotation],
    clusters: &[Cluster],
    flat_token_lines: &[usize],
) -> StructuralityAnalysis {
    let line_count = words.iter().map(|w| w.line_index).max().map(|v| v + 1)
        .or_else(|| rhythm.iter().map(|r| r.line_index).max().map(|v| v + 1))
        .unwrap_or(0);

    let word_line: HashMap<&str, usize> = words.iter()
        .map(|w| (w.id.as_str(), w.line_index))
        .collect();

    let rhythm_line_signal = build_rhythm_signal(rhythm, line_count);
    let local_line_signal = build_local_phoneme_signal(echo, clusters, &word_line, flat_token_lines, line_count);
    let sequence_line_signal = build_sequence_signal(words, annotations, line_count);
    let pause_line_signal = build_pause_signal(pauses, &word_line, line_count);

    let rhythm_raw = weighted_rhythm_mean(rhythm);
    let local_raw = mean(&local_line_signal);
    let sequence_raw = mean(&sequence_line_signal);
    let pause_raw = pause_pattern_score(&pause_line_signal);
    let coupling_raw = cross_level_coupling(&[
        rhythm_line_signal.as_slice(),
        local_line_signal.as_slice(),
        sequence_line_signal.as_slice(),
        pause_line_signal.as_slice(),
    ]);

    let rhythm = component(rhythm_raw, RHYTHM_BASELINE);
    let local_phoneme_patterning = component(local_raw, LOCAL_PHONEME_BASELINE);
    let sound_sequence_patterning = component(sequence_raw, SOUND_SEQUENCE_BASELINE);
    let pause_patterning = component(pause_raw, PAUSE_BASELINE);
    let cross_level_coupling = component(coupling_raw, COUPLING_BASELINE);

    let weights = StructuralityWeights {
        rhythm: RHYTHM_WEIGHT,
        local_phoneme_patterning: LOCAL_PHONEME_WEIGHT,
        sound_sequence_patterning: SOUND_SEQUENCE_WEIGHT,
        pause_patterning: PAUSE_WEIGHT,
        cross_level_coupling: COUPLING_WEIGHT,
    };

    let global = clamp01(
        rhythm.score * RHYTHM_WEIGHT
            + local_phoneme_patterning.score * LOCAL_PHONEME_WEIGHT
            + sound_sequence_patterning.score * SOUND_SEQUENCE_WEIGHT
            + pause_patterning.score * PAUSE_WEIGHT
            + cross_level_coupling.score * COUPLING_WEIGHT,
    );

    StructuralityAnalysis {
        rhythm,
        local_phoneme_patterning,
        sound_sequence_patterning,
        pause_patterning,
        cross_level_coupling,
        global,
        weights,
        interdependency_model: "pairwise_line_agreement_v1",
    }
}

fn build_rhythm_signal(rhythm: &[LineRhythm], line_count: usize) -> Vec<f32> {
    let mut signal = vec![0.0; line_count];
    for line in rhythm {
        if line.line_index < line_count {
            signal[line.line_index] = clamp01(line.confidence);
        }
    }
    signal
}

fn build_local_phoneme_signal(
    echo: &[EchoAnnotation],
    clusters: &[Cluster],
    word_line: &HashMap<&str, usize>,
    flat_token_lines: &[usize],
    line_count: usize,
) -> Vec<f32> {
    let mut echo_sum = vec![0.0; line_count];
    let mut echo_count = vec![0usize; line_count];

    for ann in echo {
        if let Some(&line_idx) = word_line.get(ann.source.word_id.as_str()) {
            let opacity = ((ann.opacity - DEFAULT_ALPHA_MIN) / (1.0 - DEFAULT_ALPHA_MIN)).clamp(0.0, 1.0);
            echo_sum[line_idx] += opacity;
            echo_count[line_idx] += 1;
        }
    }

    let mut cluster_sum = vec![0.0; line_count];
    let mut phoneme_count = vec![0usize; line_count];

    for &line_idx in flat_token_lines {
        if line_idx < line_count {
            phoneme_count[line_idx] += 1;
        }
    }

    for cluster in clusters {
        let peak_norm = (cluster.peak / 4.0).clamp(0.0, 1.0);
        let end = cluster.end.min(flat_token_lines.len());
        for idx in cluster.start..end {
            if let Some(&line_idx) = flat_token_lines.get(idx) {
                if line_idx < line_count {
                    cluster_sum[line_idx] += peak_norm;
                }
            }
        }
    }

    (0..line_count)
        .map(|line_idx| {
            let echo_mean = if echo_count[line_idx] == 0 {
                0.0
            } else {
                echo_sum[line_idx] / echo_count[line_idx] as f32
            };
            let cluster_density = if phoneme_count[line_idx] == 0 {
                0.0
            } else {
                (cluster_sum[line_idx] / phoneme_count[line_idx] as f32).clamp(0.0, 1.0)
            };
            clamp01(0.5 * echo_mean + 0.5 * cluster_density)
        })
        .collect()
}

fn build_sequence_signal(
    words: &[&IpaStreamWord],
    annotations: &HashMap<String, WordAnnotation>,
    line_count: usize,
) -> Vec<f32> {
    let mut rhyme_sum = vec![0.0; line_count];
    let mut rhyme_hits = vec![0usize; line_count];
    let mut structural_hits = vec![0usize; line_count];
    let mut word_count = vec![0usize; line_count];

    for word in words {
        let line_idx = word.line_index;
        if line_idx >= line_count {
            continue;
        }
        word_count[line_idx] += 1;
        if let Some(ann) = annotations.get(&word.id) {
            if let Some(score) = ann.rhyme_score {
                rhyme_sum[line_idx] += score;
                rhyme_hits[line_idx] += 1;
            }
            if ann.structural_rhyme_group.is_some() {
                structural_hits[line_idx] += 1;
            }
        }
    }

    (0..line_count)
        .map(|line_idx| {
            if word_count[line_idx] == 0 {
                return 0.0;
            }
            let rhyme_mean = if rhyme_hits[line_idx] == 0 {
                0.0
            } else {
                rhyme_sum[line_idx] / rhyme_hits[line_idx] as f32
            };
            let rhyme_share = rhyme_hits[line_idx] as f32 / word_count[line_idx] as f32;
            let structural_share = structural_hits[line_idx] as f32 / word_count[line_idx] as f32;
            clamp01(0.7 * rhyme_mean + 0.15 * rhyme_share + 0.15 * structural_share)
        })
        .collect()
}

fn build_pause_signal(
    pauses: &[PauseAnnotation],
    word_line: &HashMap<&str, usize>,
    line_count: usize,
) -> Vec<f32> {
    let mut pause_sum = vec![0.0; line_count];
    let mut pause_count = vec![0usize; line_count];

    for pause in pauses {
        if let Some(&line_idx) = word_line.get(pause.after_word_id.as_str()) {
            if line_idx < line_count {
                pause_sum[line_idx] += clamp01(pause.strength);
                pause_count[line_idx] += 1;
            }
        }
    }

    (0..line_count)
        .map(|line_idx| {
            if pause_count[line_idx] == 0 {
                0.0
            } else {
                pause_sum[line_idx] / pause_count[line_idx] as f32
            }
        })
        .collect()
}

fn weighted_rhythm_mean(rhythm: &[LineRhythm]) -> f32 {
    let total_weight: usize = rhythm.iter().map(|line| line.syllable_count.max(1)).sum();
    if total_weight == 0 {
        return 0.0;
    }
    let weighted: f32 = rhythm.iter()
        .map(|line| clamp01(line.confidence) * line.syllable_count.max(1) as f32)
        .sum();
    weighted / total_weight as f32
}

fn pause_pattern_score(signal: &[f32]) -> f32 {
    let mean_strength = mean(signal);
    if signal.len() <= 1 {
        return mean_strength;
    }
    let regularity = 1.0 - signal.windows(2)
        .map(|pair| (pair[0] - pair[1]).abs())
        .sum::<f32>() / (signal.len() - 1) as f32;
    clamp01(0.5 * mean_strength + 0.5 * regularity)
}

fn cross_level_coupling(series: &[&[f32]]) -> f32 {
    let mut agreements = Vec::new();
    for i in 0..series.len() {
        for j in (i + 1)..series.len() {
            agreements.push(series_agreement(series[i], series[j]));
        }
    }
    mean(&agreements)
}

fn series_agreement(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }
    let a = &a[..len];
    let b = &b[..len];
    let level = 1.0 - a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum::<f32>() / len as f32;
    let corr = pearson(a, b).unwrap_or(level).max(0.0);
    clamp01(0.5 * level + 0.5 * corr)
}

fn pearson(a: &[f32], b: &[f32]) -> Option<f32> {
    let len = a.len().min(b.len());
    if len < 2 {
        return None;
    }
    let a = &a[..len];
    let b = &b[..len];
    let mean_a = mean(a);
    let mean_b = mean(b);

    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let dx = x - mean_a;
        let dy = y - mean_b;
        cov += dx * dy;
        var_a += dx * dx;
        var_b += dy * dy;
    }
    if var_a <= 1e-6 || var_b <= 1e-6 {
        return None;
    }
    Some((cov / (var_a.sqrt() * var_b.sqrt())).clamp(-1.0, 1.0))
}

fn component(raw_signal: f32, baseline: f32) -> StructuralityComponent {
    StructuralityComponent {
        raw_signal: clamp01(raw_signal),
        baseline,
        score: clamp01((raw_signal - baseline) / (1.0 - baseline)),
    }
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}

fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_series_agreement_is_high_for_matching_shapes() {
        let a = [0.2, 0.8, 0.2, 0.8];
        let b = [0.1, 0.9, 0.1, 0.9];
        assert!(series_agreement(&a, &b) > 0.8);
    }

    #[test]
    fn test_component_normalises_above_baseline() {
        let c = component(0.75, 0.5);
        assert!(c.score > 0.4 && c.score < 0.6);
    }
}