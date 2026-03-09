use super::{FilterScore, PaperFilter};
use crate::types::Paper;

pub struct CategoryFilter {
    tags: Vec<String>,
}

impl CategoryFilter {
    pub fn new(tags: &[String]) -> Self {
        Self {
            tags: tags.iter().map(|t| t.to_lowercase()).collect(),
        }
    }
}

impl PaperFilter for CategoryFilter {
    fn name(&self) -> &str {
        "category"
    }

    fn score(&self, paper: &Paper) -> FilterScore {
        let matched = paper.categories.iter().any(|cat| {
            let cat_lower = cat.to_lowercase();
            self.tags.iter().any(|tag| cat_lower.contains(tag))
        });
        FilterScore::Boolean(matched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_paper_with_categories(categories: Vec<&str>) -> Paper {
        Paper {
            id: "test".to_string(),
            conference: "neurips".to_string(),
            year: 2024,
            title: "Test Paper".to_string(),
            authors: vec![],
            r#abstract: String::new(),
            url: String::new(),
            pdf_url: None,
            categories: categories.into_iter().map(String::from).collect(),
            hash: String::new(),
        }
    }

    #[test]
    fn matching_category_returns_true() {
        let filter = CategoryFilter::new(&["oral".to_string()]);
        let paper = make_paper_with_categories(vec!["Oral"]);
        match filter.score(&paper) {
            FilterScore::Boolean(b) => assert!(b),
            _ => panic!("Expected Boolean score"),
        }
    }

    #[test]
    fn no_matching_category_returns_false() {
        let filter = CategoryFilter::new(&["oral".to_string()]);
        let paper = make_paper_with_categories(vec!["Poster"]);
        match filter.score(&paper) {
            FilterScore::Boolean(b) => assert!(!b),
            _ => panic!("Expected Boolean score"),
        }
    }

    #[test]
    fn case_insensitive_matching() {
        let filter = CategoryFilter::new(&["ORAL".to_string()]);
        let paper = make_paper_with_categories(vec!["oral"]);
        match filter.score(&paper) {
            FilterScore::Boolean(b) => assert!(b),
            _ => panic!("Expected Boolean score"),
        }
    }

    #[test]
    fn partial_match() {
        let filter = CategoryFilter::new(&["conference".to_string()]);
        let paper = make_paper_with_categories(vec!["Conference Track"]);
        match filter.score(&paper) {
            FilterScore::Boolean(b) => assert!(b),
            _ => panic!("Expected Boolean score"),
        }
    }

    #[test]
    fn multiple_tags_one_matches() {
        let filter = CategoryFilter::new(&["oral".to_string(), "spotlight".to_string()]);
        let paper = make_paper_with_categories(vec!["Spotlight"]);
        match filter.score(&paper) {
            FilterScore::Boolean(b) => assert!(b),
            _ => panic!("Expected Boolean score"),
        }
    }

    #[test]
    fn empty_tags() {
        let filter = CategoryFilter::new(&[]);
        let paper = make_paper_with_categories(vec!["Oral"]);
        match filter.score(&paper) {
            FilterScore::Boolean(b) => assert!(!b),
            _ => panic!("Expected Boolean score"),
        }
    }

    #[test]
    fn empty_categories_on_paper() {
        let filter = CategoryFilter::new(&["oral".to_string()]);
        let paper = make_paper_with_categories(vec![]);
        match filter.score(&paper) {
            FilterScore::Boolean(b) => assert!(!b),
            _ => panic!("Expected Boolean score"),
        }
    }
}
