use super::fuzzy_title_match;
use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

/// OpenAlex APIを使ってタイトルからabstractを取得する
///
/// abstract_inverted_index を復元して平文テキストを返す．
pub async fn fetch_abstract_via_openalex(client: &Client, title: &str) -> Result<String> {
    let url = reqwest::Url::parse_with_params(
        "https://api.openalex.org/works",
        &[
            ("filter", format!("title.search:{}", title).as_str()),
            ("per_page", "5"),
        ],
    )?;

    let resp = client
        .get(url)
        .header("User-Agent", "conf-scraper/0.1 (mailto:user@example.com)")
        .send()
        .await?;

    if !resp.status().is_success() {
        tracing::debug!("OpenAlex returned status {} for '{}'", resp.status(), title);
        return Ok(String::new());
    }

    let body: Value = resp.json().await?;

    Ok(select_openalex_abstract(&body, title))
}

/// OpenAlexの検索結果からタイトルが一致する最初の要旨を選ぶ
fn select_openalex_abstract(body: &Value, title: &str) -> String {
    let Some(results) = body.get("results").and_then(Value::as_array) else {
        return String::new();
    };

    for item in results {
        let result_title = item
            .get("title")
            .and_then(Value::as_str)
            .or_else(|| item.get("display_name").and_then(Value::as_str))
            .unwrap_or_default();

        if !fuzzy_title_match(title, result_title) {
            tracing::debug!(
                "OpenAlex title mismatch: query='{}' result='{}'",
                title,
                result_title
            );
            continue;
        }

        // abstract_inverted_index の復元
        let Some(Value::Object(inverted)) = item.get("abstract_inverted_index") else {
            continue;
        };

        let mut pairs: Vec<(u64, &str)> = Vec::new();
        for (word, positions) in inverted {
            if let Some(positions) = positions.as_array() {
                for position in positions {
                    if let Some(position) = position.as_u64() {
                        pairs.push((position, word.as_str()));
                    }
                }
            }
        }

        if pairs.is_empty() {
            continue;
        }

        pairs.sort_by_key(|(position, _)| *position);
        return pairs
            .iter()
            .map(|(_, word)| *word)
            .collect::<Vec<_>>()
            .join(" ");
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::select_openalex_abstract;
    use serde_json::json;

    const QUERY: &str = "Privacy-Preserving Sharing and Correlation of Security Alerts";

    #[test]
    fn selects_first_matching_abstract_despite_formatting() {
        let body = json!({
            "results": [{
                "title": "  PRIVACY-PRESERVING sharing, and correlation of security alerts!  ",
                "abstract_inverted_index": {
                    "First": [0],
                    "abstract.": [1]
                }
            }]
        });

        assert_eq!(select_openalex_abstract(&body, QUERY), "First abstract.");
    }

    #[test]
    fn skips_mismatched_first_item_and_selects_second() {
        let body = json!({
            "results": [
                {
                    "title": "Lessons from the Sony CD DRM Episode",
                    "abstract_inverted_index": { "Wrong": [0], "abstract.": [1] }
                },
                {
                    "display_name": QUERY,
                    "abstract_inverted_index": { "Matching": [0], "abstract.": [1] }
                }
            ]
        });

        assert_eq!(select_openalex_abstract(&body, QUERY), "Matching abstract.");
    }

    #[test]
    fn returns_empty_when_no_title_matches_french_example() {
        let body = json!({
            "results": [{
                "title": "La sécurité des réseaux sans fil",
                "abstract_inverted_index": { "Résumé": [0], "sans": [1], "rapport.": [2] }
            }]
        });

        assert_eq!(select_openalex_abstract(&body, QUERY), "");
    }

    #[test]
    fn skips_matching_item_without_abstract() {
        let body = json!({
            "results": [
                { "title": QUERY },
                {
                    "title": QUERY,
                    "abstract_inverted_index": { "Available": [0], "abstract.": [1] }
                }
            ]
        });

        assert_eq!(
            select_openalex_abstract(&body, QUERY),
            "Available abstract."
        );
    }
}
