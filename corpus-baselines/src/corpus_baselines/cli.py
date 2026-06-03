"""CLI for the separate corpus-baselines workflow."""

from __future__ import annotations

import argparse
import json
import shutil
import urllib.error
import urllib.request
from pathlib import Path

from .config import get_default_source_plan, iter_source_labels
from .metrics import format_analysis_report, run_analysis, save_result
from .progress import ProgressReporter


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Corpus baseline workflow")
    sub = parser.add_subparsers(dest="command", required=True)

    p_plan = sub.add_parser("plan", help="Print the recommended corpus plan")
    p_plan.add_argument("--json", action="store_true", help="Print the plan as JSON")

    p_download = sub.add_parser("download", help="Download the recommended prose sources")
    p_download.add_argument("--dest", type=Path, default=Path("data/ubertext"))
    p_download.add_argument("--dry-run", action="store_true", help="Only print the URLs")
    p_download.add_argument(
        "--exclude-category",
        action="append",
        default=[],
        help="Exclude a source category from download (repeatable)",
    )
    p_download.add_argument(
        "--include-group",
        action="append",
        default=[],
        help="Only download sources from these groups (repeatable)",
    )
    p_download.add_argument(
        "--poetry-source",
        choices=["wikisource", "mova"],
        default="wikisource",
        help="Poetry corpus source: wikisource (Wikisource pages) or mova (mova.info online corpus reference)",
    )

    p_analyze = sub.add_parser("analyze", help="Analyze prose/poetry corpora and search best thresholds")
    p_analyze.add_argument("--data-root", type=Path, default=Path("data"), help="Root with poetry/prose corpus files")
    p_analyze.add_argument("--max-files", type=int, default=25, help="Limit files per group (0 means all)")
    p_analyze.add_argument("--max-lines", type=int, default=2000, help="Limit lines per file (0 means all)")
    p_analyze.add_argument("--max-words", type=int, default=50000, help="Limit words per file (0 means all)")
    p_analyze.add_argument("--output", type=Path, default=Path("data/results/metrics_report.json"), help="JSON output path")

    return parser


def command_plan(as_json: bool) -> int:
    plan = get_default_source_plan()
    if as_json:
        print(json.dumps([src.__dict__ for src in plan], ensure_ascii=False, indent=2))
        return 0

    reporter = ProgressReporter()
    reporter.header("Corpus baseline plan")
    print("Sources:", ", ".join(iter_source_labels(plan)))
    for src in plan:
        print(f"- {src.group}/{src.category}/{src.variant} -> {src.estimated_size}")
        print(f"  {src.url}")
    print("\nDefault prose choice: sentenced UberText 2.0 prose sources")
    print("Poetry choice: raw Ukrainian Wikisource pages")
    return 0


def command_download(dest: Path, dry_run: bool, exclude_category: list[str], include_group: list[str], poetry_source: str) -> int:
    plan = get_default_source_plan()
    
    # If poetry_source is mova, remove wikisource poetry and add a note
    if poetry_source == "mova":
        plan = [src for src in plan if not (src.group == "poetry" and src.category == "wikisource")]
        # For mova, we only document it; actual download requires manual access to mova.info
        if include_group and "poetry" in include_group:
            reporter = ProgressReporter()
            reporter.header("Poetry source: mova.info")
            reporter.status("Note: mova.info corpus (120M words) requires manual download from http://www.mova.info/corpus.aspx?l1=209")
            reporter.status("This corpus explicitly includes poetic and folkloric Ukrainian texts.")
            reporter.status("Contact mova.info or use web search to download.")
            return 0
    
    if include_group:
        plan = [src for src in plan if src.group in include_group]
    if exclude_category:
        plan = [src for src in plan if src.category not in exclude_category]
    reporter = ProgressReporter()
    reporter.header("Downloading corpus sources")
    dest.mkdir(parents=True, exist_ok=True)
    failures = 0

    for idx, src in enumerate(plan, start=1):
        category_dir = dest / src.group / src.category / src.variant
        category_dir.mkdir(parents=True, exist_ok=True)
        target = category_dir / src.filename
        reporter.status(f"[{idx}/{len(plan)}] {src.group}/{src.category}/{src.variant}")
        reporter.status(f"  -> {src.url}")
        if dry_run:
            continue
        request = urllib.request.Request(
            src.url,
            headers={"User-Agent": "Mozilla/5.0 corpus-baselines/1.0"},
        )
        try:
            with urllib.request.urlopen(request) as response, target.open("wb") as handle:
                shutil.copyfileobj(response, handle)
        except (urllib.error.HTTPError, urllib.error.URLError) as exc:
            failures += 1
            reporter.status(f"  skipped: {exc}")
            continue
        reporter.status(f"  saved to {target}")

    return 1 if failures else 0


def command_analyze(data_root: Path, max_files: int, max_lines: int, max_words: int, output: Path) -> int:
    reporter = ProgressReporter()
    reporter.header("Corpus metrics analysis")
    reporter.status(f"Data root: {data_root}")
    reporter.status(f"Max files per group: {max_files}")
    reporter.status(f"Max lines per file: {max_lines}")
    reporter.status(f"Max words per file: {max_words}")

    result = run_analysis(
        data_root=data_root,
        max_files_per_group=max_files,
        max_lines_per_file=max_lines,
        max_words_per_file=max_words,
    )
    save_result(result, output)
    reporter.status(format_analysis_report(result))
    reporter.status(f"\nSaved JSON: {output}")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.command == "plan":
        return command_plan(args.json)
    if args.command == "download":
        return command_download(args.dest, args.dry_run, args.exclude_category, args.include_group, args.poetry_source)
    if args.command == "analyze":
        return command_analyze(args.data_root, args.max_files, args.max_lines, args.max_words, args.output)

    return 2


if __name__ == "__main__":
    raise SystemExit(main())
