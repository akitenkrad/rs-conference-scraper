mod cache;
mod cli;
mod conference;
mod filter;
mod output;
mod scraper;
mod sync;
mod types;

use anyhow::Result;
use clap::Parser;
use cli::{CacheCommands, Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose {
        "conf_scraper=debug"
    } else {
        "conf_scraper=info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .init();

    // Resolve cache directory
    let cache_dir = resolve_cache_dir(&cli.cache_dir)?;

    match cli.command {
        Commands::Sync(args) => {
            sync::run_sync(&args, &cache_dir).await?;
        }
        Commands::Filter(args) => {
            let db = cache::CacheDb::open(&cache_dir)?;

            // Parse years
            let years = args
                .year
                .as_ref()
                .map(|y| cli::parse_year_range(y))
                .transpose()?;

            // Load papers from cache
            let papers = db.load_papers(args.conference.as_deref(), years.as_deref())?;

            if papers.is_empty() {
                if args.offline {
                    anyhow::bail!("No cached papers found. Run 'sync' first.");
                }
                println!("No papers found matching the criteria.");
                return Ok(());
            }

            tracing::info!("Loaded {} papers from cache", papers.len());

            // Build and apply synchronous filter pipeline (category → keyword)
            let pipeline = filter::FilterPipeline::build(&args);
            let mut scored = pipeline.apply(papers);

            tracing::info!("{} papers after keyword/category filters", scored.len());

            // Apply LLM filter if requested
            if args.filter.iter().any(|f| f == "llm") {
                let theme = args
                    .theme
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("--theme is required when using llm filter"))?;
                let api_key = args.api_key.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("--api-key or ANTHROPIC_API_KEY is required for llm filter")
                })?;

                let llm = filter::llm::LlmFilter::new(
                    api_key.clone(),
                    theme.clone(),
                    args.threshold,
                    4,
                );
                scored = llm.score_papers(scored).await?;
            }

            // Build output
            let conferences = args
                .conference
                .iter()
                .cloned()
                .collect::<Vec<String>>();
            let output = output::FilterOutput {
                query: output::QueryInfo {
                    conferences,
                    years: years.unwrap_or_default(),
                    theme: args.theme.clone(),
                    filters: args.filter.clone(),
                    combine: args.combine.clone(),
                },
                total: scored.len(),
                papers: scored,
            };

            // Write output
            let json = serde_json::to_string_pretty(&output)?;
            if let Some(ref path) = args.output {
                std::fs::write(path, &json)?;
                tracing::info!("Output written to {}", path.display());
            } else {
                println!("{}", json);
            }
        }
        Commands::Stats(args) => {
            let db = cache::CacheDb::open(&cache_dir)?;
            let years = args
                .year
                .as_ref()
                .map(|y| cli::parse_year_range(y))
                .transpose()?;
            let stats = db.stats(args.conference.as_deref(), years.as_deref())?;

            if stats.total == 0 {
                println!("No papers found. Run 'sync' first.");
                return Ok(());
            }

            // サマリー
            println!("=== Paper Statistics ===\n");
            println!("Total papers:       {}", stats.total);
            println!("With abstract:      {}", stats.with_abstract);
            println!("Without abstract:   {}", stats.without_abstract);
            println!("Unique authors:     {}", stats.unique_authors);

            // 会議別
            if stats.by_conference.len() > 1 || args.conference.is_none() {
                println!("\n--- By Conference ---");
                for (conf, count) in &stats.by_conference {
                    println!("  {:<16} {:>6} papers", conf, count);
                }
            }

            // 年度別
            println!("\n--- By Year ---");
            for (year, count) in &stats.by_year {
                let bar = "#".repeat((*count as f64 / stats.total as f64 * 40.0) as usize);
                println!("  {} {:>6}  {}", year, count, bar);
            }

            // カテゴリ別
            if !stats.by_category.is_empty() {
                println!("\n--- By Category ---");
                for (cat, count) in &stats.by_category {
                    println!("  {:<30} {:>6}", cat, count);
                }
            }

            // トップ著者
            if !stats.top_authors.is_empty() {
                println!("\n--- Top 10 Authors ---");
                for (i, (author, count)) in stats.top_authors.iter().enumerate() {
                    println!("  {:>2}. {:<30} {:>4} papers", i + 1, author, count);
                }
            }
        }
        Commands::Cache { command } => {
            let mut db = cache::CacheDb::open(&cache_dir)?;
            match command {
                CacheCommands::Status { conference } => {
                    let statuses = db.status(conference.as_deref())?;
                    if statuses.is_empty() {
                        println!("No cached data found.");
                    } else {
                        println!(
                            "{:<12} {:<6} {:<8} {:<22} {}",
                            "Conference", "Year", "Papers", "Last Synced", "Status"
                        );
                        println!("{}", "-".repeat(60));
                        for s in &statuses {
                            println!(
                                "{:<12} {:<6} {:<8} {:<22} {}",
                                s.conference,
                                s.year,
                                s.paper_count,
                                s.synced_at.as_deref().unwrap_or("-"),
                                if s.completed { "completed" } else { "partial" },
                            );
                        }
                    }
                }
                CacheCommands::Clear { conference, year } => {
                    if conference.is_none() && year.is_none() {
                        tracing::warn!("Clearing ALL cached data");
                    }
                    let deleted = db.clear(conference.as_deref(), year)?;
                    println!("Deleted {} papers from cache.", deleted);
                }
            }
        }
        Commands::ListConferences => {
            let conferences = conference::list_conferences();
            println!("{:<12} {}", "ID", "Name");
            println!("{}", "-".repeat(24));
            for (id, name) in &conferences {
                println!("{:<12} {}", id, name);
            }
        }
    }

    Ok(())
}

fn resolve_cache_dir(path: &str) -> Result<std::path::PathBuf> {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            Ok(home.join(&path[2..]))
        } else {
            Ok(std::path::PathBuf::from(path))
        }
    } else {
        Ok(std::path::PathBuf::from(path))
    }
}
