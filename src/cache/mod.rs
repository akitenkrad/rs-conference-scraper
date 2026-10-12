use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub mod db;
pub mod query;

pub struct CacheDb {
    conn: Connection,
}

impl CacheDb {
    pub fn open(cache_dir: &Path) -> Result<Self> {
        let db_path = cache_dir.join("cache.db");
        std::fs::create_dir_all(cache_dir)?;
        let conn = Connection::open(&db_path)?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        Self::init_with_retry(&conn)?;
        Ok(Self { conn })
    }

    /// WAL への切り替えとスキーマ作成．新しい DB を複数の接続が同時に開くと，ここが
    /// SQLITE_BUSY になる（MYTASK-3421）．ロックの格上げでぶつかった場合，SQLite はデッドロックを
    /// 避けるため busy_timeout を待たずに BUSY を返すので，間をあけて自前で取り直す．
    fn init_with_retry(conn: &Connection) -> Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut wait = std::time::Duration::from_millis(10);
        loop {
            match Self::init(conn) {
                Ok(()) => return Ok(()),
                Err(e) if is_busy(&e) && std::time::Instant::now() < deadline => {
                    std::thread::sleep(wait);
                    wait = (wait * 2).min(std::time::Duration::from_millis(500));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn init(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(include_str!("schema.sql"))
    }
}

fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if matches!(f.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 新しい空の DB を複数の接続が同時に開いても，どれも失敗しない（multi-venue の sync がこの形）
    #[test]
    fn concurrent_open_of_a_fresh_db_succeeds() {
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let path = dir.path().to_path_buf();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        CacheDb::open(&path).map(|_| ())
                    })
                })
                .collect();
            for h in handles {
                if let Err(e) = h.join().unwrap() {
                    panic!("concurrent open failed: {:#}", e);
                }
            }
        }
    }
}
