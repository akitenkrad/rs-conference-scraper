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
const API_BASE_URL: &str = "https://www.iacr.org/cryptodb/data/api/conf.php";

/// CryptoDB APIを利用したスクレイパー（CRYPTO / EUROCRYPT 対応）
pub struct CryptoDbScraper {
    venue_id: String,
    venue_name: String,
    api_base_url: String,
    interval: Duration,
    paper_cache: Arc<RwLock<HashMap<String, Paper>>>,
}

impl CryptoDbScraper {
    pub fn new(venue_id: &str, venue_name: &str) -> Self {
        Self {
            venue_id: venue_id.to_string(),
            venue_name: venue_name.to_string(),
            api_base_url: API_BASE_URL.to_string(),
            interval: DEFAULT_INTERVAL,
            paper_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// venue_idに基づいて対応年の範囲を返す
    fn year_range(&self) -> (u16, u16) {
        match self.venue_id.as_str() {
            "crypto" => (1981, 2025),
            "eurocrypt" => (1985, 2025),
            "asiacrypt" => (1991, 2025),
            _ => (2000, 2025),
        }
    }
}

#[async_trait]
impl ConferenceScraper for CryptoDbScraper {
    fn id(&self) -> &str {
        &self.venue_id
    }

    fn name(&self) -> &str {
        &self.venue_name
    }

    fn backend_id(&self) -> &str {
        "cryptodb"
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        let (start, end) = self.year_range();
        Ok((start..=end).collect())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        let url = format!(
            "{}?year={}&venue={}",
            self.api_base_url, year, self.venue_id
        );

        let body =
            crate::scraper::fetch_with_sleep(client, &url, self.interval).await?;

        let parsed = api::parse_response(&body, &self.venue_id, year)?;

        let mut entries = Vec::with_capacity(parsed.len());
        let mut cache = self.paper_cache.write().await;

        for (entry, paper) in parsed {
            cache.insert(paper.id.clone(), paper);
            entries.push(entry);
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
    fn test_scraper_id_and_name_crypto() {
        let scraper = CryptoDbScraper::new("crypto", "CRYPTO");
        assert_eq!(scraper.id(), "crypto");
        assert_eq!(scraper.name(), "CRYPTO");
    }

    #[test]
    fn test_scraper_id_and_name_eurocrypt() {
        let scraper = CryptoDbScraper::new("eurocrypt", "EUROCRYPT");
        assert_eq!(scraper.id(), "eurocrypt");
        assert_eq!(scraper.name(), "EUROCRYPT");
    }

    #[tokio::test]
    async fn test_fetch_years_crypto() {
        let scraper = CryptoDbScraper::new("crypto", "CRYPTO");
        let client = reqwest::Client::new();
        let years = scraper.fetch_years(&client).await.unwrap();
        assert_eq!(*years.first().unwrap(), 1981);
        assert_eq!(*years.last().unwrap(), 2025);
        assert_eq!(years.len(), 45);
    }

    #[tokio::test]
    async fn test_fetch_years_eurocrypt() {
        let scraper = CryptoDbScraper::new("eurocrypt", "EUROCRYPT");
        let client = reqwest::Client::new();
        let years = scraper.fetch_years(&client).await.unwrap();
        assert_eq!(*years.first().unwrap(), 1985);
        assert_eq!(*years.last().unwrap(), 2025);
        assert_eq!(years.len(), 41);
    }

    #[tokio::test]
    async fn test_paper_cache_roundtrip() {
        let scraper = CryptoDbScraper::new("crypto", "CRYPTO");

        let paper = Paper {
            id: compute_id("Test Crypto Paper"),
            conference: "crypto".to_string(),
            year: 2023,
            title: "Test Crypto Paper".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            r#abstract: "A test abstract.".to_string(),
            url: "https://doi.org/10.1007/test".to_string(),
            pdf_url: None,
            categories: vec!["Best Paper Award".to_string()],
            hash: "somehash".to_string(),
        };

        {
            let mut cache = scraper.paper_cache.write().await;
            cache.insert(paper.id.clone(), paper.clone());
        }

        let entry = PaperListEntry {
            title: "Test Crypto Paper".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            detail_url: "https://doi.org/10.1007/test".to_string(),
            track: Some("Best Paper Award".to_string()),
        };

        let client = reqwest::Client::new();
        let result = scraper.fetch_paper_detail(&client, &entry).await.unwrap();
        assert_eq!(result.title, "Test Crypto Paper");
        assert_eq!(result.id, compute_id("Test Crypto Paper"));
        assert_eq!(result.conference, "crypto");
    }

    #[tokio::test]
    async fn test_paper_cache_miss() {
        let scraper = CryptoDbScraper::new("crypto", "CRYPTO");
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
