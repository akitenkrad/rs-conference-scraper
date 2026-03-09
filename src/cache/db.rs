use crate::cache::CacheDb;
use crate::types::Paper;
use anyhow::Result;
use rusqlite::params;
use std::collections::{HashMap, HashSet};

/// 同期状況を表示するための構造体
#[derive(Debug)]
pub struct SyncStatus {
    pub conference: String,
    pub year: u16,
    pub paper_count: usize,
    pub synced_at: Option<String>,
    pub completed: bool,
}

/// 論文統計情報
#[derive(Debug)]
pub struct PaperStats {
    pub total: usize,
    pub with_abstract: usize,
    pub without_abstract: usize,
    pub by_conference: Vec<(String, usize)>,
    pub by_year: Vec<(u16, usize)>,
    pub by_category: Vec<(String, usize)>,
    pub unique_authors: usize,
    pub top_authors: Vec<(String, usize)>,
}

impl CacheDb {
    /// 論文をバッチ挿入（INSERT OR IGNORE で冪等）
    pub fn insert_papers(&mut self, papers: &[Paper]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut inserted = 0;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO papers
                 (id, conference, year, title, authors, abstract, url, pdf_url, categories, hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for p in papers {
                inserted += stmt.execute(params![
                    p.id,
                    p.conference,
                    p.year,
                    p.title,
                    serde_json::to_string(&p.authors)?,
                    p.r#abstract,
                    p.url,
                    p.pdf_url,
                    serde_json::to_string(&p.categories)?,
                    p.hash,
                ])?;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// 取得済み論文IDの集合を返す
    pub fn fetched_ids(&self, conference: &str, year: u16) -> Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM papers WHERE conference=?1 AND year=?2")?;
        let ids = stmt
            .query_map(params![conference, year], |row| row.get(0))?
            .collect::<Result<HashSet<String>, _>>()?;
        Ok(ids)
    }

    /// 同期完了をマーク
    pub fn mark_completed(&self, conference: &str, year: u16, paper_count: usize) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_years (conference, year, paper_count, synced_at, completed)
             VALUES (?1, ?2, ?3, datetime('now'), 1)
             ON CONFLICT(conference, year) DO UPDATE SET
                paper_count=excluded.paper_count,
                synced_at=excluded.synced_at,
                completed=1",
            params![conference, year, paper_count],
        )?;
        Ok(())
    }

    /// 年度の同期完了状態を確認
    pub fn is_year_completed(&self, conference: &str, year: u16) -> Result<bool> {
        let completed: bool = self
            .conn
            .query_row(
                "SELECT COALESCE(completed, 0) FROM sync_years WHERE conference=?1 AND year=?2",
                params![conference, year],
                |row| row.get(0),
            )
            .unwrap_or(false);
        Ok(completed)
    }

