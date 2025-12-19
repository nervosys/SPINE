// =============================================================================
// HYPERLIGHT WASM - WebAssembly Runtime for HLB Execution
// =============================================================================
//
// This module compiles Hyperlight Binary (HLB) instructions to WebAssembly
// for near-native execution speed. It provides:
//
// 1. HLB → WAT (WebAssembly Text) transpilation
// 2. WAT → WASM compilation via wasmtime
// 3. Sandboxed execution with memory isolation
// 4. Host function bindings for DOM operations
//
// =============================================================================

use anyhow::{Result, Context};
use hyperlight_protocol::{HyperlightBinary, Instruction, ProtocolBinOp, ProtocolUnaryOp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::*;

// =============================================================================
// WASM RUNTIME TYPES
// =============================================================================

/// Result of WASM execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmExecutionResult {
    /// Elements created during execution
    pub elements: Vec<WasmElement>,
    /// Events emitted
    pub events: Vec<WasmEvent>,
    /// Latent vectors streamed
    pub latent_streams: Vec<Vec<f32>>,
    /// Execution statistics
    pub stats: WasmStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmElement {
    pub id: u32,
    pub tag: String,
    pub attributes: HashMap<String, String>,
    pub parent_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmEvent {
    pub name: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WasmStats {
    pub compile_time_us: u64,
    pub execution_time_us: u64,
    pub wasm_size_bytes: usize,
    pub memory_used_bytes: usize,
    pub instructions_executed: usize,
}

// =============================================================================
// HOST STATE - Shared between WASM and host
// =============================================================================

#[derive(Default)]
struct HostState {
    elements: Vec<WasmElement>,
    events: Vec<WasmEvent>,
    latent_streams: Vec<Vec<f32>>,
    instruction_count: usize,
    current_latent: Vec<f32>,
    
    // New stack-based state
    value_stack: Vec<serde_json::Value>,
    variables: HashMap<String, serde_json::Value>,
    element_stack: Vec<u32>,
}

// =============================================================================
// HLB TO WAT COMPILER
// =============================================================================

/// Compiles HLB instructions to WebAssembly Text format
pub struct HlbToWatCompiler;

impl HlbToWatCompiler {
    /// Compile HLB binary to WAT (WebAssembly Text)
    pub fn compile(binary: &HyperlightBinary) -> String {
        let mut wat = String::new();
        
        // Module header
        wat.push_str("(module\n");
        
        // Import host functions
        wat.push_str("  ;; Host function imports\n");
        wat.push_str("  (import \"env\" \"define_element\" (func $define_element (param i32 i32 i32)))\n");
        wat.push_str("  (import \"env\" \"set_attribute\" (func $set_attribute (param i32 i32 i32 i32 i32)))\n");
        wat.push_str("  (import \"env\" \"add_child\" (func $add_child (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"emit_event\" (func $emit_event (param i32 i32 i32 i32)))\n");
        wat.push_str("  (import \"env\" \"stream_latent\" (func $stream_latent (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"morph_protocol\" (func $morph_protocol (param i64)))\n");
        wat.push_str("  (import \"env\" \"inject_decoy\" (func $inject_decoy (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"declare_state\" (func $declare_state (param i32 i32 i32 i32)))\n");
        wat.push_str("  (import \"env\" \"update_state\" (func $update_state (param i32 i32 i32 i32)))\n");
        
        // New stack-based host functions
        wat.push_str("  (import \"env\" \"push_value\" (func $push_value (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"pop_value\" (func $pop_value))\n");
        wat.push_str("  (import \"env\" \"load_var\" (func $load_var (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"store_var\" (func $store_var (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"bin_op\" (func $bin_op (param i32)))\n");
        wat.push_str("  (import \"env\" \"unary_op\" (func $unary_op (param i32)))\n");
        wat.push_str("  (import \"env\" \"call_func\" (func $call_func (param i32 i32 i32)))\n");
        wat.push_str("  (import \"env\" \"call_target\" (func $call_target (param i32)))\n");
        
        // New stack-based DOM host functions
        wat.push_str("  (import \"env\" \"define_element_from_stack\" (func $define_element_from_stack (param i32)))\n");
        wat.push_str("  (import \"env\" \"set_attribute_from_stack\" (func $set_attribute_from_stack (param i32 i32 i32)))\n");
        wat.push_str("  (import \"env\" \"emit_event_from_stack\" (func $emit_event_from_stack (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"define_text_from_stack\" (func $define_text_from_stack))\n");
        wat.push_str("  (import \"env\" \"declare_state_from_stack\" (func $declare_state_from_stack (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"update_state_from_stack\" (func $update_state_from_stack (param i32 i32)))\n");
        wat.push_str("\n");
        
        // Memory for string data
        wat.push_str("  ;; Linear memory for string storage\n");
        wat.push_str("  (memory (export \"memory\") 1)\n");
        wat.push_str("\n");
        
        // Build data section with all strings
        let mut data_offset = 0u32;
        let mut string_offsets: HashMap<String, (u32, u32)> = HashMap::new();
        let mut data_section = String::new();
        
        for instruction in &binary.instructions {
            match instruction {
                Instruction::DefineElement { tag, .. } => {
                    if !string_offsets.contains_key(tag) {
                        let len = tag.len() as u32;
                        string_offsets.insert(tag.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(tag)
                        ));
                        data_offset += len + 1; // +1 for null terminator space
                    }
                }
                Instruction::SetAttribute { key, value, .. } => {
                    for s in [key, value] {
                        if !string_offsets.contains_key(s) {
                            let len = s.len() as u32;
                            string_offsets.insert(s.clone(), (data_offset, len));
                            data_section.push_str(&format!(
                                "  (data (i32.const {}) \"{}\")\n",
                                data_offset,
                                escape_wat_string(s)
                            ));
                            data_offset += len + 1;
                        }
                    }
                }
                Instruction::EmitEvent { name, payload } => {
                    if !string_offsets.contains_key(name) {
                        let len = name.len() as u32;
                        string_offsets.insert(name.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(name)
                        ));
                        data_offset += len + 1;
                    }
                    let payload_str = payload.to_string();
                    if !string_offsets.contains_key(&payload_str) {
                        let len = payload_str.len() as u32;
                        string_offsets.insert(payload_str.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(&payload_str)
                        ));
                        data_offset += len + 1;
                    }
                }
                Instruction::DeclareState { name, initial_json } => {
                    for s in [name, &initial_json.to_string()] {
                        if !string_offsets.contains_key(s) {
                            let len = s.len() as u32;
                            string_offsets.insert(s.clone(), (data_offset, len));
                            data_section.push_str(&format!(
                                "  (data (i32.const {}) \"{}\")\n",
                                data_offset,
                                escape_wat_string(s)
                            ));
                            data_offset += len + 1;
                        }
                    }
                }
                Instruction::UpdateState { name, value_json } => {
                    for s in [name, &value_json.to_string()] {
                        if !string_offsets.contains_key(s) {
                            let len = s.len() as u32;
                            string_offsets.insert(s.clone(), (data_offset, len));
                            data_section.push_str(&format!(
                                "  (data (i32.const {}) \"{}\")\n",
                                data_offset,
                                escape_wat_string(s)
                            ));
                            data_offset += len + 1;
                        }
                    }
                }
                Instruction::Push(val) => {
                    let s = val.to_string();
                    if !string_offsets.contains_key(&s) {
                        let len = s.len() as u32;
                        string_offsets.insert(s.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(&s)
                        ));
                        data_offset += len + 1;
                    }
                }
                Instruction::Load(name) | Instruction::Store(name) => {
                    if !string_offsets.contains_key(name) {
                        let len = name.len() as u32;
                        string_offsets.insert(name.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(name)
                        ));
                        data_offset += len + 1;
                    }
                }
                Instruction::Call { name, .. } => {
                    if !string_offsets.contains_key(name) {
                        let len = name.len() as u32;
                        string_offsets.insert(name.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(name)
                        ));
                        data_offset += len + 1;
                    }
                }
                Instruction::SetAttributeFromStack { key, .. } => {
                    if !string_offsets.contains_key(key) {
                        let len = key.len() as u32;
                        string_offsets.insert(key.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(key)
                        ));
                        data_offset += len + 1;
                    }
                }
                Instruction::EmitEventFromStack { name } => {
                    if !string_offsets.contains_key(name) {
                        let len = name.len() as u32;
                        string_offsets.insert(name.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(name)
                        ));
                        data_offset += len + 1;
                    }
                }
                Instruction::DeclareStateFromStack { name } | Instruction::UpdateStateFromStack { name } => {
                    if !string_offsets.contains_key(name) {
                        let len = name.len() as u32;
                        string_offsets.insert(name.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(name)
                        ));
                        data_offset += len + 1;
                    }
                }
                _ => {}
            }
        }
        
        wat.push_str("  ;; String data\n");
        wat.push_str(&data_section);
        wat.push_str("\n");
        
        // Main execution function
        wat.push_str("  ;; Main HLB execution function\n");
        wat.push_str("  (func (export \"execute\") (result i32)\n");
        
        // Generate instructions
        for instruction in &binary.instructions {
            match instruction {
                Instruction::DefineElement { id, tag } => {
                    let (offset, len) = string_offsets.get(tag).unwrap();
                    wat.push_str(&format!(
                        "    ;; DefineElement id={} tag=\"{}\"\n",
                        id, tag
                    ));
                    wat.push_str(&format!(
                        "    (call $define_element (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        id, offset, len
                    ));
                }
                
                Instruction::SetAttribute { id, key, value } => {
                    let (key_offset, key_len) = string_offsets.get(key).unwrap();
                    let (val_offset, val_len) = string_offsets.get(value).unwrap();
                    wat.push_str(&format!(
                        "    ;; SetAttribute id={} key=\"{}\" value=\"{}\"\n",
                        id, key, value
                    ));
                    wat.push_str(&format!(
                        "    (call $set_attribute (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        id, key_offset, key_len, val_offset, val_len
                    ));
                }
                
                Instruction::AddChild { parent_id, child_id } => {
                    wat.push_str(&format!(
                        "    ;; AddChild parent={} child={}\n",
                        parent_id, child_id
                    ));
                    wat.push_str(&format!(
                        "    (call $add_child (i32.const {}) (i32.const {}))\n",
                        parent_id, child_id
                    ));
                }
                
                Instruction::EmitEvent { name, payload } => {
                    let (name_offset, name_len) = string_offsets.get(name).unwrap();
                    let payload_str = payload.to_string();
                    let (payload_offset, payload_len) = string_offsets.get(&payload_str).unwrap();
                    wat.push_str(&format!(
                        "    ;; EmitEvent name=\"{}\"\n",
                        name
                    ));
                    wat.push_str(&format!(
                        "    (call $emit_event (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        name_offset, name_len, payload_offset, payload_len
                    ));
                }
                
                Instruction::StreamLatent { vector } => {
                    // Store vector in memory and call host
                    let vec_offset = data_offset;
                    let vec_len = vector.len() as u32;
                    wat.push_str(&format!(
                        "    ;; StreamLatent {} dimensions\n",
                        vec_len
                    ));
                    wat.push_str(&format!(
                        "    (call $stream_latent (i32.const {}) (i32.const {}))\n",
                        vec_offset, vec_len
                    ));
                }
                
                Instruction::MorphProtocol { seed } => {
                    wat.push_str(&format!(
                        "    ;; MorphProtocol seed={}\n",
                        seed
                    ));
                    wat.push_str(&format!(
                        "    (call $morph_protocol (i64.const {}))\n",
                        seed
                    ));
                }
                
                Instruction::DeclareState { name, initial_json } => {
                    let (name_offset, name_len) = string_offsets.get(name).unwrap();
                    let initial_str = initial_json.to_string();
                    let (val_offset, val_len) = string_offsets.get(&initial_str).unwrap();
                    wat.push_str(&format!(
                        "    ;; DeclareState name=\"{}\"\n",
                        name
                    ));
                    wat.push_str(&format!(
                        "    (call $declare_state (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        name_offset, name_len, val_offset, val_len
                    ));
                }
                
                Instruction::UpdateState { name, value_json } => {
                    let (name_offset, name_len) = string_offsets.get(name).unwrap();
                    let value_str = value_json.to_string();
                    let (val_offset, val_len) = string_offsets.get(&value_str).unwrap();
                    wat.push_str(&format!(
                        "    ;; UpdateState name=\"{}\"\n",
                        name
                    ));
                    wat.push_str(&format!(
                        "    (call $update_state (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        name_offset, name_len, val_offset, val_len
                    ));
                }
                
                Instruction::Decoy { noise } => {
                    let noise_len = noise.len() as u32;
                    wat.push_str(&format!(
                        "    ;; Decoy {} dimensions\n",
                        noise_len
                    ));
                    wat.push_str(&format!(
                        "    (call $inject_decoy (i32.const 0) (i32.const {}))\n",
                        noise_len
                    ));
                }
                
                // --- Control Flow & Stack Operations ---
                Instruction::Push(val) => {
                    let val_str = val.to_string();
                    let (offset, len) = string_offsets.get(&val_str).unwrap();
                    wat.push_str(&format!("    (call $push_value (i32.const {}) (i32.const {}))\n", offset, len));
                }
                Instruction::Pop => {
                    wat.push_str("    (call $pop_value)\n");
                }
                Instruction::Load(name) => {
                    let (offset, len) = string_offsets.get(name).unwrap();
                    wat.push_str(&format!("    (call $load_var (i32.const {}) (i32.const {}))\n", offset, len));
                }
                Instruction::Store(name) => {
                    let (offset, len) = string_offsets.get(name).unwrap();
                    wat.push_str(&format!("    (call $store_var (i32.const {}) (i32.const {}))\n", offset, len));
                }
                Instruction::BinOp(op) => {
                    wat.push_str(&format!("    (call $bin_op (i32.const {}))\n", *op as i32));
                }
                Instruction::UnaryOp(op) => {
                    wat.push_str(&format!("    (call $unary_op (i32.const {}))\n", *op as i32));
                }
                Instruction::Jump(target) => {
                    // This is tricky in WAT without labels. 
                    // For now, we'll just emit a comment. 
                    // Real jump support requires a more complex WAT structure (block/loop).
                    wat.push_str(&format!("    ;; Jump to {}\n", target));
                }
                Instruction::JumpIf(target) => {
                    wat.push_str(&format!("    ;; JumpIf to {}\n", target));
                }
                Instruction::JumpIfNot(target) => {
                    wat.push_str(&format!("    ;; JumpIfNot to {}\n", target));
                }
                Instruction::Call { name, num_args } => {
                    let (offset, len) = string_offsets.get(name).unwrap();
                    wat.push_str(&format!("    (call $call_func (i32.const {}) (i32.const {}) (i32.const {}))\n", offset, len, num_args));
                }
                Instruction::CallTarget(target) => {
                    wat.push_str(&format!("    (call $call_target (i32.const {}))\n", target));
                }
                Instruction::Return => {
                    wat.push_str("    return\n");
                }
                
                // --- Stack-based DOM Operations ---
                Instruction::DefineElementFromStack { id } => {
                    wat.push_str(&format!("    (call $define_element_from_stack (i32.const {}))\n", id));
                }
                Instruction::SetAttributeFromStack { id, key } => {
                    let (offset, len) = string_offsets.get(key).unwrap();
                    wat.push_str(&format!("    (call $set_attribute_from_stack (i32.const {}) (i32.const {}) (i32.const {}))\n", id, offset, len));
                }
                Instruction::AddChildFromStack { parent_id, child_id } => {
                    wat.push_str(&format!("    (call $add_child (i32.const {}) (i32.const {}))\n", parent_id, child_id));
                }
                Instruction::EmitEventFromStack { name } => {
                    let (offset, len) = string_offsets.get(name).unwrap();
                    wat.push_str(&format!("    (call $emit_event_from_stack (i32.const {}) (i32.const {}))\n", offset, len));
                }
                Instruction::DefineTextFromStack => {
                    wat.push_str("    (call $define_text_from_stack)\n");
                }
                Instruction::DeclareStateFromStack { name } => {
                    let (offset, len) = string_offsets.get(name).unwrap();
                    wat.push_str(&format!("    (call $declare_state_from_stack (i32.const {}) (i32.const {}))\n", offset, len));
                }
                Instruction::UpdateStateFromStack { name } => {
                    let (offset, len) = string_offsets.get(name).unwrap();
                    wat.push_str(&format!("    (call $update_state_from_stack (i32.const {}) (i32.const {}))\n", offset, len));
                }
            }
        }
        
        // Return number of instructions executed
        wat.push_str(&format!(
            "    (i32.const {})\n",
            binary.instructions.len()
        ));
        wat.push_str("  )\n");
        wat.push_str(")\n");
        
        wat
    }
}

