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
            ("per_page", "1"),
        ],
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
        tracing::debug!("OpenAlex returned status {} for '{}'", resp.status(), title);
        return Ok(String::new());
    }

    let body: Value = resp.json().await?;

    let results = body.get("results").and_then(|v| v.as_array());

    let item = match results.and_then(|arr| arr.first()) {
        Some(item) => item,
        None => return Ok(String::new()),
    };

    // abstract_inverted_index の復元
    let inverted = match item.get("abstract_inverted_index") {
        Some(Value::Object(map)) => map,
        _ => return Ok(String::new()),
    };

    let mut pairs: Vec<(u64, &str)> = Vec::new();
    for (word, positions) in inverted.iter() {
        if let Some(arr) = positions.as_array() {
            for pos in arr {
                if let Some(p) = pos.as_u64() {
                    pairs.push((p, word.as_str()));
                }
            }
        }
    }

    if pairs.is_empty() {
        return Ok(String::new());
    }

    pairs.sort_by_key(|(pos, _)| *pos);
    let abstract_text: String = pairs.iter().map(|(_, w)| *w).collect::<Vec<_>>().join(" ");

    Ok(abstract_text)
}
