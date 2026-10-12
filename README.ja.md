<p align="center"><img src="docs/assets/hero.svg" width="100%"></p>

[English](README.md) | **日本語**

# conf-scraper

`conf-scraper` は，NLP・機械学習・画像認識・セキュリティ・暗号・ネットワーク・データマイニング・シミュレーション・国内学会の論文誌など，45 の学会と論文誌から論文のメタデータ（タイトル，著者，アブストラクト，URL，PDF の URL）を集め，手元の SQLite キャッシュに保存するツールです．

同期したキャッシュは，次のように使えます．

- キーワードやカテゴリで絞り込む．
- 集計する．
- ダッシュボードなど他のプログラムから JSON で読む．

各学会は，運営側が機械的な取得のために用意している窓口から取ります（ACL Anthology の XML，OpenReview，DBLP の SPARQL，IACR ePrint の OAI-PMH，CiNii Research など）．取得間隔の制御と `Retry-After` への対応も組み込んであります．

## インストール

```bash
cargo install --path .
```

## はじめかた

```bash
# 対応している学会を確認する
conf-scraper list-conferences

# NeurIPS 2024 の論文を同期する
conf-scraper sync --conference neurips --year 2024

# キーワードで絞り込む
conf-scraper filter --conference neurips --year 2024 \
  --filter keyword --keywords "transformer,attention"

# 学会 × 年の目次を JSON で出す
conf-scraper query catalog
```

キャッシュの置き場所は，既定で `~/.cache/conf-scraper` です．`--cache-dir` で変えられます．

## ドキュメント

- [使い方の例](docs/usecases.ja.md)：論文集の作成，文献調査，ダッシュボード，定期同期
- [CLI リファレンス](docs/cli.ja.md)：全コマンドとオプション，終了コード，再試行の挙動
- [学会と取得元](docs/venues.ja.md)：対応学会，それぞれの取得元，アブストラクトの有無
- [内部構成](docs/architecture.ja.md)：モジュール構成，同期の流れ，キャッシュのスキーマ，学会の追加方法

## ライセンス

MIT（[LICENSE](LICENSE) を参照）．
