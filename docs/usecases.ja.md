[English](usecases.md) | **日本語**

# 使い方の例

## 1. 分野の論文集を手元に作る

追っている学会を一度同期しておけば，あとはキャッシュを相手にオフラインで作業できます．

```bash
# セキュリティ系の学会を直近 3 年分
conf-scraper sync --conference sp,ccs,ndss,usenix-security --year 2024-2026

# IACR ePrint の全件（OAI-PMH の一括取得 1 回，60 回ほどの問い合わせ）
conf-scraper sync --conference eprint

# 何が入ったかを確かめる
conf-scraper stats --show summary,conferences,abstracts
```

学会 ID の一覧は，`conf-scraper list-conferences` と [学会と取得元](venues.ja.md) にあります．

途中で止まった場合や，取得元の失敗で取れなかった年がある場合は，同じコマンドに `--incremental` を付けて実行し直します．完了済みの年は飛ばし，足りない論文だけを取ります．

## 2. 絞り込みで文献を調べる

```bash
# キーワードで絞り込む
conf-scraper filter --conference neurips --year 2024 \
  --filter keyword --keywords "transformer,attention"

# タイトルだけを対象にする
conf-scraper filter --conference acl --year 2024 \
  --filter keyword --keywords "LLM,large language model" --fields title

# キーワードとカテゴリの組み合わせ（AND）
conf-scraper filter --filter keyword,category --keywords transformer --tags oral

# 複数年を対象にして，結果をファイルに保存する
conf-scraper filter --conference cvpr --year 2020-2024 \
  --filter keyword --keywords "object detection" --limit 0 --output results.json
```

出力形式は `--format` で選びます．`json`（既定），`csv`，`xml` のいずれかです．

## 3. 欠けたアブストラクトを補う

取得元によっては，アブストラクトを公開していません．DBLP から取る学会，AAMAS，OpenAlex と CiNii の一部がこれにあたります．

`enrich` は，アブストラクトを次の順に探し，最初に見つかったものを使います．

1. Semantic Scholar
2. OpenAlex
3. arXiv
4. Crossref
5. 論文の HTML ページ
6. PDF

```bash
conf-scraper enrich --conference sp,ccs --year 2024-2025
```

## 4. ダッシュボードから読む

`query` は，標準出力に JSON だけを出します（ログは標準エラーに出ます）．他のプログラムがこのバイナリを呼び出し，出力をそのまま解析できます．

一覧は常にページ単位で返り，アブストラクトを含みません．アブストラクトが要るときは，論文を 1 件ずつ取ります．

```bash
# 学会 × 年の目次（件数，アブストラクトの有無，同期の状態）
conf-scraper query catalog

# 論文一覧を 1 ページ分（語は AND で，大文字小文字を区別しない．1 ページ最大 500 件）
conf-scraper query papers --conference acl,emnlp --year 2024-2025 \
  --q "agent planning" --fields title,abstract --offset 0 --limit 50 --facets

# 論文 1 件（アブストラクト込み．見つからなければ {"paper": null}）
conf-scraper query paper --id <id> --conference acl --year 2025
```

JSON の形は [CLI リファレンス](cli.ja.md#query) にあります．

## 5. 定期的に同期してキャッシュを新しく保つ

会議録は段階的に公開されることがあります．たとえば，あるトラックが先に公開され，残りが数週間後に出る，という具合です．

`tools/weekly_sync.sh` は，目次にあるすべての学会について，前年と今年を `sync --incremental` で同期します．`--incremental` はこの 2 年分については完了済みでも一覧を取り直すので，前回の同期では一部しか公開されていなかった回も，あとから埋まります．

```bash
bash tools/weekly_sync.sh
```

このスクリプトの動作は次のとおりです．

- 別の実行がロックを持っている間は起動しません．
- `CINII_APPID` が環境変数に無ければ `~/.zshrc` から読みます．launchd などのスケジューラはシェルの設定を読まないためです．それでも無ければ，CiNii の学会を飛ばします．
- `$CONF_SCRAPER_BIN` があればそれを使い，無ければ `~/.cargo/bin/conf-scraper` を使います．
- 終了コードは `sync` のものをそのまま返します．取り残した年がある実行は，スケジューラ側で失敗として見えます．

週 1 回の cron の例：

```cron
0 3 * * 3  /bin/bash /path/to/conf-scraper/tools/weekly_sync.sh >> /tmp/conf-scraper-sync.log 2>&1
```
