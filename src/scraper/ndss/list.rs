use anyhow::Result;
use scraper::{Html, Selector};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::PaperListEntry;

pub async fn fetch_paper_list(
    client: &reqwest::Client,
    base_url: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<PaperListEntry>> {
    let url = format!("{}/ndss{}/accepted-papers/", base_url, year);
    let html = fetch_with_sleep(client, &url, interval).await?;
    parse_paper_list(&html, base_url)
}

fn parse_paper_list(html: &str, base_url: &str) -> Result<Vec<PaperListEntry>> {
    let document = Html::parse_document(html);

    // NDSS listing uses: <div class="pt-cv-content-item ...">
    //   <h2 class="pt-cv-title"><a href="...">Title</a></h2>
    //   <div class="pt-cv-ctf-list">
    //     <div class="pt-cv-custom-fields pt-cv-ctf-display_authors">
    //       <div class="pt-cv-ctf-value"><p>Author1, Author2, ...</p></div>
    //     </div>
    //   </div>
    // </div>

    let item_selector = Selector::parse(".pt-cv-content-item").unwrap();
    let title_selector = Selector::parse("h2.pt-cv-title a").unwrap();
    let authors_selector = Selector::parse(".pt-cv-ctf-display_authors .pt-cv-ctf-value").unwrap();

    let mut entries = Vec::new();

    for item in document.select(&item_selector) {
        let title_el = match item.select(&title_selector).next() {
            Some(el) => el,
            None => continue,
        };

        let title = title_el
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }

        let href = match title_el.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        // Only include links to /ndss-paper/ pages
        if !href.contains("/ndss-paper/") {
            continue;
        }

        let detail_url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("{}{}", base_url, href)
        };

        // Extract authors
        let authors = if let Some(authors_el) = item.select(&authors_selector).next() {
            let raw = authors_el
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            parse_authors(&raw)
        } else {
            Vec::new()
        };

        entries.push(PaperListEntry {
            title,
            authors,
            detail_url,
            track: Some("Conference".to_string()),
        });
    }

    // Fallback: if pt-cv-content-item structure is not found, try simpler h2 a pattern
    if entries.is_empty() {
        let link_selector = Selector::parse("a[href*='/ndss-paper/']").unwrap();
        for element in document.select(&link_selector) {
            let title = element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            if title.is_empty() {
                continue;
            }

            let href = match element.value().attr("href") {
                Some(h) => h,
                None => continue,
            };

            let detail_url = if href.starts_with("http") {
                href.to_string()
            } else {
                format!("{}{}", base_url, href)
            };

            entries.push(PaperListEntry {
                title,
                authors: Vec::new(),
                detail_url,
                track: Some("Conference".to_string()),
            });
        }
    }

    // Legacy NDSS listings (2014-2018) use a single-paper block and link either
    // to a programme page or directly to a PDF.
    if entries.is_empty() {
        let item_selector = Selector::parse("div.single-paper").unwrap();
        let inner_selector = Selector::parse("div.inner-wrap").unwrap();
        let title_selector = Selector::parse("h3").unwrap();
        let authors_selector = Selector::parse("p").unwrap();
        let link_selector = Selector::parse("p.paper-link-abs a").unwrap();
        let mut excluded_workshops = 0usize;

        for item in document.select(&item_selector) {
            let Some(inner) = item.select(&inner_selector).next() else {
                continue;
            };
            let Some(title_el) = inner.select(&title_selector).next() else {
                continue;
            };
            let title = title_el
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            if title.is_empty() {
                continue;
            }

            let Some(link_el) = item.select(&link_selector).next() else {
                continue;
            };
            let Some(href) = link_el.value().attr("href") else {
                continue;
            };
            if is_workshop_programme_url(href) {
                excluded_workshops += 1;
                continue;
            }

            // Some entries carry an empty <p> before the author line (e.g. NDSS 2018 "Veil").
            let authors = inner
                .select(&authors_selector)
                .map(|authors_el| {
                    let raw = authors_el.text().collect::<Vec<_>>().join(" ");
                    parse_plain_authors(&raw)
                })
                .find(|authors| !authors.is_empty())
                .unwrap_or_default();
            let detail_url = if href.starts_with("http") {
                href.to_string()
            } else {
                format!("{}{}", base_url, href)
            };

            entries.push(PaperListEntry {
                title,
                authors,
                detail_url,
                track: Some("Conference".to_string()),
            });
        }

        tracing::debug!(
            excluded_workshops,
            "Excluded workshop papers from legacy NDSS listing"
        );
    }

    Ok(entries)
}

fn is_workshop_programme_url(url: &str) -> bool {
    url.split(['/', '?', '#'])
        .find_map(|segment| segment.strip_suffix("-programme"))
        .is_some_and(|programme| {
            !programme
                .strip_prefix("ndss-")
                .is_some_and(|year| !year.is_empty() && year.chars().all(|ch| ch.is_ascii_digit()))
        })
}

pub(super) fn parse_plain_authors(raw: &str) -> Vec<String> {
    raw.split(", ")
        .flat_map(|part| part.split(" and "))
        .map(str::trim)
        .filter(|author| !author.is_empty())
        .map(str::to_string)
        .collect()
}

