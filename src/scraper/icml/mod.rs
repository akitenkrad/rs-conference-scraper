pub mod abstract_page;
pub mod list;
pub mod volumes;

use anyhow::{bail, Result};
use async_trait::async_trait;
use std::time::Duration;

use crate::conference::ConferenceScraper;
use crate::types::{Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1500);

pub struct IcmlScraper {
    base_url: String,
    interval: Duration,
}

impl IcmlScraper {
    pub fn new() -> Self {
        Self {
            base_url: "https://proceedings.mlr.press".to_string(),
            interval: DEFAULT_INTERVAL,
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

#[async_trait]
impl ConferenceScraper for IcmlScraper {
    fn id(&self) -> &str {
        "icml"
    }

    fn name(&self) -> &str {
        "ICML"
    }

    fn backend_id(&self) -> &str {
        "openreview"
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok(volumes::available_years())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        let volume = volumes::year_to_volume(year);
        match volume {
            Some(v) => list::fetch_paper_list(client, &self.base_url, v, self.interval).await,
            None => bail!(
                "ICML: unsupported year {}. Supported years: {:?}",
                year,
                volumes::available_years()
            ),
        }
    }

    async fn fetch_paper_detail(
        &self,
        client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        let year = extract_year_from_volume_url(&entry.detail_url);
        abstract_page::fetch_paper_detail(client, entry, self.id(), year, self.interval).await
    }
}

/// URLからボリューム番号を抽出し，年度に変換する
fn extract_year_from_volume_url(url: &str) -> u16 {
    // URL pattern: .../v{VOLUME}/...
    for segment in url.split('/') {
        if let Some(vol_str) = segment.strip_prefix('v') {
            if let Ok(vol) = vol_str.parse::<u16>() {
                // Reverse lookup: volume -> year
                for year in volumes::available_years() {
                    if volumes::year_to_volume(year) == Some(vol) {
                        return year;
                    }
                }
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_year_from_volume_url() {
        assert_eq!(
            extract_year_from_volume_url("https://proceedings.mlr.press/v235/paper24a.html"),
            2024
        );
        assert_eq!(
            extract_year_from_volume_url("https://proceedings.mlr.press/v202/paper23a.html"),
            2023
        );
        assert_eq!(
            extract_year_from_volume_url("https://example.com/no-volume"),
            0
        );
    }
}
