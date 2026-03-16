use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "conf-scraper", about = "Conference paper scraping and filtering tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// キャッシュディレクトリ
    #[arg(long, default_value = "~/.cache/conf-scraper")]
    pub cache_dir: String,

    /// 詳細ログ出力
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 論文メタ情報を取得してキャッシュ
    Sync(SyncArgs),
    /// キャッシュから論文をフィルタリング
    Filter(FilterArgs),
    /// キャッシュの管理
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },
    /// 収集済み論文の統計情報を表示
    Stats(StatsArgs),
    /// Semantic Scholarでメタデータを補完
    Enrich(EnrichArgs),
    /// 対応会議の一覧表示
    ListConferences,
}

#[derive(Args)]
pub struct SyncArgs {
    /// 会議ID (カンマ区切りで複数指定可)
    #[arg(long, value_delimiter = ',')]
    pub conference: Vec<String>,

    /// 対象年度 (例: 2020-2024 または 2024)
    #[arg(long)]
    pub year: Option<String>,

    /// 差分のみ更新
    #[arg(long)]
    pub incremental: bool,

    /// 強制再取得
    #[arg(long)]
    pub force: bool,

    /// 並列アブスト取得数
    #[arg(long, default_value = "4")]
    pub jobs: usize,

    /// スクレイピング間隔秒数
    #[arg(long, default_value = "1.5")]
    pub interval: f64,

    /// リトライ回数
    #[arg(long, default_value = "3")]
    pub retry: usize,

    /// 中間保存の間隔 (N件ごと)
    #[arg(long, default_value = "100")]
    pub checkpoint: usize,
}

#[derive(Args)]
#[command(after_long_help = r#"Examples:
  # キーワードでフィルタリング
  conf-scraper filter --filter keyword --keywords transformer,attention

  # 特定の会議・年度に絞ってキーワード検索
  conf-scraper filter --conference neurips --year 2024 --filter keyword --keywords "large language model"

  # カテゴリタグでフィルタリング (Oral発表のみ)
  conf-scraper filter --conference iclr --year 2024 --filter category --tags oral

  # キーワードとカテゴリの組み合わせ (AND結合)
  conf-scraper filter --filter keyword,category --keywords transformer --tags oral

  # キーワードとカテゴリの組み合わせ (OR結合)
  conf-scraper filter --filter keyword,category --keywords transformer --tags oral --combine or

  # タイトルのみを検索対象にする
  conf-scraper filter --filter keyword --keywords diffusion --fields title

  # 出力件数を制限してJSONファイルに保存
  conf-scraper filter --filter keyword --keywords reinforcement --limit 20 --output results.json

  # 複数年度を指定
  conf-scraper filter --conference cvpr --year 2020-2024 --filter keyword --keywords "object detection""#)]
pub struct FilterArgs {
    /// 会議ID (省略時: 全会議)
    #[arg(long)]
    pub conference: Option<String>,

    /// 対象年度
    #[arg(long)]
    pub year: Option<String>,

    /// フィルタ種別 (複数指定可)
    #[arg(long, value_delimiter = ',')]
    pub filter: Vec<String>,

    /// キーワードリスト (カンマ区切り)
    #[arg(long, value_delimiter = ',')]
    pub keywords: Vec<String>,

    /// 検索対象フィールド [title|abstract]
    #[arg(long, value_delimiter = ',', default_value = "title,abstract")]
    pub fields: Vec<String>,

    /// カテゴリタグ (カンマ区切り)
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,

    /// フィルタ結合方式 [and|or]
    #[arg(long, default_value = "and")]
    pub combine: String,

    /// 出力する論文の最大数 (0で無制限)
    #[arg(long, default_value = "20")]
    pub limit: usize,

    /// 出力先JSONファイル
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// 出力形式 [json|csv|xml] (デフォルト: json)
    #[arg(long, default_value = "json")]
    pub format: String,

    /// キャッシュ未存在時にエラー
    #[arg(long)]
    pub offline: bool,
}

#[derive(Args)]
pub struct StatsArgs {
    /// 会議ID (省略時: 全会議)
    #[arg(long)]
    pub conference: Option<String>,

    /// 対象年度
    #[arg(long)]
    pub year: Option<String>,

    /// 表示するセクション [summary,conferences,years,categories,authors,abstracts] (カンマ区切り，省略時: 全表示)
    #[arg(long, value_delimiter = ',')]
    pub show: Vec<String>,
}

#[derive(Args)]
pub struct EnrichArgs {
    /// 会議ID (カンマ区切りで複数指定可)
    #[arg(long, value_delimiter = ',')]
    pub conference: Vec<String>,

    /// 対象年度 (例: 2020-2024 または 2024)
    #[arg(long)]
    pub year: Option<String>,

    /// abstract取得済みの論文も再取得
    #[arg(long)]
    pub force: bool,

    /// リトライ回数
    #[arg(long, default_value = "3")]
    pub retry: u64,

    /// リトライ時の待機秒数
    #[arg(long, default_value = "10")]
    pub wait: u64,

    /// リクエスト間隔秒数
    #[arg(long, default_value = "1.0")]
    pub interval: f64,

    /// 中間保存の間隔 (N件ごと)
    #[arg(long, default_value = "50")]
    pub checkpoint: usize,
}

#[derive(Subcommand)]
pub enum CacheCommands {
    /// キャッシュの状態確認
    Status {
        #[arg(long)]
        conference: Option<String>,
    },
    /// キャッシュの削除
    Clear {
        #[arg(long)]
        conference: Option<String>,
        #[arg(long)]
        year: Option<u16>,
    },
}

/// --year の範囲パース ("2020-2024" or "2024")
pub fn parse_year_range(year_str: &str) -> anyhow::Result<Vec<u16>> {
    if let Some((start, end)) = year_str.split_once('-') {
        let s: u16 = start.parse()?;
        let e: u16 = end.parse()?;
        if s > e {
            anyhow::bail!("Invalid year range: start ({}) > end ({})", s, e);
        }
        Ok((s..=e).collect())
    } else {
        let y: u16 = year_str.parse()?;
        Ok(vec![y])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_year() {
        let result = parse_year_range("2024").unwrap();
        assert_eq!(result, vec![2024]);
    }

    #[test]
    fn year_range() {
        let result = parse_year_range("2020-2024").unwrap();
        assert_eq!(result, vec![2020, 2021, 2022, 2023, 2024]);
    }

    #[test]
    fn invalid_range_start_greater_than_end() {
        let result = parse_year_range("2024-2020");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid year range"));
    }

    #[test]
    fn invalid_input_non_numeric() {
        let result = parse_year_range("abc");
        assert!(result.is_err());
    }
}
