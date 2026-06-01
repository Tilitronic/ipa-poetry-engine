"""Console progress utilities for corpus baseline runs."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TextIO
import sys


@dataclass
class ProgressReporter:
    stream: TextIO = sys.stdout
    width: int = 28

    def header(self, title: str) -> None:
        print("=" * 72, file=self.stream)
        print(title, file=self.stream)
        print("=" * 72, file=self.stream)

    def bar(self, done: int, total: int) -> str:
        total = max(1, total)
        done = max(0, min(done, total))
        filled = int((done / total) * self.width)
        return "[" + ("#" * filled).ljust(self.width, ".") + f"] {done}/{total}"

    def category_start(self, category: str, total: int) -> None:
        print(f"\n[{category}] {self.bar(0, total)}", file=self.stream)

    def file_done(self, category: str, current: int, total: int, name: str) -> None:
        print(f"  [{category}] {self.bar(current, total)} {name}", file=self.stream)

    def category_done(self, category: str, elapsed_s: float) -> None:
        print(f"[{category}] done in {elapsed_s:.1f}s", file=self.stream)

    def status(self, message: str) -> None:
        print(message, file=self.stream)
