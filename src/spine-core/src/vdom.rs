// =============================================================================
// VIRTUAL DOM - SPINE Binary Execution Engine
// =============================================================================

use serde::{Deserialize, Serialize};
use spine_protocol::VDomPatch;
use spine_wasm::WasmExecutionResult;
use std::collections::HashMap;

/// Virtual DOM node representing an element or text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VNode {
    /// Unique identifier
    pub id: u32,
    /// Tag name or "text" for text nodes
    pub tag: String,
    /// Attributes map
    pub attributes: HashMap<String, String>,
    /// Child node IDs (for ordering)
    pub children: Vec<u32>,
    /// Parent node ID (0 = root)
    pub parent_id: u32,
}

/// The Virtual DOM - a lightweight tree representation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VirtualDom {
    /// All nodes indexed by ID
    pub nodes: HashMap<u32, VNode>,
    /// Root node IDs (top-level elements)
    pub roots: Vec<u32>,
}

impl VirtualDom {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a Virtual DOM from WASM execution results
    pub fn from_wasm(result: &WasmExecutionResult) -> Self {
        let mut vdom = Self::new();

        // First pass: create all nodes
        for element in &result.elements {
            vdom.add_element(element.id, &element.tag);
            if let Some(node) = vdom.nodes.get_mut(&element.id) {
                node.attributes = element.attributes.clone();
            }
        }

        // Second pass: establish parent-child relationships
        for element in &result.elements {
            if let Some(parent_id) = element.parent_id {
                vdom.add_child(parent_id, element.id);
            }
        }

        vdom.finalize();
        vdom
    }

    /// Add a new element node
    pub fn add_element(&mut self, id: u32, tag: &str) {
        self.nodes.insert(
            id,
            VNode {
                id,
                tag: tag.to_string(),
                attributes: HashMap::new(),
                children: Vec::new(),
                parent_id: 0,
            },
        );
    }

    /// Add a child to a parent node
    pub fn add_child(&mut self, parent_id: u32, child_id: u32) {
        // Update parent's children list
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            if !parent.children.contains(&child_id) {
                parent.children.push(child_id);
            }
        }

        // Update child's parent reference
        if let Some(child) = self.nodes.get_mut(&child_id) {
            child.parent_id = parent_id;
        }
    }

    /// Mark a node as a root if it has no parent
    pub fn finalize(&mut self) {
        let orphans: Vec<u32> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.parent_id == 0)
            .map(|(&id, _)| id)
            .collect();

        self.roots = orphans;
    }

    /// Convert to a JSON-friendly tree structure
    pub fn to_tree(&self) -> serde_json::Value {
        let root_trees: Vec<serde_json::Value> = self
            .roots
            .iter()
            .filter_map(|&id| self.node_to_tree(id))
            .collect();

        serde_json::json!({
            "type": "vdom",
            "roots": root_trees
        })
    }

    fn node_to_tree(&self, id: u32) -> Option<serde_json::Value> {
        let node = self.nodes.get(&id)?;

        let children: Vec<serde_json::Value> = node
            .children
            .iter()
            .filter_map(|&child_id| self.node_to_tree(child_id))
            .collect();

        Some(serde_json::json!({
            "id": node.id,
            "tag": node.tag,
            "attributes": node.attributes,
            "children": children
        }))
    }

    /// Generate a unified representation (UR) from the VDOM
    pub fn to_ur(&self) -> String {
        let mut lines = Vec::new();
        for &root_id in &self.roots {
            self.render_node_to_ur(&mut lines, root_id, 0);
        }
        lines.join("\n")
    }

    fn render_node_to_ur(&self, lines: &mut Vec<String>, id: u32, depth: usize) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        let indent = "  ".repeat(depth);

        // Text nodes
        if node.tag == "text" {
            if let Some(content) = node.attributes.get("content") {
                lines.push(format!("{}text: \"{}\"", indent, content));
            }
            return;
        }

        // Element nodes
        let mut attr_str = String::new();
        for (key, value) in &node.attributes {
            attr_str.push_str(&format!(" {}=\"{}\"", key, value));
        }

        lines.push(format!("{}<{}{}>", indent, node.tag, attr_str));

        for &child_id in &node.children {
            self.render_node_to_ur(lines, child_id, depth + 1);
        }

        lines.push(format!("{}</{}>", indent, node.tag));
    }
}

// =============================================================================
// DIFFERENTIAL RENDERING - Compute VDOM diffs
// =============================================================================

impl VirtualDom {
    /// Compute the minimal set of patches to transform `old` into `self`
    pub fn diff(&self, old: &VirtualDom) -> Vec<VDomPatch> {
        let mut patches = Vec::new();

        // Find removed nodes
        for &id in old.nodes.keys() {
            if !self.nodes.contains_key(&id) {
                patches.push(VDomPatch::Remove { id });
            }
        }

        // Find new and modified nodes
        for (&id, new_node) in &self.nodes {
            if let Some(old_node) = old.nodes.get(&id) {
                // Node exists - check for attribute changes
                for (key, new_value) in &new_node.attributes {
                    match old_node.attributes.get(key) {
                        Some(old_value) if old_value != new_value => {
                            patches.push(VDomPatch::SetAttr {
                                id,
                                key: key.clone(),
                                value: new_value.clone(),
                            });
                        }
                        None => {
                            patches.push(VDomPatch::SetAttr {
                                id,
                                key: key.clone(),
                                value: new_value.clone(),
                            });
                        }
                        _ => {}
                    }
                }

                // Check for removed attributes
                for key in old_node.attributes.keys() {
                    if !new_node.attributes.contains_key(key) {
                        patches.push(VDomPatch::RemoveAttr {
                            id,
                            key: key.clone(),
                        });
                    }
                }

                // Check for children changes
                if new_node.children != old_node.children {
                    patches.push(VDomPatch::ReorderChildren {
                        parent_id: id,
                        order: new_node.children.clone(),
                    });
                }
            } else {
                // New node
                patches.push(VDomPatch::Create {
                    id,
                    tag: new_node.tag.clone(),
                });

                for (key, value) in &new_node.attributes {
                    patches.push(VDomPatch::SetAttr {
                        id,
                        key: key.clone(),
                        value: value.clone(),
                    });
                }

                if new_node.parent_id != 0 {
                    patches.push(VDomPatch::AppendChild {
                        parent_id: new_node.parent_id,
                        child_id: id,
                    });
                }
            }
        }

        patches
    }
}
