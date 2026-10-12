//! ダッシュボード向けの読み出し（`conf-scraper query`）．
//!
//! 出力はすべて JSON で，Obsidian のダッシュボードが `execFile` で呼んで標準出力を読む．
//! 1 回の出力を小さく保つため，一覧は必ずページ単位で返し，アブストラクトは 1 件取得
//! (`paper`) のときだけ含める．

use anyhow::Result;
use rusqlite::types::ToSql;
use serde::Serialize;
use std::collections::BTreeMap;

use super::CacheDb;
use crate::types::Paper;

/// 出力 JSON の形を変えたら上げる
pub const SCHEMA_VERSION: u32 = 1;

/// 1 ページの上限件数（ダッシュボード側の出力バッファを溢れさせないため）
pub const MAX_LIMIT: usize = 500;

#[derive(Debug, Serialize)]
pub struct Catalog {
    pub schema: u32,
    pub total: usize,
    pub with_abstract: usize,
    pub venues: Vec<CatalogVenue>,
}

#[derive(Debug, Serialize)]
pub struct CatalogVenue {
    pub id: String,
    pub name: String,
    pub field: String,
    pub total: usize,
    pub with_abstract: usize,
    /// 新しい年から順
    pub years: Vec<CatalogYear>,
}

#[derive(Debug, Serialize)]
pub struct CatalogYear {
    pub year: u16,
    pub count: usize,
    pub with_abstract: usize,
    /// sync_years に記録があるか（無ければ同期したことのない年）
    pub synced: bool,
    pub completed: bool,
    pub synced_at: Option<String>,
}

/// 検索対象のフィールド
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    Title,
    Authors,
    Abstract,
}

