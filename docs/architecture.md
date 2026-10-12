**English** | [日本語](architecture.ja.md)

# Architecture

## Module layout

```text
src/
├── main.rs          # command dispatch
├── cli.rs           # clap definitions, --year range parsing
├── conference.rs    # ConferenceScraper trait, venue list, get_scraper()
├── types.rs         # Paper, PaperListEntry, compute_id()
├── sync/            # the sync pipeline, rate limiter, progress bars
├── scraper/         # one module per source
│   ├── mod.rs       # HTTP client, retry policy, bot-challenge detection
│   ├── acl/  aamas/  cvf/  iclr/  icml/  jasss/  ndss/  neurips/  usenix/
│   ├── dblp/        # sparql.rs (primary), api.rs, xml.rs (fallbacks)
│   ├── eprint/      # oai.rs: OAI-PMH bulk harvest, shared within a run
│   ├── openalex/    # journals by ISSN
│   └── cinii/       # Japanese journals by ISSN
├── cache/           # SQLite access (db.rs) and the JSON reads behind `query` (query.rs)
├── filter/          # keyword and category filters
├── enrich/          # abstract lookup chain (S2 → OpenAlex → arXiv → Crossref → HTML → PDF)
└── output/          # json / csv / xml writers for `filter`
```

## The scraper contract

Every venue is a `ConferenceScraper` (`src/conference.rs`):

| Method | Role |
|--------|------|
| `id()`, `name()` | Venue ID used on the command line and in the cache; display name |
| `backend_id()` | Source identifier. Venues with the same backend share one rate limiter (all DBLP venues share `dblp`) |
| `fetch_years()` | Years the venue can have |
| `fetch_paper_list(year)` | The papers of one year, as `PaperListEntry` (title, authors, detail URL) |
| `fetch_paper_detail(entry)` | The full `Paper` for one entry |
| `details_are_local()` | `true` when `fetch_paper_detail` answers from data already fetched by `fetch_paper_list` (DBLP, ePrint). The per-paper rate-limit wait is then skipped |

Sources that return everything in one response (DBLP SPARQL, ePrint OAI-PMH) build complete `Paper`s in `fetch_paper_list` and keep them in memory; `fetch_paper_detail` only looks them up. Two-pass sources (NeurIPS, ICML, CVF, NDSS, …) fetch a listing page first and then each paper's page.

## The sync pipeline

```text
for each venue (venues run concurrently; one rate limiter per backend)
  for each target year
    skip if --incremental and the year is completed (except current and previous year)
    fetch_paper_list(year)            -> list failed?   record the year as incomplete
                                      -> list empty?    leave the year not completed
    drop entries whose id is already cached
    fetch_paper_detail(entry) × N     (--jobs in parallel, --interval apart unless local)
    save in --checkpoint batches      (INSERT OR IGNORE)
    mark the year completed only if no paper failed
exit non-zero if any year is incomplete
```

`src/scraper/mod.rs` holds the shared request policy: ordinary failures back off 2, 4, 8 s; HTTP 429 (and 503 with `Retry-After`) follows `Retry-After` or backs off 15–120 s for up to 5 retries; 4xx responses other than 429 are not retried; HTML bot-challenge pages (Cloudflare, Anubis) are detected by their `<title>` and reported without retrying.

## Paper identity

A paper's ID is `SHA-256(title)` (`types::compute_id`), and the cache key is `(id, conference, year)`. Consequences:

- Re-syncing is idempotent: already cached papers are skipped, and inserts use `INSERT OR IGNORE`.
- The same title must be produced every time. Scrapers normalize titles consistently (for example, DBLP titles lose their trailing period and XML entities are decoded) — changing the normalization of an existing source creates duplicates under new IDs.
- Two papers with identical titles in the same venue and year are stored once.

## Cache schema

`cache.db` in `--cache-dir`:

```sql
-- sync state per venue × year
CREATE TABLE IF NOT EXISTS sync_years (
    conference TEXT NOT NULL,
    year       INTEGER NOT NULL,
    paper_count INTEGER NOT NULL DEFAULT 0,
    synced_at  TEXT,
    completed  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (conference, year)
);

-- paper metadata
CREATE TABLE IF NOT EXISTS papers (
    id         TEXT NOT NULL,
    conference TEXT NOT NULL,
    year       INTEGER NOT NULL,
    title      TEXT NOT NULL,
    authors    TEXT NOT NULL,      -- JSON array
    abstract   TEXT NOT NULL,      -- '' when unknown
    url        TEXT NOT NULL,
    pdf_url    TEXT,
    categories TEXT NOT NULL,      -- JSON array
    hash       TEXT NOT NULL,      -- SHA-256(title + abstract)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (id, conference, year)
);
```

## Adding a venue

1. If the venue is in DBLP, no new code is needed: add an arm to `get_scraper()` in `src/conference.rs` that builds a `DblpScraper` with the venue's DBLP stream key and first year, and add the venue to `list_conferences()`.
2. Otherwise create `src/scraper/<source>/` with a type implementing `ConferenceScraper`, register the module in `src/scraper/mod.rs`, and wire it into `get_scraper()` and `list_conferences()`.
3. Use `fetch_with_sleep` / `fetch_with_retry` for HTTP so the retry and challenge handling apply.
4. Test parsing against fixed responses (the existing scrapers use `wiremock`), then run `cargo test`.
