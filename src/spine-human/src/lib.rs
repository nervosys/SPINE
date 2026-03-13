// Allow dead code for human interaction simulation APIs
#![allow(dead_code)]

use anyhow::Result;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use spine_compiler::Compiler;
use spine_protocol::SpineBinary;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
struct SemanticInfo {
    title: String,
    page_type: PageType,
    inferred_state: HashMap<String, String>,
    intent: String,
    reasoning: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
enum PageType {
    #[default]
    Unknown,
    Content,
    Interactive,
    Navigation,
}

/// Simulates human-like interaction patterns for agentic browsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanInteractionEngine {
    pub jitter_factor: f32,
    pub typing_speed_wpm: u32,
    pub reaction_time_ms: u64,
}

impl Default for HumanInteractionEngine {
    fn default() -> Self {
        Self {
            jitter_factor: 0.1,
            typing_speed_wpm: 60,
            reaction_time_ms: 250,
        }
    }
}

impl HumanInteractionEngine {
    pub fn new(jitter: f32, wpm: u32, reaction: u64) -> Self {
        Self {
            jitter_factor: jitter,
            typing_speed_wpm: wpm,
            reaction_time_ms: reaction,
        }
    }

    /// Generates a realistic mouse path between two points.
    pub fn generate_mouse_path(
        &self,
        start: (f32, f32),
        end: (f32, f32),
        steps: usize,
    ) -> Vec<(f32, f32)> {
        let mut path = Vec::with_capacity(steps);
        let mut rng = thread_rng();

        for i in 0..=steps {
            let t = i as f32 / steps as f32;

            // Linear interpolation
            let mut x = start.0 + (end.0 - start.0) * t;
            let mut y = start.1 + (end.1 - start.1) * t;

            // Add jitter (Bezier-like curve simulation)
            if i > 0 && i < steps {
                let jitter_x =
                    (rng.gen::<f32>() - 0.5) * (end.0 - start.0).abs() * self.jitter_factor;
                let jitter_y =
                    (rng.gen::<f32>() - 0.5) * (end.1 - start.1).abs() * self.jitter_factor;
                x += jitter_x;
                y += jitter_y;
            }

            path.push((x, y));
        }

        path
    }

    /// Generates delays between keystrokes for a given string.
    pub fn generate_typing_delays(&self, text: &str) -> Vec<Duration> {
        let mut delays = Vec::with_capacity(text.len());
        let mut rng = thread_rng();

        // Average ms per character based on WPM (assuming 5 chars per word)
        let avg_ms = 60000 / (self.typing_speed_wpm * 5);

        for c in text.chars() {
            let mut delay = avg_ms as f32;

            // Add randomness
            delay *= rng.gen_range(0.5..1.5);

            // Longer delay for spaces and punctuation
            if c.is_whitespace() || c.is_ascii_punctuation() {
                delay *= 1.5;
            }

            delays.push(Duration::from_millis(delay as u64));
        }

        delays
    }

    /// Simulates a human click with variable duration.
    pub fn simulate_click_duration(&self) -> Duration {
        let mut rng = thread_rng();
        // Typical human click is 50ms to 150ms
        Duration::from_millis(rng.gen_range(50..150))
    }
}

pub struct HumanTranspiler;

