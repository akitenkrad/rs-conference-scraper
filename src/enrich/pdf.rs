use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use std::{io::ErrorKind, process::Stdio, sync::Once, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

static PDFTOTEXT_NOT_FOUND_WARNING: Once = Once::new();
const PDFTOTEXT_TIMEOUT: Duration = Duration::from_secs(30);

/// 複数の候補URLからPDFを取得してabstractを抽出する
///
/// pdf_url（S2由来），paper_url（DBLP ee等）の順で試行し，
/// 最初にabstractが取得できた時点で返す．
pub async fn fetch_abstract_via_pdf_urls(
    client: &Client,
    pdf_url: Option<&str>,
    paper_url: &str,
) -> Result<String> {
    // 候補URLを収集（重複排除）
    let candidate_urls = collect_candidate_urls(pdf_url, paper_url);

    if candidate_urls.is_empty() {
        return Ok(String::new());
    }

    for url in &candidate_urls {
        tracing::debug!("Trying PDF extraction from '{}'", url);
        match try_fetch_and_extract(client, url).await {
            Ok(text) if !text.is_empty() => {
                tracing::debug!("PDF abstract extracted from '{}'", url);
                return Ok(text);
            }
            Ok(_) => {
                tracing::debug!("PDF extraction returned empty for '{}'", url);
            }
            Err(e) => {
                tracing::debug!("PDF extraction failed for '{}': {}", url, e);
            }
        }
    }

    Ok(String::new())
}

/// 候補PDF URLを収集する（重複排除・優先順位付き）
fn collect_candidate_urls(pdf_url: Option<&str>, paper_url: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut add = |url: String| {
        if !url.is_empty() && seen.insert(url.clone()) {
            urls.push(url);
        }
    };

    // 1. S2由来のpdf_url（最優先）
    if let Some(url) = pdf_url {
        add(url.to_string());
    }

    // 2. paper_urlからPDF URLを推定
    for url in derive_pdf_urls(paper_url) {
        add(url);
    }

    urls
}

/// paper_url（DBLP ee, DOI等）からPDF直リンクを推定する
fn derive_pdf_urls(url: &str) -> Vec<String> {
    let mut urls = Vec::new();

    if url.is_empty() {
        return urls;
    }

    // arXiv: /abs/XXXX → /pdf/XXXX.pdf
    if url.contains("arxiv.org/abs/") {
        let pdf = url.replace("/abs/", "/pdf/");
        let pdf = if pdf.ends_with(".pdf") {
            pdf
        } else {
            format!("{}.pdf", pdf)
        };
        urls.push(pdf);
    }

    // DOI URL: リダイレクト先で試す（元URLをそのまま候補に追加）
    if url.starts_with("https://doi.org/") {
        urls.push(url.to_string());
    }

    // IACR ePrint: https://eprint.iacr.org/2024/123 → /2024/123.pdf
    if url.contains("eprint.iacr.org/") {
        let pdf = if url.ends_with(".pdf") {
            url.to_string()
        } else {
            format!("{}.pdf", url)
        };
        urls.push(pdf);
    }

    // OpenReview: https://openreview.net/forum?id=XXX → /pdf?id=XXX
    if url.contains("openreview.net/forum?") {
        let pdf = url.replace("/forum?", "/pdf?");
        urls.push(pdf);
    }

    // それでも候補がなければ元URLをそのまま試す（Content-Type判定で弾かれる）
    if urls.is_empty() {
        urls.push(url.to_string());
    }

    urls
}

/// 単一URLからPDFをダウンロードしてabstract抽出を試みる
async fn try_fetch_and_extract(client: &Client, url: &str) -> Result<String> {
    let resp = client
        .get(url)
        .header("Accept", "application/pdf")
        .send()
        .await?;

    if !resp.status().is_success() {
        tracing::debug!("PDF fetch returned status {} for '{}'", resp.status(), url);
        return Ok(String::new());
    }

    // Content-TypeがPDFでなければスキップ（HTMLページを無駄にパースしない）
    if let Some(ct) = resp.headers().get(reqwest::header::CONTENT_TYPE) {
        let ct_str = ct.to_str().unwrap_or("");
        if !ct_str.contains("pdf") && !ct_str.contains("octet-stream") {
            tracing::debug!("Skipping non-PDF content-type '{}' for '{}'", ct_str, url);
            return Ok(String::new());
        }
    }

    let bytes = resp.bytes().await?;

    // 最小PDFサイズチェック（ヘッダだけのPDFやエラーページを除外）
    if bytes.len() < 1024 {
        return Ok(String::new());
    }

    let bytes_clone = bytes.to_vec();
    let extraction = tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(&bytes_clone))
    })
    .await?;

    let text = match extraction {
        Ok(Ok(pages)) => pages.iter().take(2).cloned().collect::<Vec<_>>().join("\n"),
        Ok(Err(e)) => {
            tracing::debug!("PDF text extraction failed for '{}': {}", url, e);
            String::new()
        }
        Err(_) => {
            tracing::warn!("PDF text extraction panicked for '{}' (malformed PDF)", url);
            String::new()
        }
    };

    if text.is_empty() {
        let fallback_text = extract_with_pdftotext(&bytes, "pdftotext").await?;
        let abstract_text = extract_abstract_from_text(&fallback_text)?;
        if !abstract_text.is_empty() {
            tracing::debug!("pdftotext fallback succeeded for {}", url);
        }
        return Ok(abstract_text);
    }

    extract_abstract_from_text(&text)
}

