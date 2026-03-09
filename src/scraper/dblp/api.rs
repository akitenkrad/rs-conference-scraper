use anyhow::Result;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{compute_id, Paper};

// ---------------------------------------------------------------------------
// DBLP Search API レスポンス型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DblpResponse {
    pub result: DblpResult,
}

#[derive(Debug, Deserialize)]
pub struct DblpResult {
    pub hits: DblpHits,
}

#[derive(Debug, Deserialize)]
pub struct DblpHits {
    #[serde(rename = "@total")]
    pub total: String,
    #[allow(dead_code)]
    #[serde(rename = "@sent")]
    pub sent: String,
    #[allow(dead_code)]
    #[serde(rename = "@first")]
    pub first: String,
    pub hit: Option<Vec<DblpHit>>,
}

#[derive(Debug, Deserialize)]
pub struct DblpHit {
    pub info: DblpInfo,
}

#[derive(Debug, Deserialize)]
pub struct DblpInfo {
    pub title: Option<String>,
    pub authors: Option<DblpAuthors>,
    #[allow(dead_code)]
    pub venue: Option<String>,
    pub year: Option<String>,
    pub r#type: Option<String>,
    #[allow(dead_code)]
    pub doi: Option<String>,
    pub ee: Option<String>,
    pub url: Option<String>,
    #[allow(dead_code)]
    pub key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DblpAuthors {
    pub author: AuthorField,
}

/// `authors.author` は配列または単一オブジェクトの場合がある
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorField {
    Multiple(Vec<DblpAuthor>),
    Single(DblpAuthor),
}

#[derive(Debug, Deserialize)]
pub struct DblpAuthor {
    pub text: String,
}

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

const PAGE_SIZE: u64 = 1000;

/// タイトル末尾のピリオドを除去
fn clean_title(title: &str) -> String {
    title.trim().trim_end_matches('.').trim().to_string()
}

/// SHA256(title + abstract) でハッシュを計算
fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(abstract_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// `AuthorField` から著者名のベクタを取得
fn extract_authors(field: &AuthorField) -> Vec<String> {
    match field {
        AuthorField::Multiple(authors) => authors.iter().map(|a| a.text.clone()).collect(),
        AuthorField::Single(author) => vec![author.text.clone()],
    }
}

/// DBLP Hit を Paper に変換
fn hit_to_paper(hit: &DblpHit, conf_id: &str) -> Option<Paper> {
    let info = &hit.info;

    // タイプが "Conference and Workshop Papers" 以外はスキップ
    let paper_type = info.r#type.as_deref().unwrap_or("");
    if paper_type != "Conference and Workshop Papers" {
        return None;
    }

    let raw_title = info.title.as_deref().unwrap_or("");
    let title = clean_title(raw_title);
    if title.is_empty() {
        return None;
    }

    let year_str = info.year.as_deref().unwrap_or("0");
    let year: u16 = year_str.parse().unwrap_or(0);

    let authors = info
        .authors
        .as_ref()
        .map(|a| extract_authors(&a.author))
        .unwrap_or_default();

    let url = info
        .ee
        .as_deref()
        .or(info.url.as_deref())
        .unwrap_or("")
        .to_string();

    let abstract_text = String::new(); // DBLP はアブストラクトを提供しない

    Some(Paper {
        id: compute_id(&title),
        conference: conf_id.to_string(),
        year,
        title: title.clone(),
        authors,
        r#abstract: abstract_text.clone(),
        url,
        pdf_url: None,
        categories: vec![],
        hash: compute_hash(&title, &abstract_text),
    })
}

// ---------------------------------------------------------------------------
// 公開API
// ---------------------------------------------------------------------------

/// DBLP Search API から指定ベニューの全論文を取得（ページネーション対応）
pub async fn fetch_all_papers(
    client: &reqwest::Client,
    dblp_key: &str,
    conf_id: &str,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let mut papers = Vec::new();
    let mut offset: u64 = 0;

    loop {
        let url = format!(
            "https://dblp.org/search/publ/api?q=stream:streams/conf/{}:&format=json&h={}&f={}",
            dblp_key, PAGE_SIZE, offset
        );

        let body = fetch_with_sleep(client, &url, interval).await?;
        let resp: DblpResponse = serde_json::from_str(&body)?;

        let total: u64 = resp.result.hits.total.parse().unwrap_or(0);
        let hits = resp.result.hits.hit.unwrap_or_default();
        let page_len = hits.len() as u64;

        for hit in &hits {
            if let Some(paper) = hit_to_paper(hit, conf_id) {
                papers.push(paper);
            }
        }

        offset += page_len;
        if page_len == 0 || offset >= total {
            break;
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
    fn test_parse_dblp_response_multiple_authors() {
        let json = r#"{
            "result": {
                "hits": {
                    "@total": "2",
                    "@computed": "2",
                    "@sent": "2",
                    "@first": "0",
                    "hit": [
                        {
                            "info": {
                                "title": "Secure Multi-Party Computation.",
                                "authors": {
                                    "author": [
                                        {"@pid": "12/345", "text": "Alice Smith"},
                                        {"@pid": "67/890", "text": "Bob Jones"}
                                    ]
                                },
                                "venue": "SP",
                                "year": "2024",
                                "type": "Conference and Workshop Papers",
                                "doi": "10.1109/SP.2024.001",
                                "ee": "https://doi.org/10.1109/SP.2024.001",
                                "url": "https://dblp.org/rec/conf/sp/SmithJones24",
                                "key": "conf/sp/SmithJones24"
                            }
                        },
                        {
                            "info": {
                                "title": "Privacy-Preserving Analytics.",
                                "authors": {
                                    "author": [
                                        {"@pid": "11/222", "text": "Charlie Brown"}
                                    ]
                                },
                                "venue": "SP",
                                "year": "2023",
                                "type": "Conference and Workshop Papers",
                                "ee": "https://doi.org/10.1109/SP.2023.002",
                                "url": "https://dblp.org/rec/conf/sp/Brown23",
                                "key": "conf/sp/Brown23"
                            }
                        }
                    ]
                }
            }
        }"#;

        let resp: DblpResponse = serde_json::from_str(json).unwrap();
        let hits = resp.result.hits.hit.as_ref().unwrap();
        assert_eq!(hits.len(), 2);

        let papers: Vec<Paper> = hits
            .iter()
            .filter_map(|h| hit_to_paper(h, "sp"))
            .collect();
        assert_eq!(papers.len(), 2);

        // 1つ目の論文
        assert_eq!(papers[0].title, "Secure Multi-Party Computation");
        assert_eq!(papers[0].conference, "sp");
        assert_eq!(papers[0].year, 2024);
        assert_eq!(papers[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(
            papers[0].url,
            "https://doi.org/10.1109/SP.2024.001"
        );
        assert!(papers[0].pdf_url.is_none());
        assert!(papers[0].r#abstract.is_empty());
        assert_eq!(papers[0].id, compute_id("Secure Multi-Party Computation"));

        // 2つ目の論文
        assert_eq!(papers[1].title, "Privacy-Preserving Analytics");
        assert_eq!(papers[1].year, 2023);
    }

    #[test]
    fn test_parse_single_author() {
        let json = r#"{
            "result": {
                "hits": {
                    "@total": "1",
                    "@computed": "1",
                    "@sent": "1",
                    "@first": "0",
                    "hit": [
                        {
                            "info": {
                                "title": "Solo Research Paper.",
                                "authors": {
                                    "author": {"@pid": "99/100", "text": "Eve Solo"}
                                },
                                "venue": "CCS",
                                "year": "2022",
                                "type": "Conference and Workshop Papers",
                                "ee": "https://doi.org/10.1145/CCS.2022.001",
                                "url": "https://dblp.org/rec/conf/ccs/Solo22",
                                "key": "conf/ccs/Solo22"
                            }
                        }
                    ]
                }
            }
        }"#;

        let resp: DblpResponse = serde_json::from_str(json).unwrap();
        let hits = resp.result.hits.hit.as_ref().unwrap();
        let papers: Vec<Paper> = hits
            .iter()
            .filter_map(|h| hit_to_paper(h, "ccs"))
            .collect();

        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].title, "Solo Research Paper");
        assert_eq!(papers[0].authors, vec!["Eve Solo"]);
        assert_eq!(papers[0].conference, "ccs");
        assert_eq!(papers[0].year, 2022);
    }

    #[test]
    fn test_pagination_total_parsing() {
        let json = r#"{
            "result": {
                "hits": {
                    "@total": "4523",
                    "@computed": "4523",
                    "@sent": "0",
                    "@first": "0",
                    "hit": []
                }
            }
        }"#;

        let resp: DblpResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.result.hits.total, "4523");
        assert_eq!(
            resp.result.hits.total.parse::<u64>().unwrap(),
            4523
        );
        assert!(resp.result.hits.hit.unwrap_or_default().is_empty());
    }

    #[test]
    fn test_title_trailing_period_removal() {
        assert_eq!(clean_title("Paper Title."), "Paper Title");
        assert_eq!(clean_title("Paper Title"), "Paper Title");
        assert_eq!(clean_title("Title..."), "Title");
        assert_eq!(clean_title("  Title.  "), "Title");
    }

    #[test]
    fn test_non_conference_paper_filtered() {
        let json = r#"{
            "result": {
                "hits": {
                    "@total": "1",
                    "@computed": "1",
                    "@sent": "1",
                    "@first": "0",
                    "hit": [
                        {
                            "info": {
                                "title": "An Editorial Note.",
                                "authors": {
                                    "author": {"@pid": "1/1", "text": "Editor"}
                                },
                                "venue": "SP",
                                "year": "2024",
                                "type": "Editorship",
                                "url": "https://dblp.org/rec/conf/sp/Editorial24"
                            }
                        }
                    ]
                }
            }
        }"#;

        let resp: DblpResponse = serde_json::from_str(json).unwrap();
        let hits = resp.result.hits.hit.as_ref().unwrap();
        let papers: Vec<Paper> = hits
            .iter()
            .filter_map(|h| hit_to_paper(h, "sp"))
            .collect();

        assert!(papers.is_empty());
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let h1 = compute_hash("Title", "");
        let h2 = compute_hash("Title", "");
        let h3 = compute_hash("Other", "");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_extract_authors_multiple() {
        let field = AuthorField::Multiple(vec![
            DblpAuthor { text: "Alice".to_string() },
            DblpAuthor { text: "Bob".to_string() },
        ]);
        assert_eq!(extract_authors(&field), vec!["Alice", "Bob"]);
    }

    #[test]
    fn test_extract_authors_single() {
        let field = AuthorField::Single(DblpAuthor {
            text: "Charlie".to_string(),
        });
        assert_eq!(extract_authors(&field), vec!["Charlie"]);
    }

    #[test]
    fn test_null_hits_field() {
        let json = r#"{
            "result": {
                "hits": {
                    "@total": "0",
                    "@computed": "0",
                    "@sent": "0",
                    "@first": "0"
                }
            }
        }"#;

        let resp: DblpResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.hits.hit.is_none());
    }
}
