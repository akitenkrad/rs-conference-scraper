use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{Paper, compute_id};

use super::api::{clean_title, compute_hash};

// ---------------------------------------------------------------------------
// DBLP SPARQL エンドポイント（主の取得元）
// ---------------------------------------------------------------------------
//
// 2026-10 から dblp.org の通常のページ・Search API・XML はボットの確認画面（Anubis）を
// 返すようになった．公式の SPARQL エンドポイントは確認画面を出さないので，こちらを主に使う
// （MYTASK-3417）．`dblp:publishedInStream` × `dblp:yearOfEvent` × `dblp:Inproceedings`
// で引いた件数は，これまで Search API で入れた年の件数と一致することを確認している
// （KDD 2024: 642，ESORICS 2024: 150 など）．

pub const SPARQL_ENDPOINT: &str = "https://sparql.dblp.org/sparql";

/// 年は «開催年»（`dblp:yearOfEvent`）で引く．出版年は LNCS 系で開催の翌年にずれる
/// （ESORICS 2025 の本会議録は 2026 年刊）ので，学会 × 年の目次には合わない．
/// 開催年の無い論文に限り出版年で引く．
///
/// 1 論文につき «著者の数» 行が返る（著者がいなければ 1 行）．
/// 集約（GROUP_CONCAT）はエンドポイント側で遅い（S&P 2026 で 26 秒）ので，平たい行で受けて手元でまとめる．
pub(crate) fn build_query(dblp_key: &str, year: u16) -> Result<String> {
    if dblp_key.is_empty()
        || !dblp_key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("invalid DBLP stream key: {:?}", dblp_key);
    }
    Ok(format!(
        r#"PREFIX dblp: <https://dblp.org/rdf/schema#>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?paper ?title ?doc ?ord ?name WHERE {{
  ?paper a dblp:Inproceedings ;
         dblp:publishedInStream <https://dblp.org/streams/conf/{key}> ;
         dblp:title ?title .
  {{ ?paper dblp:yearOfEvent "{year}"^^xsd:gYear }}
  UNION
  {{ ?paper dblp:yearOfPublication "{year}"^^xsd:gYear .
     FILTER NOT EXISTS {{ ?paper dblp:yearOfEvent ?event }} }}
  OPTIONAL {{ ?paper dblp:primaryDocumentPage ?doc }}
  OPTIONAL {{ ?paper dblp:hasSignature ?sig .
             ?sig dblp:signatureOrdinal ?ord ;
                  dblp:signatureDblpName ?name . }}
}}"#,
        key = dblp_key,
        year = year
    ))
}

#[derive(Debug, Deserialize)]
struct SparqlResponse {
    results: SparqlResults,
    #[serde(default)]
    meta: Option<SparqlMeta>,
}

#[derive(Debug, Deserialize)]
struct SparqlResults {
    bindings: Vec<BTreeMap<String, SparqlValue>>,
}

#[derive(Debug, Deserialize)]
struct SparqlValue {
    value: String,
}

#[derive(Debug, Deserialize)]
struct SparqlMeta {
    #[serde(rename = "result-size-total")]
    result_size_total: Option<u64>,
}

#[derive(Default)]
struct Row {
    title: String,
    docs: Vec<String>,
    authors: BTreeMap<u32, String>,
}

