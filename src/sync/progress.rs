use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// sync全体の進捗管理
pub struct SyncProgress {
    multi: MultiProgress,
    year_bar: Option<ProgressBar>,
}

impl SyncProgress {
    pub fn new() -> Self {
        Self {
            multi: MultiProgress::new(),
            year_bar: None,
        }
    }

    /// 年度レベルの進捗バーを開始
    pub fn start_years(&mut self, total: u64) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new(total));
        pb.set_style(
            ProgressStyle::with_template(
                "{prefix:.bold} [{bar:30.cyan/blue}] {pos}/{len} years ({eta})",
            )
            .unwrap()
            .progress_chars("█▓░"),
        );
        pb.set_prefix("Syncing");
        self.year_bar = Some(pb.clone());
        pb
    }

    /// 論文取得の進捗バーを作成（年度ごと）
    pub fn start_papers(&self, year: u16, total: u64, skipped: u64) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new(total));
        pb.set_style(
            ProgressStyle::with_template(
                "  {prefix:.dim} [{bar:30.green/white}] {pos}/{len} papers ({per_sec}, {eta}) {msg}",
            )
            .unwrap()
            .progress_chars("█▓░"),
        );
        pb.set_prefix(format!("{}", year));
        if skipped > 0 {
            pb.set_message(format!("({} skipped)", skipped));
        }
        pb
    }

    /// 年度をスキップしたことを表示
    pub fn skip_year(&self, year: u16, reason: &str) {
        if let Some(ref bar) = self.year_bar {
            bar.println(format!("  {} skipped ({})", year, reason));
            bar.inc(1);
        }
    }

    /// 年度の進捗を完了
    pub fn finish_year(&self) {
        if let Some(ref bar) = self.year_bar {
            bar.inc(1);
        }
    }

    /// 全体完了
    pub fn finish(&self) {
        if let Some(ref bar) = self.year_bar {
            bar.finish_with_message("done");
        }
    }
}
