use anyhow::{Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;

use super::auth::{SIGN_IN_HINT, Session};
use crate::types::{Paper, compute_id};

// ---------------------------------------------------------------------------
// OpenReview API v2 (ICLR 2024+)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ApiResponseV2 {
    pub notes: Vec<NoteV2>,
    pub count: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct NoteV2 {
    #[allow(dead_code)]
    pub id: String,
    pub forum: String,
    pub content: ContentV2,
}

#[derive(Debug, Deserialize)]
pub struct ContentV2 {
    pub title: ValueWrapper<String>,
    pub authors: Option<ValueWrapper<Vec<String>>>,
    #[serde(rename = "abstract")]
    pub r#abstract: Option<ValueWrapper<String>>,
    pub pdf: Option<ValueWrapper<String>>,
    pub venue: Option<ValueWrapper<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ValueWrapper<T> {
    pub value: T,
}

// ---------------------------------------------------------------------------
// OpenReview API v1 (ICLR 2020-2023)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ApiResponseV1 {
    pub notes: Vec<NoteV1>,
    pub count: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct NoteV1 {
    #[allow(dead_code)]
    pub id: String,
    pub forum: String,
    pub content: ContentV1,
}

#[derive(Debug, Deserialize)]
pub struct ContentV1 {
    pub title: String,
    pub authors: Option<Vec<String>>,
    #[serde(rename = "abstract")]
    pub r#abstract: Option<String>,
    pub pdf: Option<String>,
    pub venue: Option<String>,
}

// ---------------------------------------------------------------------------
// OpenReview API v1 legacy structs (ICLR 2018)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DecisionContent {
    decision: String,
}

#[derive(Debug, Deserialize)]
struct DecisionNote {
    forum: String,
    content: DecisionContent,
}

#[derive(Debug, Deserialize)]
struct DecisionResponse {
    notes: Vec<DecisionNote>,
    count: Option<u64>,
}

// ---------------------------------------------------------------------------
// OpenReview API v1 legacy structs (ICLR 2019)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct NoteV1WithDetails {
    #[allow(dead_code)]
    id: String,
    forum: String,
    content: ContentV1,
    details: Option<NoteDetails>,
}

