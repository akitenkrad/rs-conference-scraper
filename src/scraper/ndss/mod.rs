pub mod list;
pub mod paper_page;
pub mod years;

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::types::{Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1500);

pub struct NdssScraper {
    base_url: String,
    interval: Duration,
    /// fetch_paper_list で設定された年を保持し fetch_paper_detail で参照する
    current_year: Arc<RwLock<u16>>,
}

impl NdssScraper {
    pub fn new() -> Self {
        Self {
            base_url: "https://www.ndss-symposium.org".to_string(),
            interval: DEFAULT_INTERVAL,
            current_year: Arc::new(RwLock::new(0)),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

#[async_trait]
impl ConferenceScraper for NdssScraper {
    fn id(&self) -> &str {
        "ndss"
    }

    fn name(&self) -> &str {
        "NDSS"
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok(years::available_years())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        // Store the current year for use in fetch_paper_detail
        {
            let mut y = self.current_year.write().await;
            *y = year;
        }
        list::fetch_paper_list(client, &self.base_url, year, self.interval).await
    }

    async fn fetch_paper_detail(
        &self,
        client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        let year = *self.current_year.read().await;
        paper_page::fetch_paper_detail(client, entry, self.id(), year, self.interval).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scraper_id() {
        let scraper = NdssScraper::new();
        assert_eq!(scraper.id(), "ndss");
    }

    #[test]
    fn test_scraper_name() {
        let scraper = NdssScraper::new();
        assert_eq!(scraper.name(), "NDSS");
    }

    #[tokio::test]
    async fn test_fetch_years() {
        let scraper = NdssScraper::new();
        let client = reqwest::Client::new();
        let years = scraper.fetch_years(&client).await.unwrap();
        assert!(years.contains(&2024));
        assert!(years.contains(&2014));
    }
}
