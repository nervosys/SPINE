use scraper::{Html, Selector, node::Node};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct UnifiedRepresentation {
    pub title: String,
    pub elements: Vec<Element>,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Element {
    Text(String),
    Heading { level: u8, text: String },
    Link { text: String, url: String },
    Button { text: String, action_id: String },
    Input { label: String, input_type: String, id: String },
    Image { alt: String, src: String },
    List { items: Vec<Element>, ordered: bool },
    Container { tag: String, children: Vec<Element> },
}

pub fn parse_html(html: &str) -> anyhow::Result<UnifiedRepresentation> {
    let document = Html::parse_document(html);
    
    // Extract title
    let title_selector = Selector::parse("title").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .map(|e| e.text().collect::<String>())
        .unwrap_or_else(|| "No Title".to_string());

    let mut elements = Vec::new();
    let body_selector = Selector::parse("body").unwrap();
    if let Some(body) = document.select(&body_selector).next() {
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

fn get_text(node: ego_tree::NodeRef<Node>) -> String {
    node.descendants()
        .filter_map(|n| {
            if let Node::Text(t) = n.value() {
                Some(t.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
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
                    let action_id = el.attr("id")
                        .or_else(|| el.attr("name"))
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("btn_{}", text.to_lowercase().replace(" ", "_")));
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
                    Some(Element::List { items, ordered: tag == "ol" })
                }
                "input" => {
                    let label = el.attr("placeholder").or(el.attr("name")).unwrap_or("input").to_string();
                    let input_type = el.attr("type").unwrap_or("text").to_string();
                    let id = el.attr("id").or(el.attr("name")).unwrap_or("unknown").to_string();
                    Some(Element::Input { label, input_type, id })
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
                        Some(Element::Container { tag: tag.to_string(), children })
                    }
                }
            }
        }
        _ => None,
    }
}