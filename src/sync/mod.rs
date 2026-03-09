use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::cache::CacheDb;
use crate::cli::{parse_year_range, SyncArgs};
use crate::conference;
use crate::scraper::build_http_client;
use crate::types::compute_id;

pub mod progress;

use progress::SyncProgress;

pub async fn run_sync(args: &SyncArgs, cache_dir: &std::path::Path) -> Result<()> {
    let interval = Duration::from_secs_f64(args.interval);
    let scraper = conference::get_scraper(&args.conference, interval)?;
    let client = build_http_client()?;
    let mut db = CacheDb::open(cache_dir)?;

    tracing::info!("Syncing {} papers...", scraper.name());

    // Get available years
    let all_years = scraper.fetch_years(&client).await?;
    let years = if let Some(ref year_str) = args.year {
        let requested = parse_year_range(year_str)?;
        all_years
            .into_iter()
            .filter(|y| requested.contains(y))
            .collect::<Vec<_>>()
    } else {
        all_years
    };

    tracing::info!("Target years: {:?}", years);

    let mut progress = SyncProgress::new();
    progress.start_years(years.len() as u64);

    for year in &years {
        // Check if already completed
        if args.incremental && db.is_year_completed(scraper.id(), *year)? {
            tracing::info!("Year {} already completed, skipping (--incremental)", year);
            progress.skip_year(*year, "--incremental");
            continue;
        }

        // Force mode: clear existing data
        if args.force {
            db.clear_year(scraper.id(), *year)?;
            tracing::info!("Year {} cleared (--force)", year);
        }

        tracing::info!(
            "Fetching paper list for {} {}...",
            scraper.name(),
            year
        );
        let paper_entries = scraper.fetch_paper_list(&client, *year).await?;
        let total = paper_entries.len();

        // Filter out already fetched papers
        let fetched_ids = db.fetched_ids(scraper.id(), *year)?;
        let pending: Vec<_> = paper_entries
            .into_iter()
            .filter(|e| {
                let id = compute_id(&e.title);
                !fetched_ids.contains(&id)
            })
            .collect();

        tracing::info!(
            "Year {}: {} total, {} already fetched, {} to fetch",
            year,
            total,
            fetched_ids.len(),
            pending.len()
        );

        if pending.is_empty() {
            let total_count = db.fetched_ids(scraper.id(), *year)?.len();
            db.mark_completed(scraper.id(), *year, total_count)?;
            progress.finish_year();
            continue;
        }

        let skipped = fetched_ids.len() as u64;
        let paper_bar = progress.start_papers(*year, pending.len() as u64, skipped);

        // Process in checkpoint-sized chunks
        let semaphore = Arc::new(Semaphore::new(args.jobs));

        for chunk in pending.chunks(args.checkpoint) {
            let mut handles = Vec::new();

            for entry in chunk {
                let permit = semaphore.clone().acquire_owned().await?;
                let client = client.clone();
                let entry = entry.clone();
                let scraper = Arc::clone(&scraper);

                let handle = tokio::spawn(async move {
                    let result = scraper.fetch_paper_detail(&client, &entry).await;
                    drop(permit);
                    result
                });
                handles.push(handle);
            }

            let mut buffer = Vec::new();
            for handle in handles {
                match handle.await? {
                    Ok(paper) => {
                        buffer.push(paper);
                        paper_bar.inc(1);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to fetch paper: {}", e);
                        paper_bar.inc(1);
                    }
                }
            }

            let inserted = db.insert_papers(&buffer)?;
            tracing::info!(
                "Checkpoint: inserted {} papers for {}/{}",
                inserted,
                scraper.id(),
                year
            );
        }

        paper_bar.finish();

        // Mark completed
        let total_count = db.fetched_ids(scraper.id(), *year)?.len();
        db.mark_completed(scraper.id(), *year, total_count)?;
        tracing::info!("Year {} completed: {} papers", year, total_count);
        progress.finish_year();
    }

    progress.finish();
    tracing::info!("Sync completed for {}", scraper.name());
    Ok(())
}