#[derive(Debug, Deserialize)]
struct ApiResponseV1WithDetails {
    notes: Vec<NoteV1WithDetails>,
    count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct NoteDetails {
    #[serde(rename = "directReplies")]
    direct_replies: Vec<ReplyNote>,
}

#[derive(Debug, Deserialize)]
struct ReplyNote {
    invitation: String,
    content: ReplyContent,
}

#[derive(Debug, Deserialize)]
struct ReplyContent {
    recommendation: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const OPENREVIEW_BASE: &str = "https://openreview.net";
const API_V1_BASE: &str = "https://api.openreview.net";
const API_V2_BASE: &str = "https://api2.openreview.net";
const PAGE_LIMIT: u64 = 1000;

fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(abstract_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract category from venue string (e.g. "ICLR 2024 poster" → "poster").
fn extract_category(venue: Option<&str>) -> String {
    match venue {
        Some(v) => {
            let lower = v.to_lowercase();
            // Common venue suffixes: poster, oral, spotlight, workshop, tiny paper
            for keyword in &["oral", "spotlight", "poster", "workshop", "tiny paper"] {
                if lower.contains(keyword) {
                    return keyword.to_string();
                }
            }
            "Conference".to_string()
        }
        None => "Conference".to_string(),
    }
}

/// ICLR 2018-2019のdecision/recommendation文字列からカテゴリを抽出する．
fn extract_legacy_category(decision: &str) -> String {
    let lower = decision.to_lowercase();
    if lower.contains("oral") {
        "oral".to_string()
    } else if lower.contains("poster") {
        "poster".to_string()
    } else if lower.contains("workshop") {
        "workshop".to_string()
    } else {
        "poster".to_string()
    }
}

fn build_paper_url(forum_id: &str) -> String {
    format!("{}/forum?id={}", OPENREVIEW_BASE, forum_id)
}

fn build_pdf_url(pdf_path: Option<&str>) -> Option<String> {
    pdf_path.map(|p| format!("{}{}", OPENREVIEW_BASE, p))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// 指定されたICLR年度のすべての採択論文をOpenReview APIから取得する．
pub async fn fetch_papers_for_year(
    client: &reqwest::Client,
    session: &Session,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let result = match year {
        2018 => fetch_v1_legacy_2018(client, session, API_V1_BASE, interval).await,
        2019 => fetch_v1_legacy_2019(client, session, API_V1_BASE, interval).await,
        2020..=2023 => fetch_v1(client, session, API_V1_BASE, year, interval).await,
        2024.. => fetch_v2(client, session, API_V2_BASE, year, interval).await,
        _ => bail!("ICLR scraping for year {} is not supported", year),
    };
    with_sign_in_hint(result, session)
}

/// 匿名で確認画面に当たったときだけ，ログインの設定を促す一文を添える．
fn with_sign_in_hint<T>(result: Result<T>, session: &Session) -> Result<T> {
    match result {
        Err(e) if session.is_anonymous() && format!("{:#}", e).contains("bot challenge") => {
            Err(e.context(SIGN_IN_HINT))
        }
        other => other,
    }
}

/// OpenReview の API を 1 回叩く．ログインしていればトークンを付ける．
async fn get(
    client: &reqwest::Client,
    session: &Session,
    base: &str,
    url: &str,
    interval: Duration,
) -> Result<String> {
    let bearer = session.bearer(client, base).await?;
    crate::scraper::fetch_with_bearer(client, url, interval, bearer.as_deref()).await
}

/// ICLR 2018の採択論文を取得する．決定ノートと投稿を別々に取得し，マッチングする．
async fn fetch_v1_legacy_2018(
    client: &reqwest::Client,
    session: &Session,
    base: &str,
    interval: Duration,
) -> Result<Vec<Paper>> {
    // Step 1: Fetch all decision notes
    let mut decisions: HashMap<String, String> = HashMap::new();
    let mut offset: u64 = 0;

    loop {
        let url = format!(
            "{}/notes?invitation=ICLR.cc/2018/Conference/-/Acceptance_Decision&limit={}&offset={}",
            base, PAGE_LIMIT, offset
        );
        tracing::debug!("GET {}", url);

        let resp = get(client, session, base, &url, interval).await?;
        let body: DecisionResponse = serde_json::from_str(&resp)?;

        let total = body.count.unwrap_or(0);
        let page_len = body.notes.len() as u64;

        for note in body.notes {
            let decision = &note.content.decision;
            if decision.starts_with("Accept") || decision.contains("Workshop") {
                decisions.insert(note.forum, decision.clone());
            }
        }

        offset += page_len;
        if page_len == 0 || offset >= total {
            break;
        }
    }

    // Step 2: Fetch all blind submissions
    let mut papers = Vec::new();
    offset = 0;

    loop {
        let url = format!(
            "{}/notes?invitation=ICLR.cc/2018/Conference/-/Blind_Submission&limit={}&offset={}",
            base, PAGE_LIMIT, offset
        );
        tracing::debug!("GET {}", url);

        let resp = get(client, session, base, &url, interval).await?;
        let body: ApiResponseV1 = serde_json::from_str(&resp)?;

        let total = body.count.unwrap_or(0);
        let page_len = body.notes.len() as u64;

        for note in body.notes {
            if let Some(decision) = decisions.get(&note.forum) {
                let title = note.content.title;
                let authors = note.content.authors.unwrap_or_default();
                let abstract_text = note.content.r#abstract.unwrap_or_default();
                let pdf_path = note.content.pdf;
                let category = extract_legacy_category(decision);

                papers.push(Paper {
                    id: compute_id(&title),
                    conference: "iclr".to_string(),
                    year: 2018,
                    title: title.clone(),
                    authors,
                    r#abstract: abstract_text.clone(),
                    url: build_paper_url(&note.forum),
                    pdf_url: build_pdf_url(pdf_path.as_deref()),
                    categories: vec![category],
                    hash: compute_hash(&title, &abstract_text),
                });
            }
        }

        offset += page_len;
        if page_len == 0 || offset >= total {
            break;
        }
    }

    Ok(papers)
}

/// ICLR 2019の採択論文を取得する．投稿のdirectRepliesからMeta_Reviewを探して判定する．
async fn fetch_v1_legacy_2019(
    client: &reqwest::Client,
    session: &Session,
    base: &str,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let mut papers = Vec::new();
    let mut offset: u64 = 0;

    loop {
        let url = format!(
            "{}/notes?invitation=ICLR.cc/2019/Conference/-/Blind_Submission&details=directReplies&limit={}&offset={}",
            base, PAGE_LIMIT, offset
        );
        tracing::debug!("GET {}", url);

        let resp = get(client, session, base, &url, interval).await?;
        let body: ApiResponseV1WithDetails = serde_json::from_str(&resp)?;

        let total = body.count.unwrap_or(0);
        let page_len = body.notes.len() as u64;

        for note in body.notes {
            if let Some(details) = &note.details {
                let meta_review = details
                    .direct_replies
                    .iter()
                    .find(|r| r.invitation.ends_with("/Meta_Review"));

                if let Some(review) = meta_review
                    && let Some(ref rec) = review.content.recommendation
                    && rec.starts_with("Accept")
                {
                    let title = note.content.title.clone();
                    let authors = note.content.authors.clone().unwrap_or_default();
                    let abstract_text = note.content.r#abstract.clone().unwrap_or_default();
                    let pdf_path = note.content.pdf.clone();
                    let category = extract_legacy_category(rec);

                    papers.push(Paper {
                        id: compute_id(&title),
                        conference: "iclr".to_string(),
                        year: 2019,
                        title: title.clone(),
                        authors,
                        r#abstract: abstract_text.clone(),
                        url: build_paper_url(&note.forum),
                        pdf_url: build_pdf_url(pdf_path.as_deref()),
                        categories: vec![category],
                        hash: compute_hash(&title, &abstract_text),
                    });
                }
            }
        }

        offset += page_len;
        if page_len == 0 || offset >= total {
            break;
        }
    }

    Ok(papers)
}

async fn fetch_v2(
    client: &reqwest::Client,
    session: &Session,
    base: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let venue_id = format!("ICLR.cc/{}/Conference", year);
    let mut papers = Vec::new();
    let mut offset: u64 = 0;

    loop {
        let url = format!(
            "{}/notes?content.venueid={}&limit={}&offset={}",
            base, venue_id, PAGE_LIMIT, offset
        );
        tracing::debug!("GET {}", url);

        // 429 の待機とボット確認画面の検出を通す（MYTASK-3422）．間隔もここであく
        let resp = get(client, session, base, &url, interval).await?;
        let body: ApiResponseV2 = serde_json::from_str(&resp)?;

        let total = body.count.unwrap_or(0);
        let page_len = body.notes.len() as u64;

        for note in body.notes {
            let title = note.content.title.value;
            let authors = note.content.authors.map(|a| a.value).unwrap_or_default();
            let abstract_text = note.content.r#abstract.map(|a| a.value).unwrap_or_default();
            let pdf_path = note.content.pdf.map(|p| p.value);
            let venue = note.content.venue.map(|v| v.value);

            let category = extract_category(venue.as_deref());

            papers.push(Paper {
                id: compute_id(&title),
                conference: "iclr".to_string(),
                year,
                title: title.clone(),
                authors,
                r#abstract: abstract_text.clone(),
                url: build_paper_url(&note.forum),
                pdf_url: build_pdf_url(pdf_path.as_deref()),
                categories: vec![category],
                hash: compute_hash(&title, &abstract_text),
            });
        }

        offset += page_len;
        if page_len == 0 || offset >= total {
            break;
        }
    }

    Ok(papers)
}

async fn fetch_v1(
    client: &reqwest::Client,
    session: &Session,
    base: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let venue_id = format!("ICLR.cc/{}/Conference", year);
    let mut papers = Vec::new();
    let mut offset: u64 = 0;

    loop {
        let url = format!(
            "{}/notes?content.venueid={}&limit={}&offset={}",
            base, venue_id, PAGE_LIMIT, offset
        );
        tracing::debug!("GET {}", url);

        // 429 の待機とボット確認画面の検出を通す（MYTASK-3422）．間隔もここであく
        let resp = get(client, session, base, &url, interval).await?;
        let body: ApiResponseV1 = serde_json::from_str(&resp)?;

        let total = body.count.unwrap_or(0);
        let page_len = body.notes.len() as u64;

        for note in body.notes {
            let title = note.content.title;
            let authors = note.content.authors.unwrap_or_default();
            let abstract_text = note.content.r#abstract.unwrap_or_default();
            let pdf_path = note.content.pdf;
            let venue = note.content.venue;

            let category = extract_category(venue.as_deref());

            papers.push(Paper {
                id: compute_id(&title),
                conference: "iclr".to_string(),
                year,
                title: title.clone(),
                authors,
                r#abstract: abstract_text.clone(),
                url: build_paper_url(&note.forum),
                pdf_url: build_pdf_url(pdf_path.as_deref()),
                categories: vec![category],
                hash: compute_hash(&title, &abstract_text),
            });
        }

        offset += page_len;
        if page_len == 0 || offset >= total {
            break;
        }
    }

    Ok(papers)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::auth::Credentials;
    use super::*;

    /// 429（Retry-After あり）のあと成功する応答で，取り直して読めること（MYTASK-3422）
    #[tokio::test]
    async fn fetch_v2_retries_after_429() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/notes"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        let body = r#"{"notes": [{"id": "a1", "forum": "a1", "content": {
            "title": {"value": "A Great Paper"}, "authors": {"value": ["Alice"]},
            "abstract": {"value": "Great."}, "venue": {"value": "ICLR 2024 poster"}}}],
            "count": 1}"#;
        Mock::given(method("GET"))
            .and(path("/notes"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let papers = fetch_v2(
            &client,
            &Session::new(Credentials::Anonymous),
            &server.uri(),
            2024,
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].title, "A Great Paper");
        assert_eq!(papers[0].r#abstract, "Great.");
    }

    /// ボットの確認画面は再試行せず，確認画面だと分かるエラーにする（MYTASK-3422）
    #[tokio::test]
    async fn fetch_v1_reports_bot_challenge() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/notes"))
            .respond_with(ResponseTemplate::new(403).set_body_string(
                "<!DOCTYPE html><html><head><title>Just a moment...</title></head></html>",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = fetch_v1(
            &client,
            &Session::new(Credentials::Anonymous),
            &server.uri(),
            2022,
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(
            format!("{:#}", err).contains("blocked by bot challenge"),
            "{:#}",
            err
        );
    }

    /// ログインしてトークンを得て，各ページの取得に Bearer を付けること（MYTASK-3426）
    #[tokio::test]
    async fn fetch_v2_signs_in_and_sends_bearer() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"token": "T1"}"#))
            .expect(1)
            .mount(&server)
            .await;
        let page = |title: &str| {
            format!(
                r#"{{"notes": [{{"id": "x", "forum": "x", "content": {{
                "title": {{"value": "{}"}}, "venue": {{"value": "ICLR 2026 Oral"}}}}}}],
                "count": 2}}"#,
                title
            )
        };
        Mock::given(method("GET"))
            .and(path("/notes"))
            .and(header("authorization", "Bearer T1"))
            .and(wiremock::matchers::query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page("First")))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/notes"))
            .and(header("authorization", "Bearer T1"))
            .and(wiremock::matchers::query_param("offset", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page("Second")))
            .expect(1)
            .mount(&server)
            .await;

        let session = Session::new(Credentials::Login {
            username: "u".into(),
            password: "p".into(),
        });
        let papers = fetch_v2(
            &reqwest::Client::new(),
            &session,
            &server.uri(),
            2026,
            Duration::ZERO,
        )
        .await
        .unwrap();
        let titles: Vec<_> = papers.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["First", "Second"]);
        assert_eq!(papers[0].categories, ["oral"]);
    }

    /// トークン直指定ならログインせず，そのトークンを付けること
    #[tokio::test]
    async fn fetch_v1_uses_the_given_token() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/notes"))
            .and(header("authorization", "Bearer TOKEN-X"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"notes": [{"id": "a", "forum": "a", "content": {"title": "Old Paper",
                "venue": "ICLR 2022 Poster"}}], "count": 1}"#,
            ))
            .expect(1)
            .mount(&server)
            .await;

        let session = Session::new(Credentials::Token("TOKEN-X".into()));
        let papers = fetch_v1(
            &reqwest::Client::new(),
            &session,
            &server.uri(),
            2022,
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(papers.len(), 1);
    }

    /// 匿名で確認画面に当たったら，ログインの設定を促す．ログイン済みなら促さない
    #[test]
    fn sign_in_hint_only_for_anonymous_challenges() {
        let challenge = || -> Result<()> {
            Err(anyhow::anyhow!(
                "blocked by bot challenge (OpenReview, HTTP 403) at x"
            ))
        };
        let anon = Session::new(Credentials::Anonymous);
        let msg = format!("{:#}", with_sign_in_hint(challenge(), &anon).unwrap_err());
        assert!(msg.contains("OPENREVIEW_USERNAME"), "{}", msg);
        assert!(msg.contains("blocked by bot challenge"), "{}", msg);

        let signed_in = Session::new(Credentials::Token("t".into()));
        let msg = format!(
            "{:#}",
            with_sign_in_hint(challenge(), &signed_in).unwrap_err()
        );
        assert!(!msg.contains("OPENREVIEW_USERNAME"), "{}", msg);

        let other = with_sign_in_hint::<()>(Err(anyhow::anyhow!("HTTP 500 at x")), &anon);
        assert!(!format!("{:#}", other.unwrap_err()).contains("OPENREVIEW"));
    }

    #[test]
    fn test_parse_v2_response() {
        let json = r#"{
            "notes": [{
                "id": "abc123",
                "forum": "abc123",
                "content": {
                    "title": {"value": "A Great Paper"},
                    "authors": {"value": ["Alice", "Bob"]},
                    "abstract": {"value": "This paper does great things."},
                    "pdf": {"value": "/pdf/abc123.pdf"},
                    "venue": {"value": "ICLR 2024 poster"},
                    "venueid": {"value": "ICLR.cc/2024/Conference"}
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV2 = serde_json::from_str(json).unwrap();
        assert_eq!(resp.notes.len(), 1);
        assert_eq!(resp.count, Some(1));

        let note = &resp.notes[0];
        assert_eq!(note.content.title.value, "A Great Paper");
        assert_eq!(
            note.content.authors.as_ref().unwrap().value,
            vec!["Alice".to_string(), "Bob".to_string()]
        );
        assert_eq!(
            note.content.r#abstract.as_ref().unwrap().value,
            "This paper does great things."
        );
        assert_eq!(note.content.pdf.as_ref().unwrap().value, "/pdf/abc123.pdf");
        assert_eq!(
            note.content.venue.as_ref().unwrap().value,
            "ICLR 2024 poster"
        );
    }

    #[test]
    fn test_parse_v1_response() {
        let json = r#"{
            "notes": [{
                "id": "xyz789",
                "forum": "xyz789",
                "content": {
                    "title": "Another Paper",
                    "authors": ["Charlie", "Diana"],
                    "abstract": "Abstract text here.",
                    "pdf": "/pdf/xyz789.pdf",
                    "venue": "ICLR 2022 Oral"
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV1 = serde_json::from_str(json).unwrap();
        assert_eq!(resp.notes.len(), 1);
        assert_eq!(resp.count, Some(1));

        let note = &resp.notes[0];
        assert_eq!(note.content.title, "Another Paper");
        assert_eq!(
            note.content.authors.as_ref().unwrap(),
            &vec!["Charlie".to_string(), "Diana".to_string()]
        );
        assert_eq!(
            note.content.r#abstract.as_ref().unwrap(),
            "Abstract text here."
        );
        assert_eq!(note.content.pdf.as_ref().unwrap(), "/pdf/xyz789.pdf");
    }

    #[test]
    fn test_parse_v2_missing_optional_fields() {
        let json = r#"{
            "notes": [{
                "id": "minimal",
                "forum": "minimal",
                "content": {
                    "title": {"value": "Minimal Paper"}
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV2 = serde_json::from_str(json).unwrap();
        let note = &resp.notes[0];
        assert_eq!(note.content.title.value, "Minimal Paper");
        assert!(note.content.authors.is_none());
        assert!(note.content.r#abstract.is_none());
        assert!(note.content.pdf.is_none());
        assert!(note.content.venue.is_none());
    }

    #[test]
    fn test_parse_v1_missing_optional_fields() {
        let json = r#"{
            "notes": [{
                "id": "min1",
                "forum": "min1",
                "content": {
                    "title": "Title Only"
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV1 = serde_json::from_str(json).unwrap();
        let note = &resp.notes[0];
        assert_eq!(note.content.title, "Title Only");
        assert!(note.content.authors.is_none());
        assert!(note.content.r#abstract.is_none());
        assert!(note.content.pdf.is_none());
        assert!(note.content.venue.is_none());
    }

    #[test]
    fn test_pagination_count_exceeds_limit() {
        // Verify that count field is parsed so pagination logic can use it
        let json = r#"{
            "notes": [],
            "count": 2500
        }"#;

        let resp: ApiResponseV2 = serde_json::from_str(json).unwrap();
        assert_eq!(resp.count, Some(2500));
        assert!(resp.notes.is_empty());
    }

    #[test]
    fn test_paper_url_construction() {
        assert_eq!(
            build_paper_url("rhgIgTSSxW"),
            "https://openreview.net/forum?id=rhgIgTSSxW"
        );
    }

    #[test]
    fn test_pdf_url_construction() {
        assert_eq!(
            build_pdf_url(Some("/pdf/abc123.pdf")),
            Some("https://openreview.net/pdf/abc123.pdf".to_string())
        );
        assert_eq!(build_pdf_url(None), None);
    }

    #[test]
    fn test_extract_category() {
        assert_eq!(extract_category(Some("ICLR 2024 poster")), "poster");
        assert_eq!(extract_category(Some("ICLR 2024 oral")), "oral");
        assert_eq!(extract_category(Some("ICLR 2024 Spotlight")), "spotlight");
        assert_eq!(extract_category(Some("ICLR 2024")), "Conference");
        assert_eq!(extract_category(None), "Conference");
    }

    #[test]
    fn test_compute_hash() {
        let h1 = compute_hash("Title A", "Abstract A");
        let h2 = compute_hash("Title A", "Abstract A");
        let h3 = compute_hash("Title B", "Abstract B");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64); // SHA256 hex length
    }

    #[test]
    fn test_extract_legacy_category() {
        assert_eq!(extract_legacy_category("Accept (Oral)"), "oral");
        assert_eq!(extract_legacy_category("Accept (Poster)"), "poster");
        assert_eq!(
            extract_legacy_category("Invite to Workshop Track"),
            "workshop"
        );
        // Fallback for unknown accept format
        assert_eq!(extract_legacy_category("Accept"), "poster");
        assert_eq!(extract_legacy_category("Accept (Spotlight)"), "poster");
    }

    #[test]
    fn test_parse_2018_decision_notes() {
        let json = r#"{
            "notes": [
                {
                    "forum": "paper1",
                    "content": { "decision": "Accept (Oral)" }
                },
                {
                    "forum": "paper2",
                    "content": { "decision": "Accept (Poster)" }
                },
                {
                    "forum": "paper3",
                    "content": { "decision": "Reject" }
                },
                {
                    "forum": "paper4",
                    "content": { "decision": "Invite to Workshop Track" }
                }
            ],
            "count": 4
        }"#;

        let resp: DecisionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.notes.len(), 4);
        assert_eq!(resp.count, Some(4));
        assert_eq!(resp.notes[0].forum, "paper1");
        assert_eq!(resp.notes[0].content.decision, "Accept (Oral)");
        assert_eq!(resp.notes[2].content.decision, "Reject");
    }

    #[test]
    fn test_parse_2019_with_direct_replies() {
        let json = r#"{
            "notes": [{
                "id": "sub1",
                "forum": "sub1",
                "content": {
                    "title": "Accepted Paper 2019",
                    "authors": ["Eve", "Frank"],
                    "abstract": "A 2019 abstract.",
                    "pdf": "/pdf/sub1.pdf"
                },
                "details": {
                    "directReplies": [
                        {
                            "invitation": "ICLR.cc/2019/Conference/Paper123/-/Meta_Review",
                            "content": {
                                "recommendation": "Accept (Oral)"
                            }
                        },
                        {
                            "invitation": "ICLR.cc/2019/Conference/Paper123/-/Official_Review",
                            "content": {}
                        }
                    ]
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV1WithDetails = serde_json::from_str(json).unwrap();
        assert_eq!(resp.notes.len(), 1);

        let note = &resp.notes[0];
        assert_eq!(note.content.title, "Accepted Paper 2019");

        let details = note.details.as_ref().unwrap();
        assert_eq!(details.direct_replies.len(), 2);

        let meta_review = details
            .direct_replies
            .iter()
            .find(|r| r.invitation.ends_with("/Meta_Review"))
            .unwrap();
        assert_eq!(
            meta_review.content.recommendation.as_ref().unwrap(),
            "Accept (Oral)"
        );
    }

    #[test]
    fn test_parse_2019_no_meta_review() {
        let json = r#"{
            "notes": [{
                "id": "sub2",
                "forum": "sub2",
                "content": {
                    "title": "Paper Without Meta Review",
                    "authors": ["Grace"]
                },
                "details": {
                    "directReplies": [
                        {
                            "invitation": "ICLR.cc/2019/Conference/Paper456/-/Official_Review",
                            "content": {}
                        }
                    ]
                }
            }],
            "count": 1
        }"#;

        let resp: ApiResponseV1WithDetails = serde_json::from_str(json).unwrap();
        let note = &resp.notes[0];
        let details = note.details.as_ref().unwrap();

        let meta_review = details
            .direct_replies
            .iter()
            .find(|r| r.invitation.ends_with("/Meta_Review"));
        assert!(meta_review.is_none());
    }

    #[test]
    fn test_parse_2018_rejected_paper_skipped() {
        // Simulate the decision-filtering logic: only "Accept" decisions are included
        let decisions_json = r#"{
            "notes": [
                { "forum": "acc1", "content": { "decision": "Accept (Poster)" } },
                { "forum": "rej1", "content": { "decision": "Reject" } }
            ],
            "count": 2
        }"#;

        let resp: DecisionResponse = serde_json::from_str(decisions_json).unwrap();
        let mut decisions: HashMap<String, String> = HashMap::new();
        for note in resp.notes {
            if note.content.decision.starts_with("Accept")
                || note.content.decision.contains("Workshop")
            {
                decisions.insert(note.forum, note.content.decision);
            }
        }

        assert!(decisions.contains_key("acc1"));
        assert!(!decisions.contains_key("rej1"));
        assert_eq!(decisions.len(), 1);
    }
}
