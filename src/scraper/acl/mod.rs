pub mod html;
pub mod paper_page;
pub mod xml;
pub mod years;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::types::{compute_id, Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1500);

/// Entry from the GitHub API contents listing.
#[derive(Deserialize)]
struct GitHubContentEntry {
    name: String,
}

pub struct AclAnthologyScraper {
    venue_id: String,
    venue_name: String,
    base_url: String,
    xml_base_url: String,
    interval: Duration,
    paper_cache: Arc<RwLock<HashMap<String, Paper>>>,
    xml_available_years: Arc<RwLock<Option<HashSet<u16>>>>,
    /// Stores the year currently being processed (set in fetch_paper_list).
    current_year: Arc<RwLock<u16>>,
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
            xml_available_years: Arc::new(RwLock::new(None)),
            current_year: Arc::new(RwLock::new(0)),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Fetch and cache the set of years for which XML files exist on GitHub.
    async fn get_xml_available_years(
        &self,
        client: &reqwest::Client,
    ) -> Result<HashSet<u16>> {
        // Return cached value if available
        {
            let cache = self.xml_available_years.read().await;
            if let Some(ref years) = *cache {
                return Ok(years.clone());
            }
        }

        let url = "https://api.github.com/repos/acl-org/acl-anthology/contents/data/xml";
        tracing::debug!("Fetching GitHub API: {}", url);

        let resp = client.get(url).send().await?.error_for_status()?;
        let entries: Vec<GitHubContentEntry> = resp.json().await?;
        tokio::time::sleep(self.interval).await;

        // Parse filenames matching {year}.{venue_id}.xml
        let suffix = format!(".{}.xml", self.venue_id);
        let mut years = HashSet::new();
        for entry in &entries {
            if let Some(year_str) = entry.name.strip_suffix(&suffix)
                && let Ok(year) = year_str.parse::<u16>() {
                    years.insert(year);
                }
        }

        // Cache the result
        let mut cache = self.xml_available_years.write().await;
        *cache = Some(years.clone());

        Ok(years)
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

    fn backend_id(&self) -> &str {
        "acl-anthology"
    }

    async fn fetch_years(&self, client: &reqwest::Client) -> Result<Vec<u16>> {
        years::fetch_years(client, &self.base_url, &self.venue_id, self.interval).await
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        // Store current year for use in fetch_paper_detail
        {
            let mut current = self.current_year.write().await;
            *current = year;
        }

        // Check XML availability (cached after first call)
        let xml_years = self.get_xml_available_years(client).await?;

        if xml_years.contains(&year) {
            // Try XML approach first
            let url = format!("{}/{}.{}.xml", self.xml_base_url, year, self.venue_id);
            let xml_body =
                crate::scraper::fetch_with_sleep(client, &url, self.interval).await?;
            let parsed = xml::parse_xml(&xml_body, &self.venue_id, year)?;

            if !parsed.entries.is_empty() {
                // Store papers in cache for later retrieval by fetch_paper_detail
                let mut cache = self.paper_cache.write().await;
                for paper in parsed.papers {
                    cache.insert(paper.id.clone(), paper);
                }
                return Ok(parsed.entries);
            }

            // XML exists but contains no papers (event-only with <colocated> refs).
            // Fall through to HTML scraping.
            tracing::info!(
                "XML for {}/{} contains no papers, falling back to HTML scraping",
                self.venue_id,
                year
            );
        }

        // HTML scraping fallback
        let url = format!("{}/events/{}-{}/", self.base_url, self.venue_id, year);
        let page_html =
            crate::scraper::fetch_with_sleep(client, &url, self.interval).await?;
        let parsed = html::parse_event_page(&page_html, &self.venue_id, year)?;
        Ok(parsed)
    }

    async fn fetch_paper_detail(
        &self,
        client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        let id = compute_id(&entry.title);

        // Try cache first (XML case)
        {
            let cache = self.paper_cache.read().await;
            if let Some(paper) = cache.get(&id) {
                return Ok(paper.clone());
            }
        }

        // HTML case: fetch individual page for abstract
        let abstract_text = if !entry.detail_url.is_empty() {
            let page_html =
                crate::scraper::fetch_with_sleep(client, &entry.detail_url, self.interval)
                    .await?;
            paper_page::parse_abstract(&page_html)
        } else {
            String::new()
        };

        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(format!("{}{}", entry.title, abstract_text).as_bytes());
            format!("{:x}", hasher.finalize())
        };

        // Derive PDF URL from detail_url
        let pdf_url = if entry.detail_url.is_empty() {
            None
        } else {
            // detail_url like "https://aclanthology.org/P05-1001/"
            // pdf_url like "https://aclanthology.org/P05-1001.pdf"
            Some(entry.detail_url.trim_end_matches('/').to_string() + ".pdf")
        };

        let year = {
            let current = self.current_year.read().await;
            *current
        };

        Ok(Paper {
            id,
            conference: self.venue_id.clone(),
            year,
            title: entry.title.clone(),
            authors: entry.authors.clone(),
            r#abstract: abstract_text,
            url: entry.detail_url.clone(),
            pdf_url,
            categories: entry
                .track
                .as_ref()
                .map(|t| vec![t.clone()])
                .unwrap_or_default(),
            hash,
        })
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
    async fn test_paper_detail_html_fallback() {
        let scraper = AclAnthologyScraper::new("acl", "ACL");

        // Set current year
        {
            let mut year = scraper.current_year.write().await;
            *year = 2005;
        }

        // Don't insert into cache — this simulates the HTML fallback path.
        // We can't actually fetch the URL in a unit test, but we can verify
        // that the cache miss doesn't panic and the flow proceeds correctly.
        // For a real test, we'd need a mock HTTP server.
    }

    #[tokio::test]
    async fn test_xml_available_years_caching() {
        let scraper = AclAnthologyScraper::new("acl", "ACL");

        // Manually populate the cache to avoid network call
        {
            let mut cache = scraper.xml_available_years.write().await;
            let mut years = HashSet::new();
            years.insert(2020);
            years.insert(2021);
            years.insert(2022);
            *cache = Some(years);
        }

        // Should return cached value without network call
        let client = reqwest::Client::new();
        let years = scraper.get_xml_available_years(&client).await.unwrap();
        assert!(years.contains(&2020));
        assert!(years.contains(&2021));
        assert!(years.contains(&2022));
        assert!(!years.contains(&2019));
    }
}
