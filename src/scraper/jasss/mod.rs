pub mod detail;
pub mod list;
pub mod years;

use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

use crate::conference::ConferenceScraper;
use crate::types::{Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1500);

pub struct JasssScraper {
    base_url: String,
    interval: Duration,
    /// fetch_paper_list で取得した year を保持（fetch_paper_detail で利用）
    current_year: std::sync::atomic::AtomicU16,
}

impl JasssScraper {
    pub fn new() -> Self {
        Self {
            base_url: "https://www.jasss.org".to_string(),
            interval: DEFAULT_INTERVAL,
            current_year: std::sync::atomic::AtomicU16::new(0),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

#[async_trait]
impl ConferenceScraper for JasssScraper {
    fn id(&self) -> &str {
        "jasss"
    }

    fn name(&self) -> &str {
        "JASSS"
    }

    fn backend_id(&self) -> &str {
        "jasss"
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok(years::available_years())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        self.current_year
            .store(year, std::sync::atomic::Ordering::Relaxed);

        let volume = years::year_to_volume(year);
        let issues = years::issues_for_volume(volume);
        let mut all_entries = Vec::new();

        for issue in issues {
            let entries = list::fetch_paper_list(
                client,
                &self.base_url,
                volume,
                issue,
                self.interval,
            )
            .await?;
            all_entries.extend(entries);
        }

        Ok(all_entries)
    }

    async fn fetch_paper_detail(
        &self,
        client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        let year = self
            .current_year
            .load(std::sync::atomic::Ordering::Relaxed);
        detail::fetch_paper_detail(client, entry, year, self.interval).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scraper_id_and_name() {
        let scraper = JasssScraper::new();
        assert_eq!(scraper.id(), "jasss");
        assert_eq!(scraper.name(), "JASSS");
        assert_eq!(scraper.backend_id(), "jasss");
    }
}
