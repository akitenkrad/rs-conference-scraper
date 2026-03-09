use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue};
use std::time::Duration;

pub mod aamas;
pub mod acl;
pub mod cryptodb;
pub mod cvf;
pub mod dblp;
pub mod iclr;
pub mod icml;
pub mod ndss;
pub mod neurips;
pub mod usenix;

/// ブラウザ偽装済みHTTPクライアントを生成
pub fn build_http_client() -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Accept",
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        ),
    );
    headers.insert(
        "Accept-Language",
        HeaderValue::from_static("en-US,en;q=0.9"),
    );

    reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/131.0.0.0 Safari/537.36",
        )
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(Into::into)
}

/// GETリクエスト + レスポンス取得 + Sleep
pub async fn fetch_with_sleep(
    client: &reqwest::Client,
    url: &str,
    interval: Duration,
) -> Result<String> {
    tracing::debug!("GET {}", url);
    let resp = client.get(url).send().await?.error_for_status()?;
    let body = resp.text().await?;
    tokio::time::sleep(interval).await;
    Ok(body)
}
