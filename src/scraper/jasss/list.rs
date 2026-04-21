use anyhow::Result;
use scraper::{Html, Selector};
use std::time::Duration;

use crate::types::PaperListEntry;

/// 指定 Volume/Issue のコンテンツページから論文リストを取得
pub async fn fetch_paper_list(
    client: &reqwest::Client,
    base_url: &str,
    volume: u16,
    issue: u16,
    interval: Duration,
) -> Result<Vec<PaperListEntry>> {
    let url = format!("{}/{}/{}/contents.html", base_url, volume, issue);
    let body = match crate::scraper::fetch_with_sleep(client, &url, interval).await {
        Ok(b) => b,
        Err(e) => {
            // 404 = issue が存在しない（最新 Volume の未刊行号など）
            tracing::debug!("Skipping {}/{}: {}", volume, issue, e);
            return Ok(Vec::new());
        }
    };
    parse_contents(&body, base_url, volume, issue)
}

/// コンテンツページの HTML をパースして論文リストを返す．
///
/// 2つのフォーマットに対応:
/// - 新形式 (Vol 10+): `<p class="item"><a href="URL">Title</a><br>Authors</p>`
/// - 旧形式 (Vol 1-9):  `<b>Authors</b><br><a href="N.html">Title</a>` (blockquote内)
pub fn parse_contents(
    html: &str,
    base_url: &str,
    volume: u16,
    issue: u16,
) -> Result<Vec<PaperListEntry>> {
    let document = Html::parse_document(html);

    // 新形式を先に試す
    let entries = parse_new_format(&document, base_url, volume, issue);
    if !entries.is_empty() {
        return Ok(entries);
    }

    // 旧形式にフォールバック
    Ok(parse_old_format(&document, base_url, volume, issue))
}

/// 新形式: `<p class="item">` を使うパターン (Vol 10+)
fn parse_new_format(
    document: &Html,
    base_url: &str,
    volume: u16,
    issue: u16,
) -> Vec<PaperListEntry> {
    let item_selector = Selector::parse("p.item").expect("valid selector");
    let a_selector = Selector::parse("a").expect("valid selector");

    let mut entries = Vec::new();

    for p_elem in document.select(&item_selector) {
        let link = match p_elem.select(&a_selector).next() {
            Some(a) => a,
            None => continue,
        };

        let href = match link.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        // editorial, review, forum など非論文エントリを除外
        if is_non_article(href) {
            continue;
        }

        let title = clean_text(&link.text().collect::<String>());
        if title.is_empty() {
            continue;
        }

        let detail_url = resolve_url(href, base_url, volume, issue);

        // 著者は <br> の後のテキストノード
        let authors = extract_authors_new_format(&p_elem);

        entries.push(PaperListEntry {
            title,
            authors,
            detail_url,
            track: None,
        });
    }

    entries
}

/// 旧形式: `<blockquote>` 内の `<b>Author</b><br><a>Title</a>` パターン (Vol 1-9)
fn parse_old_format(
    document: &Html,
    base_url: &str,
    volume: u16,
    issue: u16,
) -> Vec<PaperListEntry> {
    let a_selector = Selector::parse("blockquote a").expect("valid selector");

    let mut entries = Vec::new();

    for a_elem in document.select(&a_selector) {
        let href = match a_elem.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        // 数字.html のみを対象（editorial.html, review*.html 等を除外）
        if is_non_article(href) {
            continue;
        }

        let title = clean_text(&a_elem.text().collect::<String>());
        if title.is_empty() {
            continue;
        }

        let detail_url = resolve_url(href, base_url, volume, issue);

        // 旧形式では <b>Author</b> が <a> の直前にある
        let authors = extract_authors_old_format(&a_elem);

        entries.push(PaperListEntry {
            title,
            authors,
            detail_url,
            track: None,
        });
    }

    entries
}

/// 新形式: <p class="item"> 内の <br> 以降のテキストから著者を抽出
fn extract_authors_new_format(p_elem: &scraper::ElementRef) -> Vec<String> {
    let inner = p_elem.inner_html();

    // <br> 以降のテキスト部分を取得
    let after_br = match inner.split("<br>").nth(1).or_else(|| inner.split("<br/>").nth(1)) {
        Some(s) => s,
        None => return Vec::new(),
    };

    // HTML タグを除去
    let fragment = Html::parse_fragment(after_br);
    let text = fragment
        .root_element()
        .text()
        .collect::<String>();

    parse_author_string(&text)
}

/// 旧形式: <a> 要素の前にある <b> タグから著者を抽出
fn extract_authors_old_format(a_elem: &scraper::ElementRef) -> Vec<String> {
    // 前の兄弟ノードを遡って <b> 要素を探す（間にテキストノードや <br> がある）
    let mut node = a_elem.prev_sibling();
    while let Some(n) = node {
        if let Some(elem_ref) = scraper::ElementRef::wrap(n)
            && elem_ref.value().name() == "b"
        {
            let text = elem_ref.text().collect::<String>();
            return parse_author_string(&text);
        }
        node = n.prev_sibling();
    }
    Vec::new()
}

