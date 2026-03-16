use std::sync::Arc;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// sync全体の進捗管理
pub struct SyncProgress {
    multi: Arc<MultiProgress>,
    year_bar: Option<ProgressBar>,
    prefix: String,
}

impl SyncProgress {
    /// 単一会議用（後方互換）
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            multi: Arc::new(MultiProgress::new()),
            year_bar: None,
            prefix: String::new(),
        }
    }

    /// 共有MultiProgress上に会議ごとの進捗を作成
    pub fn with_shared(multi: Arc<MultiProgress>, prefix: &str) -> Self {
        Self {
            multi,
            year_bar: None,
            prefix: prefix.to_string(),
        }
    }

    /// 年度レベルの進捗バーを開始
    pub fn start_years(&mut self, total: u64) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new(total));
        let template = if self.prefix.is_empty() {
            "{prefix:.bold} [{bar:30.cyan/blue}] {pos}/{len} years ({eta})".to_string()
        } else {
            "{prefix:.bold} [{bar:30.cyan/blue}] {pos}/{len} years ({eta})".to_string()
        };
        pb.set_style(
            ProgressStyle::with_template(&template)
                .unwrap()
                .progress_chars("###"),
        );
        let label = if self.prefix.is_empty() {
            "Syncing".to_string()
        } else {
            format!("[{}] Syncing", self.prefix)
        };
        pb.set_prefix(label);
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
            .progress_chars("###"),
        );
        let label = if self.prefix.is_empty() {
            format!("{}", year)
        } else {
            format!("[{}] {}", self.prefix, year)
        };
        pb.set_prefix(label);
        if skipped > 0 {
            pb.set_message(format!("({} skipped)", skipped));
        }
        pb
    }

    /// 年度をスキップしたことを表示
    pub fn skip_year(&self, year: u16, reason: &str) {
        if let Some(ref bar) = self.year_bar {
            let msg = if self.prefix.is_empty() {
                format!("  {} skipped ({})", year, reason)
            } else {
                format!("  [{}] {} skipped ({})", self.prefix, year, reason)
            };
            bar.println(msg);
            bar.inc(1);
        }
    }

    /// 年度の進捗を完了
    pub fn finish_year(&self) {
        if let Some(ref bar) = self.year_bar {
            bar.inc(1);
        }
    }

    /// プログレスバーと競合しないようにログ出力
    pub fn log(&self, msg: &str) {
        if let Some(ref bar) = self.year_bar {
            if self.prefix.is_empty() {
                bar.println(msg);
            } else {
                bar.println(format!("[{}] {}", self.prefix, msg));
            }
        } else {
            eprintln!("{}", msg);
        }
    }

    /// 全体完了
    pub fn finish(&self) {
        if let Some(ref bar) = self.year_bar {
            bar.finish_and_clear();
        }
    }
}
