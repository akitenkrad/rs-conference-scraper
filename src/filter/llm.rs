use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
use tokio::sync::Semaphore;

use crate::output::ScoredPaper;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-sonnet-4-20250514";

pub struct LlmFilter {
    api_key: String,
    theme: String,
    threshold: f64,
    jobs: usize,
}

impl LlmFilter {
    pub fn new(api_key: String, theme: String, threshold: f64, jobs: usize) -> Self {
        Self {
            api_key,
            theme,
            threshold,
            jobs,
        }
    }

    /// Score a batch of papers using the LLM.
    /// This takes already-filtered ScoredPapers from previous pipeline stages.
    pub async fn score_papers(&self, papers: Vec<ScoredPaper>) -> Result<Vec<ScoredPaper>> {
        let client = reqwest::Client::new();
        let semaphore = Arc::new(Semaphore::new(self.jobs));
        let mut handles = Vec::new();

        tracing::info!(
            "LLM scoring {} papers with threshold {}...",
            papers.len(),
            self.threshold
        );

        for paper in papers {
            let permit = semaphore.clone().acquire_owned().await?;
            let client = client.clone();
            let api_key = self.api_key.clone();
            let theme = self.theme.clone();
            let threshold = self.threshold;

            let handle = tokio::spawn(async move {
                let score = score_single_paper(&client, &api_key, &theme, &paper).await;
                drop(permit);
                (paper, score, threshold)
            });
            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            let (mut paper, score_result, threshold) = handle.await?;
            match score_result {
                Ok(score) => {
                    if score >= threshold {
                        paper
                            .scores
                            .insert("llm".to_string(), serde_json::Value::from(score));
                        results.push(paper);
                    }
                }
                Err(e) => {
                    tracing::warn!("LLM scoring failed for '{}': {}", paper.title, e);
                    // Score 0.0 on failure, don't include
                }
            }
        }

        tracing::info!(
            "LLM filter: {} papers passed (threshold: {})",
            results.len(),
            self.threshold
        );
        Ok(results)
    }
}

async fn score_single_paper(
    client: &reqwest::Client,
    api_key: &str,
    theme: &str,
    paper: &ScoredPaper,
) -> Result<f64> {
    let prompt = format!(
        "You are a research paper relevance scorer.\n\n\
         Theme: {}\n\n\
         Paper:\n  Title: {}\n  Abstract: {}\n\n\
         Score the relevance of this paper to the theme on a scale from 0.0 to 1.0.\n\
         Respond ONLY with a JSON object: {{\"score\": <float>}}",
        theme, paper.title, paper.r#abstract
    );

    let body = serde_json::json!({
        "model": MODEL,
        "max_tokens": 64,
        "messages": [
            {
                "role": "user",
                "content": prompt
            }
        ]
    });

    let mut retries = 0u32;
    let max_retries = 3u32;

    loop {
        let resp = client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && retries < max_retries {
            retries += 1;
            let wait = Duration::from_secs(2u64.pow(retries));
            tracing::warn!("Rate limited (429), retrying in {:?}...", wait);
            tokio::time::sleep(wait).await;
            continue;
        }

        if status.is_server_error() && retries < max_retries {
            retries += 1;
            let wait = Duration::from_secs(2u64.pow(retries));
            tracing::warn!("Server error ({}), retrying in {:?}...", status, wait);
            tokio::time::sleep(wait).await;
            continue;
        }

        let resp_text = resp.text().await?;

        if !status.is_success() {
            bail!("API error ({}): {}", status, resp_text);
        }

        // Parse API response
        let api_response: serde_json::Value = serde_json::from_str(&resp_text)?;

        // Extract text content from the response
        let content_text = api_response["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|block| block["text"].as_str())
            .unwrap_or("");

        // Parse the score from the response text
        // Try to find {"score": X} pattern
        if let Ok(score_obj) = serde_json::from_str::<serde_json::Value>(content_text) {
            if let Some(score) = score_obj["score"].as_f64() {
                return Ok(score.clamp(0.0, 1.0));
            }
        }

        // Fallback: try to extract any float from the text
        if let Some(score) = extract_float(content_text) {
            return Ok(score.clamp(0.0, 1.0));
        }

        tracing::warn!("Could not parse LLM score from response: {}", content_text);
        return Ok(0.0);
    }
}

fn extract_float(text: &str) -> Option<f64> {
    // Find first float-like pattern in text
    let re = regex::Regex::new(r"(\d+\.?\d*)").ok()?;
    re.captures(text)?
        .get(1)?
        .as_str()
        .parse::<f64>()
        .ok()
}
