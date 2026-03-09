use anyhow::Result;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::types::{compute_id, Paper, PaperListEntry};

/// Fetch and parse the AAMAS table of contents for a given year.
/// Returns a list of paper entries and their full Paper objects.
pub async fn fetch_paper_list(
    client: &reqwest::Client,
    base_url: &str,
    year: u16,
    interval: Duration,
) -> Result<(Vec<PaperListEntry>, Vec<Paper>)> {
    let url = format!(
        "{}/Proceedings/aamas{}/forms/contents.htm",
        base_url, year
    );
    let body = crate::scraper::fetch_with_sleep(client, &url, interval).await?;
    let proceedings_base = format!("{}/Proceedings/aamas{}", base_url, year);
    parse_contents(&body, &proceedings_base, year)
}

/// Parse the HTML contents page and extract paper entries.
///
/// The HTML structure is:
/// - Section headings in `<strong>` tags (e.g., "Full Research Papers")
/// - Paper entries as `<p>` tags containing:
///   - An `<a>` link to the PDF with the paper title as text
///   - Author names after `<br>` tags, with affiliations in `<i>` tags
///   - PDF href is relative like `../pdfs/p4.pdf` or `../docs/p1.pdf`
pub fn parse_contents(
    html: &str,
    proceedings_base: &str,
    year: u16,
) -> Result<(Vec<PaperListEntry>, Vec<Paper>)> {
    let document = Html::parse_document(html);
    let p_selector =
        Selector::parse("p").expect("valid selector");
    let a_selector =
        Selector::parse("a").expect("valid selector");
    let strong_selector =
        Selector::parse("strong").expect("valid selector");
    let i_selector =
        Selector::parse("i").expect("valid selector");

    let mut entries = Vec::new();
    let mut papers = Vec::new();
    let mut current_track: Option<String> = None;

    for p_elem in document.select(&p_selector) {
        // Check if this <p> contains a <strong> (section heading)
        if let Some(strong) = p_elem.select(&strong_selector).next() {
            let heading_text = strong.text().collect::<String>().trim().to_string();
            if !heading_text.is_empty() {
                current_track = Some(heading_text);
            }
            continue;
        }

        // Look for a paper link (an <a> linking to a PDF like ../pdfs/pXXX.pdf or ../docs/pXXX.pdf)
        let link = match p_elem.select(&a_selector).next() {
            Some(a) => a,
            None => continue,
        };

        let href = match link.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        // Only process links that point to paper PDFs (p{number}.pdf pattern)
        if !is_paper_pdf(href) {
            continue;
        }

        // Extract title from the link text, cleaning up whitespace
        let title = clean_text(&link.text().collect::<String>());
        if title.is_empty() {
            continue;
        }

        // Resolve relative PDF URL to absolute
        let pdf_url = resolve_pdf_url(href, proceedings_base);

        // Extract authors from the <p> element
        // Authors appear after <br> tags, with affiliations in <i> tags
        let authors = extract_authors(&p_elem, &i_selector);

        let track = current_track.clone();

        let entry = PaperListEntry {
            title: title.clone(),
            authors: authors.clone(),
            detail_url: pdf_url.clone(),
            track: track.clone(),
        };

        // Build full Paper object (abstract is empty - not available on the site)
        let id = compute_id(&title);
        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(title.as_bytes());
            hasher.update(b"");
            format!("{:x}", hasher.finalize())
        };

        let paper = Paper {
            id,
            conference: "aamas".to_string(),
            year,
            title,
            authors,
            r#abstract: String::new(),
            url: pdf_url.clone(),
            pdf_url: Some(pdf_url),
            categories: track.into_iter().collect(),
            hash,
        };

        entries.push(entry);
        papers.push(paper);
    }

    Ok((entries, papers))
}

/// Check if a href points to a paper PDF (p{number}.pdf pattern).
fn is_paper_pdf(href: &str) -> bool {
    // Match patterns like ../pdfs/p4.pdf, ../docs/p1.pdf, ../aamas/p1.pdf
    let filename = href.rsplit('/').next().unwrap_or("");
    filename.starts_with('p')
        && filename.ends_with(".pdf")
        && filename[1..filename.len() - 4]
            .chars()
            .all(|c| c.is_ascii_digit())
}

