use regex::Regex;

/// Extract the abstract text from an ACL Anthology individual paper page.
///
/// The abstract is embedded in a JavaScript variable:
/// ```javascript
/// const paper_params={...,abstract:"the abstract text here"};
/// ```
pub fn parse_abstract(html: &str) -> String {
    // Match abstract field in paper_params JavaScript object.
    // Handles: abstract:"", abstract:"text", abstract:"text with \"escapes\""
    let re = match Regex::new(r#"abstract:"((?:[^"\\]|\\.)*)""#) {
        Ok(r) => r,
        Err(_) => return String::new(),
    };

    match re.captures(html) {
        Some(caps) => {
            let raw = &caps[1];
            // Unescape common JavaScript escape sequences
            raw.replace("\\\"", "\"")
                .replace("\\'", "'")
                .replace("\\n", "\n")
                .replace("\\\\", "\\")
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_abstract_normal() {
        let html = r#"
        <script>
        const paper_params={anthology_id:"P05-1001",title:"Test",authors:[],abstract:"This is a test abstract."};
        </script>
        "#;
        assert_eq!(parse_abstract(html), "This is a test abstract.");
    }

    #[test]
    fn test_parse_abstract_empty() {
        let html = r#"
        <script>
        const paper_params={anthology_id:"P05-1001",title:"Test",authors:[],abstract:""};
        </script>
        "#;
        assert_eq!(parse_abstract(html), "");
    }

    #[test]
    fn test_parse_abstract_escaped_quotes() {
        let html = r#"
        <script>
        const paper_params={anthology_id:"P05-1001",title:"Test",authors:[],abstract:"some \"quoted\" text"};
        </script>
        "#;
        assert_eq!(parse_abstract(html), "some \"quoted\" text");
    }

    #[test]
    fn test_parse_abstract_absent() {
        let html = r#"
        <script>
        const paper_params={anthology_id:"P05-1001",title:"Test",authors:[]};
        </script>
        "#;
        assert_eq!(parse_abstract(html), "");
    }

    #[test]
    fn test_parse_abstract_no_script() {
        let html = "<html><body><p>No JavaScript here</p></body></html>";
        assert_eq!(parse_abstract(html), "");
    }

    #[test]
    fn test_parse_abstract_with_newlines() {
        let html = r#"
        <script>
        const paper_params={anthology_id:"X",title:"T",authors:[],abstract:"line one\nline two"};
        </script>
        "#;
        assert_eq!(parse_abstract(html), "line one\nline two");
    }
}
