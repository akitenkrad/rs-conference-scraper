**English** | [日本語](venues.ja.md)

# Venues and data sources

`conf-scraper list-conferences` prints the authoritative list.

## Supported venues

| Field | ID | Venue | Years |
|-------|----|-------|-------|
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

Years run up to the current year. Asking for an edition that has not been published yet returns an empty year, which is not treated as a failure.

## Data sources

| Venues | Source | Method | Abstracts |
|--------|--------|--------|-----------|
| ACL, EMNLP, NAACL, COLING, EACL, AACL, LREC, CoNLL, SemEval, SIGDIAL, IJCNLP, WMT | ACL Anthology (GitHub XML) | XML parse | Yes |
| NeurIPS | papers.nips.cc | HTML scrape (2-pass) | Yes |
| ICLR | OpenReview API (v1/v2) | REST API | Yes |
| ICML | proceedings.mlr.press | HTML scrape (2-pass) | Yes |
| CVPR, ICCV | openaccess.thecvf.com | HTML scrape (2-pass) | Yes |
| USENIX Security | usenix.org | HTML scrape (10 s crawl delay) | Yes |
| NDSS | ndss-symposium.org | HTML scrape (2-pass) | Yes |
| AAMAS | ifaamas.org | HTML scrape | No |
| IEEE S&P, CCS, DSN, RAID, ESORICS, DIMVA, ACSAC, CNS, INFOCOM, SIGCOMM, IMC, KDD, ICDM, WSC | DBLP SPARQL endpoint (sparql.dblp.org); Search API and XML as fallbacks | SPARQL (JSON results) | No |
| CRYPTO, EUROCRYPT, ASIACRYPT | DBLP SPARQL endpoint; abstracts from the ePrint OAI-PMH feed, matched by title | SPARQL + OAI-PMH | Partial |
| IACR ePrint | eprint.iacr.org OAI-PMH (CC0 metadata) | OAI-PMH bulk harvest | Yes |
| JASSS | jasss.org | HTML scrape | Yes |
| J. Math. Sociol., Simulation & Gaming | OpenAlex API (api.openalex.org) | JSON API (ISSN filter) | Partial |
| TJSAI, IPSJ-JNL, IEICE-D, JASAG SG | CiNii Research API (cir.nii.ac.jp) | JSON-LD API (ISSN filter, requires `CINII_APPID`) | Partial (license-gated) |

Venues marked **No** or **Partial** can be completed with [`enrich`](cli.md#enrich).

## Notes per source

### DBLP

- Papers are selected by DBLP stream (`https://dblp.org/streams/conf/<key>`) and **event year** (`dblp:yearOfEvent`), falling back to the publication year only for records without an event year. Springer LNCS proceedings often appear the year after the conference (ESORICS 2025 was published in 2026); keying by event year keeps each edition under its own year.
- Only conference and workshop papers (`dblp:Inproceedings`) are included. Workshop papers published in the same proceedings stream are part of the venue.
- If the SPARQL endpoint fails, the DBLP Search API and then the XML proceedings export are tried. These HTML-facing services may answer with a bot-challenge page; the sync then reports the year as incomplete.
- Author names keep DBLP's disambiguation suffix (e.g. `Ting Chen 0002`).
- Two DBLP records with the same title collapse into one paper, because paper IDs are derived from titles.

### IACR ePrint

- The whole archive is harvested once per run through OAI-PMH (`ListRecords`, about 60 requests of 500 records). The site's robots.txt offers this metadata under CC0; PDFs are not downloaded.
- The year is taken from the paper number (`2025/1234` → 2025). OAI-PMH date filters refer to the last modification, not the paper's year, so they are not used.
- Withdrawn papers (`status="deleted"`) are excluded. Categories come from `dc:subject`.

### CRYPTO, EUROCRYPT, ASIACRYPT

- Papers come from DBLP. Abstracts are taken from the ePrint harvest when the normalized title (lowercase, letters and digits only) matches. In recent years 83–91% of papers get an abstract; papers from before ePrint started (1996) generally have none.
- If the ePrint harvest is unavailable, the year fails instead of being stored without abstracts, so the next `--incremental` run fills it in.
- The CryptoDB API on iacr.org is not used: iacr.org's robots.txt disallows `/cryptodb/data`.

### CiNii Research

- Requires `CINII_APPID`.
- Abstracts are dropped when the record's `abstractLicenseFlag` is not `"allow"`.

## Access policy

`conf-scraper` uses the interfaces each publisher offers for machine access and respects their limits: requests to one source are spaced by `--interval`, `Retry-After` is honored, and robots.txt restrictions are followed. Bot-challenge pages (Cloudflare, Anubis) are reported and never bypassed.
