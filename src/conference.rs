use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::types::{Paper, PaperListEntry};

/// 会議スクレイパーの共通インターフェース
#[async_trait]
pub trait ConferenceScraper: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    /// バックエンドサイトの識別子（レートリミッター共有に使用）
    fn backend_id(&self) -> &str;
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

/// 対応会議の一覧 (ID, 表示名, 分野)
pub fn list_conferences() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // NLP
        ("acl", "ACL", "NLP"),
        ("emnlp", "EMNLP", "NLP"),
        ("naacl", "NAACL", "NLP"),
        ("coling", "COLING", "NLP"),
        ("eacl", "EACL", "NLP"),
        ("aacl", "AACL", "NLP"),
        ("lrec", "LREC", "NLP"),
        ("conll", "CoNLL", "NLP"),
        ("semeval", "SemEval", "NLP"),
        ("sigdial", "SIGDIAL", "NLP"),
        ("ijcnlp", "IJCNLP", "NLP"),
        ("wmt", "WMT", "NLP"),
        // Computer Vision
        ("cvpr", "CVPR", "CV"),
        ("iccv", "ICCV", "CV"),
        // Machine Learning
        ("iclr", "ICLR", "ML"),
        ("icml", "ICML", "ML"),
        ("neurips", "NeurIPS", "ML"),
        // Security
        ("usenix-security", "USENIX Security", "Security"),
        ("ndss", "NDSS", "Security"),
        // Multi-Agent Systems
        ("aamas", "AAMAS", "Multi-Agent"),
        // Security
        ("sp", "IEEE S&P", "Security"),
        ("ccs", "CCS", "Security"),
        ("infocom", "IEEE INFOCOM", "Networking"),
        ("icdm", "IEEE ICDM", "Data Mining"),
        ("cns", "IEEE CNS", "Security"),
        ("dsn", "IEEE/IFIP DSN", "Security"),
        ("raid", "RAID", "Security"),
        ("esorics", "ESORICS", "Security"),
        ("dimva", "DIMVA", "Security"),
        ("acsac", "ACSAC", "Security"),
        // ACM
        ("kdd", "KDD", "Data Mining"),
        ("sigcomm", "ACM SIGCOMM", "Networking"),
        ("imc", "IMC", "Networking"),
        // Cryptography
        ("crypto", "CRYPTO", "Cryptography"),
        ("eurocrypt", "EUROCRYPT", "Cryptography"),
        ("asiacrypt", "ASIACRYPT", "Cryptography"),
        // Simulation
        ("wsc", "WSC", "Simulation"),
        // Preprint
        ("eprint", "IACR ePrint", "Cryptography"),
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
                .find(|(cid, _, _)| *cid == id)
                .map(|(_, name, _)| name)
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
        // CryptoDB API (IACR)
        "crypto" | "eurocrypt" | "asiacrypt" => {
            let name = list_conferences()
                .into_iter()
                .find(|(cid, _, _)| *cid == id)
                .map(|(_, name, _)| name)
                .unwrap();
            Ok(Arc::new(
                crate::scraper::cryptodb::CryptoDbScraper::new(id, name)
                    .with_interval(interval),
            ))
        }
        // DBLP API
        "sp" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("sp", "IEEE S&P", "sp", 1981, 2025)
                .with_interval(interval),
        )),
        "ccs" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("ccs", "CCS", "ccs", 1994, 2025)
                .with_interval(interval),
        )),
        "wsc" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("wsc", "WSC", "wsc", 1968, 2025)
                .with_interval(interval),
        )),
        // IEEE conferences (DBLP)
        "infocom" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("infocom", "IEEE INFOCOM", "infocom", 1982, 2025)
                .with_interval(interval),
        )),
        "icdm" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("icdm", "IEEE ICDM", "icdm", 2001, 2025)
                .with_interval(interval),
        )),
        "cns" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("cns", "IEEE CNS", "cns", 2013, 2025)
                .with_interval(interval),
        )),
        "dsn" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("dsn", "IEEE/IFIP DSN", "dsn", 2000, 2025)
                .with_interval(interval),
        )),
        // Security conferences (DBLP)
        "raid" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("raid", "RAID", "raid", 1998, 2025)
                .with_interval(interval),
        )),
        "esorics" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("esorics", "ESORICS", "esorics", 1990, 2025)
                .with_interval(interval),
        )),
        "dimva" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("dimva", "DIMVA", "dimva", 2004, 2025)
                .with_interval(interval),
        )),
        "acsac" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("acsac", "ACSAC", "acsac", 1985, 2025)
                .with_interval(interval),
        )),
        // ACM conferences (DBLP)
        "kdd" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("kdd", "KDD", "kdd", 1995, 2025)
                .with_interval(interval),
        )),
        "sigcomm" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("sigcomm", "ACM SIGCOMM", "sigcomm", 1988, 2025)
                .with_interval(interval),
        )),
        "imc" => Ok(Arc::new(
            crate::scraper::dblp::DblpScraper::new("imc", "IMC", "imc", 2001, 2025)
                .with_interval(interval),
        )),
        "eprint" => Ok(Arc::new(
            crate::scraper::eprint::EprintScraper::new().with_interval(interval),
        )),
        _ => bail!(
            "Unknown conference: '{}'. Use 'list-conferences' to see available conferences.",
            id
        ),
    }
}
