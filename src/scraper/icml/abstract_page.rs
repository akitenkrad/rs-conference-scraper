use anyhow::Result;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{Paper, PaperListEntry};

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

    // Extract title: prefer meta tag, fallback to entry title
    let title = get_meta_content(&document, "citation_title")
        .unwrap_or_else(|| entry.title.clone());

    // Extract authors: prefer meta tags, fallback to entry authors
    let authors = get_all_meta_contents(&document, "citation_author");
    let authors = if authors.is_empty() {
        entry.authors.clone()
    } else {
        authors
    };

    // Extract PDF URL from meta tag
    let pdf_url = get_meta_content(&document, "citation_pdf_url");

    // Extract abstract from div#abstract or div.abstract
    let abstract_text = get_abstract(&document);

    let id = compute_sha256(&title);
    let hash = compute_sha256(&format!("{}{}", title, abstract_text));

    let categories: Vec<String> = entry
        .track
        .clone()
        .into_iter()
        .collect();

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

fn get_abstract(document: &Html) -> String {
    // Try div#abstract first (ICML primary pattern)
    let selectors = ["div#abstract", "div.abstract"];

    for sel_str in &selectors {
        if let Ok(selector) = Selector::parse(sel_str) {
            if let Some(el) = document.select(&selector).next() {
                let text = el
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    return text;
                }
            }
        }
    }

    // Fallback: try meta tag
    if let Some(abstract_text) = get_meta_content(document, "citation_abstract") {
        if !abstract_text.is_empty() {
            return abstract_text;
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_paper_detail_with_div_id_abstract() {
        let html = r#"
        <html><head>
            <meta name="citation_title" content="Scaling Laws for Neural Language Models">
            <meta name="citation_author" content="Jared Kaplan">
            <meta name="citation_author" content="Sam McCandlish">
            <meta name="citation_pdf_url" content="https://raw.githubusercontent.com/mlresearch/v235/main/assets/paper.pdf">
        </head><body>
            <div id="abstract" class="abstract">We study empirical scaling laws for language model performance.</div>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Scaling Laws for Neural Language Models".to_string(),
            authors: vec!["Jared Kaplan".to_string()],
            detail_url: "https://proceedings.mlr.press/v235/kaplan24a.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "icml", 2024).unwrap();
        assert_eq!(paper.title, "Scaling Laws for Neural Language Models");
        assert_eq!(paper.authors, vec!["Jared Kaplan", "Sam McCandlish"]);
        assert_eq!(
            paper.pdf_url,
            Some("https://raw.githubusercontent.com/mlresearch/v235/main/assets/paper.pdf".to_string())
        );
        assert_eq!(
            paper.r#abstract,
            "We study empirical scaling laws for language model performance."
        );
        assert_eq!(paper.conference, "icml");
        assert_eq!(paper.year, 2024);
        assert_eq!(paper.categories, vec!["Conference"]);
        assert!(!paper.id.is_empty());
        assert!(!paper.hash.is_empty());
    }

    #[test]
    fn test_parse_paper_detail_fallback_to_entry() {
        let html = r#"
        <html><body>
            <div class="abstract">Abstract from class-based selector.</div>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Fallback Paper Title".to_string(),
            authors: vec!["Author A".to_string(), "Author B".to_string()],
            detail_url: "https://proceedings.mlr.press/v202/paper23a.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "icml", 2023).unwrap();
        assert_eq!(paper.title, "Fallback Paper Title");
        assert_eq!(paper.authors, vec!["Author A", "Author B"]);
        assert_eq!(paper.pdf_url, None);
        assert_eq!(
            paper.r#abstract,
            "Abstract from class-based selector."
        );
        assert_eq!(paper.year, 2023);
    }

    #[test]
    fn test_parse_paper_detail_no_abstract() {
        let html = r#"
        <html><head>
            <meta name="citation_title" content="Paper Without Abstract">
        </head><body>
            <p>No abstract div here.</p>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Paper Without Abstract".to_string(),
            authors: vec!["Author".to_string()],
            detail_url: "https://proceedings.mlr.press/v119/paper20a.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "icml", 2020).unwrap();
        assert_eq!(paper.title, "Paper Without Abstract");
        assert!(paper.r#abstract.is_empty());
    }

    #[test]
    fn test_compute_sha256() {
        let hash = compute_sha256("test");
        assert_eq!(hash.len(), 64);
    }
}
