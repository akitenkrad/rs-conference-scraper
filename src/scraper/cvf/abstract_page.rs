use anyhow::Result;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{compute_id, Paper, PaperListEntry};

/// 個別論文ページを取得してパースする
pub async fn fetch_paper_detail(
    client: &reqwest::Client,
    entry: &PaperListEntry,
    conference: &str,
    year: u16,
    interval: Duration,
) -> Result<Paper> {
    let html = fetch_with_sleep(client, &entry.detail_url, interval).await?;
    parse_paper_detail(&html, entry, conference, year)
}

fn compute_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn parse_paper_detail(
    html: &str,
    entry: &PaperListEntry,
    conference: &str,
    year: u16,
) -> Result<Paper> {
    let document = Html::parse_document(html);

    // Title: div#papertitle, fallback to meta tag, then entry title
    let title = get_div_text(&document, "div#papertitle")
        .or_else(|| get_meta_content(&document, "citation_title"))
        .unwrap_or_else(|| entry.title.clone());

    // Authors: div#authors > b > i, fallback to meta tags, then entry authors
    let authors = get_authors_from_div(&document)
        .or_else(|| {
            let meta_authors = get_all_meta_contents(&document, "citation_author");
            if meta_authors.is_empty() {
                None
            } else {
                Some(meta_authors)
            }
        })
        .unwrap_or_else(|| entry.authors.clone());

    // Abstract: div#abstract
    let abstract_text = get_div_text(&document, "div#abstract").unwrap_or_default();

    // PDF URL from meta tag
    let pdf_url = get_meta_content(&document, "citation_pdf_url");

    let id = compute_id(&title);
    let hash = compute_sha256(&format!("{}{}", title, abstract_text));

    let track = entry.track.clone();
    let categories: Vec<String> = track.into_iter().collect();

    Ok(Paper {
        id,
        conference: conference.to_string(),
        year,
        title,
        authors,
        r#abstract: abstract_text,
        url: entry.detail_url.clone(),
        pdf_url,
        categories,
        hash,
    })
}

/// div要素のテキストを取得
fn get_div_text(document: &Html, selector_str: &str) -> Option<String> {
    let selector = Selector::parse(selector_str).ok()?;
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// div#authors 内の <b><i> からカンマ区切りの著者リストを取得
fn get_authors_from_div(document: &Html) -> Option<Vec<String>> {
    let selector = Selector::parse("div#authors b i").ok()?;
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<Vec<_>>()
        .join(" ");
    let authors: Vec<String> = text
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if authors.is_empty() {
        None
    } else {
        Some(authors)
    }
}

fn get_meta_content(document: &Html, name: &str) -> Option<String> {
    let selector = Selector::parse(&format!("meta[name='{}']", name)).ok()?;
    document
        .select(&selector)
        .next()?
        .value()
        .attr("content")
        .map(|s| s.trim().to_string())
}

fn get_all_meta_contents(document: &Html, name: &str) -> Vec<String> {
    let selector = match Selector::parse(&format!("meta[name='{}']", name)) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    document
        .select(&selector)
        .filter_map(|el| el.value().attr("content").map(|s| s.trim().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_detail_page() {
        let html = r#"
        <html><head>
            <meta name="citation_title" content="Deep Learning for Vision">
            <meta name="citation_author" content="John Smith">
            <meta name="citation_author" content="Jane Doe">
            <meta name="citation_pdf_url" content="https://openaccess.thecvf.com/content/CVPR2024/papers/Smith_Deep_Learning_CVPR_2024_paper.pdf">
        </head><body>
            <div id="papertitle">Deep Learning for Vision</div>
            <div id="authors">
                <b><i>John Smith, Jane Doe</i></b>
            </div>
            <div id="abstract">
                We present a novel approach to deep learning for computer vision tasks.
                Our method achieves state-of-the-art results on multiple benchmarks.
            </div>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Deep Learning for Vision".to_string(),
            authors: vec!["John Smith".to_string(), "Jane Doe".to_string()],
            detail_url: "https://openaccess.thecvf.com/content/CVPR2024/html/Smith_Deep_Learning_CVPR_2024_paper.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "cvpr", 2024).unwrap();
        assert_eq!(paper.title, "Deep Learning for Vision");
        assert_eq!(paper.authors, vec!["John Smith", "Jane Doe"]);
        assert_eq!(paper.conference, "cvpr");
        assert_eq!(paper.year, 2024);
        assert!(paper.r#abstract.contains("novel approach"));
        assert!(paper.pdf_url.is_some());
        assert_eq!(paper.categories, vec!["Conference"]);
        assert!(!paper.id.is_empty());
        assert!(!paper.hash.is_empty());
    }

    #[test]
    fn test_parse_detail_page_missing_abstract() {
        let html = r#"
        <html><head>
            <meta name="citation_title" content="Some Paper Title">
        </head><body>
            <div id="papertitle">Some Paper Title</div>
            <div id="authors">
                <b><i>Author One, Author Two</i></b>
            </div>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Some Paper Title".to_string(),
            authors: vec!["Author One".to_string()],
            detail_url: "https://openaccess.thecvf.com/content/ICCV2023/html/Some_Paper_ICCV_2023_paper.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "iccv", 2023).unwrap();
        assert_eq!(paper.title, "Some Paper Title");
        assert_eq!(paper.authors, vec!["Author One", "Author Two"]);
        assert_eq!(paper.r#abstract, "");
        assert_eq!(paper.conference, "iccv");
        assert_eq!(paper.year, 2023);
    }

    #[test]
    fn test_fallback_to_meta_authors() {
        let html = r#"
        <html><head>
            <meta name="citation_title" content="Meta Only Paper">
            <meta name="citation_author" content="Meta Author One">
            <meta name="citation_author" content="Meta Author Two">
        </head><body>
            <div id="papertitle">Meta Only Paper</div>
            <div id="abstract">Abstract text here.</div>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Meta Only Paper".to_string(),
            authors: vec!["Fallback Author".to_string()],
            detail_url: "https://openaccess.thecvf.com/content/CVPR2023/html/paper.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "cvpr", 2023).unwrap();
        assert_eq!(paper.authors, vec!["Meta Author One", "Meta Author Two"]);
    }

    #[test]
    fn test_fallback_to_entry_data() {
        let html = r#"
        <html><body>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Entry Title".to_string(),
            authors: vec!["Entry Author".to_string()],
            detail_url: "https://openaccess.thecvf.com/content/CVPR2024/html/paper.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "cvpr", 2024).unwrap();
        assert_eq!(paper.title, "Entry Title");
        assert_eq!(paper.authors, vec!["Entry Author"]);
        assert_eq!(paper.r#abstract, "");
    }
}
