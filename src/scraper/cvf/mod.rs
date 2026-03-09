pub mod abstract_page;
pub mod list;
pub mod years;

use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

use crate::conference::ConferenceScraper;
use crate::scraper::fetch_with_sleep;
use crate::types::{Paper, PaperListEntry};

const DEFAULT_INTERVAL: Duration = Duration::from_millis(1500);

/// CVPR/ICCV 共通スクレイパー（openaccess.thecvf.com）
pub struct CvfScraper {
    conf_id: String,
    conf_name: String,
    conf_url_name: String,
    base_url: String,
    interval: Duration,
}

impl CvfScraper {
    pub fn new(conf_id: &str, conf_name: &str, conf_url_name: &str) -> Self {
        Self {
            conf_id: conf_id.to_string(),
            conf_name: conf_name.to_string(),
            conf_url_name: conf_url_name.to_string(),
            base_url: "https://openaccess.thecvf.com".to_string(),
            interval: DEFAULT_INTERVAL,
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

#[async_trait]
impl ConferenceScraper for CvfScraper {
    fn id(&self) -> &str {
        &self.conf_id
    }

    fn name(&self) -> &str {
        &self.conf_name
    }

    async fn fetch_years(&self, _client: &reqwest::Client) -> Result<Vec<u16>> {
        Ok(years::available_years(&self.conf_id))
    }

    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>> {
        let url = format!(
            "{}/{}{}?day=all",
            self.base_url, self.conf_url_name, year
        );
        let html = fetch_with_sleep(client, &url, self.interval).await?;
        Ok(list::parse_paper_list(&html, &self.base_url))
    }

    async fn fetch_paper_detail(
        &self,
        client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper> {
        let year = extract_year_from_url(&entry.detail_url).unwrap_or(0);
        abstract_page::fetch_paper_detail(client, entry, &self.conf_id, year, self.interval).await
    }
}

/// detail URLから年度を抽出
/// URL pattern: /content/{CONF}{YEAR}/html/...
fn extract_year_from_url(url: &str) -> Option<u16> {
    // Look for patterns like CVPR2024 or ICCV2023 in the URL path
    let re_patterns = ["CVPR", "ICCV"];
    for pattern in &re_patterns {
        if let Some(pos) = url.find(pattern) {
            let after = &url[pos + pattern.len()..];
            let year_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(year) = year_str.parse::<u16>() {
                if (2000..=2030).contains(&year) {
                    return Some(year);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_year_from_url_cvpr() {
        assert_eq!(
            extract_year_from_url(
                "https://openaccess.thecvf.com/content/CVPR2024/html/Smith_Paper_CVPR_2024_paper.html"
            ),
            Some(2024)
        );
    }

    #[test]
    fn test_extract_year_from_url_iccv() {
        assert_eq!(
            extract_year_from_url(
                "https://openaccess.thecvf.com/content/ICCV2023/html/Lee_Paper_ICCV_2023_paper.html"
            ),
            Some(2023)
        );
    }

    #[test]
    fn test_extract_year_from_url_no_match() {
        assert_eq!(
            extract_year_from_url("https://example.com/some/path"),
            None
        );
    }

    #[test]
    fn test_scraper_construction() {
        let scraper = CvfScraper::new("cvpr", "CVPR", "CVPR");
        assert_eq!(scraper.id(), "cvpr");
        assert_eq!(scraper.name(), "CVPR");

        let scraper = CvfScraper::new("iccv", "ICCV", "ICCV");
        assert_eq!(scraper.id(), "iccv");
        assert_eq!(scraper.name(), "ICCV");
    }
}