fn escape_wat_string(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_ascii_control() => {
                result.push_str(&format!("\\{:02x}", c as u8));
            }
            _ => result.push(c),
        }
    }
    result
}

// =============================================================================
// WASM RUNTIME
// =============================================================================

/// WebAssembly runtime for executing HLB programs
pub struct WasmRuntime {
    engine: Engine,
}

impl WasmRuntime {
    /// Create a new WASM runtime
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.wasm_bulk_memory(true);
        config.wasm_multi_value(true);
        
        let engine = Engine::new(&config)?;
        
        Ok(Self { engine })
    }
    
    /// Execute an HLB binary using WebAssembly
    pub fn execute(&self, binary: &HyperlightBinary) -> Result<WasmExecutionResult> {
        let compile_start = std::time::Instant::now();
        
        // Compile HLB to WAT
        let wat = HlbToWatCompiler::compile(binary);
        
        // Parse WAT to WASM binary
        let wasm_bytes = wat::parse_str(&wat)
            .context("Failed to parse WAT")?;
        
        let wasm_size = wasm_bytes.len();
        
        // Compile WASM module
        let module = Module::new(&self.engine, &wasm_bytes)
            .context("Failed to compile WASM module")?;
        
        let compile_time = compile_start.elapsed();
        
        // Create store with host state
        let host_state = Arc::new(Mutex::new(HostState::default()));
        let mut store = Store::new(&self.engine, host_state.clone());
        
        // Create linker with host functions
        let mut linker = Linker::new(&self.engine);
        
        // Define host functions
        Self::define_host_functions(&mut linker, host_state.clone())?;
        
        // Instantiate module
        let instance = linker.instantiate(&mut store, &module)
            .context("Failed to instantiate WASM module")?;
        
        // Get execute function
        let execute_fn = instance.get_typed_func::<(), i32>(&mut store, "execute")
            .context("Failed to get execute function")?;
        
        // Execute
        let exec_start = std::time::Instant::now();
        let instruction_count = execute_fn.call(&mut store, ())
            .context("WASM execution failed")?;
        let exec_time = exec_start.elapsed();
        
        // Extract results from host state
        let state = host_state.lock().unwrap();
        
        Ok(WasmExecutionResult {
            elements: state.elements.clone(),
            events: state.events.clone(),
            latent_streams: state.latent_streams.clone(),
            stats: WasmStats {
                compile_time_us: compile_time.as_micros() as u64,
                execution_time_us: exec_time.as_micros() as u64,
                wasm_size_bytes: wasm_size,
                memory_used_bytes: 65536, // 1 page = 64KB
                instructions_executed: instruction_count as usize,
            },
        })
    }
    
    fn define_host_functions(
        linker: &mut Linker<Arc<Mutex<HostState>>>,
        _state: Arc<Mutex<HostState>>,
    ) -> Result<()> {
        // define_element(id: i32, tag_ptr: i32, tag_len: i32)
        linker.func_wrap(
            "env",
            "define_element",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, id: i32, tag_ptr: i32, tag_len: i32| {
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .expect("memory export");
                
                let mut buf = vec![0u8; tag_len as usize];
                memory.read(&caller, tag_ptr as usize, &mut buf).expect("read memory");
                let tag = String::from_utf8_lossy(&buf).to_string();
                
                let state = caller.data().lock().unwrap();
                drop(state);
                
                let mut state = caller.data().lock().unwrap();
                state.elements.push(WasmElement {
                    id: id as u32,
                    tag,
                    attributes: HashMap::new(),
                    parent_id: None,
                });
                state.instruction_count += 1;
            },
        )?;
        
        // set_attribute(id: i32, key_ptr: i32, key_len: i32, val_ptr: i32, val_len: i32)
        linker.func_wrap(
            "env",
            "set_attribute",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, id: i32, key_ptr: i32, key_len: i32, val_ptr: i32, val_len: i32| {
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .expect("memory export");
                
                let mut key_buf = vec![0u8; key_len as usize];
                let mut val_buf = vec![0u8; val_len as usize];
                memory.read(&caller, key_ptr as usize, &mut key_buf).expect("read key");
                memory.read(&caller, val_ptr as usize, &mut val_buf).expect("read value");
                
                let key = String::from_utf8_lossy(&key_buf).to_string();
                let value = String::from_utf8_lossy(&val_buf).to_string();
                
                let mut state = caller.data().lock().unwrap();
                if let Some(elem) = state.elements.iter_mut().find(|e| e.id == id as u32) {
                    elem.attributes.insert(key, value);
                }
                state.instruction_count += 1;
            },
        )?;
        
        // add_child(parent_id: i32, child_id: i32)
        linker.func_wrap(
            "env",
            "add_child",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, parent_id: i32, child_id: i32| {
                let mut state = caller.data().lock().unwrap();
                if let Some(child) = state.elements.iter_mut().find(|e| e.id == child_id as u32) {
                    child.parent_id = Some(parent_id as u32);
                }
                state.instruction_count += 1;
            },
        )?;
        
        // emit_event(name_ptr: i32, name_len: i32, payload_ptr: i32, payload_len: i32)
        linker.func_wrap(
            "env",
            "emit_event",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, name_ptr: i32, name_len: i32, payload_ptr: i32, payload_len: i32| {
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .expect("memory export");
                
                let mut name_buf = vec![0u8; name_len as usize];
                let mut payload_buf = vec![0u8; payload_len as usize];
                memory.read(&caller, name_ptr as usize, &mut name_buf).expect("read name");
                memory.read(&caller, payload_ptr as usize, &mut payload_buf).expect("read payload");
                
                let name = String::from_utf8_lossy(&name_buf).to_string();
                let payload_str = String::from_utf8_lossy(&payload_buf).to_string();
                let payload: serde_json::Value = serde_json::from_str(&payload_str)
                    .unwrap_or(serde_json::Value::String(payload_str));
                
                let mut state = caller.data().lock().unwrap();
                state.events.push(WasmEvent { name, payload });
                state.instruction_count += 1;
            },
        )?;
        
        // push_value(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "push_value",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, ptr: i32, len: i32| {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory()).expect("memory");
                let mut buf = vec![0u8; len as usize];
                memory.read(&caller, ptr as usize, &mut buf).expect("read");
                let s = String::from_utf8_lossy(&buf).to_string();
                let val: serde_json::Value = serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
                
                let mut state = caller.data().lock().unwrap();
                state.value_stack.push(val);
            },
        )?;
        
        // pop_value()
        linker.func_wrap(
            "env",
            "pop_value",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>| {
                let mut state = caller.data().lock().unwrap();
                state.value_stack.pop();
            },
        )?;
        
        // load_var(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "load_var",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, ptr: i32, len: i32| {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory()).expect("memory");
                let mut buf = vec![0u8; len as usize];
                memory.read(&caller, ptr as usize, &mut buf).expect("read");
                let name = String::from_utf8_lossy(&buf).to_string();
                
                let mut state = caller.data().lock().unwrap();
                let val = state.variables.get(&name).cloned().unwrap_or(serde_json::Value::Null);
                state.value_stack.push(val);
            },
        )?;
        
        // store_var(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "store_var",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, ptr: i32, len: i32| {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory()).expect("memory");
                let mut buf = vec![0u8; len as usize];
                memory.read(&caller, ptr as usize, &mut buf).expect("read");
                let name = String::from_utf8_lossy(&buf).to_string();
                
                let mut state = caller.data().lock().unwrap();
                if let Some(val) = state.value_stack.pop() {
                    state.variables.insert(name, val);
                }
            },
        )?;
        
        // bin_op(op_code: i32)
        linker.func_wrap(
            "env",
            "bin_op",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, op_code: i32| {
                let mut state = caller.data().lock().unwrap();
                let right = state.value_stack.pop().unwrap_or(serde_json::Value::Null);
                let left = state.value_stack.pop().unwrap_or(serde_json::Value::Null);
                
                // Simple evaluation for now
                let res = match op_code {
                    0 => { // Add
                        match (left, right) {
                            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                                serde_json::json!(l.as_f64().unwrap() + r.as_f64().unwrap())
                            }
                            (serde_json::Value::String(l), serde_json::Value::String(r)) => {
                                serde_json::Value::String(format!("{}{}", l, r))
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    1 => { // Sub
                        match (left, right) {
                            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                                serde_json::json!(l.as_f64().unwrap() - r.as_f64().unwrap())
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    2 => { // Mul
                        match (left, right) {
                            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                                serde_json::json!(l.as_f64().unwrap() * r.as_f64().unwrap())
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    3 => { // Div
                        match (left, right) {
                            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                                let r_val = r.as_f64().unwrap();
                                if r_val != 0.0 {
                                    serde_json::json!(l.as_f64().unwrap() / r_val)
                                } else {
                                    serde_json::Value::Null
                                }
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    5 => { // Eq
                        serde_json::Value::Bool(left == right)
                    }
                    6 => { // Ne
                        serde_json::Value::Bool(left != right)
                    }
                    11 => { // And
                        match (left, right) {
                            (serde_json::Value::Bool(l), serde_json::Value::Bool(r)) => {
                                serde_json::Value::Bool(l && r)
                            }
                            _ => serde_json::Value::Bool(false),
                        }
                    }
                    12 => { // Or
                        match (left, right) {
                            (serde_json::Value::Bool(l), serde_json::Value::Bool(r)) => {
                                serde_json::Value::Bool(l || r)
                            }
                            _ => serde_json::Value::Bool(false),
                        }
                    }
                    13 => { // Concat
                        serde_json::Value::String(format!("{}{}", left.to_string(), right.to_string()))
                    }
                    _ => serde_json::Value::Null,
                };
                state.value_stack.push(res);
            },
        )?;
        
        // define_element_from_stack(id: i32)
        linker.func_wrap(
            "env",
            "define_element_from_stack",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, id: i32| {
                let mut state = caller.data().lock().unwrap();
                let tag = state.value_stack.pop()
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "div".to_string());
                
                state.elements.push(WasmElement {
                    id: id as u32,
                    tag,
                    attributes: HashMap::new(),
                    parent_id: None,
                });
            },
        )?;
        
        // set_attribute_from_stack(id: i32, key_ptr: i32, key_len: i32)
        linker.func_wrap(
            "env",
            "set_attribute_from_stack",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, id: i32, key_ptr: i32, key_len: i32| {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory()).expect("memory");
                let mut buf = vec![0u8; key_len as usize];
                memory.read(&caller, key_ptr as usize, &mut buf).expect("read");
                let key = String::from_utf8_lossy(&buf).to_string();
                
                let mut state = caller.data().lock().unwrap();
                let val = state.value_stack.pop()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                
                if let Some(elem) = state.elements.iter_mut().find(|e| e.id == id as u32) {
                    elem.attributes.insert(key, val);
                }
            },
        )?;
        
        // define_text_from_stack()
        linker.func_wrap(
            "env",
            "define_text_from_stack",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>| {
                let mut state = caller.data().lock().unwrap();
                let text = state.value_stack.pop()
                    .map(|v| v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()))
                    .unwrap_or_default();
                
                // Text elements are special, for now we'll just create a "text" element
                let id = state.elements.len() as u32 + 1000; // Simple ID generation
                state.elements.push(WasmElement {
                    id,
                    tag: "text".to_string(),
                    attributes: [("content".to_string(), text)].into_iter().collect(),
                    parent_id: None,
                });
            },
        )?;
        
        // declare_state_from_stack(name_ptr: i32, name_len: i32)
        linker.func_wrap(
            "env",
            "declare_state_from_stack",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, name_ptr: i32, name_len: i32| {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory()).expect("memory");
                let mut buf = vec![0u8; name_len as usize];
                memory.read(&caller, name_ptr as usize, &mut buf).expect("read");
                let name = String::from_utf8_lossy(&buf).to_string();
                
                let mut state = caller.data().lock().unwrap();
                if let Some(val) = state.value_stack.pop() {
                    state.variables.insert(name, val);
                }
            },
        )?;
        
        // update_state_from_stack(name_ptr: i32, name_len: i32)
        linker.func_wrap(
            "env",
            "update_state_from_stack",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, name_ptr: i32, name_len: i32| {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory()).expect("memory");
                let mut buf = vec![0u8; name_len as usize];
                memory.read(&caller, name_ptr as usize, &mut buf).expect("read");
                let name = String::from_utf8_lossy(&buf).to_string();
                
                let mut state = caller.data().lock().unwrap();
                if let Some(val) = state.value_stack.pop() {
                    state.variables.insert(name, val);
                }
            },
        )?;
        
        // call_func(name_ptr: i32, name_len: i32, num_args: i32)
        linker.func_wrap(
            "env",
            "call_func",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, name_ptr: i32, name_len: i32, num_args: i32| {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory()).expect("memory");
                let mut buf = vec![0u8; name_len as usize];
                memory.read(&caller, name_ptr as usize, &mut buf).expect("read");
                let name = String::from_utf8_lossy(&buf).to_string();
                
                let mut state = caller.data().lock().unwrap();
                let mut args = Vec::new();
                for _ in 0..num_args {
                    args.push(state.value_stack.pop().unwrap_or(serde_json::Value::Null));
                }
                args.reverse();
                
                // Built-in functions
                let res = match name.as_str() {
                    "len" => {
                        let arg = state.value_stack.pop().unwrap_or(serde_json::Value::Null);
                        match arg {
                            serde_json::Value::Array(a) => serde_json::json!(a.len()),
                            serde_json::Value::String(s) => serde_json::json!(s.len()),
                            serde_json::Value::Object(m) => serde_json::json!(m.len()),
                            _ => serde_json::json!(0),
                        }
                    }
                    "str" => {
                        let arg = state.value_stack.pop().unwrap_or(serde_json::Value::Null);
                        match arg {
                            serde_json::Value::String(s) => serde_json::Value::String(s),
                            _ => serde_json::Value::String(arg.to_string()),
                        }
                    }
                    "num" => {
                        let arg = state.value_stack.pop().unwrap_or(serde_json::Value::Null);
                        let n = match arg {
                            serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
                            serde_json::Value::String(s) => s.parse().unwrap_or(0.0),
                            serde_json::Value::Bool(b) => if b { 1.0 } else { 0.0 },
                            _ => 0.0,
                        };
                        serde_json::json!(n)
                    }
                    "print" => {
                        let arg = state.value_stack.pop().unwrap_or(serde_json::Value::Null);
                        println!("[HLS PRINT] {}", arg);
                        serde_json::Value::Null
                    }
                    "list" => {
                        let mut items = Vec::new();
                        for _ in 0..num_args {
                            if let Some(val) = state.value_stack.pop() {
                                items.push(val);
                            }
                        }
                        items.reverse();
                        serde_json::Value::Array(items)
                    }
                    "object" => {
                        let mut map = serde_json::Map::new();
                        for _ in 0..(num_args / 2) {
                            let val = state.value_stack.pop().unwrap_or(serde_json::Value::Null);
                            let key = state.value_stack.pop().and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default();
                            map.insert(key, val);
                        }
                        serde_json::Value::Object(map)
                    }
                    "emit" => {
                        let payload = if num_args > 1 { state.value_stack.pop().unwrap_or(serde_json::Value::Null) } else { serde_json::Value::Null };
                        let event_name = state.value_stack.pop().and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default();
                        state.events.push(WasmEvent { name: event_name, payload });
                        serde_json::Value::Null
                    }
                    _ => serde_json::Value::Null,
                };
                state.value_stack.push(res);
            },
        )?;
        
        // emit_event_from_stack(name_ptr: i32, name_len: i32)
        linker.func_wrap(
            "env",
            "emit_event_from_stack",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, name_ptr: i32, name_len: i32| {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory()).expect("memory");
                let mut buf = vec![0u8; name_len as usize];
                memory.read(&caller, name_ptr as usize, &mut buf).expect("read");
                let name = String::from_utf8_lossy(&buf).to_string();
                
                let mut state = caller.data().lock().unwrap();
                let payload = state.value_stack.pop().unwrap_or(serde_json::Value::Null);
                state.events.push(WasmEvent { name, payload });
            },
        )?;
        
        // unary_op(op_code: i32)
        linker.func_wrap(
            "env",
            "unary_op",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, op_code: i32| {
                let mut state = caller.data().lock().unwrap();
                let val = state.value_stack.pop().unwrap_or(serde_json::Value::Null);
                
                let res = match op_code {
                    0 => { // Not
                        match val {
                            serde_json::Value::Bool(b) => serde_json::Value::Bool(!b),
                            _ => serde_json::Value::Bool(false),
                        }
                    }
                    1 => { // Neg
                        match val {
                            serde_json::Value::Number(n) => {
                                serde_json::json!(-n.as_f64().unwrap())
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    _ => serde_json::Value::Null,
                };
                state.value_stack.push(res);
            },
        )?;
        
        // call_target(target: i32)
        linker.func_wrap(
            "env",
            "call_target",
            |_caller: Caller<'_, Arc<Mutex<HostState>>>, _target: i32| {
                // Internal calls not yet supported in WASM runtime
            },
        )?;

        // stream_latent(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "stream_latent",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, _ptr: i32, len: i32| {
                // In a real implementation, we'd read the vector from memory
                let mut state = caller.data().lock().unwrap();
                state.latent_streams.push(vec![0.0f32; len as usize]);
                state.instruction_count += 1;
            },
        )?;
        
        // morph_protocol(seed: i64)
        linker.func_wrap(
            "env",
            "morph_protocol",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, _seed: i64| {
                let mut state = caller.data().lock().unwrap();
                state.instruction_count += 1;
                // Morphing would be handled by the protocol layer
            },
        )?;
        
        // inject_decoy(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "inject_decoy",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, _ptr: i32, _len: i32| {
                let mut state = caller.data().lock().unwrap();
                state.instruction_count += 1;
                // Decoy injection would be handled by the protocol layer
            },
        )?;

        // declare_state(name_ptr: i32, name_len: i32, val_ptr: i32, val_len: i32)
        linker.func_wrap(
            "env",
            "declare_state",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, name_ptr: i32, name_len: i32, val_ptr: i32, val_len: i32| {
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .expect("memory export");
                
                let mut name_buf = vec![0u8; name_len as usize];
                let mut val_buf = vec![0u8; val_len as usize];
                memory.read(&caller, name_ptr as usize, &mut name_buf).expect("read name");
                memory.read(&caller, val_ptr as usize, &mut val_buf).expect("read value");
                
                let name = String::from_utf8_lossy(&name_buf).to_string();
                let val_str = String::from_utf8_lossy(&val_buf).to_string();
                let val: serde_json::Value = serde_json::from_str(&val_str).unwrap_or(serde_json::Value::Null);
                
                let mut state = caller.data().lock().unwrap();
                state.events.push(WasmEvent { 
                    name: "state_declared".to_string(), 
                    payload: serde_json::json!({ "name": name, "value": val }) 
                });
                state.instruction_count += 1;
            },
        )?;

        // update_state(name_ptr: i32, name_len: i32, val_ptr: i32, val_len: i32)
        linker.func_wrap(
            "env",
            "update_state",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, name_ptr: i32, name_len: i32, val_ptr: i32, val_len: i32| {
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .expect("memory export");
                
                let mut name_buf = vec![0u8; name_len as usize];
                let mut val_buf = vec![0u8; val_len as usize];
                memory.read(&caller, name_ptr as usize, &mut name_buf).expect("read name");
                memory.read(&caller, val_ptr as usize, &mut val_buf).expect("read value");
                
                let name = String::from_utf8_lossy(&name_buf).to_string();
                let val_str = String::from_utf8_lossy(&val_buf).to_string();
                let val: serde_json::Value = serde_json::from_str(&val_str).unwrap_or(serde_json::Value::Null);
                
                let mut state = caller.data().lock().unwrap();
                state.events.push(WasmEvent { 
                    name: "state_updated".to_string(), 
                    payload: serde_json::json!({ "name": name, "value": val }) 
                });
                state.instruction_count += 1;
            },
        )?;
        
        Ok(())
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create WASM runtime")
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_compile_simple_hlb_to_wat() {
        let binary = HyperlightBinary {
            instructions: vec![
                Instruction::DefineElement { id: 1, tag: "div".to_string() },
                Instruction::SetAttribute { id: 1, key: "class".to_string(), value: "container".to_string() },
            ],
            data: Vec::new(),
        };
        
        let wat = HlbToWatCompiler::compile(&binary);
        
        assert!(wat.contains("(module"));
        assert!(wat.contains("define_element"));
        assert!(wat.contains("set_attribute"));
        assert!(wat.contains("(export \"execute\")"));
    }
    
    #[test]
    fn test_wasm_execution() {
        let runtime = WasmRuntime::new().unwrap();
        
        let binary = HyperlightBinary {
            instructions: vec![
                Instruction::DefineElement { id: 1, tag: "div".to_string() },
                Instruction::DefineElement { id: 2, tag: "span".to_string() },
                Instruction::AddChild { parent_id: 1, child_id: 2 },
                Instruction::SetAttribute { id: 2, key: "text".to_string(), value: "Hello".to_string() },
            ],
            data: Vec::new(),
        };
        
        let result = runtime.execute(&binary).unwrap();
        
        assert_eq!(result.elements.len(), 2);
        assert_eq!(result.stats.instructions_executed, 4);
    }
    
    #[test]
    fn test_wasm_events() {
        let runtime = WasmRuntime::new().unwrap();
        
        let binary = HyperlightBinary {
            instructions: vec![
                Instruction::EmitEvent {
                    name: "click".to_string(),
                    payload: serde_json::json!({"id": 42}),
                },
            ],
            data: Vec::new(),
        };
        
        let result = runtime.execute(&binary).unwrap();
        
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].name, "click");
    }
}
