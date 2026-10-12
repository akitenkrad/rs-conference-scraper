pub mod api;
pub mod sparql;
pub mod xml;

use anyhow::{Result, bail};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conference::ConferenceScraper;
use crate::types::{Paper, PaperListEntry, compute_id};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1000);

/// DBLP を利用した会議スクレイパー
///
/// 取得元は SPARQL エンドポイント → Search API → XML Proceedings API の順に試す（MYTASK-3417）．
pub struct DblpScraper {
    conf_id: String,
    conf_name: String,
    dblp_key: String,
    year_range: (u16, u16),
    interval: Duration,
    sparql_endpoint: String,
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
            sparql_endpoint: sparql::SPARQL_ENDPOINT.to_string(),
            paper_cache: Arc::new(RwLock::new(HashMap::new())),
            cached_years: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    #[cfg(test)]
    fn with_sparql_endpoint(mut self, endpoint: &str) -> Self {
        self.sparql_endpoint = endpoint.to_string();
        self
    }

    /// 指定年の論文を DBLP から取得する．SPARQL → Search API → XML の順に試し，
    /// どれかが成功すればそれを返す（空の年も成功として扱う）．全部失敗したら 3 つのエラーをまとめる．
    async fn load_year(&self, client: &reqwest::Client, year: u16) -> Result<Vec<Paper>> {
        let sparql_err = match sparql::fetch_papers_for_year(
            client,
            &self.sparql_endpoint,
            &self.dblp_key,
            &self.conf_id,
            year,
            self.interval,
        )
        .await
        {
            Ok(papers) => return Ok(papers),
            Err(e) => e,
        };
        tracing::warn!(
            "DBLP SPARQL failed ({:#}), falling back to Search API for {} {}",
            sparql_err,
            self.conf_id,
            year
        );

        let api_err = match api::fetch_papers_for_year(
            client,
            &self.dblp_key,
            &self.conf_id,
            year,
            self.interval,
        )
        .await
        {
            Ok(papers) => return Ok(papers),
            Err(e) => e,
        };
        tracing::warn!(
            "DBLP Search API failed ({:#}), falling back to XML proceedings API for {} {}",
            api_err,
            self.conf_id,
            year
        );

        match xml::fetch_papers_xml(client, &self.dblp_key, &self.conf_id, year, self.interval)
            .await
        {
            Ok(papers) => Ok(papers),
            Err(xml_err) => bail!(
                "all DBLP sources failed for {} {}: SPARQL: {:#}; Search API: {:#}; XML: {:#}",
                self.conf_id,
                year,
                sparql_err,
                api_err,
                xml_err
            ),
        }
    }

    /// 指定年の論文がキャッシュになければ DBLP から取得してキャッシュに保存
    async fn ensure_year_cached(&self, client: &reqwest::Client, year: u16) -> Result<()> {
        {
            if let Some(years) = self.cached_years.read().await.as_ref()
                && years.contains(&year)
            {
                return Ok(());
            }
        }

        let papers = self.load_year(client, year).await?;

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

    fn backend_id(&self) -> &str {
        "dblp"
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
        Ok(cache
            .values()
            .filter(|p| p.year == year)
            .map(|p| PaperListEntry {
                title: p.title.clone(),
                authors: p.authors.clone(),
                detail_url: p.url.clone(),
                track: None,
            })
            .collect())
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

    #[tokio::test]
    async fn fetch_paper_list_uses_sparql_first() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = r#"{"head": {"vars": ["paper", "title", "doc", "ord", "name"]},
            "results": {"bindings": [
              {"paper": {"type": "uri", "value": "https://dblp.org/rec/conf/sp/X26"},
               "title": {"type": "literal", "value": "SPARQL Paper."},
               "doc": {"type": "uri", "value": "https://doi.org/10.1/x"},
               "ord": {"type": "literal", "value": "1"},
               "name": {"type": "literal", "value": "Xavier"}}
            ]}, "meta": {"result-size-total": 1}}"#;
        Mock::given(method("GET"))
            .and(path("/sparql"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;

        let scraper = DblpScraper::new("sp", "IEEE S&P", "sp", 1981, 2026)
            .with_interval(Duration::ZERO)
            .with_sparql_endpoint(&format!("{}/sparql", server.uri()));
        let client = reqwest::Client::new();
        let entries = scraper.fetch_paper_list(&client, 2026).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "SPARQL Paper");
        assert_eq!(entries[0].authors, vec!["Xavier"]);

        let paper = scraper
            .fetch_paper_detail(&client, &entries[0])
            .await
            .unwrap();
        assert_eq!(paper.url, "https://doi.org/10.1/x");
        assert_eq!(paper.year, 2026);
    }
}
