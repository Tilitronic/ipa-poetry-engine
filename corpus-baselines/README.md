# Corpus Baselines

This subproject keeps the corpus-baseline workflow separate from the main text analyzer.

## What it does

- downloads or plans corpus sources for baseline analysis
- keeps prose and poetry grouped separately
- produces baseline summaries for the main analyzer in `IPA/`
- provides console feedback so long runs do not look frozen

## Recommended UberText 2.0 choice

For the prose baseline, the best default source is **UberText 2.0 sentenced** variants:

- `news/sentenced`
- `wikipedia/sentenced`
- `fiction/sentenced`

Why this choice:

- sentence boundaries are already explicit, which matches the line-oriented analysis pipeline
- punctuation is preserved enough for pause and boundary metrics
- the text is cleaned, so the analyzer spends less effort on garbage tokens
- `tokenized` is less convenient for our pipeline because it strips useful punctuation structure
- `court` is a special register and `social` is noisier, so both are weaker defaults for a general prose baseline

For poetry, keep a separate poetry corpus source. UberText 2.0 is useful for prose baselines, not as the main poetry source.

For the poetry baseline in this project we use raw Ukrainian Wikisource pages, one work per file. That keeps the poetry group separate from prose and preserves line/pause structure.

## Citation

Please cite the corpus authors when using UberText 2.0:

```bibtex
@inproceedings{chaplynskyi-2023-introducing,
  title = "Introducing {U}ber{T}ext 2.0: A Corpus of Modern {U}krainian at Scale",
  author = "Chaplynskyi, Dmytro",
  booktitle = "Proceedings of the Second Ukrainian Natural Language Processing Workshop",
  month = may,
  year = "2023",
  address = "Dubrovnik, Croatia",
  publisher = "Association for Computational Linguistics",
  url = "https://aclanthology.org/2023.unlp-1.1",
  pages = "1--10",
}
```

## Poetry corpus sources

By default, poetry is downloaded from **Ukrainian Wikisource** (3 works, raw page text). This source is small but proven to work reliably.

Alternative poetry sources available:

### mova.info corpus (120M words, explicit poetic texts)

Use `--poetry-source mova` to reference this online corpus. mova.info is a searchable linguistic corpus that explicitly includes Ukrainian poetic and folkloric texts. However, it requires manual download (no direct download link available). You can:

- Visit http://www.mova.info/corpus.aspx?l1=209
- Use web search tools to extract poetry texts
- Contact mova.info maintainers for batch access

Example: `python -m corpus_baselines.cli download --include-group poetry --poetry-source mova`

### Lang-uk corpus (alternative for investigation)

For future work, consider the **Lang-uk** corpus (2.5+ billion words, includes художні тексти / artistic texts). This is available as a downloadable archive and could serve as a richer poetry baseline.

## Usage

```bash
# See the full plan
python -m corpus_baselines.cli plan

# Download default sources (Wikisource poetry, UberText prose where available)
python -m corpus_baselines.cli download --dest ./data

# Download only poetry (Wikisource by default)
python -m corpus_baselines.cli download --include-group poetry --dest ./data/poetry

# Reference mova.info instead of Wikisource
python -m corpus_baselines.cli download --include-group poetry --poetry-source mova --dest ./data/poetry-mova

# Dry run (print URLs without downloading)
python -m corpus_baselines.cli download --dest ./data --dry-run
```
