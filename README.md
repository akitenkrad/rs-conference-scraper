# conf-scraper

Conference paper scraping and filtering tool. Collects paper metadata (title, authors, abstract, PDF URL) from major CS conferences and stores them in a local SQLite cache for keyword/category/LLM-based filtering.

## Installation

```bash
cargo install --path .
```

## Supported Conferences (21)

| Category | Conferences |
|----------|------------|
| **NLP** | ACL, EMNLP, NAACL, COLING, EACL, AACL, LREC, CoNLL, SemEval, SIGDIAL, IJCNLP, WMT |
| **ML/AI** | NeurIPS, ICLR, ICML |
| **CV** | CVPR, ICCV |
| **Security** | USENIX Security, NDSS |
| **Agents** | AAMAS |

```bash
conf-scraper list-conferences
```

## Usage

### Sync papers

Scrape paper metadata from a conference and cache it locally.

```bash
# Sync NeurIPS 2024 papers
conf-scraper sync --conference neurips --year 2024

# Sync ACL 2023-2024 with 8 parallel jobs
conf-scraper sync --conference acl --year 2023-2024 --jobs 8

# Incremental sync (skip already-completed years)
conf-scraper sync --conference emnlp --year 2020-2024 --incremental
```

### Filter papers

Search cached papers by keywords, categories, or LLM scoring.

```bash
# Keyword filter
conf-scraper filter --conference neurips --year 2024 \
  --filter keyword --keywords "transformer,attention"

# Keyword filter on title only
conf-scraper filter --conference acl --year 2024 \
  --filter keyword --keywords "LLM,large language model" --fields title

# Category filter
conf-scraper filter --conference neurips --year 2024 \
  --filter category --tags "Datasets and Benchmarks"

# Combined filters (AND)
conf-scraper filter --conference acl --year 2024 \
  --filter keyword,category --keywords "summarization" --tags "Long Papers"

# LLM scoring with Anthropic API
conf-scraper filter --conference neurips --year 2024 \
  --filter llm --theme "papers about efficient inference for LLMs" \
  --threshold 0.8

# Save results to JSON
conf-scraper filter --conference iclr --year 2024 \
  --filter keyword --keywords "diffusion" --output results.json
```

### View statistics

```bash
# All conferences
conf-scraper stats

# Specific conference and year
conf-scraper stats --conference neurips --year 2024
```

### Cache management

```bash
# Check cache status
conf-scraper cache status
conf-scraper cache status --conference neurips

# Clear cache
conf-scraper cache clear --conference neurips --year 2023
conf-scraper cache clear  # clear all
```

## Data Sources

| Conference | Source | Method |
|-----------|--------|--------|
| ACL, EMNLP, NAACL, COLING, EACL, AACL, LREC, CoNLL, SemEval, SIGDIAL, IJCNLP, WMT | ACL Anthology (GitHub XML) | XML parse |
| NeurIPS | papers.nips.cc | HTML scrape |
| ICLR | OpenReview API (v1/v2) | REST API |
| ICML | proceedings.mlr.press | HTML scrape |
| CVPR, ICCV | openaccess.thecvf.com | HTML scrape |
| USENIX Security | usenix.org | HTML scrape (10s crawl delay) |
| NDSS | ndss-symposium.org | HTML scrape |
| AAMAS | ifaamas.org | HTML scrape (no abstracts) |

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `--cache-dir` | `~/.cache/conf-scraper` | SQLite cache directory |
| `--interval` | `1.5` | Seconds between HTTP requests |
| `--jobs` | `4` | Parallel abstract fetch concurrency |
| `--checkpoint` | `100` | Papers per batch save |
| `-v, --verbose` | off | Debug logging |

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Run with verbose logging
cargo run -- -v sync --conference neurips --year 2024
```

## License

MIT
