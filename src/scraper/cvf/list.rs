use scraper::{Html, Selector};

use crate::types::PaperListEntry;

/// 論文一覧ページのHTMLをパースして PaperListEntry のリストを返す
pub fn parse_paper_list(html: &str, base_url: &str) -> Vec<PaperListEntry> {
    let document = Html::parse_document(html);
    let dt_selector = Selector::parse("dt.ptitle").unwrap();
    let a_selector = Selector::parse("a").unwrap();
    let author_input_selector = Selector::parse("input[name=\"query_author\"]").unwrap();
    let pdf_link_selector = Selector::parse("a[href$=\".pdf\"]").unwrap();

    let mut entries = Vec::new();

    // Collect all <dt> and <dd> elements in document order
    let dt_elements: Vec<_> = document.select(&dt_selector).collect();

    // For each dt.ptitle, find the following dd siblings
    // We use the raw tree structure: after each dt.ptitle there are dd elements
    for dt in &dt_elements {
        // Extract title and detail URL from <a> inside dt.ptitle
        let a_element = match dt.select(&a_selector).next() {
            Some(a) => a,
            None => continue,
        };

        let title = a_element
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }

        let detail_url = match a_element.value().attr("href") {
            Some(href) => {
                if href.starts_with("http") {
                    href.to_string()
                } else if href.starts_with('/') {
                    format!("{}{}", base_url, href)
                } else {
                    format!("{}/{}", base_url, href)
                }
            }
            None => continue,
        };

        // Find the following <dd> siblings by traversing next siblings of this <dt>
        let mut authors = Vec::new();
        let mut pdf_url = None;

        let mut sibling = dt.next_sibling();
        while let Some(node) = sibling {
            if let Some(element) = node.value().as_element() {
                if element.name() == "dt" {
                    // Reached the next <dt>, stop
                    break;
                }
                if element.name() == "dd" {
                    let dd_ref = scraper::ElementRef::wrap(node).unwrap();

                    // Extract authors from hidden inputs
                    for input in dd_ref.select(&author_input_selector) {
                        if let Some(author) = input.value().attr("value") {
                            let author = author.trim().to_string();
                            if !author.is_empty() {
                                authors.push(author);
                            }
                        }
                    }

                    // Extract PDF link
                    if pdf_url.is_none()
                        && let Some(pdf_a) = dd_ref.select(&pdf_link_selector).next()
                            && let Some(href) = pdf_a.value().attr("href") {
                                pdf_url = Some(if href.starts_with("http") {
                                    href.to_string()
                                } else if href.starts_with('/') {
                                    format!("{}{}", base_url, href)
                                } else {
                                    format!("{}/{}", base_url, href)
                                });
                            }
                }
            }
            sibling = node.next_sibling();
        }

        entries.push(PaperListEntry {
            title,
            authors,
            detail_url,
            track: Some("Conference".to_string()),
        });
    }

    entries
}

/// インデックスページから日別ページへのリンクを抽出する
/// (例: CVPR2018.py?day=2018-06-19)
pub fn parse_day_links(html: &str, base_url: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    let a_selector = Selector::parse("a").unwrap();

    let mut day_urls = Vec::new();
    for a in document.select(&a_selector) {
        if let Some(href) = a.value().attr("href")
            && href.contains("?day=") && href.contains(".py") {
                let url = if href.starts_with("http") {
                    href.to_string()
                } else if href.starts_with('/') {
                    format!("{}{}", base_url, href)
                } else {
                    format!("{}/{}", base_url, href)
                };
                if !day_urls.contains(&url) {
                    day_urls.push(url);
                }
            }
    }
    day_urls
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_HTML: &str = r##"
    <html><body>
    <dl>
      <dt class="ptitle">
        <a href="/content/CVPR2024/html/Smith_Deep_Learning_CVPR_2024_paper.html">Deep Learning for Vision</a>
      </dt>
      <dd>
        <form class="authsearch">
          <input type="hidden" name="query_author" value="John Smith">
          <a href="#">John Smith</a>
        </form>
        <form class="authsearch">
          <input type="hidden" name="query_author" value="Jane Doe">
          <a href="#">Jane Doe</a>
        </form>
      </dd>
      <dd>
        [<a href="/content/CVPR2024/papers/Smith_Deep_Learning_CVPR_2024_paper.pdf">pdf</a>]
        [<a href="http://arxiv.org/abs/2401.12345">arXiv</a>]
      </dd>

      <dt class="ptitle">
        <a href="/content/CVPR2024/html/Lee_Transformers_CVPR_2024_paper.html">Transformers in Computer Vision</a>
      </dt>
      <dd>
        <form class="authsearch">
          <input type="hidden" name="query_author" value="Alice Lee">
          <a href="#">Alice Lee</a>
        </form>
      </dd>
      <dd>
        [<a href="/content/CVPR2024/papers/Lee_Transformers_CVPR_2024_paper.pdf">pdf</a>]
      </dd>
    </dl>
    </body></html>
    "##;

    #[test]
    fn test_parse_multiple_papers() {
        let entries = parse_paper_list(MOCK_HTML, "https://openaccess.thecvf.com");
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].title, "Deep Learning for Vision");
        assert_eq!(entries[1].title, "Transformers in Computer Vision");
    }

    #[test]
    fn test_author_extraction() {
        let entries = parse_paper_list(MOCK_HTML, "https://openaccess.thecvf.com");
        assert_eq!(entries[0].authors, vec!["John Smith", "Jane Doe"]);
        assert_eq!(entries[1].authors, vec!["Alice Lee"]);
    }

    #[test]
    fn test_pdf_link_extraction() {
        let entries = parse_paper_list(MOCK_HTML, "https://openaccess.thecvf.com");

        // PDF URLs are not stored in PaperListEntry (no field for it)
        // but detail_url should be absolute
        assert_eq!(
            entries[0].detail_url,
            "https://openaccess.thecvf.com/content/CVPR2024/html/Smith_Deep_Learning_CVPR_2024_paper.html"
        );
        assert_eq!(
            entries[1].detail_url,
            "https://openaccess.thecvf.com/content/CVPR2024/html/Lee_Transformers_CVPR_2024_paper.html"
        );
    }

    #[test]
    fn test_track_defaults_to_conference() {
        let entries = parse_paper_list(MOCK_HTML, "https://openaccess.thecvf.com");
        for entry in &entries {
            assert_eq!(entry.track, Some("Conference".to_string()));
        }
    }

    #[test]
    fn test_absolute_url_passthrough() {
        let html = r##"
        <html><body>
        <dl>
          <dt class="ptitle">
            <a href="https://example.com/paper.html">Absolute URL Paper</a>
          </dt>
          <dd>
            <form class="authsearch">
              <input type="hidden" name="query_author" value="Author">
              <a href="#">Author</a>
            </form>
          </dd>
        </dl>
        </body></html>
        "##;
        let entries = parse_paper_list(html, "https://openaccess.thecvf.com");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].detail_url, "https://example.com/paper.html");
    }

    #[test]
    fn test_empty_html() {
        let entries = parse_paper_list("<html><body></body></html>", "https://openaccess.thecvf.com");
        assert!(entries.is_empty());
    }
}
