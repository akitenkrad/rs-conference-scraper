use anyhow::Result;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::types::{compute_id, Paper, PaperListEntry};

// ---------------------------------------------------------------------------
// CryptoDB API レスポンス構造体
// ---------------------------------------------------------------------------

/// CryptoDB API は `{"papers": [...]}` 形式でレスポンスを返す
#[derive(Debug, Deserialize)]
pub struct CryptoDbResponse {
    pub papers: Vec<CryptoDbPaper>,
}

#[derive(Debug, Deserialize)]
pub struct CryptoDbPaper {
    pub title: String,
    pub authors: Vec<String>,
    #[serde(rename = "abstract")]
    pub paper_abstract: Option<String>,
    #[serde(rename = "DOI")]
    pub doi: Option<String>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
    pub award: Option<String>,
    #[allow(dead_code)]
    pub year: Option<u16>,
    #[allow(dead_code)]
    pub venue: Option<String>,
    #[allow(dead_code)]
    pub pubkey: Option<u64>,
    #[allow(dead_code)]
    pub pages: Option<serde_json::Value>,
    #[allow(dead_code)]
    pub youtube: Option<String>,
}

// ---------------------------------------------------------------------------
// ヘルパー関数
// ---------------------------------------------------------------------------

fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}{}", title, abstract_text).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn build_url(doi: Option<&str>, url: Option<&str>) -> String {
    if let Some(doi) = doi {
        if !doi.is_empty() {
            return format!("https://doi.org/{}", doi);
        }
    }
    url.unwrap_or("").to_string()
}

