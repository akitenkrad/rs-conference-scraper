use anyhow::Result;
use scraper::{Html, Selector};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::PaperListEntry;

pub async fn fetch_paper_list(
    client: &reqwest::Client,
    base_url: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<PaperListEntry>> {
    let url = format!("{}/ndss{}/accepted-papers/", base_url, year);
    let html = fetch_with_sleep(client, &url, interval).await?;
    parse_paper_list(&html, base_url)
}

fn parse_paper_list(html: &str, base_url: &str) -> Result<Vec<PaperListEntry>> {
    let document = Html::parse_document(html);

    // NDSS listing uses: <div class="pt-cv-content-item ...">
    //   <h2 class="pt-cv-title"><a href="...">Title</a></h2>
    //   <div class="pt-cv-ctf-list">
    //     <div class="pt-cv-custom-fields pt-cv-ctf-display_authors">
    //       <div class="pt-cv-ctf-value"><p>Author1, Author2, ...</p></div>
    //     </div>
    //   </div>
    // </div>

    let item_selector = Selector::parse(".pt-cv-content-item").unwrap();
    let title_selector = Selector::parse("h2.pt-cv-title a").unwrap();
    let authors_selector = Selector::parse(".pt-cv-ctf-display_authors .pt-cv-ctf-value").unwrap();

    let mut entries = Vec::new();

    for item in document.select(&item_selector) {
        let title_el = match item.select(&title_selector).next() {
            Some(el) => el,
            None => continue,
        };

        let title = title_el
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }

        let href = match title_el.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        // Only include links to /ndss-paper/ pages
        if !href.contains("/ndss-paper/") {
            continue;
        }

        let detail_url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("{}{}", base_url, href)
        };

        // Extract authors
        let authors = if let Some(authors_el) = item.select(&authors_selector).next() {
            let raw = authors_el
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            parse_authors(&raw)
        } else {
            Vec::new()
        };

        entries.push(PaperListEntry {
            title,
            authors,
            detail_url,
            track: Some("Conference".to_string()),
        });
    }

    // Fallback: if pt-cv-content-item structure is not found, try simpler h2 a pattern
    if entries.is_empty() {
        let link_selector = Selector::parse("a[href*='/ndss-paper/']").unwrap();
        for element in document.select(&link_selector) {
            let title = element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            if title.is_empty() {
                continue;
            }

            let href = match element.value().attr("href") {
                Some(h) => h,
                None => continue,
            };

            let detail_url = if href.starts_with("http") {
                href.to_string()
            } else {
                format!("{}{}", base_url, href)
            };

            entries.push(PaperListEntry {
                title,
                authors: Vec::new(),
                detail_url,
                track: Some("Conference".to_string()),
            });
        }
    }

    Ok(entries)
}

/// "Author1 (Affiliation1), Author2 (Affiliation2)" 形式の著者文字列をパース
fn parse_authors(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }

    // Split by "), " to separate author entries, then clean up
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
                    let author = extract_name(&current);
                    if !author.is_empty() {
                        authors.push(author);
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
        // Could be multiple comma-separated authors without affiliations
        for part in remaining.split(',') {
            let name = part.trim().trim_start_matches("and ").trim();
            if !name.is_empty() {
                authors.push(name.to_string());
            }
        }
    }

    authors
}

/// "Name (Affiliation)" から名前部分を抽出
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
    fn test_parse_paper_list_basic() {
        let html = r#"
        <html><body>
        <div class="pt-cv-view">
            <div class="pt-cv-content-item pt-cv-2-col" data-pid="16886">
                <h2 class="pt-cv-title">
                    <a href="https://www.ndss-symposium.org/ndss-paper/paper-one/" class="_self">Paper One Title</a>
                </h2>
                <div class="pt-cv-ctf-list">
                    <div class="pt-cv-custom-fields pt-cv-ctf-display_authors">
                        <div class="pt-cv-ctf-value"><p>Alice Smith (MIT), Bob Jones (Stanford)</p></div>
                    </div>
                </div>
            </div>
            <div class="pt-cv-content-item pt-cv-2-col" data-pid="16915">
                <h2 class="pt-cv-title">
                    <a href="https://www.ndss-symposium.org/ndss-paper/paper-two/" class="_self">Paper Two Title</a>
                </h2>
                <div class="pt-cv-ctf-list">
                    <div class="pt-cv-custom-fields pt-cv-ctf-display_authors">
                        <div class="pt-cv-ctf-value"><p>Charlie Brown (CMU)</p></div>
                    </div>
                </div>
            </div>
        </div>
        </body></html>
        "#;

        let entries =
            parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Paper One Title");
        assert_eq!(entries[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(
            entries[0].detail_url,
            "https://www.ndss-symposium.org/ndss-paper/paper-one/"
        );
        assert_eq!(entries[0].track, Some("Conference".to_string()));
        assert_eq!(entries[1].title, "Paper Two Title");
        assert_eq!(entries[1].authors, vec!["Charlie Brown"]);
    }

    #[test]
    fn test_parse_paper_list_fallback() {
        // When pt-cv-content-item is not present, fall back to simple link parsing
        let html = r#"
        <html><body>
        <ul>
            <li><a href="/ndss-paper/my-paper-slug/">My Paper Title</a></li>
            <li><a href="/ndss-paper/another-paper/">Another Paper</a></li>
        </ul>
        </body></html>
        "#;

        let entries =
            parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "My Paper Title");
        assert_eq!(
            entries[0].detail_url,
            "https://www.ndss-symposium.org/ndss-paper/my-paper-slug/"
        );
        assert!(entries[0].authors.is_empty());
    }

    #[test]
    fn test_parse_paper_list_skips_non_paper_links() {
        let html = r#"
        <html><body>
        <div class="pt-cv-content-item">
            <h2 class="pt-cv-title">
                <a href="https://www.ndss-symposium.org/ndss-paper/real-paper/">Real Paper</a>
            </h2>
            <div class="pt-cv-ctf-list">
                <div class="pt-cv-custom-fields pt-cv-ctf-display_authors">
                    <div class="pt-cv-ctf-value"><p>Author One (Uni)</p></div>
                </div>
            </div>
        </div>
        <div class="pt-cv-content-item">
            <h2 class="pt-cv-title">
                <a href="https://www.ndss-symposium.org/other-page/">Not A Paper</a>
            </h2>
        </div>
        </body></html>
        "#;

        let entries =
            parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Real Paper");
    }

    #[test]
    fn test_parse_authors() {
        let authors =
            parse_authors("Alice Smith (MIT), Bob Jones (Stanford), Charlie Brown (CMU)");
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones", "Charlie Brown"]);
    }

    #[test]
    fn test_parse_authors_with_and() {
        let authors = parse_authors(
            "Alice Smith (MIT) and Bob Jones (Stanford)",
        );
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones"]);
    }

    #[test]
    fn test_parse_authors_empty() {
        let authors = parse_authors("");
        assert!(authors.is_empty());
    }
}
