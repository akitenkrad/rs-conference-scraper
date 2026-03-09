use anyhow::Result;
use scraper::{Html, Selector};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::PaperListEntry;

pub async fn fetch_paper_list(
    client: &reqwest::Client,
    base_url: &str,
    volume: u16,
    interval: Duration,
) -> Result<Vec<PaperListEntry>> {
    let url = format!("{}/v{}/", base_url, volume);
    let html = fetch_with_sleep(client, &url, interval).await?;
    parse_paper_list(&html, base_url)
}

fn parse_paper_list(html: &str, base_url: &str) -> Result<Vec<PaperListEntry>> {
    let document = Html::parse_document(html);
    let paper_selector = Selector::parse("div.paper").unwrap();
    let title_link_selector = Selector::parse("p.title a").unwrap();
    let authors_selector = Selector::parse("span.authors").unwrap();
    let links_selector = Selector::parse("p.links a").unwrap();

    let mut entries = Vec::new();

    for paper_el in document.select(&paper_selector) {
        // Extract title and detail URL
        let title_link = match paper_el.select(&title_link_selector).next() {
            Some(el) => el,
            None => continue,
        };

        let title = title_link
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }

        let href = match title_link.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        let detail_url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("{}{}", base_url, href)
        };

        // Extract authors
        let authors: Vec<String> = paper_el
            .select(&authors_selector)
            .next()
            .map(|el| {
                el.text()
                    .collect::<Vec<_>>()
                    .join("")
                    .split(',')
                    .map(|a| a.trim().to_string())
                    .filter(|a| !a.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        // Extract PDF URL from links
        let pdf_url = paper_el
            .select(&links_selector)
            .find(|el| {
                el.text()
                    .collect::<Vec<_>>()
                    .join("")
                    .contains("PDF")
            })
            .and_then(|el| el.value().attr("href"))
            .map(|url| url.to_string());

        // Store PDF URL in track field temporarily (will be extracted in detail fetch)
        // Actually, we store it in PaperListEntry. But PaperListEntry doesn't have pdf_url.
        // We'll use track to pass the pdf_url since ICML track is always "Conference".
        // Better approach: just use track for "Conference" and re-extract PDF from detail page
        // or pass pdf_url through. Since PaperListEntry doesn't have pdf_url, we store it
        // as part of detail_url with a separator, or we just re-fetch it from the detail page.
        //
        // Simplest: store PDF URL in the track field with a prefix, then parse it out.
        // Actually the cleanest approach: just set track to the pdf_url so we can use it later.
        // The detail fetcher will set the actual track.
        //
        // Let's just use track = pdf_url and handle it in mod.rs
        let _ = pdf_url; // We'll re-extract from detail page; PaperListEntry has no pdf_url field

        entries.push(PaperListEntry {
            title,
            authors,
            detail_url,
            track: Some("Conference".to_string()),
        });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_paper_list_basic() {
        let html = r#"
        <html><body>
        <div class="paper">
            <p class="title">
                <a href="/v235/paper-id24a.html">Deep Learning for Everything</a>
            </p>
            <p class="details">
                <span class="authors">Alice Smith, Bob Jones, Charlie Brown</span>
                <br>
                <span class="info"><i>Proceedings of ICML</i>, PMLR 235:1-30, 2024.</span>
            </p>
            <p class="links">
                [<a href="/v235/paper-id24a.html">abs</a>]
                [<a href="https://raw.githubusercontent.com/mlresearch/v235/main/assets/paper-id24a/paper-id24a.pdf">Download PDF</a>]
            </p>
        </div>
        <div class="paper">
            <p class="title">
                <a href="/v235/another-paper24a.html">Transformers Revisited</a>
            </p>
            <p class="details">
                <span class="authors">David Lee</span>
            </p>
            <p class="links">
                [<a href="/v235/another-paper24a.html">abs</a>]
            </p>
        </div>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://proceedings.mlr.press").unwrap();
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].title, "Deep Learning for Everything");
        assert_eq!(
            entries[0].authors,
            vec!["Alice Smith", "Bob Jones", "Charlie Brown"]
        );
        assert_eq!(
            entries[0].detail_url,
            "https://proceedings.mlr.press/v235/paper-id24a.html"
        );
        assert_eq!(entries[0].track, Some("Conference".to_string()));

        assert_eq!(entries[1].title, "Transformers Revisited");
        assert_eq!(entries[1].authors, vec!["David Lee"]);
    }

    #[test]
    fn test_parse_paper_list_empty() {
        let html = r#"<html><body><p>No papers here</p></body></html>"#;
        let entries = parse_paper_list(html, "https://proceedings.mlr.press").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_paper_list_absolute_urls() {
        let html = r#"
        <html><body>
        <div class="paper">
            <p class="title">
                <a href="https://proceedings.mlr.press/v202/some-paper23a.html">A Paper</a>
            </p>
            <p class="details">
                <span class="authors">Author One</span>
            </p>
        </div>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://proceedings.mlr.press").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].detail_url,
            "https://proceedings.mlr.press/v202/some-paper23a.html"
        );
    }

    #[test]
    fn test_parse_paper_list_skips_empty_titles() {
        let html = r#"
        <html><body>
        <div class="paper">
            <p class="title"><a href="/v235/foo.html">  </a></p>
        </div>
        <div class="paper">
            <p class="title"><a href="/v235/bar.html">Valid Title</a></p>
            <p class="details"><span class="authors">Auth</span></p>
        </div>
        </body></html>
        "#;

        let entries = parse_paper_list(html, "https://proceedings.mlr.press").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Valid Title");
    }
}
