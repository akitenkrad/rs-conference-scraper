**English** | [日本語](usecases.ja.md)

# Use cases

## 1. Build a local corpus for a field

Sync the venues you follow once, then work offline against the cache.

```bash
# Security venues, last three years
conf-scraper sync --conference sp,ccs,ndss,usenix-security --year 2024-2026

# The whole IACR ePrint archive (one OAI-PMH harvest, about 60 requests)
conf-scraper sync --conference eprint

# Check what you have
conf-scraper stats --show summary,conferences,abstracts
```

Every venue ID is listed by `conf-scraper list-conferences` and in [Venues and data sources](venues.md).
If a run is interrupted or a source fails, run the same command again with `--incremental`: completed years are skipped and only the missing papers are fetched.

## 2. Literature survey with filters

```bash
# Keyword filter
conf-scraper filter --conference neurips --year 2024 \
  --filter keyword --keywords "transformer,attention"

# Keyword filter on title only
conf-scraper filter --conference acl --year 2024 \
  --filter keyword --keywords "LLM,large language model" --fields title

# Keywords and category tags combined (AND)
conf-scraper filter --filter keyword,category --keywords transformer --tags oral

# Several years, saved to a file
conf-scraper filter --conference cvpr --year 2020-2024 \
  --filter keyword --keywords "object detection" --limit 0 --output results.json
```

`--format` switches the output between `json` (default), `csv` and `xml`.

## 3. Fill in missing abstracts

Some sources do not publish abstracts (DBLP-based venues, AAMAS, part of OpenAlex and CiNii). `enrich` looks them up in Semantic Scholar, OpenAlex, arXiv, Crossref, the paper's HTML page and finally its PDF, stopping at the first hit.

```bash
conf-scraper enrich --conference sp,ccs --year 2024-2025
```

## 4. Feed a dashboard

`query` prints JSON only on stdout (logs go to stderr), so another program can run the binary and parse its output directly. Listings are paged and never include abstracts; fetch one paper to get its abstract.

```bash
# Venue × year catalog: paper counts, abstract coverage, sync state
conf-scraper query catalog

# One page of papers (terms are AND-ed, case-insensitive; max 500 per page)
conf-scraper query papers --conference acl,emnlp --year 2024-2025 \
  --q "agent planning" --fields title,abstract --offset 0 --limit 50 --facets

# One paper including its abstract ({"paper": null} if not found)
conf-scraper query paper --id <id> --conference acl --year 2025
```

The JSON shapes are described in the [CLI reference](cli.md#query).

## 5. Keep the cache fresh on a schedule

Proceedings appear in stages — a conference may publish one track first and the rest weeks later. `tools/weekly_sync.sh` runs `sync --incremental` for every venue in the catalog over the previous and current year. `--incremental` re-checks the listing of completed years for those two years, so editions that were only partially published at the last sync fill in over time.

```bash
bash tools/weekly_sync.sh
```

The script:

- refuses to start while another run holds its lock
- reads `CINII_APPID` from `~/.zshrc` when it is not set in the environment (schedulers such as launchd do not load shell profiles), and skips the CiNii venues if it is still missing
- uses `$CONF_SCRAPER_BIN` when set, otherwise `~/.cargo/bin/conf-scraper`
- exits with the status of `sync`, so a run that left years incomplete shows up as a failure in the scheduler

A weekly cron entry, for example:

```cron
0 3 * * 3  /bin/bash /path/to/conf-scraper/tools/weekly_sync.sh >> /tmp/conf-scraper-sync.log 2>&1
```
