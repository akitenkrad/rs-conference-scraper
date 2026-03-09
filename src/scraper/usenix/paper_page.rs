use anyhow::Result;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{compute_id, Paper, PaperListEntry};

pub async fn fetch_paper_detail(
    client: &reqwest::Client,
    entry: &PaperListEntry,
    year: u16,
    interval: Duration,
) -> Result<Paper> {
    let html = fetch_with_sleep(client, &entry.detail_url, interval).await?;
    parse_paper_detail(&html, entry, year)
}

fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}{}", title, abstract_text).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn parse_paper_detail(html: &str, entry: &PaperListEntry, year: u16) -> Result<Paper> {
    let document = Html::parse_document(html);

    // Title: try article heading, then h1, fallback to entry title
    let title = get_text_content(&document, "article h2.node-title")
        .or_else(|| get_text_content(&document, "h1.page-title"))
        .or_else(|| get_text_content(&document, "h1"))
        .unwrap_or_else(|| entry.title.clone());

    // Authors: look for author field markup
    let authors = extract_authors(&document);
    let authors = if authors.is_empty() {
        entry.authors.clone()
    } else {
        authors
    };

    // Abstract
    let abstract_text = extract_abstract(&document);

    // PDF URL
    let pdf_url = extract_pdf_url(&document);

    let id = compute_id(&title);
    let hash = compute_hash(&title, &abstract_text);

    let track = entry.track.clone();
    let categories: Vec<String> = track.into_iter().collect();

    Ok(Paper {
        id,
        conference: "usenix-security".to_string(),
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

fn get_text_content(document: &Html, selector_str: &str) -> Option<String> {
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

fn extract_authors(document: &Html) -> Vec<String> {
    // USENIX pages typically have authors in a field with class "field-name-field-paper-people-text"
    // or similar markup with author names as links or plain text
    let selectors = [
        "div.field-name-field-paper-people-text a",
        "div.field-name-field-paper-people-text",
        "div.field-paper-people a",
        "span.field-paper-people a",
        "div.authors a",
        "span.author",
    ];

    for sel_str in &selectors {
        if let Ok(selector) = Selector::parse(sel_str) {
            let authors: Vec<String> = document
                .select(&selector)
                .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            if !authors.is_empty() {
                // If we matched the container div (not links), split by common separators
                if sel_str.ends_with("-text") && authors.len() == 1 {
                    return split_author_string(&authors[0]);
                }
                return authors;
            }
        }
    }

    Vec::new()
}

fn split_author_string(text: &str) -> Vec<String> {
    // Authors might be separated by commas, semicolons, or " and "
    let text = text.replace(" and ", ", ");
    text.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn extract_abstract(document: &Html) -> String {
    let selectors = [
        "div.field-name-field-paper-description p",
        "div.field-name-field-paper-description",
        "div.field-paper-description p",
        "div.field-paper-description",
        "div.paper-abstract p",
        "div.paper-abstract",
        "div#abstract p",
        "div#abstract",
        "p.abstract",
    ];

    for sel_str in &selectors {
        if let Ok(selector) = Selector::parse(sel_str) {
            let texts: Vec<String> = document
                .select(&selector)
                .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            if !texts.is_empty() {
                return texts.join(" ");
            }
        }
    }

    String::new()
}

fn extract_pdf_url(document: &Html) -> Option<String> {
    // Look for links ending in .pdf
    if let Ok(selector) = Selector::parse("a[href$='.pdf']") {
        if let Some(el) = document.select(&selector).next() {
            return el.value().attr("href").map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(title: &str, url: &str) -> PaperListEntry {
        PaperListEntry {
            title: title.to_string(),
            authors: Vec::new(),
            detail_url: url.to_string(),
            track: Some("Conference".to_string()),
        }
    }

    #[test]
    fn test_parse_paper_detail_full() {
        let html = r#"
        <html><body>
        <article>
            <h2 class="node-title">Breaking TLS with Quantum Attacks</h2>
        </article>
        <div class="field-name-field-paper-people-text">
            <a href="/user/alice">Alice Smith</a>,
            <a href="/user/bob">Bob Jones</a>
        </div>
        <div class="field-name-field-paper-description">
            <p>We present a novel quantum attack on TLS 1.3.</p>
        </div>
        <a href="https://www.usenix.org/system/files/paper.pdf">PDF</a>
        </body></html>
        "#;

        let entry = make_entry(
            "Breaking TLS with Quantum Attacks",
            "https://www.usenix.org/conference/usenixsecurity24/presentation/smith",
        );
        let paper = parse_paper_detail(html, &entry, 2024).unwrap();

        assert_eq!(paper.title, "Breaking TLS with Quantum Attacks");
        assert_eq!(paper.authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(
            paper.r#abstract,
            "We present a novel quantum attack on TLS 1.3."
        );
        assert_eq!(
            paper.pdf_url,
            Some("https://www.usenix.org/system/files/paper.pdf".to_string())
        );
        assert_eq!(paper.conference, "usenix-security");
        assert_eq!(paper.year, 2024);
        assert_eq!(paper.categories, vec!["Conference"]);
        assert!(!paper.id.is_empty());
        assert!(!paper.hash.is_empty());
    }

    #[test]
    fn test_parse_paper_detail_fallback_title() {
        let html = r#"
        <html><body>
        <h1 class="page-title">Fallback Title Here</h1>
        <div class="field-name-field-paper-description">
            <p>Some abstract text.</p>
        </div>
        </body></html>
        "#;

        let entry = make_entry(
            "Entry Title",
            "https://www.usenix.org/conference/usenixsecurity23/presentation/test",
        );
        let paper = parse_paper_detail(html, &entry, 2023).unwrap();
        assert_eq!(paper.title, "Fallback Title Here");
    }

    #[test]
    fn test_parse_paper_detail_entry_fallback() {
        let html = r#"
        <html><body>
        <p>Minimal page with no structure.</p>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "My Paper Title".to_string(),
            authors: vec!["Author One".to_string()],
            detail_url: "https://www.usenix.org/conference/usenixsecurity22/presentation/x"
                .to_string(),
            track: Some("Conference".to_string()),
        };
        let paper = parse_paper_detail(html, &entry, 2022).unwrap();
        assert_eq!(paper.title, "My Paper Title");
        assert_eq!(paper.authors, vec!["Author One"]);
        assert_eq!(paper.r#abstract, "");
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let h1 = compute_hash("title", "abstract");
        let h2 = compute_hash("title", "abstract");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_split_author_string() {
        let authors = split_author_string("Alice Smith, Bob Jones and Carol White");
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones", "Carol White"]);
    }

    #[test]
    fn test_extract_pdf_url() {
        let html = r#"
        <html><body>
        <a href="https://www.usenix.org/system/files/sec24-paper.pdf">Download PDF</a>
        <a href="/program">Program</a>
        </body></html>
        "#;
        let document = Html::parse_document(html);
        let url = extract_pdf_url(&document);
        assert_eq!(
            url,
            Some("https://www.usenix.org/system/files/sec24-paper.pdf".to_string())
        );
    }
}
