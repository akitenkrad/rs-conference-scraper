use anyhow::Result;
use scraper::{Html, Selector};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;

pub async fn fetch_years(
    client: &reqwest::Client,
    base_url: &str,
    interval: Duration,
) -> Result<Vec<u16>> {
    let html = fetch_with_sleep(client, base_url, interval).await?;
    parse_years(&html)
}

fn parse_years(html: &str) -> Result<Vec<u16>> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("ul li a").unwrap();
    let mut years = Vec::new();

    for element in document.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            // Skip external links (e.g. datasets-benchmarks-proceedings.neurips.cc)
            if href.contains("datasets-benchmarks") {
                continue;
            }
            if let Some(year_str) = href.strip_prefix("/paper_files/paper/") {
                if let Ok(year) = year_str.trim_end_matches('/').parse::<u16>() {
                    years.push(year);
                }
            }
        }
    }

    years.sort();
    years.dedup();
    Ok(years)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_years_basic() {
        let html = r#"
        <html><body>
        <ul>
            <li><a href="/paper_files/paper/2023">2023</a></li>
            <li><a href="/paper_files/paper/2022">2022</a></li>
            <li><a href="/paper_files/paper/2021/">2021</a></li>
            <li><a href="https://datasets-benchmarks-proceedings.neurips.cc/paper/2021">DB 2021</a></li>
        </ul>
        </body></html>
        "#;
        let years = parse_years(html).unwrap();
        assert_eq!(years, vec![2021, 2022, 2023]);
    }

    #[test]
    fn test_parse_years_empty() {
        let html = "<html><body><p>No links here</p></body></html>";
        let years = parse_years(html).unwrap();
        assert!(years.is_empty());
    }

    /// The NeurIPS proceedings page lists years from 1987 to present.
    /// Verify the parser handles the full range including old years.
    #[test]
    fn test_parse_years_full_range_including_old() {
        let html = r#"
        <html><body>
        <ul>
            <li><a href="/paper_files/paper/2024">Advances in Neural Information Processing Systems 37</a></li>
            <li><a href="/paper_files/paper/2023">Advances in Neural Information Processing Systems 36</a></li>
            <li><a href="/paper_files/paper/2010">Advances in Neural Information Processing Systems 23</a></li>
            <li><a href="/paper_files/paper/2000">Advances in Neural Information Processing Systems 13</a></li>
            <li><a href="/paper_files/paper/1988">Neural Information Processing Systems 1</a></li>
            <li><a href="/paper_files/paper/1987">Neural Information Processing Systems 0</a></li>
            <li><a href="https://datasets-benchmarks-proceedings.neurips.cc/paper/2021">DB 2021</a></li>
        </ul>
        </body></html>
        "#;
        let years = parse_years(html).unwrap();
        assert_eq!(years, vec![1987, 1988, 2000, 2010, 2023, 2024]);
        assert!(!years.contains(&2021)); // datasets-benchmarks link is excluded
    }
}
