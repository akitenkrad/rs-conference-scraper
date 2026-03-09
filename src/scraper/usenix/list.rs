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
    let yy = format!("{:02}", year % 100);
    let url = format!(
        "{}/conference/usenixsecurity{}/technical-sessions",
        base_url, yy
    );
    let html = fetch_with_sleep(client, &url, interval).await?;
    parse_paper_list(&html, base_url)
}

fn parse_paper_list(html: &str, base_url: &str) -> Result<Vec<PaperListEntry>> {
    let document = Html::parse_document(html);
    let link_selector = Selector::parse("a[href*='/presentation/']").unwrap();

    let mut entries = Vec::new();
    let mut seen_urls = HashSet::new();

    for element in document.select(&link_selector) {
        if let Some(href) = element.value().attr("href") {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_paper_list_basic() {
        let html = r#"
        <html><body>
        <div class="view-content">
            <div class="node">
                <h2 class="node-title">
                    <a href="/conference/usenixsecurity24/presentation/smith">A Novel Attack on TLS</a>
                </h2>
            </div>
            <div class="node">
                <h2 class="node-title">
                    <a href="/conference/usenixsecurity24/presentation/jones">Defense Against Adversarial ML</a>
                </h2>
            </div>
        </div>
        </body></html>
        "#;
        let entries = parse_paper_list(html, "https://www.usenix.org").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "A Novel Attack on TLS");
        assert_eq!(
            entries[0].detail_url,
            "https://www.usenix.org/conference/usenixsecurity24/presentation/smith"
        );
        assert_eq!(entries[0].track, Some("Conference".to_string()));
        assert_eq!(entries[1].title, "Defense Against Adversarial ML");
    }

    #[test]
    fn test_parse_paper_list_deduplication() {
        let html = r#"
        <html><body>
        <a href="/conference/usenixsecurity24/presentation/smith">Paper A</a>
        <a href="/conference/usenixsecurity24/presentation/smith">Paper A</a>
        </body></html>
        "#;
        let entries = parse_paper_list(html, "https://www.usenix.org").unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_parse_paper_list_absolute_urls() {
        let html = r#"
        <html><body>
        <a href="https://www.usenix.org/conference/usenixsecurity24/presentation/doe">Absolute URL Paper</a>
        </body></html>
        "#;
        let entries = parse_paper_list(html, "https://www.usenix.org").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].detail_url,
            "https://www.usenix.org/conference/usenixsecurity24/presentation/doe"
        );
    }

    #[test]
    fn test_parse_paper_list_skips_empty_titles() {
        let html = r#"
        <html><body>
        <a href="/conference/usenixsecurity24/presentation/empty">   </a>
        <a href="/conference/usenixsecurity24/presentation/real">Real Paper</a>
        </body></html>
        "#;
        let entries = parse_paper_list(html, "https://www.usenix.org").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Real Paper");
    }

    #[test]
    fn test_parse_paper_list_ignores_non_presentation_links() {
        let html = r#"
        <html><body>
        <a href="/conference/usenixsecurity24/program">Program</a>
        <a href="/conference/usenixsecurity24/presentation/smith">A Paper</a>
        <a href="/about">About</a>
        </body></html>
        "#;
        let entries = parse_paper_list(html, "https://www.usenix.org").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "A Paper");
    }
}
