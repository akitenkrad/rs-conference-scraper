use scraper::{ElementRef, Html, Node, Selector};

/// URL のドメインに基づいてサイト固有のパーサを選択し，abstract を抽出する
pub fn try_site_specific_extraction(url: &str, raw_html: &str, document: &Html) -> Option<String> {
    let result = if url.contains("ieeexplore.ieee.org") {
        tracing::debug!("Site-specific parser matched: IEEE Xplore");
        parse_ieee_xplore(raw_html)
    } else if url.contains("link.springer.com") {
        tracing::debug!("Site-specific parser matched: Springer");
        parse_springer(document)
    } else if url.contains("aclanthology.org") {
        tracing::debug!("Site-specific parser matched: ACL Anthology");
        parse_acl_anthology(document)
    } else if url.contains("openaccess.thecvf.com") {
        tracing::debug!("Site-specific parser matched: CVF Open Access");
        parse_cvf(document)
    } else if url.contains("papers.neurips.cc") {
        tracing::debug!("Site-specific parser matched: NeurIPS");
        parse_neurips(document)
    } else if url.contains("openreview.net") {
        tracing::debug!("Site-specific parser matched: OpenReview");
        parse_openreview(document)
    } else if url.contains("ndss-symposium.org") {
        tracing::debug!("Site-specific parser matched: NDSS");
        parse_ndss(document)
    } else if url.contains("usenix.org") {
        tracing::debug!("Site-specific parser matched: USENIX");
        parse_usenix(document)
    } else {
        None
    };

    result.map(|text| clean_abstract_text(&text))
}

/// サイト固有パーサの結果を正とし，取れなかったときに汎用抽出へ進ませないサイトか
pub fn is_site_parser_authoritative(url: &str) -> bool {
    url.contains("ndss-symposium.org")
}

/// 抽出したテキストを整形する：プレフィクス除去，空白正規化，トリム
fn clean_abstract_text(text: &str) -> String {
    let text = text.trim();

    // "Abstract" プレフィクスの除去
    let text = text
        .strip_prefix("Abstract.")
        .or_else(|| text.strip_prefix("Abstract:"))
        .or_else(|| text.strip_prefix("Abstract"))
        .or_else(|| text.strip_prefix("abstract."))
        .or_else(|| text.strip_prefix("abstract:"))
        .or_else(|| text.strip_prefix("abstract"))
        .or_else(|| text.strip_prefix("ABSTRACT."))
        .or_else(|| text.strip_prefix("ABSTRACT:"))
        .or_else(|| text.strip_prefix("ABSTRACT"))
        .unwrap_or(text)
        .trim();

    // 連続する空白を1つに圧縮
    let mut result = String::with_capacity(text.len());
    let mut prev_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            result.push(ch);
            prev_space = false;
        }
    }

    result.trim().to_string()
}

