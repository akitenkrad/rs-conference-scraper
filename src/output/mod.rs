use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::Paper;

#[derive(Debug, Serialize, Deserialize)]
pub struct FilterOutput {
    pub query: QueryInfo,
    pub total: usize,
    pub papers: Vec<ScoredPaper>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryInfo {
    pub conferences: Vec<String>,
    pub years: Vec<u16>,
    pub filters: Vec<String>,
    pub combine: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredPaper {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub r#abstract: String,
    pub conference: String,
    pub year: u16,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf_url: Option<String>,
    pub categories: Vec<String>,
    pub scores: HashMap<String, serde_json::Value>,
}

impl ScoredPaper {
    pub fn from_paper(paper: Paper) -> Self {
        Self {
            id: paper.id,
            title: paper.title,
            authors: paper.authors,
            r#abstract: paper.r#abstract,
            conference: paper.conference,
            year: paper.year,
            url: paper.url,
            pdf_url: paper.pdf_url,
            categories: paper.categories,
            scores: HashMap::new(),
        }
    }

    pub fn to_paper_ref(&self) -> Paper {
        Paper {
            id: self.id.clone(),
            conference: self.conference.clone(),
            year: self.year,
            title: self.title.clone(),
            authors: self.authors.clone(),
            r#abstract: self.r#abstract.clone(),
            url: self.url.clone(),
            pdf_url: self.pdf_url.clone(),
            categories: self.categories.clone(),
            hash: String::new(),
        }
    }
}

impl FilterOutput {
    /// 指定フォーマットで文字列に変換
    pub fn format(&self, fmt: &str) -> Result<String> {
        match fmt {
            "json" => Ok(serde_json::to_string_pretty(self)?),
            "csv" => self.to_csv(),
            "xml" => Ok(self.to_xml()),
            _ => bail!("Unsupported format: '{}'. Use json, csv, or xml.", fmt),
        }
    }

    fn to_csv(&self) -> Result<String> {
        let mut wtr = csv::Writer::from_writer(Vec::new());
        wtr.write_record([
            "id",
            "title",
            "authors",
            "abstract",
            "conference",
            "year",
            "url",
            "pdf_url",
            "categories",
        ])?;
        for p in &self.papers {
            wtr.write_record([
                &p.id,
                &p.title,
                &p.authors.join("; "),
                &p.r#abstract,
                &p.conference,
                &p.year.to_string(),
                &p.url,
                &p.pdf_url.as_deref().unwrap_or("").to_string(),
                &p.categories.join("; "),
            ])?;
        }
        wtr.flush()?;
        Ok(String::from_utf8(wtr.into_inner()?)?)
    }

    fn to_xml(&self) -> String {
        let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        xml.push('\n');
        xml.push_str("<filter_output>\n");

        // query
        xml.push_str("  <query>\n");
        for c in &self.query.conferences {
            xml.push_str(&format!("    <conference>{}</conference>\n", xml_escape(c)));
        }
        for y in &self.query.years {
            xml.push_str(&format!("    <year>{y}</year>\n"));
        }
        for f in &self.query.filters {
            xml.push_str(&format!("    <filter>{}</filter>\n", xml_escape(f)));
        }
        xml.push_str(&format!(
            "    <combine>{}</combine>\n",
            xml_escape(&self.query.combine)
        ));
        xml.push_str("  </query>\n");

        xml.push_str(&format!("  <total>{}</total>\n", self.total));

        // papers
        xml.push_str("  <papers>\n");
        for p in &self.papers {
            xml.push_str("    <paper>\n");
            xml.push_str(&format!(
                "      <id>{}</id>\n",
                xml_escape(&p.id)
            ));
            xml.push_str(&format!(
                "      <title>{}</title>\n",
                xml_escape(&p.title)
            ));
            xml.push_str("      <authors>\n");
            for a in &p.authors {
                xml.push_str(&format!(
                    "        <author>{}</author>\n",
                    xml_escape(a)
                ));
            }
            xml.push_str("      </authors>\n");
            xml.push_str(&format!(
                "      <abstract>{}</abstract>\n",
                xml_escape(&p.r#abstract)
            ));
            xml.push_str(&format!(
                "      <conference>{}</conference>\n",
                xml_escape(&p.conference)
            ));
            xml.push_str(&format!("      <year>{}</year>\n", p.year));
            xml.push_str(&format!(
                "      <url>{}</url>\n",
                xml_escape(&p.url)
            ));
            if let Some(ref pdf) = p.pdf_url {
                xml.push_str(&format!(
                    "      <pdf_url>{}</pdf_url>\n",
                    xml_escape(pdf)
                ));
            }
            if !p.categories.is_empty() {
                xml.push_str("      <categories>\n");
                for c in &p.categories {
                    xml.push_str(&format!(
                        "        <category>{}</category>\n",
                        xml_escape(c)
                    ));
                }
                xml.push_str("      </categories>\n");
            }
            xml.push_str("    </paper>\n");
        }
        xml.push_str("  </papers>\n");
        xml.push_str("</filter_output>\n");
        xml
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_paper() -> Paper {
        Paper {
            id: "test_id".to_string(),
            conference: "neurips".to_string(),
            year: 2024,
            title: "Test Paper Title".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            r#abstract: "This is an abstract".to_string(),
            url: "https://example.com/paper".to_string(),
            pdf_url: Some("https://example.com/paper.pdf".to_string()),
            categories: vec!["Oral".to_string()],
            hash: "abc123".to_string(),
        }
    }

    #[test]
    fn scored_paper_from_paper_preserves_all_fields() {
        let paper = make_paper();
        let scored = ScoredPaper::from_paper(paper.clone());
        assert_eq!(scored.id, paper.id);
        assert_eq!(scored.title, paper.title);
        assert_eq!(scored.authors, paper.authors);
        assert_eq!(scored.r#abstract, paper.r#abstract);
        assert_eq!(scored.conference, paper.conference);
        assert_eq!(scored.year, paper.year);
        assert_eq!(scored.url, paper.url);
        assert_eq!(scored.pdf_url, paper.pdf_url);
        assert_eq!(scored.categories, paper.categories);
        assert!(scored.scores.is_empty());
    }

    #[test]
    fn filter_output_serializes_to_expected_json() {
        let paper = make_paper();
        let scored = ScoredPaper::from_paper(paper);

        let output = FilterOutput {
            query: QueryInfo {
                conferences: vec!["neurips".to_string()],
                years: vec![2024],
                filters: vec!["keyword".to_string()],
                combine: "and".to_string(),
            },
            total: 1,
            papers: vec![scored],
        };

        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["total"], 1);
        assert_eq!(json["query"]["conferences"][0], "neurips");
        assert_eq!(json["query"]["years"][0], 2024);
        assert_eq!(json["query"]["filters"][0], "keyword");
        assert_eq!(json["query"]["combine"], "and");
        assert_eq!(json["papers"][0]["title"], "Test Paper Title");
    }

}
