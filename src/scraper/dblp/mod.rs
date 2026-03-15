pub mod api;
pub mod xml;

use anyhow::{bail, Result};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::types::{compute_id, Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1000);

/// DBLP Search API を利用した会議スクレイパー
pub struct DblpScraper {
    conf_id: String,
    conf_name: String,
    dblp_key: String,
    year_range: (u16, u16),
    interval: Duration,
    paper_cache: Arc<RwLock<HashMap<String, Paper>>>,
    cached_years: Arc<RwLock<Option<HashSet<u16>>>>,
}

impl DblpScraper {
    pub fn new(
        conf_id: &str,
        conf_name: &str,
        dblp_key: &str,
        year_start: u16,
        year_end: u16,
    ) -> Self {
        Self {
            conf_id: conf_id.to_string(),
            conf_name: conf_name.to_string(),
            dblp_key: dblp_key.to_string(),
            year_range: (year_start, year_end),
            interval: DEFAULT_INTERVAL,
            paper_cache: Arc::new(RwLock::new(HashMap::new())),
            cached_years: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// 指定年の論文がキャッシュになければ DBLP から取得してキャッシュに保存
    async fn ensure_year_cached(&self, client: &reqwest::Client, year: u16) -> Result<()> {
        {
            if let Some(years) = self.cached_years.read().await.as_ref() {
                if years.contains(&year) {
                    return Ok(());
                }
            }
        }

        let papers = api::fetch_papers_for_year(
            client,
            &self.dblp_key,
            &self.conf_id,
            year,
            self.interval,
        )
        .await?;

        let mut cache = self.paper_cache.write().await;
        for paper in papers {
            cache.insert(paper.id.clone(), paper);
        }

        let mut years = self.cached_years.write().await;
        let set = years.get_or_insert_with(HashSet::new);
        set.insert(year);

        Ok(())
    }
}

#[async_trait]
impl ConferenceScraper for DblpScraper {
    fn id(&self) -> &str {
        &self.conf_id
    }

    fn name(&self) -> &str {
        &self.conf_name
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok((self.year_range.0..=self.year_range.1).collect())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        // Search API で年ごとにキャッシュを構築し，失敗時は XML API にフォールバック
        match self.ensure_year_cached(client, year).await {
            Ok(()) => {
                let cache = self.paper_cache.read().await;
                let entries: Vec<PaperListEntry> = cache
                    .values()
                    .filter(|p| p.year == year)
                    .map(|p| PaperListEntry {
                        title: p.title.clone(),
                        authors: p.authors.clone(),
                        detail_url: p.url.clone(),
                        track: None,
                    })
                    .collect();

                Ok(entries)
            }
            Err(e) => {
                tracing::warn!(
                    "DBLP Search API failed ({}), falling back to XML proceedings API for {} {}",
                    e,
                    self.conf_id,
                    year
                );

                // XML API から論文を取得
                let papers = xml::fetch_papers_xml(
                    client,
                    &self.dblp_key,
                    &self.conf_id,
                    year,
                    self.interval,
                )
                .await?;

                // fetch_paper_detail 用にキャッシュに保存
                let entries: Vec<PaperListEntry> = papers
                    .iter()
                    .map(|p| PaperListEntry {
                        title: p.title.clone(),
                        authors: p.authors.clone(),
                        detail_url: p.url.clone(),
                        track: None,
                    })
                    .collect();

                let mut cache = self.paper_cache.write().await;
                for paper in papers {
                    cache.insert(paper.id.clone(), paper);
                }

                Ok(entries)
            }
        }
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
        let scraper = DblpScraper::new("sp", "IEEE S&P", "sp", 1981, 2025);
        assert_eq!(scraper.id(), "sp");
        assert_eq!(scraper.name(), "IEEE S&P");
    }

    #[tokio::test]
    async fn test_fetch_years_sp() {
        let scraper = DblpScraper::new("sp", "IEEE S&P", "sp", 1981, 2025);
        let client = reqwest::Client::new();
        let years = scraper.fetch_years(&client).await.unwrap();
        assert_eq!(years.len(), 2025 - 1981 + 1);
        assert_eq!(*years.first().unwrap(), 1981);
        assert_eq!(*years.last().unwrap(), 2025);
    }

    #[tokio::test]
    async fn test_fetch_years_ccs() {
        let scraper = DblpScraper::new("ccs", "CCS", "ccs", 1994, 2025);
        let client = reqwest::Client::new();
        let years = scraper.fetch_years(&client).await.unwrap();
        assert_eq!(*years.first().unwrap(), 1994);
        assert_eq!(*years.last().unwrap(), 2025);
    }

    #[tokio::test]
    async fn test_fetch_years_wsc() {
        let scraper = DblpScraper::new("wsc", "WSC", "wsc", 1968, 2025);
        let client = reqwest::Client::new();
        let years = scraper.fetch_years(&client).await.unwrap();
        assert_eq!(*years.first().unwrap(), 1968);
        assert_eq!(*years.last().unwrap(), 2025);
    }

    #[tokio::test]
    async fn test_paper_cache_roundtrip() {
        let scraper = DblpScraper::new("sp", "IEEE S&P", "sp", 1981, 2025);

        let paper = Paper {
            id: compute_id("Test DBLP Paper"),
            conference: "sp".to_string(),
            year: 2024,
            title: "Test DBLP Paper".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            r#abstract: String::new(),
            url: "https://doi.org/10.1109/SP.2024.001".to_string(),
            pdf_url: None,
            categories: vec![],
            hash: "somehash".to_string(),
        };

        {
            let mut cache = scraper.paper_cache.write().await;
            cache.insert(paper.id.clone(), paper.clone());
        }

        let entry = PaperListEntry {
            title: "Test DBLP Paper".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            detail_url: "https://doi.org/10.1109/SP.2024.001".to_string(),
            track: None,
        };

        let client = reqwest::Client::new();
        let result = scraper.fetch_paper_detail(&client, &entry).await.unwrap();
        assert_eq!(result.title, "Test DBLP Paper");
        assert_eq!(result.id, compute_id("Test DBLP Paper"));
        assert_eq!(result.conference, "sp");
    }

    #[tokio::test]
    async fn test_paper_cache_miss() {
        let scraper = DblpScraper::new("sp", "IEEE S&P", "sp", 1981, 2025);
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

    #[tokio::test]
    async fn test_fetch_paper_list_filters_by_year() {
        let scraper = DblpScraper::new("sp", "IEEE S&P", "sp", 2023, 2025);

        // キャッシュに2つの年の論文を入れる
        {
            let mut cache = scraper.paper_cache.write().await;
            cache.insert(
                compute_id("Paper 2024"),
                Paper {
                    id: compute_id("Paper 2024"),
                    conference: "sp".to_string(),
                    year: 2024,
                    title: "Paper 2024".to_string(),
                    authors: vec![],
                    r#abstract: String::new(),
                    url: String::new(),
                    pdf_url: None,
                    categories: vec![],
                    hash: String::new(),
                },
            );
            cache.insert(
                compute_id("Paper 2023"),
                Paper {
                    id: compute_id("Paper 2023"),
                    conference: "sp".to_string(),
                    year: 2023,
                    title: "Paper 2023".to_string(),
                    authors: vec![],
                    r#abstract: String::new(),
                    url: String::new(),
                    pdf_url: None,
                    categories: vec![],
                    hash: String::new(),
                },
            );
        }
        // cached_years にも登録して ensure_year_cached をスキップさせる
        {
            let mut years = scraper.cached_years.write().await;
            let set = years.get_or_insert_with(HashSet::new);
            set.insert(2023);
            set.insert(2024);
        }

        let client = reqwest::Client::new();
        let entries = scraper.fetch_paper_list(&client, 2024).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Paper 2024");
    }
}
