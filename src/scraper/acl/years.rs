use anyhow::Result;
use regex::Regex;
use std::time::Duration;

use crate::scraper::fetch_with_sleep;

pub async fn fetch_years(
    client: &reqwest::Client,
    base_url: &str,
    venue_id: &str,
    interval: Duration,
) -> Result<Vec<u16>> {
    let url = format!("{}/venues/{}/", base_url, venue_id);
    let html = fetch_with_sleep(client, &url, interval).await?;
    parse_years(&html, venue_id)
}

fn parse_years(html: &str, venue_id: &str) -> Result<Vec<u16>> {
    let re = Regex::new(&format!(r#"href="/events/{}-(\d{{4}})/""#, regex::escape(venue_id)))?;
    let mut years: Vec<u16> = re
        .captures_iter(html)
        .filter_map(|cap| cap[1].parse::<u16>().ok())
        .collect();

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
        <div>
            <a href="/events/acl-2024/">ACL 2024</a>
            <a href="/events/acl-2023/">ACL 2023</a>
            <a href="/events/acl-2022/">ACL 2022</a>
        </div>
        </body></html>
        "#;
        let years = parse_years(html, "acl").unwrap();
        assert_eq!(years, vec![2022, 2023, 2024]);
    }

    #[test]
    fn test_parse_years_empty() {
        let html = "<html><body><p>No links here</p></body></html>";
        let years = parse_years(html, "acl").unwrap();
        assert!(years.is_empty());
    }

    #[test]
    fn test_parse_years_deduplication() {
        let html = r#"
        <html><body>
            <a href="/events/acl-2023/">ACL 2023</a>
            <a href="/events/acl-2023/">ACL 2023 (duplicate)</a>
            <a href="/events/acl-2022/">ACL 2022</a>
        </body></html>
        "#;
        let years = parse_years(html, "acl").unwrap();
        assert_eq!(years, vec![2022, 2023]);
    }

    #[test]
    fn test_parse_years_ignores_other_venues() {
        let html = r#"
        <html><body>
            <a href="/events/acl-2024/">ACL 2024</a>
            <a href="/events/emnlp-2024/">EMNLP 2024</a>
            <a href="/events/naacl-2024/">NAACL 2024</a>
        </body></html>
        "#;
        let years = parse_years(html, "acl").unwrap();
        assert_eq!(years, vec![2024]);
    }

    #[test]
    fn test_parse_years_different_venue() {
        let html = r#"
        <html><body>
            <a href="/events/emnlp-2024/">EMNLP 2024</a>
            <a href="/events/emnlp-2023/">EMNLP 2023</a>
            <a href="/events/acl-2024/">ACL 2024</a>
        </body></html>
        "#;
        let years = parse_years(html, "emnlp").unwrap();
        assert_eq!(years, vec![2023, 2024]);
    }
}