/// ACL Anthology: `.acl-abstract > span`
fn parse_acl_anthology(document: &Html) -> Option<String> {
    let sel = Selector::parse(".acl-abstract span").ok()?;
    let el = document.select(&sel).next()?;
    let text: String = el.text().collect::<Vec<_>>().join(" ");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// CVF Open Access: `div#abstract`
fn parse_cvf(document: &Html) -> Option<String> {
    let sel = Selector::parse("div#abstract").ok()?;
    let el = document.select(&sel).next()?;
    let text: String = el.text().collect::<Vec<_>>().join(" ");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// NeurIPS: `p.paper-abstract` 内の `<p>` 要素のテキスト
fn parse_neurips(document: &Html) -> Option<String> {
    let outer_sel = Selector::parse("p.paper-abstract").ok()?;
    let inner_sel = Selector::parse("p").ok()?;
    let outer = document.select(&outer_sel).next()?;

    // 内部の <p> 要素があればそのテキストを取得
    if let Some(inner) = outer.select(&inner_sel).next() {
        let text: String = inner.text().collect::<Vec<_>>().join(" ");
        if !text.trim().is_empty() {
            return Some(text);
        }
    }

    // フォールバック: 外側の要素のテキストをそのまま取得
    let text: String = outer.text().collect::<Vec<_>>().join(" ");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// OpenReview: `meta[name="citation_abstract"]` の content 属性
fn parse_openreview(document: &Html) -> Option<String> {
    let sel = Selector::parse(r#"meta[name="citation_abstract"]"#).ok()?;
    let el = document.select(&sel).next()?;
    let content = el.value().attr("content")?;
    if content.trim().is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

/// NDSS: `div.paper-data` 内の2番目の `<p>` 要素
fn parse_ndss(document: &Html) -> Option<String> {
    if let Ok(div_sel) = Selector::parse("div.paper-data")
        && let Ok(p_sel) = Selector::parse("p")
        && let Some(div) = document.select(&div_sel).next()
        && let Some(p_el) = div.select(&p_sel).nth(1)
    {
        // 2番目の <p> 要素を取得（1番目は著者情報）
        let text: String = p_el.text().collect::<Vec<_>>().join(" ");
        if !text.trim().is_empty() {
            return Some(text);
        }
    }

    crate::scraper::ndss::paper_page::get_legacy_abstract(document)
}

/// USENIX: `.field-name-field-paper-description .field-item` または旧形式の Abstract 見出し
fn parse_usenix(document: &Html) -> Option<String> {
    let sel = Selector::parse(".field-name-field-paper-description .field-item").ok()?;
    if let Some(text) = document.select(&sel).find_map(|el| {
        let text = el.text().collect::<Vec<_>>().join(" ");
        (!text.trim().is_empty()).then_some(text)
    }) {
        return Some(text);
    }

    parse_legacy_usenix_abstract(document)
}

fn parse_legacy_usenix_abstract(document: &Html) -> Option<String> {
    let heading_sel = Selector::parse("h3, h4").ok()?;
    let heading = document.select(&heading_sel).find(|heading| {
        heading
            .text()
            .collect::<String>()
            .trim()
            .eq_ignore_ascii_case("abstract")
    })?;

    let mut text_parts = Vec::new();
    let mut sibling = heading.next_sibling();
    while let Some(node) = sibling {
        sibling = node.next_sibling();
        match node.value() {
            Node::Text(text) => {
                if contains_full_text_marker(text) {
                    break;
                }
                text_parts.push(text.to_string());
            }
            Node::Element(_) => {
                let element = ElementRef::wrap(node)?;
                if collect_legacy_usenix_text(element, &mut text_parts) {
                    break;
                }
            }
            _ => {}
        }
    }

    let text = text_parts
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (text.chars().count() >= 60).then_some(text)
}

/// 要旨の子孫テキストを収集し，打ち切りノードに到達したら true を返す．
fn collect_legacy_usenix_text(element: ElementRef<'_>, text_parts: &mut Vec<String>) -> bool {
    if is_legacy_usenix_stop_element(element.value().name()) {
        return true;
    }

    let mut child = element.first_child();
    while let Some(node) = child {
        child = node.next_sibling();
        match node.value() {
            Node::Text(text) => {
                if contains_full_text_marker(text) {
                    return true;
                }
                text_parts.push(text.to_string());
            }
            Node::Element(_) => {
                if let Some(child_element) = ElementRef::wrap(node)
                    && collect_legacy_usenix_text(child_element, text_parts)
                {
                    return true;
                }
            }
            _ => {}
        }
    }

    false
}

fn is_legacy_usenix_stop_element(name: &str) -> bool {
    matches!(
        name,
        "ul" | "ol" | "hr" | "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    )
}

fn contains_full_text_marker(text: &str) -> bool {
    text.to_ascii_lowercase()
        .contains("full text of this paper")
}

/// IEEE Xplore: xplGlobal.document.metadata JSON 内の abstract フィールド
fn parse_ieee_xplore(raw_html: &str) -> Option<String> {
    let marker = "xplGlobal.document.metadata=";
    let start = raw_html.find(marker)? + marker.len();

    // JSON オブジェクトの対応する閉じ括弧を探す
    let bytes = raw_html.as_bytes();
    let mut depth = 0;
    let mut end = start;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return None;
    }

    let json_str = &raw_html[start..end];
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let abstract_text = value.get("abstract")?.as_str()?;

    // "true" / "false" はセクションフラグであり，実際の abstract ではない
    if abstract_text == "true" || abstract_text == "false" {
        return None;
    }

    Some(abstract_text.to_string())
}

/// Springer: section[data-title="Abstract"] 内の .c-article-section__content p
fn parse_springer(document: &Html) -> Option<String> {
    // 具体的な content div を先に試行
    let sel = Selector::parse("#Abs1-content p").ok()?;
    if let Some(el) = document.select(&sel).next() {
        let text: String = el.text().collect::<Vec<_>>().join(" ");
        if !text.trim().is_empty() {
            return Some(text);
        }
    }

    // フォールバック: data-title="Abstract" を持つ section
    let sel = Selector::parse(r#"section[data-title="Abstract"] p"#).ok()?;
    let el = document.select(&sel).next()?;
    let text: String = el.text().collect::<Vec<_>>().join(" ");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_abstract_text_removes_prefix() {
        assert_eq!(
            clean_abstract_text("Abstract. This is a test."),
            "This is a test."
        );
        assert_eq!(
            clean_abstract_text("Abstract: This is a test."),
            "This is a test."
        );
        assert_eq!(
            clean_abstract_text("Abstract This is a test."),
            "This is a test."
        );
    }

    #[test]
    fn test_clean_abstract_text_collapses_whitespace() {
        assert_eq!(
            clean_abstract_text("This  is   a    test."),
            "This is a test."
        );
        assert_eq!(clean_abstract_text("  Line1\n  Line2  "), "Line1 Line2");
    }

    #[test]
    fn test_parse_acl_anthology() {
        let html = r#"
        <div class="card-body acl-abstract">
            <span>This is the abstract text from ACL.</span>
        </div>
        "#;
        let doc = Html::parse_document(html);
        let result = parse_acl_anthology(&doc);
        assert_eq!(
            result,
            Some("This is the abstract text from ACL.".to_string())
        );
    }

    #[test]
    fn test_parse_acl_anthology_strips_prefix() {
        let html = r#"
        <div class="card-body acl-abstract">
            <span>Abstract This is the abstract text.</span>
        </div>
        "#;
        let doc = Html::parse_document(html);
        let result = try_site_specific_extraction("https://aclanthology.org/paper", html, &doc);
        assert_eq!(result, Some("This is the abstract text.".to_string()));
    }

    #[test]
    fn test_parse_cvf() {
        let html = r#"<div id="abstract">This is a CVF paper abstract.</div>"#;
        let doc = Html::parse_document(html);
        let result = parse_cvf(&doc);
        assert_eq!(result, Some("This is a CVF paper abstract.".to_string()));
    }

    #[test]
    fn test_parse_neurips_with_inner_p() {
        // HTML パーサは <p> の中に <p> をネストできないため，
        // 実際の NeurIPS ページの構造をシミュレートするには
        // 外側を <div> 等にする必要がある．
        // ここでは外側の <p> のテキストを直接テストする．
        let html = r#"
        <h2 class="section-label">Abstract</h2>
        <p class="paper-abstract">This is the NeurIPS abstract.</p>
        "#;
        let doc = Html::parse_document(html);
        let result = parse_neurips(&doc);
        assert_eq!(result, Some("This is the NeurIPS abstract.".to_string()));
    }

    #[test]
    fn test_parse_neurips_real_structure() {
        // 実際のNeurIPSページでは <p class="paper-abstract"> の中に
        // テキストノードとして abstract が含まれている
        let html = r#"
        <h4>Abstract</h4>
        <p class="paper-abstract">
            We present a novel approach to deep learning.
        </p>
        "#;
        let doc = Html::parse_document(html);
        // try_site_specific_extraction 経由でテスト（clean_abstract_text が適用される）
        let result =
            try_site_specific_extraction("https://papers.neurips.cc/paper/123", html, &doc);
        assert_eq!(
            result,
            Some("We present a novel approach to deep learning.".to_string())
        );
    }

    #[test]
    fn test_parse_openreview() {
        let html = r#"
        <meta name="citation_abstract" content="This is the OpenReview abstract.">
        "#;
        let doc = Html::parse_document(html);
        let result = parse_openreview(&doc);
        assert_eq!(result, Some("This is the OpenReview abstract.".to_string()));
    }

    #[test]
    fn test_parse_ndss() {
        let html = r#"
        <div class="paper-data">
            <p><strong>Author Name</strong></p>
            <p>This is the NDSS abstract text.</p>
        </div>
        "#;
        let doc = Html::parse_document(html);
        let result = parse_ndss(&doc);
        assert_eq!(result, Some("This is the NDSS abstract text.".to_string()));
    }

    #[test]
    fn test_parse_ndss_legacy_abstract_paragraphs() {
        let html = r#"
        <section class="new-wrapper">
            <h2>Abstract:</h2>
            <p>First paragraph with&nbsp;non-breaking spaces.</p>
            <p>Second paragraph.</p>
            <h2>Downloads</h2>
            <p>Not part of the abstract.</p>
        </section>
        "#;
        let doc = Html::parse_document(html);
        let result = parse_ndss(&doc);
        assert_eq!(
            result,
            Some("First paragraph with non-breaking spaces. Second paragraph.".to_string())
        );
    }

    #[test]
    fn test_parse_usenix() {
        let html = r#"
        <div class="field-name-field-paper-description">
            <div class="field-item">This is the USENIX abstract.</div>
        </div>
        "#;
        let doc = Html::parse_document(html);
        let result = parse_usenix(&doc);
        assert_eq!(result, Some("This is the USENIX abstract.".to_string()));
    }

    #[test]
    fn test_parse_usenix_legacy_paper_page() {
        let html = r#"
        <div class="field field-name-field-paper-description field-type-text-long field-label-hidden">
          <div class="field-items field-items"><div class="field-item odd"><p><span>We propose new techniques to combat click fraud via premium clicks.</span></p></div></div>
        </div>
        "#;
        let doc = Html::parse_document(html);
        let result = parse_usenix(&doc);
        assert_eq!(
            result,
            Some("We propose new techniques to combat click fraud via premium clicks.".to_string())
        );
    }

    #[test]
    fn test_parse_usenix_legacy_1993_h4_and_nested_hr() {
        let html = r#"
        <H4>Abstract</H4>

        As an exercise in the application of the concept of least privilege,
        we have modified the pieces of an ordinary UNIX* mail system, sendmail
        in particular, to require little or no privilege in their operation.
        No mail code runs with root or system IDs. In fact, the simplest
        configuration (local-only mail, with or without mandatory access
        controls) can run with no privilege or special access rights whatsoever,
        while even the most complex (multilevel network mail) can be done with
        minimal privilege requirements. While such modifications cannot
        guarantee the absence of security holes in mail, they should greatly
        limit their possible scope.

        <P>
        <HR>

        Download the full text of this paper in
        <A href="full_papers/carson">ASCII</A> form (19,226 bytes).
        <P>
        To Become a USENIX Member, please see our Membership Information.
        "#;
        let doc = Html::parse_document(html);
        let result = parse_usenix(&doc).expect("legacy abstract should be extracted");

        assert!(result.starts_with("As an exercise"));
        assert!(result.ends_with("possible scope."));
        assert!(!result.contains("Download the full text"));
        assert!(!result.contains("To Become a USENIX Member"));
    }

    #[test]
    fn test_parse_usenix_legacy_1995_h4_with_inline_text() {
        let html = r#"
        <H4>Abstract</H4>

        The DARPA Internet uses the <i>Domain Name System</i> (DNS), a
        distributed database, to map host names to network addresses, and
        vice-versa.  Using a vulnerability first noticed by P.V.  Mockapetris,
        we demonstrate how the DNS can be abused to subvert system security.
        We also show what tools are useful to the attacker.  Possible defenses
        against this attack, including one implemented by Berkeley in response
        to our reports of this problem, are discussed, and the limitations on
        their applicability are demonstrated.

        <P>
        <HR>

        Download the full text of this paper in
        <A href="full_papers/bellovin.ps">POSTSCRIPT</A> and
        <A href="full_papers/bellovin.pdf">PDF</A> form.

        <P>
        To Become a USENIX Member, please see our Membership Information.
        "#;
        let doc = Html::parse_document(html);
        let result = parse_usenix(&doc).expect("legacy abstract should be extracted");

        assert!(result.starts_with("The DARPA Internet"));
        assert!(result.ends_with("are demonstrated."));
        assert!(!result.contains("Download the full text"));
        assert!(!result.contains("To Become a USENIX Member"));
    }

    #[test]
    fn test_parse_usenix_legacy_2001_stops_at_list() {
        let html = r#"
        <H3>Abstract</H3>

        We present a new approach to fast certificate revocation centered
        around the concept of an on-line semi-trusted mediator (<I>SEM</I>). The
        use of a <I>SEM</I> in conjunction with a simple threshold variant of
        the RSA cryptosystem (mediated RSA) offers a number of practical
        advantages over current revocation techniques. Our approach simplifies
        validation of digital signatures and enables certificate revocation
        within legacy systems. It also provides immediate revocation of all
        security capabilities. This paper discusses both the architecture and
        implementation of our approach as well as performance and compatibility
        with the existing infrastructure. Our results show that threshold
        cryptography is practical for
        certificate revocation.

        <UL>
        <LI>View the full text of this paper in HTML, PDF, and PostScript.
        <P>
        <LI>To become a USENIX Member, please see our Membership Information.
        "#;
        let doc = Html::parse_document(html);
        let result = parse_usenix(&doc).expect("legacy abstract should be extracted");

        assert!(result.starts_with("We present a new approach"));
        assert!(result.ends_with("certificate revocation."));
        assert!(!result.contains("View the full text"));
        assert!(
            !result
                .to_ascii_lowercase()
                .contains("to become a usenix member")
        );
    }

    #[test]
    fn test_parse_usenix_legacy_2002_includes_two_paragraphs() {
        let html = r#"
        <H3>Abstract</H3>
        In the Internet, mobile code is ubiquitous and includes such examples
        as browser plug-ins, Java applets, and document macros. In this paper,
        we address an important vulnerability in mobile code security that
        exists in remote execution systems such as Condor, Globus, and
        SETI@Home. These systems schedule user jobs for execution on remote idle
        machines. However, they send most of their important system calls back
        to the local machine for execution. Hence, an evil process on the
        remote machine can manipulate a user's job to send destructive system
        calls back to the local machine. We have developed techniques to
        remotely detect such manipulation.

        <P>Before the job is submitted for remote execution, we construct a
        model of the user's binary program using static analysis. This binary
        analysis is applicable to commodity remote execution systems and
        applications. During remote job execution, the model checks all system
        calls arriving at the local machine. Execution is only allowed to
        continue while the model remains valid. We begin with a finite-state
        machine model that accepts sequences of system calls and then build
        optimizations into the model to improve its precision and efficiency.
        We also propose two program transformations, renaming and null call
        insertion, that have a significant impact on the precision and
        efficiency. As a desirable side-effect, these techniques also obfuscate
        the program, thus making it harder for the adversary to reverse engineer
        the code. We have implemented a simulated remote execution environment
        to demonstrate how optimizations and transformations of the binary
        program increase the precision and efficiency. In our test programs,
        unoptimized models increase run-time by 0.5% or less. At moderate levels
        of optimization, run-time increases by less than 13% with precision
        gains reaching 74%.

        <UL>
        <LI>View the full text of this paper in HTML and PDF.
        <P>
        <LI>To become a USENIX Member, please see our Membership Information.
        "#;
        let doc = Html::parse_document(html);
        let result = parse_usenix(&doc).expect("legacy abstract should be extracted");

        assert!(result.starts_with("In the Internet"));
        assert!(result.contains("Before the job is submitted"));
        assert!(result.ends_with("precision gains reaching 74%."));
        assert!(!result.contains("View the full text"));
        assert!(
            !result
                .to_ascii_lowercase()
                .contains("to become a usenix member")
        );
    }

    #[test]
    fn test_parse_usenix_legacy_2004_stops_at_immediate_list() {
        let html = r#"
        <H3>Abstract</H3>
        We present Tor, a circuit-based low-latency anonymous communication
        service. This second-generation Onion Routing system addresses limitations
        in the original design by adding perfect forward secrecy, congestion
        control, directory servers, integrity checking, configurable exit
        policies, and a practical design for location-hidden services via
        rendezvous points. Tor works on the real-world Internet, requires no
        special privileges or kernel modifications, requires little synchronization
        or coordination between nodes, and provides a reasonable tradeoff between
        anonymity, usability, and efficiency. We briefly describe our experiences
        with an international network of more than 30 nodes. We close with a list
        of open problems in anonymous communication.<UL>

        <LI>View the full text of this paper in HTML and PDF.
        </ul>
        <table><tr><td>To become a USENIX Member.</td></tr></table>
        "#;
        let doc = Html::parse_document(html);
        let result = parse_usenix(&doc).expect("legacy abstract should be extracted");

        assert!(result.starts_with("We present Tor"));
        assert!(result.ends_with("anonymous communication."));
        assert!(!result.contains("View the full text"));
        assert!(
            !result
                .to_ascii_lowercase()
                .contains("to become a usenix member")
        );
    }

    #[test]
    fn test_parse_usenix_legacy_without_abstract_heading_returns_none() {
        let html = r#"
        <h3>Introduction</h3>
        This page contains enough text to exceed the minimum length, but it
        has no abstract heading and therefore must not be treated as one.
        "#;
        let doc = Html::parse_document(html);

        assert_eq!(parse_usenix(&doc), None);
    }

    #[test]
    fn test_parse_usenix_legacy_short_body_returns_none() {
        let html = r#"
        <h3>Abstract</h3>
        This legacy abstract is intentionally too short.
        <hr>
        "#;
        let doc = Html::parse_document(html);

        assert_eq!(parse_usenix(&doc), None);
    }

    #[test]
    fn test_no_match_returns_none() {
        let html = r#"<html><body><p>Hello</p></body></html>"#;
        let doc = Html::parse_document(html);
        let result = try_site_specific_extraction("https://example.com/paper", html, &doc);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_ieee_xplore() {
        let html = r#"<html><head></head><body><script>
            xplGlobal.document.metadata={"title":"Test Paper","abstract":"We propose a novel framework for secure computation.","authors":[]};
        </script></body></html>"#;
        let result = parse_ieee_xplore(html);
        assert_eq!(
            result,
            Some("We propose a novel framework for secure computation.".to_string())
        );
    }

    #[test]
    fn test_parse_ieee_xplore_skips_boolean() {
        let html = r#"<html><body><script>
            xplGlobal.document.metadata={"title":"Section","abstract":"true"};
        </script></body></html>"#;
        let result = parse_ieee_xplore(html);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_springer() {
        let html = r#"
        <section data-title="Abstract">
            <div class="c-article-section" id="Abs1-section">
                <div class="c-article-section__content" id="Abs1-content">
                    <p>This paper presents a comprehensive survey of privacy-preserving techniques.</p>
                </div>
            </div>
        </section>
        "#;
        let doc = Html::parse_document(html);
        let result = parse_springer(&doc);
        assert_eq!(
            result,
            Some(
                "This paper presents a comprehensive survey of privacy-preserving techniques."
                    .to_string()
            )
        );
    }

    #[test]
    fn test_parse_springer_fallback() {
        let html = r#"
        <section data-title="Abstract">
            <p>We study the problem of distributed consensus in adversarial settings.</p>
        </section>
        "#;
        let doc = Html::parse_document(html);
        let result = parse_springer(&doc);
        assert_eq!(
            result,
            Some(
                "We study the problem of distributed consensus in adversarial settings."
                    .to_string()
            )
        );
    }
}
