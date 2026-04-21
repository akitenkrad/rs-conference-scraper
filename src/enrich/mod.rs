mod arxiv;
mod crossref;
mod html_scraper;
mod openalex;
mod pdf;
mod site_parsers;

use crate::cache::CacheDb;
use crate::cli::EnrichArgs;
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use ss_tools::structs::PaperField;
use ss_tools::{QueryParams, SemanticScholar};
use std::path::Path;

// ANSI カラーコード
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[1;33m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

/// スキップ原因の分類
struct SkipCounts {
    /// 全ティアが応答したがabstractが空だった
    all_empty: usize,
    /// 全ティアがAPIエラーだった
    all_error: usize,
    /// エラーと空の混合
    mixed: usize,
}

impl SkipCounts {
    fn new() -> Self {
        Self {
            all_empty: 0,
            all_error: 0,
            mixed: 0,
        }
    }

    fn total(&self) -> usize {
        self.all_empty + self.all_error + self.mixed
    }

    /// スキップ原因を分類して加算する
    fn record(&mut self, empty_count: usize, error_count: usize) {
        let total = empty_count + error_count;
        if total == 0 {
            return;
        }
        if error_count == total {
            self.all_error += 1;
        } else if empty_count == total {
            self.all_empty += 1;
        } else {
            self.mixed += 1;
        }
    }
}

/// 各ティアの取得件数
struct TierCounts {
    s2: usize,
    oa: usize,
    arxiv: usize,
    cr: usize,
    pdf: usize,
    html: usize,
    skip: SkipCounts,
}

/// 現在アクティブなティア
enum ActiveTier {
    S2,
    OpenAlex,
    ArXiv,
    CrossRef,
    Html,
    Pdf,
    Done,
}

impl TierCounts {
    fn new() -> Self {
        Self {
            s2: 0,
            oa: 0,
            arxiv: 0,
            cr: 0,
            pdf: 0,
            html: 0,
            skip: SkipCounts::new(),
        }
    }

    fn total_enriched(&self) -> usize {
        self.s2 + self.oa + self.arxiv + self.cr + self.pdf + self.html
    }

    /// ティアラベルに色をつける: ヒットありなら緑，なしならdim
    fn colored_count(&self, label: &str, count: usize) -> String {
        if count > 0 {
            format!("{GREEN}{}{RESET}:{}", label, count)
        } else {
            format!("{DIM}{}{RESET}:{}", label, count)
        }
    }

    /// プログレスバー用のメッセージを生成する
    fn build_msg(&self, active: &ActiveTier) -> String {
        let counts = format!(
            "{} {} {} {} {} {} {RED}skip{RESET}:{}(E:{}/R:{}/M:{})",
            self.colored_count("S2", self.s2),
            self.colored_count("OA", self.oa),
            self.colored_count("arXiv", self.arxiv),
            self.colored_count("CR", self.cr),
            self.colored_count("HTML", self.html),
            self.colored_count("PDF", self.pdf),
            self.skip.total(),
            self.skip.all_empty,
            self.skip.all_error,
            self.skip.mixed,
        );

        let tier_indicator = match active {
            ActiveTier::S2 => format!("{YELLOW}▶ S2{RESET}"),
            ActiveTier::OpenAlex => format!("{YELLOW}▶ OpenAlex{RESET}"),
            ActiveTier::ArXiv => format!("{YELLOW}▶ arXiv{RESET}"),
            ActiveTier::CrossRef => format!("{YELLOW}▶ CrossRef{RESET}"),
            ActiveTier::Html => format!("{YELLOW}▶ HTML{RESET}"),
            ActiveTier::Pdf => format!("{YELLOW}▶ PDF{RESET}"),
            ActiveTier::Done => format!("{GREEN}✓{RESET}"),
        };

        format!("{} | {}", counts, tier_indicator)
    }
}

/// Semantic ScholarおよびOpenAlex/arXiv/CrossRef/HTMLスクレイピング/PDFのフォールバックチェインで
/// キャッシュ済み論文のメタデータを補完する
pub async fn run_enrich(args: &EnrichArgs, cache_dir: &Path) -> Result<()> {
    let db = CacheDb::open(cache_dir)?;

    // Parse years
    let years = args
        .year
        .as_ref()
        .map(|y| crate::cli::parse_year_range(y))
        .transpose()?;

    // 対象論文の読み込み
    let papers = if args.force {
        if args.conference.is_empty() {
            db.load_papers(None, years.as_deref())?
        } else {
            let mut all = Vec::new();
            for conf in &args.conference {
                all.extend(db.load_papers(Some(conf), years.as_deref())?);
            }
            all
        }
    } else {
        if args.conference.is_empty() {
            db.load_papers_without_abstract(None, years.as_deref())?
        } else {
            let mut all = Vec::new();
            for conf in &args.conference {
                all.extend(db.load_papers_without_abstract(Some(conf), years.as_deref())?);
            }
            all
        }
    };

    if papers.is_empty() {
        println!("No papers to enrich.");
        return Ok(());
    }

    tracing::info!(
        "Found {} papers to enrich via 6-tier fallback chain",
        papers.len()
    );
    tracing::info!(
        "Skip reason legend: E=all_empty, R=all_error, M=mixed"
    );

    // SemanticScholar クライアント初期化
    let mut ss = SemanticScholar::new();

    // HTTP クライアント（全ティアで共有）
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("conf-scraper/0.1 (academic research tool)")
        .build()
        .context("Failed to create HTTP client")?;

    // プログレスバー
    let pb = ProgressBar::new(papers.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}) {msg}",
            )?
            .progress_chars("█▓▒░"),
    );

    let mut counts = TierCounts::new();
    let interval = std::time::Duration::from_secs_f64(args.interval);

    for paper in papers.iter() {
        // Semantic Scholarにタイトルで問い合わせ
        let mut query_params = QueryParams::default();
        query_params.query_text(&paper.title);
        query_params.fields(vec![
            PaperField::Title,
            PaperField::Abstract,
            PaperField::OpenAccessPdf,
            PaperField::ExternalIds,
        ]);

        let mut abstract_text = String::new();
        let mut pdf_url: Option<String> = None;
        let mut source = "none";
        let mut tier_empty_count: usize = 0;
        let mut tier_error_count: usize = 0;

        // Tier 1: Semantic Scholar
        pb.set_message(counts.build_msg(&ActiveTier::S2));
        match ss
            .query_a_paper_by_title(query_params, args.retry, args.wait)
            .await
        {
            Ok(ss_paper) => {
                let ss_abstract = ss_paper.abstract_text.unwrap_or_default();
                pdf_url = ss_paper.open_access_pdf.and_then(|p| p.url);

                if !ss_abstract.is_empty() {
                    abstract_text = ss_abstract;
                    source = "s2";
                    tracing::debug!("Tier1(S2) hit for '{}'", paper.title);
                } else {
                    tier_empty_count += 1;
                }
            }
            Err(e) => {
                tier_error_count += 1;
                tracing::debug!(
                    "Semantic Scholar failed for '{}': {}",
                    paper.title,
                    e
                );
            }
        }

        // Tier 2: OpenAlex
        if abstract_text.is_empty() {
            pb.set_message(counts.build_msg(&ActiveTier::OpenAlex));
            match openalex::fetch_abstract_via_openalex(&http_client, &paper.title).await {
                Ok(text) if !text.is_empty() => {
                    abstract_text = text;
                    source = "oa";
                    tracing::debug!("Tier2(OA) hit for '{}'", paper.title);
                }
                Ok(_) => {
                    tier_empty_count += 1;
                    tracing::debug!("OpenAlex returned empty for '{}'", paper.title);
                }
                Err(e) => {
                    tier_error_count += 1;
                    tracing::debug!("OpenAlex failed for '{}': {}", paper.title, e);
                }
            }
        }

        // Tier 3: arXiv（レート制限: 3秒間隔）
        // arXivプレプリントが存在しない会議（IEEE/ACM系等）ではスキップ
        if abstract_text.is_empty()
            && crate::conference::has_arxiv_preprints(&paper.conference)
        {
            pb.set_message(counts.build_msg(&ActiveTier::ArXiv));
            // arXivのレート制限を守るため3秒待つ
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            match arxiv::fetch_abstract_via_arxiv(&http_client, &paper.title).await {
                Ok(text) if !text.is_empty() => {
                    abstract_text = text;
                    source = "arxiv";
                    tracing::debug!("Tier3(arXiv) hit for '{}'", paper.title);
                }
                Ok(_) => {
                    tier_empty_count += 1;
                    tracing::debug!("arXiv returned empty for '{}'", paper.title);
                }
                Err(e) => {
                    tier_error_count += 1;
                    tracing::debug!("arXiv failed for '{}': {}", paper.title, e);
                }
            }
        }

        // Tier 4: CrossRef
        if abstract_text.is_empty() {
            pb.set_message(counts.build_msg(&ActiveTier::CrossRef));
            match crossref::fetch_abstract_via_crossref(&http_client, &paper.title).await {
                Ok(text) if !text.is_empty() => {
                    abstract_text = text;
                    source = "cr";
                    tracing::debug!("Tier4(CR) hit for '{}'", paper.title);
                }
                Ok(_) => {
                    tier_empty_count += 1;
                    tracing::debug!("CrossRef returned empty for '{}'", paper.title);
                }
                Err(e) => {
                    tier_error_count += 1;
                    tracing::debug!("CrossRef failed for '{}': {}", paper.title, e);
                }
            }
        }

        // Tier 5: HTMLスクレイピング（直接抽出のみ）
        if abstract_text.is_empty() {
            pb.set_message(counts.build_msg(&ActiveTier::Html));
            match html_scraper::fetch_abstract_via_html(&http_client, &paper.title, &paper.url)
                .await
            {
                Ok(html_scraper::HtmlResult::Direct(text)) => {
                    abstract_text = text;
                    source = "html";
                    tracing::debug!("Tier5(HTML) hit for '{}'", paper.title);
                }
                Ok(html_scraper::HtmlResult::Empty) => {
                    tier_empty_count += 1;
                    tracing::debug!("HTML scraping returned empty for '{}'", paper.title);
                }
                Err(e) => {
                    tier_error_count += 1;
                    tracing::debug!("HTML scraping failed for '{}': {}", paper.title, e);
                }
            }
        }

        // Tier 6: PDF直接抽出（S2由来のpdf_url + paper.urlの両方を試行）
        if abstract_text.is_empty() {
            pb.set_message(counts.build_msg(&ActiveTier::Pdf));
            match pdf::fetch_abstract_via_pdf_urls(
                &http_client,
                pdf_url.as_deref(),
                &paper.url,
            )
            .await
            {
                Ok(text) if !text.is_empty() => {
                    abstract_text = text;
                    source = "pdf";
                    tracing::debug!("Tier6(PDF) hit for '{}'", paper.title);
                }
                Ok(_) => {
                    tier_empty_count += 1;
                    tracing::debug!("PDF extraction returned empty for '{}'", paper.title);
                }
                Err(e) => {
                    tier_error_count += 1;
                    tracing::debug!("PDF extraction failed for '{}': {}", paper.title, e);
                }
            }
        }

        // 結果の反映
        if !abstract_text.is_empty() {
            db.update_paper_metadata(
                &paper.id,
                &paper.conference,
                paper.year,
                &abstract_text,
                pdf_url.as_deref(),
            )?;
            match source {
                "s2" => counts.s2 += 1,
                "oa" => counts.oa += 1,
                "arxiv" => counts.arxiv += 1,
                "cr" => counts.cr += 1,
                "pdf" => counts.pdf += 1,
                "html" => counts.html += 1,
                _ => {}
            }
        } else {
            counts.skip.record(tier_empty_count, tier_error_count);
        }

        pb.set_message(counts.build_msg(&ActiveTier::Done));
        pb.inc(1);

        // レート制限
        tokio::time::sleep(interval).await;
    }

    pb.finish_and_clear();

    let skip = &counts.skip;
    println!(
        "Enrichment complete: {} enriched (S2: {}, OA: {}, arXiv: {}, CR: {}, HTML: {}, PDF: {}), {} skipped (all_empty: {}, all_error: {}, mixed: {})",
        counts.total_enriched(),
        counts.s2,
        counts.oa,
        counts.arxiv,
        counts.cr,
        counts.html,
        counts.pdf,
        skip.total(),
        skip.all_empty,
        skip.all_error,
        skip.mixed,
    );

    Ok(())
}
