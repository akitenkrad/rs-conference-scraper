use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue};
use std::time::Duration;

pub mod aamas;
pub mod acl;
pub mod cryptodb;
pub mod cvf;
pub mod dblp;
pub mod eprint;
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

/// GETリクエスト + レスポンス取得 + Sleep（リトライ付き）
pub async fn fetch_with_sleep(
    client: &reqwest::Client,
    url: &str,
    interval: Duration,
) -> Result<String> {
    fetch_with_retry(client, url, interval, 3).await
}

/// GETリクエスト + レスポンス取得 + Sleep + リトライ
pub async fn fetch_with_retry(
    client: &reqwest::Client,
    url: &str,
    interval: Duration,
    max_retries: usize,
) -> Result<String> {
    tracing::debug!("GET {}", url);
    let mut last_err = None;
    for attempt in 0..=max_retries {
        if attempt > 0 {
            let backoff = Duration::from_secs(2u64.pow(attempt as u32));
            tracing::debug!("Retry {}/{} after {:?} for {}", attempt, max_retries, backoff, url);
            tokio::time::sleep(backoff).await;
        }
        match client.get(url).send().await {
            Ok(resp) => match resp.error_for_status() {
                Ok(resp) => {
                    let body = resp.text().await?;
                    tokio::time::sleep(interval).await;
                    return Ok(body);
                }
                Err(e) => {
                    // 4xx はリトライしない（429 Too Many Requests を除く）
                    if e.status().is_some_and(|s| s.is_client_error() && s.as_u16() != 429) {
                        return Err(e.into());
                    }
                    last_err = Some(e.into());
                }
            },
            Err(e) => {
                last_err = Some(e.into());
            }
        }
    }
    Err(last_err.unwrap())
}
