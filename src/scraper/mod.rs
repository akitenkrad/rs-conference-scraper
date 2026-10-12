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
            let value = if SENSITIVE_QUERY_PARAMS
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&key))
            {
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
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
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
    fetch_with_policy(client, url, interval, &RetryPolicy::standard(max_retries)).await
}

/// 再試行の待ち方．
///
/// - 通信エラー・5xx: `backoff_base * 2^n`（既定 2, 4, 8 秒）で `max_retries` 回まで．
/// - 回数制限（429，または `Retry-After` 付きの 503）: `Retry-After`（秒）に従う．
///   無ければ `throttle_base * 2^(n-1)`（既定 15, 30, 60, 120, 120 秒）で `throttle_retries` 回まで．
///   1 回の待ちは `max_wait` で頭打ちにする（取得元が何時間も待てと言っても付き合わない）．
/// - ボットの確認画面（Cloudflare / Anubis）は再試行しても通らないので，すぐに諦める．
#[derive(Debug, Clone)]
pub(crate) struct RetryPolicy {
    pub max_retries: usize,
    pub throttle_retries: usize,
    pub backoff_base: Duration,
    pub throttle_base: Duration,
    pub max_wait: Duration,
}

impl RetryPolicy {
    pub fn standard(max_retries: usize) -> Self {
        Self {
            max_retries,
            throttle_retries: 5,
            backoff_base: Duration::from_secs(1),
            throttle_base: Duration::from_secs(15),
            max_wait: Duration::from_secs(120),
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        (self.backoff_base * 2u32.saturating_pow(attempt)).min(self.max_wait)
    }

    fn throttle_wait(&self, nth: u32, retry_after: Option<Duration>) -> Duration {
        retry_after
            .unwrap_or_else(|| self.throttle_base * 2u32.saturating_pow(nth.saturating_sub(1)))
            .min(self.max_wait)
    }
}

/// `Retry-After` の秒数形式を読む（HTTP-date 形式は読まず，既定の待ちに任せる）．
fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// 本文がボットの確認画面なら，その種類を返す．
///
/// 本物の論文ページ・XML・JSON を誤判定しないよう，先頭 4 KiB に HTML 文書があり，
/// その `<title>` が確認画面のものと一致する場合だけを拾う．
pub(crate) fn detect_bot_challenge(body: &str) -> Option<&'static str> {
    let mut end = body.len().min(4096);
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    let head = body[..end].to_ascii_lowercase();
    // OpenReview の API は JSON で確認を求める（Accept が JSON のとき．2026-10 から）
    if head.trim_start().starts_with('{') && head.contains("\"challengerequirederror\"") {
        return Some("OpenReview");
    }
    if !head.contains("<html") {
        return None;
    }
    let title_start = head.find("<title>")? + "<title>".len();
    let title_len = head[title_start..].find("</title>")?;
    let title = head[title_start..title_start + title_len].trim();
    if title.starts_with("just a moment") || title.starts_with("attention required") {
        Some("Cloudflare")
    } else if title.starts_with("making sure you") {
        Some("Anubis")
    } else if title.starts_with("verifying your browser") {
        // OpenReview の API は Accept が HTML だと /challenge へ転送し，Turnstile の画面を 200 で返す
        Some("OpenReview")
    } else {
        None
    }
}

fn challenge_error(kind: &str, status: reqwest::StatusCode, safe_url: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "blocked by bot challenge ({}, HTTP {}) at {}",
        kind,
        status.as_u16(),
        safe_url
    )
}

