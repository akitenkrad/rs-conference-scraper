**English** | [日本語](cli.ja.md)

# CLI reference

```text
conf-scraper [OPTIONS] <COMMAND>
```

| Command | Purpose |
|---------|---------|
| [`sync`](#sync) | Fetch paper metadata and store it in the cache |
| [`filter`](#filter) | Filter cached papers by keyword and/or category |
| [`stats`](#stats) | Summarize what is in the cache |
| [`enrich`](#enrich) | Look up missing abstracts in external services |
| [`query`](#query) | Machine-readable JSON for dashboards |
| [`cache`](#cache) | Inspect or clear the cache |
| `list-conferences` | Print every supported venue ID, name and field |

Global options:

| Option | Default | Description |
|--------|---------|-------------|
| `--cache-dir <DIR>` | `~/.cache/conf-scraper` | Directory holding `cache.db` |
| `-v, --verbose` | off | Debug logging (to stderr) |
| `-V, --version` | | Print the version |

`--year` accepts a single year (`2024`) or an inclusive range (`2020-2024`) wherever it appears.

## sync

Fetches paper metadata for one or more venues and stores it in the cache.

```bash
# Sync NeurIPS 2024 papers
conf-scraper sync --conference neurips --year 2024

# Sync ACL 2023-2024 with 8 parallel jobs
conf-scraper sync --conference acl --year 2023-2024 --jobs 8

# Sync CRYPTO 2020-2024 (DBLP; abstracts matched from ePrint by title)
conf-scraper sync --conference crypto --year 2020-2024

# Sync IEEE S&P 2024 (via the DBLP SPARQL endpoint, no abstracts)
conf-scraper sync --conference sp --year 2024

# Sync Journal of Mathematical Sociology 2020-2024 (via OpenAlex API)
conf-scraper sync --conference jms --year 2020-2024

# Sync TJSAI via CiNii Research — requires CINII_APPID
export CINII_APPID=<your-appid>
conf-scraper sync --conference tjsai --year 2020-2024

# Incremental sync (skip already-completed years)
conf-scraper sync --conference emnlp --year 2020-2024 --incremental
```

| Option | Default | Description |
|--------|---------|-------------|
| `--conference <IDS>` | (required) | Venue IDs, comma-separated |
| `--year <YEARS>` | every year the venue has | Year or range |
| `--incremental` | off | Skip years already marked completed (see below) |
| `--force` | off | Delete the cached papers of each target year and fetch them again |
| `--jobs <N>` | `4` | Papers fetched concurrently |
| `--interval <SEC>` | `1.5` | Minimum seconds between requests to the same source |
| `--retry <N>` | `3` | Retries for ordinary request failures |
| `--checkpoint <N>` | `100` | Papers saved to the cache per batch |

**What gets fetched.** For each year, `sync` fetches the paper list and then only the papers that are not already in the cache — with or without `--incremental`. `--incremental` additionally skips years that are already marked completed, except the current and previous year: their listings are re-checked, because proceedings are often published in stages.

**Completion and exit status.** A year is marked completed only when its paper list was fetched and every paper in it was saved. A year whose list is empty (not yet published) is not a failure and is not marked completed. If any year is left incomplete, `sync` names those years and exits with a non-zero status; running it again with `--incremental` fetches only what is missing. Typical causes:

- the paper list could not be fetched
- some papers failed
- the source rate-limited the requests
- the source answered with a bot-challenge page

**Rate limits and challenges.** Requests to the same source are spaced by `--interval`. On HTTP 429 (or 503 with `Retry-After`), a request waits as the `Retry-After` header says, or 15, 30, 60, 120, 120 s when it is missing, for at most 5 retries. Other failures back off 2, 4, 8 s. Bot-challenge pages (Cloudflare, Anubis) are reported as `blocked by bot challenge` and are not retried. Sources whose papers all come from one bulk response (DBLP, ePrint) are not throttled per paper, only per request.

## filter

Searches the cache. Without `--conference`, every venue is searched.

```bash
# Keyword filter
conf-scraper filter --filter keyword --keywords transformer,attention

# Restrict to a venue and year
conf-scraper filter --conference neurips --year 2024 --filter keyword --keywords "large language model"

# Category tags (e.g. oral presentations)
conf-scraper filter --conference iclr --year 2024 --filter category --tags oral

# Keywords and categories combined (AND)
conf-scraper filter --filter keyword,category --keywords transformer --tags oral

# Keywords or categories (OR)
conf-scraper filter --filter keyword,category --keywords transformer --tags oral --combine or

# Title only
conf-scraper filter --filter keyword --keywords diffusion --fields title

# Limit the output and save it as JSON
conf-scraper filter --filter keyword --keywords reinforcement --limit 20 --output results.json
```

| Option | Default | Description |
|--------|---------|-------------|
| `--conference <ID>` | all venues | Venue ID |
| `--year <YEARS>` | all years | Year or range |
| `--filter <KINDS>` | | `keyword`, `category`, or both (comma-separated) |
| `--keywords <WORDS>` | | Keywords, comma-separated |
| `--fields <FIELDS>` | `title,abstract` | Fields the keywords are matched against |
| `--tags <TAGS>` | | Category tags, comma-separated |
| `--combine <MODE>` | `and` | How several filters combine: `and` or `or` |
| `--limit <N>` | `20` | Maximum papers to output (`0` = no limit) |
| `--output <FILE>` | stdout | Write the result to a file |
| `--format <FMT>` | `json` | `json`, `csv` or `xml` |
| `--offline` | off | Exit with an error, instead of printing a message, when no cached papers match the venue/year |

The JSON output echoes the query and lists full paper records (with abstracts):

```json
{
  "query": { "conferences": ["eprint"], "years": [2025], "filters": ["keyword"], "combine": "and" },
  "total": 1,
  "papers": [
    { "id": "fc5eff44…", "title": "…", "authors": ["…"], "abstract": "…",
      "conference": "eprint", "year": 2025, "url": "https://eprint.iacr.org/2025/901",
      "pdf_url": "https://eprint.iacr.org/2025/901.pdf", "…": "…" }
  ]
}
```

## stats

```bash
# All conferences
conf-scraper stats

# Specific conference and year
conf-scraper stats --conference neurips --year 2024
```

`--show` selects sections: `summary`, `conferences`, `years`, `categories`, `authors`, `abstracts` (comma-separated; all by default).

## enrich

Looks up abstracts for cached papers that have none, trying in order: Semantic Scholar, OpenAlex, arXiv (only for venues where preprints are common), Crossref, the paper's HTML page, and the PDF. The first source with an abstract wins.

```bash
conf-scraper enrich --conference sp,ccs --year 2024-2025
```

| Option | Default | Description |
|--------|---------|-------------|
| `--conference <IDS>` | | Venue IDs, comma-separated |
| `--year <YEARS>` | | Year or range |
| `--force` | off | Also re-fetch papers that already have an abstract |
| `--retry <N>` | `3` | Retries per request |
| `--wait <SEC>` | `10` | Wait before a retry |
| `--interval <SEC>` | `1.0` | Seconds between requests |
| `--checkpoint <N>` | `50` | Papers saved per batch |

## query

Prints JSON only on stdout; logs go to stderr. Every response carries `"schema": 1`.

```bash
# Venue × year catalog: paper counts, abstract coverage, sync state
conf-scraper query catalog

# One page of papers (terms are AND-ed, case-insensitive; max 500 per page)
conf-scraper query papers --conference acl,emnlp --year 2024-2025 \
  --q "agent planning" --fields title,abstract --offset 0 --limit 50 --facets

# One paper including its abstract ({"paper": null} if not found)
conf-scraper query paper --id <id> --conference acl --year 2025
```

`query papers` options:

| Option | Default | Description |
|--------|---------|-------------|
| `--conference <IDS>` | all venues | Venue IDs, comma-separated |
| `--year <YEARS>` | all years | Year or range |
| `--q <TERMS>` | | Whitespace-separated terms, AND-ed, case-insensitive |
| `--fields <FIELDS>` | `title` | Any of `title`, `authors`, `abstract` |
| `--offset <N>` | `0` | Papers to skip |
| `--limit <N>` | `50` | Page size (1–500) |
| `--sort <ORDER>` | `year-desc` | `year-desc`, `year-asc` or `title` |
| `--facets` | off | Add per-venue and per-year counts, each computed with only its own axis filter removed |

`query catalog`:

```json
{ "schema": 1, "total": 296504, "with_abstract": 265834,
  "venues": [
    { "id": "acl", "name": "ACL", "field": "NLP", "total": 23208, "with_abstract": 23154,
      "years": [ { "year": 2026, "count": 6371, "with_abstract": 6317,
                   "synced": true, "completed": true, "synced_at": "2026-10-11 09:23:41" } ] } ] }
```

`query papers` (listings never include abstracts; `has_abstract` says whether one exists):

```json
{ "schema": 1, "total": 98, "offset": 0, "limit": 1,
  "papers": [ { "id": "fc5eff44…", "conference": "eprint", "year": 2025, "title": "…",
                "authors": ["…"], "url": "https://eprint.iacr.org/2025/901",
                "pdf_url": "https://eprint.iacr.org/2025/901.pdf",
                "categories": ["Cryptographic protocols"], "has_abstract": true } ],
  "facets": { "conference": [ { "value": "eprint", "count": 98 } ],
              "year": [ { "value": 2025, "count": 98 } ] } }
```

## cache

```bash
# Check cache status
conf-scraper cache status
conf-scraper cache status --conference neurips

# Clear cache
conf-scraper cache clear --conference neurips --year 2023
conf-scraper cache clear  # clear all
```

## Environment variables

| Variable | Required for | Purpose |
|----------|--------------|---------|
| `CINII_APPID` | `tjsai`, `ipsj-jnl`, `ieice-d`, `jasag-sng` | CiNii Research API key. Get one at <https://support.nii.ac.jp/ja/cir/r_opensearch>. |

Sensitive query parameter values (`appid`, `api_key`, `token`, `apikey`, `key`) are redacted to `***` in `-v` debug logs.