impl HumanTranspiler {
    pub fn transpile(html: &str, css: &str, js: &str) -> Result<SpineBinary> {
        let document = scraper::Html::parse_document(html);
        let mut hls_source = String::new();

        // 1. Semantic Analysis Pass
        let semantic_info = Self::analyze_semantics(&document);
        hls_source.push_str(&format!("// Semantic Analysis: {}\n", semantic_info.title));
        hls_source.push_str(&format!("// Page Type: {:?}\n", semantic_info.page_type));
        hls_source.push_str(&format!("// Inferred Intent: {}\n", semantic_info.intent));
        for reason in &semantic_info.reasoning {
            hls_source.push_str(&format!("// Reasoning: {}\n", reason));
        }

        // 2. State Generation
        for (name, initial) in semantic_info.inferred_state {
            hls_source.push_str(&format!("state {} = {}\n", name, initial));
        }

        // 3. Capability Requirements
        hls_source.push_str("capability network\n");
        hls_source.push_str("capability storage\n\n");

        // Extract global styles from <style> tags
        let style_selector = scraper::Selector::parse("style")
            .map_err(|_| anyhow::anyhow!("Invalid CSS selector: style"))?;
        let mut global_css = css.to_string();
        for style_node in document.select(&style_selector) {
            global_css.push_str(&style_node.text().collect::<String>());
        }

        // Extract global scripts from <script> tags
        let script_selector = scraper::Selector::parse("script")
            .map_err(|_| anyhow::anyhow!("Invalid CSS selector: script"))?;
        let mut global_js = js.to_string();
        for script_node in document.select(&script_selector) {
            global_js.push_str(&script_node.text().collect::<String>());
        }

        if !global_css.is_empty() {
            hls_source.push_str(&format!("// Global CSS: {} bytes\n", global_css.len()));
        }
        if !global_js.is_empty() {
            hls_source.push_str(&format!("// Global JS: {} bytes\n", global_js.len()));
        }

        hls_source.push_str("fn render() {\n");
        hls_source.push_str("  element Root {\n");

        // Traverse the body
        let body_selector = scraper::Selector::parse("body")
            .map_err(|_| anyhow::anyhow!("Invalid CSS selector: body"))?;
        if let Some(body) = document.select(&body_selector).next() {
            for child in body.children() {
                Self::convert_node(child, &mut hls_source, 2);
            }
        } else {
            // Fallback to root children if no body
            for child in document.tree.root().children() {
                Self::convert_node(child, &mut hls_source, 2);
            }
        }

        hls_source.push_str("  }\n");
        hls_source.push_str("}\n");

        // Compile the generated HLS to HLB
        Compiler::compile(&hls_source)
    }

    fn analyze_semantics(doc: &scraper::Html) -> SemanticInfo {
        let mut info = SemanticInfo::default();

        // Title extraction
        let Ok(title_selector) = scraper::Selector::parse("title") else {
            return info;
        };
        if let Some(title) = doc.select(&title_selector).next() {
            info.title = title.text().collect();
        }

        // Page type inference
        let Ok(form_selector) = scraper::Selector::parse("form") else {
            return info;
        };
        let Ok(article_selector) = scraper::Selector::parse("article") else {
            return info;
        };
        let Ok(search_selector) =
            scraper::Selector::parse("input[type='search'], input[name='q'], input[name='query']")
        else {
            return info;
        };
        let Ok(login_selector) = scraper::Selector::parse("input[type='password']") else {
            return info;
        };

        if doc.select(&form_selector).next().is_some() {
            info.page_type = PageType::Interactive;
            info.reasoning
                .push("Found form elements, suggesting interactivity.".to_string());
        } else if doc.select(&article_selector).next().is_some() {
            info.page_type = PageType::Content;
            info.reasoning
                .push("Found article tags, suggesting content-heavy page.".to_string());
        }

        // Intent inference
        if doc.select(&search_selector).next().is_some() {
            info.intent = "Search".to_string();
            info.reasoning
                .push("Detected search input field.".to_string());
        } else if doc.select(&login_selector).next().is_some() {
            info.intent = "Authentication".to_string();
            info.reasoning
                .push("Detected password input field.".to_string());
        } else if info.page_type == PageType::Content {
            info.intent = "Information Retrieval".to_string();
        } else {
            info.intent = "Exploration".to_string();
        }

        // Inferred state from inputs
        let Ok(input_selector) = scraper::Selector::parse("input[name]") else {
            return info;
        };
        for input in doc.select(&input_selector) {
            if let Some(name) = input.value().attr("name") {
                let safe_name = name.replace("-", "_").replace(" ", "_");
                info.inferred_state.insert(safe_name, "\"\"".to_string());
            }
        }

        info
    }

    fn convert_node(node: ego_tree::NodeRef<scraper::node::Node>, hls: &mut String, indent: usize) {
        use scraper::node::Node;

        let indent_str = "  ".repeat(indent);

        match node.value() {
            Node::Element(elem) => {
                let tag = elem.name();

                // Skip script and style tags as they are handled separately or ignored
                if tag == "script" || tag == "style" {
                    return;
                }

                let hls_tag = Self::map_tag(tag);
                hls.push_str(&format!("{}element {} {{\n", indent_str, hls_tag));

                // Attributes
                for (name, value) in elem.attrs() {
                    if name == "style" {
                        // Parse inline CSS
                        Self::convert_css(value, hls, indent + 1);
                    } else if name.starts_with("on") {
                        // Parse JS event listeners
                        Self::convert_js_event(name, value, hls, indent + 1);
                    } else {
                        hls.push_str(&format!(
                            "{}  attribute {} \"{}\"\n",
                            indent_str,
                            name,
                            value.replace("\"", "\\\"")
                        ));
                    }
                }

                // Children
                for child in node.children() {
                    Self::convert_node(child, hls, indent + 1);
                }

                hls.push_str(&format!("{}}}\n", indent_str));
            }
            Node::Text(text) => {
                let content = text.trim();
                if !content.is_empty() {
                    hls.push_str(&format!(
                        "{}text \"{}\"\n",
                        indent_str,
                        content.replace("\"", "\\\"")
                    ));
                }
            }
            _ => {}
        }
    }

