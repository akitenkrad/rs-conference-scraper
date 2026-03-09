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

    // Title: from <h1> on the page, fallback to entry title
    let title = get_text_content(&document, "h1")
        .unwrap_or_else(|| entry.title.clone());

    // Authors: from paper-data div (first <p> with <strong>), fallback to entry authors
    let authors = get_authors(&document).unwrap_or_else(|| entry.authors.clone());

    // Abstract: from paper-data div, second paragraph content
    let abstract_text = get_abstract(&document);

    // PDF URL: from .pdf-button link
    let pdf_url = get_pdf_url(&document);

    let id = compute_sha256(&title);
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

fn get_authors(document: &Html) -> Option<Vec<String>> {
    // NDSS paper page structure (raw HTML):
    //   <div class="paper-data">
    //     <p><strong><p>Author1 (Affil), Author2 (Affil)</p></strong></p>
    //     <p><p>Abstract text...</p></p>
    //   </div>
    //
    // Due to HTML spec, <p> cannot nest. The parser restructures the DOM so
    // the author text ends up in a standalone <p> sibling of <strong>.
    // We identify the author paragraph by looking for text containing
    // parenthesized affiliations (e.g., "(University)").
    let selector = Selector::parse("div.paper-data").ok()?;
    let paper_data = document.select(&selector).next()?;

    let p_selector = Selector::parse("p").ok()?;

    // Scan all <p> elements in paper-data and find one that looks like an author list
    // (contains parenthesized affiliations and is not too long to be an abstract)
    for p_el in paper_data.select(&p_selector) {
        let raw = p_el
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
        if raw.is_empty() {
            continue;
        }

        // Author lines contain parenthesized affiliations
        if raw.contains('(') && raw.contains(')') {
            let authors = parse_author_string(&raw);
            if !authors.is_empty() {
                return Some(authors);
            }
        }
    }

    None
}

