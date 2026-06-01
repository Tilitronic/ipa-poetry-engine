"""Corpus source selection for the baseline-analysis workflow."""

from __future__ import annotations

from dataclasses import dataclass
import re
import unicodedata
from typing import Iterable, List
from urllib.parse import quote

UBERTEXT_BASE = "https://text-mining.in.ua/static/downloads/ubertext2.0"
WIKISOURCE_BASE = "https://uk.wikisource.org/w/index.php?title="

# Default prose sources for the baseline workflow.
# These are the most suitable for sentence-aware analysis.
DEFAULT_PROSE_SOURCES = {
    "factual_prose": ["news", "wikipedia"],
    "literary_prose": ["fiction"],
}

# Poetry should be handled by a separate poetry corpus source.
# We use raw Wikisource pages so prose and poetry remain separate.
DEFAULT_POETRY_SOURCES = [
    "Український співаник/Ой там орав мужик",
    "Вовчі сини (Логос)",
    "До української дитини",
]

# Alternative poetry source: mova.info corpus (120M words, explicitly includes poetic texts)
# Can be used via: --poetry-source mova
ALTERNATIVE_POETRY_MOVA = [
    ("http://www.mova.info/corpus.aspx?l1=209", "mova_info_corpus", "Corpus of Ukrainian texts (120M words): includes poetic, folkloric texts"),
]


@dataclass(frozen=True)
class CorpusSource:
    group: str
    category: str
    variant: str
    url: str
    estimated_size: str
    filename: str


def ubertext_download_url(category: str, variant: str) -> str:
    return (
        f"{UBERTEXT_BASE}/{category}/{variant}/"
        f"ubertext.{category}.filter_rus_gcld+short.text_only.txt.bz2"
    )


def wikisource_raw_url(title: str) -> str:
    encoded = quote(title, safe="")
    return f"{WIKISOURCE_BASE}{encoded}&action=raw"


def slugify(text: str) -> str:
    normalized = unicodedata.normalize("NFKC", text)
    slug = re.sub(r"[^\w]+", "_", normalized, flags=re.UNICODE).strip("_").lower()
    return slug or "item"


def recommended_ubertext_variant(category: str) -> str:
    # Sentence-aware analysis works best when punctuation and sentence boundaries are preserved.
    return "sentenced"


def get_default_source_plan() -> list[CorpusSource]:
    plan: list[CorpusSource] = []
    for group, categories in DEFAULT_PROSE_SOURCES.items():
        for category in categories:
            variant = recommended_ubertext_variant(category)
            plan.append(
                CorpusSource(
                    group="prose",
                    category=category,
                    variant=variant,
                    url=ubertext_download_url(category, variant),
                    estimated_size={
                        "news": "3.4 GB",
                        "wikipedia": "803 MB",
                        "fiction": "398 MB",
                    }.get(category, "unknown"),
                    filename=f"ubertext_{category}_{variant}.txt.bz2",
                )
            )

    for title in DEFAULT_POETRY_SOURCES:
        plan.append(
            CorpusSource(
                group="poetry",
                category="wikisource",
                variant="raw",
                url=wikisource_raw_url(title),
                estimated_size="single work",
                filename=f"poetry_{slugify(title)}.txt",
            )
        )
    return plan


def iter_source_labels(sources: Iterable[CorpusSource]) -> List[str]:
    return [f"{src.group}/{src.category}/{src.variant}" for src in sources]
