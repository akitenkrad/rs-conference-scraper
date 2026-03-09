use anyhow::{bail, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::types::{compute_id, Paper};

// ---------------------------------------------------------------------------
// OpenReview API v2 (ICLR 2024+)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ApiResponseV2 {
    pub notes: Vec<NoteV2>,
    pub count: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct NoteV2 {
    #[allow(dead_code)]
    pub id: String,
    pub forum: String,
    pub content: ContentV2,
}

#[derive(Debug, Deserialize)]
pub struct ContentV2 {
    pub title: ValueWrapper<String>,
    pub authors: Option<ValueWrapper<Vec<String>>>,
    #[serde(rename = "abstract")]
    pub r#abstract: Option<ValueWrapper<String>>,
    pub pdf: Option<ValueWrapper<String>>,
    pub venue: Option<ValueWrapper<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ValueWrapper<T> {
    pub value: T,
}

// ---------------------------------------------------------------------------
// OpenReview API v1 (ICLR 2020-2023)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ApiResponseV1 {
    pub notes: Vec<NoteV1>,
    pub count: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct NoteV1 {
    #[allow(dead_code)]
    pub id: String,
    pub forum: String,
    pub content: ContentV1,
}

#[derive(Debug, Deserialize)]
pub struct ContentV1 {
    pub title: String,
    pub authors: Option<Vec<String>>,
    #[serde(rename = "abstract")]
    pub r#abstract: Option<String>,
    pub pdf: Option<String>,
    pub venue: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const OPENREVIEW_BASE: &str = "https://openreview.net";
const API_V1_BASE: &str = "https://api.openreview.net";
const API_V2_BASE: &str = "https://api2.openreview.net";
const PAGE_LIMIT: u64 = 1000;

fn api_version(year: u16) -> u8 {
    if year >= 2024 { 2 } else { 1 }
}

fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(abstract_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract category from venue string (e.g. "ICLR 2024 poster" → "poster").
fn extract_category(venue: Option<&str>) -> String {
    match venue {
        Some(v) => {
            let lower = v.to_lowercase();
            // Common venue suffixes: poster, oral, spotlight, workshop, tiny paper
            for keyword in &["oral", "spotlight", "poster", "workshop", "tiny paper"] {
                if lower.contains(keyword) {
                    return keyword.to_string();
                }
            }
            "Conference".to_string()
        }
        None => "Conference".to_string(),
    }
}

fn build_paper_url(forum_id: &str) -> String {
    format!("{}/forum?id={}", OPENREVIEW_BASE, forum_id)
}

fn build_pdf_url(pdf_path: Option<&str>) -> Option<String> {
    pdf_path.map(|p| format!("{}{}", OPENREVIEW_BASE, p))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch all accepted papers for a given ICLR year from the OpenReview API.
pub async fn fetch_papers_for_year(
    client: &reqwest::Client,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    if year < 2020 {
        bail!(
            "ICLR scraping for years before 2020 is not supported (requested {})",
            year
        );
    }

    let version = api_version(year);
    match version {
        2 => fetch_v2(client, year, interval).await,
        _ => fetch_v1(client, year, interval).await,
    }
}

async fn fetch_v2(
    client: &reqwest::Client,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let venue_id = format!("ICLR.cc/{}/Conference", year);
    let mut papers = Vec::new();
    let mut offset: u64 = 0;

    loop {
        let url = format!(
            "{}/notes?content.venueid={}&limit={}&offset={}",
            API_V2_BASE, venue_id, PAGE_LIMIT, offset
        );
        tracing::debug!("GET {}", url);

        let resp = client.get(&url).send().await?.error_for_status()?;
        let body: ApiResponseV2 = resp.json().await?;

        let total = body.count.unwrap_or(0);
        let page_len = body.notes.len() as u64;

        for note in body.notes {
            let title = note.content.title.value;
            let authors = note
                .content
                .authors
                .map(|a| a.value)
                .unwrap_or_default();
            let abstract_text = note
                .content
                .r#abstract
                .map(|a| a.value)
                .unwrap_or_default();
            let pdf_path = note.content.pdf.map(|p| p.value);
            let venue = note.content.venue.map(|v| v.value);

            let category = extract_category(venue.as_deref());

            papers.push(Paper {
                id: compute_id(&title),
                conference: "iclr".to_string(),
                year,
                title: title.clone(),
                authors,
                r#abstract: abstract_text.clone(),
                url: build_paper_url(&note.forum),
                pdf_url: build_pdf_url(pdf_path.as_deref()),
                categories: vec![category],
                hash: compute_hash(&title, &abstract_text),
            });
        }

        offset += page_len;
        if page_len == 0 || offset >= total {
            break;
        }

        tokio::time::sleep(interval).await;
    }

    Ok(papers)
}

async fn fetch_v1(
    client: &reqwest::Client,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let venue_id = format!("ICLR.cc/{}/Conference", year);
    let mut papers = Vec::new();
    let mut offset: u64 = 0;

    loop {
        let url = format!(
            "{}/notes?content.venueid={}&limit={}&offset={}",
            API_V1_BASE, venue_id, PAGE_LIMIT, offset
        );
        tracing::debug!("GET {}", url);

        let resp = client.get(&url).send().await?.error_for_status()?;
        let body: ApiResponseV1 = resp.json().await?;

        let total = body.count.unwrap_or(0);
        let page_len = body.notes.len() as u64;

        for note in body.notes {
            let title = note.content.title;
            let authors = note.content.authors.unwrap_or_default();
            let abstract_text = note.content.r#abstract.unwrap_or_default();
            let pdf_path = note.content.pdf;
            let venue = note.content.venue;

            let category = extract_category(venue.as_deref());

            papers.push(Paper {
                id: compute_id(&title),
                conference: "iclr".to_string(),
                year,
                title: title.clone(),
                authors,
                r#abstract: abstract_text.clone(),
                url: build_paper_url(&note.forum),
                pdf_url: build_pdf_url(pdf_path.as_deref()),
                categories: vec![category],
                hash: compute_hash(&title, &abstract_text),
            });
        }

        offset += page_len;
        if page_len == 0 || offset >= total {
            break;
        }

        tokio::time::sleep(interval).await;
    }

    Ok(papers)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_v2_response() {
        let json = r#"{
            "notes": [{
                "id": "abc123",
                "forum": "abc123",
                "content": {
                    "title": {"value": "A Great Paper"},
                    "authors": {"value": ["Alice", "Bob"]},
                    "abstract": {"value": "This paper does great things."},
                    "pdf": {"value": "/pdf/abc123.pdf"},
                    "venue": {"value": "ICLR 2024 poster"},
                    "venueid": {"value": "ICLR.cc/2024/Conference"}
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV2 = serde_json::from_str(json).unwrap();
        assert_eq!(resp.notes.len(), 1);
        assert_eq!(resp.count, Some(1));

        let note = &resp.notes[0];
        assert_eq!(note.content.title.value, "A Great Paper");
        assert_eq!(
            note.content.authors.as_ref().unwrap().value,
            vec!["Alice".to_string(), "Bob".to_string()]
        );
        assert_eq!(
            note.content.r#abstract.as_ref().unwrap().value,
            "This paper does great things."
        );
        assert_eq!(
            note.content.pdf.as_ref().unwrap().value,
            "/pdf/abc123.pdf"
        );
        assert_eq!(
            note.content.venue.as_ref().unwrap().value,
            "ICLR 2024 poster"
        );
    }

    #[test]
    fn test_parse_v1_response() {
        let json = r#"{
            "notes": [{
                "id": "xyz789",
                "forum": "xyz789",
                "content": {
                    "title": "Another Paper",
                    "authors": ["Charlie", "Diana"],
                    "abstract": "Abstract text here.",
                    "pdf": "/pdf/xyz789.pdf",
                    "venue": "ICLR 2022 Oral"
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV1 = serde_json::from_str(json).unwrap();
        assert_eq!(resp.notes.len(), 1);
        assert_eq!(resp.count, Some(1));

        let note = &resp.notes[0];
        assert_eq!(note.content.title, "Another Paper");
        assert_eq!(
            note.content.authors.as_ref().unwrap(),
            &vec!["Charlie".to_string(), "Diana".to_string()]
        );
        assert_eq!(
            note.content.r#abstract.as_ref().unwrap(),
            "Abstract text here."
        );
        assert_eq!(note.content.pdf.as_ref().unwrap(), "/pdf/xyz789.pdf");
    }

    #[test]
    fn test_parse_v2_missing_optional_fields() {
        let json = r#"{
            "notes": [{
                "id": "minimal",
                "forum": "minimal",
                "content": {
                    "title": {"value": "Minimal Paper"}
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV2 = serde_json::from_str(json).unwrap();
        let note = &resp.notes[0];
        assert_eq!(note.content.title.value, "Minimal Paper");
        assert!(note.content.authors.is_none());
        assert!(note.content.r#abstract.is_none());
        assert!(note.content.pdf.is_none());
        assert!(note.content.venue.is_none());
    }

    #[test]
    fn test_parse_v1_missing_optional_fields() {
        let json = r#"{
            "notes": [{
                "id": "min1",
                "forum": "min1",
                "content": {
                    "title": "Title Only"
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV1 = serde_json::from_str(json).unwrap();
        let note = &resp.notes[0];
        assert_eq!(note.content.title, "Title Only");
        assert!(note.content.authors.is_none());
        assert!(note.content.r#abstract.is_none());
        assert!(note.content.pdf.is_none());
        assert!(note.content.venue.is_none());
    }

    #[test]
    fn test_pagination_count_exceeds_limit() {
        // Verify that count field is parsed so pagination logic can use it
        let json = r#"{
            "notes": [],
            "count": 2500
        }"#;

        let resp: ApiResponseV2 = serde_json::from_str(json).unwrap();
        assert_eq!(resp.count, Some(2500));
        assert!(resp.notes.is_empty());
    }

    #[test]
    fn test_paper_url_construction() {
        assert_eq!(
            build_paper_url("rhgIgTSSxW"),
            "https://openreview.net/forum?id=rhgIgTSSxW"
        );
    }

    #[test]
    fn test_pdf_url_construction() {
        assert_eq!(
            build_pdf_url(Some("/pdf/abc123.pdf")),
            Some("https://openreview.net/pdf/abc123.pdf".to_string())
        );
        assert_eq!(build_pdf_url(None), None);
    }

    #[test]
    fn test_extract_category() {
        assert_eq!(extract_category(Some("ICLR 2024 poster")), "poster");
        assert_eq!(extract_category(Some("ICLR 2024 oral")), "oral");
        assert_eq!(
            extract_category(Some("ICLR 2024 Spotlight")),
            "spotlight"
        );
        assert_eq!(extract_category(Some("ICLR 2024")), "Conference");
        assert_eq!(extract_category(None), "Conference");
    }

    #[test]
    fn test_compute_hash() {
        let h1 = compute_hash("Title A", "Abstract A");
        let h2 = compute_hash("Title A", "Abstract A");
        let h3 = compute_hash("Title B", "Abstract B");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64); // SHA256 hex length
    }

    #[test]
    fn test_api_version_selection() {
        assert_eq!(api_version(2020), 1);
        assert_eq!(api_version(2023), 1);
        assert_eq!(api_version(2024), 2);
        assert_eq!(api_version(2025), 2);
    }
}
