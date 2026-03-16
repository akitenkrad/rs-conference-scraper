use anyhow::{Context, Result};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{compute_id, Paper};

use super::api::{clean_title, compute_hash};

// ---------------------------------------------------------------------------
// DBLP XML Proceedings API（Search API のフォールバック）
// ---------------------------------------------------------------------------

/// DBLP XML Proceedings API から指定年の論文を取得
///
/// エンドポイント: `https://dblp.org/db/conf/{venue}/{venue}{year}.xml`
/// Search API が 500 エラーを返す場合のフォールバックとして使用
pub async fn fetch_papers_xml(
    client: &reqwest::Client,
    dblp_key: &str,
    conf_id: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let url = format!(
        "https://dblp.org/db/conf/{}/{}{}.xml",
        dblp_key, dblp_key, year
    );

    let body = fetch_with_sleep(client, &url, interval)
        .await
        .with_context(|| format!("Failed to fetch XML proceedings for {} {}", conf_id, year))?;

    parse_bht_xml(&body, conf_id)
}

/// BHT XML をパースして Paper のベクタを返す
fn parse_bht_xml(xml: &str, conf_id: &str) -> Result<Vec<Paper>> {
    let doc = roxmltree::Document::parse(xml)
        .with_context(|| "Failed to parse DBLP BHT XML")?;

    let mut papers = Vec::new();

    for node in doc.descendants() {
        if node.tag_name().name() != "inproceedings" {
            continue;
        }

        if let Some(paper) = parse_inproceedings(&node, conf_id) {
            papers.push(paper);
        }
    }

    Ok(papers)
}

/// `<inproceedings>` 要素から Paper を生成
fn parse_inproceedings(node: &roxmltree::Node, conf_id: &str) -> Option<Paper> {
    let mut title = String::new();
    let mut authors = Vec::new();
    let mut year: u16 = 0;
    let mut ee_url = String::new();
    let mut ee_found = false;

    for child in node.children() {
        match child.tag_name().name() {
            "title" => {
                // タイトルはテキストコンテンツから取得
                let raw = collect_text(&child);
                title = clean_title(&raw);
            }
            "author" => {
                let author_name = collect_text(&child);
                if !author_name.is_empty() {
                    authors.push(author_name);
                }
            }
            "year" => {
                if let Some(text) = child.text() {
                    year = text.trim().parse().unwrap_or(0);
                }
            }
            "ee" => {
                // 最初の <ee> のみ使用
                if !ee_found
                    && let Some(text) = child.text() {
                        ee_url = text.trim().to_string();
                        ee_found = true;
                    }
            }
            _ => {}
        }
    }

    if title.is_empty() {
        return None;
    }

    let abstract_text = String::new(); // DBLP はアブストラクトを提供しない

    Some(Paper {
        id: compute_id(&title),
        conference: conf_id.to_string(),
        year,
        title: title.clone(),
        authors,
        r#abstract: abstract_text.clone(),
        url: ee_url,
        pdf_url: None,
        categories: vec![],
        hash: compute_hash(&title, &abstract_text),
    })
}

