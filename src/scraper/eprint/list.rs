use anyhow::Result;
use scraper::{Html, Selector};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{compute_id, Paper, PaperListEntry};

/// 年別一覧の 1 ページあたりの件数（`?offset=` の刻み）
const PAGE_SIZE: usize = 100;
/// ページ送りの上限（1 年あたり最大 PAGE_SIZE * MAX_PAGES 件）
const MAX_PAGES: usize = 200;

/// 年別一覧を全ページ取得する．
///
/// 現行の一覧ページ（`.paperList`）はタイトル・著者・カテゴリ・アブストラクトを含むので，
/// 詳細ページを取らずに `Paper` まで組み立てて返す．現行形式で 1 件も取れない場合は
/// 旧形式のパーサで `PaperListEntry` だけを返す（`Paper` は空）．
pub async fn fetch_paper_list(
    client: &reqwest::Client,
    base_url: &str,
    year: u16,
    interval: Duration,
) -> Result<(Vec<PaperListEntry>, Vec<Paper>)> {
    let mut entries = Vec::new();
    let mut papers = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for page in 0..MAX_PAGES {
        let url = format!("{}/{}/?offset={}", base_url, year, page * PAGE_SIZE);
        let html = fetch_with_sleep(client, &url, interval).await?;
        let parsed = parse_paper_list_page(&html, base_url, year, "eprint");

        if parsed.is_empty() {
            if page == 0 {
                return Ok((parse_paper_list(&html, base_url, year)?, Vec::new()));
            }
            break;
        }

        let mut new_count = 0;
        for (entry, paper) in parsed {
            if seen.insert(entry.detail_url.clone()) {
                entries.push(entry);
                papers.push(paper);
                new_count += 1;
            }
        }
        // offset が範囲外でも同じページを返すサイトに備え，新規が無ければ終了
        if new_count == 0 {
            break;
        }
    }

    Ok((entries, papers))
}

/// 現行形式の一覧ページ（`div.paperList`）を解析する．
///
/// 1 論文は 2 つの兄弟 `div` で表される:
///   - 見出し: `<div class="flex-grow-1"><a href="/2024/2100">2024/2100</a> ...`
///   - 本体:   `.papertitle` / `.summaryauthors` / `.category` / `.paper-abstract`
fn parse_paper_list_page(
    html: &str,
    base_url: &str,
    year: u16,
    conference: &str,
) -> Vec<(PaperListEntry, Paper)> {
    let document = Html::parse_document(html);
    let list_sel = Selector::parse("div.paperList").unwrap();
    let link_sel = Selector::parse("a[href]").unwrap();
    let title_sel = Selector::parse(".papertitle").unwrap();
    let authors_sel = Selector::parse(".summaryauthors").unwrap();
    let category_sel = Selector::parse(".category").unwrap();
    let abstract_sel = Selector::parse(".paper-abstract").unwrap();

    let text_of = |el: scraper::ElementRef| el.text().collect::<String>().trim().to_string();

    let mut out = Vec::new();
    for list in document.select(&list_sel) {
        let mut current_url: Option<String> = None;
        for child in list.children().filter_map(scraper::ElementRef::wrap) {
            if let Some(title_el) = child.select(&title_sel).next() {
                let Some(detail_url) = current_url.take() else {
                    continue;
                };
                let title = text_of(title_el);
                if title.is_empty() {
                    continue;
                }
                let authors = child
                    .select(&authors_sel)
                    .next()
                    .map(|el| parse_author_names(&text_of(el)))
                    .unwrap_or_default();
                let mut categories: Vec<String> = Vec::new();
                for el in child.select(&category_sel) {
                    let c = text_of(el);
                    if !c.is_empty() && !categories.contains(&c) {
                        categories.push(c);
                    }
                }
                let abstract_text = child
                    .select(&abstract_sel)
                    .next()
                    .map(text_of)
                    .unwrap_or_default();

                let entry = PaperListEntry {
                    title: title.clone(),
                    authors: authors.clone(),
                    detail_url: detail_url.clone(),
                    track: None,
                };
                let paper = Paper {
                    id: compute_id(&title),
                    conference: conference.to_string(),
                    year,
                    hash: super::paper_page::compute_hash(&title, &abstract_text),
                    pdf_url: Some(format!("{}.pdf", detail_url)),
                    title,
                    authors,
                    r#abstract: abstract_text,
                    url: detail_url,
                    categories,
                };
                out.push((entry, paper));
            } else if let Some(href) = child
                .select(&link_sel)
                .filter_map(|a| a.value().attr("href"))
                .find(|h| is_paper_link(h, year))
            {
                current_url = Some(normalize_url(href, base_url));
            }
        }
    }
    out
}

