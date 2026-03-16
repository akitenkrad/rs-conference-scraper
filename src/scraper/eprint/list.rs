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
    let url = format!("{}/{}", base_url, year);
    let html = fetch_with_sleep(client, &url, interval).await?;
    parse_paper_list(&html, base_url, year)
}

fn parse_paper_list(html: &str, base_url: &str, year: u16) -> Result<Vec<PaperListEntry>> {
    let document = Html::parse_document(html);

    // ePrint year listing page structure:
    // Each paper entry is a row/item containing:
    //   - A link to /{YEAR}/{NUMBER} with the paper title
    //   - Author names (typically in the same row)
    //   - Category information
    //
    // The listing page uses a structure like:
    //   <div class="paperList">
    //     <div class="paperListItem">
    //       <a href="/2024/001">Title</a>
    //       <span>Authors</span>
    //     </div>
    //   </div>
    //
    // Alternatively, entries may appear as:
    //   <a href="/2024/001">2024/001</a> <b>Title</b> <i>Authors</i>
    //
    // We use a flexible approach: find all links matching /{YEAR}/{NUMBER}

    let mut entries = Vec::new();

    // Strategy 1: Look for .paperListItem or similar container
    let item_selector = Selector::parse(".paperListItem").ok();
    if let Some(ref sel) = item_selector {
        let title_link_sel = Selector::parse("a").unwrap();
        for item in document.select(sel) {
            if let Some(entry) = parse_list_item_container(&item, base_url, year, &title_link_sel) {
                entries.push(entry);
            }
        }
    }

    // Strategy 2: If no items found, try table rows (some years use tables)
    if entries.is_empty() {
        if let Ok(row_sel) = Selector::parse("tr") {
            let link_sel = Selector::parse("a").unwrap();
            for row in document.select(&row_sel) {
                if let Some(entry) = parse_table_row(&row, base_url, year, &link_sel) {
                    entries.push(entry);
                }
            }
        }
    }

    // Strategy 3: Fallback - find all links matching /{YEAR}/{NUMBER} pattern
    if entries.is_empty() {
        let link_pattern = format!("a[href*='/{}/']", year);
        if let Ok(link_sel) = Selector::parse(&link_pattern) {
            for link in document.select(&link_sel) {
                if let Some(href) = link.value().attr("href") {
                    if !is_paper_link(href, year) {
                        continue;
                    }

                    let title = link
                        .text()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_string();

                    // Skip if the link text is just the paper ID (e.g. "2024/001")
                    if title.is_empty() || title.contains('/') {
                        // Try to get title from next sibling or parent
                        continue;
                    }

                    let detail_url = normalize_url(href, base_url);
                    entries.push(PaperListEntry {
                        title,
                        authors: Vec::new(),
                        detail_url,
                        track: None,
                    });
                }
            }
        }
    }

    Ok(entries)
}

/// Parse a container element (e.g., .paperListItem) for paper info
fn parse_list_item_container(
    item: &scraper::ElementRef,
    base_url: &str,
    year: u16,
    link_sel: &Selector,
) -> Option<PaperListEntry> {
    // Find a link that points to a paper page
    for link in item.select(link_sel) {
        let href = link.value().attr("href")?;
        if !is_paper_link(href, year) {
            continue;
        }

        let detail_url = normalize_url(href, base_url);

        // Get title - could be the link text or a nearby element
        let link_text = link
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        // Try to find title from bold/strong element or the link text itself
        let title = if link_text.contains('/') || link_text.is_empty() {
            // Link text is just ID like "2024/001", look for title elsewhere
            let bold_sel = Selector::parse("b, strong, .title").ok()?;
            item.select(&bold_sel)
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .unwrap_or(link_text)
        } else {
            link_text
        };

        if title.is_empty() {
            continue;
        }

        // Try to extract authors from italic or .authors element
        let authors = extract_authors_from_container(item);

        return Some(PaperListEntry {
            title,
            authors,
            detail_url,
            track: None,
        });
    }
    None
}