/// ノードの全テキストコンテンツを再帰的に収集
fn collect_text(node: &roxmltree::Node) -> String {
    let mut text = String::new();
    for child in node.children() {
        if child.is_text() {
            text.push_str(child.text().unwrap_or(""));
        } else {
            // <sub>, <sup>, <i> などのインライン要素内のテキストも収集
            text.push_str(&collect_text(&child));
        }
    }
    text
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BHT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<bht key="db/conf/sp/sp2024.bht" title="SP 2024">
<h1>45th SP 2024: San Francisco, CA, USA</h1>
<dblpcites>
<r><proceedings key="conf/sp/2024" mdate="2025-01-01">
<editor pid="111/222">Jane Editor</editor>
<title>Proceedings of SP 2024</title>
<booktitle>SP</booktitle>
<year>2024</year>
</proceedings></r>
<r style="ee"><inproceedings key="conf/sp/NahapetyanPCOLKR24" mdate="2026-02-01">
<author orcid="0000-0001-2345-6789" pid="331/2482">Aleksandr Nahapetyan</author>
<author pid="252/6374">Sathvik Prasad</author>
<title>On SMS Phishing Tactics and Infrastructure.</title>
<pages>1-16</pages>
<year>2024</year>
<booktitle>SP</booktitle>
<ee>https://doi.org/10.1109/SP54263.2024.00169</ee>
<crossref>conf/sp/2024</crossref>
<url>db/conf/sp/sp2024.html#NahapetyanPCOLKR24</url>
<stream>streams/conf/sp</stream>
</inproceedings></r>
<r style="ee"><inproceedings key="conf/sp/SmithDoe24" mdate="2026-02-01">
<author pid="100/200">Alice Smith</author>
<title>A Study on Zero-Knowledge Proofs.</title>
<pages>17-32</pages>
<year>2024</year>
<booktitle>SP</booktitle>
<ee>https://doi.org/10.1109/SP54263.2024.00170</ee>
<ee>https://example.com/alternate</ee>
<crossref>conf/sp/2024</crossref>
<url>db/conf/sp/sp2024.html#SmithDoe24</url>
<stream>streams/conf/sp</stream>
</inproceedings></r>
</dblpcites>
</bht>"#;

    #[test]
    fn test_parse_bht_xml_basic() {
        let papers = parse_bht_xml(SAMPLE_BHT_XML, "sp").unwrap();

        // proceedings 要素はスキップされ，inproceedings のみがパースされる
        assert_eq!(papers.len(), 2);
    }

    #[test]
    fn test_parse_bht_xml_first_paper() {
        let papers = parse_bht_xml(SAMPLE_BHT_XML, "sp").unwrap();
        let p = &papers[0];

        assert_eq!(p.title, "On SMS Phishing Tactics and Infrastructure");
        assert_eq!(p.conference, "sp");
        assert_eq!(p.year, 2024);
        assert_eq!(
            p.authors,
            vec!["Aleksandr Nahapetyan", "Sathvik Prasad"]
        );
        assert_eq!(p.url, "https://doi.org/10.1109/SP54263.2024.00169");
        assert!(p.pdf_url.is_none());
        assert!(p.r#abstract.is_empty());
        assert_eq!(p.id, compute_id("On SMS Phishing Tactics and Infrastructure"));
    }

    #[test]
    fn test_parse_bht_xml_single_author() {
        let papers = parse_bht_xml(SAMPLE_BHT_XML, "sp").unwrap();
        let p = &papers[1];

        assert_eq!(p.title, "A Study on Zero-Knowledge Proofs");
        assert_eq!(p.authors, vec!["Alice Smith"]);
    }

    #[test]
    fn test_parse_bht_xml_uses_first_ee() {
        let papers = parse_bht_xml(SAMPLE_BHT_XML, "sp").unwrap();
        let p = &papers[1];

        // 複数の <ee> がある場合，最初のものを使用
        assert_eq!(p.url, "https://doi.org/10.1109/SP54263.2024.00170");
    }

    #[test]
    fn test_parse_bht_xml_title_period_removed() {
        let papers = parse_bht_xml(SAMPLE_BHT_XML, "sp").unwrap();

        // タイトル末尾のピリオドが除去されている
        assert!(!papers[0].title.ends_with('.'));
        assert!(!papers[1].title.ends_with('.'));
    }

    #[test]
    fn test_parse_bht_xml_hash_deterministic() {
        let papers = parse_bht_xml(SAMPLE_BHT_XML, "sp").unwrap();

        assert_eq!(papers[0].hash.len(), 64);
        assert_ne!(papers[0].hash, papers[1].hash);
    }

    #[test]
    fn test_parse_bht_xml_empty_document() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<bht key="db/conf/sp/sp1999.bht" title="SP 1999">
<h1>SP 1999</h1>
<dblpcites>
</dblpcites>
</bht>"#;

        let papers = parse_bht_xml(xml, "sp").unwrap();
        assert!(papers.is_empty());
    }

    #[test]
    fn test_parse_bht_xml_inline_markup_in_title() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<bht key="db/conf/sp/sp2024.bht" title="SP 2024">
<dblpcites>
<r style="ee"><inproceedings key="conf/sp/Test24" mdate="2024-01-01">
<author pid="1/1">Test Author</author>
<title>A <i>Novel</i> Approach to <sub>2</sub>-Factor Auth.</title>
<year>2024</year>
<booktitle>SP</booktitle>
<ee>https://doi.org/10.1109/test</ee>
</inproceedings></r>
</dblpcites>
</bht>"#;

        let papers = parse_bht_xml(xml, "sp").unwrap();
        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].title, "A Novel Approach to 2-Factor Auth");
    }

    #[test]
    fn test_parse_bht_xml_no_ee() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<bht key="db/conf/sp/sp2024.bht" title="SP 2024">
<dblpcites>
<r style="ee"><inproceedings key="conf/sp/NoUrl24" mdate="2024-01-01">
<author pid="1/1">Author One</author>
<title>Paper Without URL.</title>
<year>2024</year>
<booktitle>SP</booktitle>
</inproceedings></r>
</dblpcites>
</bht>"#;

        let papers = parse_bht_xml(xml, "sp").unwrap();
        assert_eq!(papers.len(), 1);
        assert!(papers[0].url.is_empty());
    }

    #[test]
    fn test_parse_bht_xml_conference_id_passthrough() {
        let papers = parse_bht_xml(SAMPLE_BHT_XML, "custom-conf").unwrap();
        for p in &papers {
            assert_eq!(p.conference, "custom-conf");
        }
    }
}
