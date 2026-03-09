-- 同期状態の管理（会議×年度）
CREATE TABLE IF NOT EXISTS sync_years (
    conference TEXT NOT NULL,
    year       INTEGER NOT NULL,
    paper_count INTEGER NOT NULL DEFAULT 0,
    synced_at  TEXT,
    completed  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (conference, year)
);

-- 論文メタ情報
CREATE TABLE IF NOT EXISTS papers (
    id         TEXT NOT NULL,
    conference TEXT NOT NULL,
    year       INTEGER NOT NULL,
    title      TEXT NOT NULL,
    authors    TEXT NOT NULL,
    abstract   TEXT NOT NULL,
    url        TEXT NOT NULL,
    pdf_url    TEXT,
    categories TEXT NOT NULL,
    hash       TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (id, conference, year)
);

CREATE INDEX IF NOT EXISTS idx_papers_conference_year ON papers(conference, year);
CREATE INDEX IF NOT EXISTS idx_papers_title ON papers(title);
