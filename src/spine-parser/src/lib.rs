use scraper::{node::Node, Html, Selector};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub mod extraction;

// Cached selectors: Selector::parse is expensive — compile once, reuse forever
static TITLE_SELECTOR: OnceLock<Selector> = OnceLock::new();
static BODY_SELECTOR: OnceLock<Selector> = OnceLock::new();

#[inline]
fn title_selector() -> &'static Selector {
    TITLE_SELECTOR.get_or_init(|| Selector::parse("title").unwrap())
}

#[inline]
fn body_selector() -> &'static Selector {
    BODY_SELECTOR.get_or_init(|| Selector::parse("body").unwrap())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedRepresentation {
    pub title: String,
    pub elements: Vec<Element>,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Element {
    Text(String),
    Heading {
        level: u8,
        text: String,
    },
    Link {
        text: String,
        url: String,
    },
    Button {
        text: String,
        action_id: String,
    },
    Input {
        label: String,
        input_type: String,
        id: String,
    },
    Image {
        alt: String,
        src: String,
    },
    List {
        items: Vec<Element>,
        ordered: bool,
    },
    Container {
        tag: String,
        children: Vec<Element>,
    },
}

pub fn parse_html(html: &str) -> anyhow::Result<UnifiedRepresentation> {
    let document = Html::parse_document(html);

    // Use cached selectors (avoids re-compiling CSS selectors on every call)
    let title = document
        .select(title_selector())
        .next()
        .map(|e| e.text().collect::<String>())
        .unwrap_or_else(|| "No Title".to_string());

    let mut elements = Vec::new();
    if let Some(body) = document.select(body_selector()).next() {
        for child in body.children() {
            if let Some(el) = parse_node(child) {
                elements.push(el);
            }
        }
    }

    Ok(UnifiedRepresentation {
        title,
        elements,
        metadata: std::collections::HashMap::new(),
    })
}

/// Single-pass text extraction: collects descendant text into a String directly
/// without intermediate Vec<String> + join.
fn get_text(node: ego_tree::NodeRef<Node>) -> String {
    let mut result = String::new();
    for n in node.descendants() {
        if let Node::Text(t) = n.value() {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push_str(trimmed);
            }
        }
    }
    result
}