fn get_abstract(document: &Html) -> String {
    // The abstract is in <div class="paper-data">, typically the second (or last)
    // paragraph that is not inside <strong> and is longer than a threshold.
    if let Ok(selector) = Selector::parse("div.paper-data") {
        if let Some(paper_data) = document.select(&selector).next() {
            if let Ok(p_sel) = Selector::parse("p") {
                let paragraphs: Vec<String> = paper_data
                    .select(&p_sel)
                    .map(|p| p.text().collect::<Vec<_>>().join(" ").trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();

                // The abstract is typically the longest paragraph
                // Skip short entries that are likely author lines
                for p in paragraphs.iter().rev() {
                    // Abstract paragraphs are usually > 100 chars
                    if p.len() > 100 {
                        return p.clone();
                    }
                }

                // If all paragraphs are short, try the last one that isn't an author line
                if paragraphs.len() > 1 {
                    return paragraphs.last().unwrap_or(&String::new()).clone();
                }
            }
        }
    }

    // Fallback: look for common abstract selectors
    let selectors = [
        "div.abstract",
        "p.abstract",
        "div#abstract",
        "blockquote",
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

fn get_pdf_url(document: &Html) -> Option<String> {
    // Look for .pdf-button link first
    if let Ok(selector) = Selector::parse("a.pdf-button") {
        if let Some(el) = document.select(&selector).next() {
            if let Some(href) = el.value().attr("href") {
                return Some(href.to_string());
            }
        }
    }

    // Fallback: any link to a .pdf file in wp-content/uploads
    if let Ok(selector) = Selector::parse("a[href*='wp-content/uploads']") {
        for el in document.select(&selector) {
            if let Some(href) = el.value().attr("href") {
                if href.ends_with(".pdf") && !href.contains("slides") {
                    return Some(href.to_string());
                }
            }
        }
    }

    None
}

/// "Author1 (Affil), Author2 (Affil)" 形式の文字列から著者名を抽出
fn parse_author_string(raw: &str) -> Vec<String> {
    let mut authors = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();

    for ch in raw.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
                if depth == 0 {
                    let name = extract_name(&current);
                    if !name.is_empty() {
                        authors.push(name);
                    }
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    // Handle remaining text (author without affiliation)
    let remaining = current.trim().trim_matches(',').trim();
    if !remaining.is_empty() {
        for part in remaining.split(',') {
            let name = part.trim().trim_start_matches("and ").trim();
            if !name.is_empty() {
                authors.push(name.to_string());
            }
        }
    }

    authors
}

fn extract_name(entry: &str) -> String {
    if let Some(paren_pos) = entry.find('(') {
        entry[..paren_pos]
            .trim()
            .trim_matches(',')
            .trim()
            .trim_start_matches("and ")
            .trim()
            .to_string()
    } else {
        entry.trim().trim_matches(',').trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_paper_detail_full() {
        let html = r#"
        <html><body>
        <h1>50 Shades of Support: A Device-Centric Analysis</h1>
        <div class="paper-data">
            <p><strong>
            <p>Abbas Acar (Florida International University), Bob Jones (Stanford)</p>
            </strong></p>
            <p><p>Android is by far the most popular OS with over three billion active mobile devices. As in any software, uncovering vulnerabilities on Android devices and applying timely patches are both critical. This paper presents a comprehensive analysis of Android security update behavior across different device manufacturers and regions worldwide.</p></p>
        </div>
        <div class="paper-buttons">
            <a role="button" class="btn btn-light btn-sm pdf-button" target="_blank"
               href="https://www.ndss-symposium.org/wp-content/uploads/2024-175-paper.pdf">Paper</a>
        </div>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "50 Shades of Support".to_string(),
            authors: Vec::new(),
            detail_url: "https://www.ndss-symposium.org/ndss-paper/50-shades/".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "ndss", 2024).unwrap();
        assert_eq!(
            paper.title,
            "50 Shades of Support: A Device-Centric Analysis"
        );
        assert_eq!(paper.authors, vec!["Abbas Acar", "Bob Jones"]);
        assert!(paper.r#abstract.contains("Android"));
        assert!(paper.r#abstract.len() > 100);
        assert_eq!(
            paper.pdf_url,
            Some(
                "https://www.ndss-symposium.org/wp-content/uploads/2024-175-paper.pdf".to_string()
            )
        );
        assert_eq!(paper.conference, "ndss");
        assert_eq!(paper.year, 2024);
        assert_eq!(paper.categories, vec!["Conference"]);
    }

    #[test]
    fn test_parse_paper_detail_no_pdf() {
        let html = r#"
        <html><body>
        <h1>A Paper Without PDF</h1>
        <div class="paper-data">
            <p><strong>
            <p>Author One (University A)</p>
            </strong></p>
            <p><p>This is the abstract of the paper. It should be long enough to be recognized as an abstract by the parser, which requires more than one hundred characters of content in the paragraph text.</p></p>
        </div>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "A Paper Without PDF".to_string(),
            authors: vec!["Author One".to_string()],
            detail_url: "https://www.ndss-symposium.org/ndss-paper/no-pdf/".to_string(),
            track: Some("Conference".to_string()),
        };

        let paper = parse_paper_detail(html, &entry, "ndss", 2023).unwrap();
        assert_eq!(paper.title, "A Paper Without PDF");
        assert_eq!(paper.authors, vec!["Author One"]);
        assert!(paper.r#abstract.contains("abstract of the paper"));
        assert!(paper.pdf_url.is_none());
    }

    #[test]
    fn test_parse_paper_detail_fallback_authors() {
        let html = r#"
        <html><body>
        <h1>Minimal Paper</h1>
        <div class="paper-data">
            <p><p>Short abstract that is over one hundred characters long to be detected by the parser as an abstract paragraph rather than an author line.</p></p>
        </div>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Minimal Paper".to_string(),
            authors: vec!["Fallback Author".to_string()],
            detail_url: "https://www.ndss-symposium.org/ndss-paper/minimal/".to_string(),
            track: None,
        };

        let paper = parse_paper_detail(html, &entry, "ndss", 2022).unwrap();
        assert_eq!(paper.title, "Minimal Paper");
        assert_eq!(paper.authors, vec!["Fallback Author"]);
        assert!(paper.categories.is_empty());
    }

    #[test]
    fn test_compute_sha256() {
        let hash = compute_sha256("test");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_get_pdf_url_skips_slides() {
        let html = r#"
        <html><body>
        <div class="paper-buttons">
            <a href="https://example.com/wp-content/uploads/slides.pdf">Slides</a>
            <a href="https://example.com/wp-content/uploads/2024-paper.pdf">Paper</a>
        </div>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let url = get_pdf_url(&document);
        assert_eq!(
            url,
            Some("https://example.com/wp-content/uploads/2024-paper.pdf".to_string())
        );
    }
}
