use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue};
use std::time::Duration;

/// URLのクエリ文字列に含まれる機密パラメータ名（値をログ出力時にマスクする）
const SENSITIVE_QUERY_PARAMS: &[&str] = &["appid", "api_key", "apikey", "token", "key"];

/// URLログ出力前に機密クエリパラメータを `***` に置換する．
///
/// `appid=secret&issn=1234-5678` → `appid=***&issn=1234-5678`
/// パースに失敗した場合は元の文字列をそのまま返す（panic しない）．
pub(crate) fn redact_url(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url.to_string();
    };
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            let key = k.into_owned();
            let value = if SENSITIVE_QUERY_PARAMS.iter().any(|s| s.eq_ignore_ascii_case(&key)) {
                "***".to_string()
            } else {
                v.into_owned()
            };
            (key, value)
        })
        .collect();
    if pairs.is_empty() {
        return parsed.to_string();
    }
    parsed.query_pairs_mut().clear().extend_pairs(&pairs);
    parsed.to_string()
}

pub mod aamas;
pub mod acl;
pub mod cinii;
pub mod cryptodb;
pub mod cvf;
pub mod dblp;
pub mod eprint;
pub mod iclr;
pub mod icml;
pub mod jasss;
pub mod ndss;
pub mod neurips;
pub mod openalex;
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
    let safe_url = redact_url(url);
    tracing::debug!("GET {}", safe_url);
    let mut last_err = None;
    for attempt in 0..=max_retries {
        if attempt > 0 {
            let backoff = Duration::from_secs(2u64.pow(attempt as u32));
            tracing::debug!("Retry {}/{} after {:?} for {}", attempt, max_retries, backoff, safe_url);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_masks_appid_value() {
        let url = "https://cir.nii.ac.jp/opensearch/v2/articles?appid=SECRET-CREDENTIAL&issn=1346-8030&from=2024";
        let got = redact_url(url);
        assert!(!got.contains("SECRET-CREDENTIAL"), "appid value leaked: {}", got);
        assert!(got.contains("appid=***"), "expected appid=***, got: {}", got);
        assert!(got.contains("issn=1346-8030"), "non-sensitive param dropped: {}", got);
    }

    #[test]
    fn redact_url_masks_api_key_and_token() {
        let url = "https://example.com/api?api_key=AAA&token=BBB&q=test";
        let got = redact_url(url);
        assert!(!got.contains("AAA"));
        assert!(!got.contains("BBB"));
        assert!(got.contains("api_key=***"));
        assert!(got.contains("token=***"));
        assert!(got.contains("q=test"));
    }

    #[test]
    fn redact_url_preserves_non_sensitive_param_values() {
        // URLの再エンコード（`:` → `%3A` 等）は許容する．契約は「機密パラメータ以外の値が改変されない」こと．
        let url = "https://api.openalex.org/works?filter=issn:1046-8781&per_page=200";
        let got = redact_url(url);
        assert!(got.contains("1046-8781"), "non-sensitive value lost: {}", got);
        assert!(got.contains("per_page=200"), "non-sensitive param lost: {}", got);
        assert!(!got.contains("***"), "no masking should happen: {}", got);
    }

    #[test]
    fn redact_url_returns_unchanged_for_unparseable_input() {
        let invalid = "not-a-url-at-all";
        let got = redact_url(invalid);
        assert_eq!(got, invalid);
    }

    #[test]
    fn redact_url_handles_empty_appid_value() {
        let url = "https://example.com?appid=";
        let got = redact_url(url);
        assert!(got.contains("appid=***") || got == url);
    }
}
