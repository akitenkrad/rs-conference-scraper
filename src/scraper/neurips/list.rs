use anyhow::Result;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::PaperListEntry;

pub async fn fetch_paper_list(
    client: &reqwest::Client,
    base_url: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<PaperListEntry>> {
    let url = format!("{}/paper_files/paper/{}", base_url, year);
    let html = fetch_with_sleep(client, &url, interval).await?;
    parse_paper_list(&html, base_url)
}

fn parse_paper_list(html: &str, base_url: &str) -> Result<Vec<PaperListEntry>> {
    let document = Html::parse_document(html);

    // NeurIPS paper links contain /hash/ in the href and point to Abstract pages
    let link_selector = Selector::parse("a[href*='/hash/']").unwrap();

    let mut entries = Vec::new();
    let mut seen_urls = HashSet::new();

    for element in document.select(&link_selector) {
        if let Some(href) = element.value().attr("href") {
            // Only include Abstract pages, not Review/Supplemental etc.
            if !href.contains("-Abstract") {
                continue;
            }

            let detail_url = if href.starts_with("http") {
                href.to_string()
            } else {
                format!("{}{}", base_url, href)
            };

            if !seen_urls.insert(detail_url.clone()) {
                continue;
            }

            let title = element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            if title.is_empty() {
                continue;
            }

            let track = if detail_url.contains("Datasets_and_Benchmarks") {
                Some("Datasets and Benchmarks".to_string())
            } else {
                Some("Conference".to_string())
            };

            entries.push(PaperListEntry {
                title,
                authors: Vec::new(), // Will be fetched from detail page
                detail_url,
                track,
            });
        }
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_paper_list() {
        let html = r#"
        <html><body>
        <ul>
            <li><a href="/paper_files/paper/2023/hash/abc123-Abstract-Conference.html">Some Paper Title</a></li>
            <li><a href="/paper_files/paper/2023/hash/def456-Abstract-Datasets_and_Benchmarks.html">Dataset Paper</a></li>
            <li><a href="/paper_files/paper/2023/hash/abc123-Supplemental.html">Supplemental</a></li>
        </ul>
        </body></html>
        "#;
        let entries = parse_paper_list(html, "https://papers.neurips.cc").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Some Paper Title");
        assert_eq!(entries[0].track, Some("Conference".to_string()));
        assert_eq!(entries[1].title, "Dataset Paper");
        assert_eq!(
            entries[1].track,
            Some("Datasets and Benchmarks".to_string())
        );
    }

    #[test]
    fn test_parse_paper_list_deduplication() {
        let html = r#"
        <html><body>
        <a href="/paper_files/paper/2023/hash/abc123-Abstract-Conference.html">Paper A</a>
        <a href="/paper_files/paper/2023/hash/abc123-Abstract-Conference.html">Paper A</a>
        </body></html>
        "#;
        let entries = parse_paper_list(html, "https://papers.neurips.cc").unwrap();
        assert_eq!(entries.len(), 1);
    }

    /// Older NeurIPS years (1987-2021) use `-Abstract.html` without a track suffix.
    /// The parser must still accept these links and default to "Conference" track.
    #[test]
    fn test_parse_paper_list_old_year_no_track_suffix() {
        let html = r#"
        <html><body>
        <ul>
            <li>
                <a href="/paper_files/paper/1987/hash/03004620ea802b9118dd44d69f07af56-Abstract.html">Synchronization in Neural Nets</a>
                Vidal, Jacques, Haggerty, John
            </li>
            <li>
                <a href="/paper_files/paper/1987/hash/0316d8d63a0c252a3ec57921d7d2429b-Abstract.html">Another Paper Title</a>
                Some Author
            </li>
        </ul>
        </body></html>
        "#;
        let entries = parse_paper_list(html, "https://papers.neurips.cc").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Synchronization in Neural Nets");
        assert_eq!(entries[0].track, Some("Conference".to_string()));
        assert_eq!(
            entries[0].detail_url,
            "https://papers.neurips.cc/paper_files/paper/1987/hash/03004620ea802b9118dd44d69f07af56-Abstract.html"
        );
        assert_eq!(entries[1].title, "Another Paper Title");
        assert_eq!(entries[1].track, Some("Conference".to_string()));
    }

    /// Mix of old-style (no track suffix) and new-style (with track suffix) links
    /// should both be parsed correctly.
    #[test]
    fn test_parse_paper_list_mixed_old_and_new_style() {
        let html = r#"
        <html><body>
        <ul>
            <li><a href="/paper_files/paper/2000/hash/aaa111-Abstract.html">Old Style Paper</a></li>
            <li><a href="/paper_files/paper/2023/hash/bbb222-Abstract-Conference.html">New Style Paper</a></li>
            <li><a href="/paper_files/paper/2023/hash/ccc333-Abstract-Datasets_and_Benchmarks.html">Dataset Paper</a></li>
            <li><a href="/paper_files/paper/2000/hash/ddd444-Supplemental.html">Old Supplemental</a></li>
        </ul>
        </body></html>
        "#;
        let entries = parse_paper_list(html, "https://papers.neurips.cc").unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].title, "Old Style Paper");
        assert_eq!(entries[0].track, Some("Conference".to_string()));
        assert_eq!(entries[1].title, "New Style Paper");
        assert_eq!(entries[1].track, Some("Conference".to_string()));
        assert_eq!(entries[2].title, "Dataset Paper");
        assert_eq!(entries[2].track, Some("Datasets and Benchmarks".to_string()));
    }
}
