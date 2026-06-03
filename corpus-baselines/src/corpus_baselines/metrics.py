"""Corpus metrics and threshold search for poetry vs prose separation."""

from __future__ import annotations

from dataclasses import dataclass
import bz2
import json
import re
from pathlib import Path
from statistics import mean
from typing import Iterable


WORD_RE = re.compile(r"[\u0400-\u04FF'’\-]+", flags=re.UNICODE)
PUNCT_RE = re.compile(r"[.,;:!?…—\-]", flags=re.UNICODE)


@dataclass(frozen=True)
class SampleMetrics:
    label: str
    path: str
    avg_words_per_line: float
    short_line_ratio: float
    punctuation_density: float
    lexical_diversity: float
    stress_known_ratio: float
    stress_position_mean: float
    line_end_echo_ratio: float

    def as_dict(self) -> dict[str, float | str]:
        return {
            "label": self.label,
            "path": self.path,
            "avg_words_per_line": self.avg_words_per_line,
            "short_line_ratio": self.short_line_ratio,
            "punctuation_density": self.punctuation_density,
            "lexical_diversity": self.lexical_diversity,
            "stress_known_ratio": self.stress_known_ratio,
            "stress_position_mean": self.stress_position_mean,
            "line_end_echo_ratio": self.line_end_echo_ratio,
        }


@dataclass(frozen=True)
class ThresholdResult:
    metric: str
    threshold: float
    direction: str
    balanced_accuracy: float
    precision: float
    recall: float

    def as_dict(self) -> dict[str, float | str]:
        return {
            "metric": self.metric,
            "threshold": self.threshold,
            "direction": self.direction,
            "balanced_accuracy": self.balanced_accuracy,
            "precision": self.precision,
            "recall": self.recall,
        }


class StressBackend:
    """Dictionary-first stress resolver with Luscinia fallback."""

    def __init__(self) -> None:
        try:
            import ukrainian_stress as ua_stress  # type: ignore
        except Exception:
            ua_stress = None

        try:
            import luscinia  # type: ignore
        except Exception:
            luscinia = None

        self.ua_stress = ua_stress
        self.luscinia = luscinia
        self.luscinia_predictor = None
        self._luscinia_disabled = False

        if self.luscinia is not None:
            try:
                try:
                    import onnxruntime as ort  # type: ignore

                    so = ort.SessionOptions()
                    so.graph_optimization_level = ort.GraphOptimizationLevel.ORT_DISABLE_ALL
                    so.enable_mem_pattern = False
                    so.enable_cpu_mem_arena = False
                    so.intra_op_num_threads = 1
                    so.inter_op_num_threads = 1
                    self.luscinia_predictor = self.luscinia.LusciniaPredictor(session_options=so)
                except Exception:
                    # Fallback to package defaults if onnxruntime tuning fails.
                    self.luscinia_predictor = self.luscinia.LusciniaPredictor()
            except Exception:
                self._luscinia_disabled = True

    @property
    def available(self) -> bool:
        return self.ua_stress is not None

    def resolve(self, word: str) -> tuple[str | None, int | None, int | None, str]:
        """Return (ipa, stress_index, syllable_count, source)."""
        normalized = word.lower()
        if self.ua_stress is None:
            return None, None, None, "none"

        try:
            lookup = self.ua_stress.lookup(normalized)
            readings = lookup.get("readings") if isinstance(lookup, dict) else None
            if readings:
                reading = readings[0]
                return (
                    reading.get("ipa"),
                    reading.get("syllable_index"),
                    reading.get("syllable_count"),
                    "dict",
                )
        except Exception:
            pass

        if self.luscinia is not None:
            if self._luscinia_disabled:
                return None, None, None, "none"
            try:
                stress_index = int(
                    self.luscinia.predict_stress(
                        normalized,
                        predictor=self.luscinia_predictor,
                    )
                )
                return None, stress_index, None, "ml"
            except Exception:
                self._luscinia_disabled = True
                pass

        return None, None, None, "none"

    def resolve_many(self, words: list[str]) -> dict[str, tuple[str | None, int | None, int | None, str]]:
        """Resolve many words in a batch-like pass.

        The current Python API exposes single-word `lookup`, so we batch at the
        workflow level and resolve words concurrently.
        """
        if not words:
            return {}

        unique_words = sorted(set(words))
        # The native binding is not guaranteed to be thread-safe. Resolve in one
        # pass to keep batch behavior deterministic and stable.
        return {word: self.resolve(word) for word in unique_words}


