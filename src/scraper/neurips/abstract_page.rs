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

    // Try meta tags first (preferred)
    let title = get_meta_content(&document, "citation_title")
        .unwrap_or_else(|| get_text_content(&document, "h4").unwrap_or_else(|| entry.title.clone()));

    let authors = get_all_meta_contents(&document, "citation_author");
    let authors = if authors.is_empty() {
        // Fallback: use authors from the list entry
        entry.authors.clone()
    } else {
        authors
    };

    let pdf_url = get_meta_content(&document, "citation_pdf_url");

    // Get abstract
    let abstract_text = get_abstract(&document);

    let id = compute_sha256(&title);
    let hash = compute_sha256(&format!("{}{}", title, abstract_text));

    // Get track info from detail page, fallback to list entry
    let track = get_track(&document).or_else(|| entry.track.clone());
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

fn get_text_content(document: &Html, selector_str: &str) -> Option<String> {
    let selector = Selector::parse(selector_str).ok()?;
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<Vec<_>>()
        .join(" ");
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn get_abstract(document: &Html) -> String {
    // NeurIPS uses <section class="paper-section"><h2>Abstract</h2><p class="paper-abstract"><p>...</p></p></section>
    // Since <p> cannot nest in HTML, the parser restructures it so the text ends up
    // in a sibling <p>. We find the section containing "Abstract" and extract all
    // paragraph text from it.
    if let Ok(selector) = Selector::parse("section.paper-section") {
        for section in document.select(&selector) {
            let section_text = section.text().collect::<Vec<_>>().join(" ");
            if section_text.contains("Abstract") {
                // Get text from all <p> children (skipping the h2)
                if let Ok(p_sel) = Selector::parse("p") {
                    let paragraphs: Vec<String> = section
                        .select(&p_sel)
                        .map(|p| p.text().collect::<Vec<_>>().join(" ").trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                    if !paragraphs.is_empty() {
                        return paragraphs.join(" ");
                    }
                }
            }
        }
    }

    // Fallback selectors for non-NeurIPS or older pages
    let selectors = [
        "p.paper-abstract",
        "p.abstract",
        "div.abstract",
        "blockquote",
        "div#abstract",
    ];

    for sel_str in &selectors {
        if let Ok(selector) = Selector::parse(sel_str) {
            if let Some(el) = document.select(&selector).next() {
                let text = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
                if !text.is_empty() {
                    return text;
                }
            }
        }
    }

    String::new()
}

/// 個別論文ページからトラック情報を取得
fn get_track(document: &Html) -> Option<String> {
    let selector = Selector::parse("span.paper-track").ok()?;
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_paper_detail_with_meta_tags() {
        let html = r#"
        <html><head>
            <meta name="citation_title" content="A Great Paper">
            <meta name="citation_author" content="Alice Smith">
            <meta name="citation_author" content="Bob Jones">
            <meta name="citation_pdf_url" content="https://example.com/paper.pdf">
        </head><body>
            <h4>A Great Paper</h4>
            <section class="paper-section">
                <h2 class="section-label">Abstract</h2>
                <p class="paper-abstract"><p>This is the abstract text.</p></p>
            </section>
            <span class="paper-track">Main Conference Track</span>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "A Great Paper".to_string(),
            authors: Vec::new(),
            detail_url: "https://papers.neurips.cc/paper_files/paper/2023/hash/abc-Abstract-Conference.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "neurips", 2023).unwrap();
        assert_eq!(paper.title, "A Great Paper");
        assert_eq!(paper.authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(
            paper.pdf_url,
            Some("https://example.com/paper.pdf".to_string())
        );
        assert_eq!(paper.r#abstract, "This is the abstract text.");
        assert_eq!(paper.conference, "neurips");
        assert_eq!(paper.year, 2023);
        assert_eq!(paper.categories, vec!["Main Conference Track"]);
    }

    #[test]
    fn test_parse_paper_detail_fallback() {
        let html = r#"
        <html><body>
            <h4>Fallback Title</h4>
            <blockquote>Abstract from blockquote.</blockquote>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Original Title".to_string(),
            authors: vec!["Author One".to_string()],
            detail_url: "https://example.com/paper".to_string(),
            track: None,
        };

        let paper = parse_paper_detail(html, &entry, "neurips", 2022).unwrap();
        assert_eq!(paper.title, "Fallback Title");
        assert_eq!(paper.authors, vec!["Author One"]);
        assert_eq!(paper.r#abstract, "Abstract from blockquote.");
    }

    #[test]
    fn test_compute_sha256() {
        let hash = compute_sha256("test");
        assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
    }

    /// Older NeurIPS papers (1987-2021) use the same meta tag structure
    /// but lack the `paper-track` span. The parser should fall back to the
    /// track from the PaperListEntry.
    #[test]
    fn test_parse_paper_detail_old_year_no_track() {
        let html = r#"
        <html><head>
            <meta name="citation_title" content="Synchronization in Neural Nets">
            <meta name="citation_author" content="Vidal, Jacques">
            <meta name="citation_author" content="Haggerty, John">
            <meta name="citation_pdf_url" content="https://proceedings.neurips.cc/paper_files/paper/1987/file/03004620ea802b9118dd44d69f07af56-Paper.pdf">
        </head><body>
            <h4>Synchronization in Neural Nets</h4>
            <section class="paper-section">
                <h2 class="section-label">Abstract</h2>
                <p class="paper-abstract"><p>The paper presents an artificial neural network concept.</p></p>
            </section>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Synchronization in Neural Nets".to_string(),
            authors: Vec::new(),
            detail_url: "https://papers.neurips.cc/paper_files/paper/1987/hash/03004620ea802b9118dd44d69f07af56-Abstract.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "neurips", 1987).unwrap();
        assert_eq!(paper.title, "Synchronization in Neural Nets");
        assert_eq!(paper.authors, vec!["Vidal, Jacques", "Haggerty, John"]);
        assert_eq!(paper.year, 1987);
        assert_eq!(paper.conference, "neurips");
        assert_eq!(paper.categories, vec!["Conference"]);
        assert!(!paper.r#abstract.is_empty());
        assert!(paper.pdf_url.is_some());
    }

    /// Test that a paper from year 2000 with standard structure parses correctly.
    #[test]
    fn test_parse_paper_detail_year_2000() {
        let html = r#"
        <html><head>
            <meta name="citation_title" content="Reinforcement Learning with Function Approximation Converges to a Region">
            <meta name="citation_author" content="Gordon, Geoffrey J.">
            <meta name="citation_pdf_url" content="https://proceedings.neurips.cc/paper_files/paper/2000/file/04df4d434d481c5bb723be1b6df1ee65-Paper.pdf">
        </head><body>
            <h4>Reinforcement Learning with Function Approximation Converges to a Region</h4>
            <section class="paper-section">
                <h2 class="section-label">Abstract</h2>
                <p class="paper-abstract"><p>Many algorithms for approximate reinforcement learning are not known to converge.</p></p>
            </section>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Reinforcement Learning with Function Approximation Converges to a Region".to_string(),
            authors: Vec::new(),
            detail_url: "https://papers.neurips.cc/paper_files/paper/2000/hash/04df4d434d481c5bb723be1b6df1ee65-Abstract.html".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "neurips", 2000).unwrap();
        assert_eq!(paper.title, "Reinforcement Learning with Function Approximation Converges to a Region");
        assert_eq!(paper.authors, vec!["Gordon, Geoffrey J."]);
        assert_eq!(paper.year, 2000);
        assert_eq!(paper.categories, vec!["Conference"]);
        assert!(paper.r#abstract.contains("reinforcement learning"));
    }
}