/// Parse a table row for paper info
fn parse_table_row(
    row: &scraper::ElementRef,
    base_url: &str,
    year: u16,
    link_sel: &Selector,
) -> Option<PaperListEntry> {
    for link in row.select(link_sel) {
        let href = link.value().attr("href")?;
        if !is_paper_link(href, year) {
            continue;
        }

        let detail_url = normalize_url(href, base_url);

        // Get all text from the row to extract title and authors
        let link_text = link
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        let title = if link_text.contains('/') || link_text.is_empty() {
            // Look for title in other cells
            let td_sel = Selector::parse("td").ok()?;
            let cells: Vec<String> = row
                .select(&td_sel)
                .map(|td| td.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .filter(|t| !t.is_empty() && !t.contains('/'))
                .collect();
            cells.first().cloned().unwrap_or(link_text)
        } else {
            link_text
        };

        if title.is_empty() {
            continue;
        }

        return Some(PaperListEntry {
            title,
            authors: Vec::new(),
            detail_url,
            track: None,
        });
    }
    None
}

/// Extract authors from a container element
fn extract_authors_from_container(item: &scraper::ElementRef) -> Vec<String> {
    // Try .authors class first
    if let Ok(sel) = Selector::parse(".authors, i, em") {
        if let Some(el) = item.select(&sel).next() {
            let raw = el
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            if !raw.is_empty() {
                return parse_author_names(&raw);
            }
        }
    }
    Vec::new()
}

/// Parse comma-separated or "and"-separated author names
fn parse_author_names(raw: &str) -> Vec<String> {
    raw.split(',')
        .flat_map(|part| part.split(" and "))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// Check if a URL matches the pattern /{YEAR}/{NUMBER}
fn is_paper_link(href: &str, year: u16) -> bool {
    let year_str = year.to_string();
    // Match patterns like /2024/001 or /2024/1234
    let parts: Vec<&str> = href.trim_matches('/').split('/').collect();
    if parts.len() >= 2 {
        let last_two: Vec<&str> = parts.iter().rev().take(2).copied().collect();
        // last_two[0] is the number, last_two[1] is the year
        if last_two[1] == year_str {
            return last_two[0].chars().all(|c| c.is_ascii_digit()) && !last_two[0].is_empty();
        }
    }
    false
}

/// Normalize a URL (make absolute if relative)
fn normalize_url(href: &str, base_url: &str) -> String {
    if href.starts_with("http") {
        href.to_string()
    } else {
        format!("{}{}", base_url.trim_end_matches('/'), if href.starts_with('/') { href.to_string() } else { format!("/{}", href) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_paper_link() {
        assert!(is_paper_link("/2024/001", 2024));
        assert!(is_paper_link("/2024/1234", 2024));
        assert!(is_paper_link("https://eprint.iacr.org/2024/001", 2024));
        assert!(!is_paper_link("/2024/001", 2023));
        assert!(!is_paper_link("/2024/abc", 2024));
        assert!(!is_paper_link("/2024/001.pdf", 2024));
        assert!(!is_paper_link("/search?category=crypto", 2024));
    }

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            normalize_url("/2024/001", "https://eprint.iacr.org"),
            "https://eprint.iacr.org/2024/001"
        );
        assert_eq!(
            normalize_url("https://eprint.iacr.org/2024/001", "https://eprint.iacr.org"),
            "https://eprint.iacr.org/2024/001"
        );
    }

    #[test]
    fn test_parse_author_names() {
        let authors = parse_author_names("Alice Smith, Bob Jones, Charlie Brown");
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones", "Charlie Brown"]);
    }

    #[test]
    fn test_parse_author_names_with_and() {
        let authors = parse_author_names("Alice Smith and Bob Jones");
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones"]);
    }

    #[test]
    fn test_parse_paper_list_with_links() {
        let html = r#"
        <html><body>
        <div>
            <a href="/2024/001">A Novel Cryptographic Protocol</a>
            <a href="/2024/002">Efficient Zero-Knowledge Proofs</a>
            <a href="/search?category=crypto">Crypto Category</a>
        </div>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://eprint.iacr.org", 2024).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "A Novel Cryptographic Protocol");
        assert_eq!(
            entries[0].detail_url,
            "https://eprint.iacr.org/2024/001"
        );
        assert_eq!(entries[1].title, "Efficient Zero-Knowledge Proofs");
        assert_eq!(
            entries[1].detail_url,
            "https://eprint.iacr.org/2024/002"
        );
    }

    #[test]
    fn test_parse_paper_list_table_format() {
        let html = r#"
        <html><body>
        <table>
            <tr>
                <td><a href="/2023/100">2023/100</a></td>
                <td>Post-Quantum Signatures</td>
            </tr>
            <tr>
                <td><a href="/2023/101">2023/101</a></td>
                <td>Lattice-Based Encryption</td>
            </tr>
        </table>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://eprint.iacr.org", 2023).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Post-Quantum Signatures");
        assert_eq!(
            entries[0].detail_url,
            "https://eprint.iacr.org/2023/100"
        );
    }

    #[test]
    fn test_parse_paper_list_container_format() {
        let html = r#"
        <html><body>
        <div class="paperList">
            <div class="paperListItem">
                <a href="/2024/500">2024/500</a>
                <b>Secure Multi-Party Computation</b>
                <i>Alice Smith, Bob Jones</i>
            </div>
            <div class="paperListItem">
                <a href="/2024/501">2024/501</a>
                <b>Homomorphic Encryption Schemes</b>
                <em>Charlie Brown and Dave Wilson</em>
            </div>
        </div>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://eprint.iacr.org", 2024).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Secure Multi-Party Computation");
        assert_eq!(entries[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(entries[1].title, "Homomorphic Encryption Schemes");
        assert_eq!(entries[1].authors, vec!["Charlie Brown", "Dave Wilson"]);
    }

    #[test]
    fn test_parse_paper_list_empty() {
        let html = r#"<html><body><p>No papers yet.</p></body></html>"#;
        let entries = parse_paper_list(html, "https://eprint.iacr.org", 2024).unwrap();
        assert!(entries.is_empty());
    }
}
