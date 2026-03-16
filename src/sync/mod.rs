use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use indicatif::MultiProgress;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Instant;

use crate::cache::CacheDb;
use crate::cli::{parse_year_range, SyncArgs};
use crate::conference;
use crate::scraper::build_http_client;
use crate::types::compute_id;

pub mod progress;

use progress::SyncProgress;

/// グローバルレートリミッター
/// 前回のリクエストから最低 `interval` 秒空けることを保証する
#[derive(Clone)]
struct RateLimiter {
    last_request: Arc<Mutex<Instant>>,
    interval: Duration,
}

impl RateLimiter {
    fn new(interval: Duration) -> Self {
        Self {
            // 初回は即時実行可能にするため過去の時刻を設定
            last_request: Arc::new(Mutex::new(Instant::now() - interval)),
            interval,
        }
    }

    /// 次のリクエストが許可されるまで待機する
    async fn acquire(&self) {
        let mut last = self.last_request.lock().await;
        let elapsed = last.elapsed();
        if elapsed < self.interval {
            tokio::time::sleep(self.interval - elapsed).await;
        }
        *last = Instant::now();
    }
}

/// tokio::spawn に渡すための設定値
#[derive(Clone)]
struct SyncConfig {
    year: Option<String>,
    incremental: bool,
    force: bool,
    jobs: usize,
    checkpoint: usize,
}

pub async fn run_sync(args: &SyncArgs, cache_dir: &Path) -> Result<()> {
    let interval = Duration::from_secs_f64(args.interval);

    // conference が空の場合はエラー
    if args.conference.is_empty() {
        anyhow::bail!("At least one conference must be specified with --conference");
    }

    // Build scrapers for all requested conferences
    let scrapers: Vec<Arc<dyn conference::ConferenceScraper>> = args
        .conference
        .iter()
        .map(|id| conference::get_scraper(id, interval))
        .collect::<Result<_>>()?;

    // 単一会議の場合はそのまま実行（オーバーヘッド回避）
    if scrapers.len() == 1 {
        let scraper = scrapers.into_iter().next().unwrap();
        let rate_limiter = RateLimiter::new(interval);
        let multi = Arc::new(MultiProgress::new());
        let config = SyncConfig {
            year: args.year.clone(),
            incremental: args.incremental,
            force: args.force,
            jobs: args.jobs,
            checkpoint: args.checkpoint,
        };
        return run_sync_single(scraper, &config, cache_dir, rate_limiter, multi).await;
    }

    // Group rate limiters by backend_id
    let mut rate_limiters: HashMap<String, RateLimiter> = HashMap::new();
    for scraper in &scrapers {
        rate_limiters
            .entry(scraper.backend_id().to_string())
            .or_insert_with(|| RateLimiter::new(interval));
    }

    let config = SyncConfig {
        year: args.year.clone(),
        incremental: args.incremental,
        force: args.force,
        jobs: args.jobs,
        checkpoint: args.checkpoint,
    };

    let multi = Arc::new(MultiProgress::new());

    // Spawn one task per conference
    let mut handles = Vec::new();
    for scraper in scrapers {
        let limiter = rate_limiters[scraper.backend_id()].clone();
        let config = config.clone();
        let cache_dir = cache_dir.to_path_buf();
        let conf_name = scraper.name().to_string();
        let multi = Arc::clone(&multi);

        let handle = tokio::spawn(async move {
            run_sync_single(scraper, &config, &cache_dir, limiter, multi).await
        });
        handles.push((conf_name, handle));
    }

    // Collect results, report errors
    let mut had_error = false;
    for (name, handle) in handles {
        match handle.await? {
            Ok(()) => tracing::info!("{} sync completed", name),
            Err(e) => {
                tracing::error!("{} sync failed: {}", name, e);
                had_error = true;
            }
        }
    }

    if had_error {
        anyhow::bail!("One or more conference syncs failed. See errors above.");
    }

    Ok(())
}