    fn map_tag(tag: &str) -> String {
        match tag {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "Heading".to_string(),
            "a" => "Link".to_string(),
            "button" => "Button".to_string(),
            "img" => "Image".to_string(),
            "ul" | "ol" => "List".to_string(),
            "li" => "ListItem".to_string(),
            "input" => "Input".to_string(),
            "label" => "Label".to_string(),
            "form" => "Form".to_string(),
            "table" => "Table".to_string(),
            "tr" => "Row".to_string(),
            "td" | "th" => "Cell".to_string(),
            "nav" => "Navigation".to_string(),
            "footer" => "Footer".to_string(),
            "header" => "Header".to_string(),
            "main" => "MainContent".to_string(),
            "section" => "Section".to_string(),
            "article" => "Article".to_string(),
            "aside" => "Aside".to_string(),
            _ => {
                // Default: capitalize first letter
                let mut chars = tag.chars();
                match chars.next() {
                    None => "Element".to_string(),
                    Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                }
            }
        }
    }

    fn convert_css(style: &str, hls: &mut String, indent: usize) {
        let indent_str = "  ".repeat(indent);
        for declaration in style.split(';') {
            let parts: Vec<&str> = declaration.split(':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().replace("-", "_");
                let value = parts[1].trim();
                hls.push_str(&format!(
                    "{}attribute {} \"{}\"\n",
                    indent_str,
                    key,
                    value.replace("\"", "\\\"")
                ));
            }
        }
    }

    fn convert_js_event(name: &str, value: &str, hls: &mut String, indent: usize) {
        let indent_str = "  ".repeat(indent);
        let event_name = name.strip_prefix("on").unwrap_or(name);

        // Map common events to HLS event emitters
        hls.push_str(&format!(
            "{}on_{} -> emit(\"{}\", {{ \"script\": \"{}\" }})\n",
            indent_str,
            event_name,
            event_name,
            value.replace("\"", "\\\"")
        ));
    }
}

/// Reasoning Engine: Analyzes the Unified Representation to suggest agentic actions.
pub struct ReasoningEngine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub action_type: String,
    pub target_id: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub confidence: f32,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlan {
    pub goal: String,
    pub steps: Vec<AgentAction>,
    pub estimated_success: f32,
}

impl Default for ReasoningEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ReasoningEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn suggest_actions(&self, ur: &spine_parser::UnifiedRepresentation) -> Vec<AgentAction> {
        let mut suggestions = Vec::new();
        use spine_parser::Element;