pub(crate) async fn fetch_with_policy(
    client: &reqwest::Client,
    url: &str,
    interval: Duration,
    policy: &RetryPolicy,
) -> Result<String> {
    let safe_url = redact_url(url);
    tracing::debug!("GET {}", safe_url);
    let mut failures: u32 = 0;
    let mut throttles: u32 = 0;
    loop {
        let err: anyhow::Error = match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                let retry_after = parse_retry_after(resp.headers());
                if status.is_success() {
                    let body = resp.text().await?;
                    if let Some(kind) = detect_bot_challenge(&body) {
                        return Err(challenge_error(kind, status, &safe_url));
                    }
                    tokio::time::sleep(interval).await;
                    return Ok(body);
                }
                let body = resp.text().await.unwrap_or_default();
                if let Some(kind) = detect_bot_challenge(&body) {
                    return Err(challenge_error(kind, status, &safe_url));
                }
                let throttled = status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || (status == reqwest::StatusCode::SERVICE_UNAVAILABLE
                        && retry_after.is_some());
                if throttled {
                    throttles += 1;
                    if throttles as usize > policy.throttle_retries {
                        anyhow::bail!(
                            "HTTP {} (rate limited) at {}; gave up after {} retries",
                            status.as_u16(),
                            safe_url,
                            policy.throttle_retries
                        );
                    }
                    let wait = policy.throttle_wait(throttles, retry_after);
                    tracing::warn!(
                        "HTTP {} at {}; waiting {:?} before retry {}/{}",
                        status.as_u16(),
                        safe_url,
                        wait,
                        throttles,
                        policy.throttle_retries
                    );
                    tokio::time::sleep(wait).await;
                    continue;
                }
                // 4xx は再試行しない
                if status.is_client_error() {
                    anyhow::bail!("HTTP {} at {}", status.as_u16(), safe_url);
                }
                anyhow::anyhow!("HTTP {} at {}", status.as_u16(), safe_url)
            }
            Err(e) => e.into(),
        };
        failures += 1;
        if failures as usize > policy.max_retries {
            return Err(err);
        }
        let wait = policy.backoff(failures);
        tracing::debug!(
            "Retry {}/{} after {:?} for {}: {}",
            failures,
            policy.max_retries,
            wait,
            safe_url,
            err
        );
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_masks_appid_value() {
        let url = "https://cir.nii.ac.jp/opensearch/v2/articles?appid=SECRET-CREDENTIAL&issn=1346-8030&from=2024";
        let got = redact_url(url);
        assert!(
            !got.contains("SECRET-CREDENTIAL"),
            "appid value leaked: {}",
            got
        );
        assert!(
            got.contains("appid=***"),
            "expected appid=***, got: {}",
            got
        );
        assert!(
            got.contains("issn=1346-8030"),
            "non-sensitive param dropped: {}",
            got
        );
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
        assert!(
            got.contains("1046-8781"),
            "non-sensitive value lost: {}",
            got
        );
        assert!(
            got.contains("per_page=200"),
            "non-sensitive param lost: {}",
            got
        );
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

    fn fast_policy() -> RetryPolicy {
        RetryPolicy {
            max_retries: 2,
            throttle_retries: 3,
            backoff_base: Duration::from_millis(1),
            throttle_base: Duration::from_millis(1),
            max_wait: Duration::from_millis(20),
        }
    }

    async fn fetch(server: &wiremock::MockServer, policy: &RetryPolicy) -> Result<String> {
        let client = reqwest::Client::new();
        let url = format!("{}/page", server.uri());
        fetch_with_policy(&client, &url, Duration::ZERO, policy).await
    }

    const ANUBIS_PAGE: &str = "<!doctype html><html lang=\"en\"><head><title>Making sure you&#39;re not a bot!</title></head><body></body></html>";
    const CLOUDFLARE_PAGE: &str = "<!DOCTYPE html><html lang=\"en-US\"><head><title>Just a moment...</title></head><body></body></html>";

    #[test]
    fn detects_anubis_and_cloudflare_challenges() {
        assert_eq!(detect_bot_challenge(ANUBIS_PAGE), Some("Anubis"));
        assert_eq!(detect_bot_challenge(CLOUDFLARE_PAGE), Some("Cloudflare"));
    }

    #[test]
    fn detects_openreview_challenges() {
        // Accept が HTML：/challenge へ転送された先の Turnstile の画面（200）
        let page = "<!DOCTYPE html><html lang=\"en\"><head><title>Verifying your browser | OpenReview</title></head></html>";
        assert_eq!(detect_bot_challenge(page), Some("OpenReview"));
        // Accept が JSON：403 の JSON
        let json = r#"{"name":"ChallengeRequiredError","message":"Challenge verification required","status":403}"#;
        assert_eq!(detect_bot_challenge(json), Some("OpenReview"));
        // 論文の JSON にその語が題として出ても，name でなければ拾わない
        assert_eq!(
            detect_bot_challenge(r#"{"notes": [{"title": "ChallengeRequiredError in practice"}]}"#),
            None
        );
    }

    #[test]
    fn ordinary_pages_are_not_challenges() {
        assert_eq!(
            detect_bot_challenge("<html><head><title>IACR ePrint 2024</title></head></html>"),
            None
        );
        // DBLP の XML に同じ題の論文があっても HTML 文書でなければ拾わない
        assert_eq!(
            detect_bot_challenge(
                "<?xml version=\"1.0\"?><dblp><title>Just a moment...</title></dblp>"
            ),
            None
        );
        assert_eq!(detect_bot_challenge("{\"result\": {}}"), None);
        assert_eq!(detect_bot_challenge(""), None);
    }

    #[test]
    fn throttle_wait_prefers_retry_after_and_is_capped() {
        let p = RetryPolicy::standard(3);
        assert_eq!(p.throttle_wait(1, None), Duration::from_secs(15));
        assert_eq!(p.throttle_wait(3, None), Duration::from_secs(60));
        assert_eq!(p.throttle_wait(5, None), Duration::from_secs(120));
        assert_eq!(
            p.throttle_wait(1, Some(Duration::from_secs(42))),
            Duration::from_secs(42)
        );
        assert_eq!(
            p.throttle_wait(1, Some(Duration::from_secs(3600))),
            Duration::from_secs(120)
        );
        // 通常の失敗の待ちは従来どおり 2, 4, 8 秒
        assert_eq!(p.backoff(1), Duration::from_secs(2));
        assert_eq!(p.backoff(3), Duration::from_secs(8));
    }

    #[test]
    fn parses_retry_after_seconds_only() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_static(" 30 "),
        );
        assert_eq!(parse_retry_after(&h), Some(Duration::from_secs(30)));
        h.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT"),
        );
        assert_eq!(parse_retry_after(&h), None);
    }

    #[tokio::test]
    async fn retries_after_429_until_success() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(2)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .with_priority(2)
            .mount(&server)
            .await;

        assert_eq!(fetch(&server, &fast_policy()).await.unwrap(), "ok");
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn gives_up_after_throttle_retries() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let err = fetch(&server, &fast_policy())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("HTTP 429"), "{}", err);
        assert!(err.contains("rate limited"), "{}", err);
        // 最初の 1 回 + throttle_retries 回
        assert_eq!(server.received_requests().await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn challenge_page_fails_without_retry() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        for (status, page, kind) in [
            (200, ANUBIS_PAGE, "Anubis"),
            (403, CLOUDFLARE_PAGE, "Cloudflare"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status).set_body_string(page))
                .mount(&server)
                .await;

            let err = fetch(&server, &fast_policy())
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("bot challenge"), "{}", err);
            assert!(err.contains(kind), "{}", err);
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn client_errors_are_not_retried() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let err = fetch(&server, &fast_policy())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("HTTP 404"), "{}", err);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn server_errors_use_normal_backoff() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let err = fetch(&server, &fast_policy())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("HTTP 503"), "{}", err);
        // Retry-After の無い 503 は回数制限ではなく通常の失敗として max_retries 回
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }
}
