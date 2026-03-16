pub mod list;
pub mod paper_page;
pub mod years;

use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

use crate::conference::ConferenceScraper;
use crate::types::{Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_secs(10);

pub struct UsenixScraper {
    base_url: String,
    interval: Duration,
}

impl UsenixScraper {
    pub fn new() -> Self {
        Self {
            base_url: "https://www.usenix.org".to_string(),
            interval: DEFAULT_INTERVAL,
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        // Enforce minimum 10-second crawl delay per robots.txt
        if interval < DEFAULT_INTERVAL {
            self.interval = DEFAULT_INTERVAL;
        } else {
            self.interval = interval;
        }
        self
    }
}

#[async_trait]
impl ConferenceScraper for UsenixScraper {
    fn id(&self) -> &str {
        "usenix-security"
    }

    fn name(&self) -> &str {
        "USENIX Security"
    }

    fn backend_id(&self) -> &str {
        "usenix"
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok(years::available_years())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        list::fetch_paper_list(client, &self.base_url, year, self.interval).await
    }

    async fn fetch_paper_detail(
        &self,
        client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        let year = extract_year_from_url(&entry.detail_url).unwrap_or(0);
        paper_page::fetch_paper_detail(client, entry, year, self.interval).await
    }
}

/// Extract year from USENIX Security URL pattern:
/// /conference/usenixsecurity{YY}/presentation/...
fn extract_year_from_url(url: &str) -> Option<u16> {
    let parts: Vec<&str> = url.split('/').collect();
    for part in &parts {
        if let Some(yy_str) = part.strip_prefix("usenixsecurity") {
            if let Ok(yy) = yy_str.parse::<u16>() {
                // Convert 2-digit year to 4-digit
                if yy < 100 {
                    return Some(2000 + yy);
                }
                return Some(yy);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_year_from_url() {
        assert_eq!(
            extract_year_from_url(
                "https://www.usenix.org/conference/usenixsecurity24/presentation/smith"
            ),
            Some(2024)
        );
        assert_eq!(
            extract_year_from_url(
                "https://www.usenix.org/conference/usenixsecurity14/presentation/jones"
            ),
            Some(2014)
        );
        assert_eq!(
            extract_year_from_url("https://www.usenix.org/about"),
            None
        );
    }

    #[test]
    fn test_default_interval_is_10_seconds() {
        let scraper = UsenixScraper::new();
        assert_eq!(scraper.interval, Duration::from_secs(10));
    }

    #[test]
    fn test_with_interval_enforces_minimum() {
        let scraper = UsenixScraper::new().with_interval(Duration::from_secs(1));
        assert_eq!(scraper.interval, Duration::from_secs(10));

        let scraper = UsenixScraper::new().with_interval(Duration::from_secs(15));
        assert_eq!(scraper.interval, Duration::from_secs(15));
    }

    #[test]
    fn test_scraper_id_and_name() {
        let scraper = UsenixScraper::new();
        assert_eq!(scraper.id(), "usenix-security");
        assert_eq!(scraper.name(), "USENIX Security");
    }
}