fn parse_node(node: ego_tree::NodeRef<Node>) -> Option<Element> {
    match node.value() {
        Node::Text(text) => {
            let content = text.trim();
            if content.is_empty() {
                None
            } else {
                Some(Element::Text(content.to_string()))
            }
        }
        Node::Element(el) => {
            let tag = el.name();
            match tag {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let level = tag[1..].parse().unwrap_or(1);
                    let text = get_text(node);
                    Some(Element::Heading { level, text })
                }
                "a" => {
                    let text = get_text(node);
                    let url = el.attr("href").unwrap_or_default().to_string();
                    Some(Element::Link { text, url })
                }
                "button" => {
                    let text = get_text(node);
                    let action_id = el
                        .attr("id")
                        .or_else(|| el.attr("name"))
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            format!("btn_{}", text.to_lowercase().replace(" ", "_"))
                        });
                    Some(Element::Button { text, action_id })
                }
                "img" => {
                    let alt = el.attr("alt").unwrap_or_default().to_string();
                    let src = el.attr("src").unwrap_or_default().to_string();
                    Some(Element::Image { alt, src })
                }
                "ul" | "ol" => {
                    let mut items = Vec::new();
                    for child in node.children() {
                        if let Node::Element(child_el) = child.value() {
                            if child_el.name() == "li" {
                                for li_child in child.children() {
                                    if let Some(parsed) = parse_node(li_child) {
                                        items.push(parsed);
                                    }
                                }
                            }
                        }
                    }
                    Some(Element::List {
                        items,
                        ordered: tag == "ol",
                    })
                }
                "input" => {
                    let label = el
                        .attr("placeholder")
                        .or(el.attr("name"))
                        .unwrap_or("input")
                        .to_string();
                    let input_type = el.attr("type").unwrap_or("text").to_string();
                    let id = el
                        .attr("id")
                        .or(el.attr("name"))
                        .unwrap_or("unknown")
                        .to_string();
                    Some(Element::Input {
                        label,
                        input_type,
                        id,
                    })
                }
                _ => {
                    let mut children = Vec::new();
                    for child in node.children() {
                        if let Some(parsed) = parse_node(child) {
                            children.push(parsed);
                        }
                    }
                    if children.is_empty() {
                        None
                    } else {
                        Some(Element::Container {
                            tag: tag.to_string(),
                            children,
                        })
                    }
                }
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_title_extraction() {
        let ur = parse_html("<html><head><title>My Page</title></head><body></body></html>").unwrap();
        assert_eq!(ur.title, "My Page");
    }

    #[test]
    fn test_parse_no_title_fallback() {
        let ur = parse_html("<html><body><p>Hello</p></body></html>").unwrap();
        assert_eq!(ur.title, "No Title");
    }

    #[test]
    fn test_parse_heading_levels() {
        for level in 1..=6 {
            let html = format!("<html><body><h{level}>Title</h{level}></body></html>");
            let ur = parse_html(&html).unwrap();
            assert_eq!(ur.elements.len(), 1);
            match &ur.elements[0] {
                Element::Heading { level: l, text } => {
                    assert_eq!(*l, level);
                    assert_eq!(text, "Title");
                }
                _ => panic!("Expected Heading, got {:?}", ur.elements[0]),
            }
        }
    }

    #[test]
    fn test_parse_link() {
        let ur = parse_html(r#"<body><a href="https://example.com">Click</a></body>"#).unwrap();
        match &ur.elements[0] {
            Element::Link { text, url } => {
                assert_eq!(text, "Click");
                assert_eq!(url, "https://example.com");
            }
            _ => panic!("Expected Link"),
        }
    }

    #[test]
    fn test_parse_link_no_href() {
        let ur = parse_html(r#"<body><a>No URL</a></body>"#).unwrap();
        match &ur.elements[0] {
            Element::Link { text, url } => {
                assert_eq!(text, "No URL");
                assert_eq!(url, "");
            }
            _ => panic!("Expected Link"),
        }
    }

    #[test]
    fn test_parse_button_with_id() {
        let ur = parse_html(r#"<body><button id="submit-btn">Submit</button></body>"#).unwrap();
        match &ur.elements[0] {
            Element::Button { text, action_id } => {
                assert_eq!(text, "Submit");
                assert_eq!(action_id, "submit-btn");
            }
            _ => panic!("Expected Button"),
        }
    }

    #[test]
    fn test_parse_button_fallback_id() {
        let ur = parse_html(r#"<body><button>Go Now</button></body>"#).unwrap();
        match &ur.elements[0] {
            Element::Button { text, action_id } => {
                assert_eq!(text, "Go Now");
                assert_eq!(action_id, "btn_go_now");
            }
            _ => panic!("Expected Button"),
        }
    }

    #[test]
    fn test_parse_image() {
        let ur = parse_html(r#"<body><img src="/cat.jpg" alt="A cat"></body>"#).unwrap();
        match &ur.elements[0] {
            Element::Image { alt, src } => {
                assert_eq!(alt, "A cat");
                assert_eq!(src, "/cat.jpg");
            }
            _ => panic!("Expected Image"),
        }
    }

    #[test]
    fn test_parse_input_types() {
        let ur = parse_html(r#"<body><input type="email" name="user_email" placeholder="Email"></body>"#).unwrap();
        match &ur.elements[0] {
            Element::Input { label, input_type, id } => {
                assert_eq!(label, "Email");
                assert_eq!(input_type, "email");
                assert_eq!(id, "user_email");
            }
            _ => panic!("Expected Input"),
        }
    }

    #[test]
    fn test_parse_input_defaults() {
        let ur = parse_html(r#"<body><input></body>"#).unwrap();
        match &ur.elements[0] {
            Element::Input { label, input_type, id } => {
                assert_eq!(label, "input");
                assert_eq!(input_type, "text");
                assert_eq!(id, "unknown");
            }
            _ => panic!("Expected Input"),
        }
    }

    #[test]
    fn test_parse_ordered_list() {
        let ur = parse_html(r#"<body><ol><li>First</li><li>Second</li></ol></body>"#).unwrap();
        match &ur.elements[0] {
            Element::List { items, ordered } => {
                assert!(*ordered);
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn test_parse_unordered_list() {
        let ur = parse_html(r#"<body><ul><li>A</li><li>B</li></ul></body>"#).unwrap();
        match &ur.elements[0] {
            Element::List { items, ordered } => {
                assert!(!*ordered);
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn test_parse_container() {
        let ur = parse_html(r#"<body><div><span>Hello</span></div></body>"#).unwrap();
        match &ur.elements[0] {
            Element::Container { tag, children } => {
                assert_eq!(tag, "div");
                assert!(!children.is_empty());
            }
            _ => panic!("Expected Container"),
        }
    }

    #[test]
    fn test_parse_empty_div_skipped() {
        let ur = parse_html(r#"<body><div></div></body>"#).unwrap();
        assert!(ur.elements.is_empty());
    }

    #[test]
    fn test_parse_text_whitespace_trimmed() {
        let ur = parse_html(r#"<body>   Hello   </body>"#).unwrap();
        match &ur.elements[0] {
            Element::Text(t) => assert_eq!(t, "Hello"),
            _ => panic!("Expected Text"),
        }
    }

    #[test]
    fn test_parse_empty_text_skipped() {
        let ur = parse_html(r#"<body>   </body>"#).unwrap();
        assert!(ur.elements.is_empty());
    }

    #[test]
    fn test_ur_serde_roundtrip() {
        let ur = parse_html(r#"<body><h1>Title</h1><a href="/x">Link</a></body>"#).unwrap();
        let json = serde_json::to_string(&ur).unwrap();
        let ur2: UnifiedRepresentation = serde_json::from_str(&json).unwrap();
        assert_eq!(ur.title, ur2.title);
        assert_eq!(ur.elements.len(), ur2.elements.len());
    }

    #[test]
    fn test_parse_complex_document() {
        let html = r#"
        <html>
        <head><title>Test</title></head>
        <body>
            <h1>Welcome</h1>
            <p>Some text</p>
            <a href="/login">Sign In</a>
            <form>
                <input type="text" name="username" placeholder="User">
                <input type="password" name="pass">
                <button id="login-btn">Login</button>
            </form>
        </body>
        </html>"#;
        let ur = parse_html(html).unwrap();
        assert_eq!(ur.title, "Test");
        assert!(!ur.elements.is_empty());
    }
}
