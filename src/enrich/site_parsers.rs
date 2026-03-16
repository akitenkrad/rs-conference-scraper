use scraper::{Html, Selector};

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
    let div_sel = Selector::parse("div.paper-data").ok()?;
    let p_sel = Selector::parse("p").ok()?;
    let div = document.select(&div_sel).next()?;

    // 2番目の <p> 要素を取得（1番目は著者情報）
    let p_el = div.select(&p_sel).nth(1)?;
    let text: String = p_el.text().collect::<Vec<_>>().join(" ");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// USENIX: `.field-name-field-paper-description .field-item`
fn parse_usenix(document: &Html) -> Option<String> {
    let sel =
        Selector::parse(".field-name-field-paper-description .field-item").ok()?;
    let el = document.select(&sel).next()?;
    let text: String = el.text().collect::<Vec<_>>().join(" ");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
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
        assert_eq!(
            clean_abstract_text("  Line1\n  Line2  "),
            "Line1 Line2"
        );
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
        assert_eq!(result, Some("This is the abstract text from ACL.".to_string()));
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
        let result = try_site_specific_extraction("https://papers.neurips.cc/paper/123", html, &doc);
        assert_eq!(result, Some("We present a novel approach to deep learning.".to_string()));
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
            Some("This paper presents a comprehensive survey of privacy-preserving techniques.".to_string())
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
            Some("We study the problem of distributed consensus in adversarial settings.".to_string())
        );
    }
}