async fn extract_with_pdftotext(bytes: &[u8], program: &str) -> Result<String> {
    let mut child = match Command::new(program)
        .args(["-f", "1", "-l", "2", "-enc", "UTF-8", "-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            PDFTOTEXT_NOT_FOUND_WARNING.call_once(|| {
                tracing::warn!("pdftotext executable was not found; PDF fallback is disabled");
            });
            return Ok(String::new());
        }
        Err(error) => {
            tracing::debug!("Failed to start pdftotext fallback: {}", error);
            return Ok(String::new());
        }
    };

    let Some(mut stdin) = child.stdin.take() else {
        tracing::debug!("Failed to open stdin for pdftotext fallback");
        return Ok(String::new());
    };

    let write_input = async move {
        let write_result = stdin.write_all(bytes).await;
        let shutdown_result = stdin.shutdown().await;
        drop(stdin);
        write_result.and(shutdown_result)
    };
    let communication = async { tokio::join!(write_input, child.wait_with_output()) };
    let (write_result, output_result) = match timeout(PDFTOTEXT_TIMEOUT, communication).await {
        Ok(results) => results,
        Err(_) => {
            tracing::debug!("pdftotext fallback timed out after 30 seconds");
            return Ok(String::new());
        }
    };

    if let Err(error) = write_result {
        tracing::debug!("Failed to write PDF to pdftotext stdin: {}", error);
        return Ok(String::new());
    }

    let output = match output_result {
        Ok(output) => output,
        Err(error) => {
            tracing::debug!("Failed to wait for pdftotext fallback: {}", error);
            return Ok(String::new());
        }
    };
    if !output.status.success() {
        tracing::debug!("pdftotext fallback exited with status {}", output.status);
        return Ok(String::new());
    }

    match String::from_utf8(output.stdout) {
        Ok(text) => Ok(text.replace('\x0c', "\n")),
        Err(error) => {
            tracing::debug!("pdftotext fallback returned invalid UTF-8: {}", error);
            Ok(String::new())
        }
    }
}

/// テキストから "Abstract" セクションを抽出する
fn extract_abstract_from_text(text: &str) -> Result<String> {
    let abstract_re = Regex::new(
        r"(?i)\babstract\b[\s\.:—\-]*\n?([\s\S]+?)(?:\n\s*(?:1[\.\s]?\s*Introduction|Keywords|Index Terms|Categories|CCS Concepts|I\.\s+INTRODUCTION)\b|\z)",
    )?;

    if let Some(caps) = abstract_re.captures(text)
        && let Some(m) = caps.get(1)
    {
        let abstract_text = normalize_pdf_text(m.as_str());
        if abstract_text.len() > 50 {
            return Ok(abstract_text);
        }
    }

    Ok(String::new())
}

