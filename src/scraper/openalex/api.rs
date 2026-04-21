use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{compute_id, Paper};

// ---------------------------------------------------------------------------
// OpenAlex Works API レスポンス型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct OpenAlexResponse {
    pub meta: OpenAlexMeta,
    pub results: Vec<Work>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAlexMeta {
    #[allow(dead_code)]
    pub count: u64,
    /// ページネーション終了時は `None`
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Work {
    pub title: Option<String>,
    pub authorships: Option<Vec<Authorship>>,
    /// abstract は word→positions の inverted index 形式で返る
    pub abstract_inverted_index: Option<HashMap<String, Vec<u32>>>,
    pub doi: Option<String>,
    pub primary_location: Option<Location>,
    pub publication_year: Option<u16>,
    pub r#type: Option<String>,
    pub concepts: Option<Vec<Concept>>,
}

#[derive(Debug, Deserialize)]
pub struct Authorship {
    pub author: Author,
}

#[derive(Debug, Deserialize)]
pub struct Author {
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Location {
    pub landing_page_url: Option<String>,
    pub pdf_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Concept {
    pub display_name: Option<String>,
    pub level: Option<u32>,
    pub score: Option<f32>,
}

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

const PAGE_SIZE: u32 = 200;

/// 返却対象の OpenAlex type（front matter や erratum は除外）
fn is_accepted_type(t: &str) -> bool {
    matches!(t, "article" | "review")
}

/// 上位（level<=1）かつスコアが十分な concept のみを categories として採用
const CONCEPT_MAX_LEVEL: u32 = 1;
const CONCEPT_MIN_SCORE: f32 = 0.3;

/// OpenAlex の `abstract_inverted_index` を単一の平文文字列に復元
pub fn decode_abstract(inv: &HashMap<String, Vec<u32>>) -> String {
    if inv.is_empty() {
        return String::new();
    }
    let max_pos = inv.values().flatten().copied().max().unwrap_or(0) as usize;
    let mut words: Vec<String> = vec![String::new(); max_pos + 1];
    for (token, positions) in inv {
        for &pos in positions {
            if let Some(slot) = words.get_mut(pos as usize) {
                *slot = token.clone();
            }
        }
    }
    words
        .into_iter()
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// SHA256(title + abstract) でハッシュを計算
fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(abstract_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// OpenAlex Work を Paper に変換
fn work_to_paper(w: &Work, conf_id: &str) -> Option<Paper> {
    let t = w.r#type.as_deref().unwrap_or("");
    if !is_accepted_type(t) {
        return None;
    }

    let title = w.title.as_deref().unwrap_or("").trim().to_string();
    if title.is_empty() {
        return None;
    }

    let year = w.publication_year.unwrap_or(0);

    let authors: Vec<String> = w
        .authorships
        .as_ref()
        .map(|auths| {
            auths
                .iter()
                .filter_map(|a| a.author.display_name.clone())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let r#abstract = w
        .abstract_inverted_index
        .as_ref()
        .map(decode_abstract)
        .unwrap_or_default();

    let url = w
        .doi
        .clone()
        .or_else(|| {
            w.primary_location
                .as_ref()
                .and_then(|l| l.landing_page_url.clone())
        })
        .unwrap_or_default();

    let pdf_url = w
        .primary_location
        .as_ref()
        .and_then(|l| l.pdf_url.clone())
        .filter(|s| !s.is_empty());

    let categories: Vec<String> = w
        .concepts
        .as_ref()
        .map(|cs| {
            cs.iter()
                .filter(|c| {
                    c.level.unwrap_or(u32::MAX) <= CONCEPT_MAX_LEVEL
                        && c.score.unwrap_or(0.0) >= CONCEPT_MIN_SCORE
                })
                .filter_map(|c| c.display_name.clone())
                .collect()
        })
        .unwrap_or_default();

    let hash = compute_hash(&title, &r#abstract);

    Some(Paper {
        id: compute_id(&title),
        conference: conf_id.to_string(),
        year,
        title,
        authors,
        r#abstract,
        url,
        pdf_url,
        categories,
        hash,
    })
}

// ---------------------------------------------------------------------------
// 公開API
// ---------------------------------------------------------------------------

/// OpenAlex Works API から指定 ISSN・指定年の論文を取得（cursor ページネーション対応）
pub async fn fetch_papers_for_year(
    client: &reqwest::Client,
    issn: &str,
    conf_id: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let mut papers = Vec::new();
    let mut cursor = "*".to_string();
    let select = "title,authorships,abstract_inverted_index,doi,primary_location,publication_year,type,concepts";

    loop {
        let mut url = reqwest::Url::parse("https://api.openalex.org/works")
            .context("Failed to parse OpenAlex base URL")?;
        url.query_pairs_mut()
            .append_pair(
                "filter",
                &format!(
                    "primary_location.source.issn:{},publication_year:{}",
                    issn, year
                ),
            )
            .append_pair("per_page", &PAGE_SIZE.to_string())
            .append_pair("cursor", &cursor)
            .append_pair("select", select);

        let body = fetch_with_sleep(client, url.as_str(), interval)
            .await
            .with_context(|| {
                format!("OpenAlex fetch failed for ISSN {} year {}", issn, year)
            })?;

        let resp: OpenAlexResponse =
            serde_json::from_str(&body).context("Failed to parse OpenAlex response")?;

        for w in &resp.results {
            if let Some(p) = work_to_paper(w, conf_id) {
                papers.push(p);
            }
        }

        // next_cursor が無い，または空文字列なら終端
        match resp.meta.next_cursor {
            Some(c) if !c.is_empty() => cursor = c,
            _ => break,
        }
    }

    Ok(papers)
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_abstract_basic() {
        let mut inv: HashMap<String, Vec<u32>> = HashMap::new();
        inv.insert("Hello".to_string(), vec![0]);
        inv.insert("world".to_string(), vec![1]);
        inv.insert("hello".to_string(), vec![2]);
        assert_eq!(decode_abstract(&inv), "Hello world hello");
    }

    #[test]
    fn test_decode_abstract_repeated_word() {
        // "the cat and the dog" → the が 2箇所
        let mut inv: HashMap<String, Vec<u32>> = HashMap::new();
        inv.insert("the".to_string(), vec![0, 3]);
        inv.insert("cat".to_string(), vec![1]);
        inv.insert("and".to_string(), vec![2]);
        inv.insert("dog".to_string(), vec![4]);
        assert_eq!(decode_abstract(&inv), "the cat and the dog");
    }

    #[test]
    fn test_decode_abstract_empty() {
        let inv: HashMap<String, Vec<u32>> = HashMap::new();
        assert_eq!(decode_abstract(&inv), "");
    }

    #[test]
    fn test_is_accepted_type_filters_non_article() {
        assert!(is_accepted_type("article"));
        assert!(is_accepted_type("review"));
        assert!(!is_accepted_type("paratext"));
        assert!(!is_accepted_type("erratum"));
        assert!(!is_accepted_type("editorial"));
        assert!(!is_accepted_type(""));
    }

    #[test]
    fn test_work_to_paper_full() {
        let json = r#"{
            "title": "A Mathematical Model of Diffusion",
            "authorships": [
                { "author": { "display_name": "Jane Doe" } },
                { "author": { "display_name": "John Smith" } }
            ],
            "abstract_inverted_index": {
                "This": [0],
                "is": [1],
                "an": [2],
                "abstract": [3]
            },
            "doi": "https://doi.org/10.1080/0022250x.2024.001",
            "primary_location": {
                "landing_page_url": "https://www.tandfonline.com/doi/full/10.1080/0022250X.2024.001",
                "pdf_url": "https://www.tandfonline.com/doi/pdf/10.1080/0022250X.2024.001"
            },
            "publication_year": 2024,
            "type": "article",
            "concepts": [
                { "display_name": "Sociology", "level": 0, "score": 0.85 },
                { "display_name": "Mathematics", "level": 0, "score": 0.75 },
                { "display_name": "Specific Subfield", "level": 3, "score": 0.95 },
                { "display_name": "Low Relevance", "level": 1, "score": 0.1 }
            ]
        }"#;

        let w: Work = serde_json::from_str(json).unwrap();
        let p = work_to_paper(&w, "jms").unwrap();

        assert_eq!(p.title, "A Mathematical Model of Diffusion");
        assert_eq!(p.authors, vec!["Jane Doe", "John Smith"]);
        assert_eq!(p.r#abstract, "This is an abstract");
        assert_eq!(p.url, "https://doi.org/10.1080/0022250x.2024.001");
        assert_eq!(
            p.pdf_url.as_deref(),
            Some("https://www.tandfonline.com/doi/pdf/10.1080/0022250X.2024.001")
        );
        assert_eq!(p.year, 2024);
        assert_eq!(p.conference, "jms");
        // level<=1 かつ score>=0.3 の concept のみ
        assert_eq!(p.categories, vec!["Sociology", "Mathematics"]);
        assert_eq!(p.id, compute_id("A Mathematical Model of Diffusion"));
        assert_eq!(p.hash.len(), 64);
    }

    #[test]
    fn test_work_to_paper_skips_paratext() {
        let json = r#"{
            "title": "Editorial Board",
            "type": "paratext",
            "publication_year": 2024
        }"#;
        let w: Work = serde_json::from_str(json).unwrap();
        assert!(work_to_paper(&w, "jms").is_none());
    }

    #[test]
    fn test_work_to_paper_skips_erratum() {
        let json = r#"{
            "title": "Erratum for ...",
            "type": "erratum",
            "publication_year": 2024
        }"#;
        let w: Work = serde_json::from_str(json).unwrap();
        assert!(work_to_paper(&w, "jms").is_none());
    }

    #[test]
    fn test_work_to_paper_skips_empty_title() {
        let json = r#"{
            "title": "",
            "type": "article",
            "publication_year": 2024
        }"#;
        let w: Work = serde_json::from_str(json).unwrap();
        assert!(work_to_paper(&w, "jms").is_none());
    }

    #[test]
    fn test_work_to_paper_falls_back_to_landing_page_when_no_doi() {
        let json = r#"{
            "title": "Paper Without DOI",
            "type": "article",
            "publication_year": 2020,
            "primary_location": {
                "landing_page_url": "https://example.com/paper",
                "pdf_url": null
            }
        }"#;
        let w: Work = serde_json::from_str(json).unwrap();
        let p = work_to_paper(&w, "jms").unwrap();
        assert_eq!(p.url, "https://example.com/paper");
        assert!(p.pdf_url.is_none());
    }

    #[test]
    fn test_work_to_paper_no_abstract() {
        let json = r#"{
            "title": "Paper With No Abstract",
            "type": "article",
            "publication_year": 2020
        }"#;
        let w: Work = serde_json::from_str(json).unwrap();
        let p = work_to_paper(&w, "jms").unwrap();
        assert_eq!(p.r#abstract, "");
    }

    #[test]
    fn test_parse_openalex_response() {
        let json = r#"{
            "meta": { "count": 2, "next_cursor": "MTIzNDU=" },
            "results": [
                { "title": "A", "type": "article", "publication_year": 2024 },
                { "title": "B", "type": "article", "publication_year": 2024 }
            ]
        }"#;
        let r: OpenAlexResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.meta.count, 2);
        assert_eq!(r.meta.next_cursor.as_deref(), Some("MTIzNDU="));
        assert_eq!(r.results.len(), 2);
    }

    #[test]
    fn test_parse_openalex_response_null_cursor() {
        let json = r#"{
            "meta": { "count": 0, "next_cursor": null },
            "results": []
        }"#;
        let r: OpenAlexResponse = serde_json::from_str(json).unwrap();
        assert!(r.meta.next_cursor.is_none());
    }
}
