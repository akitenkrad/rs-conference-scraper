pub mod api;

use anyhow::{bail, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::types::{compute_id, Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1500);

pub struct IclrScraper {
    interval: Duration,
    paper_cache: Arc<RwLock<HashMap<String, Paper>>>,
}

impl IclrScraper {
    pub fn new() -> Self {
        Self {
            interval: DEFAULT_INTERVAL,
            paper_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

#[async_trait]
impl ConferenceScraper for IclrScraper {
    fn id(&self) -> &str {
        "iclr"
    }

    fn name(&self) -> &str {
        "ICLR"
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok(vec![2018, 2019, 2020, 2021, 2022, 2023, 2024, 2025])
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        let papers = api::fetch_papers_for_year(client, year, self.interval).await?;

        let mut entries = Vec::with_capacity(papers.len());
        let mut cache = self.paper_cache.write().await;

        for paper in papers {
            entries.push(PaperListEntry {
                title: paper.title.clone(),
                authors: paper.authors.clone(),
                detail_url: paper.url.clone(),
                track: paper.categories.first().cloned(),
            });
            cache.insert(paper.id.clone(), paper);
        }

        Ok(entries)
    }

    async fn fetch_paper_detail(
        &self,
        _client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        let id = compute_id(&entry.title);
        let cache = self.paper_cache.read().await;
        match cache.get(&id) {
            Some(paper) => Ok(paper.clone()),
            None => bail!(
                "Paper not found in cache: '{}'. Was fetch_paper_list called first?",
                entry.title
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scraper_id_and_name() {
        let scraper = IclrScraper::new();
        assert_eq!(scraper.id(), "iclr");
        assert_eq!(scraper.name(), "ICLR");
    }

    #[tokio::test]
    async fn test_fetch_years() {
        let scraper = IclrScraper::new();
        let client = reqwest::Client::new();
        let years = scraper.fetch_years(&client).await.unwrap();
        assert_eq!(years, vec![2018, 2019, 2020, 2021, 2022, 2023, 2024, 2025]);
    }

    #[tokio::test]
    async fn test_paper_cache_roundtrip() {
        let scraper = IclrScraper::new();

        let paper = Paper {
            id: compute_id("Test ICLR Paper"),
            conference: "iclr".to_string(),
            year: 2024,
            title: "Test ICLR Paper".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            r#abstract: "An abstract.".to_string(),
            url: "https://openreview.net/forum?id=test123".to_string(),
            pdf_url: Some("https://openreview.net/pdf/test123.pdf".to_string()),
            categories: vec!["poster".to_string()],
            hash: "somehash".to_string(),
        };

        {
            let mut cache = scraper.paper_cache.write().await;
            cache.insert(paper.id.clone(), paper.clone());
        }

        let entry = PaperListEntry {
            title: "Test ICLR Paper".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            detail_url: "https://openreview.net/forum?id=test123".to_string(),
            track: Some("poster".to_string()),
        };

        let client = reqwest::Client::new();
        let result = scraper.fetch_paper_detail(&client, &entry).await.unwrap();
        assert_eq!(result.title, "Test ICLR Paper");
        assert_eq!(result.id, compute_id("Test ICLR Paper"));
    }

    #[tokio::test]
    async fn test_paper_cache_miss() {
        let scraper = IclrScraper::new();
        let entry = PaperListEntry {
            title: "Nonexistent".to_string(),
            authors: vec![],
            detail_url: "https://example.com".to_string(),
            track: None,
        };

        let client = reqwest::Client::new();
        let result = scraper.fetch_paper_detail(&client, &entry).await;
        assert!(result.is_err());
    }
}