/// PDF抽出テキストの正規化（改行・空白の整理）
fn normalize_pdf_text(text: &str) -> String {
    // ハイフネーション結合: "opti-\nmize" → "optimize"
    let text = Regex::new(r"-\s*\n\s*").unwrap().replace_all(text, "");

    // 段落区切り（空行）を一時マーカーに置換
    let text = Regex::new(r"\n\n+")
        .unwrap()
        .replace_all(&text, "\x00PARA\x00");

    // 残りの単一改行をスペースに置換
    let text = text.replace('\n', " ");

    // マーカーを改行に戻す
    let text = text.replace("\x00PARA\x00", "\n");

    // 連続空白を単一スペースに
    let text = Regex::new(r"[ \t]+").unwrap().replace_all(&text, " ");

    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_pdf_with_abstract() -> Vec<u8> {
        let content = concat!(
            "BT\n",
            "/F1 12 Tf\n",
            "72 720 Td\n",
            "(Abstract) Tj\n",
            "0 -20 Td\n",
            "(This paper presents a compact fallback extraction test with more than sixty characters of body text.) Tj\n",
            "0 -20 Td\n",
            "(1 Introduction) Tj\n",
            "ET\n",
        );
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}endstream", content.len(), content),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];

        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
        }

        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[tokio::test]
    async fn test_extract_with_pdftotext_missing_program_returns_empty() {
        let text =
            extract_with_pdftotext(b"not a PDF", "pdftotext-does-not-exist-conf-scraper-test")
                .await
                .unwrap();

        assert!(text.is_empty());
    }

    #[tokio::test]
    async fn test_extract_with_pdftotext_extracts_abstract_from_generated_pdf() {
        if tokio::process::Command::new("pdftotext")
            .arg("-v")
            .output()
            .await
            .is_err()
        {
            return;
        }

        let text = extract_with_pdftotext(&minimal_pdf_with_abstract(), "pdftotext")
            .await
            .unwrap();
        let abstract_text = extract_abstract_from_text(&text).unwrap();

        assert!(!text.contains('\x0c'));
        assert!(abstract_text.contains("compact fallback extraction test"));
        assert!(!abstract_text.contains("Introduction"));
    }

    #[test]
    fn test_extract_abstract_basic() {
        let text = r#"
Some Paper Title

Abstract
We propose a novel method for solving complex problems.
This method achieves state-of-the-art results on multiple benchmarks.
Our approach is simple yet effective.

1. Introduction
In recent years, there has been growing interest..."#;

        let result = extract_abstract_from_text(text).unwrap();
        assert!(result.contains("novel method"));
        assert!(result.contains("simple yet effective"));
        assert!(!result.contains("Introduction"));
    }

    #[test]
    fn test_extract_abstract_with_keywords() {
        let text = r#"
Abstract: This paper presents a comprehensive survey of deep learning techniques
applied to natural language processing tasks.

Keywords: deep learning, NLP, survey"#;

        let result = extract_abstract_from_text(text).unwrap();
        assert!(result.contains("comprehensive survey"));
        assert!(!result.contains("Keywords"));
    }

    #[test]
    fn test_extract_abstract_empty() {
        let text = "Some random text without an abstract section.";
        let result = extract_abstract_from_text(text).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_normalize_hyphenation() {
        let text = "opti-\nmize this ap-\nproach";
        let result = normalize_pdf_text(text);
        assert_eq!(result, "optimize this approach");
    }

    // --- derive_pdf_urls テスト ---

    #[test]
    fn test_derive_arxiv_url() {
        let urls = derive_pdf_urls("https://arxiv.org/abs/2301.12345");
        assert_eq!(urls, vec!["https://arxiv.org/pdf/2301.12345.pdf"]);
    }

    #[test]
    fn test_derive_arxiv_url_already_pdf() {
        let urls = derive_pdf_urls("https://arxiv.org/abs/2301.12345.pdf");
        // .pdf が二重にならない
        assert!(urls[0].ends_with(".pdf"));
        assert!(!urls[0].ends_with(".pdf.pdf"));
    }

    #[test]
    fn test_derive_doi_url() {
        let urls = derive_pdf_urls("https://doi.org/10.1109/SP.2024.001");
        assert_eq!(urls, vec!["https://doi.org/10.1109/SP.2024.001"]);
    }

    #[test]
    fn test_derive_iacr_url() {
        let urls = derive_pdf_urls("https://eprint.iacr.org/2024/123");
        assert_eq!(urls, vec!["https://eprint.iacr.org/2024/123.pdf"]);
    }

    #[test]
    fn test_derive_openreview_url() {
        let urls = derive_pdf_urls("https://openreview.net/forum?id=abc123");
        assert_eq!(urls, vec!["https://openreview.net/pdf?id=abc123"]);
    }

    #[test]
    fn test_derive_unknown_url() {
        let urls = derive_pdf_urls("https://example.com/paper/123");
        assert_eq!(urls, vec!["https://example.com/paper/123"]);
    }

    #[test]
    fn test_derive_empty_url() {
        let urls = derive_pdf_urls("");
        assert!(urls.is_empty());
    }

    // --- collect_candidate_urls テスト ---

    #[test]
    fn test_collect_dedup() {
        let urls = collect_candidate_urls(
            Some("https://arxiv.org/pdf/2301.12345.pdf"),
            "https://arxiv.org/abs/2301.12345",
        );
        // S2由来のURLとarXiv変換後のURLが同一なら重複排除される
        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn test_collect_priority_order() {
        let urls = collect_candidate_urls(
            Some("https://s2-pdf.example.com/paper.pdf"),
            "https://arxiv.org/abs/2301.12345",
        );
        assert_eq!(urls[0], "https://s2-pdf.example.com/paper.pdf");
        assert_eq!(urls[1], "https://arxiv.org/pdf/2301.12345.pdf");
    }

    #[test]
    fn test_collect_no_pdf_url() {
        let urls = collect_candidate_urls(None, "https://doi.org/10.1109/SP.2024.001");
        assert_eq!(urls, vec!["https://doi.org/10.1109/SP.2024.001"]);
    }
}
