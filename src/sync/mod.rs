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

/// `--incremental` でも，完了済みの年の一覧を取り直すか．
///
/// 開催直後の年は，取得元に一部しか載っていない段階で同期して «完了» になることがある
/// （例: NeurIPS 2025 を Creative AI トラックの 64 件だけで完了にした．2026-10-11）．
/// そのため今年と前年は完了済みでも一覧を取り直し，未取得の論文があれば差分だけを取る．
/// 新しい論文が無ければ下の «pending が空» の経路で完了のまま終わる．
/// それより古い年は変わらないものとして飛ばす（毎回の一覧取得を増やさないため）．
fn recheck_completed_year(year: u16, current_year: u16) -> bool {
    year.saturating_add(1) >= current_year
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

    // 取りこぼした年（一覧を取れなかった年・論文の取得に失敗した年）．最後にまとめて失敗として返す
    let mut incomplete: Vec<String> = Vec::new();

    for year in &years {
        // Check if already completed
        if config.incremental
            && db.is_year_completed(scraper.id(), *year)?
            && !recheck_completed_year(*year, conference::current_year())
        {
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
                progress.warn(&format!(
                    "Failed to fetch paper list for {}: {:#}, skipping",
                    year, e
                ));
                incomplete.push(format!("{} (paper list: {:#})", year, e));
                progress.finish_year();
                continue;
            }
        };
        let total = paper_entries.len();

        // 一覧が空の年（未開催・未公開）は completed にしない．
        // completed にすると，公開後も --incremental で永久に飛ばされる．
        if total == 0 {
            progress.warn(&format!(
                "No papers listed for {} (not yet published?), not marking completed",
                year
            ));
            progress.finish_year();
            continue;
        }

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
        let mut failed_papers = 0usize;

        for chunk in pending.chunks(config.checkpoint) {
            let mut handles = Vec::new();

            for entry in chunk {
                let permit = semaphore.clone().acquire_owned().await?;
                let client = client.clone();
                let entry = entry.clone();
                let scraper = Arc::clone(&scraper);
                let limiter = rate_limiter.clone();

                let local = scraper.details_are_local();
                let handle = tokio::spawn(async move {
                    if !local {
                        limiter.acquire().await;
                    }
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
                progress.warn(&format!("Failed to fetch paper ({}): {}", year, err));
            }
            failed_papers += errors.len();

            let inserted = db.insert_papers(&buffer)?;
            tracing::debug!(
                "Checkpoint: inserted {} papers for {}/{}",
                inserted,
                scraper.id(),
                year
            );
        }

        paper_bar.finish_with_message("done");

        let total_count = db.fetched_ids(scraper.id(), *year)?.len();
        // 取れなかった論文がある年は completed にしない（--incremental で次回その分だけ取り直す）
        if failed_papers > 0 {
            progress.warn(&format!(
                "Year {} incomplete: {} of {} papers failed, not marking completed",
                year,
                failed_papers,
                pending.len()
            ));
            incomplete.push(format!(
                "{} ({} of {} papers failed)",
                year,
                failed_papers,
                pending.len()
            ));
            progress.finish_year();
            continue;
        }
        db.mark_completed(scraper.id(), *year, total_count)?;
        progress.log(&format!("Year {} completed: {} papers", year, total_count));
        progress.finish_year();
    }

    progress.finish();
    incomplete_result(scraper.name(), &incomplete)
}

/// 取りこぼした年があれば，その一覧を持つエラーにする（終了コードを 0 にしないため）．
fn incomplete_result(name: &str, incomplete: &[String]) -> Result<()> {
    if incomplete.is_empty() {
        tracing::info!("Sync completed for {}", name);
        return Ok(());
    }
    anyhow::bail!(
        "{}: {} year(s) incomplete: {}",
        name,
        incomplete.len(),
        incomplete.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recheck_only_current_and_previous_year() {
        assert!(recheck_completed_year(2026, 2026));
        assert!(recheck_completed_year(2025, 2026));
        assert!(!recheck_completed_year(2024, 2026));
        assert!(!recheck_completed_year(1990, 2026));
        // 来年の回（年の範囲に入ることは無いが）も取り直す側に倒す
        assert!(recheck_completed_year(2027, 2026));
    }

    #[test]
    fn incomplete_years_make_the_sync_fail() {
        assert!(incomplete_result("ePrint", &[]).is_ok());
        let err = incomplete_result(
            "ePrint",
            &["2008 (paper list: HTTP 429)".into(), "2009 (3 of 500 papers failed)".into()],
        )
        .unwrap_err()
        .to_string();
        assert!(err.starts_with("ePrint: 2 year(s) incomplete"), "{}", err);
        assert!(err.contains("2008 (paper list: HTTP 429)"), "{}", err);
        assert!(err.contains("2009 (3 of 500 papers failed)"), "{}", err);
    }
}
