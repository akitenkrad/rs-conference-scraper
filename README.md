# conf-scraper

Academic paper scraping and filtering tool. Collects paper metadata (title, authors, abstract, PDF URL) from major conferences and journals, and stores them in a local SQLite cache for keyword/category/LLM-based filtering.

## Installation

```bash
cargo install --path .
```

## Supported Venues

Run `conf-scraper list-conferences` for the authoritative list.

| Category | ID | Venue | Year Range |
|----------|----|-------|------------|
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
| **ML** | `neurips` | NeurIPS | 1987– |
| | `iclr` | ICLR | 2018– |
| | `icml` | ICML | 2018– |
| **CV** | `cvpr` | CVPR | 2013– |
| | `iccv` | ICCV | 2013– (biennial) |
| **Security** | `usenix-security` | USENIX Security | 2014– |
| | `ndss` | NDSS | 2014– |
| | `sp` | IEEE S&P | 1981– |
| | `ccs` | CCS | 1994– |
| | `dsn` | IEEE/IFIP DSN | 2000– |
| | `raid` | RAID | 1998– |
| | `esorics` | ESORICS | 1990– |
| | `dimva` | DIMVA | 2004– |
| | `acsac` | ACSAC | 1985– |
| | `cns` | IEEE CNS | 2013– |
| **Cryptography** | `crypto` | CRYPTO | 1981– |
| | `eurocrypt` | EUROCRYPT | 1985– |
| | `asiacrypt` | ASIACRYPT | 1991– |
| | `eprint` | IACR ePrint | 1996– |
| **Networking** | `sigcomm` | ACM SIGCOMM | 1988– |
| | `infocom` | IEEE INFOCOM | 1982– |
| | `imc` | IMC | 2001– |
| **Data Mining** | `kdd` | KDD | 1995– |
| | `icdm` | IEEE ICDM | 2001– |
| **Multi-Agent** | `aamas` | AAMAS | 2013– |
| **Simulation** | `jasss` | JASSS | 1998– |
| | `wsc` | WSC | 1968– |
| | `sng` | Simulation & Gaming (SAGE) | 1976– |
| | `jasag-sng` | シミュレーション&ゲーミング (JASAG) | 1991– |
| **Sociology** | `jms` | J. Math. Sociol. | 1971– |
| **Japanese** | `tjsai` | 人工知能学会論文誌 | 1986– |
| | `ipsj-jnl` | 情報処理学会論文誌 | 1960– |
| | `ieice-d` | 電子情報通信学会論文誌D | 1985– |

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

# Sync Journal of Mathematical Sociology 2020-2024 (via OpenAlex API)
conf-scraper sync --conference jms --year 2020-2024

# Sync TJSAI (人工知能学会論文誌) via CiNii Research — requires CINII_APPID
export CINII_APPID=<your-appid>
conf-scraper sync --conference tjsai --year 2020-2024

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

| Venues | Source | Method | Abstracts |
|--------|--------|--------|-----------|
| ACL, EMNLP, NAACL, COLING, EACL, AACL, LREC, CoNLL, SemEval, SIGDIAL, IJCNLP, WMT | ACL Anthology (GitHub XML) | XML parse | Yes |
| NeurIPS | papers.nips.cc | HTML scrape (2-pass) | Yes |
| ICLR | OpenReview API (v1/v2) | REST API | Yes |
| ICML | proceedings.mlr.press | HTML scrape (2-pass) | Yes |
| CVPR, ICCV | openaccess.thecvf.com | HTML scrape (2-pass) | Yes |
| USENIX Security | usenix.org | HTML scrape (10s crawl delay) | Yes |
| NDSS | ndss-symposium.org | HTML scrape (2-pass) | Yes |
| AAMAS | ifaamas.org | HTML scrape | No |
| CRYPTO, EUROCRYPT, ASIACRYPT | CryptoDB API (iacr.org) | JSON API | Yes |
| IACR ePrint | eprint.iacr.org | HTML scrape | Yes |
| IEEE S&P, CCS, DSN, RAID, ESORICS, DIMVA, ACSAC, CNS, INFOCOM, SIGCOMM, IMC, KDD, ICDM, WSC | DBLP Search API (dblp.org) | JSON API | No |
| JASSS | jasss.org | HTML scrape | Yes |
| J. Math. Sociol., Simulation & Gaming | OpenAlex API (api.openalex.org) | JSON API (ISSN filter) | Partial |
| TJSAI, IPSJ-JNL, IEICE-D, JASAG SG | CiNii Research API (cir.nii.ac.jp) | JSON-LD API (ISSN filter, requires `CINII_APPID`) | Partial (license-gated) |

Abstracts marked **No** or **Partial** are supplemented by the enrichment pipeline (HTML → PDF → LLM tiers).
CiNii journals additionally drop abstracts when `abstractLicenseFlag != "allow"`.

## Configuration

### CLI options

| Option | Default | Description |
|--------|---------|-------------|
| `--cache-dir` | `~/.cache/conf-scraper` | SQLite cache directory |
| `--interval` | `1.5` | Seconds between HTTP requests |
| `--jobs` | `4` | Parallel abstract fetch concurrency |
| `--checkpoint` | `100` | Papers per batch save |
| `--retry` | `3` | Number of retries on failure |
| `-v, --verbose` | off | Debug logging |

### Environment variables

| Variable | Required for | Purpose |
|----------|--------------|---------|
| `CINII_APPID` | `tjsai`, `ipsj-jnl`, `ieice-d`, `jasag-sng` | CiNii Research API key. Get one at <https://support.nii.ac.jp/ja/cir/r_opensearch>. |
| `ANTHROPIC_API_KEY` | `filter --filter llm` | Anthropic API key for LLM scoring. |

Sensitive query parameter values (`appid`, `api_key`, `token`, `apikey`, `key`) are
automatically redacted to `***` in `-v` debug logs to prevent credential leakage.

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

---
*This file was generated by Claude Code.*
