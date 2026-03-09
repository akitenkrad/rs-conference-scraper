use regex::RegexBuilder;

use super::{FilterScore, PaperFilter};
use crate::types::Paper;

pub struct KeywordFilter {
    patterns: Vec<regex::Regex>,
    search_title: bool,
    search_abstract: bool,
}

impl KeywordFilter {
    pub fn new(keywords: &[String], fields: &[String]) -> Self {
        let patterns = keywords
            .iter()
            .filter_map(|kw| {
                RegexBuilder::new(&regex::escape(kw))
                    .case_insensitive(true)
                    .build()
                    .ok()
            })
            .collect();

        let search_title = fields.iter().any(|f| f == "title");
        let search_abstract = fields.iter().any(|f| f == "abstract");

        Self {
            patterns,
            search_title,
            search_abstract,
        }
    }
}

impl PaperFilter for KeywordFilter {
    fn name(&self) -> &str {
        "keyword"
    }

    fn score(&self, paper: &Paper) -> FilterScore {
        if self.patterns.is_empty() {
            return FilterScore::Numeric(0.0);
        }

        let matched = self
            .patterns
            .iter()
            .filter(|pat| {
                let in_title = self.search_title && pat.is_match(&paper.title);
                let in_abstract = self.search_abstract && pat.is_match(&paper.r#abstract);
                in_title || in_abstract
            })
            .count();

        FilterScore::Numeric(matched as f64 / self.patterns.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_paper(title: &str, abstract_text: &str) -> Paper {
        Paper {
            id: "test".to_string(),
            conference: "neurips".to_string(),
            year: 2024,
            title: title.to_string(),
            authors: vec![],
            r#abstract: abstract_text.to_string(),
            url: String::new(),
            pdf_url: None,
            categories: vec![],
            hash: String::new(),
        }
    }

    fn both_fields() -> Vec<String> {
        vec!["title".to_string(), "abstract".to_string()]
    }

    #[test]
    fn keyword_matches_in_title() {
        let filter = KeywordFilter::new(
            &["transformer".to_string()],
            &both_fields(),
        );
        let paper = make_paper("A Transformer Model", "Nothing relevant here");
        match filter.score(&paper) {
            FilterScore::Numeric(v) => assert!(v > 0.0),
            _ => panic!("Expected Numeric score"),
        }
    }

    #[test]
    fn keyword_matches_in_abstract() {
        let filter = KeywordFilter::new(
            &["attention".to_string()],
            &both_fields(),
        );
        let paper = make_paper("Some Title", "We use attention mechanisms");
        match filter.score(&paper) {
            FilterScore::Numeric(v) => assert!(v > 0.0),
            _ => panic!("Expected Numeric score"),
        }
    }

    #[test]
    fn case_insensitive_matching() {
        let filter = KeywordFilter::new(
            &["TRANSFORMER".to_string()],
            &both_fields(),
        );
        let paper = make_paper("a transformer model", "");
        match filter.score(&paper) {
            FilterScore::Numeric(v) => assert!(v > 0.0),
            _ => panic!("Expected Numeric score"),
        }
    }

    #[test]
    fn score_calculation_partial() {
        let filter = KeywordFilter::new(
            &[
                "transformer".to_string(),
                "attention".to_string(),
                "diffusion".to_string(),
            ],
            &both_fields(),
        );
        let paper = make_paper("A Transformer Model", "");
        match filter.score(&paper) {
            FilterScore::Numeric(v) => {
                let expected = 1.0 / 3.0;
                assert!((v - expected).abs() < 1e-9);
            }
            _ => panic!("Expected Numeric score"),
        }
    }

    #[test]
    fn no_match_returns_zero() {
        let filter = KeywordFilter::new(
            &["quantum".to_string()],
            &both_fields(),
        );
        let paper = make_paper("Neural Networks", "Deep learning study");
        match filter.score(&paper) {
            FilterScore::Numeric(v) => assert_eq!(v, 0.0),
            _ => panic!("Expected Numeric score"),
        }
    }

    #[test]
    fn all_keywords_match_returns_one() {
        let filter = KeywordFilter::new(
            &["transformer".to_string(), "attention".to_string()],
            &both_fields(),
        );
        let paper = make_paper("Transformer with Attention", "Uses attention in transformer");
        match filter.score(&paper) {
            FilterScore::Numeric(v) => assert_eq!(v, 1.0),
            _ => panic!("Expected Numeric score"),
        }
    }

    #[test]
    fn fields_restriction_title_only() {
        let filter = KeywordFilter::new(
            &["attention".to_string()],
            &["title".to_string()],
        );
        // keyword only in abstract, not title
        let paper = make_paper("Some Title", "Uses attention mechanisms");
        match filter.score(&paper) {
            FilterScore::Numeric(v) => assert_eq!(v, 0.0),
            _ => panic!("Expected Numeric score"),
        }
    }

    #[test]
    fn fields_restriction_abstract_only() {
        let filter = KeywordFilter::new(
            &["transformer".to_string()],
            &["abstract".to_string()],
        );
        // keyword only in title, not abstract
        let paper = make_paper("Transformer Model", "Nothing here");
        match filter.score(&paper) {
            FilterScore::Numeric(v) => assert_eq!(v, 0.0),
            _ => panic!("Expected Numeric score"),
        }
    }

    #[test]
    fn empty_keywords_returns_zero() {
        let filter = KeywordFilter::new(&[], &both_fields());
        let paper = make_paper("Any Title", "Any abstract");
        match filter.score(&paper) {
            FilterScore::Numeric(v) => assert_eq!(v, 0.0),
            _ => panic!("Expected Numeric score"),
        }
    }
}
