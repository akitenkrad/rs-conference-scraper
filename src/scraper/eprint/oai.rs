use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::OnceCell;

use crate::scraper::fetch_with_sleep;
use crate::types::{Paper, compute_id};

// ---------------------------------------------------------------------------
// ePrint の OAI-PMH（MYTASK-3419）
// ---------------------------------------------------------------------------
//
// 論文ページを 1 件ずつ取ると 429 で止まる．robots.txt は «メタデータは OAI-PMH・RSS・Atom で
// CC0 として提供» と明記しているので，`ListRecords` で全件をまとめて取る（500 件 / ページ，
// 全体で 60 ページ弱）．`from` / `until` は «最終更新日» で絞るものなので年の指定には使えない．
// 年は論文番号（`2025/1234`）の上 4 桁で決める．
//
// タイトル・著者は前後の空白を除くだけにする．旧方式（一覧ページ）で入れた 1996〜2007 年の
// 2,411 件とこの形で完全に一致する（空白を詰めると ID が変わり重複する）．

const OAI_NS: &str = "http://www.openarchives.org/OAI/2.0/";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
/// resumptionToken が返り続けたときの打ち切り（実測は 57 ページ）
const MAX_PAGES: usize = 1000;

/// 1 回の OAI-PMH 一括取得の結果
#[derive(Debug, Default)]
pub struct Harvest {
    /// 論文番号の年 → その年の論文（取り下げは含まない）
    pub by_year: BTreeMap<u16, Vec<Paper>>,
    /// `title_key` → アブストラクト（同じキーが複数あれば論文番号の小さいもの）
    abstracts: HashMap<String, String>,
}

impl Harvest {
    fn from_papers(mut papers: Vec<(String, Paper)>) -> Self {
        // 論文番号順に並べる（`2025/0012` と `2025/12` を同じ順に扱うため数値で比べる）
        papers.sort_by_key(|(number, _)| number_order(number));
        let mut h = Harvest::default();
        for (_, paper) in papers {
            let key = title_key(&paper.title);
            if !key.is_empty() && !paper.r#abstract.is_empty() {
                h.abstracts
                    .entry(key)
                    .or_insert_with(|| paper.r#abstract.clone());
            }
            h.by_year.entry(paper.year).or_default().push(paper);
        }
        h
    }

    /// タイトルが一致する ePrint 論文のアブストラクト
    pub fn abstract_for(&self, title: &str) -> Option<&str> {
        self.abstracts.get(&title_key(title)).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.by_year.values().map(Vec::len).sum()
    }
}

