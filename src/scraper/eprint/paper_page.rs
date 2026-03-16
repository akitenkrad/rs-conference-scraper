use anyhow::Result;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{compute_id, Paper, PaperListEntry};

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

fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}{}", title, abstract_text).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn parse_paper_detail(
    html: &str,
    entry: &PaperListEntry,
    conference: &str,
    year: u16,
) -> Result<Paper> {
    let document = Html::parse_document(html);

    // Title: from <h3> or <h2> or <h1>, fallback to entry title
    let title = get_title(&document).unwrap_or_else(|| entry.title.clone());

    // Authors: from <span class="authorName"> or similar
    let authors = get_authors(&document).unwrap_or_else(|| entry.authors.clone());

    // Abstract: from <div class="paper-abstract"> or <p class="abstract">
    let abstract_text = get_abstract(&document);

    // Category: from category links
    let categories = get_categories(&document);

    // PDF URL: /{YEAR}/{NUMBER}.pdf
    let pdf_url = get_pdf_url(&document, &entry.detail_url);

    let id = compute_id(&title);
    let hash = compute_hash(&title, &abstract_text);

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

fn get_title(document: &Html) -> Option<String> {
    // Try h3 first (ePrint paper pages use h3 for the title)
    for sel_str in &["h3", "h2.paper-title", "h1"] {
        if let Ok(selector) = Selector::parse(sel_str)
            && let Some(el) = document.select(&selector).next() {
                let text = el
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
    }
    None
}

fn get_authors(document: &Html) -> Option<Vec<String>> {
    // Try <span class="authorName"> first (ePrint structure)
    if let Ok(selector) = Selector::parse("span.authorName") {
        let authors: Vec<String> = document
            .select(&selector)
            .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        if !authors.is_empty() {
            return Some(authors);
        }
    }

    // Fallback: try <p class="authors"> or <div class="authors">
    if let Ok(selector) = Selector::parse(".authors, p.authors, div.authors")
        && let Some(el) = document.select(&selector).next() {
            let raw = el
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            if !raw.is_empty() {
                let authors: Vec<String> = raw
                    .split(',')
                    .flat_map(|part| part.split(" and "))
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .collect();
                if !authors.is_empty() {
                    return Some(authors);
                }
            }
        }

    None
}

fn get_abstract(document: &Html) -> String {
    // Primary: <div class="paper-abstract">
    let selectors = [
        "div.paper-abstract",
        "p.paper-abstract",
        "div#abstract",
        "div.abstract",
        "p.abstract",
    ];

    for sel_str in &selectors {
        if let Ok(selector) = Selector::parse(sel_str)
            && let Some(el) = document.select(&selector).next() {
                let text = el
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string();
                // Remove leading "Abstract" or "Abstract:" prefix
                let cleaned = text
                    .strip_prefix("Abstract")
                    .unwrap_or(&text)
                    .trim_start_matches(':')
                    .trim_start_matches('.')
                    .trim()
                    .to_string();
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
    }

    String::new()
}

fn get_categories(document: &Html) -> Vec<String> {
    // ePrint categories are linked as <a href="/search?category=...">
    if let Ok(selector) = Selector::parse("a[href*='search?category=']") {
        let cats: Vec<String> = document
            .select(&selector)
            .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .filter(|cat| !cat.is_empty())
            .collect();
        if !cats.is_empty() {
            return cats;
        }
    }

    // Fallback: look for category in metadata or other elements
    if let Ok(selector) = Selector::parse(".category, span.category") {
        let cats: Vec<String> = document
            .select(&selector)
            .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .filter(|cat| !cat.is_empty())
            .collect();
        if !cats.is_empty() {
            return cats;
        }
    }

    Vec::new()
}

fn get_pdf_url(document: &Html, detail_url: &str) -> Option<String> {
    // Try to find a direct PDF link first
    if let Ok(selector) = Selector::parse("a[href$='.pdf']")
        && let Some(el) = document.select(&selector).next()
            && let Some(href) = el.value().attr("href") {
                if href.starts_with("http") {
                    return Some(href.to_string());
                } else {
                    // Make absolute
                    let base = "https://eprint.iacr.org";
                    let path = if href.starts_with('/') {
                        href.to_string()
                    } else {
                        format!("/{}", href)
                    };
                    return Some(format!("{}{}", base, path));
                }
            }

    // Construct PDF URL from detail URL: /2024/001 -> /2024/001.pdf
    let pdf = format!("{}.pdf", detail_url.trim_end_matches('/'));
    Some(pdf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_paper_detail_full() {
        let html = r#"
        <html><body>
        <h3>A Novel Post-Quantum Signature Scheme</h3>
        <div class="authors">
            <span class="authorName">Alice Smith</span>,
            <span class="authorName">Bob Jones</span>
        </div>
        <div class="paper-abstract">
            <p>Abstract: We present a novel post-quantum signature scheme based on lattice problems.
            Our construction achieves optimal parameters while maintaining strong security guarantees
            against quantum adversaries.</p>
        </div>
        <a href="/search?category=public-key cryptography">public-key cryptography</a>
        <a href="/2024/001.pdf">PDF</a>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "A Novel Post-Quantum Signature Scheme".to_string(),
            authors: Vec::new(),
            detail_url: "https://eprint.iacr.org/2024/001".to_string(),
            track: None,
        };

        let paper = parse_paper_detail(html, &entry, "eprint", 2024).unwrap();
        assert_eq!(paper.title, "A Novel Post-Quantum Signature Scheme");
        assert_eq!(paper.authors, vec!["Alice Smith", "Bob Jones"]);
        assert!(paper.r#abstract.contains("post-quantum signature scheme"));
        assert!(!paper.r#abstract.starts_with("Abstract"));
        assert_eq!(paper.categories, vec!["public-key cryptography"]);
        assert_eq!(
            paper.pdf_url,
            Some("https://eprint.iacr.org/2024/001.pdf".to_string())
        );
        assert_eq!(paper.conference, "eprint");
        assert_eq!(paper.year, 2024);
    }

    #[test]
    fn test_parse_paper_detail_minimal() {
        let html = r#"
        <html><body>
        <h3>Minimal Paper</h3>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Minimal Paper".to_string(),
            authors: vec!["Fallback Author".to_string()],
            detail_url: "https://eprint.iacr.org/2023/500".to_string(),
            track: None,
        };

        let paper = parse_paper_detail(html, &entry, "eprint", 2023).unwrap();
        assert_eq!(paper.title, "Minimal Paper");
        assert_eq!(paper.authors, vec!["Fallback Author"]);
        assert!(paper.r#abstract.is_empty());
        assert!(paper.categories.is_empty());
        // Should construct PDF URL from detail URL
        assert_eq!(
            paper.pdf_url,
            Some("https://eprint.iacr.org/2023/500.pdf".to_string())
        );
    }

    #[test]
    fn test_parse_paper_detail_fallback_title() {
        let html = r#"<html><body><p>No title element</p></body></html>"#;

        let entry = PaperListEntry {
            title: "Entry Title".to_string(),
            authors: Vec::new(),
            detail_url: "https://eprint.iacr.org/2024/100".to_string(),
            track: None,
        };

        let paper = parse_paper_detail(html, &entry, "eprint", 2024).unwrap();
        assert_eq!(paper.title, "Entry Title");
    }

    #[test]
    fn test_parse_paper_detail_with_abstract_prefix() {
        let html = r#"
        <html><body>
        <h3>Some Paper</h3>
        <div class="paper-abstract">Abstract: This is the abstract text.</div>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Some Paper".to_string(),
            authors: Vec::new(),
            detail_url: "https://eprint.iacr.org/2024/200".to_string(),
            track: None,
        };

        let paper = parse_paper_detail(html, &entry, "eprint", 2024).unwrap();
        assert_eq!(paper.r#abstract, "This is the abstract text.");
    }

    #[test]
    fn test_compute_hash() {
        let hash = compute_hash("title", "abstract");
        assert_eq!(hash.len(), 64);
        // Same inputs should give same hash
        assert_eq!(hash, compute_hash("title", "abstract"));
        // Different inputs should give different hash
        assert_ne!(hash, compute_hash("title2", "abstract"));
    }

    #[test]
    fn test_get_authors_comma_separated() {
        let html = r#"
        <html><body>
        <div class="authors">Alice Smith, Bob Jones, Charlie Brown</div>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let authors = get_authors(&document).unwrap();
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones", "Charlie Brown"]);
    }

    #[test]
    fn test_get_categories() {
        let html = r#"
        <html><body>
        <a href="/search?category=public-key cryptography">public-key cryptography</a>
        <a href="/search?category=hash functions">hash functions</a>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let categories = get_categories(&document);
        assert_eq!(categories, vec!["public-key cryptography", "hash functions"]);
    }

    #[test]
    fn test_get_pdf_url_from_link() {
        let html = r#"
        <html><body>
        <a href="/2024/001.pdf">Download PDF</a>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let url = get_pdf_url(&document, "https://eprint.iacr.org/2024/001");
        assert_eq!(
            url,
            Some("https://eprint.iacr.org/2024/001.pdf".to_string())
        );
    }
}
