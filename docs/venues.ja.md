[English](venues.md) | **日本語**

# 学会と取得元

正式な一覧は `conf-scraper list-conferences` で表示できます．

## 対応している学会

| 分野 | ID | 学会・論文誌 | 年 |
|------|----|-------------|----|
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
| | `iccv` | ICCV | 2013–（隔年） |
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
| | `sng` | Simulation & Gaming（SAGE） | 1976– |
| | `jasag-sng` | シミュレーション&ゲーミング（JASAG） | 1991– |
| **Sociology** | `jms` | J. Math. Sociol. | 1971– |
| **Japanese** | `tjsai` | 人工知能学会論文誌 | 1986– |
| | `ipsj-jnl` | 情報処理学会論文誌 | 1960– |
| | `ieice-d` | 電子情報通信学会論文誌D | 1985– |

年の上限は今年です．まだ公開されていない回を指定すると空の年になりますが，失敗としては扱いません．

## 取得元

| 学会・論文誌 | 取得元 | 方式 | アブストラクト |
|-------------|--------|------|---------------|
| ACL，EMNLP，NAACL，COLING，EACL，AACL，LREC，CoNLL，SemEval，SIGDIAL，IJCNLP，WMT | ACL Anthology（GitHub の XML） | XML の解析 | あり |
| NeurIPS | papers.nips.cc | HTML の取得（2 段階） | あり |
| ICLR | OpenReview API（v1/v2） | REST API（OpenReview のアカウントが必要） | あり |
| ICML | proceedings.mlr.press | HTML の取得（2 段階） | あり |
| CVPR，ICCV | openaccess.thecvf.com | HTML の取得（2 段階） | あり |
| USENIX Security | usenix.org | HTML の取得（10 秒間隔） | あり |
| NDSS | ndss-symposium.org | HTML の取得（2 段階） | あり |
| AAMAS | ifaamas.org | HTML の取得 | なし |
| IEEE S&P，CCS，DSN，RAID，ESORICS，DIMVA，ACSAC，CNS，INFOCOM，SIGCOMM，IMC，KDD，ICDM，WSC | DBLP の SPARQL エンドポイント（sparql.dblp.org）．検索 API と XML を予備にする | SPARQL（JSON） | なし |
| CRYPTO，EUROCRYPT，ASIACRYPT | DBLP の SPARQL エンドポイント．アブストラクトは ePrint の OAI-PMH とタイトルで照合 | SPARQL ＋ OAI-PMH | 一部 |
| IACR ePrint | eprint.iacr.org の OAI-PMH（メタデータは CC0） | OAI-PMH の一括取得 | あり |
| JASSS | jasss.org | HTML の取得 | あり |
| J. Math. Sociol.，Simulation & Gaming | OpenAlex API（api.openalex.org） | JSON API（ISSN で絞り込み） | 一部 |
| 人工知能学会論文誌，情報処理学会論文誌，電子情報通信学会論文誌D，シミュレーション&ゲーミング | CiNii Research API（cir.nii.ac.jp） | JSON-LD API（ISSN で絞り込み．`CINII_APPID` が必要） | 一部（ライセンスによる） |

アブストラクトが «なし» や «一部» の学会は，[`enrich`](cli.ja.md#enrich) で補えます．

## 取得元ごとの注意

### DBLP

- **年の決め方**：DBLP の stream（`https://dblp.org/streams/conf/<key>`）と **開催年**（`dblp:yearOfEvent`）で論文を選びます．開催年の無い記録に限り，出版年で選びます．
    - Springer の LNCS の会議録は，開催の翌年に出ることがよくあります（ESORICS 2025 は 2026 年刊）．開催年で引けば，各回が自分の年に収まります．
- **対象**：会議とワークショップの論文（`dblp:Inproceedings`）だけを対象にします．同じ会議録の stream に載るワークショップの論文も，その学会に含めます．
- **予備の取得元**：SPARQL エンドポイントで失敗したら，DBLP の検索 API，続いて XML の会議録を試します．
    - これらは HTML 向けのサービスなので，ボットの確認画面を返すことがあります．その場合，同期はその年を取り残しとして報告します．
- **著者名**：DBLP の同名識別子が付いたまま入ります（例：`Ting Chen 0002`）．
- **同じタイトルの扱い**：論文の ID はタイトルから作ります．そのため，同じタイトルの記録が DBLP に 2 つあると，1 件にまとまります．

### IACR ePrint

- **取得方法**：アーカイブ全体を，1 回の実行につき 1 度だけ OAI-PMH で取ります（`ListRecords`．500 件ずつ，60 回ほどの問い合わせ）．
    - サイトの robots.txt は，このメタデータを CC0 で提供すると明記しています．PDF は取りません．
- **年の決め方**：年は論文番号から決めます（`2025/1234` なら 2025 年）．
    - OAI-PMH の日付による絞り込みは «最終更新日» に対するもので，論文の年ではありません．そのため使いません．
- **取り下げとカテゴリ**：取り下げられた論文（`status="deleted"`）は除きます．カテゴリは `dc:subject` から取ります．

### CRYPTO，EUROCRYPT，ASIACRYPT

- **アブストラクトの補い方**：論文は DBLP から取ります．アブストラクトは，正規化したタイトル（小文字にして英字と数字だけを残す）が ePrint の論文と一致したときに，そこから付けます．
    - 近年の論文は 83〜91% にアブストラクトが付きます．ePrint が始まった 1996 年より前の論文には，ほぼ付きません．
- **ePrint を取れなかったとき**：アブストラクト無しのまま保存せず，その年を失敗にします．次の `--incremental` で埋まります．
- **CryptoDB は使いません**：iacr.org の robots.txt が `/cryptodb/data` を禁止しているためです．

### OpenReview（ICLR）

- OpenReview のアカウントが必要です．`OPENREVIEW_TOKEN`，または `OPENREVIEW_USERNAME` と `OPENREVIEW_PASSWORD` を設定します（[CLI リファレンス](cli.ja.md#環境変数)）．2026 年 10 月から，OpenReview は匿名の API 利用者にブラウザの確認を求めるようになりました．ログインした問い合わせには求めません．
- 区分（`oral`，`spotlight`，`poster`）は各論文の venue の文字列から取るので，ICLR では `filter --tags oral` が使えます．

### CiNii Research

- `CINII_APPID` が必要です．
- 記録の `abstractLicenseFlag` が `"allow"` でなければ，アブストラクトを捨てます．

## アクセスの方針

`conf-scraper` は，各取得元が機械的な取得のために用意している窓口を使い，その制限を守ります．

- 同じ取得元への問い合わせは `--interval` の間隔をあけます．
- `Retry-After` に従います．
- robots.txt の制限に従います．
- ボットの確認画面（Cloudflare，Anubis）は報告するだけで，すり抜けません．
