// =============================================================================
// VIRTUAL DOM - Hyperlight Binary Execution Engine
// =============================================================================

use hyperlight_protocol::{HyperlightBinary, Instruction, VDomPatch};
use hyperlight_wasm::{WasmExecutionResult, WasmElement};
use serde::{Deserialize, Serialize};
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

/// Events emitted during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VEvent {
    pub name: String,
    pub payload: serde_json::Value,
    pub timestamp_ns: u64,
}

/// Protocol morphing request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MorphRequest {
    pub seed: u64,
}

/// Decoy traffic injected during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoyInjection {
    pub noise_dimensions: usize,
    pub entropy_estimate: f64,
}

/// Latent vector streamed from the program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatentStream {
    pub vector: Vec<f32>,
    pub dimensions: usize,
}

/// Execution result containing all outputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// The built Virtual DOM
    pub vdom: VirtualDom,
    /// Events emitted during execution
    pub events: Vec<VEvent>,
    /// Protocol morph requests
    pub morph_requests: Vec<MorphRequest>,
    /// Decoy injections
    pub decoys: Vec<DecoyInjection>,
    /// Latent streams
    pub latent_streams: Vec<LatentStream>,
    /// Execution statistics
    pub stats: ExecutionStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionStats {
    pub instructions_executed: usize,
    pub elements_created: usize,
    pub attributes_set: usize,
    pub events_emitted: usize,
    pub execution_time_us: u64,
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
        self.nodes.insert(id, VNode {
            id,
            tag: tag.to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
            parent_id: 0,
        });
    }
    
    /// Set an attribute on a node
    pub fn set_attribute(&mut self, id: u32, key: &str, value: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.attributes.insert(key.to_string(), value.to_string());
        }
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
        let orphans: Vec<u32> = self.nodes.iter()
            .filter(|(_, node)| node.parent_id == 0)
            .map(|(&id, _)| id)
            .collect();
        
        self.roots = orphans;
    }
    
    /// Convert to a JSON-friendly tree structure
    pub fn to_tree(&self) -> serde_json::Value {
        let root_trees: Vec<serde_json::Value> = self.roots.iter()
            .filter_map(|&id| self.node_to_tree(id))
            .collect();
        
        serde_json::json!({
            "type": "vdom",
            "roots": root_trees
        })
    }
    
    fn node_to_tree(&self, id: u32) -> Option<serde_json::Value> {
        let node = self.nodes.get(&id)?;
        
        let children: Vec<serde_json::Value> = node.children.iter()
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
        let Some(node) = self.nodes.get(&id) else { return };
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
// HLB RUNTIME - Execute Hyperlight Binary
// =============================================================================

pub struct HlbRuntime;

impl HlbRuntime {
    /// Execute a Hyperlight Binary and produce a Virtual DOM
    pub fn execute(binary: &HyperlightBinary) -> ExecutionResult {
        let start_time = std::time::Instant::now();
        
        let mut vdom = VirtualDom::new();
        let mut events = Vec::new();
        let mut morph_requests = Vec::new();
        let mut decoys = Vec::new();
        let mut latent_streams = Vec::new();
        let mut stats = ExecutionStats::default();
        
        for instruction in &binary.instructions {
            stats.instructions_executed += 1;
            
            match instruction {
                Instruction::DefineElement { id, tag } => {
                    vdom.add_element(*id, tag);
                    stats.elements_created += 1;
                }
                
                Instruction::SetAttribute { id, key, value } => {
                    vdom.set_attribute(*id, key, value);
                    stats.attributes_set += 1;
                }
                
                Instruction::AddChild { parent_id, child_id } => {
                    vdom.add_child(*parent_id, *child_id);
                }
                
                Instruction::EmitEvent { name, payload } => {
                    let timestamp_ns = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(0);
                    
                    events.push(VEvent {
                        name: name.clone(),
                        payload: payload.clone(),
                        timestamp_ns,
                    });
                    stats.events_emitted += 1;
                }
                
                Instruction::StreamLatent { vector } => {
                    latent_streams.push(LatentStream {
                        dimensions: vector.len(),
                        vector: vector.clone(),
                    });
                }
                
                Instruction::MorphProtocol { seed } => {
                    morph_requests.push(MorphRequest { seed: *seed });
                }
                
                Instruction::Decoy { noise } => {
                    let entropy = Self::estimate_entropy(noise);
                    decoys.push(DecoyInjection {
                        noise_dimensions: noise.len(),
                        entropy_estimate: entropy,
                    });
                }
            }
        }
        
        // Finalize VDOM (identify root nodes)
        vdom.finalize();
        
        stats.execution_time_us = start_time.elapsed().as_micros() as u64;
        
        ExecutionResult {
            vdom,
            events,
            morph_requests,
            decoys,
            latent_streams,
            stats,
        }
    }
    
    /// Estimate Shannon entropy of noise vector
    fn estimate_entropy(noise: &[f32]) -> f64 {
        if noise.is_empty() {
            return 0.0;
        }
        
        // Quantize to bins for entropy estimation
        let num_bins = 16;
        let min_val = noise.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_val = noise.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let range = (max_val - min_val).max(1e-6);
        
        let mut bins = vec![0usize; num_bins];
        for &v in noise {
            let idx = (((v - min_val) / range) * (num_bins - 1) as f32).floor() as usize;
            bins[idx.min(num_bins - 1)] += 1;
        }
        
        let total = noise.len() as f64;
        let mut entropy = 0.0;
        for count in bins {
            if count > 0 {
                let p = count as f64 / total;
                entropy -= p * p.log2();
            }
        }
        
        entropy
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
                        patches.push(VDomPatch::RemoveAttr { id, key: key.clone() });
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
