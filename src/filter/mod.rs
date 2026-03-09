pub mod category;
pub mod keyword;
pub mod llm;

use crate::cli::FilterArgs;
use crate::output::ScoredPaper;
use crate::types::Paper;

/// フィルタの結果スコア
#[derive(Debug, Clone)]
pub enum FilterScore {
    Numeric(f64),
    Boolean(bool),
}

/// 論文フィルタの共通インターフェース
pub trait PaperFilter {
    /// フィルタ名
    fn name(&self) -> &str;
    /// 論文にスコアを付与
    fn score(&self, paper: &Paper) -> FilterScore;
}

/// フィルタパイプライン
pub struct FilterPipeline {
    stages: Vec<Box<dyn PaperFilter>>,
    combine: CombineMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CombineMode {
    And,
    Or,
}

impl FilterPipeline {
    /// FilterArgs からパイプラインを構築
    /// 適用順: category → keyword (→ llm は別途)
    pub fn build(args: &FilterArgs) -> Self {
        let mut stages: Vec<Box<dyn PaperFilter>> = Vec::new();
        let combine = match args.combine.as_str() {
            "or" => CombineMode::Or,
            _ => CombineMode::And,
        };

        // Order: category → keyword (lightweight first)
        if args.filter.iter().any(|f| f == "category") && !args.tags.is_empty() {
            stages.push(Box::new(category::CategoryFilter::new(&args.tags)));
        }

        if args.filter.iter().any(|f| f == "keyword") && !args.keywords.is_empty() {
            stages.push(Box::new(keyword::KeywordFilter::new(
                &args.keywords,
                &args.fields,
            )));
        }

        // LLM filter is added separately (it's async)

        Self { stages, combine }
    }

    /// パイプラインを適用して結果を返す
    pub fn apply(&self, papers: Vec<Paper>) -> Vec<ScoredPaper> {
        match self.combine {
            CombineMode::And => self.apply_and(papers),
            CombineMode::Or => self.apply_or(papers),
        }
    }

    fn apply_and(&self, papers: Vec<Paper>) -> Vec<ScoredPaper> {
        let mut scored: Vec<ScoredPaper> =
            papers.into_iter().map(ScoredPaper::from_paper).collect();

        for stage in &self.stages {
            scored = scored
                .into_iter()
                .filter_map(|mut sp| {
                    let paper = sp.to_paper_ref();
                    let score = stage.score(&paper);
                    match &score {
                        FilterScore::Boolean(false) => None,
                        FilterScore::Numeric(v) if *v <= 0.0 => None,
                        _ => {
                            let score_value = match score {
                                FilterScore::Numeric(v) => serde_json::Value::from(v),
                                FilterScore::Boolean(b) => serde_json::Value::from(b),
                            };
                            sp.scores.insert(stage.name().to_string(), score_value);
                            Some(sp)
                        }
                    }
                })
                .collect();

            tracing::info!("{} filter: {} papers passed", stage.name(), scored.len());
        }

        scored
    }

    fn apply_or(&self, papers: Vec<Paper>) -> Vec<ScoredPaper> {
        use std::collections::HashMap;
        let mut results: HashMap<String, ScoredPaper> = HashMap::new();

        for stage in &self.stages {
            for paper in &papers {
                let score = stage.score(paper);
                let passed = match &score {
                    FilterScore::Boolean(b) => *b,
                    FilterScore::Numeric(v) => *v > 0.0,
                };
                if passed {
                    let entry = results
                        .entry(paper.id.clone())
                        .or_insert_with(|| ScoredPaper::from_paper(paper.clone()));
                    let score_value = match score {
                        FilterScore::Numeric(v) => serde_json::Value::from(v),
                        FilterScore::Boolean(b) => serde_json::Value::from(b),
                    };
                    entry.scores.insert(stage.name().to_string(), score_value);
                }
            }
        }

        results.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Paper;

    fn make_paper(id: &str, title: &str, abstract_text: &str, categories: Vec<&str>) -> Paper {
        Paper {
            id: id.to_string(),
            conference: "neurips".to_string(),
            year: 2024,
            title: title.to_string(),
            authors: vec![],
            r#abstract: abstract_text.to_string(),
            url: String::new(),
            pdf_url: None,
            categories: categories.into_iter().map(String::from).collect(),
            hash: String::new(),
        }
    }

    fn make_pipeline(
        stages: Vec<Box<dyn PaperFilter>>,
        combine: CombineMode,
    ) -> FilterPipeline {
        FilterPipeline { stages, combine }
    }

    #[test]
    fn and_mode_paper_must_pass_all_filters() {
        let cat_filter = Box::new(category::CategoryFilter::new(&["oral".to_string()]));
        let kw_filter = Box::new(keyword::KeywordFilter::new(
            &["transformer".to_string()],
            &["title".to_string(), "abstract".to_string()],
        ));
        let pipeline = make_pipeline(vec![cat_filter, kw_filter], CombineMode::And);

        let papers = vec![
            // Passes both
            make_paper("p1", "Transformer Model", "", vec!["Oral"]),
            // Passes category only
            make_paper("p2", "CNN Model", "", vec!["Oral"]),
            // Passes keyword only
            make_paper("p3", "Transformer Model", "", vec!["Poster"]),
        ];

        let results = pipeline.apply(papers);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "p1");
    }

    #[test]
    fn or_mode_paper_passes_if_any_filter_matches() {
        let cat_filter = Box::new(category::CategoryFilter::new(&["oral".to_string()]));
        let kw_filter = Box::new(keyword::KeywordFilter::new(
            &["transformer".to_string()],
            &["title".to_string(), "abstract".to_string()],
        ));
        let pipeline = make_pipeline(vec![cat_filter, kw_filter], CombineMode::Or);

        let papers = vec![
            make_paper("p1", "Transformer Model", "", vec!["Poster"]),
            make_paper("p2", "CNN Model", "", vec!["Oral"]),
            make_paper("p3", "CNN Model", "", vec!["Poster"]),
        ];

        let results = pipeline.apply(papers);
        assert_eq!(results.len(), 2);
        let ids: std::collections::HashSet<String> =
            results.iter().map(|r| r.id.clone()).collect();
        assert!(ids.contains("p1"));
        assert!(ids.contains("p2"));
    }

    #[test]
    fn empty_pipeline_returns_all_papers() {
        let pipeline = make_pipeline(vec![], CombineMode::And);
        let papers = vec![
            make_paper("p1", "Title A", "", vec![]),
            make_paper("p2", "Title B", "", vec![]),
        ];
        let results = pipeline.apply(papers);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn pipeline_applies_category_then_keyword() {
        // Category filter first removes non-Oral, then keyword filter further filters
        let cat_filter = Box::new(category::CategoryFilter::new(&["oral".to_string()]));
        let kw_filter = Box::new(keyword::KeywordFilter::new(
            &["attention".to_string()],
            &["title".to_string(), "abstract".to_string()],
        ));
        let pipeline = make_pipeline(vec![cat_filter, kw_filter], CombineMode::And);

        let papers = vec![
            make_paper("p1", "Attention Model", "", vec!["Oral"]),
            make_paper("p2", "CNN Model", "", vec!["Oral"]),
            make_paper("p3", "Attention Model", "", vec!["Poster"]),
        ];

        let results = pipeline.apply(papers);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "p1");
        // Should have scores from both filters
        assert!(results[0].scores.contains_key("category"));
        assert!(results[0].scores.contains_key("keyword"));
    }
}