/// SPARQL の JSON 応答を論文の一覧にする．
///
/// - 論文（`?paper`）ごとに行をまとめ，著者は `signatureOrdinal` の順に並べる
/// - URL は `primaryDocumentPage`（複数あれば辞書順で最小のもの），無ければ DBLP のレコード URL
/// - 返る順は論文の IRI 順（実行ごとに変わらない）
pub(crate) fn parse_response(body: &str, conf_id: &str, year: u16) -> Result<Vec<Paper>> {
    let resp: SparqlResponse =
        serde_json::from_str(body).context("DBLP SPARQL response is not SPARQL JSON")?;

    let rows = resp.results.bindings.len() as u64;
    if let Some(total) = resp.meta.and_then(|m| m.result_size_total)
        && total > rows
    {
        bail!(
            "DBLP SPARQL response truncated: {} of {} rows returned",
            rows,
            total
        );
    }

    let mut papers: BTreeMap<String, Row> = BTreeMap::new();
    for b in &resp.results.bindings {
        let (Some(iri), Some(title)) = (b.get("paper"), b.get("title")) else {
            continue;
        };
        let row = papers.entry(iri.value.clone()).or_default();
        if row.title.is_empty() {
            row.title = title.value.clone();
        }
        if let Some(doc) = b.get("doc")
            && !row.docs.contains(&doc.value)
        {
            row.docs.push(doc.value.clone());
        }
        if let (Some(ord), Some(name)) = (b.get("ord"), b.get("name"))
            && let Ok(ord) = ord.value.trim().parse::<u32>()
        {
            row.authors.entry(ord).or_insert_with(|| name.value.clone());
        }
    }

    let mut out = Vec::with_capacity(papers.len());
    for (iri, row) in papers {
        let title = clean_title(&row.title);
        if title.is_empty() {
            continue;
        }
        let url = row.docs.iter().min().cloned().unwrap_or(iri);
        let abstract_text = String::new(); // DBLP はアブストラクトを提供しない
        out.push(Paper {
            id: compute_id(&title),
            conference: conf_id.to_string(),
            year,
            title: title.clone(),
            authors: row.authors.into_values().collect(),
            r#abstract: abstract_text.clone(),
            url,
            pdf_url: None,
            categories: vec![],
            hash: compute_hash(&title, &abstract_text),
        });
    }
    Ok(out)
}

