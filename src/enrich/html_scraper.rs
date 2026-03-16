use anyhow::Result;
use openai_tools::common::models::ChatModel;
use openai_tools::responses::request::Responses;
use reqwest::Client;
use scraper::{Html, Selector};

/// HTMLスクレイピングの結果を示す列挙型
pub enum HtmlResult {
    /// HTML直接抽出で取得できた
    Direct(String),
    /// LLM経由で取得できた
    Llm(String),
    /// 取得できなかった
    Empty,
}

/// 論文のURLからHTMLを取得し，abstractを抽出する
///
/// 1. HTMLから直接abstractを抽出（metaタグ，abstract要素など）
/// 2. 直接抽出できない場合，本文テキストをLLMに渡してabstractを抽出
pub async fn fetch_abstract_via_html(
    client: &Client,
    title: &str,
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

    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("HTML body read failed for '{}': {}", url, e);
            return Ok(HtmlResult::Empty);
        }
    };

    let document = Html::parse_document(&body);

    // Step 1: 直接抽出を試みる
    if let Some(abstract_text) = try_direct_extraction(&document) {
        let trimmed = abstract_text.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(HtmlResult::Direct(trimmed));
        }
    }

    // Step 2: メインテキストを抽出してLLMに渡す
    let main_text = extract_main_text(&document);
    if main_text.is_empty() {
        return Ok(HtmlResult::Empty);
    }

    // テキストを3000文字に制限
    let truncated: String = main_text.chars().take(3000).collect();

    match fetch_abstract_via_llm(title, &truncated).await {
        Ok(text) if !text.is_empty() => Ok(HtmlResult::Llm(text)),
        Ok(_) => Ok(HtmlResult::Empty),
        Err(e) => {
            tracing::debug!("LLM extraction failed for '{}': {}", title, e);
            Ok(HtmlResult::Empty)
        }
    }
}

/// HTMLから直接abstractを抽出する
fn try_direct_extraction(document: &Html) -> Option<String> {
    // 1. class/idに"abstract"を含む要素（最も信頼性が高い）
    if let Some(text) = extract_abstract_element(document) {
        if text.len() > 50 {
            return Some(text);
        }
    }

    // 2. <meta name="description"> or <meta property="og:description">
    if let Some(text) = extract_meta_description(document) {
        if text.len() > 50 {
            return Some(text);
        }
    }

    // 3. "Abstract"見出しの後の<p>や<blockquote>
    if let Some(text) = extract_abstract_after_heading(document) {
        if text.len() > 50 {
            return Some(text);
        }
    }

    None
}

/// metaタグからdescriptionを取得
fn extract_meta_description(document: &Html) -> Option<String> {
    // <meta name="description" content="...">
    let sel = Selector::parse(r#"meta[name="description"]"#).ok()?;
    if let Some(el) = document.select(&sel).next() {
        if let Some(content) = el.value().attr("content") {
            return Some(content.to_string());
        }
    }

    // <meta property="og:description" content="...">
    let sel = Selector::parse(r#"meta[property="og:description"]"#).ok()?;
    if let Some(el) = document.select(&sel).next() {
        if let Some(content) = el.value().attr("content") {
            return Some(content.to_string());
        }
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

/// ページのメインテキストを抽出する
fn extract_main_text(document: &Html) -> String {
    let body_sel = Selector::parse("body").unwrap();

    let mut texts = Vec::new();

    if let Some(body) = document.select(&body_sel).next() {
        for el in body.descendants() {
            // script/style要素の中身はスキップ
            if let Some(parent) = el.parent() {
                if let Some(parent_el) = parent.value().as_element() {
                    if parent_el.name() == "script" || parent_el.name() == "style" {
                        continue;
                    }
                }
            }
            if let Some(text) = el.value().as_text() {
                let t = text.trim();
                if !t.is_empty() {
                    texts.push(t.to_string());
                }
            }
        }
    }

    texts.join(" ")
}

/// LLMを使ってテキストからabstractを抽出する（web_searchなし）
async fn fetch_abstract_via_llm(title: &str, text: &str) -> Result<String> {
    // OPENAI_API_KEY が未設定の場合は空文字を返す
    if std::env::var("OPENAI_API_KEY").is_err() {
        tracing::debug!(
            "OPENAI_API_KEY not set, skipping LLM extraction for '{}'",
            title
        );
        return Ok(String::new());
    }

    let prompt = format!(
        "以下のWebページから学術論文のabstractを抽出してください．\
         abstractのテキストのみを返してください．\
         見つからない場合は空文字を返してください．\n\n{}",
        text
    );

    let mut client = Responses::new();
    let result = client
        .model(ChatModel::Gpt5Mini)
        .instructions(
            "あなたは学術論文のabstractを抽出するアシスタントです．\
             与えられたテキストからabstractを見つけて，そのテキストのみを返してください．",
        )
        .str_message(&prompt)
        .complete()
        .await;

    match result {
        Ok(response) => {
            if let Some(text) = response.output_text() {
                let text = text.trim().to_string();
                return Ok(text);
            }
            Ok(String::new())
        }
        Err(e) => {
            tracing::debug!("LLM extraction failed for '{}': {}", title, e);
            Ok(String::new())
        }
    }
}
