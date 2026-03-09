use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub mod db;

pub struct CacheDb {
    conn: Connection,
}

impl CacheDb {
    pub fn open(cache_dir: &Path) -> Result<Self> {
        let db_path = cache_dir.join("cache.db");
        std::fs::create_dir_all(cache_dir)?;
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    fn migrate(conn: &Connection) -> Result<()> {
        conn.execute_batch(include_str!("schema.sql"))?;
        Ok(())
    }
}
