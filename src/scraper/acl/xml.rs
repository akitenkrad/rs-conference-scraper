use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::types::{compute_id, Paper, PaperListEntry};

/// Parsed result from ACL XML containing both list entries and full paper details.
pub struct ParsedPapers {
    pub entries: Vec<PaperListEntry>,
    pub papers: Vec<Paper>,
}

fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}{}", title, abstract_text).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn map_volume_id_to_track(volume_id: &str) -> String {
    match volume_id {
        "long" => "Long Papers".to_string(),
        "short" => "Short Papers".to_string(),
        "demo" | "demos" => "System Demonstrations".to_string(),
        "tutorial" | "tutorials" => "Tutorials".to_string(),
        "srw" => "Student Research Workshop".to_string(),
        "findings" => "Findings".to_string(),
        "industry" => "Industry".to_string(),
        other => other.to_string(),
    }
}

/// Parse ACL Anthology XML into paper entries and full paper details.
pub fn parse_xml(xml: &str, conference: &str, year: u16) -> Result<ParsedPapers> {
    let doc = roxmltree::Document::parse(xml)?;
    let root = doc.root_element();

    let mut entries = Vec::new();
    let mut papers = Vec::new();

    for volume in root.children().filter(|n| n.has_tag_name("volume")) {
        let volume_id = volume.attribute("id").unwrap_or("");
        let track = map_volume_id_to_track(volume_id);

        for paper_node in volume.children().filter(|n| n.has_tag_name("paper")) {
            let title = match get_child_text(&paper_node, "title") {
                Some(t) if !t.is_empty() => t,
                _ => continue,
            };

            let authors = get_authors(&paper_node);
            let abstract_text = get_child_text(&paper_node, "abstract").unwrap_or_default();
            let url_text = get_child_text(&paper_node, "url").unwrap_or_default();

            let page_url = if url_text.is_empty() {
                String::new()
            } else {
                format!("https://aclanthology.org/{}/", url_text)
            };

            let pdf_url = if url_text.is_empty() {
                None
            } else {
                Some(format!("https://aclanthology.org/{}.pdf", url_text))
            };

            let id = compute_id(&title);
            let hash = compute_hash(&title, &abstract_text);

            entries.push(PaperListEntry {
                title: title.clone(),
                authors: authors.clone(),
                detail_url: page_url.clone(),
                track: Some(track.clone()),
            });

            papers.push(Paper {
                id,
                conference: conference.to_string(),
                year,
                title,
                authors,
                r#abstract: abstract_text,
                url: page_url,
                pdf_url,
                categories: vec![track.clone()],
                hash,
            });
        }
    }

    Ok(ParsedPapers { entries, papers })
}

