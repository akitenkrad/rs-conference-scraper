# conf-scraper

Conference paper scraping and filtering tool. Collects paper metadata (title, authors, abstract, PDF URL) from major CS conferences and stores them in a local SQLite cache for keyword/category/LLM-based filtering.

## Installation

```bash
cargo install --path .
```

## Supported Conferences (26)

| Category | ID | Conference | Year Range |
|----------|----|-----------|------------|
| **NLP** | `acl` | ACL | 2002– |
| | `emnlp` | EMNLP | 2002– |
| | `naacl` | NAACL | 2003– |
| | `coling` | COLING | 2002– |
| | `eacl` | EACL | 2003– |
| | `aacl` | AACL | 2020– |
| | `lrec` | LREC | 2002– |
| | `conll` | CoNLL | 2002– |
| | `semeval` | SemEval | 2007– |
| | `sigdial` | SIGDIAL | 2003– |
| | `ijcnlp` | IJCNLP | 2005– |
| | `wmt` | WMT | 2006– |
| **ML/AI** | `neurips` | NeurIPS | 1987– |
| | `iclr` | ICLR | 2020– |
| | `icml` | ICML | 2013– |
| **CV** | `cvpr` | CVPR | 2013– |
| | `iccv` | ICCV | 2013– |
| **Security** | `usenix-security` | USENIX Security | 2014– |
| | `ndss` | NDSS | 2014– |
| | `sp` | IEEE S&P | 1981– |
| | `ccs` | CCS | 1994– |
| **Cryptography** | `crypto` | CRYPTO | 1981– |
| | `eurocrypt` | EUROCRYPT | 1985– |
| **Simulation** | `wsc` | WSC | 1968– |
| **Agents** | `aamas` | AAMAS | 2002– |

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

# Sync CRYPTO 2020-2024 (abstracts included via CryptoDB API)
conf-scraper sync --conference crypto --year 2020-2024

# Sync IEEE S&P 2024 (via DBLP API, no abstracts)
conf-scraper sync --conference sp --year 2024

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

| Conference | Source | Method | Abstracts |
|-----------|--------|--------|-----------|
| ACL, EMNLP, NAACL, COLING, EACL, AACL, LREC, CoNLL, SemEval, SIGDIAL, IJCNLP, WMT | ACL Anthology (GitHub XML) | XML parse | Yes |
| NeurIPS | papers.nips.cc | HTML scrape (2-pass) | Yes |
| ICLR | OpenReview API (v1/v2) | REST API | Yes |
| ICML | proceedings.mlr.press | HTML scrape (2-pass) | Yes |
| CVPR, ICCV | openaccess.thecvf.com | HTML scrape (2-pass) | Yes |
| USENIX Security | usenix.org | HTML scrape (10s crawl delay) | Yes |
| NDSS | ndss-symposium.org | HTML scrape (2-pass) | Yes |
| AAMAS | ifaamas.org | HTML scrape | No |
| CRYPTO, EUROCRYPT | CryptoDB API (iacr.org) | JSON API | Yes |
| IEEE S&P, CCS, WSC | DBLP Search API (dblp.org) | JSON API | No |

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `--cache-dir` | `~/.cache/conf-scraper` | SQLite cache directory |
| `--interval` | `1.5` | Seconds between HTTP requests |
| `--jobs` | `4` | Parallel abstract fetch concurrency |
| `--checkpoint` | `100` | Papers per batch save |
| `--retry` | `3` | Number of retries on failure |
| `-v, --verbose` | off | Debug logging |

## Development

```bash
# Build
cargo build

# Run tests (195 tests)
cargo test

# Run with verbose logging
cargo run -- -v sync --conference neurips --year 2024
```

## License

MIT