/// SPARQL エンドポイントから指定年の論文を取得する
pub async fn fetch_papers_for_year(
    client: &reqwest::Client,
    endpoint: &str,
    dblp_key: &str,
    conf_id: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let query = build_query(dblp_key, year)?;
    let url = reqwest::Url::parse_with_params(endpoint, &[("query", query.as_str())])
        .with_context(|| format!("invalid SPARQL endpoint: {}", endpoint))?;
    let body = fetch_with_sleep(client, url.as_str(), interval)
        .await
        .with_context(|| format!("DBLP SPARQL query failed for {} {}", conf_id, year))?;
    parse_response(&body, conf_id, year)
        .with_context(|| format!("DBLP SPARQL response unusable for {} {}", conf_id, year))
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(paper: &str, title: &str, doc: Option<&str>, author: Option<(u32, &str)>) -> String {
        let mut fields = vec![
            format!(r#""paper": {{"type": "uri", "value": "{}"}}"#, paper),
            format!(r#""title": {{"type": "literal", "value": "{}"}}"#, title),
        ];
        if let Some(d) = doc {
            fields.push(format!(r#""doc": {{"type": "uri", "value": "{}"}}"#, d));
        }
        if let Some((o, n)) = author {
            fields.push(format!(
                r#""ord": {{"type": "literal", "datatype": "http://www.w3.org/2001/XMLSchema#int", "value": "{}"}}"#,
                o
            ));
            fields.push(format!(
                r#""name": {{"type": "literal", "value": "{}"}}"#,
                n
            ));
        }
        format!("{{{}}}", fields.join(", "))
    }

    fn response(bindings: &[String], total: Option<usize>) -> String {
        let meta = total
            .map(|t| {
                format!(
                    r#", "meta": {{"query-time-ms": 5, "result-size-total": {}}}"#,
                    t
                )
            })
            .unwrap_or_default();
        format!(
            r#"{{"head": {{"vars": ["paper", "title", "doc", "ord", "name"]}}, "results": {{"bindings": [{}]}}{}}}"#,
            bindings.join(", "),
            meta
        )
    }

    #[test]
    fn query_targets_stream_year_and_inproceedings() {
        let q = build_query("kdd", 2026).unwrap();
        assert!(q.contains("<https://dblp.org/streams/conf/kdd>"), "{}", q);
        assert!(q.contains(r#"dblp:yearOfEvent "2026"^^xsd:gYear"#), "{}", q);
        // 開催年の無い論文に限って出版年で引く
        assert!(
            q.contains(r#"dblp:yearOfPublication "2026"^^xsd:gYear"#),
            "{}",
            q
        );
        assert!(
            q.contains("FILTER NOT EXISTS { ?paper dblp:yearOfEvent ?event }"),
            "{}",
            q
        );
        assert!(q.contains("a dblp:Inproceedings"), "{}", q);
    }

    #[test]
    fn query_rejects_unsafe_keys() {
        assert!(build_query("sp", 2024).is_ok());
        assert!(build_query("ijcnn-2", 2024).is_ok());
        for bad in ["", "sp> } #", "a b", "sp\"", "../x"] {
            assert!(build_query(bad, 2024).is_err(), "accepted {:?}", bad);
        }
    }

    #[test]
    fn groups_rows_and_orders_authors_by_ordinal() {
        let p = "https://dblp.org/rec/conf/sp/A26";
        let body = response(
            &[
                binding(
                    p,
                    "Paper A.",
                    Some("https://doi.org/10.1/a"),
                    Some((10, "Judy")),
                ),
                binding(
                    p,
                    "Paper A.",
                    Some("https://doi.org/10.1/a"),
                    Some((2, "Bob 0002")),
                ),
                binding(
                    p,
                    "Paper A.",
                    Some("https://doi.org/10.1/a"),
                    Some((1, "Alice")),
                ),
            ],
            Some(3),
        );
        let papers = parse_response(&body, "sp", 2026).unwrap();
        assert_eq!(papers.len(), 1);
        let a = &papers[0];
        assert_eq!(a.title, "Paper A");
        assert_eq!(a.id, compute_id("Paper A"));
        // 数値として並べる（"10" が "2" より前に来ない）
        assert_eq!(a.authors, vec!["Alice", "Bob 0002", "Judy"]);
        assert_eq!(a.url, "https://doi.org/10.1/a");
        assert_eq!(a.conference, "sp");
        assert_eq!(a.year, 2026);
        assert_eq!(a.hash, compute_hash("Paper A", ""));
    }

    #[test]
    fn url_prefers_smallest_document_page_then_record() {
        let body = response(
            &[
                binding(
                    "https://dblp.org/rec/conf/sp/B26",
                    "Paper B",
                    Some("https://z.example/b"),
                    None,
                ),
                binding(
                    "https://dblp.org/rec/conf/sp/B26",
                    "Paper B",
                    Some("https://doi.org/10.1/b"),
                    None,
                ),
                binding("https://dblp.org/rec/conf/sp/C26", "Paper C", None, None),
            ],
            None,
        );
        let papers = parse_response(&body, "sp", 2026).unwrap();
        assert_eq!(papers.len(), 2);
        assert_eq!(papers[0].url, "https://doi.org/10.1/b");
        assert!(papers[0].authors.is_empty());
        assert_eq!(papers[1].url, "https://dblp.org/rec/conf/sp/C26");
    }

    #[test]
    fn skips_empty_titles_and_accepts_empty_results() {
        let body = response(
            &[binding(
                "https://dblp.org/rec/conf/sp/D26",
                " . ",
                None,
                None,
            )],
            None,
        );
        assert!(parse_response(&body, "sp", 2026).unwrap().is_empty());
        assert!(
            parse_response(&response(&[], Some(0)), "sp", 2026)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn truncated_results_are_an_error() {
        let body = response(
            &[binding("https://dblp.org/rec/conf/sp/E26", "E", None, None)],
            Some(5),
        );
        let err = parse_response(&body, "sp", 2026).unwrap_err().to_string();
        assert!(err.contains("truncated"), "{}", err);
    }

    #[test]
    fn non_json_is_an_error() {
        assert!(parse_response("<html></html>", "sp", 2026).is_err());
    }

    #[tokio::test]
    async fn fetch_sends_query_and_parses() {
        use wiremock::matchers::{method, path, query_param_contains};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = response(
            &[binding(
                "https://dblp.org/rec/conf/kdd/F26",
                "Paper F.",
                Some("https://doi.org/10.1/f"),
                Some((1, "Frank")),
            )],
            Some(1),
        );
        Mock::given(method("GET"))
            .and(path("/sparql"))
            .and(query_param_contains(
                "query",
                "<https://dblp.org/streams/conf/kdd>",
            ))
            .and(query_param_contains("query", r#""2026"^^xsd:gYear"#))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let endpoint = format!("{}/sparql", server.uri());
        let papers = fetch_papers_for_year(&client, &endpoint, "kdd", "kdd", 2026, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].title, "Paper F");
        assert_eq!(papers[0].authors, vec!["Frank"]);
    }
}