fn parse_paper_list(html: &str, base_url: &str, year: u16) -> Result<Vec<PaperListEntry>> {
    let document = Html::parse_document(html);

    // ePrint year listing page structure:
    // Each paper entry is a row/item containing:
    //   - A link to /{YEAR}/{NUMBER} with the paper title
    //   - Author names (typically in the same row)
    //   - Category information
    //
    // The listing page uses a structure like:
    //   <div class="paperList">
    //     <div class="paperListItem">
    //       <a href="/2024/001">Title</a>
    //       <span>Authors</span>
    //     </div>
    //   </div>
    //
    // Alternatively, entries may appear as:
    //   <a href="/2024/001">2024/001</a> <b>Title</b> <i>Authors</i>
    //
    // We use a flexible approach: find all links matching /{YEAR}/{NUMBER}

    let mut entries = Vec::new();

    // Strategy 1: Look for .paperListItem or similar container
    let item_selector = Selector::parse(".paperListItem").ok();
    if let Some(ref sel) = item_selector {
        let title_link_sel = Selector::parse("a").unwrap();
        for item in document.select(sel) {
            if let Some(entry) = parse_list_item_container(&item, base_url, year, &title_link_sel) {
                entries.push(entry);
            }
        }
    }

    // Strategy 2: If no items found, try table rows (some years use tables)
    if entries.is_empty()
        && let Ok(row_sel) = Selector::parse("tr") {
            let link_sel = Selector::parse("a").unwrap();
            for row in document.select(&row_sel) {
                if let Some(entry) = parse_table_row(&row, base_url, year, &link_sel) {
                    entries.push(entry);
                }
            }
        }

    // Strategy 3: Fallback - find all links matching /{YEAR}/{NUMBER} pattern
    if entries.is_empty() {
        let link_pattern = format!("a[href*='/{}/']", year);
        if let Ok(link_sel) = Selector::parse(&link_pattern) {
            for link in document.select(&link_sel) {
                if let Some(href) = link.value().attr("href") {
                    if !is_paper_link(href, year) {
                        continue;
                    }

                    let title = link
                        .text()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_string();

                    // Skip if the link text is just the paper ID (e.g. "2024/001")
                    if title.is_empty() || title.contains('/') {
                        // Try to get title from next sibling or parent
                        continue;
                    }

                    let detail_url = normalize_url(href, base_url);
                    entries.push(PaperListEntry {
                        title,
                        authors: Vec::new(),
                        detail_url,
                        track: None,
                    });
                }
            }
        }
    }

    Ok(entries)
}

/// Parse a container element (e.g., .paperListItem) for paper info
fn parse_list_item_container(
    item: &scraper::ElementRef,
    base_url: &str,
    year: u16,
    link_sel: &Selector,
) -> Option<PaperListEntry> {
    // Find a link that points to a paper page
    for link in item.select(link_sel) {
        let href = link.value().attr("href")?;
        if !is_paper_link(href, year) {
            continue;
        }

        let detail_url = normalize_url(href, base_url);

        // Get title - could be the link text or a nearby element
        let link_text = link
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        // Try to find title from bold/strong element or the link text itself
        let title = if link_text.contains('/') || link_text.is_empty() {
            // Link text is just ID like "2024/001", look for title elsewhere
            let bold_sel = Selector::parse("b, strong, .title").ok()?;
            item.select(&bold_sel)
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .unwrap_or(link_text)
        } else {
            link_text
        };

        if title.is_empty() {
            continue;
        }

        // Try to extract authors from italic or .authors element
        let authors = extract_authors_from_container(item);

        return Some(PaperListEntry {
            title,
            authors,
            detail_url,
            track: None,
        });
    }
    None
}

