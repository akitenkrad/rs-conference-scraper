pub mod list;
pub mod years;

use anyhow::{bail, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::types::{compute_id, Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1500);

pub struct AamasScraper {
    base_url: String,
    interval: Duration,
    paper_cache: Arc<RwLock<HashMap<String, Paper>>>,
}

impl AamasScraper {
    pub fn new() -> Self {
        Self {
            base_url: "https://www.ifaamas.org".to_string(),
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
impl ConferenceScraper for AamasScraper {
    fn id(&self) -> &str {
        "aamas"
    }

    fn name(&self) -> &str {
        "AAMAS"
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok(years::available_years())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        let (entries, papers) =
            list::fetch_paper_list(client, &self.base_url, year, self.interval).await?;

        // Cache all papers for later retrieval by fetch_paper_detail
        let mut cache = self.paper_cache.write().await;
        for paper in papers {
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
        let scraper = AamasScraper::new();
        assert_eq!(scraper.id(), "aamas");
        assert_eq!(scraper.name(), "AAMAS");
    }

    #[tokio::test]
    async fn test_paper_cache_roundtrip() {
        let scraper = AamasScraper::new();

        let paper = Paper {
            id: compute_id("Test AAMAS Paper"),
            conference: "aamas".to_string(),
            year: 2024,
            title: "Test AAMAS Paper".to_string(),
            authors: vec!["Alice Smith".to_string()],
            r#abstract: String::new(),
            url: "https://www.ifaamas.org/Proceedings/aamas2024/pdfs/p4.pdf".to_string(),
            pdf_url: Some(
                "https://www.ifaamas.org/Proceedings/aamas2024/pdfs/p4.pdf".to_string(),
            ),
            categories: vec!["Full Research Papers".to_string()],
            hash: "somehash".to_string(),
        };

        {
            let mut cache = scraper.paper_cache.write().await;
            cache.insert(paper.id.clone(), paper.clone());
        }

        let entry = PaperListEntry {
            title: "Test AAMAS Paper".to_string(),
            authors: vec!["Alice Smith".to_string()],
            detail_url: "https://www.ifaamas.org/Proceedings/aamas2024/pdfs/p4.pdf".to_string(),
            track: Some("Full Research Papers".to_string()),
        };

        let client = reqwest::Client::new();
        let result = scraper.fetch_paper_detail(&client, &entry).await.unwrap();
        assert_eq!(result.title, "Test AAMAS Paper");
        assert_eq!(result.id, compute_id("Test AAMAS Paper"));
        assert!(result.r#abstract.is_empty());
    }

    #[tokio::test]
    async fn test_paper_cache_miss() {
        let scraper = AamasScraper::new();
        let entry = PaperListEntry {
            title: "Nonexistent Paper".to_string(),
            authors: vec![],
            detail_url: "https://example.com".to_string(),
            track: None,
        };

        let client = reqwest::Client::new();
        let result = scraper.fetch_paper_detail(&client, &entry).await;
        assert!(result.is_err());
    }
}