impl SearchField {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim() {
            "title" => Ok(Self::Title),
            "authors" => Ok(Self::Authors),
            "abstract" => Ok(Self::Abstract),
            other => anyhow::bail!("Unknown field '{}': use title, authors or abstract", other),
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Authors => "authors",
            Self::Abstract => "abstract",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    YearDesc,
    YearAsc,
    Title,
}

impl SortOrder {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "year-desc" => Ok(Self::YearDesc),
            "year-asc" => Ok(Self::YearAsc),
            "title" => Ok(Self::Title),
            other => anyhow::bail!("Unknown sort '{}': use year-desc, year-asc or title", other),
        }
    }

    fn sql(self) -> &'static str {
        // 同順位は会議 → タイトルで決める（ページ送りで行が揺れないように）
        match self {
            Self::YearDesc => "year DESC, conference ASC, title ASC",
            Self::YearAsc => "year ASC, conference ASC, title ASC",
            Self::Title => "title ASC, year DESC, conference ASC",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaperQuery {
    pub conferences: Vec<String>,
    pub years: Vec<u16>,
    /// 空白区切りの語をすべて含む（AND）．各語は fields のどれかに含まれればよい
    pub q: String,
    pub fields: Vec<SearchField>,
    pub offset: usize,
    pub limit: usize,
    pub sort: SortOrder,
    pub facets: bool,
}

#[derive(Debug, Serialize)]
pub struct PaperPage {
    pub schema: u32,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub papers: Vec<PaperRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facets: Option<Facets>,
}

/// 一覧の 1 行（アブストラクトは含めない）
#[derive(Debug, Serialize)]
pub struct PaperRow {
    pub id: String,
    pub conference: String,
    pub year: u16,
    pub title: String,
    pub authors: Vec<String>,
    pub url: String,
    pub pdf_url: Option<String>,
    pub categories: Vec<String>,
    pub has_abstract: bool,
}

/// 各軸の件数．その軸の絞り込みだけを外して数える（選択肢を選ぶと他の選択肢が消える，を防ぐ）
#[derive(Debug, Serialize)]
pub struct Facets {
    pub conference: Vec<FacetCount<String>>,
    pub year: Vec<FacetCount<u16>>,
}

#[derive(Debug, Serialize)]
pub struct FacetCount<T> {
    pub value: T,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct PaperDetail {
    pub schema: u32,
    pub paper: Option<Paper>,
}

/// WHERE 句とその引数
struct Where {
    sql: String,
    params: Vec<Box<dyn ToSql>>,
}

impl Where {
    fn new() -> Self {
        Self {
            sql: String::from(" WHERE 1=1"),
            params: Vec::new(),
        }
    }

    fn next(&self) -> usize {
        self.params.len() + 1
    }

    fn refs(&self) -> Vec<&dyn ToSql> {
        self.params.iter().map(|p| p.as_ref()).collect()
    }
}

/// LIKE の特殊文字をエスケープする（`ESCAPE '\'` と組で使う）
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl PaperQuery {
    fn build_where(&self, use_conference: bool, use_year: bool) -> Where {
        let mut w = Where::new();

        if use_conference && !self.conferences.is_empty() {
            let ph: Vec<String> = (0..self.conferences.len())
                .map(|i| format!("?{}", w.next() + i))
                .collect();
            w.sql
                .push_str(&format!(" AND conference IN ({})", ph.join(",")));
            for c in &self.conferences {
                w.params.push(Box::new(c.clone()));
            }
        }

        if use_year && !self.years.is_empty() {
            let ph: Vec<String> = (0..self.years.len())
                .map(|i| format!("?{}", w.next() + i))
                .collect();
            w.sql.push_str(&format!(" AND year IN ({})", ph.join(",")));
            for y in &self.years {
                w.params.push(Box::new(*y as i64));
            }
        }

        let fields: &[SearchField] = if self.fields.is_empty() {
            &[SearchField::Title]
        } else {
            &self.fields
        };
        for term in self.q.split_whitespace() {
            let n = w.next();
            let ors: Vec<String> = fields
                .iter()
                .map(|f| format!("{} LIKE ?{} ESCAPE '\\'", f.column(), n))
                .collect();
            w.sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
            w.params.push(Box::new(format!("%{}%", escape_like(term))));
        }

        w
    }
}

impl CacheDb {
    /// 学会 × 年の目次
    pub fn query_catalog(&self) -> Result<Catalog> {
        // 論文テーブルからの実数
        let mut counts: BTreeMap<(String, u16), (usize, usize)> = BTreeMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT conference, year, COUNT(*), SUM(CASE WHEN abstract != '' THEN 1 ELSE 0 END)
                 FROM papers GROUP BY conference, year",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u16,
                    r.get::<_, i64>(2)? as usize,
                    r.get::<_, i64>(3)? as usize,
                ))
            })?;
            for row in rows {
                let (c, y, n, a) = row?;
                counts.insert((c, y), (n, a));
            }
        }

        // 同期の記録（0 件の年もここにだけ現れる）
        let mut sync: BTreeMap<(String, u16), (bool, Option<String>)> = BTreeMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT conference, year, completed, synced_at FROM sync_years")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u16,
                    r.get::<_, i64>(2)? != 0,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?;
            for row in rows {
                let (c, y, done, at) = row?;
                sync.insert((c, y), (done, at));
            }
        }

        let mut venues = Vec::new();
        let mut total = 0;
        let mut with_abstract = 0;
        for (id, name, field) in crate::conference::list_conferences() {
            let mut years: BTreeMap<u16, CatalogYear> = BTreeMap::new();
            for ((c, y), (n, a)) in counts.range((id.to_string(), 0)..=(id.to_string(), u16::MAX)) {
                debug_assert_eq!(c, id);
                years.insert(
                    *y,
                    CatalogYear {
                        year: *y,
                        count: *n,
                        with_abstract: *a,
                        synced: false,
                        completed: false,
                        synced_at: None,
                    },
                );
            }
            for ((_, y), (done, at)) in sync.range((id.to_string(), 0)..=(id.to_string(), u16::MAX))
            {
                let e = years.entry(*y).or_insert(CatalogYear {
                    year: *y,
                    count: 0,
                    with_abstract: 0,
                    synced: false,
                    completed: false,
                    synced_at: None,
                });
                e.synced = true;
                e.completed = *done;
                e.synced_at = at.clone();
            }

            let v_total: usize = years.values().map(|y| y.count).sum();
            let v_abs: usize = years.values().map(|y| y.with_abstract).sum();
            total += v_total;
            with_abstract += v_abs;
            venues.push(CatalogVenue {
                id: id.to_string(),
                name: name.to_string(),
                field: field.to_string(),
                total: v_total,
                with_abstract: v_abs,
                years: years.into_values().rev().collect(),
            });
        }

        Ok(Catalog {
            schema: SCHEMA_VERSION,
            total,
            with_abstract,
            venues,
        })
    }

    /// 1 ページ分の一覧
    pub fn query_papers(&self, q: &PaperQuery) -> Result<PaperPage> {
        let limit = q.limit.clamp(1, MAX_LIMIT);
        let w = q.build_where(true, true);

        let total: usize = self.conn.query_row(
            &format!("SELECT COUNT(*) FROM papers{}", w.sql),
            w.refs().as_slice(),
            |r| r.get::<_, i64>(0),
        )? as usize;

        let n = w.next();
        let sql = format!(
            "SELECT id, conference, year, title, authors, url, pdf_url, categories, abstract != ''
             FROM papers{} ORDER BY {} LIMIT ?{} OFFSET ?{}",
            w.sql,
            q.sort.sql(),
            n,
            n + 1
        );
        let mut params = w.refs();
        let limit_i = limit as i64;
        let offset_i = q.offset as i64;
        params.push(&limit_i);
        params.push(&offset_i);

        let mut stmt = self.conn.prepare(&sql)?;
        let papers = stmt
            .query_map(params.as_slice(), |r| {
                let authors: String = r.get(4)?;
                let categories: String = r.get(7)?;
                Ok(PaperRow {
                    id: r.get(0)?,
                    conference: r.get(1)?,
                    year: r.get::<_, i64>(2)? as u16,
                    title: r.get(3)?,
                    authors: serde_json::from_str(&authors).unwrap_or_default(),
                    url: r.get(5)?,
                    pdf_url: r.get(6)?,
                    categories: serde_json::from_str(&categories).unwrap_or_default(),
                    has_abstract: r.get::<_, i64>(8)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let facets = if q.facets {
            Some(self.query_facets(q)?)
        } else {
            None
        };

        Ok(PaperPage {
            schema: SCHEMA_VERSION,
            total,
            offset: q.offset,
            limit,
            papers,
            facets,
        })
    }

    fn query_facets(&self, q: &PaperQuery) -> Result<Facets> {
        let w = q.build_where(false, true);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT conference, COUNT(*) FROM papers{} GROUP BY conference ORDER BY 2 DESC, 1 ASC",
            w.sql
        ))?;
        let conference = stmt
            .query_map(w.refs().as_slice(), |r| {
                Ok(FacetCount {
                    value: r.get::<_, String>(0)?,
                    count: r.get::<_, i64>(1)? as usize,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let w = q.build_where(true, false);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT year, COUNT(*) FROM papers{} GROUP BY year ORDER BY 1 DESC",
            w.sql
        ))?;
        let year = stmt
            .query_map(w.refs().as_slice(), |r| {
                Ok(FacetCount {
                    value: r.get::<_, i64>(0)? as u16,
                    count: r.get::<_, i64>(1)? as usize,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Facets { conference, year })
    }

    /// 1 件（アブストラクト込み）．無ければ `paper: null`
    pub fn query_paper(&self, id: &str, conference: &str, year: u16) -> Result<PaperDetail> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conference, year, title, authors, abstract, url, pdf_url, categories, hash
             FROM papers WHERE id = ?1 AND conference = ?2 AND year = ?3",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id, conference, year as i64], |r| {
            let authors: String = r.get(4)?;
            let categories: String = r.get(8)?;
            Ok(Paper {
                id: r.get(0)?,
                conference: r.get(1)?,
                year: r.get::<_, i64>(2)? as u16,
                title: r.get(3)?,
                authors: serde_json::from_str(&authors).unwrap_or_default(),
                r#abstract: r.get(5)?,
                url: r.get(6)?,
                pdf_url: r.get(7)?,
                categories: serde_json::from_str(&categories).unwrap_or_default(),
                hash: r.get(9)?,
            })
        })?;
        let paper = rows.next().transpose()?;
        Ok(PaperDetail {
            schema: SCHEMA_VERSION,
            paper,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn paper(id: &str, conf: &str, year: u16, title: &str, authors: &[&str], abs: &str) -> Paper {
        Paper {
            id: id.to_string(),
            conference: conf.to_string(),
            year,
            title: title.to_string(),
            authors: authors.iter().map(|s| s.to_string()).collect(),
            r#abstract: abs.to_string(),
            url: format!("https://example.com/{}", id),
            pdf_url: None,
            categories: vec!["Main".to_string()],
            hash: format!("h{}", id),
        }
    }

    fn seeded() -> (tempfile::TempDir, CacheDb) {
        let dir = tempdir().unwrap();
        let mut db = CacheDb::open(dir.path()).unwrap();
        db.insert_papers(&[
            paper(
                "a",
                "acl",
                2025,
                "Agents that Plan",
                &["Alice Smith"],
                "We study LLM agents.",
            ),
            paper(
                "b",
                "acl",
                2024,
                "Parsing with Grammars",
                &["Bob Jones"],
                "",
            ),
            paper(
                "c",
                "neurips",
                2025,
                "Diffusion for Agents",
                &["Carol"],
                "Diffusion models.",
            ),
            paper(
                "d",
                "neurips",
                2025,
                "100% Accurate_Model",
                &["Dan"],
                "A model.",
            ),
            paper(
                "e",
                "tjsai",
                2025,
                "エージェントによる社会シミュレーション",
                &["山田 太郎"],
                "概要",
            ),
        ])
        .unwrap();
        db.mark_completed("acl", 2025, 1).unwrap();
        db.mark_completed("jasss", 2026, 0).unwrap();
        (dir, db)
    }

    fn base() -> PaperQuery {
        PaperQuery {
            conferences: vec![],
            years: vec![],
            q: String::new(),
            fields: vec![SearchField::Title],
            offset: 0,
            limit: 50,
            sort: SortOrder::YearDesc,
            facets: false,
        }
    }

    #[test]
    fn catalog_lists_every_supported_venue_with_counts_and_sync_state() {
        let (_d, db) = seeded();
        let cat = db.query_catalog().unwrap();
        assert_eq!(cat.total, 5);
        assert_eq!(cat.with_abstract, 4);
        assert_eq!(
            cat.venues.len(),
            crate::conference::list_conferences().len()
        );

        let acl = cat.venues.iter().find(|v| v.id == "acl").unwrap();
        assert_eq!(acl.total, 2);
        assert_eq!(acl.with_abstract, 1);
        assert_eq!(
            acl.years.iter().map(|y| y.year).collect::<Vec<_>>(),
            vec![2025, 2024]
        );
        assert!(acl.years[0].completed && acl.years[0].synced);
        assert!(!acl.years[1].synced);

        // 論文 0 件でも同期の記録がある年は出る
        let jasss = cat.venues.iter().find(|v| v.id == "jasss").unwrap();
        assert_eq!(jasss.years.len(), 1);
        assert_eq!(jasss.years[0].count, 0);
        assert!(jasss.years[0].completed);

        // 一度も同期していない学会は空の years で出る
        let crypto = cat.venues.iter().find(|v| v.id == "crypto").unwrap();
        assert!(crypto.years.is_empty());
    }

    #[test]
    fn papers_filters_by_conference_year_and_terms() {
        let (_d, db) = seeded();
        let q = PaperQuery {
            conferences: vec!["neurips".into()],
            ..base()
        };
        assert_eq!(db.query_papers(&q).unwrap().total, 2);

        let q = PaperQuery {
            years: vec![2024],
            ..base()
        };
        let page = db.query_papers(&q).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.papers[0].id, "b");
        assert!(!page.papers[0].has_abstract);

        // 語は AND・大文字小文字を区別しない
        let q = PaperQuery {
            q: "agents DIFFUSION".into(),
            ..base()
        };
        let page = db.query_papers(&q).unwrap();
        assert_eq!(
            page.papers
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );

        // 日本語もそのまま当たる
        let q = PaperQuery {
            q: "エージェント".into(),
            ..base()
        };
        assert_eq!(db.query_papers(&q).unwrap().total, 1);
    }

    #[test]
    fn papers_searches_selected_fields_only() {
        let (_d, db) = seeded();
        let q = PaperQuery {
            q: "LLM".into(),
            ..base()
        };
        assert_eq!(db.query_papers(&q).unwrap().total, 0);

        let q = PaperQuery {
            q: "LLM".into(),
            fields: vec![SearchField::Abstract],
            ..base()
        };
        assert_eq!(db.query_papers(&q).unwrap().total, 1);

        let q = PaperQuery {
            q: "smith".into(),
            fields: vec![SearchField::Authors],
            ..base()
        };
        assert_eq!(db.query_papers(&q).unwrap().total, 1);
    }

    #[test]
    fn like_wildcards_in_terms_are_literal() {
        let (_d, db) = seeded();
        let q = PaperQuery {
            q: "%".into(),
            ..base()
        };
        let page = db.query_papers(&q).unwrap();
        assert_eq!(
            page.papers
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            vec!["d"]
        );

        let q = PaperQuery {
            q: "_".into(),
            ..base()
        };
        assert_eq!(db.query_papers(&q).unwrap().total, 1);
    }

    #[test]
    fn papers_pages_with_stable_order() {
        let (_d, db) = seeded();
        let q = PaperQuery { limit: 2, ..base() };
        let p1 = db.query_papers(&q).unwrap();
        let p2 = db
            .query_papers(&PaperQuery {
                offset: 2,
                ..q.clone()
            })
            .unwrap();
        let p3 = db
            .query_papers(&PaperQuery {
                offset: 4,
                ..q.clone()
            })
            .unwrap();
        assert_eq!(p1.total, 5);
        let ids: Vec<_> = [p1, p2, p3]
            .into_iter()
            .flat_map(|p| p.papers.into_iter().map(|r| r.id))
            .collect();
        // year DESC → conference ASC → title ASC
        assert_eq!(ids, vec!["a", "d", "c", "e", "b"]);
    }

    #[test]
    fn limit_is_clamped() {
        let (_d, db) = seeded();
        let page = db.query_papers(&PaperQuery { limit: 0, ..base() }).unwrap();
        assert_eq!(page.limit, 1);
        let page = db
            .query_papers(&PaperQuery {
                limit: 100_000,
                ..base()
            })
            .unwrap();
        assert_eq!(page.limit, MAX_LIMIT);
    }

    #[test]
    fn facets_drop_only_their_own_axis() {
        let (_d, db) = seeded();
        let q = PaperQuery {
            conferences: vec!["acl".into()],
            years: vec![2025],
            facets: true,
            ..base()
        };
        let page = db.query_papers(&q).unwrap();
        assert_eq!(page.total, 1);
        let f = page.facets.unwrap();
        // 会議の件数は年 2025 だけで数える（会議の絞り込みは外す）
        let conf: Vec<_> = f
            .conference
            .iter()
            .map(|c| (c.value.as_str(), c.count))
            .collect();
        assert_eq!(conf, vec![("neurips", 2), ("acl", 1), ("tjsai", 1)]);
        // 年の件数は acl だけで数える（年の絞り込みは外す）
        let year: Vec<_> = f.year.iter().map(|y| (y.value, y.count)).collect();
        assert_eq!(year, vec![(2025, 1), (2024, 1)]);
    }

    #[test]
    fn paper_returns_abstract_or_null() {
        let (_d, db) = seeded();
        let d = db.query_paper("a", "acl", 2025).unwrap();
        assert_eq!(d.paper.unwrap().r#abstract, "We study LLM agents.");
        assert!(db.query_paper("a", "acl", 2024).unwrap().paper.is_none());
    }

    #[test]
    fn parse_options() {
        assert_eq!(
            SearchField::parse("abstract").unwrap(),
            SearchField::Abstract
        );
        assert!(SearchField::parse("body").is_err());
        assert_eq!(SortOrder::parse("title").unwrap(), SortOrder::Title);
        assert!(SortOrder::parse("random").is_err());
    }
}