fn build_categories(award: Option<&str>) -> Vec<String> {
    match award {
        Some(a) if !a.is_empty() => vec![a.to_string()],
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// パース関数
// ---------------------------------------------------------------------------

/// CryptoDB APIのJSONレスポンスを解析し，PaperListEntryとPaperのペアを返す
pub fn parse_response(
    json_body: &str,
    venue_id: &str,
    year: u16,
) -> Result<Vec<(PaperListEntry, Paper)>> {
    // CryptoDB API は `{"papers": [...]}` または直接 `[...]` で返す場合がある
    let papers: Vec<CryptoDbPaper> = if json_body.trim_start().starts_with('{') {
        let resp: CryptoDbResponse = serde_json::from_str(json_body)?;
        resp.papers
    } else {
        serde_json::from_str(json_body)?
    };

    let results = papers
        .into_iter()
        .map(|p| {
            let abstract_text = p.paper_abstract.unwrap_or_default();
            let url = build_url(p.doi.as_deref(), p.url.as_deref());
            let categories = build_categories(p.award.as_deref());

            let entry = PaperListEntry {
                title: p.title.clone(),
                authors: p.authors.clone(),
                detail_url: url.clone(),
                track: categories.first().cloned(),
            };

            let paper = Paper {
                id: compute_id(&p.title),
                conference: venue_id.to_string(),
                year,
                title: p.title.clone(),
                authors: p.authors,
                r#abstract: abstract_text.clone(),
                url,
                pdf_url: None,
                categories,
                hash: compute_hash(&p.title, &abstract_text),
            };

            (entry, paper)
        })
        .collect();

    Ok(results)
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_response_with_multiple_papers() {
        let json = r#"[
            {
                "pubkey": 1001,
                "title": "Efficient Lattice-Based Signatures",
                "authors": ["Alice Smith", "Bob Jones"],
                "abstract": "We present a new lattice-based signature scheme.",
                "DOI": "10.1007/978-3-030-12345-6_1",
                "year": 2023,
                "venue": "crypto",
                "pages": "1-30",
                "URL": "https://example.com/paper1",
                "youtube": "https://youtube.com/watch?v=abc",
                "award": "Best Paper Award"
            },
            {
                "pubkey": 1002,
                "title": "Zero-Knowledge Proofs Revisited",
                "authors": ["Charlie Brown", "Diana Prince", "Eve Adams"],
                "abstract": "A comprehensive study of zero-knowledge proof systems.",
                "DOI": "10.1007/978-3-030-12345-6_2",
                "year": 2023,
                "venue": "crypto",
                "pages": "31-60",
                "URL": null,
                "youtube": null,
                "award": null
            }
        ]"#;

        let results = parse_response(json, "crypto", 2023).unwrap();
        assert_eq!(results.len(), 2);

        // 1つ目の論文を検証
        let (entry1, paper1) = &results[0];
        assert_eq!(entry1.title, "Efficient Lattice-Based Signatures");
        assert_eq!(entry1.authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(
            entry1.detail_url,
            "https://doi.org/10.1007/978-3-030-12345-6_1"
        );
        assert_eq!(entry1.track, Some("Best Paper Award".to_string()));

        assert_eq!(paper1.id, compute_id("Efficient Lattice-Based Signatures"));
        assert_eq!(paper1.conference, "crypto");
        assert_eq!(paper1.year, 2023);
        assert_eq!(
            paper1.r#abstract,
            "We present a new lattice-based signature scheme."
        );
        assert_eq!(
            paper1.url,
            "https://doi.org/10.1007/978-3-030-12345-6_1"
        );
        assert!(paper1.pdf_url.is_none());
        assert_eq!(paper1.categories, vec!["Best Paper Award"]);
        assert_eq!(paper1.hash.len(), 64);

        // 2つ目の論文を検証
        let (entry2, paper2) = &results[1];
        assert_eq!(entry2.title, "Zero-Knowledge Proofs Revisited");
        assert_eq!(entry2.authors.len(), 3);
        assert_eq!(
            entry2.detail_url,
            "https://doi.org/10.1007/978-3-030-12345-6_2"
        );
        assert_eq!(entry2.track, None);

        assert_eq!(paper2.conference, "crypto");
        assert!(paper2.categories.is_empty());
    }

    #[test]
    fn test_parse_missing_optional_fields() {
        let json = r#"[
            {
                "pubkey": 2001,
                "title": "Minimal Paper",
                "authors": ["Solo Author"],
                "abstract": null,
                "DOI": null,
                "year": 2022,
                "venue": "eurocrypt",
                "pages": null,
                "URL": null,
                "youtube": null,
                "award": null
            }
        ]"#;

        let results = parse_response(json, "eurocrypt", 2022).unwrap();
        assert_eq!(results.len(), 1);

        let (entry, paper) = &results[0];
        assert_eq!(entry.title, "Minimal Paper");
        assert_eq!(entry.authors, vec!["Solo Author"]);
        assert_eq!(entry.detail_url, "");
        assert_eq!(entry.track, None);

        assert_eq!(paper.conference, "eurocrypt");
        assert_eq!(paper.year, 2022);
        assert_eq!(paper.r#abstract, "");
        assert_eq!(paper.url, "");
        assert!(paper.pdf_url.is_none());
        assert!(paper.categories.is_empty());
    }

    #[test]
    fn test_parse_empty_response() {
        let json = "[]";
        let results = parse_response(json, "crypto", 2020).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_wrapped_response() {
        // CryptoDB API は `{"papers": [...]}` 形式でレスポンスを返す
        let json = r#"{
            "papers": [
                {
                    "pubkey": 3001,
                    "title": "Wrapped Paper",
                    "authors": ["Author One"],
                    "abstract": null,
                    "DOI": "10.1007/test",
                    "year": 2024,
                    "venue": "crypto",
                    "pages": 42,
                    "URL": null,
                    "youtube": null,
                    "award": null
                }
            ]
        }"#;

        let results = parse_response(json, "crypto", 2024).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.title, "Wrapped Paper");
    }

    #[test]
    fn test_parse_wrapped_empty_response() {
        let json = r#"{"papers": []}"#;
        let results = parse_response(json, "crypto", 2020).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_build_url_with_doi() {
        assert_eq!(
            build_url(Some("10.1007/test"), Some("https://fallback.com")),
            "https://doi.org/10.1007/test"
        );
    }

    #[test]
    fn test_build_url_without_doi_with_url() {
        assert_eq!(
            build_url(None, Some("https://example.com/paper")),
            "https://example.com/paper"
        );
    }

    #[test]
    fn test_build_url_empty_doi_falls_back_to_url() {
        assert_eq!(
            build_url(Some(""), Some("https://fallback.com")),
            "https://fallback.com"
        );
    }

    #[test]
    fn test_build_url_all_none() {
        assert_eq!(build_url(None, None), "");
    }

    #[test]
    fn test_build_categories_with_award() {
        assert_eq!(
            build_categories(Some("Best Paper Award")),
            vec!["Best Paper Award"]
        );
    }

    #[test]
    fn test_build_categories_without_award() {
        let empty: Vec<String> = vec![];
        assert_eq!(build_categories(None), empty);
        assert_eq!(build_categories(Some("")), empty);
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let h1 = compute_hash("Title", "Abstract");
        let h2 = compute_hash("Title", "Abstract");
        let h3 = compute_hash("Different", "Abstract");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
    }
}
