use anyhow::Result;
use reqwest::Client;

/// arXiv APIを使ってタイトルからabstractを取得する
///
/// Atom XMLレスポンスから`<summary>`要素を抽出する．
/// タイトルの一致をファジーチェック（小文字化＋句読点除去）してから返す．
pub async fn fetch_abstract_via_arxiv(client: &Client, title: &str) -> Result<String> {
    let query_url = reqwest::Url::parse_with_params(
        "http://export.arxiv.org/api/query",
        &[
            ("search_query", format!("ti:{}", title).as_str()),
            ("max_results", "1"),
        ],
    )?;

    let resp = client.get(query_url).send().await?;

    if !resp.status().is_success() {
        tracing::debug!("arXiv returned status {} for '{}'", resp.status(), title);
        return Ok(String::new());
    }

    let body = resp.text().await?;
    let doc = roxmltree::Document::parse(&body)?;

    // Atom XML: <entry> の中から <title> と <summary> を取得
    let entry = match doc.descendants().find(|n| n.has_tag_name("entry")) {
        Some(e) => e,
        None => return Ok(String::new()),
    };

    let entry_title = entry
        .descendants()
        .find(|n| n.has_tag_name("title"))
        .and_then(|n| n.text())
        .unwrap_or_default();

    // ファジータイトルマッチ
    if !fuzzy_title_match(title, entry_title) {
        tracing::debug!(
            "arXiv title mismatch: query='{}' result='{}'",
            title,
            entry_title
        );
        return Ok(String::new());
    }

    let summary = entry
        .descendants()
        .find(|n| n.has_tag_name("summary"))
        .and_then(|n| n.text())
        .unwrap_or_default()
        .trim()
        .to_string();

    Ok(summary)
}

/// 小文字化＋句読点除去で2つのタイトルを比較する
fn fuzzy_title_match(a: &str, b: &str) -> bool {
    let normalize = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    normalize(a) == normalize(b)
}
