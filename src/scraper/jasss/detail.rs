use anyhow::Result;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::types::{compute_id, Paper, PaperListEntry};

/// 論文詳細ページから Paper を構築
pub async fn fetch_paper_detail(
    client: &reqwest::Client,
    entry: &PaperListEntry,
    year: u16,
    interval: Duration,
) -> Result<Paper> {
    let body =
        crate::scraper::fetch_with_sleep(client, &entry.detail_url, interval).await?;
    parse_detail(&body, entry, year)
}

/// 詳細ページ HTML から Paper を構築．
///
/// メタデータの取得優先順位:
/// - Abstract: `DC.Abstract` → `<h3>Abstract</h3>` 後テキスト
/// - Authors:  `citation_author` → `DC.Creator` → コンテンツページ由来
/// - PDF URL:  `citation_pdf_url`
/// - Keywords: `DC.Subject`
pub fn parse_detail(html: &str, entry: &PaperListEntry, year: u16) -> Result<Paper> {
    let document = Html::parse_document(html);

    // Abstract: DC.Abstract メタタグ → HTML パース
    let r#abstract = extract_meta(&document, "DC.Abstract")
        .or_else(|| extract_abstract_from_html(&document))
        .unwrap_or_default();

    // Authors: citation_author → DC.Creator → entry.authors
    let authors = extract_citation_authors(&document)
        .or_else(|| extract_dc_creator(&document))
        .unwrap_or_else(|| entry.authors.clone());

    // PDF URL
    let pdf_url = extract_meta(&document, "citation_pdf_url");

    // Keywords → categories
    let categories = extract_meta(&document, "DC.Subject")
        .map(|s| {
            s.split(',')
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let title = extract_meta(&document, "DC.Title")
        .map(|t| clean_text(&t))
        .unwrap_or_else(|| entry.title.clone());

    let id = compute_id(&title);
    let hash = {
        let mut hasher = Sha256::new();
        hasher.update(title.as_bytes());
        hasher.update(r#abstract.as_bytes());
        format!("{:x}", hasher.finalize())
    };

    Ok(Paper {
        id,
        conference: "jasss".to_string(),
        year,
        title,
        authors,
        r#abstract,
        url: entry.detail_url.clone(),
        pdf_url,
        categories,
        hash,
    })
}

/// `<meta name="NAME" content="...">` からコンテンツを取得
fn extract_meta(document: &Html, name: &str) -> Option<String> {
    let selector_str = format!(r#"meta[name="{}"]"#, name);
    let selector = Selector::parse(&selector_str).ok()?;
    document
        .select(&selector)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(clean_text)
        .filter(|s| !s.is_empty())
}

/// `citation_author` メタタグから著者リストを取得（複数タグ）
fn extract_citation_authors(document: &Html) -> Option<Vec<String>> {
    let selector = Selector::parse(r#"meta[name="citation_author"]"#).ok()?;
    let authors: Vec<String> = document
        .select(&selector)
        .filter_map(|el| el.value().attr("content"))
        .map(clean_text)
        .filter(|s| !s.is_empty())
        .collect();
    if authors.is_empty() {
        None
    } else {
        Some(authors)
    }
}

/// `DC.Creator` メタタグから著者リストを取得
fn extract_dc_creator(document: &Html) -> Option<Vec<String>> {
    let content = extract_meta(document, "DC.Creator")?;
    let authors: Vec<String> = content
        .split(" and ")
        .flat_map(|part| part.split(','))
        .map(clean_text)
        .filter(|s| !s.is_empty())
        .collect();
    if authors.is_empty() {
        None
    } else {
        Some(authors)
    }
}

/// `<h3>Abstract</h3>` の後のテキストから abstract を抽出（旧形式）
fn extract_abstract_from_html(document: &Html) -> Option<String> {
    let h3_selector = Selector::parse("h3").ok()?;

    for h3 in document.select(&h3_selector) {
        let text = h3.text().collect::<String>();
        if !text.contains("Abstract") {
            continue;
        }

        // h3 の次の兄弟要素を探す
        let mut sibling = h3.next_sibling();
        while let Some(node) = sibling {
            if let Some(elem_ref) = scraper::ElementRef::wrap(node) {
                let tag = elem_ref.value().name();
                // <dl>, <p>, <div> のいずれかからテキストを抽出
                if tag == "dl" || tag == "p" || tag == "div" {
                    let abstract_text = elem_ref
                        .text()
                        .collect::<String>();
                    let cleaned = clean_abstract_text(&abstract_text);
                    if !cleaned.is_empty() {
                        return Some(cleaned);
                    }
                }
                // 次の <h3> に到達したら終了
                if tag == "h3" || tag == "hr" {
                    break;
                }
            }
            sibling = node.next_sibling();
        }
    }
    None
}

/// Abstract テキストからキーワード行などを除去してクリーンアップ
fn clean_abstract_text(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let mut result_parts = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        // Keywords: 行以降はスキップ
        if trimmed.starts_with("Keywords:") || trimmed.starts_with("Key words:") {
            break;
        }
        if !trimmed.is_empty() {
            result_parts.push(trimmed);
        }
    }

    clean_text(&result_parts.join(" "))
}

fn clean_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_detail_with_meta_tags() {
        let html = r#"
        <html><head>
        <meta name="DC.Title" content="Test Paper Title">
        <meta name="DC.Abstract" content="This is the abstract of the paper.">
        <meta name="DC.Subject" content="simulation, agents, modeling">
        <meta name="citation_author" content="Alice Smith"/>
        <meta name="citation_author" content="Bob Jones"/>
        <meta name="citation_pdf_url" content="https://www.jasss.org/29/1/7/7.pdf"/>
        </head><body></body></html>
        "#;

        let entry = PaperListEntry {
            title: "Test Paper Title".to_string(),
            authors: vec!["Fallback Author".to_string()],
            detail_url: "https://www.jasss.org/29/1/7.html".to_string(),
            track: None,
        };

        let paper = parse_detail(html, &entry, 2026).unwrap();

        assert_eq!(paper.title, "Test Paper Title");
        assert_eq!(paper.r#abstract, "This is the abstract of the paper.");
        assert_eq!(paper.authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(
            paper.pdf_url,
            Some("https://www.jasss.org/29/1/7/7.pdf".to_string())
        );
        assert_eq!(
            paper.categories,
            vec!["simulation", "agents", "modeling"]
        );
        assert_eq!(paper.conference, "jasss");
        assert_eq!(paper.year, 2026);
    }

    #[test]
    fn test_parse_detail_html_abstract_fallback() {
        let html = r#"
        <html><head>
        <meta name="DC.Title" content="Old Paper">
        <meta name="DC.Creator" content="Jim Doran">
        </head><body>
        <h3>Abstract</h3>
        <dl><dt><dd>
        This is the abstract text from the HTML body.
        <p><dt><b>Keywords:</b><dd>simulation, agents
        </dl>
        </body></html>
        "#;

        let entry = PaperListEntry {
            title: "Old Paper".to_string(),
            authors: vec![],
            detail_url: "https://www.jasss.org/1/1/3.html".to_string(),
            track: None,
        };

        let paper = parse_detail(html, &entry, 1998).unwrap();

        assert_eq!(paper.title, "Old Paper");
        assert_eq!(paper.r#abstract, "This is the abstract text from the HTML body.");
        assert_eq!(paper.authors, vec!["Jim Doran"]);
    }

    #[test]
    fn test_parse_detail_fallback_to_entry_authors() {
        let html = r#"<html><head></head><body></body></html>"#;

        let entry = PaperListEntry {
            title: "Paper".to_string(),
            authors: vec!["Entry Author".to_string()],
            detail_url: "https://www.jasss.org/1/1/1.html".to_string(),
            track: None,
        };

        let paper = parse_detail(html, &entry, 1998).unwrap();
        assert_eq!(paper.authors, vec!["Entry Author"]);
    }

    #[test]
    fn test_extract_meta() {
        let html = r#"<html><head><meta name="DC.Title" content="Hello World"></head></html>"#;
        let doc = Html::parse_document(html);
        assert_eq!(extract_meta(&doc, "DC.Title"), Some("Hello World".to_string()));
        assert_eq!(extract_meta(&doc, "DC.Abstract"), None);
    }
}
