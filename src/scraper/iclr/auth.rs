//! OpenReview へのログイン．
//!
//! OpenReview の API は匿名の問い合わせにボットの確認（`ChallengeRequiredError`）を
//! 求めるが，ログインして得たトークンを付けた問い合わせには求めない（MYTASK-3425）．
//! 資格情報は環境変数から読む:
//!
//! - `OPENREVIEW_TOKEN`: そのまま `Authorization: Bearer` に使う．
//! - `OPENREVIEW_USERNAME` と `OPENREVIEW_PASSWORD`: `POST {api}/login` でトークンを得る．
//!
//! どちらも無ければ匿名で問い合わせる．トークンとパスワードはログにもエラーにも出さない．

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use tokio::sync::Mutex;

pub const ENV_TOKEN: &str = "OPENREVIEW_TOKEN";
pub const ENV_USERNAME: &str = "OPENREVIEW_USERNAME";
pub const ENV_PASSWORD: &str = "OPENREVIEW_PASSWORD";

/// 確認画面に当たったとき，エラーに添える案内．
pub const SIGN_IN_HINT: &str = "OpenReview asks anonymous API clients for a browser check; \
     set OPENREVIEW_TOKEN, or OPENREVIEW_USERNAME and OPENREVIEW_PASSWORD, to sign in";

/// 資格情報．`Debug` は実装しない（パスワードを出さないため）．
pub enum Credentials {
    Anonymous,
    Token(String),
    Login { username: String, password: String },
}

impl Credentials {
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok();
        Self::from_values(get(ENV_TOKEN), get(ENV_USERNAME), get(ENV_PASSWORD))
    }

    /// トークンがあればそれを優先する．ユーザー名とパスワードは両方そろったときだけ使う．
    fn from_values(token: Option<String>, user: Option<String>, pass: Option<String>) -> Self {
        let nonempty =
            |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(token) = nonempty(token) {
            return Self::Token(token);
        }
        match (nonempty(user), pass.filter(|p| !p.is_empty())) {
            (Some(username), Some(password)) => Self::Login { username, password },
            (Some(_), None) | (None, Some(_)) => {
                tracing::warn!(
                    "OpenReview: set both {} and {} to sign in; querying anonymously",
                    ENV_USERNAME,
                    ENV_PASSWORD
                );
                Self::Anonymous
            }
            (None, None) => Self::Anonymous,
        }
    }

    pub fn is_anonymous(&self) -> bool {
        matches!(self, Self::Anonymous)
    }
}

/// 1 回の実行の中でトークンを使い回す．ログインは API の基底 URL ごとに 1 回だけ行う．
pub struct Session {
    credentials: Credentials,
    tokens: Mutex<HashMap<String, String>>,
}

impl Session {
    pub fn new(credentials: Credentials) -> Self {
        Self {
            credentials,
            tokens: Mutex::new(HashMap::new()),
        }
    }

    pub fn from_env() -> Self {
        Self::new(Credentials::from_env())
    }

    pub fn is_anonymous(&self) -> bool {
        self.credentials.is_anonymous()
    }

    /// `base` への問い合わせに付けるトークン．匿名なら `None`．
    pub async fn bearer(&self, client: &reqwest::Client, base: &str) -> Result<Option<String>> {
        match &self.credentials {
            Credentials::Anonymous => Ok(None),
            Credentials::Token(token) => Ok(Some(token.clone())),
            Credentials::Login { username, password } => {
                // ロックを持ったままログインする．並行に呼ばれても 1 回で済む
                let mut tokens = self.tokens.lock().await;
                if let Some(token) = tokens.get(base) {
                    return Ok(Some(token.clone()));
                }
                let token = login(client, base, username, password).await?;
                tokens.insert(base.to_string(), token.clone());
                Ok(Some(token))
            }
        }
    }
}

#[derive(Deserialize)]
struct LoginResponse {
    token: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    message: Option<String>,
}

async fn login(
    client: &reqwest::Client,
    base: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let url = format!("{}/login", base);
    tracing::debug!("POST {} (sign in)", url);
    let resp = client
        .post(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&serde_json::json!({ "id": username, "password": password }))
        .send()
        .await
        .with_context(|| format!("OpenReview sign-in request to {} failed", url))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        if let Some(kind) = crate::scraper::detect_bot_challenge(&body) {
            bail!(
                "OpenReview sign-in at {} was blocked by bot challenge ({})",
                url,
                kind
            );
        }
        // サーバーの文言（例: "Invalid username or password"）だけを添える．送った値は出さない
        let message = serde_json::from_str::<ErrorResponse>(&body)
            .ok()
            .and_then(|e| e.message)
            .unwrap_or_default();
        bail!(
            "OpenReview sign-in at {} failed: HTTP {} {}",
            url,
            status.as_u16(),
            message
        );
    }
    let parsed: LoginResponse = serde_json::from_str(&body)
        .with_context(|| format!("OpenReview sign-in at {} returned no token", url))?;
    Ok(parsed.token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn token_wins_over_username_and_password() {
        let c = Credentials::from_values(s("tok"), s("u"), s("p"));
        assert!(matches!(c, Credentials::Token(t) if t == "tok"));
    }

    #[test]
    fn username_and_password_are_both_required() {
        assert!(Credentials::from_values(None, s("u"), None).is_anonymous());
        assert!(Credentials::from_values(None, None, s("p")).is_anonymous());
        assert!(Credentials::from_values(s("  "), s(""), s("p")).is_anonymous());
        assert!(matches!(
            Credentials::from_values(None, s("u"), s("p")),
            Credentials::Login { .. }
        ));
    }

    #[tokio::test]
    async fn login_once_per_base_and_reuse_the_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login"))
            .and(body_json(
                serde_json::json!({"id": "me@example.com", "password": "pw"}),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"token": "T123", "user": {}}"#),
            )
            .expect(1)
            .mount(&server)
            .await;

        let session = Session::new(Credentials::Login {
            username: "me@example.com".into(),
            password: "pw".into(),
        });
        let client = reqwest::Client::new();
        for _ in 0..3 {
            let t = session.bearer(&client, &server.uri()).await.unwrap();
            assert_eq!(t.as_deref(), Some("T123"));
        }
    }

    #[tokio::test]
    async fn failed_login_reports_the_server_message_without_the_password() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(ResponseTemplate::new(400).set_body_string(
                r#"{"name": "InvalidCredentials", "message": "Invalid username or password"}"#,
            ))
            .mount(&server)
            .await;

        let session = Session::new(Credentials::Login {
            username: "me@example.com".into(),
            password: "SECRET-PW".into(),
        });
        let err = session
            .bearer(&reqwest::Client::new(), &server.uri())
            .await
            .unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("HTTP 400"), "{}", msg);
        assert!(msg.contains("Invalid username or password"), "{}", msg);
        assert!(!msg.contains("SECRET-PW"), "{}", msg);
    }

    #[tokio::test]
    async fn anonymous_session_has_no_token() {
        let session = Session::new(Credentials::Anonymous);
        let t = session
            .bearer(&reqwest::Client::new(), "http://unused.invalid")
            .await
            .unwrap();
        assert!(t.is_none());
    }
}
