pub mod xml;
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

pub struct AclAnthologyScraper {
    venue_id: String,
    venue_name: String,
    base_url: String,
    xml_base_url: String,
    interval: Duration,
    paper_cache: Arc<RwLock<HashMap<String, Paper>>>,
}

impl AclAnthologyScraper {
    pub fn new(venue_id: &str, venue_name: &str) -> Self {
        Self {
            venue_id: venue_id.to_string(),
            venue_name: venue_name.to_string(),
            base_url: "https://aclanthology.org".to_string(),
            xml_base_url: "https://raw.githubusercontent.com/acl-org/acl-anthology/master/data/xml"
                .to_string(),
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
impl ConferenceScraper for AclAnthologyScraper {
    fn id(&self) -> &str {
        &self.venue_id
    }

    fn name(&self) -> &str {
        &self.venue_name
    }

    async fn fetch_years(&self, client: &reqwest::Client) -> Result<Vec<u16>> {
        years::fetch_years(client, &self.base_url, &self.venue_id, self.interval).await
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        let url = format!("{}/{}.{}.xml", self.xml_base_url, year, self.venue_id);
        let xml_body =
            crate::scraper::fetch_with_sleep(client, &url, self.interval).await?;
        let parsed = xml::parse_xml(&xml_body, &self.venue_id, year)?;

        // Store papers in cache for later retrieval by fetch_paper_detail
        let mut cache = self.paper_cache.write().await;
        for paper in parsed.papers {
            cache.insert(paper.id.clone(), paper);
        }

        Ok(parsed.entries)
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
        let scraper = AclAnthologyScraper::new("acl", "ACL");
        assert_eq!(scraper.id(), "acl");
        assert_eq!(scraper.name(), "ACL");
    }

    #[test]
    fn test_scraper_custom_venue() {
        let scraper = AclAnthologyScraper::new("emnlp", "EMNLP");
        assert_eq!(scraper.id(), "emnlp");
        assert_eq!(scraper.name(), "EMNLP");
    }

    #[tokio::test]
    async fn test_paper_cache_roundtrip() {
        let scraper = AclAnthologyScraper::new("acl", "ACL");

        // Manually insert a paper into the cache
        let paper = Paper {
            id: compute_id("Test Title"),
            conference: "acl".to_string(),
            year: 2024,
            title: "Test Title".to_string(),
            authors: vec!["Alice Smith".to_string()],
            r#abstract: "Test abstract.".to_string(),
            url: "https://aclanthology.org/2024.acl-long.1/".to_string(),
            pdf_url: Some("https://aclanthology.org/2024.acl-long.1.pdf".to_string()),
            categories: vec!["Long Papers".to_string()],
            hash: "somehash".to_string(),
        };

        {
            let mut cache = scraper.paper_cache.write().await;
            cache.insert(paper.id.clone(), paper.clone());
        }

        let entry = PaperListEntry {
            title: "Test Title".to_string(),
            authors: vec!["Alice Smith".to_string()],
            detail_url: "https://aclanthology.org/2024.acl-long.1/".to_string(),
            track: Some("Long Papers".to_string()),
        };

        let client = reqwest::Client::new();
        let result = scraper.fetch_paper_detail(&client, &entry).await.unwrap();
        assert_eq!(result.title, "Test Title");
        assert_eq!(result.id, compute_id("Test Title"));
    }

    #[tokio::test]
    async fn test_paper_cache_miss() {
        let scraper = AclAnthologyScraper::new("acl", "ACL");
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
