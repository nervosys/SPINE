use anyhow::Result;
use hyperlight_compiler::Compiler;
use hyperlight_protocol::HyperlightBinary;
use serde::{Serialize, Deserialize};
use std::time::Duration;
use rand::prelude::*;

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
    pub fn generate_mouse_path(&self, start: (f32, f32), end: (f32, f32), steps: usize) -> Vec<(f32, f32)> {
        let mut path = Vec::with_capacity(steps);
        let mut rng = thread_rng();

        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            
            // Linear interpolation
            let mut x = start.0 + (end.0 - start.0) * t;
            let mut y = start.1 + (end.1 - start.1) * t;

            // Add jitter (Bezier-like curve simulation)
            if i > 0 && i < steps {
                let jitter_x = (rng.gen::<f32>() - 0.5) * (end.0 - start.0).abs() * self.jitter_factor;
                let jitter_y = (rng.gen::<f32>() - 0.5) * (end.1 - start.1).abs() * self.jitter_factor;
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
    pub fn transpile(html: &str, css: &str, js: &str) -> Result<HyperlightBinary> {
        let document = scraper::Html::parse_document(html);
        let mut hls_source = String::new();
        
        // Extract global styles from <style> tags
        let style_selector = scraper::Selector::parse("style").unwrap();
        let mut global_css = css.to_string();
        for style_node in document.select(&style_selector) {
            global_css.push_str(&style_node.text().collect::<String>());
        }

        // Extract global scripts from <script> tags
        let script_selector = scraper::Selector::parse("script").unwrap();
        let mut global_js = js.to_string();
        for script_node in document.select(&script_selector) {
            global_js.push_str(&script_node.text().collect::<String>());
        }

        if !global_css.is_empty() {
            hls_source.push_str(&format!("// Global CSS: {} bytes\n", global_css.len()));
            // In a full implementation, we would parse this CSS and build a style map
        }
        if !global_js.is_empty() {
            hls_source.push_str(&format!("// Global JS: {} bytes\n", global_js.len()));
        }

        hls_source.push_str("element Root {\n");
        
        // Traverse the body
        let body_selector = scraper::Selector::parse("body").unwrap();
        if let Some(body) = document.select(&body_selector).next() {
            for child in body.children() {
                Self::convert_node(child, &mut hls_source, 1);
            }
        } else {
            // Fallback to root children if no body
            for child in document.tree.root().children() {
                Self::convert_node(child, &mut hls_source, 1);
            }
        }
        
        hls_source.push_str("}\n");
        
        // Compile the generated HLS to HLB
        Compiler::compile(&hls_source)
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
                        hls.push_str(&format!("{}  attribute {} \"{}\"\n", indent_str, name, value.replace("\"", "\\\"")));
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
                    hls.push_str(&format!("{}text \"{}\"\n", indent_str, content.replace("\"", "\\\"")));
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
                hls.push_str(&format!("{}attribute {} \"{}\"\n", indent_str, key, value.replace("\"", "\\\"")));
            }
        }
    }

    fn convert_js_event(name: &str, value: &str, hls: &mut String, indent: usize) {
        let indent_str = "  ".repeat(indent);
        let event_name = if name.starts_with("on") {
            &name[2..]
        } else {
            name
        };
        
        // Map common events to HLS event emitters
        hls.push_str(&format!("{}on_{} -> emit(\"{}\", {{ \"script\": \"{}\" }})\n", 
            indent_str, event_name, event_name, value.replace("\"", "\\\"")));
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
}