/// "Author1 (Affiliation1), Author2 (Affiliation2)" 形式の著者文字列をパース
fn parse_authors(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }

    // Split by "), " to separate author entries, then clean up
    let mut authors = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();

    for ch in raw.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
                if depth == 0 {
                    let author = extract_name(&current);
                    if !author.is_empty() {
                        authors.push(author);
                    }
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    // Handle remaining text (author without affiliation)
    let remaining = current.trim().trim_matches(',').trim();
    if !remaining.is_empty() {
        // Could be multiple comma-separated authors without affiliations
        for part in remaining.split(',') {
            let name = part.trim().trim_start_matches("and ").trim();
            if !name.is_empty() {
                authors.push(name.to_string());
            }
        }
    }

    authors
}

/// "Name (Affiliation)" から名前部分を抽出
fn extract_name(entry: &str) -> String {
    if let Some(paren_pos) = entry.find('(') {
        entry[..paren_pos]
            .trim()
            .trim_matches(',')
            .trim()
            .trim_start_matches("and ")
            .trim()
            .to_string()
    } else {
        entry.trim().trim_matches(',').trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_paper_list_basic() {
        let html = r#"
        <html><body>
        <div class="pt-cv-view">
            <div class="pt-cv-content-item pt-cv-2-col" data-pid="16886">
                <h2 class="pt-cv-title">
                    <a href="https://www.ndss-symposium.org/ndss-paper/paper-one/" class="_self">Paper One Title</a>
                </h2>
                <div class="pt-cv-ctf-list">
                    <div class="pt-cv-custom-fields pt-cv-ctf-display_authors">
                        <div class="pt-cv-ctf-value"><p>Alice Smith (MIT), Bob Jones (Stanford)</p></div>
                    </div>
                </div>
            </div>
            <div class="pt-cv-content-item pt-cv-2-col" data-pid="16915">
                <h2 class="pt-cv-title">
                    <a href="https://www.ndss-symposium.org/ndss-paper/paper-two/" class="_self">Paper Two Title</a>
                </h2>
                <div class="pt-cv-ctf-list">
                    <div class="pt-cv-custom-fields pt-cv-ctf-display_authors">
                        <div class="pt-cv-ctf-value"><p>Charlie Brown (CMU)</p></div>
                    </div>
                </div>
            </div>
        </div>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Paper One Title");
        assert_eq!(entries[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(
            entries[0].detail_url,
            "https://www.ndss-symposium.org/ndss-paper/paper-one/"
        );
        assert_eq!(entries[0].track, Some("Conference".to_string()));
        assert_eq!(entries[1].title, "Paper Two Title");
        assert_eq!(entries[1].authors, vec!["Charlie Brown"]);
    }

    #[test]
    fn test_parse_paper_list_fallback() {
        // When pt-cv-content-item is not present, fall back to simple link parsing
        let html = r#"
        <html><body>
        <ul>
            <li><a href="/ndss-paper/my-paper-slug/">My Paper Title</a></li>
            <li><a href="/ndss-paper/another-paper/">Another Paper</a></li>
        </ul>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "My Paper Title");
        assert_eq!(
            entries[0].detail_url,
            "https://www.ndss-symposium.org/ndss-paper/my-paper-slug/"
        );
        assert!(entries[0].authors.is_empty());
    }

    #[test]
    fn test_parse_paper_list_skips_non_paper_links() {
        let html = r#"
        <html><body>
        <div class="pt-cv-content-item">
            <h2 class="pt-cv-title">
                <a href="https://www.ndss-symposium.org/ndss-paper/real-paper/">Real Paper</a>
            </h2>
            <div class="pt-cv-ctf-list">
                <div class="pt-cv-custom-fields pt-cv-ctf-display_authors">
                    <div class="pt-cv-ctf-value"><p>Author One (Uni)</p></div>
                </div>
            </div>
        </div>
        <div class="pt-cv-content-item">
            <h2 class="pt-cv-title">
                <a href="https://www.ndss-symposium.org/other-page/">Not A Paper</a>
            </h2>
        </div>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Real Paper");
    }

    #[test]
    fn test_parse_paper_list_2018_single_paper_with_pdf() {
        let html = r#"
        <div class="wp-block-group single-paper">
          <div class="wp-block-group inner-wrap"><div class="wp-block-group__inner-container">
            <h3 class="wp-block-heading">IoTFuzzer: Discovering Memory Corruptions in IoT Through App-based Fuzzing</h3>
            <p class="wp-block-paragraph">Jiongyi Chen, Wenrui Diao and Kehuan Zhang</p>
          </div></div>
          <p class="paper-link-abs wp-block-paragraph"><strong>
            <a href="/wp-content/uploads/2018/02/ndss2018_01A-1_Chen_paper.pdf">Read More</a>
          </strong></p>
        </div>
        "#;

        let entries = parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].title,
            "IoTFuzzer: Discovering Memory Corruptions in IoT Through App-based Fuzzing"
        );
        assert_eq!(
            entries[0].authors,
            vec!["Jiongyi Chen", "Wenrui Diao", "Kehuan Zhang"]
        );
        assert_eq!(
            entries[0].detail_url,
            "https://www.ndss-symposium.org/wp-content/uploads/2018/02/ndss2018_01A-1_Chen_paper.pdf"
        );
    }

    #[test]
    fn test_parse_paper_list_single_paper_skips_empty_paragraph() {
        let html = r#"
        <div class="wp-block-group single-paper">
          <div class="wp-block-group inner-wrap"><div class="wp-block-group__inner-container">
            <h3 class="wp-block-heading">Veil: Private Browsing Semantics Without Browser-side Assistance</h3>
            <p class="wp-block-paragraph"></p>
            <p class="wp-block-paragraph">Frank Wang, James Mickens and Nickolai Zeldovich</p>
          </div></div>
          <p class="paper-link-abs wp-block-paragraph"><strong>
            <a href="/wp-content/uploads/2018/02/ndss2018_06B-4_Wang_paper.pdf">Read More</a>
          </strong></p>
        </div>
        "#;

        let entries = parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].authors,
            vec!["Frank Wang", "James Mickens", "Nickolai Zeldovich"]
        );
    }

    #[test]
    fn test_parse_paper_list_2014_excludes_workshops() {
        let html = r#"
        <div class="wp-block-group single-paper">
          <div class="wp-block-group inner-wrap"><div class="wp-block-group__inner-container">
            <h3>Conference Paper One</h3>
            <p>Alice Smith, Bob Jones and Carol Lee</p>
          </div></div>
          <p class="paper-link-abs"><a href="/ndss2014/ndss-2014-programme/paper-one/">Read More</a></p>
        </div>
        <div class="wp-block-group single-paper">
          <div class="wp-block-group inner-wrap"><div class="wp-block-group__inner-container">
            <h3>Conference Paper Two</h3>
            <p>David Chen and Erin Brown</p>
          </div></div>
          <p class="paper-link-abs"><a href="https://www.ndss-symposium.org/ndss2014/ndss-2014-programme/paper-two/">Read More</a></p>
        </div>
        <div class="wp-block-group single-paper">
          <div class="wp-block-group inner-wrap"><div class="wp-block-group__inner-container">
            <h3>USEC Workshop Paper</h3><p>Workshop Author</p>
          </div></div>
          <p class="paper-link-abs"><a href="/ndss2014/ndss-2014-usec-programme/usec-paper/">Read More</a></p>
        </div>
        <div class="wp-block-group single-paper">
          <div class="wp-block-group inner-wrap"><div class="wp-block-group__inner-container">
            <h3>SENT Workshop Paper</h3><p>Workshop Author</p>
          </div></div>
          <p class="paper-link-abs"><a href="/ndss2014/ndss-2014-sent-programme/sent-paper/">Read More</a></p>
        </div>
        "#;

        let entries = parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Conference Paper One");
        assert_eq!(entries[1].title, "Conference Paper Two");
        assert_eq!(
            entries[0].authors,
            vec!["Alice Smith", "Bob Jones", "Carol Lee"]
        );
        assert_eq!(
            entries[0].detail_url,
            "https://www.ndss-symposium.org/ndss2014/ndss-2014-programme/paper-one/"
        );
    }

    #[test]
    fn test_legacy_main_programme_urls_are_not_workshops() {
        let html = r#"
        <div class="single-paper"><div class="inner-wrap">
          <h3>1997 Main Conference Paper</h3>
          <p>Alice Example and Bob Example</p>
          <p class="paper-link-abs"><a href="https://www.ndss-symposium.org/ndss1997/paper-slug/">Read More</a></p>
        </div></div>
        <div class="single-paper"><div class="inner-wrap">
          <h3>2012 Main Conference Paper</h3>
          <p>Carol Example</p>
          <p class="paper-link-abs"><a href="https://www.ndss-symposium.org/ndss2012/ndss-2012-programme/another-paper/">Read More</a></p>
        </div></div>
        <div class="single-paper"><div class="inner-wrap">
          <h3>Workshop Paper</h3>
          <p>Mallory Example</p>
          <p class="paper-link-abs"><a href="https://www.ndss-symposium.org/ndss2012/usec-2012-programme/workshop-paper/">Read More</a></p>
        </div></div>
        "#;

        let entries = parse_paper_list(html, "https://www.ndss-symposium.org").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "1997 Main Conference Paper");
        assert_eq!(entries[1].title, "2012 Main Conference Paper");
    }

    #[test]
    fn test_parse_authors() {
        let authors = parse_authors("Alice Smith (MIT), Bob Jones (Stanford), Charlie Brown (CMU)");
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones", "Charlie Brown"]);
    }

    #[test]
    fn test_parse_authors_with_and() {
        let authors = parse_authors("Alice Smith (MIT) and Bob Jones (Stanford)");
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones"]);
    }

    #[test]
    fn test_parse_authors_empty() {
        let authors = parse_authors("");
        assert!(authors.is_empty());
    }
}
