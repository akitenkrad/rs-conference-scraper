pub mod abstract_page;
pub mod list;
pub mod years;

use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

use crate::conference::ConferenceScraper;
use crate::types::{Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1500);

pub struct NeurIpsScraper {
    base_url: String,
    interval: Duration,
}

impl NeurIpsScraper {
    pub fn new() -> Self {
        Self {
            base_url: "https://papers.neurips.cc".to_string(),
            interval: DEFAULT_INTERVAL,
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

#[async_trait]
impl ConferenceScraper for NeurIpsScraper {
    fn id(&self) -> &str {
        "neurips"
    }

    fn name(&self) -> &str {
        "NeurIPS"
    }

    fn backend_id(&self) -> &str {
        "openreview"
    }

    async fn fetch_years(&self, client: &reqwest::Client) -> Result<Vec<u16>> {
        years::fetch_years(client, &self.base_url, self.interval).await
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
        abstract_page::fetch_paper_detail(client, entry, self.id(), year, self.interval).await
    }
}

fn extract_year_from_url(url: &str) -> Option<u16> {
    // URL pattern: .../paper_files/paper/{YEAR}/hash/...
    let parts: Vec<&str> = url.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if *part == "paper" {
            if let Some(next) = parts.get(i + 1) {
                if let Ok(year) = next.parse::<u16>() {
                    if (1980..=2030).contains(&year) {
                        return Some(year);
                    }
                }
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
                "https://papers.neurips.cc/paper_files/paper/2023/hash/abc-Abstract.html"
            ),
            Some(2023)
        );
        assert_eq!(
            extract_year_from_url("https://papers.neurips.cc/paper_files/paper/1987/hash/xyz.html"),
            Some(1987)
        );
        assert_eq!(extract_year_from_url("https://example.com/no-year"), None);
    }
}