/// Resolve a relative PDF href to an absolute URL.
fn resolve_pdf_url(href: &str, proceedings_base: &str) -> String {
    if href.starts_with("http") {
        return href.to_string();
    }
    // href is like "../pdfs/p4.pdf" — relative to the forms/ directory
    // proceedings_base is like "https://www.ifaamas.org/Proceedings/aamas2024"
    // So ../pdfs/p4.pdf -> {proceedings_base}/pdfs/p4.pdf
    let stripped = href.strip_prefix("../").unwrap_or(href);
    format!("{}/{}", proceedings_base, stripped)
}

/// Extract author names from a <p> element.
/// Authors appear as text nodes after <br> tags, with affiliations in <i> tags.
/// Format: "AuthorName (Affiliation)" — we want only the name part.
fn extract_authors(
    p_elem: &scraper::ElementRef,
    _i_selector: &Selector,
) -> Vec<String> {
    let inner_html = p_elem.inner_html();
    let mut authors = Vec::new();

    // Split by <br> to get individual lines
    for line in inner_html.split("<br>").skip(1) {
        // Skip lines that are empty or contain only whitespace/tags
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse this fragment to extract text before <i> (affiliation)
        let fragment = Html::parse_fragment(line);

        // Get all text, but stop at the <i> tag (affiliation)
        let mut name_parts = Vec::new();
        for node in fragment.root_element().children() {
            match node.value() {
                scraper::node::Node::Text(text) => {
                    let t = text.trim();
                    if !t.is_empty() {
                        name_parts.push(t.to_string());
                    }
                }
                scraper::node::Node::Element(_) => {
                    // Check if this is an <i> tag (affiliation) - stop collecting name
                    break;
                }
                _ => {}
            }
        }

        let name = clean_text(&name_parts.join(" "));
        if !name.is_empty() && !name.starts_with('(') {
            authors.push(name);
        }
    }

    authors
}

