[English](architecture.md) | **日本語**

# 内部構成

## モジュール構成

```text
src/
├── main.rs          # コマンドの振り分け
├── cli.rs           # clap の定義，--year の範囲の解析
├── conference.rs    # ConferenceScraper トレイト，学会の一覧，get_scraper()
├── types.rs         # Paper，PaperListEntry，compute_id()
├── sync/            # 同期の処理，取得間隔の制御，進捗表示
├── scraper/         # 取得元ごとのモジュール
│   ├── mod.rs       # HTTP クライアント，再試行の方針，確認画面の検出
│   ├── acl/  aamas/  cvf/  iclr/  icml/  jasss/  ndss/  neurips/  usenix/
│   ├── dblp/        # sparql.rs（主），api.rs と xml.rs（予備）
│   ├── eprint/      # oai.rs：OAI-PMH の一括取得．1 回の実行の中で共有する
│   ├── openalex/    # ISSN で引く論文誌
│   └── cinii/       # ISSN で引く国内の論文誌
├── cache/           # SQLite の読み書き（db.rs）と query の JSON（query.rs）
├── filter/          # キーワードとカテゴリの絞り込み
├── enrich/          # アブストラクトの探索（S2 → OpenAlex → arXiv → Crossref → HTML → PDF）
└── output/          # filter の json / csv / xml 出力
```

## scraper の約束事

学会はすべて `ConferenceScraper`（`src/conference.rs`）として実装します．

| メソッド | 役割 |
|---------|------|
| `id()`，`name()` | コマンドラインとキャッシュで使う学会 ID と，表示名 |
| `backend_id()` | 取得元の識別子．同じ取得元の学会は，取得間隔の制御を 1 つ共有する（DBLP の学会はすべて `dblp`） |
| `fetch_years()` | その学会がありうる年 |
| `fetch_paper_list(year)` | 1 年分の論文を `PaperListEntry`（タイトル，著者，詳細ページの URL）で返す |
| `fetch_paper_detail(entry)` | 1 件分の完全な `Paper` を返す |
| `details_are_local()` | `fetch_paper_detail` が，`fetch_paper_list` で取得済みのデータから返すなら `true`（DBLP，ePrint）．このとき，論文ごとの取得間隔の待ちを飛ばす |

すべてを 1 回の応答で返す取得元（DBLP の SPARQL，ePrint の OAI-PMH）は，`fetch_paper_list` の中で完全な `Paper` を組み立ててメモリに持ちます．`fetch_paper_detail` はそれを引くだけです．

2 段階の取得元（NeurIPS，ICML，CVF，NDSS など）は，まず一覧のページを取り，続いて論文ごとのページを取ります．

## 同期の流れ

```text
学会ごと（学会は並行に走る．取得間隔の制御は取得元ごとに 1 つ）
  対象の年ごと
    --incremental で完了済みなら飛ばす（今年と前年は除く）
    fetch_paper_list(year)            -> 一覧を取れない：その年を取り残しとして記録
                                      -> 一覧が空：その年は完了にしない
    キャッシュにある ID の論文を除く
    fetch_paper_detail(entry) × N     （--jobs 並列．手元から返す場合を除き --interval の間隔）
    --checkpoint 件ごとに保存          （INSERT OR IGNORE）
    失敗した論文が無いときだけ，その年を完了にする
取り残した年があれば 0 以外の終了コードで終わる
```

問い合わせの共通の方針は `src/scraper/mod.rs` にあります．

- 通常の失敗は 2，4，8 秒と待って再試行します．
- HTTP 429（と `Retry-After` 付きの 503）は，`Retry-After` に従います．ヘッダが無ければ 15〜120 秒待ち，再試行は最大 5 回です．
- 429 以外の 4xx は再試行しません．
- HTML のボット確認画面（Cloudflare，Anubis）は `<title>` で見分け，再試行せずに報告します．

## 論文の同一性

論文の ID は `SHA-256(title)`（`types::compute_id`）です．キャッシュのキーは `(id, conference, year)` です．ここから次のことが言えます．

- **同期を繰り返しても結果は変わりません．** キャッシュにある論文は飛ばし，保存には `INSERT OR IGNORE` を使います．
- **毎回同じタイトルを作る必要があります．** scraper はタイトルを一貫した形に整えます（たとえば DBLP では，末尾のピリオドを除き，XML の実体参照を元の文字に戻します）．既存の取得元で整え方を変えると，新しい ID で重複が入ります．
- **同じ学会・同じ年に同じタイトルの論文が 2 つあると，1 件として保存されます．**

## キャッシュのスキーマ

`--cache-dir` の下の `cache.db`：

```sql
-- 学会 × 年ごとの同期の状態
CREATE TABLE IF NOT EXISTS sync_years (
    conference TEXT NOT NULL,
    year       INTEGER NOT NULL,
    paper_count INTEGER NOT NULL DEFAULT 0,
    synced_at  TEXT,
    completed  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (conference, year)
);

-- 論文のメタデータ
CREATE TABLE IF NOT EXISTS papers (
    id         TEXT NOT NULL,
    conference TEXT NOT NULL,
    year       INTEGER NOT NULL,
    title      TEXT NOT NULL,
    authors    TEXT NOT NULL,      -- JSON の配列
    abstract   TEXT NOT NULL,      -- 不明なら ''
    url        TEXT NOT NULL,
    pdf_url    TEXT,
    categories TEXT NOT NULL,      -- JSON の配列
    hash       TEXT NOT NULL,      -- SHA-256(title + abstract)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (id, conference, year)
);
```

## 学会の追加

1. **DBLP にある学会**：新しいコードは要りません．
    - `src/conference.rs` の `get_scraper()` に，DBLP の stream キーと最初の年を渡して `DblpScraper` を作る分岐を足します．
    - `list_conferences()` に学会を足します．
2. **それ以外の学会**：
    - `src/scraper/<source>/` を作り，`ConferenceScraper` を実装する型を置きます．
    - `src/scraper/mod.rs` にモジュールを登録します．
    - `get_scraper()` と `list_conferences()` につなぎます．
3. **HTTP の問い合わせ**：`fetch_with_sleep` か `fetch_with_retry` を使います．再試行と確認画面の扱いがそのまま効きます．
4. **テスト**：決まった応答に対する解析をテストし（既存の scraper は `wiremock` を使っています），`cargo test` を走らせます．
