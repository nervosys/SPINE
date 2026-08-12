use scraper::{node::Node, Html, Selector};
use serde::{Deserialize, Serialize};
use spine_name::{Link, Rel, SpineUri};
use std::sync::OnceLock;

pub mod extraction;

/// Split a `data-capabilities` attribute into normalized terms.
fn parse_capability_list(raw: &str) -> Vec<String> {
    raw.split([',', ' '])
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

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
    /// A link into the *agent* web: a `spine://` name rather than an HTTP URL.
    ///
    /// Kept distinct from [`Element::Link`] because the two are not
    /// interchangeable — one is a name that resolves through the SPINE
    /// namespace and can be verified against its own authority, the other is a
    /// host-dependent URL. `target` is stored as the canonical string form so
    /// the UR stays a plain serializable tree; parse it with
    /// `SpineUri::parse` when a typed value is needed.
    AgentLink {
        text: String,
        target: String,
        /// Relation, in [`Rel`]'s wire spelling.
        rel: String,
        /// Capability terms the target is claimed to offer.
        capabilities: Vec<String>,
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

impl UnifiedRepresentation {
    /// Every agent-web edge in this representation, as typed [`Link`]s ready to
    /// hand to a [`spine_name::CrawlFrontier`].
    ///
    /// Walks the whole tree, since links nest inside containers and lists. Any
    /// `AgentLink` whose target does not parse is skipped rather than guessed
    /// at — a malformed name is a publishing bug, and silently coercing it to
    /// something plausible would send crawlers somewhere nobody asked for.
    pub fn agent_links(&self) -> Vec<Link> {
        let mut out = Vec::new();
        for element in &self.elements {
            collect_agent_links(element, &mut out);
        }
        out
    }

    /// Whether this representation participates in the agent web at all.
    pub fn has_agent_links(&self) -> bool {
        !self.agent_links().is_empty()
    }
}

fn collect_agent_links(element: &Element, out: &mut Vec<Link>) {
    match element {
        Element::AgentLink {
            text,
            target,
            rel,
            capabilities,
        } => {
            if let Ok(uri) = SpineUri::parse(target) {
                let mut link = Link::new(Rel::parse(rel), uri);
                if !text.is_empty() {
                    link = link.with_title(text.clone());
                }
                for cap in capabilities {
                    link = link.with_capability(cap.clone());
                }
                out.push(link);
            }
        }
        Element::List { items, .. } => {
            for item in items {
                collect_agent_links(item, out);
            }
        }
        Element::Container { children, .. } => {
            for child in children {
                collect_agent_links(child, out);
            }
        }
        _ => {}
    }
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
                    // A `spine://` href is an edge in the agent web, not a
                    // pointer back into the human one. Classifying it here means
                    // every consumer of a UR — crawler, planner, gateway — sees
                    // the distinction without re-parsing the href themselves.
                    match SpineUri::parse(&url) {
                        Ok(target) => Some(Element::AgentLink {
                            text,
                            target: target.to_string(),
                            rel: el
                                .attr("rel")
                                .map(|r| Rel::parse(r).as_str().to_string())
                                .unwrap_or_else(|| Rel::Peer.as_str().to_string()),
                            capabilities: el
                                .attr("data-capabilities")
                                .map(parse_capability_list)
                                .unwrap_or_default(),
                        }),
                        Err(_) => Some(Element::Link { text, url }),
                    }
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
mod agent_web_tests {
    use super::*;

    fn did() -> String {
        spine_name::SpineUri::did([3u8; 32]).to_string()
    }

    fn page(body: &str) -> UnifiedRepresentation {
        parse_html(&format!("<html><body>{body}</body></html>")).unwrap()
    }

    #[test]
    fn an_http_href_stays_an_ordinary_link() {
        let ur = page(r#"<a href="https://example.com/docs">Docs</a>"#);
        let link = ur
            .elements
            .iter()
            .find(|e| matches!(e, Element::Link { .. }))
            .expect("expected an Element::Link");
        match link {
            Element::Link { text, url } => {
                assert_eq!(text, "Docs");
                assert_eq!(url, "https://example.com/docs");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(
            !ur.has_agent_links(),
            "an HTTP URL is not an edge in the agent web"
        );
    }

    #[test]
    fn a_spine_href_becomes_a_typed_agent_link() {
        let ur = page(&format!(
            r#"<a href="{}" rel="provides" data-capabilities="web.search, Data.Analyze">Search</a>"#,
            did()
        ));
        let links = ur.agent_links();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].rel, spine_name::Rel::Provides);
        assert_eq!(links[0].title.as_deref(), Some("Search"));
        assert_eq!(
            links[0].capabilities,
            vec!["web.search".to_string(), "data.analyze".to_string()],
            "capability terms are normalized at parse time"
        );
        assert_eq!(links[0].target.public_key(), Some(&[3u8; 32]));
    }

    #[test]
    fn an_agent_link_without_a_rel_defaults_to_peer() {
        let ur = page(&format!(r#"<a href="{}">Somewhere</a>"#, did()));
        assert_eq!(ur.agent_links()[0].rel, spine_name::Rel::Peer);
    }

    #[test]
    fn an_unknown_rel_is_preserved_rather_than_discarded() {
        let ur = page(&format!(r#"<a href="{}" rel="mirrors">M</a>"#, did()));
        assert_eq!(
            ur.agent_links()[0].rel,
            spine_name::Rel::Other("mirrors".into())
        );
    }

    #[test]
    fn agent_links_are_found_inside_containers_and_lists() {
        let ur = page(&format!(
            r#"<div><ul><li><a href="{}" rel="child">Nested</a></li></ul></div>"#,
            did()
        ));
        assert_eq!(
            ur.agent_links().len(),
            1,
            "links nest; a shallow scan would miss them"
        );
    }

    #[test]
    fn a_malformed_spine_name_is_skipped_not_guessed_at() {
        // Parses as a spine URI attempt but the key is not 32 bytes.
        let ur = page(r#"<a href="spine://did:mzxw6/">Broken</a>"#);
        assert!(
            ur.agent_links().is_empty(),
            "a malformed name must not be coerced into a plausible one"
        );
    }

    #[test]
    fn a_representation_can_mix_both_webs() {
        let ur = page(&format!(
            r#"<a href="https://example.com">Human</a><a href="{}" rel="child">Agent</a>"#,
            did()
        ));
        assert_eq!(ur.agent_links().len(), 1);
        assert_eq!(
            ur.elements
                .iter()
                .filter(|e| matches!(e, Element::Link { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn extracted_links_feed_a_crawl_frontier_directly() {
        // The whole point of typing links in the UR: a crawler gets its edges
        // without re-parsing hrefs or guessing at relations.
        let ur = page(&format!(
            r#"<a href="{}" rel="requires">Dep</a>"#,
            did()
        ));
        let mut frontier = spine_name::CrawlFrontier::new(spine_name::CrawlBudget::default());
        let seed = spine_name::SpineUri::did([1u8; 32]);
        frontier.seed(seed.clone());
        frontier.next_visit();
        assert_eq!(frontier.expand(&seed, &ur.agent_links(), 0), 1);

        let visit = frontier.next_visit().unwrap();
        assert_eq!(visit.via, Some(spine_name::Rel::Requires));
        assert_eq!(visit.depth, 1);
    }

    #[test]
    fn agent_links_survive_a_json_roundtrip() {
        let ur = page(&format!(r#"<a href="{}" rel="child">X</a>"#, did()));
        let json = serde_json::to_string(&ur).unwrap();
        let back: UnifiedRepresentation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.agent_links().len(), 1);
        assert_eq!(back.agent_links()[0].rel, spine_name::Rel::Child);
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