/// 照合用のタイトルのキー：小文字にして英数字以外を除く
pub fn title_key(title: &str) -> String {
    title
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn number_order(number: &str) -> (u16, u32) {
    let (y, n) = number.split_once('/').unwrap_or((number, ""));
    (y.parse().unwrap_or(0), n.parse().unwrap_or(0))
}

fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(abstract_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 1 ページ分の応答
#[derive(Debug)]
struct Page {
    papers: Vec<(String, Paper)>,
    token: Option<String>,
}

/// `ListRecords` の応答 1 ページを読む．`noRecordsMatch` は空のページとして扱う．
fn parse_page(body: &str, conf_id: &str) -> Result<Page> {
    let doc = roxmltree::Document::parse(body).context("ePrint OAI-PMH response is not XML")?;
    let root = doc.root_element();
    if !root.has_tag_name((OAI_NS, "OAI-PMH")) {
        bail!("ePrint OAI-PMH response has no OAI-PMH root element");
    }
    if let Some(err) = root.children().find(|n| n.has_tag_name((OAI_NS, "error"))) {
        let code = err.attribute("code").unwrap_or("");
        if code == "noRecordsMatch" {
            return Ok(Page {
                papers: vec![],
                token: None,
            });
        }
        bail!(
            "ePrint OAI-PMH error {}: {}",
            code,
            err.text().unwrap_or("").trim()
        );
    }
    let Some(list) = root
        .children()
        .find(|n| n.has_tag_name((OAI_NS, "ListRecords")))
    else {
        bail!("ePrint OAI-PMH response has no ListRecords element");
    };

    let mut papers = Vec::new();
    let mut token = None;
    for node in list.children().filter(|n| n.is_element()) {
        if node.has_tag_name((OAI_NS, "resumptionToken")) {
            token = node
                .text()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(String::from);
            continue;
        }
        if !node.has_tag_name((OAI_NS, "record")) {
            continue;
        }
        let Some(header) = node.children().find(|n| n.has_tag_name((OAI_NS, "header"))) else {
            continue;
        };
        if header.attribute("status") == Some("deleted") {
            continue;
        }
        let Some(number) = header
            .descendants()
            .find(|n| n.has_tag_name((OAI_NS, "identifier")))
            .and_then(|n| n.text())
            .and_then(|id| id.trim().rsplit(':').next())
            .map(String::from)
        else {
            continue;
        };
        let Some(year) = number
            .split_once('/')
            .and_then(|(y, _)| y.parse::<u16>().ok())
        else {
            continue;
        };

        let dc = |name: &str| -> Vec<String> {
            node.descendants()
                .filter(|n| n.has_tag_name((DC_NS, name)))
                .map(|n| n.text().unwrap_or("").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        let Some(title) = dc("title").into_iter().next() else {
            continue;
        };
        let abstract_text = dc("description").into_iter().next().unwrap_or_default();
        let url = dc("identifier")
            .into_iter()
            .find(|u| u.starts_with("http"))
            .unwrap_or_else(|| format!("https://eprint.iacr.org/{}", number));
        let mut categories = dc("subject");
        categories.dedup();
        if categories.is_empty() {
            categories.push("Uncategorized".to_string());
        }

        papers.push((
            number,
            Paper {
                id: compute_id(&title),
                conference: conf_id.to_string(),
                year,
                hash: compute_hash(&title, &abstract_text),
                pdf_url: Some(format!("{}.pdf", url)),
                authors: dc("creator"),
                r#abstract: abstract_text,
                url,
                categories,
                title,
            },
        ));
    }
    Ok(Page { papers, token })
}

/// OAI-PMH で全件を取る（`resumptionToken` を最後までたどる）
pub async fn harvest(
    client: &reqwest::Client,
    base_url: &str,
    conf_id: &str,
    interval: Duration,
) -> Result<Harvest> {
    let endpoint = format!("{}/oai", base_url.trim_end_matches('/'));
    let mut params = vec![("verb", "ListRecords".to_string())];
    params.push(("metadataPrefix", "oai_dc".to_string()));
    let mut all = Vec::new();
    for page_no in 0..MAX_PAGES {
        let url = reqwest::Url::parse_with_params(&endpoint, &params)
            .with_context(|| format!("invalid ePrint OAI-PMH endpoint: {}", endpoint))?;
        let body = fetch_with_sleep(client, url.as_str(), interval)
            .await
            .with_context(|| format!("ePrint OAI-PMH page {} failed", page_no))?;
        let page = parse_page(&body, conf_id)
            .with_context(|| format!("ePrint OAI-PMH page {} unusable", page_no))?;
        all.extend(page.papers);
        match page.token {
            Some(token) => {
                params = vec![
                    ("verb", "ListRecords".to_string()),
                    ("resumptionToken", token),
                ];
            }
            None => return Ok(Harvest::from_papers(all)),
        }
    }
    bail!("ePrint OAI-PMH did not finish within {} pages", MAX_PAGES)
}

type Slot = Arc<OnceCell<Arc<Harvest>>>;
static SHARED: LazyLock<Mutex<HashMap<String, Slot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 1 回のプロセスの中で一括取得を使い回す（ePrint の各年と CRYPTO 系のアブストラクト補完で共有する）．
/// 失敗した結果は保存しないので，次の呼び出しで取り直す．
pub async fn shared_harvest(
    client: &reqwest::Client,
    base_url: &str,
    interval: Duration,
) -> Result<Arc<Harvest>> {
    let slot = {
        let mut map = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(base_url.to_string()).or_default().clone()
    };
    slot.get_or_try_init(|| async {
        tracing::info!("Harvesting ePrint metadata via OAI-PMH ({})", base_url);
        let h = harvest(client, base_url, "eprint", interval).await?;
        tracing::info!("ePrint OAI-PMH: {} records", h.len());
        Ok(Arc::new(h))
    })
    .await
    .cloned()
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn record(number: &str, title: &str, abs: &str, subjects: &[&str]) -> String {
        let subj: String = subjects
            .iter()
            .map(|s| format!("<dc:subject>{}</dc:subject>", s))
            .collect();
        format!(
            r#"<record><header><identifier>oai:eprint.iacr.org:{n}</identifier><datestamp>2025-01-01T00:00:00Z</datestamp></header>
<metadata><oai_dc:dc xmlns:oai_dc="http://www.openarchives.org/OAI/2.0/oai_dc/" xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier>https://eprint.iacr.org/{n}</dc:identifier><dc:title>{t}</dc:title>
<dc:creator>Alice</dc:creator><dc:creator> Bob  Builder </dc:creator>{subj}
<dc:description>{a}</dc:description><dc:date>2025-01-01T00:00:00Z</dc:date></oai_dc:dc></metadata></record>"#,
            n = number,
            t = title,
            a = abs,
            subj = subj
        )
    }

    pub(crate) fn page(records: &[String], token: Option<&str>) -> String {
        let tok = token
            .map(|t| format!(r#"<resumptionToken cursor="0">{}</resumptionToken>"#, t))
            .unwrap_or_else(|| r#"<resumptionToken cursor="500"/>"#.to_string());
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/"><responseDate>2026-10-12T00:00:00Z</responseDate><request verb="ListRecords">x</request><ListRecords>{}{}</ListRecords></OAI-PMH>"#,
            records.join(""),
            tok
        )
    }

    const DELETED: &str = r#"<record><header status="deleted"><identifier>oai:eprint.iacr.org:2025/9</identifier><datestamp>2025-01-01T00:00:00Z</datestamp></header></record>"#;

    #[test]
    fn parses_records_and_skips_deleted() {
        let body = page(
            &[
                record(
                    "2025/1234",
                    "  A  Paper &amp; More ",
                    "We show $x^2$.",
                    &["Foundations"],
                ),
                DELETED.to_string(),
                record("2008/545", "Old", "", &[]),
            ],
            Some("tok1"),
        );
        let p = parse_page(&body, "eprint").unwrap();
        assert_eq!(p.token.as_deref(), Some("tok1"));
        assert_eq!(p.papers.len(), 2);
        let (n, a) = &p.papers[0];
        assert_eq!(n, "2025/1234");
        assert_eq!(a.year, 2025);
        // 前後の空白だけ除く（内側の二重空白は残す）
        assert_eq!(a.title, "A  Paper & More");
        assert_eq!(a.id, compute_id("A  Paper & More"));
        assert_eq!(a.authors, vec!["Alice", "Bob  Builder"]);
        assert_eq!(a.r#abstract, "We show $x^2$.");
        assert_eq!(a.url, "https://eprint.iacr.org/2025/1234");
        assert_eq!(
            a.pdf_url.as_deref(),
            Some("https://eprint.iacr.org/2025/1234.pdf")
        );
        assert_eq!(a.categories, vec!["Foundations"]);
        assert_eq!(a.hash, compute_hash("A  Paper & More", "We show $x^2$."));
        assert_eq!(p.papers[1].1.year, 2008);
        assert_eq!(p.papers[1].1.categories, vec!["Uncategorized"]);
    }

    #[test]
    fn empty_token_ends_the_list_and_no_records_match_is_empty() {
        let p = parse_page(&page(&[], None), "eprint").unwrap();
        assert!(p.token.is_none());
        let body = r#"<?xml version="1.0"?><OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/"><error code="noRecordsMatch">none</error></OAI-PMH>"#;
        assert!(parse_page(body, "eprint").unwrap().papers.is_empty());
    }

    #[test]
    fn oai_errors_and_non_xml_are_errors() {
        let body = r#"<?xml version="1.0"?><OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/"><error code="badResumptionToken">expired</error></OAI-PMH>"#;
        let err = parse_page(body, "eprint").unwrap_err().to_string();
        assert!(err.contains("badResumptionToken"), "{}", err);
        assert!(parse_page("<html>Just a moment...</html>", "eprint").is_err());
        assert!(parse_page("not xml", "eprint").is_err());
    }

    #[test]
    fn title_key_ignores_case_punctuation_and_spacing() {
        assert_eq!(
            title_key("Signature-Free  Broadcast with $O(n^{2})$ Messages."),
            title_key("signature free broadcast with O(n2) messages")
        );
    }

    #[test]
    fn harvest_groups_by_year_and_prefers_lowest_number_for_abstracts() {
        let mk = |n: &str, t: &str, a: &str| {
            parse_page(&page(&[record(n, t, a, &[])], None), "eprint")
                .unwrap()
                .papers
                .remove(0)
        };
        let h = Harvest::from_papers(vec![
            mk("2024/100", "Same Title", "later"),
            mk("2024/12", "Same Title", "earlier"),
            mk("2023/5", "Other", ""),
        ]);
        assert_eq!(h.len(), 3);
        assert_eq!(h.by_year[&2024].len(), 2);
        assert_eq!(h.by_year[&2024][0].r#abstract, "earlier");
        assert_eq!(h.abstract_for("same title."), Some("earlier"));
        // 空のアブストラクトは照合に使わない
        assert_eq!(h.abstract_for("Other"), None);
    }

    #[tokio::test]
    async fn harvest_follows_resumption_tokens() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/oai"))
            .and(query_param("metadataPrefix", "oai_dc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page(
                &[record("2025/1", "First", "a1", &[])],
                Some("next page"),
            )))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/oai"))
            .and(query_param("resumptionToken", "next page"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(page(&[record("2026/7", "Second", "a2", &[])], None)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let h = harvest(&client, &server.uri(), "eprint", Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h.by_year[&2025][0].title, "First");
        assert_eq!(h.by_year[&2026][0].title, "Second");
    }
}
