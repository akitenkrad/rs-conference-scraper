mod cache;
mod cli;
mod conference;
mod enrich;
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

            // Apply limit (0 = unlimited)
            if args.limit > 0 {
                scored.truncate(args.limit);
                tracing::info!("Limited output to {} papers", scored.len());
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
                    filters: args.filter.clone(),
                    combine: args.combine.clone(),
                },
                total: scored.len(),
                papers: scored,
            };

            // Write output
            let formatted = output.format(&args.format)?;
            if let Some(ref path) = args.output {
                std::fs::write(path, &formatted)?;
                tracing::info!("Output written to {}", path.display());
            } else {
                println!("{}", formatted);
            }
        }
        Commands::Enrich(args) => {
            enrich::run_enrich(&args, &cache_dir).await?;
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

            // --show が未指定なら全セクション表示
            let show_all = args.show.is_empty();
            let show_summary = show_all || args.show.iter().any(|s| s == "summary");
            let show_conferences = show_all || args.show.iter().any(|s| s == "conferences");
            let show_years = show_all || args.show.iter().any(|s| s == "years");
            let show_categories = show_all || args.show.iter().any(|s| s == "categories");
            let show_authors = show_all || args.show.iter().any(|s| s == "authors");
            let show_abstracts = show_all || args.show.iter().any(|s| s == "abstracts");

            // ヘッダー
            if let Some(ref conf) = args.conference {
                println!("=== {} Paper Statistics ===\n", conf.to_uppercase());
            } else {
                println!("=== Paper Statistics ===\n");
            }

            // サマリー
            if show_summary {
                print_ascii_table(
                    &["Metric", "Value"],
                    &[
                        vec!["Total papers".to_string(), stats.total.to_string()],
                        vec!["With abstract".to_string(), stats.with_abstract.to_string()],
                        vec![
                            "Without abstract".to_string(),
                            stats.without_abstract.to_string(),
                        ],
                        vec![
                            "Unique authors".to_string(),
                            stats.unique_authors.to_string(),
                        ],
                    ],
                );
            }

            // 国際会議ごとの論文数
            if show_conferences && args.conference.is_none() {
                let rows: Vec<Vec<String>> = stats
                    .by_conference
                    .iter()
                    .map(|(conf, count)| vec![conf.to_string(), count.to_string()])
                    .collect();

                println!("\n--- By Conference ---");
                print_ascii_table(&["Conference", "Papers"], &rows);
            }

            // 年度別
            if show_years {
                println!("\n--- By Year ---");
                let year_rows: Vec<Vec<String>> = stats
                    .by_year
                    .iter()
                    .map(|(year, count)| {
                        let bar =
                            "#".repeat((*count as f64 / stats.total as f64 * 40.0) as usize);
                        vec![year.to_string(), count.to_string(), bar]
                    })
                    .collect();
                print_ascii_table(&["Year", "Papers", "Distribution"], &year_rows);
            }

            // カテゴリ別
            if show_categories && !stats.by_category.is_empty() {
                println!("\n--- By Category ---");
                let cat_rows: Vec<Vec<String>> = stats
                    .by_category
                    .iter()
                    .map(|(cat, count)| {
                        let pct = format!("{:.1}%", *count as f64 / stats.total as f64 * 100.0);
                        vec![cat.to_string(), count.to_string(), pct]
                    })
                    .collect();
                print_ascii_table(&["Category", "Papers", "Ratio"], &cat_rows);
            }

            // トップ著者
            if show_authors && !stats.top_authors.is_empty() {
                println!("\n--- Top Authors ---");
                let author_rows: Vec<Vec<String>> = stats
                    .top_authors
                    .iter()
                    .map(|(author, count)| vec![author.to_string(), count.to_string()])
                    .collect();
                print_ascii_table(&["Author", "Papers"], &author_rows);
            }

            // 年度別abstract内訳
            if show_abstracts && !stats.abstract_by_year.is_empty() {
                println!("\n--- Abstract Coverage by Year ---");
                let abs_rows: Vec<Vec<String>> = stats
                    .abstract_by_year
                    .iter()
                    .map(|(year, with_abs, without_abs)| {
                        let total_year = with_abs + without_abs;
                        let pct = if total_year > 0 {
                            format!("{:.1}%", *with_abs as f64 / total_year as f64 * 100.0)
                        } else {
                            "N/A".to_string()
                        };
                        vec![
                            year.to_string(),
                            with_abs.to_string(),
                            without_abs.to_string(),
                            pct,
                        ]
                    })
                    .collect();
                print_ascii_table(
                    &["Year", "With Abstract", "Without Abstract", "Coverage"],
                    &abs_rows,
                );
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
            let rows: Vec<Vec<String>> = conferences
                .iter()
                .map(|(id, name, field)| {
                    vec![id.to_string(), name.to_string(), field.to_string()]
                })
                .collect();
            print_ascii_table(&["ID", "Name", "Field"], &rows);
        }
    }

    Ok(())
}

/// ASCIIテーブルを出力する
fn print_ascii_table(headers: &[&str], rows: &[Vec<String>]) {
    // 各カラムの最大幅を計算
    let col_count = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_count && cell.len() > widths[i] {
                widths[i] = cell.len();
            }
        }
    }

    // 罫線
    let separator: String = widths
        .iter()
        .map(|w| "-".repeat(w + 2))
        .collect::<Vec<_>>()
        .join("+");
    let separator = format!("+{}+", separator);

    // ヘッダー
    println!("{}", separator);
    let header_line: String = widths
        .iter()
        .enumerate()
        .map(|(i, w)| format!(" {:<width$} ", headers[i], width = w))
        .collect::<Vec<_>>()
        .join("|");
    println!("|{}|", header_line);
    println!("{}", separator);

    // データ行
    for row in rows {
        let line: String = widths
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
                // 数値は右寄せ，それ以外は左寄せ
                if cell.parse::<f64>().is_ok() {
                    format!(" {:>width$} ", cell, width = w)
                } else {
                    format!(" {:<width$} ", cell, width = w)
                }
            })
            .collect::<Vec<_>>()
            .join("|");
        println!("|{}|", line);
    }
    println!("{}", separator);
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