        for node in &ur.elements {
            match node {
                Element::Button { text, action_id }
                | Element::Link {
                    text,
                    url: action_id,
                } => {
                    let text_low = text.to_lowercase();
                    if text_low.contains("search") || text_low.contains("find") {
                        suggestions.push(AgentAction {
                            action_type: "Search".to_string(),
                            target_id: Some(action_id.clone()),
                            payload: None,
                            confidence: 0.85,
                            reasoning: format!("Found search-related element with text '{}'", text),
                        });
                    } else if text_low.contains("login") || text_low.contains("sign in") {
                        suggestions.push(AgentAction {
                            action_type: "Authenticate".to_string(),
                            target_id: Some(action_id.clone()),
                            payload: None,
                            confidence: 0.9,
                            reasoning: "Detected authentication entry point.".to_string(),
                        });
                    }
                }
                Element::Input {
                    label,
                    input_type,
                    id,
                } => {
                    let label_low = label.to_lowercase();
                    if label_low.contains("search")
                        || input_type == "search"
                        || id.contains("search")
                    {
                        suggestions.push(AgentAction {
                            action_type: "InputSearch".to_string(),
                            target_id: Some(id.clone()),
                            payload: Some(serde_json::json!({ "label": label })),
                            confidence: 0.95,
                            reasoning: "Identified primary search input field.".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }

        // Sort by confidence
        suggestions.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        suggestions
    }

    pub fn create_plan(&self, goal: &str, ur: &spine_parser::UnifiedRepresentation) -> AgentPlan {
        let suggestions = self.suggest_actions(ur);
        let mut steps = Vec::new();
        let mut confidence_sum = 0.0;

        // Heuristic: If goal contains "search", prioritize search actions
        if goal.to_lowercase().contains("search") {
            if let Some(search_input) = suggestions.iter().find(|a| a.action_type == "InputSearch")
            {
                steps.push(search_input.clone());
                confidence_sum += search_input.confidence;
            }
            if let Some(search_btn) = suggestions.iter().find(|a| a.action_type == "Search") {
                steps.push(search_btn.clone());
                confidence_sum += search_btn.confidence;
            }
        } else if goal.to_lowercase().contains("login") || goal.to_lowercase().contains("auth") {
            if let Some(auth_btn) = suggestions.iter().find(|a| a.action_type == "Authenticate") {
                steps.push(auth_btn.clone());
                confidence_sum += auth_btn.confidence;
            }
        }

        let estimated_success = if steps.is_empty() {
            0.0
        } else {
            confidence_sum / steps.len() as f32
        };

        AgentPlan {
            goal: goal.to_string(),
            steps,
            estimated_success,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpile_simple() {
        let html = "<div><h1>Hello</h1><button>Click</button></div>";
        let result = HumanTranspiler::transpile(html, "", "");
        assert!(result.is_ok());
        let bin = result.unwrap();
        assert!(!bin.instructions.is_empty());
    }

    #[test]
    fn test_human_interaction() {
        let engine = HumanInteractionEngine::default();

        let path = engine.generate_mouse_path((0.0, 0.0), (100.0, 100.0), 10);
        assert_eq!(path.len(), 11);
        assert_eq!(path[0], (0.0, 0.0));
        assert_eq!(path[10], (100.0, 100.0));

        let delays = engine.generate_typing_delays("Hello World");
        assert_eq!(delays.len(), 11);

        let click = engine.simulate_click_duration();
        assert!(click.as_millis() >= 50 && click.as_millis() <= 150);
    }

    #[test]
    fn test_engine_custom_params() {
        let engine = HumanInteractionEngine::new(0.5, 120, 500);
        assert_eq!(engine.jitter_factor, 0.5);
        assert_eq!(engine.typing_speed_wpm, 120);
        assert_eq!(engine.reaction_time_ms, 500);
    }

    #[test]
    fn test_mouse_path_single_step() {
        let engine = HumanInteractionEngine::default();
        let path = engine.generate_mouse_path((10.0, 20.0), (50.0, 60.0), 1);
        assert_eq!(path.len(), 2);
        assert_eq!(path[0], (10.0, 20.0));
        assert_eq!(path[1], (50.0, 60.0));
    }

    #[test]
    fn test_mouse_path_zero_jitter() {
        let engine = HumanInteractionEngine::new(0.0, 60, 250);
        let path = engine.generate_mouse_path((0.0, 0.0), (100.0, 0.0), 5);
        // With zero jitter, intermediate points are on the line
        for (i, &(x, y)) in path.iter().enumerate() {
            let expected_x = i as f32 * 20.0;
            assert!((x - expected_x).abs() < 0.01, "x={x} expected={expected_x}");
            assert!((y - 0.0).abs() < 0.01);
        }
    }

    #[test]
    fn test_typing_delays_empty_string() {
        let engine = HumanInteractionEngine::default();
        let delays = engine.generate_typing_delays("");
        assert!(delays.is_empty());
    }

    #[test]
    fn test_typing_delays_count_matches_chars() {
        let engine = HumanInteractionEngine::default();
        let text = "Hello, World! 123";
        let delays = engine.generate_typing_delays(text);
        assert_eq!(delays.len(), text.len());
    }

    #[test]
    fn test_engine_serde_roundtrip() {
        let engine = HumanInteractionEngine::new(0.2, 80, 300);
        let json = serde_json::to_string(&engine).unwrap();
        let restored: HumanInteractionEngine = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.jitter_factor, 0.2);
        assert_eq!(restored.typing_speed_wpm, 80);
        assert_eq!(restored.reaction_time_ms, 300);
    }

    #[test]
    fn test_map_tag_headings() {
        for tag in &["h1", "h2", "h3", "h4", "h5", "h6"] {
            assert_eq!(HumanTranspiler::map_tag(tag), "Heading");
        }
    }

    #[test]
    fn test_map_tag_semantic_elements() {
        assert_eq!(HumanTranspiler::map_tag("nav"), "Navigation");
        assert_eq!(HumanTranspiler::map_tag("footer"), "Footer");
        assert_eq!(HumanTranspiler::map_tag("header"), "Header");
        assert_eq!(HumanTranspiler::map_tag("main"), "MainContent");
        assert_eq!(HumanTranspiler::map_tag("article"), "Article");
        assert_eq!(HumanTranspiler::map_tag("section"), "Section");
    }

    #[test]
    fn test_map_tag_form_elements() {
        assert_eq!(HumanTranspiler::map_tag("form"), "Form");
        assert_eq!(HumanTranspiler::map_tag("input"), "Input");
        assert_eq!(HumanTranspiler::map_tag("button"), "Button");
        assert_eq!(HumanTranspiler::map_tag("label"), "Label");
    }

    #[test]
    fn test_map_tag_unknown_capitalizes() {
        assert_eq!(HumanTranspiler::map_tag("custom"), "Custom");
        assert_eq!(HumanTranspiler::map_tag("div"), "Div");
        assert_eq!(HumanTranspiler::map_tag("span"), "Span");
    }

    #[test]
    fn test_map_tag_empty() {
        assert_eq!(HumanTranspiler::map_tag(""), "Element");
    }

    #[test]
    fn test_transpile_with_css_and_js() {
        let html = "<body><h1>Test</h1></body>";
        let css = "body { color: red; }";
        let js = "console.log('hi');";
        let result = HumanTranspiler::transpile(html, css, js);
        assert!(result.is_ok());
    }

    #[test]
    fn test_transpile_with_form() {
        let html = r#"<body><form><input type="text" name="q"><button>Search</button></form></body>"#;
        let result = HumanTranspiler::transpile(html, "", "");
        assert!(result.is_ok());
    }

    #[test]
    fn test_reasoning_engine_search_actions() {
        let engine = ReasoningEngine::new();
        let ur = spine_parser::parse_html(
            r#"<body><input type="search" id="q" placeholder="Search"><button>Search</button></body>"#,
        ).unwrap();
        let actions = engine.suggest_actions(&ur);
        assert!(!actions.is_empty());
        // Should have InputSearch action
        assert!(actions.iter().any(|a| a.action_type == "InputSearch"));
        assert!(actions.iter().any(|a| a.action_type == "Search"));
        // Sorted by confidence descending
        for w in actions.windows(2) {
            assert!(w[0].confidence >= w[1].confidence);
        }
    }

    #[test]
    fn test_reasoning_engine_login_actions() {
        let engine = ReasoningEngine::new();
        let ur = spine_parser::parse_html(
            r#"<body><a href="/login">Sign In</a></body>"#,
        ).unwrap();
        let actions = engine.suggest_actions(&ur);
        assert!(actions.iter().any(|a| a.action_type == "Authenticate"));
    }

    #[test]
    fn test_reasoning_engine_no_actions() {
        let engine = ReasoningEngine::new();
        let ur = spine_parser::parse_html("<body><p>Plain text</p></body>").unwrap();
        let actions = engine.suggest_actions(&ur);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_create_plan_search_goal() {
        let engine = ReasoningEngine::new();
        let ur = spine_parser::parse_html(
            r#"<body><input type="search" id="q" placeholder="Search"><button>Search</button></body>"#,
        ).unwrap();
        let plan = engine.create_plan("search for cats", &ur);
        assert_eq!(plan.goal, "search for cats");
        assert!(!plan.steps.is_empty());
        assert!(plan.estimated_success > 0.0);
    }

    #[test]
    fn test_create_plan_login_goal() {
        let engine = ReasoningEngine::new();
        let ur = spine_parser::parse_html(
            r#"<body><button>Login</button></body>"#,
        ).unwrap();
        let plan = engine.create_plan("login to site", &ur);
        assert!(!plan.steps.is_empty());
    }

    #[test]
    fn test_create_plan_empty_goal() {
        let engine = ReasoningEngine::new();
        let ur = spine_parser::parse_html("<body><p>Hello</p></body>").unwrap();
        let plan = engine.create_plan("browse around", &ur);
        assert_eq!(plan.estimated_success, 0.0);
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn test_agent_action_serde() {
        let action = AgentAction {
            action_type: "Search".to_string(),
            target_id: Some("btn1".to_string()),
            payload: Some(serde_json::json!({"query": "test"})),
            confidence: 0.85,
            reasoning: "Found search button".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let restored: AgentAction = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.action_type, "Search");
        assert_eq!(restored.confidence, 0.85);
    }

    #[test]
    fn test_agent_plan_serde() {
        let plan = AgentPlan {
            goal: "test".to_string(),
            steps: vec![],
            estimated_success: 0.5,
        };
        let json = serde_json::to_string(&plan).unwrap();
        let restored: AgentPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.goal, "test");
    }
}