/// "Author1, Author2 and Author3" 形式の文字列を個別著者に分割
fn parse_author_string(s: &str) -> Vec<String> {
    let s = clean_text(s);
    if s.is_empty() {
        return Vec::new();
    }

    // "and" で分割してから "," でさらに分割
    let mut authors = Vec::new();
    for part in s.split(" and ") {
        for name in part.split(',') {
            let name = clean_text(name);
            if !name.is_empty() {
                authors.push(name);
            }
        }
    }
    authors
}

/// href が論文を指しているか判定（editorial, review, forum 等を除外）
fn is_non_article(href: &str) -> bool {
    let filename = href.rsplit('/').next().unwrap_or(href);
    let lower = filename.to_lowercase();

    // 数字のみのファイル名（N.html）は論文
    if let Some(stem) = lower.strip_suffix(".html")
        && stem.chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }

    true
}

/// 相対/絶対 URL を正規化
fn resolve_url(href: &str, base_url: &str, volume: u16, issue: u16) -> String {
    let href = href.trim();
    if href.starts_with("http") {
        return href.to_string();
    }
    // "N.html" → "https://www.jasss.org/{vol}/{issue}/N.html"
    format!("{}/{}/{}/{}", base_url, volume, issue, href)
}

/// テキストの空白を正規化
fn clean_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_new_format() {
        let html = r#"
        <html><body>
        <p class="item"><a href="https://www.jasss.org/29/1/7.html">
        All Models Are Wrong</a><br>
           Emma Von Hoene, Sara Von Hoene and Taylor Anderson</p>
        <p class="item"><a href="https://www.jasss.org/29/1/5.html">
        OPOSim: An Agent-Based Model</a><br>
           Michael A. Duprey and Georgiy V. Bobashev</p>
        </body></html>
        "#;

        let entries =
            parse_contents(html, "https://www.jasss.org", 29, 1).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "All Models Are Wrong");
        assert_eq!(
            entries[0].authors,
            vec!["Emma Von Hoene", "Sara Von Hoene", "Taylor Anderson"]
        );
        assert_eq!(
            entries[0].detail_url,
            "https://www.jasss.org/29/1/7.html"
        );
    }

    #[test]
    fn test_parse_old_format() {
        let html = r#"
        <html><body>
        <blockquote>
        <b>Dwight W. Read</b><br>
        <a href="1.html">Kinship based demographic simulation</a><p>
        <b>Jim Doran</b><br>
        <a href="3.html">Simulating Collective Misbelief</a><p>
        </blockquote>
        </body></html>
        "#;

        let entries =
            parse_contents(html, "https://www.jasss.org", 1, 1).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Kinship based demographic simulation");
        assert_eq!(entries[0].authors, vec!["Dwight W. Read"]);
        assert_eq!(
            entries[0].detail_url,
            "https://www.jasss.org/1/1/1.html"
        );
    }

    #[test]
    fn test_is_non_article() {
        assert!(!is_non_article("1.html"));
        assert!(!is_non_article("7.html"));
        assert!(!is_non_article("https://www.jasss.org/29/1/7.html"));
        assert!(is_non_article("editorial.html"));
        assert!(is_non_article("review1.html"));
        assert!(is_non_article("contents.html"));
    }

    #[test]
    fn test_parse_author_string() {
        assert_eq!(
            parse_author_string("Alice Smith, Bob Jones and Carol White"),
            vec!["Alice Smith", "Bob Jones", "Carol White"]
        );
        assert_eq!(
            parse_author_string("Single Author"),
            vec!["Single Author"]
        );
        assert_eq!(
            parse_author_string("A and B"),
            vec!["A", "B"]
        );
    }

    #[test]
    fn test_skip_editorial_and_review() {
        let html = r#"
        <html><body>
        <blockquote>
        <h3><a href="editorial.html">Editorial</a></h3>
        <b>Author A</b><br>
        <a href="1.html">Real Paper</a><p>
        <a href="reviews/review1.html">A Book Review</a><br>
        </blockquote>
        </body></html>
        "#;

        let entries =
            parse_contents(html, "https://www.jasss.org", 1, 1).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Real Paper");
    }

    #[test]
    fn test_resolve_url() {
        assert_eq!(
            resolve_url("3.html", "https://www.jasss.org", 1, 1),
            "https://www.jasss.org/1/1/3.html"
        );
        assert_eq!(
            resolve_url("https://www.jasss.org/29/1/7.html", "https://www.jasss.org", 29, 1),
            "https://www.jasss.org/29/1/7.html"
        );
        // 先頭にスペースがある絶対URL（旧形式の一部ページで発生）
        assert_eq!(
            resolve_url(" https://www.jasss.org/6/2/10.html", "https://www.jasss.org", 6, 2),
            "https://www.jasss.org/6/2/10.html"
        );
    }
}