def discover_corpus_files(data_root: Path) -> tuple[list[Path], list[Path]]:
    poetry_files: list[Path] = []
    prose_files: list[Path] = []
    binary_suffixes = {
        ".pdf", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".zip", ".7z", ".rar",
        ".wasm", ".exe", ".dll", ".so", ".dylib", ".bin",
    }
    for file_path in data_root.rglob("*"):
        if not file_path.is_file():
            continue
        parts = [p.lower() for p in file_path.parts]
        if "poetry" not in parts and "prose" not in parts:
            continue
        suffix = file_path.suffix.lower()
        if suffix in binary_suffixes:
            continue
        if "poetry" in parts:
            poetry_files.append(file_path)
        elif "prose" in parts:
            prose_files.append(file_path)
    return sorted(poetry_files), sorted(prose_files)


def iter_text_lines(path: Path) -> Iterable[str]:
    if path.suffix.lower() == ".bz2":
        with bz2.open(path, mode="rt", encoding="utf-8", errors="ignore") as handle:
            for line in handle:
                yield line.rstrip("\n")
        return

    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line in handle:
            yield line.rstrip("\n")


def _safe_mean(values: list[float]) -> float:
    return mean(values) if values else 0.0


def _line_end_key(ipa: str | None, fallback_word: str) -> str:
    source = ipa if ipa else fallback_word
    letters = [c for c in source.lower() if c.isalpha()]
    if not letters:
        return ""
    return "".join(letters[-3:])


def analyze_file(path: Path, label: str, backend: StressBackend, max_lines: int, max_words: int) -> SampleMetrics:
    non_empty_lines = 0
    words_per_line: list[int] = []
    line_words: list[list[str]] = []
    all_words: list[str] = []
    punctuation_count = 0
    char_count = 0
    stress_known = 0
    stress_positions: list[float] = []
    line_end_keys: list[str] = []

    for idx, line in enumerate(iter_text_lines(path)):
        if max_lines > 0 and idx >= max_lines:
            break

        char_count += len(line)
        punctuation_count += len(PUNCT_RE.findall(line))
        words = [w.lower() for w in WORD_RE.findall(line)]
        if not words:
            continue

        if max_words > 0:
            remaining = max_words - len(all_words)
            if remaining <= 0:
                break
            if len(words) > remaining:
                words = words[:remaining]

        non_empty_lines += 1
        words_per_line.append(len(words))
        line_words.append(words)
        all_words.extend(words)
        if max_words > 0 and len(all_words) >= max_words:
            break

    word_cache = backend.resolve_many(all_words)

    for words in line_words:
        for word in words:
            ipa, stress_idx, syllable_count, _ = word_cache.get(word, (None, None, None, "none"))
            if stress_idx is not None:
                stress_known += 1
                if syllable_count and syllable_count > 1:
                    stress_positions.append(stress_idx / (syllable_count - 1))

        end_word = words[-1]
        end_ipa, _, _, _ = word_cache.get(end_word, (None, None, None, "none"))
        line_end_keys.append(_line_end_key(end_ipa, end_word))

    total_words = len(all_words)
    unique_words = len(set(all_words))
    short_lines = sum(1 for n in words_per_line if n <= 5)

    echoes = 0
    echo_comparisons = max(0, len(line_end_keys) - 1)
    for i in range(echo_comparisons):
        if line_end_keys[i] and line_end_keys[i] == line_end_keys[i + 1]:
            echoes += 1

    return SampleMetrics(
        label=label,
        path=str(path),
        avg_words_per_line=_safe_mean([float(n) for n in words_per_line]),
        short_line_ratio=(short_lines / non_empty_lines) if non_empty_lines else 0.0,
        punctuation_density=(punctuation_count / char_count) if char_count else 0.0,
        lexical_diversity=(unique_words / total_words) if total_words else 0.0,
        stress_known_ratio=(stress_known / total_words) if total_words else 0.0,
        stress_position_mean=_safe_mean(stress_positions),
        line_end_echo_ratio=(echoes / echo_comparisons) if echo_comparisons else 0.0,
    )


def _predict(metric_value: float, threshold: float, direction: str) -> int:
    if direction == ">=":
        return 1 if metric_value >= threshold else 0
    return 1 if metric_value <= threshold else 0


def _classification_stats(y_true: list[int], y_pred: list[int]) -> tuple[float, float, float]:
    tp = sum(1 for t, p in zip(y_true, y_pred) if t == 1 and p == 1)
    tn = sum(1 for t, p in zip(y_true, y_pred) if t == 0 and p == 0)
    fp = sum(1 for t, p in zip(y_true, y_pred) if t == 0 and p == 1)
    fn = sum(1 for t, p in zip(y_true, y_pred) if t == 1 and p == 0)

    tpr = tp / (tp + fn) if (tp + fn) else 0.0
    tnr = tn / (tn + fp) if (tn + fp) else 0.0
    precision = tp / (tp + fp) if (tp + fp) else 0.0
    recall = tpr
    balanced_accuracy = (tpr + tnr) / 2.0
    return balanced_accuracy, precision, recall