fn get_child_text(node: &roxmltree::Node, tag: &str) -> Option<String> {
    node.children()
        .find(|n| n.has_tag_name(tag))
        .map(|n| {
            n.descendants()
                .filter(|d| d.is_text())
                .map(|d| d.text().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("")
                .trim()
                .to_string()
        })
}

fn get_authors(node: &roxmltree::Node) -> Vec<String> {
    node.children()
        .filter(|n| n.has_tag_name("author"))
        .filter_map(|author| {
            let first = get_child_text(&author, "first").unwrap_or_default();
            let last = get_child_text(&author, "last").unwrap_or_default();
            let name = format!("{} {}", first, last).trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_paper() {
        let xml = r#"<?xml version='1.0' encoding='UTF-8'?>
        <collection id="2024.acl">
          <volume id="long" type="proceedings">
            <meta><booktitle>Proceedings</booktitle><venue>ACL</venue></meta>
            <paper id="1">
              <title>Test Paper Title</title>
              <author><first>Alice</first><last>Smith</last></author>
              <abstract>This is the abstract.</abstract>
              <url hash="abc123">2024.acl-long.1</url>
            </paper>
          </volume>
        </collection>"#;

        let result = parse_xml(xml, "acl", 2024).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.papers.len(), 1);

        let entry = &result.entries[0];
        assert_eq!(entry.title, "Test Paper Title");
        assert_eq!(entry.authors, vec!["Alice Smith"]);
        assert_eq!(entry.track, Some("Long Papers".to_string()));
        assert_eq!(
            entry.detail_url,
            "https://aclanthology.org/2024.acl-long.1/"
        );

        let paper = &result.papers[0];
        assert_eq!(paper.title, "Test Paper Title");
        assert_eq!(paper.r#abstract, "This is the abstract.");
        assert_eq!(paper.year, 2024);
        assert_eq!(paper.conference, "acl");
        assert_eq!(paper.categories, vec!["Long Papers"]);
        assert_eq!(
            paper.pdf_url,
            Some("https://aclanthology.org/2024.acl-long.1.pdf".to_string())
        );
        assert_eq!(paper.id, compute_id("Test Paper Title"));
    }

    #[test]
    fn test_parse_multiple_authors() {
        let xml = r#"<?xml version='1.0' encoding='UTF-8'?>
        <collection id="2024.acl">
          <volume id="short" type="proceedings">
            <meta><booktitle>Proceedings</booktitle><venue>ACL</venue></meta>
            <paper id="1">
              <title>Multi Author Paper</title>
              <author><first>Alice</first><last>Smith</last></author>
              <author><first>Bob</first><last>Jones</last></author>
              <author><first>Carol</first><last>Williams</last></author>
              <abstract>Abstract text here.</abstract>
              <url hash="def456">2024.acl-short.1</url>
            </paper>
          </volume>
        </collection>"#;

        let result = parse_xml(xml, "acl", 2024).unwrap();
        let paper = &result.papers[0];
        assert_eq!(
            paper.authors,
            vec!["Alice Smith", "Bob Jones", "Carol Williams"]
        );
        assert_eq!(paper.categories, vec!["Short Papers"]);
    }

    #[test]
    fn test_parse_missing_abstract() {
        let xml = r#"<?xml version='1.0' encoding='UTF-8'?>
        <collection id="2024.acl">
          <volume id="long" type="proceedings">
            <meta><booktitle>Proceedings</booktitle><venue>ACL</venue></meta>
            <paper id="1">
              <title>No Abstract Paper</title>
              <author><first>Alice</first><last>Smith</last></author>
              <url hash="ghi789">2024.acl-long.1</url>
            </paper>
          </volume>
        </collection>"#;

        let result = parse_xml(xml, "acl", 2024).unwrap();
        assert_eq!(result.papers.len(), 1);
        assert_eq!(result.papers[0].r#abstract, "");
    }

    #[test]
    fn test_parse_multiple_volumes() {
        let xml = r#"<?xml version='1.0' encoding='UTF-8'?>
        <collection id="2024.acl">
          <volume id="long" type="proceedings">
            <meta><booktitle>Proceedings</booktitle><venue>ACL</venue></meta>
            <paper id="1">
              <title>Long Paper</title>
              <author><first>Alice</first><last>Smith</last></author>
              <abstract>Long abstract.</abstract>
              <url hash="aaa">2024.acl-long.1</url>
            </paper>
          </volume>
          <volume id="demo" type="proceedings">
            <meta><booktitle>Demos</booktitle><venue>ACL</venue></meta>
            <paper id="1">
              <title>Demo Paper</title>
              <author><first>Bob</first><last>Jones</last></author>
              <abstract>Demo abstract.</abstract>
              <url hash="bbb">2024.acl-demo.1</url>
            </paper>
          </volume>
        </collection>"#;

        let result = parse_xml(xml, "acl", 2024).unwrap();
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.papers.len(), 2);

        assert_eq!(result.papers[0].categories, vec!["Long Papers"]);
        assert_eq!(result.papers[1].categories, vec!["System Demonstrations"]);
    }

    #[test]
    fn test_skip_paper_without_title() {
        let xml = r#"<?xml version='1.0' encoding='UTF-8'?>
        <collection id="2024.acl">
          <volume id="long" type="proceedings">
            <meta><booktitle>Proceedings</booktitle><venue>ACL</venue></meta>
            <paper id="1">
              <author><first>Alice</first><last>Smith</last></author>
              <abstract>No title here.</abstract>
              <url hash="xyz">2024.acl-long.1</url>
            </paper>
            <paper id="2">
              <title>Valid Paper</title>
              <author><first>Bob</first><last>Jones</last></author>
              <abstract>Valid abstract.</abstract>
              <url hash="abc">2024.acl-long.2</url>
            </paper>
          </volume>
        </collection>"#;

        let result = parse_xml(xml, "acl", 2024).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].title, "Valid Paper");
    }

    #[test]
    fn test_hash_computation() {
        let xml = r#"<?xml version='1.0' encoding='UTF-8'?>
        <collection id="2024.acl">
          <volume id="long" type="proceedings">
            <meta><booktitle>Proceedings</booktitle><venue>ACL</venue></meta>
            <paper id="1">
              <title>Hash Test</title>
              <author><first>Alice</first><last>Smith</last></author>
              <abstract>Some abstract.</abstract>
              <url hash="xxx">2024.acl-long.1</url>
            </paper>
          </volume>
        </collection>"#;

        let result = parse_xml(xml, "acl", 2024).unwrap();
        let paper = &result.papers[0];
        assert_eq!(paper.hash.len(), 64);
        assert_eq!(paper.hash, compute_hash("Hash Test", "Some abstract."));
    }
}
