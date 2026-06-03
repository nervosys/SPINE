// =============================================================================
// VIRTUAL DOM - SPINE Binary Execution Engine
// =============================================================================

use serde::{Deserialize, Serialize};
use spine_protocol::{Instruction, ProtocolBinOp, ProtocolUnaryOp, SpineBinary, VDomPatch};
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
// HLB RUNTIME - Execute SPINE Binary
// =============================================================================

#[derive(Default)]
pub struct HlbRuntime {
    pub state: HashMap<String, serde_json::Value>,
    pub stack: Vec<serde_json::Value>,
    pub call_stack: Vec<usize>,
}

impl HlbRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Execute a SPINE Binary and produce a Virtual DOM
    pub fn execute(&mut self, binary: &SpineBinary, start_pc: usize) -> ExecutionResult {
        let start_time = std::time::Instant::now();

        let mut vdom = VirtualDom::new();
        let mut events = Vec::new();
        let mut morph_requests = Vec::new();
        let mut decoys = Vec::new();
        let mut latent_streams = Vec::new();
        let mut stats = ExecutionStats::default();

        let mut pc = start_pc;
        let mut element_stack: Vec<u32> = Vec::new();

        while pc < binary.instructions.len() {
            let instruction = &binary.instructions[pc];
            stats.instructions_executed += 1;

            match instruction {
                Instruction::DefineElement { id, tag } => {
                    vdom.add_element(*id, tag);
                    if let Some(&parent_id) = element_stack.last() {
                        vdom.add_child(parent_id, *id);
                    }
                    element_stack.push(*id);
                    stats.elements_created += 1;
                }

                Instruction::SetAttribute { id, key, value } => {
                    vdom.set_attribute(*id, key, value);
                    stats.attributes_set += 1;
                }

                Instruction::AddChild {
                    parent_id,
                    child_id,
                } => {
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

                Instruction::DeclareState { name, initial_json } => {
                    self.state.insert(name.clone(), initial_json.clone());
                }

                Instruction::UpdateState { name, value_json } => {
                    self.state.insert(name.clone(), value_json.clone());
                }

                // --- Control Flow & Stack Operations ---
                Instruction::Push(val) => {
                    self.stack.push(val.clone());
                }
                Instruction::Pop => {
                    self.stack.pop();
                }
                Instruction::Load(name) => {
                    let val = self
                        .state
                        .get(name)
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    self.stack.push(val);
                }
                Instruction::Store(name) => {
                    if let Some(val) = self.stack.pop() {
                        self.state.insert(name.clone(), val);
                    }
                }
                Instruction::BinOp(op) => {
                    if let (Some(right), Some(left)) = (self.stack.pop(), self.stack.pop()) {
                        let result = self.eval_binop(left, *op, right);
                        self.stack.push(result);
                    }
                }
                Instruction::UnaryOp(op) => {
                    if let Some(val) = self.stack.pop() {
                        let result = self.eval_unaryop(*op, val);
                        self.stack.push(result);
                    }
                }
                Instruction::Jump(target) => {
                    pc = *target;
                    continue;
                }
                Instruction::JumpIf(target) => {
                    if let Some(val) = self.stack.pop() {
                        if val.as_bool().unwrap_or(false) {
                            pc = *target;
                            continue;
                        }
                    }
                }
                Instruction::JumpIfNot(target) => {
                    if let Some(val) = self.stack.pop() {
                        if !val.as_bool().unwrap_or(false) {
                            pc = *target;
                            continue;
                        }
                    }
                }
                Instruction::CallTarget(target) => {
                    self.call_stack.push(pc + 1);
                    pc = *target;
                    continue;
                }
                Instruction::Call { name, num_args } => match name.as_str() {
                    "len" => {
                        let arg = self.stack.pop().unwrap_or(serde_json::Value::Null);
                        let len = match arg {
                            serde_json::Value::Array(a) => a.len(),
                            serde_json::Value::String(s) => s.len(),
                            serde_json::Value::Object(m) => m.len(),
                            _ => 0,
                        };
                        self.stack.push(serde_json::json!(len));
                    }
                    "str" => {
                        let arg = self.stack.pop().unwrap_or(serde_json::Value::Null);
                        let s = match arg {
                            serde_json::Value::String(s) => s,
                            _ => arg.to_string(),
                        };
                        self.stack.push(serde_json::Value::String(s));
                    }
                    "num" => {
                        let arg = self.stack.pop().unwrap_or(serde_json::Value::Null);
                        let n = match arg {
                            serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
                            serde_json::Value::String(s) => s.parse().unwrap_or(0.0),
                            serde_json::Value::Bool(b)
                                if b => {
                                    1.0
                                }
                            _ => 0.0,
                        };
                        self.stack.push(serde_json::json!(n));
                    }
                    "print" => {
                        let arg = self.stack.pop().unwrap_or(serde_json::Value::Null);
                        println!("[HLS PRINT] {}", arg);
                        self.stack.push(serde_json::Value::Null);
                    }
                    "morph" => {
                        morph_requests.push(MorphRequest { seed: 12345 });
                        self.stack.push(serde_json::Value::Null);
                    }
                    "decoy" => {
                        decoys.push(DecoyInjection {
                            noise_dimensions: 3,
                            entropy_estimate: 0.5,
                        });
                        self.stack.push(serde_json::Value::Null);
                    }
                    "stream_latent" => {
                        latent_streams.push(LatentStream {
                            dimensions: 3,
                            vector: vec![0.5, 0.5, 0.5],
                        });
                        self.stack.push(serde_json::Value::Null);
                    }
                    "emit" => {
                        let payload = if *num_args > 1 {
                            self.stack.pop().unwrap_or(serde_json::Value::Null)
                        } else {
                            serde_json::Value::Null
                        };
                        let event_name = self
                            .stack
                            .pop()
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .unwrap_or_default();
                        events.push(VEvent {
                            name: event_name,
                            payload,
                            timestamp_ns: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_nanos() as u64,
                        });
                        self.stack.push(serde_json::Value::Null);
                    }
                    "list" => {
                        let mut items = Vec::new();
                        for _ in 0..*num_args {
                            if let Some(val) = self.stack.pop() {
                                items.push(val);
                            }
                        }
                        items.reverse();
                        self.stack.push(serde_json::Value::Array(items));
                    }
                    "object" => {
                        let mut map = serde_json::Map::new();
                        for _ in 0..(*num_args / 2) {
                            let val = self.stack.pop().unwrap_or(serde_json::Value::Null);
                            let key = self
                                .stack
                                .pop()
                                .and_then(|v| v.as_str().map(|s| s.to_string()))
                                .unwrap_or_default();
                            map.insert(key, val);
                        }
                        self.stack.push(serde_json::Value::Object(map));
                    }
                    _ => {
                        self.stack.push(serde_json::Value::Null);
                    }
                },
                Instruction::Return => {
                    if let Some(return_pc) = self.call_stack.pop() {
                        pc = return_pc;
                        continue;
                    } else {
                        break;
                    }
                }

                // --- Stack-based DOM Operations ---
                Instruction::DefineElementFromStack { id } => {
                    let tag = self
                        .stack
                        .pop()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| "div".to_string());
                    vdom.add_element(*id, &tag);
                    if let Some(&parent_id) = element_stack.last() {
                        vdom.add_child(parent_id, *id);
                    }
                    element_stack.push(*id);
                }
                Instruction::SetAttributeFromStack { id, key } => {
                    let val = self
                        .stack
                        .pop()
                        .map(|v| {
                            v.as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| v.to_string())
                        })
                        .unwrap_or_default();
                    vdom.set_attribute(*id, key, &val);
                }
                Instruction::AddChildFromStack {
                    parent_id,
                    child_id,
                } => {
                    vdom.add_child(*parent_id, *child_id);
                }
                Instruction::EmitEventFromStack { name } => {
                    let payload = self.stack.pop().unwrap_or(serde_json::Value::Null);
                    events.push(VEvent {
                        name: name.clone(),
                        payload,
                        timestamp_ns: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64,
                    });
                }
                Instruction::DefineTextFromStack => {
                    let text = self
                        .stack
                        .pop()
                        .map(|v| {
                            v.as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| v.to_string())
                        })
                        .unwrap_or_default();
                    let id = 1000 + stats.instructions_executed as u32;
                    vdom.add_element(id, "text");
                    vdom.set_attribute(id, "content", &text);
                    if let Some(&parent_id) = element_stack.last() {
                        vdom.add_child(parent_id, id);
                    }
                }
                Instruction::DeclareStateFromStack { name } => {
                    let val = self.stack.pop().unwrap_or(serde_json::Value::Null);
                    self.state.insert(name.clone(), val);
                }
                Instruction::UpdateStateFromStack { name } => {
                    let val = self.stack.pop().unwrap_or(serde_json::Value::Null);
                    self.state.insert(name.clone(), val);
                }
                Instruction::NavigateFromStack => {
                    self.stack.pop();
                }
                Instruction::SearchFromStack => {
                    self.stack.pop();
                }
                Instruction::StoreKnowledgeFromStack { .. } => {
                    self.stack.pop();
                    self.stack.pop();
                }
                Instruction::QueryKnowledgeFromStack { .. } => {
                    self.stack.pop();
                }
            }
            pc += 1;
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

    fn eval_binop(
        &self,
        left: serde_json::Value,
        op: ProtocolBinOp,
        right: serde_json::Value,
    ) -> serde_json::Value {
        match op {
            ProtocolBinOp::Add => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    serde_json::json!(l + r)
                } else if let (Some(l), Some(r)) = (left.as_str(), right.as_str()) {
                    serde_json::json!(format!("{}{}", l, r))
                } else {
                    serde_json::Value::Null
                }
            }
            ProtocolBinOp::Sub => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    serde_json::json!(l - r)
                } else {
                    serde_json::Value::Null
                }
            }
            ProtocolBinOp::Mul => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    serde_json::json!(l * r)
                } else {
                    serde_json::Value::Null
                }
            }
            ProtocolBinOp::Div => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    serde_json::json!(l / r)
                } else {
                    serde_json::Value::Null
                }
            }
            ProtocolBinOp::Eq => serde_json::json!(left == right),
            ProtocolBinOp::Ne => serde_json::json!(left != right),
            ProtocolBinOp::Lt => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    serde_json::json!(l < r)
                } else {
                    serde_json::Value::Null
                }
            }
            ProtocolBinOp::Gt => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    serde_json::json!(l > r)
                } else {
                    serde_json::Value::Null
                }
            }
            ProtocolBinOp::Le => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    serde_json::json!(l <= r)
                } else {
                    serde_json::Value::Null
                }
            }
            ProtocolBinOp::Ge => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    serde_json::json!(l >= r)
                } else {
                    serde_json::Value::Null
                }
            }
            ProtocolBinOp::Mod => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    serde_json::json!(l % r)
                } else {
                    serde_json::Value::Null
                }
            }
            ProtocolBinOp::And => serde_json::json!(
                left.as_bool().unwrap_or(false) && right.as_bool().unwrap_or(false)
            ),
            ProtocolBinOp::Or => serde_json::json!(
                left.as_bool().unwrap_or(false) || right.as_bool().unwrap_or(false)
            ),
            ProtocolBinOp::Concat => {
                let l_str = if left.is_string() {
                    left.as_str().unwrap().to_string()
                } else {
                    left.to_string()
                };
                let r_str = if right.is_string() {
                    right.as_str().unwrap().to_string()
                } else {
                    right.to_string()
                };
                serde_json::json!(format!("{}{}", l_str, r_str))
            }
        }
    }

    fn eval_unaryop(&self, op: ProtocolUnaryOp, val: serde_json::Value) -> serde_json::Value {
        match op {
            ProtocolUnaryOp::Not => serde_json::json!(!val.as_bool().unwrap_or(false)),
            ProtocolUnaryOp::Neg => {
                if let Some(n) = val.as_f64() {
                    serde_json::json!(-n)
                } else {
                    serde_json::Value::Null
                }
            }
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
