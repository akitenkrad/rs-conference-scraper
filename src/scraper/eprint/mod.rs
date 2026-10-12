pub mod oai;

use anyhow::{Result, bail};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::types::{Paper, PaperListEntry, compute_id};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(2000);
pub const EPRINT_BASE_URL: &str = "https://eprint.iacr.org";

/// IACR Cryptology ePrint Archive
///
/// 論文は OAI-PMH の一括取得から組み立てる（MYTASK-3419）．一括取得は 1 回のプロセスで
/// 1 度だけ行い，各年はその結果を年で切り出す．論文ページは取りに行かない．
pub struct EprintScraper {
    base_url: String,
    interval: Duration,
    /// 一括取得から組み立てた論文（title の ID → Paper）
    paper_cache: Arc<RwLock<HashMap<String, Paper>>>,
}

impl EprintScraper {
    pub fn new() -> Self {
        Self {
            base_url: EPRINT_BASE_URL.to_string(),
            interval: DEFAULT_INTERVAL,
            paper_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.to_string();
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

    fn details_are_local(&self) -> bool {
        true
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok((1996..=crate::conference::current_year()).collect())
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        let harvest = oai::shared_harvest(client, &self.base_url, self.interval).await?;
        let papers = harvest.by_year.get(&year).map(Vec::as_slice).unwrap_or(&[]);
        let mut cache = self.paper_cache.write().await;
        let mut entries = Vec::with_capacity(papers.len());
        for paper in papers {
            entries.push(PaperListEntry {
                title: paper.title.clone(),
                authors: paper.authors.clone(),
                detail_url: paper.url.clone(),
                track: None,
            });
            cache.insert(paper.id.clone(), paper.clone());
        }
        Ok(entries)
    }

    async fn fetch_paper_detail(
        &self,
        _client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        match self.paper_cache.read().await.get(&compute_id(&entry.title)) {
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
        assert_eq!(
            years.len(),
            (crate::conference::current_year() - 1996 + 1) as usize
        );
    }

    #[tokio::test]
    async fn years_share_one_harvest_and_details_come_from_it() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/oai"))
            .respond_with(ResponseTemplate::new(200).set_body_string(oai::tests::page(
                &[
                    oai::tests::record("2025/1", "Paper One", "abs one", &["Foundations"]),
                    oai::tests::record("2025/2", "Paper Two", "abs two", &[]),
                    oai::tests::record("2026/1", "Paper Three", "abs three", &[]),
                ],
                None,
            )))
            .expect(1)
            .mount(&server)
            .await;

        let scraper = EprintScraper::new()
            .with_base_url(&server.uri())
            .with_interval(Duration::ZERO);
        let client = reqwest::Client::new();
        let y2025 = scraper.fetch_paper_list(&client, 2025).await.unwrap();
        let y2026 = scraper.fetch_paper_list(&client, 2026).await.unwrap();
        let y2027 = scraper.fetch_paper_list(&client, 2027).await.unwrap();
        assert_eq!(y2025.len(), 2);
        assert_eq!(y2026.len(), 1);
        assert!(y2027.is_empty());
        assert!(scraper.details_are_local());

        let p = scraper
            .fetch_paper_detail(&client, &y2025[0])
            .await
            .unwrap();
        assert_eq!(p.title, "Paper One");
        assert_eq!(p.r#abstract, "abs one");
        assert_eq!(p.conference, "eprint");
    }
}
