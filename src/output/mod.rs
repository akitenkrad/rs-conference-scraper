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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
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
                theme: Some("deep learning".to_string()),
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
        assert_eq!(json["query"]["theme"], "deep learning");
        assert_eq!(json["query"]["filters"][0], "keyword");
        assert_eq!(json["query"]["combine"], "and");
        assert_eq!(json["papers"][0]["title"], "Test Paper Title");
    }

    #[test]
    fn query_info_with_none_theme_skips_field() {
        let query = QueryInfo {
            conferences: vec!["neurips".to_string()],
            years: vec![2024],
            theme: None,
            filters: vec![],
            combine: "and".to_string(),
        };

        let json = serde_json::to_value(&query).unwrap();
        assert!(json.get("theme").is_none());
    }
}
