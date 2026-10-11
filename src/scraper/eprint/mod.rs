pub mod list;
pub mod paper_page;

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::types::{compute_id, Paper, PaperListEntry};
use std::collections::HashMap;

const DEFAULT_INTERVAL: Duration = Duration::from_millis(2000);

pub struct EprintScraper {
    base_url: String,
    interval: Duration,
    /// fetch_paper_list で設定された年を保持し fetch_paper_detail で参照する
    current_year: Arc<RwLock<u16>>,
    /// 一覧ページから組み立てた論文（title の ID → Paper）．あれば詳細ページを取らない
    paper_cache: Arc<RwLock<HashMap<String, Paper>>>,
}

impl EprintScraper {
    pub fn new() -> Self {
        Self {
            base_url: "https://eprint.iacr.org".to_string(),
            interval: DEFAULT_INTERVAL,
            current_year: Arc::new(RwLock::new(0)),
            paper_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

#[async_trait]
impl ConferenceScraper for EprintScraper {
    fn id(&self) -> &str {
        "eprint"
    }

    fn name(&self) -> &str {
        "IACR ePrint"
    }

    fn backend_id(&self) -> &str {
        "eprint"
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok((1996..=crate::conference::current_year()).collect())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        {
            let mut y = self.current_year.write().await;
            *y = year;
        }
        let (entries, papers) =
            list::fetch_paper_list(client, &self.base_url, year, self.interval).await?;
        let mut cache = self.paper_cache.write().await;
        for paper in papers {
            cache.insert(paper.id.clone(), paper);
        }
        Ok(entries)
    }

    async fn fetch_paper_detail(
        &self,
        client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        if let Some(paper) = self.paper_cache.read().await.get(&compute_id(&entry.title)) {
            return Ok(paper.clone());
        }
        let year = *self.current_year.read().await;
        paper_page::fetch_paper_detail(client, entry, self.id(), year, self.interval).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scraper_id() {
        let scraper = EprintScraper::new();
        assert_eq!(scraper.id(), "eprint");
    }

    #[test]
    fn test_scraper_name() {
        let scraper = EprintScraper::new();
        assert_eq!(scraper.name(), "IACR ePrint");
    }

    #[tokio::test]
    async fn test_fetch_years() {
        let scraper = EprintScraper::new();
        let client = reqwest::Client::new();
        let years = scraper.fetch_years(&client).await.unwrap();
        assert!(years.contains(&1996));
        assert!(years.contains(&2025));
        assert_eq!(*years.last().unwrap(), crate::conference::current_year());
        assert_eq!(years.len(), (crate::conference::current_year() - 1996 + 1) as usize);
    }
}