/// Clean up whitespace in text: collapse runs of whitespace to single spaces, trim.
fn clean_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_html(body_content: &str) -> String {
        format!(
            r#"<HTML><HEAD><TITLE>Test</TITLE></HEAD><BODY>
            <table><tr><td>{}</td></tr></table>
            </BODY></HTML>"#,
            body_content
        )
    }

    #[test]
    fn test_parse_single_paper() {
        let html = make_html(
            r#"
            <p align="left"><strong>Full Research Papers</strong></p>
            <p align="left"><a href="../pdfs/p4.pdf">Team Performance in Mixed Human-Agent Teams</a>&nbsp;<font size="1">(Page 4)</font><font size="2"><br>
            Sami Abuhaimed<i> (The University of Tulsa)</i><br>
            Sandip Sen<i> (The University of Tulsa)</i></font></p>
            "#,
        );

        let (entries, papers) =
            parse_contents(&html, "https://www.ifaamas.org/Proceedings/aamas2024", 2024)
                .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Team Performance in Mixed Human-Agent Teams");
        assert_eq!(
            entries[0].authors,
            vec!["Sami Abuhaimed", "Sandip Sen"]
        );
        assert_eq!(
            entries[0].detail_url,
            "https://www.ifaamas.org/Proceedings/aamas2024/pdfs/p4.pdf"
        );
        assert_eq!(
            entries[0].track,
            Some("Full Research Papers".to_string())
        );

        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].conference, "aamas");
        assert_eq!(papers[0].year, 2024);
        assert!(papers[0].r#abstract.is_empty());
        assert_eq!(papers[0].id, compute_id("Team Performance in Mixed Human-Agent Teams"));
    }

    #[test]
    fn test_parse_multiple_tracks() {
        let html = make_html(
            r#"
            <p align="left"><strong>Keynote Talks</strong></p>
            <p align="left"><a href="../pdfs/p1.pdf">Trustworthy RL</a>&nbsp;<font size="1">(Page 1)</font><font size="2"><br>
            Ann Nowe<i> (VUB)</i></font></p>

            <p align="left"><strong>Full Research Papers</strong></p>
            <p align="left"><a href="../pdfs/p4.pdf">Paper One</a>&nbsp;<font size="1">(Page 4)</font><font size="2"><br>
            Alice Smith<i> (MIT)</i></font></p>
            <p align="left"><a href="../pdfs/p13.pdf">Paper Two</a>&nbsp;<font size="1">(Page 13)</font><font size="2"><br>
            Bob Jones<i> (Stanford)</i><br>
            Carol White<i> (CMU)</i></font></p>

            <p align="left"><strong>Extended Abstracts</strong></p>
            <p align="left"><a href="../pdfs/p2111.pdf">Abstract One</a>&nbsp;<font size="1">(Page 2111)</font><font size="2"><br>
            Dave Brown<i> (Berkeley)</i></font></p>
            "#,
        );

        let (entries, _papers) =
            parse_contents(&html, "https://www.ifaamas.org/Proceedings/aamas2024", 2024)
                .unwrap();

        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0].track, Some("Keynote Talks".to_string()));
        assert_eq!(entries[1].track, Some("Full Research Papers".to_string()));
        assert_eq!(entries[2].track, Some("Full Research Papers".to_string()));
        assert_eq!(entries[3].track, Some("Extended Abstracts".to_string()));

        assert_eq!(entries[2].authors, vec!["Bob Jones", "Carol White"]);
    }

    #[test]
    fn test_parse_docs_directory() {
        // 2013 uses ../docs/ instead of ../pdfs/
        let html = make_html(
            r#"
            <p align="left"><strong>Session: A1 - Robotics I</strong></p>
            <p align="left"><a href="../docs/p3.pdf">A Robot Paper</a>&nbsp;<font size="1">(Page 3)</font><font size="2"><br>
            John Doe<i> (MIT)</i></font></p>
            "#,
        );

        let (entries, _) =
            parse_contents(&html, "https://www.ifaamas.org/Proceedings/aamas2013", 2013)
                .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].detail_url,
            "https://www.ifaamas.org/Proceedings/aamas2013/docs/p3.pdf"
        );
    }

    #[test]
    fn test_skip_non_paper_pdfs() {
        let html = make_html(
            r#"
            <p align="left"><a href="../pdfs/welcome.pdf">AAMAS Chairs Welcome</a></p>
            <p align="left"><a href="../pdfs/organization.pdf">Conference Organization</a></p>
            <p align="left"><a href="sponsors.htm">Sponsors</a></p>
            <p align="left"><strong>Full Research Papers</strong></p>
            <p align="left"><a href="../pdfs/p4.pdf">A Real Paper</a>&nbsp;<font size="1">(Page 4)</font><font size="2"><br>
            Author One<i> (Univ)</i></font></p>
            "#,
        );

        let (entries, _) =
            parse_contents(&html, "https://www.ifaamas.org/Proceedings/aamas2024", 2024)
                .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "A Real Paper");
    }

    #[test]
    fn test_is_paper_pdf() {
        assert!(is_paper_pdf("../pdfs/p4.pdf"));
        assert!(is_paper_pdf("../docs/p123.pdf"));
        assert!(is_paper_pdf("../aamas/p1.pdf"));
        assert!(!is_paper_pdf("../pdfs/welcome.pdf"));
        assert!(!is_paper_pdf("../pdfs/organization.pdf"));
        assert!(!is_paper_pdf("sponsors.htm"));
        assert!(!is_paper_pdf("../pdfs/pc.pdf"));
    }

    #[test]
    fn test_clean_text() {
        assert_eq!(clean_text("  hello   world  "), "hello world");
        assert_eq!(clean_text("single"), "single");
        assert_eq!(clean_text("  "), "");
    }

    #[test]
    fn test_paper_hash_uses_empty_abstract() {
        let html = make_html(
            r#"
            <p align="left"><strong>Papers</strong></p>
            <p align="left"><a href="../pdfs/p1.pdf">Test Title</a><font size="2"><br>
            Author<i> (Univ)</i></font></p>
            "#,
        );

        let (_, papers) =
            parse_contents(&html, "https://www.ifaamas.org/Proceedings/aamas2024", 2024)
                .unwrap();

        assert_eq!(papers.len(), 1);
        // Hash should be SHA256(title + "")
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"Test Title");
        hasher.update(b"");
        let expected_hash = format!("{:x}", hasher.finalize());
        assert_eq!(papers[0].hash, expected_hash);
    }
}
