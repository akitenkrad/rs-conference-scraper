use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

/// CrossRef APIを使ってタイトルからabstractを取得する
///
/// レスポンスに含まれるabstractからHTMLタグを除去して返す．
pub async fn fetch_abstract_via_crossref(client: &Client, title: &str) -> Result<String> {
    let url = reqwest::Url::parse_with_params(
        "https://api.crossref.org/works",
        &[("query.title", title), ("rows", "1")],
    )?;

    let resp = client
        .get(url)
        .header(
            "User-Agent",
            "conf-scraper/0.1 (mailto:user@example.com)",
        )
        .send()
        .await?;

    if !resp.status().is_success() {
        tracing::debug!(
            "CrossRef returned status {} for '{}'",
            resp.status(),
            title
        );
        return Ok(String::new());
    }

    let body: Value = resp.json().await?;

    let abstract_raw = body
        .pointer("/message/items/0/abstract")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if abstract_raw.is_empty() {
        return Ok(String::new());
    }

    // HTMLタグの除去
    let abstract_text = strip_html_tags(abstract_raw);

    Ok(abstract_text)
}

/// HTMLタグを正規表現で除去する
fn strip_html_tags(s: &str) -> String {
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    let text = re.replace_all(s, "");
    text.trim().to_string()
}
