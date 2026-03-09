use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::types::{Paper, PaperListEntry};

/// 会議スクレイパーの共通インターフェース
#[async_trait]
pub trait ConferenceScraper: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    async fn fetch_years(&self, client: &reqwest::Client) -> Result<Vec<u16>>;
    async fn fetch_paper_list(
        &self,
        client: &reqwest::Client,
        year: u16,
    ) -> Result<Vec<PaperListEntry>>;
    async fn fetch_paper_detail(
        &self,
        client: &reqwest::Client,
        entry: &PaperListEntry,
    ) -> Result<Paper>;
}

/// 対応会議の一覧
pub fn list_conferences() -> Vec<(&'static str, &'static str)> {
    vec![
        ("acl", "ACL"),
        ("emnlp", "EMNLP"),
        ("naacl", "NAACL"),
        ("coling", "COLING"),
        ("eacl", "EACL"),
        ("aacl", "AACL"),
        ("lrec", "LREC"),
        ("conll", "CoNLL"),
        ("semeval", "SemEval"),
        ("sigdial", "SIGDIAL"),
        ("ijcnlp", "IJCNLP"),
        ("wmt", "WMT"),
        ("cvpr", "CVPR"),
        ("iccv", "ICCV"),
        ("iclr", "ICLR"),
        ("icml", "ICML"),
        ("neurips", "NeurIPS"),
        ("usenix-security", "USENIX Security"),
        ("ndss", "NDSS"),
        ("aamas", "AAMAS"),
    ]
}

/// 会議IDからスクレイパーインスタンスを取得
pub fn get_scraper(
    id: &str,
    interval: std::time::Duration,
) -> Result<Arc<dyn ConferenceScraper>> {
    match id {
        "acl" | "emnlp" | "naacl" | "coling" | "eacl" | "aacl" | "lrec" | "conll" | "semeval"
        | "sigdial" | "ijcnlp" | "wmt" => {
            let name = list_conferences()
                .into_iter()
                .find(|(cid, _)| *cid == id)
                .map(|(_, name)| name)
                .unwrap();
            Ok(Arc::new(
                crate::scraper::acl::AclAnthologyScraper::new(id, name)
                    .with_interval(interval),
            ))
        }
        "cvpr" => Ok(Arc::new(
            crate::scraper::cvf::CvfScraper::new("cvpr", "CVPR", "CVPR")
                .with_interval(interval),
        )),
        "iccv" => Ok(Arc::new(
            crate::scraper::cvf::CvfScraper::new("iccv", "ICCV", "ICCV")
                .with_interval(interval),
        )),
        "iclr" => Ok(Arc::new(
            crate::scraper::iclr::IclrScraper::new().with_interval(interval),
        )),
        "icml" => Ok(Arc::new(
            crate::scraper::icml::IcmlScraper::new().with_interval(interval),
        )),
        "neurips" => Ok(Arc::new(
            crate::scraper::neurips::NeurIpsScraper::new().with_interval(interval),
        )),
        "usenix-security" => Ok(Arc::new(
            crate::scraper::usenix::UsenixScraper::new().with_interval(interval),
        )),
        "ndss" => Ok(Arc::new(
            crate::scraper::ndss::NdssScraper::new().with_interval(interval),
        )),
        "aamas" => Ok(Arc::new(
            crate::scraper::aamas::AamasScraper::new().with_interval(interval),
        )),
        _ => bail!(
            "Unknown conference: '{}'. Use 'list-conferences' to see available conferences.",
            id
        ),
    }
}
