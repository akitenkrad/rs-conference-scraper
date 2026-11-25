use super::fuzzy_title_match;
use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

/// CrossRef APIを使ってタイトルからabstractを取得する
///
/// レスポンスに含まれるabstractからHTMLタグを除去して返す．
pub async fn fetch_abstract_via_crossref(client: &Client, title: &str) -> Result<String> {
    let url = reqwest::Url::parse_with_params(
        "https://api.crossref.org/works",
        &[("query.title", title), ("rows", "5")],
    )?;

    let resp = client
        .get(url)
        .header("User-Agent", "conf-scraper/0.1 (mailto:user@example.com)")
        .send()
        .await?;

    if !resp.status().is_success() {
        tracing::debug!("CrossRef returned status {} for '{}'", resp.status(), title);
        return Ok(String::new());
    }

    let body: Value = resp.json().await?;

    Ok(select_crossref_abstract(&body, title))
}

/// CrossRefの検索結果からタイトルが一致する最初の要旨を選ぶ
fn select_crossref_abstract(body: &Value, title: &str) -> String {
    let Some(items) = body.pointer("/message/items").and_then(Value::as_array) else {
        return String::new();
    };

    for item in items {
        let result_title = item
            .pointer("/title/0")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if !fuzzy_title_match(title, result_title) {
            tracing::debug!(
                "CrossRef title mismatch: query='{}' result='{}'",
                title,
                result_title
            );
            continue;
        }

        let abstract_raw = item
            .get("abstract")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let abstract_text = strip_html_tags(abstract_raw);
        if !abstract_text.is_empty() {
            return abstract_text;
        }
    }

    String::new()
}

/// HTMLタグを正規表現で除去する
fn strip_html_tags(s: &str) -> String {
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    let text = re.replace_all(s, "");
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::select_crossref_abstract;
    use serde_json::json;

    const QUERY: &str = "Privacy-Preserving Sharing and Correlation of Security Alerts";

    #[test]
    fn selects_first_matching_abstract_despite_formatting() {
        let body = json!({
            "message": {
                "items": [{
                    "title": ["  PRIVACY-PRESERVING sharing, and correlation of security alerts!  "],
                    "abstract": "<jats:p>First abstract.</jats:p>"
                }]
            }
        });

        assert_eq!(select_crossref_abstract(&body, QUERY), "First abstract.");
    }

    #[test]
    fn skips_mismatched_first_item_and_selects_second() {
        let body = json!({
            "message": {
                "items": [
                    {
                        "title": ["Lessons from the Sony CD DRM Episode"],
                        "abstract": "Wrong abstract."
                    },
                    {
                        "title": [QUERY],
                        "abstract": "<p>Matching abstract.</p>"
                    }
                ]
            }
        });

        assert_eq!(select_crossref_abstract(&body, QUERY), "Matching abstract.");
    }

    #[test]
    fn returns_empty_when_no_title_matches_french_example() {
        let body = json!({
            "message": {
                "items": [{
                    "title": ["La sécurité des réseaux sans fil"],
                    "abstract": "Résumé sans rapport."
                }]
            }
        });

        assert_eq!(select_crossref_abstract(&body, QUERY), "");
    }

    #[test]
    fn skips_matching_item_without_abstract() {
        let body = json!({
            "message": {
                "items": [
                    { "title": [QUERY] },
                    {
                        "title": [QUERY],
                        "abstract": "Available abstract."
                    }
                ]
            }
        });

        assert_eq!(
            select_crossref_abstract(&body, QUERY),
            "Available abstract."
        );
    }
}