    /// 指定会議・年度の論文を取得
    pub fn load_papers(
        &self,
        conference: Option<&str>,
        years: Option<&[u16]>,
    ) -> Result<Vec<Paper>> {
        let mut sql = String::from(
            "SELECT id, conference, year, title, authors, abstract, url, pdf_url, categories, hash FROM papers WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(conf) = conference {
            sql.push_str(&format!(" AND conference=?{}", params_vec.len() + 1));
            params_vec.push(Box::new(conf.to_string()));
        }
        if let Some(yrs) = years {
            if !yrs.is_empty() {
                let placeholders: Vec<String> = yrs
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", params_vec.len() + i + 1))
                    .collect();
                sql.push_str(&format!(" AND year IN ({})", placeholders.join(",")));
                for y in yrs {
                    params_vec.push(Box::new(*y as i64));
                }
            }
        }
        sql.push_str(" ORDER BY year DESC, title ASC");

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let papers = stmt
            .query_map(params_refs.as_slice(), |row| {
                let authors_json: String = row.get(4)?;
                let categories_json: String = row.get(8)?;
                Ok(Paper {
                    id: row.get(0)?,
                    conference: row.get(1)?,
                    year: row.get::<_, i64>(2)? as u16,
                    title: row.get(3)?,
                    authors: serde_json::from_str(&authors_json).unwrap_or_default(),
                    r#abstract: row.get(5)?,
                    url: row.get(6)?,
                    pdf_url: row.get(7)?,
                    categories: serde_json::from_str(&categories_json).unwrap_or_default(),
                    hash: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(papers)
    }

    /// キャッシュ状況を取得
    pub fn status(&self, conference: Option<&str>) -> Result<Vec<SyncStatus>> {
        let mut sql = String::from(
            "SELECT conference, year, paper_count, synced_at, completed FROM sync_years",
        );
        if conference.is_some() {
            sql.push_str(" WHERE conference=?1");
        }
        sql.push_str(" ORDER BY conference, year DESC");

        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<SyncStatus> = if let Some(conf) = conference {
            stmt.query_map(params![conf], |row| {
                Ok(SyncStatus {
                    conference: row.get(0)?,
                    year: row.get::<_, i64>(1)? as u16,
                    paper_count: row.get::<_, i64>(2)? as usize,
                    synced_at: row.get(3)?,
                    completed: row.get::<_, i64>(4)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], |row| {
                Ok(SyncStatus {
                    conference: row.get(0)?,
                    year: row.get::<_, i64>(1)? as u16,
                    paper_count: row.get::<_, i64>(2)? as usize,
                    synced_at: row.get(3)?,
                    completed: row.get::<_, i64>(4)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    /// キャッシュ削除
    pub fn clear(&mut self, conference: Option<&str>, year: Option<u16>) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let deleted;
        match (conference, year) {
            (Some(conf), Some(y)) => {
                deleted = tx.execute(
                    "DELETE FROM papers WHERE conference=?1 AND year=?2",
                    params![conf, y],
                )?;
                tx.execute(
                    "DELETE FROM sync_years WHERE conference=?1 AND year=?2",
                    params![conf, y],
                )?;
            }
            (Some(conf), None) => {
                deleted = tx.execute(
                    "DELETE FROM papers WHERE conference=?1",
                    params![conf],
                )?;
                tx.execute(
                    "DELETE FROM sync_years WHERE conference=?1",
                    params![conf],
                )?;
            }
            (None, Some(y)) => {
                deleted = tx.execute("DELETE FROM papers WHERE year=?1", params![y])?;
                tx.execute("DELETE FROM sync_years WHERE year=?1", params![y])?;
            }
            (None, None) => {
                deleted = tx.execute("DELETE FROM papers", [])?;
                tx.execute("DELETE FROM sync_years", [])?;
            }
        }
        tx.commit()?;
        Ok(deleted)
    }

    /// 統計情報を取得
    pub fn stats(
        &self,
        conference: Option<&str>,
        years: Option<&[u16]>,
    ) -> Result<PaperStats> {
        // WHERE句の構築
        let (where_clause, params_vec) = self.build_where_clause(conference, years);

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        // 総論文数
        let total: usize = {
            let sql = format!("SELECT COUNT(*) FROM papers{}", where_clause);
            self.conn.query_row(&sql, params_refs.as_slice(), |row| {
                row.get::<_, i64>(0)
            })? as usize
        };

        // 会議別集計
        let by_conference: Vec<(String, usize)> = {
            let sql = format!(
                "SELECT conference, COUNT(*) FROM papers{} GROUP BY conference ORDER BY COUNT(*) DESC",
                where_clause
            );
            let mut stmt = self.conn.prepare(&sql)?;
            stmt.query_map(params_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        // 年度別集計
        let by_year: Vec<(u16, usize)> = {
            let sql = format!(
                "SELECT year, COUNT(*) FROM papers{} GROUP BY year ORDER BY year DESC",
                where_clause
            );
            let mut stmt = self.conn.prepare(&sql)?;
            stmt.query_map(params_refs.as_slice(), |row| {
                Ok((row.get::<_, i64>(0)? as u16, row.get::<_, i64>(1)? as usize))
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        // カテゴリ別集計（categoriesはJSON配列で格納されている）
        let by_category: Vec<(String, usize)> = {
            let sql = format!(
                "SELECT categories FROM papers{}",
                where_clause
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut cat_counts: HashMap<String, usize> = HashMap::new();
            let rows = stmt.query_map(params_refs.as_slice(), |row| {
                row.get::<_, String>(0)
            })?;
            for row in rows {
                let json = row?;
                if let Ok(cats) = serde_json::from_str::<Vec<String>>(&json) {
                    for cat in cats {
                        *cat_counts.entry(cat).or_insert(0) += 1;
                    }
                }
            }
            let mut sorted: Vec<_> = cat_counts.into_iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1));
            sorted
        };

        // abstract有無の集計
        let with_abstract: usize = {
            let sql = format!(
                "SELECT COUNT(*) FROM papers{} AND abstract != ''",
                if where_clause.is_empty() {
                    " WHERE 1=1".to_string()
                } else {
                    where_clause.clone()
                }
            );
            self.conn.query_row(&sql, params_refs.as_slice(), |row| {
                row.get::<_, i64>(0)
            })? as usize
        };

        // ユニーク著者数とトップ著者
        let (unique_authors, top_authors) = {
            let sql = format!(
                "SELECT authors FROM papers{}",
                where_clause
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut author_counts: HashMap<String, usize> = HashMap::new();
            let rows = stmt.query_map(params_refs.as_slice(), |row| {
                row.get::<_, String>(0)
            })?;
            for row in rows {
                let json = row?;
                if let Ok(authors) = serde_json::from_str::<Vec<String>>(&json) {
                    for author in authors {
                        *author_counts.entry(author).or_insert(0) += 1;
                    }
                }
            }
            let unique = author_counts.len();
            let mut sorted: Vec<_> = author_counts.into_iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1));
            sorted.truncate(10);
            (unique, sorted)
        };

        Ok(PaperStats {
            total,
            with_abstract,
            without_abstract: total - with_abstract,
            by_conference,
            by_year,
            by_category,
            unique_authors,
            top_authors,
        })
    }

    /// WHERE句を構築するヘルパー
    fn build_where_clause(
        &self,
        conference: Option<&str>,
        years: Option<&[u16]>,
    ) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
        let mut clauses = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(conf) = conference {
            params_vec.push(Box::new(conf.to_string()));
            clauses.push(format!("conference=?{}", params_vec.len()));
        }
        if let Some(yrs) = years {
            if !yrs.is_empty() {
                let placeholders: Vec<String> = yrs
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", params_vec.len() + i + 1))
                    .collect();
                clauses.push(format!("year IN ({})", placeholders.join(",")));
                for y in yrs {
                    params_vec.push(Box::new(*y as i64));
                }
            }
        }

        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };

        (where_clause, params_vec)
    }

    /// 年度の論文を強制削除（--force用）
    pub fn clear_year(&mut self, conference: &str, year: u16) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM papers WHERE conference=?1 AND year=?2",
            params![conference, year],
        )?;
        tx.execute(
            "DELETE FROM sync_years WHERE conference=?1 AND year=?2",
            params![conference, year],
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheDb;
    use tempfile::tempdir;

    fn make_test_paper(id: &str, conference: &str, year: u16) -> Paper {
        Paper {
            id: id.to_string(),
            conference: conference.to_string(),
            year,
            title: format!("Test Paper {}", id),
            authors: vec!["Author A".to_string(), "Author B".to_string()],
            r#abstract: "Test abstract content".to_string(),
            url: format!("https://example.com/{}", id),
            pdf_url: Some(format!("https://example.com/{}.pdf", id)),
            categories: vec!["Conference".to_string()],
            hash: format!("hash_{}", id),
        }
    }

    fn open_test_db() -> CacheDb {
        let dir = tempdir().unwrap();
        CacheDb::open(dir.path()).unwrap()
    }

    #[test]
    fn insert_papers_returns_count() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "neurips", 2024),
        ];
        let count = db.insert_papers(&papers).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn insert_papers_is_idempotent() {
        let mut db = open_test_db();
        let papers = vec![make_test_paper("p1", "neurips", 2024)];
        db.insert_papers(&papers).unwrap();
        // Second insert of the same paper should be ignored
        let count = db.insert_papers(&papers).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn fetched_ids_returns_correct_set() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "neurips", 2024),
            make_test_paper("p3", "icml", 2024),
        ];
        db.insert_papers(&papers).unwrap();
        let ids = db.fetched_ids("neurips", 2024).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains("p1"));
        assert!(ids.contains("p2"));
        assert!(!ids.contains("p3"));
    }

    #[test]
    fn mark_completed_and_is_year_completed() {
        let db = open_test_db();
        assert!(!db.is_year_completed("neurips", 2024).unwrap());
        db.mark_completed("neurips", 2024, 100).unwrap();
        assert!(db.is_year_completed("neurips", 2024).unwrap());
    }

    #[test]
    fn load_papers_with_conference_filter() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "icml", 2024),
        ];
        db.insert_papers(&papers).unwrap();
        let loaded = db.load_papers(Some("neurips"), None).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].conference, "neurips");
    }

    #[test]
    fn load_papers_with_year_filter() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "neurips", 2023),
        ];
        db.insert_papers(&papers).unwrap();
        let years = vec![2024u16];
        let loaded = db.load_papers(None, Some(&years)).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].year, 2024);
    }

    #[test]
    fn load_papers_with_both_filters() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "neurips", 2023),
            make_test_paper("p3", "icml", 2024),
        ];
        db.insert_papers(&papers).unwrap();
        let years = vec![2024u16];
        let loaded = db.load_papers(Some("neurips"), Some(&years)).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "p1");
    }

    #[test]
    fn load_papers_no_filters_returns_all() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "icml", 2023),
        ];
        db.insert_papers(&papers).unwrap();
        let loaded = db.load_papers(None, None).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn status_returns_correct_sync_info() {
        let db = open_test_db();
        db.mark_completed("neurips", 2024, 50).unwrap();
        db.mark_completed("icml", 2023, 30).unwrap();

        let all_status = db.status(None).unwrap();
        assert_eq!(all_status.len(), 2);

        let neurips_status = db.status(Some("neurips")).unwrap();
        assert_eq!(neurips_status.len(), 1);
        assert_eq!(neurips_status[0].conference, "neurips");
        assert_eq!(neurips_status[0].paper_count, 50);
        assert!(neurips_status[0].completed);
    }

    #[test]
    fn clear_with_conference_filter() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "icml", 2024),
        ];
        db.insert_papers(&papers).unwrap();
        let deleted = db.clear(Some("neurips"), None).unwrap();
        assert_eq!(deleted, 1);
        let remaining = db.load_papers(None, None).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].conference, "icml");
    }

    #[test]
    fn clear_with_year_filter() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "neurips", 2023),
        ];
        db.insert_papers(&papers).unwrap();
        let deleted = db.clear(None, Some(2023)).unwrap();
        assert_eq!(deleted, 1);
        let remaining = db.load_papers(None, None).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].year, 2024);
    }

    #[test]
    fn clear_all() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "icml", 2023),
        ];
        db.insert_papers(&papers).unwrap();
        let deleted = db.clear(None, None).unwrap();
        assert_eq!(deleted, 2);
        let remaining = db.load_papers(None, None).unwrap();
        assert!(remaining.is_empty());
    }

    #[test]
    fn stats_returns_correct_totals() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "neurips", 2023),
            make_test_paper("p3", "icml", 2024),
        ];
        db.insert_papers(&papers).unwrap();

        let stats = db.stats(None, None).unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.with_abstract, 3); // all have "Test abstract content"
        assert_eq!(stats.without_abstract, 0);
        assert_eq!(stats.by_conference.len(), 2);
        assert_eq!(stats.by_year.len(), 2);
    }

    #[test]
    fn stats_with_conference_filter() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "icml", 2024),
        ];
        db.insert_papers(&papers).unwrap();

        let stats = db.stats(Some("neurips"), None).unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.by_conference.len(), 1);
        assert_eq!(stats.by_conference[0].0, "neurips");
    }

    #[test]
    fn stats_with_year_filter() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "neurips", 2023),
        ];
        db.insert_papers(&papers).unwrap();

        let years = vec![2024u16];
        let stats = db.stats(None, Some(&years)).unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.by_year.len(), 1);
        assert_eq!(stats.by_year[0].0, 2024);
    }

    #[test]
    fn stats_counts_unique_authors_and_categories() {
        let mut db = open_test_db();
        // make_test_paper gives each paper authors=["Author A", "Author B"] and categories=["Conference"]
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "neurips", 2024),
        ];
        db.insert_papers(&papers).unwrap();

        let stats = db.stats(None, None).unwrap();
        assert_eq!(stats.unique_authors, 2); // "Author A" and "Author B"
        assert_eq!(stats.top_authors.len(), 2);
        assert_eq!(stats.top_authors[0].1, 2); // each author appears in 2 papers
        assert_eq!(stats.by_category.len(), 1); // "Conference"
        assert_eq!(stats.by_category[0], ("Conference".to_string(), 2));
    }

    #[test]
    fn stats_empty_db_returns_zeros() {
        let db = open_test_db();
        let stats = db.stats(None, None).unwrap();
        assert_eq!(stats.total, 0);
        assert_eq!(stats.unique_authors, 0);
        assert!(stats.by_conference.is_empty());
        assert!(stats.by_year.is_empty());
    }

    #[test]
    fn clear_year_removes_papers_and_sync() {
        let mut db = open_test_db();
        let papers = vec![
            make_test_paper("p1", "neurips", 2024),
            make_test_paper("p2", "neurips", 2023),
        ];
        db.insert_papers(&papers).unwrap();
        db.mark_completed("neurips", 2024, 1).unwrap();

        db.clear_year("neurips", 2024).unwrap();

        let remaining = db.load_papers(Some("neurips"), None).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].year, 2023);
        assert!(!db.is_year_completed("neurips", 2024).unwrap());
    }
}
