use super::site_parsers;
use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};

/// HTMLスクレイピングの結果を示す列挙型
pub enum HtmlResult {
    /// HTML直接抽出で取得できた
    Direct(String),
    /// 取得できなかった
    Empty,
}

/// 論文のURLからHTMLを取得し，abstractを直接抽出する
///
/// 直接抽出できない場合は `NeedLlm(main_text)` を返し，
/// 呼び出し元が後でLLMティアに渡せるようにする
pub async fn fetch_abstract_via_html(
    client: &Client,
    _title: &str,
    url: &str,
) -> Result<HtmlResult> {
    // URLが空の場合はスキップ
    if url.is_empty() {
        return Ok(HtmlResult::Empty);
    }

    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("HTML fetch failed for '{}': {}", url, e);
            return Ok(HtmlResult::Empty);
        }
    };

    if !resp.status().is_success() {
        tracing::debug!("HTML fetch returned status {} for '{}'", resp.status(), url);
        return Ok(HtmlResult::Empty);
    }

    // リダイレクト後の最終URLを取得（DOI → 出版社サイトの解決用）
    let final_url = resp.url().to_string();

    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("HTML body read failed for '{}': {}", url, e);
            return Ok(HtmlResult::Empty);
        }
    };

    let document = Html::parse_document(&body);

    // Step 0: サイト固有パーサを試行
    if let Some(abstract_text) = site_parsers::try_site_specific_extraction(&final_url, &body, &document) {
        let trimmed = abstract_text.trim().to_string();
        if trimmed.len() > 50 {
            return Ok(HtmlResult::Direct(trimmed));
        }
    }

    // Step 1: 直接抽出を試みる
    if let Some(abstract_text) = try_direct_extraction(&document) {
        let trimmed = abstract_text.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(HtmlResult::Direct(trimmed));
        }
    }

    Ok(HtmlResult::Empty)
}

/// HTMLから直接abstractを抽出する
fn try_direct_extraction(document: &Html) -> Option<String> {
    // 1. class/idに"abstract"を含む要素（最も信頼性が高い）
    if let Some(text) = extract_abstract_element(document)
        && text.len() > 50 {
            return Some(text);
        }

    // 2. <meta name="description"> or <meta property="og:description">
    if let Some(text) = extract_meta_description(document)
        && text.len() > 50 {
            return Some(text);
        }

    // 3. "Abstract"見出しの後の<p>や<blockquote>
    if let Some(text) = extract_abstract_after_heading(document)
        && text.len() > 50 {
            return Some(text);
        }

    None
}

/// metaタグからdescriptionを取得
fn extract_meta_description(document: &Html) -> Option<String> {
    // <meta name="description" content="...">
    let sel = Selector::parse(r#"meta[name="description"]"#).ok()?;
    if let Some(el) = document.select(&sel).next()
        && let Some(content) = el.value().attr("content") {
            return Some(content.to_string());
        }

    // <meta property="og:description" content="...">
    let sel = Selector::parse(r#"meta[property="og:description"]"#).ok()?;
    if let Some(el) = document.select(&sel).next()
        && let Some(content) = el.value().attr("content") {
            return Some(content.to_string());
        }

    None
}

/// class/idに"abstract"を含む要素からテキストを取得
fn extract_abstract_element(document: &Html) -> Option<String> {
    let selectors = [
        r#"[class*="abstract"]"#,
        r#"[id*="abstract"]"#,
        r#"[class*="Abstract"]"#,
        r#"[id*="Abstract"]"#,
    ];

    for sel_str in &selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            for el in document.select(&sel) {
                let text: String = el.text().collect::<Vec<_>>().join(" ");
                let text = text.trim().to_string();
                // "Abstract" という見出しテキスト自体を除去
                let text = text
                    .strip_prefix("Abstract")
                    .or_else(|| text.strip_prefix("abstract"))
                    .or_else(|| text.strip_prefix("ABSTRACT"))
                    .unwrap_or(&text)
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }

    None
}

/// "Abstract"見出しの近くにある<blockquote>からテキストを取得
fn extract_abstract_after_heading(document: &Html) -> Option<String> {
    if let Ok(sel) = Selector::parse("blockquote") {
        for el in document.select(&sel) {
            let text: String = el.text().collect::<Vec<_>>().join(" ");
            let text = text.trim().to_string();
            if text.len() > 100 && text.len() < 5000 {
                return Some(text);
            }
        }
    }

    None
}

