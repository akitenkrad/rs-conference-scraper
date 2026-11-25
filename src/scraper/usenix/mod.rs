pub mod list;
pub mod paper_page;
pub mod years;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::scraper::dblp::DblpScraper;
use crate::types::{Paper, PaperListEntry, compute_id};

const DEFAULT_INTERVAL: Duration = Duration::from_secs(10);

pub struct UsenixScraper {
    base_url: String,
    interval: Duration,
    dblp: DblpScraper,
    paper_years: Arc<RwLock<HashMap<String, u16>>>,
}

#[derive(Debug, PartialEq, Eq)]
enum Backend {
    Dblp,
    Official,
}

fn backend_for_year(year: u16) -> Backend {
    if (1993..=2013).contains(&year) {
        Backend::Dblp
    } else {
        Backend::Official
    }
}

fn new_dblp_scraper() -> DblpScraper {
    DblpScraper::new("usenix-security", "USENIX Security", "uss", 1993, 2013)
        .with_main_proceedings_only()
}

impl UsenixScraper {
    pub fn new() -> Self {
        Self {
            base_url: "https://www.usenix.org".to_string(),
            interval: DEFAULT_INTERVAL,
            dblp: new_dblp_scraper(),
            paper_years: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        // Enforce minimum 10-second crawl delay per robots.txt
        if interval < DEFAULT_INTERVAL {
            self.interval = DEFAULT_INTERVAL;
        } else {
            self.interval = interval;
        }
        let dblp = std::mem::replace(&mut self.dblp, new_dblp_scraper());
        self.dblp = dblp.with_interval(interval);
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
        let entries = match backend_for_year(year) {
            Backend::Dblp => self.dblp.fetch_paper_list(client, year).await?,
            Backend::Official => {
                list::fetch_paper_list(client, &self.base_url, year, self.interval).await?
            }
        };

        let mut paper_years = self.paper_years.write().await;
        for entry in &entries {
            paper_years.insert(compute_id(&entry.title), year);
        }
        Ok(entries)
    }

    async fn fetch_paper_detail(
        &self,
        client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        let paper_id = compute_id(&entry.title);
        let year = self
            .paper_years
            .read()
            .await
            .get(&paper_id)
            .copied()
            .or_else(|| extract_year_from_url(&entry.detail_url))
            .unwrap_or(0);

        match backend_for_year(year) {
            Backend::Dblp => self.dblp.fetch_paper_detail(client, entry).await,
            Backend::Official => {
                paper_page::fetch_paper_detail(client, entry, year, self.interval).await
            }
        }
    }
}

/// Extract year from USENIX Security URL pattern:
/// /conference/usenixsecurity{YY}/presentation/...
fn extract_year_from_url(url: &str) -> Option<u16> {
    let parts: Vec<&str> = url.split('/').collect();
    for part in &parts {
        if let Some(yy_str) = part.strip_prefix("usenixsecurity")
            && let Ok(yy) = yy_str.parse::<u16>()
        {
            // Convert 2-digit year to 4-digit
            if yy < 100 {
                return Some(2000 + yy);
            }
            return Some(yy);
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
        assert_eq!(extract_year_from_url("https://www.usenix.org/about"), None);
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

    #[test]
    fn test_list_and_detail_backend_routing_at_2013_boundary() {
        assert_eq!(backend_for_year(1993), Backend::Dblp);
        assert_eq!(backend_for_year(2013), Backend::Dblp);
        assert_eq!(backend_for_year(2014), Backend::Official);
        assert!(!UsenixScraper::new().details_are_local());
    }
}