/// Parse a table row for paper info
fn parse_table_row(
    row: &scraper::ElementRef,
    base_url: &str,
    year: u16,
    link_sel: &Selector,
) -> Option<PaperListEntry> {
    for link in row.select(link_sel) {
        let href = link.value().attr("href")?;
        if !is_paper_link(href, year) {
            continue;
        }

        let detail_url = normalize_url(href, base_url);

        // Get all text from the row to extract title and authors
        let link_text = link
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        let title = if link_text.contains('/') || link_text.is_empty() {
            // Look for title in other cells
            let td_sel = Selector::parse("td").ok()?;
            let cells: Vec<String> = row
                .select(&td_sel)
                .map(|td| td.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .filter(|t| !t.is_empty() && !t.contains('/'))
                .collect();
            cells.first().cloned().unwrap_or(link_text)
        } else {
            link_text
        };

        if title.is_empty() {
            continue;
        }

        return Some(PaperListEntry {
            title,
            authors: Vec::new(),
            detail_url,
            track: None,
        });
    }
    None
}

/// Extract authors from a container element
fn extract_authors_from_container(item: &scraper::ElementRef) -> Vec<String> {
    // Try .authors class first
    if let Ok(sel) = Selector::parse(".authors, i, em")
        && let Some(el) = item.select(&sel).next() {
            let raw = el
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            if !raw.is_empty() {
                return parse_author_names(&raw);
            }
        }
    Vec::new()
}

/// Parse comma-separated or "and"-separated author names
fn parse_author_names(raw: &str) -> Vec<String> {
    raw.split(',')
        .flat_map(|part| part.split(" and "))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// Check if a URL matches the pattern /{YEAR}/{NUMBER}
fn is_paper_link(href: &str, year: u16) -> bool {
    let year_str = year.to_string();
    // Match patterns like /2024/001 or /2024/1234
    let parts: Vec<&str> = href.trim_matches('/').split('/').collect();
    if parts.len() >= 2 {
        let last_two: Vec<&str> = parts.iter().rev().take(2).copied().collect();
        // last_two[0] is the number, last_two[1] is the year
        if last_two[1] == year_str {
            return last_two[0].chars().all(|c| c.is_ascii_digit()) && !last_two[0].is_empty();
        }
    }
    false
}

/// Normalize a URL (make absolute if relative)
fn normalize_url(href: &str, base_url: &str) -> String {
    if href.starts_with("http") {
        href.to_string()
    } else {
        format!("{}{}", base_url.trim_end_matches('/'), if href.starts_with('/') { href.to_string() } else { format!("/{}", href) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_paper_link() {
        assert!(is_paper_link("/2024/001", 2024));
        assert!(is_paper_link("/2024/1234", 2024));
        assert!(is_paper_link("https://eprint.iacr.org/2024/001", 2024));
        assert!(!is_paper_link("/2024/001", 2023));
        assert!(!is_paper_link("/2024/abc", 2024));
        assert!(!is_paper_link("/2024/001.pdf", 2024));
        assert!(!is_paper_link("/search?category=crypto", 2024));
    }

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            normalize_url("/2024/001", "https://eprint.iacr.org"),
            "https://eprint.iacr.org/2024/001"
        );
        assert_eq!(
            normalize_url("https://eprint.iacr.org/2024/001", "https://eprint.iacr.org"),
            "https://eprint.iacr.org/2024/001"
        );
    }

    #[test]
    fn test_parse_author_names() {
        let authors = parse_author_names("Alice Smith, Bob Jones, Charlie Brown");
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones", "Charlie Brown"]);
    }

    #[test]
    fn test_parse_author_names_with_and() {
        let authors = parse_author_names("Alice Smith and Bob Jones");
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones"]);
    }

    #[test]
    fn test_parse_paper_list_with_links() {
        let html = r#"
        <html><body>
        <div>
            <a href="/2024/001">A Novel Cryptographic Protocol</a>
            <a href="/2024/002">Efficient Zero-Knowledge Proofs</a>
            <a href="/search?category=crypto">Crypto Category</a>
        </div>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://eprint.iacr.org", 2024).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "A Novel Cryptographic Protocol");
        assert_eq!(
            entries[0].detail_url,
            "https://eprint.iacr.org/2024/001"
        );
        assert_eq!(entries[1].title, "Efficient Zero-Knowledge Proofs");
        assert_eq!(
            entries[1].detail_url,
            "https://eprint.iacr.org/2024/002"
        );
    }

    #[test]
    fn test_parse_paper_list_table_format() {
        let html = r#"
        <html><body>
        <table>
            <tr>
                <td><a href="/2023/100">2023/100</a></td>
                <td>Post-Quantum Signatures</td>
            </tr>
            <tr>
                <td><a href="/2023/101">2023/101</a></td>
                <td>Lattice-Based Encryption</td>
            </tr>
        </table>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://eprint.iacr.org", 2023).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Post-Quantum Signatures");
        assert_eq!(
            entries[0].detail_url,
            "https://eprint.iacr.org/2023/100"
        );
    }

    #[test]
    fn test_parse_paper_list_container_format() {
        let html = r#"
        <html><body>
        <div class="paperList">
            <div class="paperListItem">
                <a href="/2024/500">2024/500</a>
                <b>Secure Multi-Party Computation</b>
                <i>Alice Smith, Bob Jones</i>
            </div>
            <div class="paperListItem">
                <a href="/2024/501">2024/501</a>
                <b>Homomorphic Encryption Schemes</b>
                <em>Charlie Brown and Dave Wilson</em>
            </div>
        </div>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://eprint.iacr.org", 2024).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Secure Multi-Party Computation");
        assert_eq!(entries[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(entries[1].title, "Homomorphic Encryption Schemes");
        assert_eq!(entries[1].authors, vec!["Charlie Brown", "Dave Wilson"]);
    }

    #[test]
    fn test_parse_paper_list_empty() {
        let html = r#"<html><body><p>No papers yet.</p></body></html>"#;
        let entries = parse_paper_list(html, "https://eprint.iacr.org", 2024).unwrap();
        assert!(entries.is_empty());
    }

    const CURRENT_FORMAT: &str = r#"
<div class="paperList ms-lg-4 me-lg-4">
  <div class="d-flex mb-1">
    <div class="flex-grow-1"><a href="/2024/2100">2024/2100</a>
      <a class="ms-2" href="/2024/2100.pdf">(PDF)</a>
    </div>
    <div><small>Last updated:&nbsp; 2024-12-31</small></div>
  </div>
  <div class="ms-3 ms-md-5 mb-3">
    <div class="d-flex justify-content-between">
      <div class="papertitle">Compact Key Storage in the Standard Model</div>
      <div class="ms-3 d-none d-md-block">
        <a href="/search?category=PROTOCOLS"><small class="badge category category-PROTOCOLS">Cryptographic protocols</small></a>
      </div>
    </div>
    <div class="summaryauthors">Yevgeniy Dodis and Daniel Jost</div>
    <div class="d-block d-md-none">
      <a href="/search?category=PROTOCOLS"><small class="badge category category-PROTOCOLS">Cryptographic protocols</small></a>
    </div>
    <div><div id="abstract-2024-2100" class="paper-abstract">In recent work [Crypto&#39;24], we study CKS.</div></div>
  </div>
  <div class="d-flex mb-1">
    <div class="flex-grow-1"><a href="/2024/2099">2024/2099</a></div>
  </div>
  <div class="ms-3 ms-md-5 mb-3">
    <div class="papertitle">Second Paper</div>
    <div class="summaryauthors">Alice, Bob, and Carol</div>
  </div>
</div>
"#;

    #[test]
    fn test_parse_paper_list_page_current_format() {
        let parsed = parse_paper_list_page(CURRENT_FORMAT, "https://eprint.iacr.org", 2024, "eprint");
        assert_eq!(parsed.len(), 2);

        let (entry, paper) = &parsed[0];
        assert_eq!(entry.title, "Compact Key Storage in the Standard Model");
        assert_eq!(entry.detail_url, "https://eprint.iacr.org/2024/2100");
        assert_eq!(paper.authors, vec!["Yevgeniy Dodis", "Daniel Jost"]);
        assert_eq!(paper.categories, vec!["Cryptographic protocols"]);
        assert_eq!(paper.r#abstract, "In recent work [Crypto'24], we study CKS.");
        assert_eq!(paper.pdf_url.as_deref(), Some("https://eprint.iacr.org/2024/2100.pdf"));
        assert_eq!(paper.id, compute_id(&paper.title));

        let (_, second) = &parsed[1];
        assert_eq!(second.title, "Second Paper");
        assert_eq!(second.authors, vec!["Alice", "Bob", "Carol"]);
        assert!(second.r#abstract.is_empty());
    }

    #[test]
    fn test_parse_paper_list_page_ignores_old_format() {
        let html = r#"<html><body><a href="/2024/001">2024/001</a> <b>Title</b></body></html>"#;
        assert!(parse_paper_list_page(html, "https://eprint.iacr.org", 2024, "eprint").is_empty());
    }
}
