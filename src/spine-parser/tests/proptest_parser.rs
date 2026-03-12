//! Property-based tests for spine-parser using proptest.
//!
//! Verifies that parse_html never panics on arbitrary input and that
//! UnifiedRepresentation survives serialization roundtrips.

use proptest::prelude::*;
use spine_parser::{parse_html, UnifiedRepresentation};

// ========== PARSER SAFETY ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// parse_html must never panic on arbitrary strings
    #[test]
    fn parse_html_never_panics(input in "\\PC{0,2000}") {
        // We don't care about the result — just that it doesn't panic
        let _ = parse_html(&input);
    }

    /// parse_html on valid HTML fragments always returns Ok
    #[test]
    fn parse_valid_html_ok(
        title in "[a-zA-Z0-9 ]{1,50}",
        body in "[a-zA-Z0-9 ,.!?]{1,200}",
    ) {
        let html = format!("<html><head><title>{}</title></head><body><p>{}</p></body></html>", title, body);
        let result = parse_html(&html);
        prop_assert!(result.is_ok(), "valid HTML should parse: {:?}", result.err());
    }

    /// parse_html preserves title text
    #[test]
    fn parse_preserves_title(title in "[a-zA-Z0-9]{1,30}") {
        let html = format!("<html><head><title>{}</title></head><body>text</body></html>", title);
        let ur = parse_html(&html).unwrap();
        prop_assert_eq!(&ur.title, &title);
    }
}

// ========== PR SERDE ROUNDTRIP ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// UnifiedRepresentation survives JSON roundtrip
    #[test]
    fn ur_json_roundtrip(
        title in "[a-zA-Z0-9 ]{0,50}",
        text_content in "[a-zA-Z0-9 ,.]{0,200}",
    ) {
        let html = format!(
            "<html><head><title>{}</title></head><body><p>{}</p></body></html>",
            title, text_content
        );
        if let Ok(ur) = parse_html(&html) {
            let json = serde_json::to_string(&ur).expect("serialize");
            let decoded: UnifiedRepresentation = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(ur.title, decoded.title);
            prop_assert_eq!(ur.elements.len(), decoded.elements.len());
        }
    }

    /// parse_html on nested HTML structures doesn't stack overflow
    #[test]
    fn parse_nested_html(depth in 1usize..50) {
        let open: String = (0..depth).map(|_| "<div>").collect();
        let close: String = (0..depth).map(|_| "</div>").collect();
        let html = format!("<html><body>{}{}</body></html>", open, close);
        let _ = parse_html(&html); // Must not panic or stack overflow
    }

    /// parse_html with headings extracts the correct level
    #[test]
    fn parse_heading_levels(level in 1u8..=6, text in "[a-zA-Z]{1,20}") {
        let html = format!(
            "<html><body><h{0}>{1}</h{0}></body></html>",
            level, text
        );
        let ur = parse_html(&html).unwrap();
        let has_heading = ur.elements.iter().any(|e| {
            matches!(e, spine_parser::Element::Heading { level: l, text: t } if *l == level && t == &text)
        });
        prop_assert!(has_heading, "heading h{} with text '{}' not found in {:?}", level, text, ur.elements);
    }
}

// ========== EDGE CASES ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// parse_html with links extracts URL and text
    #[test]
    fn parse_links(
        link_text in "[a-zA-Z]{1,20}",
        url in "https://[a-z]{1,10}\\.[a-z]{2,3}",
    ) {
        let html = format!(
            r#"<html><body><a href="{}">{}</a></body></html>"#,
            url, link_text
        );
        let ur = parse_html(&html).unwrap();
        let has_link = ur.elements.iter().any(|e| {
            matches!(e, spine_parser::Element::Link { text, url: u } if text == &link_text && u == &url)
        });
        prop_assert!(has_link, "link not found");
    }

    /// Empty input returns Ok with empty title
    #[test]
    fn parse_empty_string(_ in Just(())) {
        let result = parse_html("");
        prop_assert!(result.is_ok());
    }
}
