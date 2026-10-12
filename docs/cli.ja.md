[English](cli.md) | **日本語**

# CLI リファレンス

```text
conf-scraper [OPTIONS] <COMMAND>
```

| コマンド | 役割 |
|---------|------|
| [`sync`](#sync) | 論文のメタデータを取得してキャッシュに保存する |
| [`filter`](#filter) | キャッシュの論文をキーワードやカテゴリで絞り込む |
| [`stats`](#stats) | キャッシュの中身を集計する |
| [`enrich`](#enrich) | 欠けたアブストラクトを外部のサービスで探す |
| [`query`](#query) | ダッシュボード向けに JSON で読み出す |
| [`cache`](#cache) | キャッシュの状態を見る，消す |
| `list-conferences` | 対応する学会の ID・名前・分野を一覧する |

全体のオプション：

| オプション | 既定値 | 説明 |
|-----------|--------|------|
| `--cache-dir <DIR>` | `~/.cache/conf-scraper` | `cache.db` を置くディレクトリ |
| `-v, --verbose` | 無効 | デバッグログを出す（標準エラー） |
| `-V, --version` | | バージョンを表示する |

`--year` は，どのコマンドでも 1 年（`2024`）か，両端を含む範囲（`2020-2024`）を受け付けます．

## sync

1 つ以上の学会について論文のメタデータを取得し，キャッシュに保存します．

```bash
# NeurIPS 2024 の論文を同期する
conf-scraper sync --conference neurips --year 2024

# ACL 2023-2024 を 8 並列で同期する
conf-scraper sync --conference acl --year 2023-2024 --jobs 8

# CRYPTO 2020-2024 を同期する（DBLP から．アブストラクトは ePrint とタイトルで照合）
conf-scraper sync --conference crypto --year 2020-2024

# IEEE S&P 2024 を同期する（DBLP の SPARQL から．アブストラクトは無し）
conf-scraper sync --conference sp --year 2024

# Journal of Mathematical Sociology 2020-2024 を同期する（OpenAlex API から）
conf-scraper sync --conference jms --year 2020-2024

# 人工知能学会論文誌を CiNii Research から同期する（CINII_APPID が必要）
export CINII_APPID=<your-appid>
conf-scraper sync --conference tjsai --year 2020-2024

# 差分だけ同期する（完了済みの年を飛ばす）
conf-scraper sync --conference emnlp --year 2020-2024 --incremental
```

| オプション | 既定値 | 説明 |
|-----------|--------|------|
| `--conference <IDS>` | （必須） | 学会 ID．カンマ区切りで複数指定できる |
| `--year <YEARS>` | その学会のすべての年 | 年または範囲 |
| `--incremental` | 無効 | 完了済みの年を飛ばす（後述） |
| `--force` | 無効 | 対象の年のキャッシュを消してから取り直す |
| `--jobs <N>` | `4` | 同時に取得する論文の数 |
| `--interval <SEC>` | `1.5` | 同じ取得元への問い合わせの最小間隔（秒） |
| `--retry <N>` | `3` | 通常の失敗に対する再試行の回数 |
| `--checkpoint <N>` | `100` | 何件ごとにキャッシュへ保存するか |

### 取得の範囲

`sync` は年ごとに，まず論文の一覧を取ります．そのうえで，キャッシュにまだ無い論文だけを取ります．これは `--incremental` の有無にかかわらず同じです．

`--incremental` を付けると，これに加えて完了済みの年を飛ばします．ただし今年と前年は，完了済みでも一覧を取り直します．会議録は段階的に公開されることが多いためです．

### 完了の扱いと終了コード

ある年が «完了» になるのは，一覧を取得でき，かつその年の論文をすべて保存できたときだけです．一覧が空の年（未公開）は失敗にせず，完了にもしません．

取り残した年があると，`sync` はその年を挙げて 0 以外の終了コードで終わります．`--incremental` を付けて実行し直せば，足りない分だけを取ります．取り残しの主な原因は次のとおりです．

- 一覧を取得できなかった
- 一部の論文で失敗した
- 取得元にアクセスを制限された
- 取得元がボットの確認画面を返した

### アクセス制限と確認画面

同じ取得元への問い合わせは，`--interval` の間隔をあけて送ります．

- HTTP 429（または `Retry-After` 付きの 503）を受けたら，`Retry-After` の秒数だけ待ちます．ヘッダが無ければ 15，30，60，120，120 秒と待ち，再試行は最大 5 回です．
- それ以外の失敗は 2，4，8 秒と待って再試行します．
- ボットの確認画面（Cloudflare，Anubis，OpenReview）は `blocked by bot challenge` として報告し，再試行しません．
- 論文をまとめて 1 回で返す取得元（DBLP，ePrint）は，論文ごとではなく問い合わせごとに間隔をあけます．

## filter

キャッシュを検索します．`--conference` を省くと，すべての学会が対象になります．

```bash
# キーワードで絞り込む
conf-scraper filter --filter keyword --keywords transformer,attention

# 学会と年を指定する
conf-scraper filter --conference neurips --year 2024 --filter keyword --keywords "large language model"

# カテゴリタグで絞り込む（例：Oral 発表）
conf-scraper filter --conference iclr --year 2024 --filter category --tags oral

# キーワードとカテゴリの組み合わせ（AND）
conf-scraper filter --filter keyword,category --keywords transformer --tags oral

# キーワードとカテゴリの組み合わせ（OR）
conf-scraper filter --filter keyword,category --keywords transformer --tags oral --combine or

# タイトルだけを対象にする
conf-scraper filter --filter keyword --keywords diffusion --fields title

# 件数を制限して JSON に保存する
conf-scraper filter --filter keyword --keywords reinforcement --limit 20 --output results.json
```

| オプション | 既定値 | 説明 |
|-----------|--------|------|
| `--conference <ID>` | すべての学会 | 学会 ID |
| `--year <YEARS>` | すべての年 | 年または範囲 |
| `--filter <KINDS>` | | `keyword`，`category`，または両方（カンマ区切り） |
| `--keywords <WORDS>` | | キーワード（カンマ区切り） |
| `--fields <FIELDS>` | `title,abstract` | キーワードを照合する項目 |
| `--tags <TAGS>` | | カテゴリタグ（カンマ区切り） |
| `--combine <MODE>` | `and` | 複数のフィルタの結合方法．`and` か `or` |
| `--limit <N>` | `20` | 出力する論文の最大数（`0` で無制限） |
| `--output <FILE>` | 標準出力 | 結果をファイルに書く |
| `--format <FMT>` | `json` | `json`，`csv`，`xml` |
| `--offline` | 無効 | 指定した学会・年の論文がキャッシュに無いとき，メッセージを出すのではなくエラーで終わる |

JSON の出力には，問い合わせの内容と，論文の全項目（アブストラクトを含む）が入ります．

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
# すべての学会
conf-scraper stats

# 学会と年を指定する
conf-scraper stats --conference neurips --year 2024
```

表示する項目は `--show` で選びます．`summary`，`conferences`，`years`，`categories`，`authors`，`abstracts` のいずれかで，カンマ区切りで複数指定できます．既定ではすべて表示します．

## enrich

アブストラクトの無い論文について，次の順にアブストラクトを探します．最初に見つかったものを使います．

1. Semantic Scholar
2. OpenAlex
3. arXiv（プレプリントが一般的な学会だけ）
4. Crossref
5. 論文の HTML ページ
6. PDF

```bash
conf-scraper enrich --conference sp,ccs --year 2024-2025
```

| オプション | 既定値 | 説明 |
|-----------|--------|------|
| `--conference <IDS>` | | 学会 ID（カンマ区切り） |
| `--year <YEARS>` | | 年または範囲 |
| `--force` | 無効 | アブストラクトのある論文も取り直す |
| `--retry <N>` | `3` | 問い合わせごとの再試行の回数 |
| `--wait <SEC>` | `10` | 再試行の前に待つ秒数 |
| `--interval <SEC>` | `1.0` | 問い合わせの間隔（秒） |
| `--checkpoint <N>` | `50` | 何件ごとに保存するか |

## query

標準出力に JSON だけを出します（ログは標準エラーに出ます）．どの応答にも `"schema": 1` が入ります．

```bash
# 学会 × 年の目次（件数，アブストラクトの有無，同期の状態）
conf-scraper query catalog

# 論文一覧を 1 ページ分（語は AND で，大文字小文字を区別しない．1 ページ最大 500 件）
conf-scraper query papers --conference acl,emnlp --year 2024-2025 \
  --q "agent planning" --fields title,abstract --offset 0 --limit 50 --facets

# 論文 1 件（アブストラクト込み．見つからなければ {"paper": null}）
conf-scraper query paper --id <id> --conference acl --year 2025
```

`query papers` のオプション：

| オプション | 既定値 | 説明 |
|-----------|--------|------|
| `--conference <IDS>` | すべての学会 | 学会 ID（カンマ区切り） |
| `--year <YEARS>` | すべての年 | 年または範囲 |
| `--q <TERMS>` | | 空白区切りの検索語．AND で結び，大文字小文字を区別しない |
| `--fields <FIELDS>` | `title` | `title`，`authors`，`abstract` から選ぶ |
| `--offset <N>` | `0` | 先頭から飛ばす件数 |
| `--limit <N>` | `50` | 1 ページの件数（1〜500） |
| `--sort <ORDER>` | `year-desc` | `year-desc`，`year-asc`，`title` |
| `--facets` | 無効 | 学会ごと・年ごとの件数も返す．それぞれ，その軸の絞り込みだけを外して数える |

`query catalog` の出力：

```json
{ "schema": 1, "total": 296504, "with_abstract": 265834,
  "venues": [
    { "id": "acl", "name": "ACL", "field": "NLP", "total": 23208, "with_abstract": 23154,
      "years": [ { "year": 2026, "count": 6371, "with_abstract": 6317,
                   "synced": true, "completed": true, "synced_at": "2026-10-11 09:23:41" } ] } ] }
```

`query papers` の出力（一覧にアブストラクトは含まれません．あるかどうかは `has_abstract` で分かります）：

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
# キャッシュの状態を見る
conf-scraper cache status
conf-scraper cache status --conference neurips

# キャッシュを消す
conf-scraper cache clear --conference neurips --year 2023
conf-scraper cache clear  # すべて消す
```

## 環境変数

| 変数 | 必要な学会・コマンド | 用途 |
|------|---------------------|------|
| `CINII_APPID` | `tjsai`，`ipsj-jnl`，`ieice-d`，`jasag-sng` | CiNii Research API のキー．<https://support.nii.ac.jp/ja/cir/r_opensearch> で取得する |
| `OPENREVIEW_TOKEN` | `iclr` | OpenReview API のトークン．`Authorization: Bearer` として送る．下の 2 つより優先する |
| `OPENREVIEW_USERNAME`，`OPENREVIEW_PASSWORD` | `iclr` | OpenReview のアカウント．1 回の実行につき 1 回だけログインし，得たトークンを使い回す |

OpenReview は匿名の API 利用者にブラウザの確認を求めるため，ICLR には `OPENREVIEW_TOKEN`，または `OPENREVIEW_USERNAME` と `OPENREVIEW_PASSWORD` の両方が必要です．設定が無いと，ICLR の各年は設定すべき変数を示すメッセージとともに失敗します．トークンとパスワードは，ログにもエラーメッセージにも出しません．

`-v` のデバッグログでは，秘密にすべき問い合わせパラメータの値（`appid`，`api_key`，`token`，`apikey`，`key`）を `***` に置き換えて出します．
