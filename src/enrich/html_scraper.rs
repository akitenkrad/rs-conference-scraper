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

    Ok(match extract_abstract_from_html(&final_url, &body) {
        Some(text) => HtmlResult::Direct(text),
        None => HtmlResult::Empty,
    })
}

/// 取得済みの HTML から abstract を抽出する（HTTP を伴わない部分）
fn extract_abstract_from_html(final_url: &str, body: &str) -> Option<String> {
    let document = Html::parse_document(body);

    // Step 0: サイト固有パーサを試行
    if let Some(abstract_text) =
        site_parsers::try_site_specific_extraction(final_url, body, &document)
    {
        let trimmed = abstract_text.trim().to_string();
        if trimmed.len() > 50 {
            return Some(trimmed);
        }
    }

    // サイト固有パーサが正とされるサイトでは汎用抽出に進まない
    // （NDSS 旧ページの og:description は著者・Download 等のメタデータで，要旨ではない）
    if site_parsers::is_site_parser_authoritative(final_url) {
        return None;
    }

    // Step 1: 直接抽出を試みる
    try_direct_extraction(&document)
        .map(|abstract_text| abstract_text.trim().to_string())
        .filter(|trimmed| !trimmed.is_empty())
}

/// HTMLから直接abstractを抽出する
fn try_direct_extraction(document: &Html) -> Option<String> {
    // 1. class/idに"abstract"を含む要素（最も信頼性が高い）
    if let Some(text) = extract_abstract_element(document)
        && text.len() > 50
    {
        return Some(text);
    }

    // 2. <meta name="description"> or <meta property="og:description">
    if let Some(text) = extract_meta_description(document)
        && text.len() > 50
    {
        return Some(text);
    }

    // 3. "Abstract"見出しの後の<p>や<blockquote>
    if let Some(text) = extract_abstract_after_heading(document)
        && text.len() > 50
    {
        return Some(text);
    }

    None
}

/// metaタグからdescriptionを取得
fn extract_meta_description(document: &Html) -> Option<String> {
    // <meta name="description" content="...">
    let sel = Selector::parse(r#"meta[name="description"]"#).ok()?;
    if let Some(el) = document.select(&sel).next()
        && let Some(content) = el.value().attr("content")
    {
        return Some(content.to_string());
    }

    // <meta property="og:description" content="...">
    let sel = Selector::parse(r#"meta[property="og:description"]"#).ok()?;
    if let Some(el) = document.select(&sel).next()
        && let Some(content) = el.value().attr("content")
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// NDSS 2005 の旧詳細ページ（要旨なし．og:description はメタデータ）
    const NDSS_LEGACY_NO_ABSTRACT: &str = r#"<html><head>
<meta property="og:description" content="Author(s): Alina Oprea , Michael K. Reiter, Ke Yang Download: Paper (PDF) Date: 3 Feb 2005 Document Type: Reports Associated Event: NDSS Symposium 2005" />
</head><body><div class="entry-content">
<p><strong>Author(s):</strong> Alina Oprea , Michael K. Reiter, Ke Yang</p>
<p><strong>Download:</strong> <a href="https://www.ndss-symposium.org/wp-content/uploads/2017/09/Space-Efficient-Block-Storage-Integrity-Alina-Oprea.pdf">Paper (PDF)</a></p>
</div></body></html>"#;

    #[test]
    fn test_ndss_legacy_page_without_abstract_does_not_fall_back_to_meta() {
        let url =
            "https://www.ndss-symposium.org/ndss2005/space-efficient-block-storage-integrity/";
        assert_eq!(
            extract_abstract_from_html(url, NDSS_LEGACY_NO_ABSTRACT),
            None
        );
    }

    #[test]
    fn test_non_authoritative_site_still_uses_meta_description() {
        let url = "https://example.org/paper/1";
        let result = extract_abstract_from_html(url, NDSS_LEGACY_NO_ABSTRACT);
        assert!(result.unwrap().starts_with("Author(s): Alina Oprea"));
    }

    #[test]
    fn test_ndss_page_with_abstract_is_still_extracted() {
        let html = r#"<html><head><meta property="og:description" content="meta text that is long enough to be picked up by the generic extraction path" /></head>
<body><section class="new-wrapper"><h2>Abstract:</h2>
<p>We present a new approach to detecting malicious domains at the upper DNS hierarchy using passive traffic.</p>
<h2>Downloads</h2></section></body></html>"#;
        let url = "https://www.ndss-symposium.org/ndss2012/ndss-2012-programme/some-paper/";
        let result = extract_abstract_from_html(url, html).unwrap();
        assert!(result.starts_with("We present a new approach"));
    }
}