def find_best_threshold(poetry: list[float], prose: list[float], metric: str) -> ThresholdResult:
    values = sorted(set(poetry + prose))
    if not values:
        return ThresholdResult(metric=metric, threshold=0.0, direction=">=", balanced_accuracy=0.0, precision=0.0, recall=0.0)

    y_true = [1] * len(poetry) + [0] * len(prose)
    all_values = poetry + prose

    best = ThresholdResult(metric=metric, threshold=values[0], direction=">=", balanced_accuracy=-1.0, precision=0.0, recall=0.0)
    for threshold in values:
        for direction in (">=", "<="):
            y_pred = [_predict(v, threshold, direction) for v in all_values]
            ba, prec, rec = _classification_stats(y_true, y_pred)
            if ba > best.balanced_accuracy:
                best = ThresholdResult(
                    metric=metric,
                    threshold=threshold,
                    direction=direction,
                    balanced_accuracy=ba,
                    precision=prec,
                    recall=rec,
                )
    return best


def summarize_metrics(samples: list[SampleMetrics]) -> dict[str, float]:
    if not samples:
        return {}

    fields = [
        "avg_words_per_line",
        "short_line_ratio",
        "punctuation_density",
        "lexical_diversity",
        "stress_known_ratio",
        "stress_position_mean",
        "line_end_echo_ratio",
    ]
    summary: dict[str, float] = {}
    for field in fields:
        summary[field] = _safe_mean([float(getattr(s, field)) for s in samples])
    return summary


def run_analysis(data_root: Path, max_files_per_group: int, max_lines_per_file: int, max_words_per_file: int) -> dict[str, object]:
    backend = StressBackend()
    poetry_files, prose_files = discover_corpus_files(data_root)

    if max_files_per_group > 0:
        poetry_files = poetry_files[:max_files_per_group]
        prose_files = prose_files[:max_files_per_group]

    poetry_samples = [analyze_file(path, "poetry", backend, max_lines_per_file, max_words_per_file) for path in poetry_files]
    prose_samples = [analyze_file(path, "prose", backend, max_lines_per_file, max_words_per_file) for path in prose_files]
    all_samples = poetry_samples + prose_samples

    metric_names = [
        "avg_words_per_line",
        "short_line_ratio",
        "punctuation_density",
        "lexical_diversity",
        "stress_known_ratio",
        "stress_position_mean",
        "line_end_echo_ratio",
    ]

    thresholds: list[ThresholdResult] = []
    for metric in metric_names:
        poetry_values = [float(getattr(s, metric)) for s in poetry_samples]
        prose_values = [float(getattr(s, metric)) for s in prose_samples]
        thresholds.append(find_best_threshold(poetry_values, prose_values, metric))

    thresholds = sorted(thresholds, key=lambda x: x.balanced_accuracy, reverse=True)

    return {
        "backend": {
            "ua_stress_engine": backend.ua_stress is not None,
            "luscinia": backend.luscinia is not None,
            "luscinia_active": (backend.luscinia is not None) and (not backend._luscinia_disabled) and (backend.luscinia_predictor is not None),
        },
        "counts": {
            "poetry_files": len(poetry_samples),
            "prose_files": len(prose_samples),
        },
        "poetry_summary": summarize_metrics(poetry_samples),
        "prose_summary": summarize_metrics(prose_samples),
        "best_thresholds": [t.as_dict() for t in thresholds],
        "samples": [s.as_dict() for s in all_samples],
    }


def format_analysis_report(result: dict[str, object]) -> str:
    lines: list[str] = []
    backend = result.get("backend", {})
    counts = result.get("counts", {})
    best_thresholds = result.get("best_thresholds", [])

    lines.append("Corpus Metrics Report")
    lines.append("=" * 72)
    lines.append(
        "Backends: ua-stress-engine={ua} luscinia={ls} luscinia_active={lsa}".format(
            ua=backend.get("ua_stress_engine"),
            ls=backend.get("luscinia"),
            lsa=backend.get("luscinia_active"),
        )
    )
    lines.append(f"Files: poetry={counts.get('poetry_files')} prose={counts.get('prose_files')}")
    lines.append("")
    lines.append("Top thresholds by balanced accuracy:")

    for row in best_thresholds[:5]:
        if isinstance(row, dict):
            lines.append(
                "- {metric}: poetry if value {direction} {threshold:.4f} | BA={ba:.3f} P={p:.3f} R={r:.3f}".format(
                    metric=row.get("metric", "?"),
                    direction=row.get("direction", ">="),
                    threshold=float(row.get("threshold", 0.0)),
                    ba=float(row.get("balanced_accuracy", 0.0)),
                    p=float(row.get("precision", 0.0)),
                    r=float(row.get("recall", 0.0)),
                )
            )
    return "\n".join(lines)


def save_result(result: dict[str, object], output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8") as handle:
        json.dump(result, handle, ensure_ascii=False, indent=2)