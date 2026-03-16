use anyhow::Result;
use scraper::{Html, Selector};

use crate::types::PaperListEntry;

const BASE_URL: &str = "https://aclanthology.org";

/// Parse an ACL Anthology event page into a list of paper entries.
pub fn parse_event_page(html: &str, _conference: &str, _year: u16) -> Result<Vec<PaperListEntry>> {
    let document = Html::parse_document(html);

    let title_selector = Selector::parse("strong > a.align-middle").unwrap();
    let author_selector = Selector::parse("span.d-block > a[href*=\"/people/\"]").unwrap();
    let volume_header_selector = Selector::parse("h4").unwrap();
    let volume_section_selector = Selector::parse("div[id]").unwrap();

    // Build a map of volume section element positions to their track names.
    // We collect all volume sections with their h4 text.
    let mut volume_tracks: Vec<(String, String)> = Vec::new(); // (section_id, track_name)
    for section in document.select(&volume_section_selector) {
        if let Some(id) = section.value().attr("id")
            && let Some(h4) = section.select(&volume_header_selector).next() {
                let track_name = h4.text().collect::<Vec<_>>().join(" ").trim().to_string();
                if !track_name.is_empty() {
                    volume_tracks.push((id.to_string(), track_name));
                }
            }
    }

    let mut entries = Vec::new();
    let mut current_track: Option<String> = None;

    // Walk through the document tree to find paper entries and track volume sections.
    // We iterate over all elements in document order.
    let all_selector = Selector::parse("div[id], div.d-sm-flex.align-items-stretch.mb-3").unwrap();

    for element in document.select(&all_selector) {
        // Check if this is a volume section div
        if let Some(id) = element.value().attr("id") {
            // Update current track from our pre-built map
            for (section_id, track_name) in &volume_tracks {
                if section_id == id {
                    current_track = Some(track_name.clone());
                    break;
                }
            }
            // If this element is also a paper entry div, continue processing below;
            // otherwise skip.
            if !element.value().classes().any(|c| c == "d-sm-flex") {
                continue;
            }
        }

        // This should be a paper entry div
        if !element.value().classes().any(|c| c == "mb-3") {
            continue;
        }

        // Extract title and detail URL from strong > a
        let title_el = match element.select(&title_selector).next() {
            Some(el) => el,
            None => continue,
        };

        let href = match title_el.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        // Skip volume/proceedings entries (URL contains /volumes/ or ID ends in 000)
        if href.contains("/volumes/") {
            continue;
        }

        // Check if the paper ID ends in "000" pattern (frontmatter)
        // e.g., /P05-1000/ or /2024.acl-long.0/
        let path_trimmed = href.trim_end_matches('/');
        if let Some(last_segment) = path_trimmed.rsplit('/').next() {
            // For old-style IDs like P05-1000
            if last_segment.ends_with("000") || last_segment.ends_with("-0") {
                continue;
            }
            // For new-style IDs like 2024.acl-long.0
            if last_segment.ends_with(".0") {
                continue;
            }
        }

        let title = title_el.text().collect::<Vec<_>>().join("").trim().to_string();
        if title.is_empty() {
            continue;
        }

        // Skip entries whose title starts with "Proceedings of"
        if title.starts_with("Proceedings of") {
            continue;
        }

        let detail_url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("{}{}", BASE_URL, href)
        };

        // Extract authors
        let authors: Vec<String> = element
            .select(&author_selector)
            .map(|a| a.text().collect::<Vec<_>>().join("").trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();

        entries.push(PaperListEntry {
            title,
            authors,
            detail_url,
            track: current_track.clone(),
        });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_event_page_basic() {
        let html = r#"
        <html><body>
        <div id="p05-1">
            <h4>Proceedings of the 43rd Annual Meeting of the ACL</h4>
            <div class="d-sm-flex align-items-stretch mb-3">
                <div class="d-block me-2 list-button-row">
                    <span class="d-inline d-sm-block text-nowrap">
                        <a class="badge text-bg-primary align-middle me-1" href="https://aclanthology.org/P05-1000.pdf">pdf</a>
                    </span>
                </div>
                <span class="d-block">
                    <strong><a class="align-middle" href="/volumes/P05-1/">Proceedings of the 43rd Annual Meeting</a></strong>
                </span>
            </div>
            <div class="d-sm-flex align-items-stretch mb-3">
                <div class="d-block me-2 list-button-row">
                    <span class="d-inline d-sm-block text-nowrap">
                        <a class="badge text-bg-primary align-middle me-1" href="https://aclanthology.org/P05-1001.pdf">pdf</a>
                    </span>
                </div>
                <span class="d-block">
                    <strong><a class="align-middle" href="/P05-1001/">Paper Title Here</a></strong>
                    <br>
                    <a href="/people/author-id/">Alice Smith</a>
                    <a href="/people/other-id/">Bob Jones</a>
                </span>
            </div>
            <div class="d-sm-flex align-items-stretch mb-3">
                <div class="d-block me-2 list-button-row">
                    <span class="d-inline d-sm-block text-nowrap">
                        <a class="badge text-bg-primary align-middle me-1" href="https://aclanthology.org/P05-1002.pdf">pdf</a>
                    </span>
                </div>
                <span class="d-block">
                    <strong><a class="align-middle" href="/P05-1002/">Another Paper</a></strong>
                    <br>
                    <a href="/people/carol-id/">Carol Williams</a>
                </span>
            </div>
        </div>
        </body></html>
        "#;

        let entries = parse_event_page(html, "acl", 2005).unwrap();
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].title, "Paper Title Here");
        assert_eq!(entries[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(
            entries[0].detail_url,
            "https://aclanthology.org/P05-1001/"
        );
        assert_eq!(
            entries[0].track,
            Some("Proceedings of the 43rd Annual Meeting of the ACL".to_string())
        );

        assert_eq!(entries[1].title, "Another Paper");
        assert_eq!(entries[1].authors, vec!["Carol Williams"]);
    }

    #[test]
    fn test_parse_event_page_skips_proceedings_title() {
        let html = r#"
        <html><body>
        <div id="sec1">
            <h4>Main Track</h4>
            <div class="d-sm-flex align-items-stretch mb-3">
                <span class="d-block">
                    <strong><a class="align-middle" href="/X-1000/">Proceedings of Something</a></strong>
                </span>
            </div>
            <div class="d-sm-flex align-items-stretch mb-3">
                <span class="d-block">
                    <strong><a class="align-middle" href="/X-1001/">Real Paper</a></strong>
                    <br>
                    <a href="/people/a/">Author A</a>
                </span>
            </div>
        </div>
        </body></html>
        "#;

        let entries = parse_event_page(html, "acl", 2020).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Real Paper");
    }

    #[test]
    fn test_parse_event_page_multiple_tracks() {
        let html = r#"
        <html><body>
        <div id="track1">
            <h4>Long Papers</h4>
            <div class="d-sm-flex align-items-stretch mb-3">
                <span class="d-block">
                    <strong><a class="align-middle" href="/2024.acl-long.1/">Long Paper 1</a></strong>
                    <br>
                    <a href="/people/a/">Author A</a>
                </span>
            </div>
        </div>
        <div id="track2">
            <h4>Short Papers</h4>
            <div class="d-sm-flex align-items-stretch mb-3">
                <span class="d-block">
                    <strong><a class="align-middle" href="/2024.acl-short.1/">Short Paper 1</a></strong>
                    <br>
                    <a href="/people/b/">Author B</a>
                </span>
            </div>
        </div>
        </body></html>
        "#;

        let entries = parse_event_page(html, "acl", 2024).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].track, Some("Long Papers".to_string()));
        assert_eq!(entries[1].track, Some("Short Papers".to_string()));
    }

    #[test]
    fn test_parse_event_page_empty() {
        let html = "<html><body><p>No papers here</p></body></html>";
        let entries = parse_event_page(html, "acl", 2020).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_event_page_skips_volumes_link() {
        let html = r#"
        <html><body>
        <div id="sec1">
            <h4>Track</h4>
            <div class="d-sm-flex align-items-stretch mb-3">
                <span class="d-block">
                    <strong><a class="align-middle" href="/volumes/P05-1/">Volume Entry</a></strong>
                </span>
            </div>
        </div>
        </body></html>
        "#;

        let entries = parse_event_page(html, "acl", 2005).unwrap();
        assert!(entries.is_empty());
    }
}
