pub mod api;

use anyhow::{bail, Result};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::types::{compute_id, Paper, PaperListEntry};

/// OpenAlex 推奨レート (10 req/s) に収まる間隔
const DEFAULT_INTERVAL: Duration = Duration::from_millis(150);

/// OpenAlex Works API を ISSN フィルタで叩くジャーナルスクレイパー
pub struct OpenAlexScraper {
    conf_id: String,
    conf_name: String,
    issn: String,
    year_range: (u16, u16),
    interval: Duration,
    paper_cache: Arc<RwLock<HashMap<String, Paper>>>,
    cached_years: Arc<RwLock<Option<HashSet<u16>>>>,
}

impl OpenAlexScraper {
    pub fn new(
        conf_id: &str,
        conf_name: &str,
        issn: &str,
        year_start: u16,
        year_end: u16,
    ) -> Self {
        Self {
            conf_id: conf_id.to_string(),
            conf_name: conf_name.to_string(),
            issn: issn.to_string(),
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

    /// 指定年の論文がキャッシュになければ OpenAlex から取得してキャッシュに保存
    async fn ensure_year_cached(&self, client: &reqwest::Client, year: u16) -> Result<()> {
        {
            if let Some(years) = self.cached_years.read().await.as_ref()
                && years.contains(&year)
            {
                return Ok(());
            }
        }

        let papers = api::fetch_papers_for_year(
            client,
            &self.issn,
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
impl ConferenceScraper for OpenAlexScraper {
    fn id(&self) -> &str {
        &self.conf_id
    }

    fn name(&self) -> &str {
        &self.conf_name
    }

    fn backend_id(&self) -> &str {
        "openalex"
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok((self.year_range.0..=self.year_range.1).collect())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        self.ensure_year_cached(client, year).await?;
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

    async fn fetch_paper_detail(
        &self,
        _client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        let id = compute_id(&entry.title);
        let cache = self.paper_cache.read().await;
        match cache.get(&id) {
            Some(p) => Ok(p.clone()),
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
        let s = OpenAlexScraper::new("jms", "J. Math. Sociol.", "0022-250X", 1971, 2025);
        assert_eq!(s.id(), "jms");
        assert_eq!(s.name(), "J. Math. Sociol.");
        assert_eq!(s.backend_id(), "openalex");
    }

    #[tokio::test]
    async fn test_fetch_years_range() {
        let s = OpenAlexScraper::new("jms", "J. Math. Sociol.", "0022-250X", 1971, 2025);
        let client = reqwest::Client::new();
        let years = s.fetch_years(&client).await.unwrap();
        assert_eq!(years.len(), 2025 - 1971 + 1);
        assert_eq!(*years.first().unwrap(), 1971);
        assert_eq!(*years.last().unwrap(), 2025);
    }

    #[tokio::test]
    async fn test_paper_cache_roundtrip() {
        let s = OpenAlexScraper::new("jms", "J. Math. Sociol.", "0022-250X", 1971, 2025);

        let paper = Paper {
            id: compute_id("Test OpenAlex Paper"),
            conference: "jms".to_string(),
            year: 2024,
            title: "Test OpenAlex Paper".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            r#abstract: "An abstract.".to_string(),
            url: "https://doi.org/10.1080/0022250x.2024.001".to_string(),
            pdf_url: None,
            categories: vec!["Sociology".to_string()],
            hash: "deadbeef".to_string(),
        };

        {
            let mut cache = s.paper_cache.write().await;
            cache.insert(paper.id.clone(), paper.clone());
        }

        let entry = PaperListEntry {
            title: "Test OpenAlex Paper".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            detail_url: "https://doi.org/10.1080/0022250x.2024.001".to_string(),
            track: None,
        };

        let client = reqwest::Client::new();
        let got = s.fetch_paper_detail(&client, &entry).await.unwrap();
        assert_eq!(got.title, "Test OpenAlex Paper");
        assert_eq!(got.conference, "jms");
        assert_eq!(got.r#abstract, "An abstract.");
    }

    #[tokio::test]
    async fn test_paper_cache_miss() {
        let s = OpenAlexScraper::new("jms", "J. Math. Sociol.", "0022-250X", 1971, 2025);
        let entry = PaperListEntry {
            title: "Nonexistent".to_string(),
            authors: vec![],
            detail_url: String::new(),
            track: None,
        };
        let client = reqwest::Client::new();
        assert!(s.fetch_paper_detail(&client, &entry).await.is_err());
    }

    #[tokio::test]
    async fn test_fetch_paper_list_filters_by_year() {
        let s = OpenAlexScraper::new("jms", "J. Math. Sociol.", "0022-250X", 2020, 2025);

        {
            let mut cache = s.paper_cache.write().await;
            cache.insert(
                compute_id("Paper 2024"),
                Paper {
                    id: compute_id("Paper 2024"),
                    conference: "jms".to_string(),
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
                    conference: "jms".to_string(),
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
        {
            let mut years = s.cached_years.write().await;
            let set = years.get_or_insert_with(HashSet::new);
            set.insert(2023);
            set.insert(2024);
        }

        let client = reqwest::Client::new();
        let entries = s.fetch_paper_list(&client, 2024).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Paper 2024");
    }
}
