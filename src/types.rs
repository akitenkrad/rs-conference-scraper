use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// タイトルからIDを計算（SHA256）
pub fn compute_id(title: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 論文リストページから取得する中間データ（会議非依存）
#[derive(Debug, Clone)]
pub struct PaperListEntry {
    pub title: String,
    pub authors: Vec<String>,
    pub detail_url: String,
    pub track: Option<String>,
}

/// 完全な論文メタ情報（DB保存 + フィルタ入力 + JSON出力で共用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paper {
    pub id: String,
    pub conference: String,
    pub year: u16,
    pub title: String,
    pub authors: Vec<String>,
    pub r#abstract: String,
    pub url: String,
    pub pdf_url: Option<String>,
    pub categories: Vec<String>,
    pub hash: String,
}