async fn run_sync_single(
    scraper: Arc<dyn conference::ConferenceScraper>,
    config: &SyncConfig,
    cache_dir: &Path,
    rate_limiter: RateLimiter,
    multi: Arc<MultiProgress>,
) -> Result<()> {
    let client = build_http_client()?;
    let mut db = CacheDb::open(cache_dir)?;

    tracing::info!("Syncing {} papers...", scraper.name());

    // Get available years
    let all_years = scraper.fetch_years(&client).await?;
    let years = if let Some(ref year_str) = config.year {
        let requested = parse_year_range(year_str)?;
        all_years
            .into_iter()
            .filter(|y| requested.contains(y))
            .collect::<Vec<_>>()
    } else {
        all_years
    };

    tracing::info!("[{}] Target years: {:?}", scraper.name(), years);

    let mut progress = SyncProgress::with_shared(multi, scraper.name());
    progress.start_years(years.len() as u64);

    for year in &years {
        // Check if already completed
        if config.incremental && db.is_year_completed(scraper.id(), *year)? {
            progress.skip_year(*year, "--incremental");
            continue;
        }

        // Force mode: clear existing data
        if config.force {
            db.clear_year(scraper.id(), *year)?;
            progress.log(&format!("Year {} cleared (--force)", year));
        }

        progress.log(&format!("Fetching paper list for {}...", year));
        let paper_entries = match scraper.fetch_paper_list(&client, *year).await {
            Ok(entries) => entries,
            Err(e) => {
                progress.log(&format!(
                    "  WARN: Failed to fetch paper list for {}: {}, skipping",
                    year, e
                ));
                progress.finish_year();
                continue;
            }
        };
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

        progress.log(&format!(
            "Year {}: {} total, {} already fetched, {} to fetch",
            year,
            total,
            fetched_ids.len(),
            pending.len()
        ));

        if pending.is_empty() {
            let total_count = db.fetched_ids(scraper.id(), *year)?.len();
            db.mark_completed(scraper.id(), *year, total_count)?;
            progress.finish_year();
            continue;
        }

        let skipped = fetched_ids.len() as u64;
        let paper_bar = progress.start_papers(*year, pending.len() as u64, skipped);

        // Process in checkpoint-sized chunks
        let semaphore = Arc::new(Semaphore::new(config.jobs));

        for chunk in pending.chunks(config.checkpoint) {
            let mut handles = Vec::new();

            for entry in chunk {
                let permit = semaphore.clone().acquire_owned().await?;
                let client = client.clone();
                let entry = entry.clone();
                let scraper = Arc::clone(&scraper);
                let limiter = rate_limiter.clone();

                let handle = tokio::spawn(async move {
                    limiter.acquire().await;
                    let result = scraper.fetch_paper_detail(&client, &entry).await;
                    drop(permit);
                    result
                });
                handles.push(handle);
            }

            let mut buffer = Vec::new();
            let mut errors = Vec::new();
            for handle in handles {
                match handle.await? {
                    Ok(paper) => {
                        buffer.push(paper);
                        paper_bar.inc(1);
                    }
                    Err(e) => {
                        errors.push(format!("{}", e));
                        paper_bar.inc(1);
                    }
                }
            }

            for err in &errors {
                progress.log(&format!("  WARN: Failed to fetch paper: {}", err));
            }

            let inserted = db.insert_papers(&buffer)?;
            tracing::debug!(
                "Checkpoint: inserted {} papers for {}/{}",
                inserted,
                scraper.id(),
                year
            );
        }

        paper_bar.finish_with_message("done");

        // Mark completed
        let total_count = db.fetched_ids(scraper.id(), *year)?.len();
        db.mark_completed(scraper.id(), *year, total_count)?;
        progress.log(&format!("Year {} completed: {} papers", year, total_count));
        progress.finish_year();
    }

    progress.finish();
    tracing::info!("Sync completed for {}", scraper.name());
    Ok(())
}
