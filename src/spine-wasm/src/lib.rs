// Allow dead code for WASM runtime API surface
#![allow(dead_code)]

// =============================================================================
// SPINE WASM - WebAssembly Runtime for HLB Execution
// =============================================================================
//
// This module compiles SPINE Binary (HLB) instructions to WebAssembly
// for near-native execution speed. It provides:
//
// 1. HLB → WAT (WebAssembly Text) transpilation
// 2. WAT → WASM compilation via wasmtime
// 3. Sandboxed execution with memory isolation
// 4. Host function bindings for DOM operations
//
// =============================================================================

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use spine_protocol::{Instruction, SpineBinary};
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
    /// Agentic actions requested
    pub actions: Vec<WasmAction>,
    /// Execution statistics
    pub stats: WasmStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WasmAction {
    Navigate(String),
    Search(String),
    StoreKnowledge {
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    },
    QueryKnowledge {
        query: String,
        tags: Vec<String>,
        limit: usize,
    },
    Reason(String),
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
    /// Size of the guest's linear memory at the end of the run, read from
    /// the instance (one 64 KiB page for generated programs). It does not
    /// include host-side values; those are bounded by `MAX_LIVE_BYTES`.
    pub memory_used_bytes: usize,
    pub instructions_executed: usize,
}

// =============================================================================
// HOST STATE - Shared between WASM and host
// =============================================================================

/// Maximum number of instructions in one program. Checked before any
/// WAT is built, so an oversized frame is refused before the compiler
/// spends time or memory on it.
pub const MAX_INSTRUCTIONS: usize = 65_536;

/// Maximum depth of the host value stack.
pub const MAX_STACK_DEPTH: usize = 1024;

/// Maximum size, in estimated bytes, of one host value (a string's
/// length, or the recursive sum for an array or object). Also bounds a
/// string produced by `Concat` or `Add`.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;

/// Maximum nesting depth of one host value (arrays and objects built by
/// `list`/`object`, or parsed from a payload).
pub const MAX_VALUE_DEPTH: usize = 64;

/// Maximum total estimated bytes of live host values: the value stack,
/// the variables, and every element, event, action and latent vector
/// produced so far.
pub const MAX_LIVE_BYTES: usize = 1024 * 1024;

/// Maximum number of elements one execution may create.
pub const MAX_ELEMENTS: usize = 4096;

/// Maximum number of events one execution may emit.
pub const MAX_EVENTS: usize = 4096;

/// Maximum number of actions (navigate, search, store, query, reason)
/// one execution may request.
pub const MAX_ACTIONS: usize = 64;

/// Maximum `num_args` of a `Call` instruction.
pub const MAX_CALL_ARGS: usize = 32;

/// Maximum number of latent vectors one execution may stream.
pub const MAX_LATENT_STREAMS: usize = 256;

/// Maximum dimensions of one streamed latent vector.
pub const MAX_LATENT_DIMS: usize = 16_384;

/// Maximum bytes of string data a program may carry. The guest's linear
/// memory is one 64 KiB page, so data past this cannot be instantiated
/// anyway; refusing it up front avoids building and parsing the WAT.
pub const MAX_STRING_DATA_BYTES: usize = 64 * 1024;

/// Fuel charged for every host call, on top of the guest instructions
/// wasmtime already meters. Host calls also pay one unit per byte they
/// read or scan.
pub const HOST_CALL_FUEL: u64 = 64;

/// Rough per-node overhead used when estimating the size of a value.
const VALUE_OVERHEAD: usize = 16;

/// Rough per-record overhead for elements, events and actions.
const RECORD_OVERHEAD: usize = 32;

/// Estimated size of a JSON value in bytes, or `None` when it nests
/// deeper than [`MAX_VALUE_DEPTH`].
fn value_size(v: &serde_json::Value, depth: usize) -> Option<usize> {
    if depth > MAX_VALUE_DEPTH {
        return None;
    }
    match v {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            Some(VALUE_OVERHEAD)
        }
        serde_json::Value::String(s) => Some(VALUE_OVERHEAD + s.len()),
        serde_json::Value::Array(a) => {
            let mut total = VALUE_OVERHEAD;
            for item in a {
                total = total.saturating_add(value_size(item, depth + 1)?);
            }
            Some(total)
        }
        serde_json::Value::Object(m) => {
            let mut total = VALUE_OVERHEAD;
            for (k, item) in m {
                total = total.saturating_add(k.len().saturating_add(value_size(item, depth + 1)?));
            }
            Some(total)
        }
    }
}

/// Bytes in one wasm linear-memory page.
const WASM_PAGE_BYTES: usize = 65_536;

/// Resource limits wasmtime enforces on the guest itself: linear memory
/// is capped at `DEFAULT_MAX_MEMORY_PAGES` pages, with one instance, one
/// memory and one table at most. A failed `memory.grow` returns -1 in the
/// guest.
fn guest_limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(DEFAULT_MAX_MEMORY_PAGES * WASM_PAGE_BYTES)
        .instances(1)
        .memories(1)
        .tables(1)
        .build()
}

/// A `fmt::Write` sink that only counts, and fails once the count passes
/// [`MAX_VALUE_BYTES`], so the length of a rendering is known before the
/// string is built.
struct CountingWriter(usize);

impl std::fmt::Write for CountingWriter {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0 = self.0.saturating_add(s.len());
        if self.0 > MAX_VALUE_BYTES {
            Err(std::fmt::Error)
        } else {
            Ok(())
        }
    }
}

/// Length of `format!("{}{}", left, right)` without building it, or
/// `None` when it exceeds [`MAX_VALUE_BYTES`].
fn concat_len(left: &serde_json::Value, right: &serde_json::Value) -> Option<usize> {
    let mut w = CountingWriter(0);
    std::fmt::write(&mut w, format_args!("{}{}", left, right)).ok()?;
    Some(w.0)
}

#[derive(Default)]
struct HostState {
    elements: Vec<WasmElement>,
    events: Vec<WasmEvent>,
    latent_streams: Vec<Vec<f32>>,
    actions: Vec<WasmAction>,
    instruction_count: usize,
    current_latent: Vec<f32>,

    // New stack-based state
    value_stack: Vec<serde_json::Value>,
    variables: HashMap<String, serde_json::Value>,
    element_stack: Vec<u32>,

    /// Estimated bytes of live host values, checked against
    /// [`MAX_LIVE_BYTES`].
    live_bytes: usize,
    /// The first host budget that was exceeded, if any. Host functions
    /// return an error (a wasmtime trap) when a budget is exceeded; this
    /// keeps the message so `execute` can report the budget by name
    /// instead of the generic trap text.
    violation: Option<String>,
}

impl HostState {
    /// Record a budget violation and return it as an error.
    fn violate(&mut self, budget: &str, detail: String) -> anyhow::Error {
        let msg = format!("host budget exceeded: {budget} ({detail})");
        if self.violation.is_none() {
            self.violation = Some(msg.clone());
        }
        anyhow::anyhow!(msg)
    }

    /// Count `n` more live bytes, refusing past [`MAX_LIVE_BYTES`].
    fn charge(&mut self, n: usize) -> Result<()> {
        let total = self.live_bytes.saturating_add(n);
        if total > MAX_LIVE_BYTES {
            return Err(self.violate(
                "MAX_LIVE_BYTES",
                format!("{total} bytes of live values, limit {MAX_LIVE_BYTES}"),
            ));
        }
        self.live_bytes = total;
        Ok(())
    }

    /// Stop counting `n` live bytes.
    fn release(&mut self, n: usize) {
        self.live_bytes = self.live_bytes.saturating_sub(n);
    }

    /// Estimated size of `v`, refusing a value over [`MAX_VALUE_BYTES`]
    /// or nested past [`MAX_VALUE_DEPTH`].
    fn sized(&mut self, v: &serde_json::Value) -> Result<usize> {
        match value_size(v, 0) {
            Some(n) if n <= MAX_VALUE_BYTES => Ok(n),
            Some(n) => Err(self.violate(
                "MAX_VALUE_BYTES",
                format!("value of {n} bytes, limit {MAX_VALUE_BYTES}"),
            )),
            None => Err(self.violate(
                "MAX_VALUE_DEPTH",
                format!("value nested deeper than {MAX_VALUE_DEPTH}"),
            )),
        }
    }

    /// Refuse a string longer than [`MAX_VALUE_BYTES`].
    fn check_str(&mut self, s: &str) -> Result<()> {
        if s.len() > MAX_VALUE_BYTES {
            return Err(self.violate(
                "MAX_VALUE_BYTES",
                format!("string of {} bytes, limit {MAX_VALUE_BYTES}", s.len()),
            ));
        }
        Ok(())
    }

    /// Push a value, enforcing depth, per-value and total budgets.
    fn push_val(&mut self, v: serde_json::Value) -> Result<()> {
        if self.value_stack.len() >= MAX_STACK_DEPTH {
            return Err(self.violate(
                "MAX_STACK_DEPTH",
                format!("value stack deeper than {MAX_STACK_DEPTH}"),
            ));
        }
        let size = self.sized(&v)?;
        self.charge(size)?;
        self.value_stack.push(v);
        Ok(())
    }

    /// Pop a value and stop counting its bytes.
    fn pop_val(&mut self) -> Option<serde_json::Value> {
        let v = self.value_stack.pop()?;
        self.release(value_size(&v, 0).unwrap_or(0));
        Some(v)
    }

    /// Pop a value and take it as a string if it is one.
    fn pop_str(&mut self) -> Option<String> {
        self.pop_val().and_then(|v| match v {
            serde_json::Value::String(s) => Some(s),
            _ => None,
        })
    }

    /// Bind a variable, charging the new value and releasing the old one.
    fn set_var(&mut self, name: String, val: serde_json::Value) -> Result<()> {
        let size = self.sized(&val)?;
        let name_cost = if self.variables.contains_key(&name) {
            0
        } else {
            name.len() + RECORD_OVERHEAD
        };
        self.charge(size + name_cost)?;
        if let Some(old) = self.variables.insert(name, val) {
            self.release(value_size(&old, 0).unwrap_or(0));
        }
        Ok(())
    }

    /// Set an attribute on the first element with `id`, if there is one.
    fn set_element_attribute(&mut self, id: u32, key: String, val: String) -> Result<()> {
        self.check_str(&key)?;
        self.check_str(&val)?;
        if let Some(i) = self.elements.iter().position(|e| e.id == id) {
            self.charge(RECORD_OVERHEAD + key.len() + val.len())?;
            let key_len = key.len();
            if let Some(old) = self.elements[i].attributes.insert(key, val) {
                self.release(RECORD_OVERHEAD + key_len + old.len());
            }
        }
        Ok(())
    }

    fn add_element(&mut self, e: WasmElement) -> Result<()> {
        if self.elements.len() >= MAX_ELEMENTS {
            return Err(self.violate("MAX_ELEMENTS", format!("more than {MAX_ELEMENTS} elements")));
        }
        self.check_str(&e.tag)?;
        let mut cost = RECORD_OVERHEAD + e.tag.len();
        for (k, v) in &e.attributes {
            self.check_str(v)?;
            cost += RECORD_OVERHEAD + k.len() + v.len();
        }
        self.charge(cost)?;
        self.elements.push(e);
        Ok(())
    }

    fn add_event(&mut self, e: WasmEvent) -> Result<()> {
        if self.events.len() >= MAX_EVENTS {
            return Err(self.violate("MAX_EVENTS", format!("more than {MAX_EVENTS} events")));
        }
        self.check_str(&e.name)?;
        let payload = self.sized(&e.payload)?;
        self.charge(RECORD_OVERHEAD + e.name.len() + payload)?;
        self.events.push(e);
        Ok(())
    }

    fn add_action(&mut self, a: WasmAction) -> Result<()> {
        if self.actions.len() >= MAX_ACTIONS {
            return Err(self.violate("MAX_ACTIONS", format!("more than {MAX_ACTIONS} actions")));
        }
        let cost = match &a {
            WasmAction::Navigate(s) | WasmAction::Search(s) | WasmAction::Reason(s) => {
                self.check_str(s)?;
                s.len()
            }
            WasmAction::StoreKnowledge { key, value, tags } => {
                self.check_str(key)?;
                let v = self.sized(value)?;
                key.len() + v + tags.iter().map(|t| t.len() + 24).sum::<usize>()
            }
            WasmAction::QueryKnowledge { query, tags, .. } => {
                self.check_str(query)?;
                query.len() + tags.iter().map(|t| t.len() + 24).sum::<usize>()
            }
        };
        self.charge(RECORD_OVERHEAD + cost)?;
        self.actions.push(a);
        Ok(())
    }
}

/// The wasmtime store's data: the host state the host functions work on,
/// and the resource limits wasmtime enforces on the guest itself (linear
/// memory growth, instances, tables).
struct StoreData {
    host: Arc<Mutex<HostState>>,
    limits: StoreLimits,
}

/// Lock the host state of a call.
fn host<'a>(caller: &'a Caller<'_, StoreData>) -> std::sync::MutexGuard<'a, HostState> {
    caller
        .data()
        .host
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Charge `units` of fuel for host work. Guest instructions are metered
/// by wasmtime; this makes host calls (and the bytes they read, scan or
/// build) draw on the same budget, so total work is bounded by fuel.
fn charge_fuel(caller: &mut Caller<'_, StoreData>, units: u64) -> Result<()> {
    let left = caller.get_fuel()?;
    if left < units {
        let err = host(caller).violate(
            "fuel",
            format!("host work needs {units} units, {left} left"),
        );
        caller.set_fuel(0)?;
        return Err(err);
    }
    caller.set_fuel(left - units)
}

/// Read `len` bytes of guest memory at `ptr`, refusing negative or
/// oversized ranges before allocating.
fn read_guest(caller: &mut Caller<'_, StoreData>, ptr: i32, len: i32) -> Result<Vec<u8>> {
    if ptr < 0 || len < 0 {
        anyhow::bail!("guest memory range out of bounds");
    }
    let len = len as usize;
    if len > MAX_VALUE_BYTES {
        return Err(host(caller).violate(
            "MAX_VALUE_BYTES",
            format!("guest read of {len} bytes, limit {MAX_VALUE_BYTES}"),
        ));
    }
    charge_fuel(caller, len as u64)?;
    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .context("guest memory export missing")?;
    let mut buf = vec![0u8; len];
    memory
        .read(&*caller, ptr as usize, &mut buf)
        .map_err(|e| anyhow::anyhow!("guest memory read out of bounds: {e}"))?;
    Ok(buf)
}

/// Read a (lossy UTF-8) string from guest memory.
fn read_guest_string(caller: &mut Caller<'_, StoreData>, ptr: i32, len: i32) -> Result<String> {
    let buf = read_guest(caller, ptr, len)?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

// =============================================================================
// HLB TO WAT COMPILER
// =============================================================================

/// Compiles HLB instructions to WebAssembly Text format
pub struct HlbToWatCompiler;

impl HlbToWatCompiler {
    /// Compile HLB binary to WAT (WebAssembly Text).
    ///
    /// Refuses a program of more than [`MAX_INSTRUCTIONS`] instructions,
    /// or one whose string data does not fit [`MAX_STRING_DATA_BYTES`].
    pub fn compile(binary: &SpineBinary) -> Result<String> {
        if binary.instructions.len() > MAX_INSTRUCTIONS {
            anyhow::bail!(
                "program has {} instructions, exceeds MAX_INSTRUCTIONS ({})",
                binary.instructions.len(),
                MAX_INSTRUCTIONS
            );
        }
        let mut wat = String::new();

        // Module header
        wat.push_str("(module\n");

        // Import host functions
        wat.push_str("  ;; Host function imports\n");
        wat.push_str(
            "  (import \"env\" \"define_element\" (func $define_element (param i32 i32 i32)))\n",
        );
        wat.push_str("  (import \"env\" \"set_attribute\" (func $set_attribute (param i32 i32 i32 i32 i32)))\n");
        wat.push_str("  (import \"env\" \"add_child\" (func $add_child (param i32 i32)))\n");
        wat.push_str(
            "  (import \"env\" \"emit_event\" (func $emit_event (param i32 i32 i32 i32)))\n",
        );
        wat.push_str(
            "  (import \"env\" \"stream_latent\" (func $stream_latent (param i32 i32)))\n",
        );
        wat.push_str("  (import \"env\" \"morph_protocol\" (func $morph_protocol (param i64)))\n");
        wat.push_str("  (import \"env\" \"inject_decoy\" (func $inject_decoy (param i32 i32)))\n");
        wat.push_str(
            "  (import \"env\" \"declare_state\" (func $declare_state (param i32 i32 i32 i32)))\n",
        );
        wat.push_str(
            "  (import \"env\" \"update_state\" (func $update_state (param i32 i32 i32 i32)))\n",
        );

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
        wat.push_str(
            "  (import \"env\" \"define_text_from_stack\" (func $define_text_from_stack))\n",
        );
        wat.push_str("  (import \"env\" \"declare_state_from_stack\" (func $declare_state_from_stack (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"update_state_from_stack\" (func $update_state_from_stack (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"navigate_from_stack\" (func $navigate_from_stack))\n");
        wat.push_str("  (import \"env\" \"search_from_stack\" (func $search_from_stack))\n");
        wat.push_str("  (import \"env\" \"store_knowledge_from_stack\" (func $store_knowledge_from_stack (param i32 i32)))\n");
        wat.push_str("  (import \"env\" \"query_knowledge_from_stack\" (func $query_knowledge_from_stack (param i32 i32 i32)))\n");
        wat.push('\n');

        // Memory for string data
        wat.push_str("  ;; Linear memory for string storage\n");
        wat.push_str("  (memory (export \"memory\") 1)\n");
        wat.push('\n');

        // Build data section with all strings
        let mut data_offset = 0u32;
        let mut string_offsets: HashMap<String, (u32, u32)> = HashMap::new();
        let mut data_section = String::new();

        for instruction in &binary.instructions {
            match instruction {
                Instruction::DefineElement { tag, .. } if !string_offsets.contains_key(tag) => {
                    let len = tag.len() as u32;
                    string_offsets.insert(tag.clone(), (data_offset, len));
                    data_section.push_str(&format!(
                        "  (data (i32.const {}) \"{}\")\n",
                        data_offset,
                        escape_wat_string(tag)
                    ));
                    data_offset += len + 1; // +1 for null terminator space
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
                Instruction::Load(name) | Instruction::Store(name)
                    if !string_offsets.contains_key(name) =>
                {
                    let len = name.len() as u32;
                    string_offsets.insert(name.clone(), (data_offset, len));
                    data_section.push_str(&format!(
                        "  (data (i32.const {}) \"{}\")\n",
                        data_offset,
                        escape_wat_string(name)
                    ));
                    data_offset += len + 1;
                }
                Instruction::Call { name, .. } if !string_offsets.contains_key(name) => {
                    let len = name.len() as u32;
                    string_offsets.insert(name.clone(), (data_offset, len));
                    data_section.push_str(&format!(
                        "  (data (i32.const {}) \"{}\")\n",
                        data_offset,
                        escape_wat_string(name)
                    ));
                    data_offset += len + 1;
                }
                Instruction::SetAttributeFromStack { key, .. }
                    if !string_offsets.contains_key(key) =>
                {
                    let len = key.len() as u32;
                    string_offsets.insert(key.clone(), (data_offset, len));
                    data_section.push_str(&format!(
                        "  (data (i32.const {}) \"{}\")\n",
                        data_offset,
                        escape_wat_string(key)
                    ));
                    data_offset += len + 1;
                }
                Instruction::EmitEventFromStack { name } if !string_offsets.contains_key(name) => {
                    let len = name.len() as u32;
                    string_offsets.insert(name.clone(), (data_offset, len));
                    data_section.push_str(&format!(
                        "  (data (i32.const {}) \"{}\")\n",
                        data_offset,
                        escape_wat_string(name)
                    ));
                    data_offset += len + 1;
                }
                Instruction::DeclareStateFromStack { name }
                | Instruction::UpdateStateFromStack { name }
                    if !string_offsets.contains_key(name) =>
                {
                    let len = name.len() as u32;
                    string_offsets.insert(name.clone(), (data_offset, len));
                    data_section.push_str(&format!(
                        "  (data (i32.const {}) \"{}\")\n",
                        data_offset,
                        escape_wat_string(name)
                    ));
                    data_offset += len + 1;
                }
                // The tag list is string data like any other: it is registered
                // here, before the data section is written into the module.
                Instruction::StoreKnowledgeFromStack { tags }
                | Instruction::QueryKnowledgeFromStack { tags, .. } => {
                    let tags_json = serde_json::json!(tags).to_string();
                    if !string_offsets.contains_key(&tags_json) {
                        let len = tags_json.len() as u32;
                        string_offsets.insert(tags_json.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(&tags_json)
                        ));
                        data_offset += len + 1;
                    }
                }
                _ => {}
            }
        }

        if data_offset as usize > MAX_STRING_DATA_BYTES {
            anyhow::bail!(
                "program string data is {} bytes, exceeds MAX_STRING_DATA_BYTES ({})",
                data_offset,
                MAX_STRING_DATA_BYTES
            );
        }

        wat.push_str("  ;; String data\n");
        wat.push_str(&data_section);
        wat.push('\n');

        // Main execution function
        wat.push_str("  ;; Main HLB execution function\n");
        wat.push_str("  (func (export \"execute\") (result i32)\n");

        // Generate instructions
        for instruction in &binary.instructions {
            match instruction {
                Instruction::DefineElement { id, tag } => {
                    let (offset, len) = string_offsets
                        .get(tag)
                        .context("missing string offset for tag")?;
                    wat.push_str(&format!("    ;; DefineElement id={} tag=\"{}\"\n", id, tag));
                    wat.push_str(&format!(
                        "    (call $define_element (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        id, offset, len
                    ));
                }

                Instruction::SetAttribute { id, key, value } => {
                    let (key_offset, key_len) = string_offsets
                        .get(key)
                        .context("missing string offset for attribute key")?;
                    let (val_offset, val_len) = string_offsets
                        .get(value)
                        .context("missing string offset for attribute value")?;
                    wat.push_str(&format!(
                        "    ;; SetAttribute id={} key=\"{}\" value=\"{}\"\n",
                        id, key, value
                    ));
                    wat.push_str(&format!(
                        "    (call $set_attribute (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        id, key_offset, key_len, val_offset, val_len
                    ));
                }

                Instruction::AddChild {
                    parent_id,
                    child_id,
                } => {
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
                    let (name_offset, name_len) = string_offsets
                        .get(name)
                        .context("missing string offset for event name")?;
                    let payload_str = payload.to_string();
                    let (payload_offset, payload_len) = string_offsets
                        .get(&payload_str)
                        .context("missing string offset for event payload")?;
                    wat.push_str(&format!("    ;; EmitEvent name=\"{}\"\n", name));
                    wat.push_str(&format!(
                        "    (call $emit_event (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        name_offset, name_len, payload_offset, payload_len
                    ));
                }

                Instruction::StreamLatent { vector } => {
                    // Store vector in memory and call host
                    let vec_offset = data_offset;
                    let vec_len = vector.len() as u32;
                    wat.push_str(&format!("    ;; StreamLatent {} dimensions\n", vec_len));
                    wat.push_str(&format!(
                        "    (call $stream_latent (i32.const {}) (i32.const {}))\n",
                        vec_offset, vec_len
                    ));
                }

                Instruction::MorphProtocol { seed } => {
                    wat.push_str(&format!("    ;; MorphProtocol seed={}\n", seed));
                    wat.push_str(&format!(
                        "    (call $morph_protocol (i64.const {}))\n",
                        seed
                    ));
                }

                Instruction::DeclareState { name, initial_json } => {
                    let (name_offset, name_len) = string_offsets
                        .get(name)
                        .context("missing string offset for state name")?;
                    let initial_str = initial_json.to_string();
                    let (val_offset, val_len) = string_offsets
                        .get(&initial_str)
                        .context("missing string offset for state initial value")?;
                    wat.push_str(&format!("    ;; DeclareState name=\"{}\"\n", name));
                    wat.push_str(&format!(
                        "    (call $declare_state (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        name_offset, name_len, val_offset, val_len
                    ));
                }

                Instruction::UpdateState { name, value_json } => {
                    let (name_offset, name_len) = string_offsets
                        .get(name)
                        .context("missing string offset for update state name")?;
                    let value_str = value_json.to_string();
                    let (val_offset, val_len) = string_offsets
                        .get(&value_str)
                        .context("missing string offset for update state value")?;
                    wat.push_str(&format!("    ;; UpdateState name=\"{}\"\n", name));
                    wat.push_str(&format!(
                        "    (call $update_state (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        name_offset, name_len, val_offset, val_len
                    ));
                }

                Instruction::Decoy { noise } => {
                    let noise_len = noise.len() as u32;
                    wat.push_str(&format!("    ;; Decoy {} dimensions\n", noise_len));
                    wat.push_str(&format!(
                        "    (call $inject_decoy (i32.const 0) (i32.const {}))\n",
                        noise_len
                    ));
                }

                // --- Control Flow & Stack Operations ---
                Instruction::Push(val) => {
                    let val_str = val.to_string();
                    let (offset, len) = string_offsets
                        .get(&val_str)
                        .context("missing string offset for push value")?;
                    wat.push_str(&format!(
                        "    (call $push_value (i32.const {}) (i32.const {}))\n",
                        offset, len
                    ));
                }
                Instruction::Pop => {
                    wat.push_str("    (call $pop_value)\n");
                }
                Instruction::Load(name) => {
                    let (offset, len) = string_offsets
                        .get(name)
                        .context("missing string offset for load variable")?;
                    wat.push_str(&format!(
                        "    (call $load_var (i32.const {}) (i32.const {}))\n",
                        offset, len
                    ));
                }
                Instruction::Store(name) => {
                    let (offset, len) = string_offsets
                        .get(name)
                        .context("missing string offset for store variable")?;
                    wat.push_str(&format!(
                        "    (call $store_var (i32.const {}) (i32.const {}))\n",
                        offset, len
                    ));
                }
                Instruction::BinOp(op) => {
                    wat.push_str(&format!("    (call $bin_op (i32.const {}))\n", *op as i32));
                }
                Instruction::UnaryOp(op) => {
                    wat.push_str(&format!(
                        "    (call $unary_op (i32.const {}))\n",
                        *op as i32
                    ));
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
                    let (offset, len) = string_offsets
                        .get(name)
                        .context("missing string offset for call function name")?;
                    wat.push_str(&format!(
                        "    (call $call_func (i32.const {}) (i32.const {}) (i32.const {}))\n",
                        offset, len, num_args
                    ));
                }
                Instruction::CallTarget(target) => {
                    wat.push_str(&format!("    (call $call_target (i32.const {}))\n", target));
                }
                Instruction::Return => {
                    wat.push_str("    return\n");
                }

                // --- Stack-based DOM Operations ---
                Instruction::DefineElementFromStack { id } => {
                    wat.push_str(&format!(
                        "    (call $define_element_from_stack (i32.const {}))\n",
                        id
                    ));
                }
                Instruction::SetAttributeFromStack { id, key } => {
                    let (offset, len) = string_offsets
                        .get(key)
                        .context("missing string offset for stack attribute key")?;
                    wat.push_str(&format!("    (call $set_attribute_from_stack (i32.const {}) (i32.const {}) (i32.const {}))\n", id, offset, len));
                }
                Instruction::AddChildFromStack {
                    parent_id,
                    child_id,
                } => {
                    wat.push_str(&format!(
                        "    (call $add_child (i32.const {}) (i32.const {}))\n",
                        parent_id, child_id
                    ));
                }
                Instruction::EmitEventFromStack { name } => {
                    let (offset, len) = string_offsets
                        .get(name)
                        .context("missing string offset for stack event name")?;
                    wat.push_str(&format!(
                        "    (call $emit_event_from_stack (i32.const {}) (i32.const {}))\n",
                        offset, len
                    ));
                }
                Instruction::DefineTextFromStack => {
                    wat.push_str("    (call $define_text_from_stack)\n");
                }
                Instruction::DeclareStateFromStack { name } => {
                    let (offset, len) = string_offsets
                        .get(name)
                        .context("missing string offset for declare state name")?;
                    wat.push_str(&format!(
                        "    (call $declare_state_from_stack (i32.const {}) (i32.const {}))\n",
                        offset, len
                    ));
                }
                Instruction::UpdateStateFromStack { name } => {
                    let (offset, len) = string_offsets
                        .get(name)
                        .context("missing string offset for update state from stack name")?;
                    wat.push_str(&format!(
                        "    (call $update_state_from_stack (i32.const {}) (i32.const {}))\n",
                        offset, len
                    ));
                }
                Instruction::NavigateFromStack => {
                    wat.push_str("    (call $navigate_from_stack)\n");
                }
                Instruction::SearchFromStack => {
                    wat.push_str("    (call $search_from_stack)\n");
                }
                Instruction::StoreKnowledgeFromStack { tags } => {
                    let tags_json = serde_json::json!(tags).to_string();
                    if !string_offsets.contains_key(&tags_json) {
                        let len = tags_json.len() as u32;
                        string_offsets.insert(tags_json.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(&tags_json)
                        ));
                        data_offset += len + 1;
                    }
                    let (offset, len) = string_offsets
                        .get(&tags_json)
                        .context("missing string offset for store knowledge tags")?;
                    wat.push_str(&format!(
                        "    (call $store_knowledge_from_stack (i32.const {}) (i32.const {}))\n",
                        offset, len
                    ));
                }
                Instruction::QueryKnowledgeFromStack { tags, limit } => {
                    let tags_json = serde_json::json!(tags).to_string();
                    if !string_offsets.contains_key(&tags_json) {
                        let len = tags_json.len() as u32;
                        string_offsets.insert(tags_json.clone(), (data_offset, len));
                        data_section.push_str(&format!(
                            "  (data (i32.const {}) \"{}\")\n",
                            data_offset,
                            escape_wat_string(&tags_json)
                        ));
                        data_offset += len + 1;
                    }
                    let (offset, len) = string_offsets
                        .get(&tags_json)
                        .context("missing string offset for query knowledge tags")?;
                    wat.push_str(&format!("    (call $query_knowledge_from_stack (i32.const {}) (i32.const {}) (i32.const {}))\n", offset, len, limit));
                }
            }
        }

        // Return number of instructions executed
        wat.push_str(&format!("    (i32.const {})\n", binary.instructions.len()));
        wat.push_str("  )\n");
        wat.push_str(")\n");

        Ok(wat)
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
    /// Fuel units allocated to each `execute()` call. See
    /// [`DEFAULT_FUEL_BUDGET`] for the rationale.
    fuel_budget: u64,
}

/// Per-execution fuel budget for untrusted HLS code. wasmtime charges
/// roughly one fuel unit per bytecode instruction, so 1B units bounds
/// a single `execute()` call to ~seconds of CPU on modern hardware
/// before it gets trapped. Bound is enforced even when the host
/// function side does no work — a tight `loop {}` inside HLS traps.
///
/// This is the **production default**. Trusted callers that need to
/// run long-running compute should construct `WasmRuntime` via
/// [`WasmRuntime::with_fuel`] with a higher (or `u64::MAX`) value.
pub const DEFAULT_FUEL_BUDGET: u64 = 1_000_000_000;

/// Hard cap on the number of linear-memory pages a single HLS program
/// can grow to. wasmtime pages are 64 KiB, so 256 pages = 16 MiB —
/// enough for non-trivial DOM trees, small enough that a hostile
/// program cannot exhaust host RAM before the fuel budget kicks in.
pub const DEFAULT_MAX_MEMORY_PAGES: usize = 256;

impl WasmRuntime {
    /// Create a new WASM runtime with the production-default sandbox
    /// limits: 1B fuel units per `execute()`, 16 MiB memory cap.
    pub fn new() -> Result<Self> {
        Self::with_fuel(DEFAULT_FUEL_BUDGET)
    }

    /// Create a new WASM runtime with a custom fuel budget. Use
    /// `u64::MAX` to opt out of metering entirely (NOT recommended for
    /// untrusted code).
    pub fn with_fuel(fuel_budget: u64) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_bulk_memory(true);
        config.wasm_multi_value(true);
        // Sandbox controls — close the CPU-DoS vector for /api/sessions/
        // {id}/execute. consume_fuel is the wasmtime-recommended way to
        // bound an untrusted program's CPU; epoch interruption would
        // also work but requires a separate thread driving the ticker.
        config.consume_fuel(true);

        let engine = Engine::new(&config)?;

        Ok(Self {
            engine,
            fuel_budget,
        })
    }

    /// Execute an HLB binary using WebAssembly.
    ///
    /// Work is bounded three ways: [`MAX_INSTRUCTIONS`] before anything
    /// is compiled, guest fuel plus per-call host fuel while it runs, and
    /// the host budgets ([`MAX_STACK_DEPTH`], [`MAX_VALUE_BYTES`],
    /// [`MAX_LIVE_BYTES`], [`MAX_ELEMENTS`], [`MAX_EVENTS`],
    /// [`MAX_ACTIONS`], [`MAX_CALL_ARGS`]). A program that exceeds one is
    /// stopped and an error naming the budget is returned.
    pub fn execute(&self, binary: &SpineBinary) -> Result<WasmExecutionResult> {
        let compile_start = std::time::Instant::now();

        // Compile HLB to WAT (refuses an oversized program first)
        let wat = HlbToWatCompiler::compile(binary)?;

        // Parse WAT to WASM binary
        let wasm_bytes = wat::parse_str(&wat).context("Failed to parse WAT")?;

        let wasm_size = wasm_bytes.len();

        // Compile WASM module
        let module =
            Module::new(&self.engine, &wasm_bytes).context("Failed to compile WASM module")?;

        let compile_time = compile_start.elapsed();

        // Create store with host state and the guest resource limits
        let host_state = Arc::new(Mutex::new(HostState::default()));
        let limits = guest_limits();
        let mut store = Store::new(
            &self.engine,
            StoreData {
                host: host_state.clone(),
                limits,
            },
        );
        store.limiter(|data| &mut data.limits);
        // Charge the per-execution fuel budget. wasmtime traps with
        // `wasmtime::Trap::OutOfFuel` once the program consumes this
        // many bytecode units — bounds a tight loop inside HLS to a
        // sub-second of CPU. `set_fuel` only succeeds when `consume_fuel`
        // was enabled on the engine config (see `WasmRuntime::new`).
        store
            .set_fuel(self.fuel_budget)
            .context("failed to set fuel budget on WASM store")?;

        // Create linker with host functions
        let mut linker = Linker::new(&self.engine);

        // Define host functions
        Self::define_host_functions(&mut linker)?;

        // Instantiate module
        let instance = linker
            .instantiate(&mut store, &module)
            .context("Failed to instantiate WASM module")?;

        // Get execute function
        let execute_fn = instance
            .get_typed_func::<(), i32>(&mut store, "execute")
            .context("Failed to get execute function")?;

        // Execute
        let exec_start = std::time::Instant::now();
        let call = execute_fn.call(&mut store, ());
        let exec_time = exec_start.elapsed();
        let instruction_count = match call {
            Ok(n) => n,
            Err(e) => {
                // A host budget trap reaches here as a generic wasm
                // trap; report the budget by name instead.
                let violation = host_state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .violation
                    .clone();
                if let Some(msg) = violation {
                    anyhow::bail!("{msg}");
                }
                return Err(e.context("WASM execution failed"));
            }
        };

        // Real linear-memory size of the guest.
        let memory_used_bytes = instance
            .get_memory(&mut store, "memory")
            .map(|m| m.data_size(&store))
            .unwrap_or(0);

        // Extract results from host state
        let state = host_state.lock().unwrap_or_else(|p| p.into_inner());

        Ok(WasmExecutionResult {
            elements: state.elements.clone(),
            events: state.events.clone(),
            latent_streams: state.latent_streams.clone(),
            actions: state.actions.clone(),
            stats: WasmStats {
                compile_time_us: compile_time.as_micros() as u64,
                execution_time_us: exec_time.as_micros() as u64,
                wasm_size_bytes: wasm_size,
                memory_used_bytes,
                instructions_executed: instruction_count as usize,
            },
        })
    }

    /// Register the `env.*` host functions. Every one is fallible: a
    /// returned error traps the guest, which is how a host budget stops a
    /// program. Each charges [`HOST_CALL_FUEL`] plus the bytes it
    /// touches, and checks its budget before it allocates.
    fn define_host_functions(linker: &mut Linker<StoreData>) -> Result<()> {
        // define_element(id: i32, tag_ptr: i32, tag_len: i32)
        linker.func_wrap(
            "env",
            "define_element",
            |mut caller: Caller<'_, StoreData>,
             id: i32,
             tag_ptr: i32,
             tag_len: i32|
             -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let tag = read_guest_string(&mut caller, tag_ptr, tag_len)?;
                let mut state = host(&caller);
                state.add_element(WasmElement {
                    id: id as u32,
                    tag,
                    attributes: HashMap::new(),
                    parent_id: None,
                })?;
                state.instruction_count += 1;
                Ok(())
            },
        )?;

        // set_attribute(id: i32, key_ptr: i32, key_len: i32, val_ptr: i32, val_len: i32)
        linker.func_wrap(
            "env",
            "set_attribute",
            |mut caller: Caller<'_, StoreData>,
             id: i32,
             key_ptr: i32,
             key_len: i32,
             val_ptr: i32,
             val_len: i32|
             -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let key = read_guest_string(&mut caller, key_ptr, key_len)?;
                let value = read_guest_string(&mut caller, val_ptr, val_len)?;
                let scanned = host(&caller).elements.len() as u64;
                charge_fuel(&mut caller, scanned)?;
                let mut state = host(&caller);
                state.set_element_attribute(id as u32, key, value)?;
                state.instruction_count += 1;
                Ok(())
            },
        )?;

        // add_child(parent_id: i32, child_id: i32)
        linker.func_wrap(
            "env",
            "add_child",
            |mut caller: Caller<'_, StoreData>, parent_id: i32, child_id: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let scanned = host(&caller).elements.len() as u64;
                charge_fuel(&mut caller, scanned)?;
                let mut state = host(&caller);
                if let Some(child) = state.elements.iter_mut().find(|e| e.id == child_id as u32) {
                    child.parent_id = Some(parent_id as u32);
                }
                state.instruction_count += 1;
                Ok(())
            },
        )?;

        // emit_event(name_ptr: i32, name_len: i32, payload_ptr: i32, payload_len: i32)
        linker.func_wrap(
            "env",
            "emit_event",
            |mut caller: Caller<'_, StoreData>,
             name_ptr: i32,
             name_len: i32,
             payload_ptr: i32,
             payload_len: i32|
             -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let name = read_guest_string(&mut caller, name_ptr, name_len)?;
                let payload_str = read_guest_string(&mut caller, payload_ptr, payload_len)?;
                let payload: serde_json::Value = serde_json::from_str(&payload_str)
                    .unwrap_or(serde_json::Value::String(payload_str));
                let mut state = host(&caller);
                state.add_event(WasmEvent { name, payload })?;
                state.instruction_count += 1;
                Ok(())
            },
        )?;

        // push_value(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "push_value",
            |mut caller: Caller<'_, StoreData>, ptr: i32, len: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let s = read_guest_string(&mut caller, ptr, len)?;
                let val: serde_json::Value =
                    serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
                host(&caller).push_val(val)
            },
        )?;

        // pop_value()
        linker.func_wrap(
            "env",
            "pop_value",
            |mut caller: Caller<'_, StoreData>| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                host(&caller).pop_val();
                Ok(())
            },
        )?;

        // load_var(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "load_var",
            |mut caller: Caller<'_, StoreData>, ptr: i32, len: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let name = read_guest_string(&mut caller, ptr, len)?;
                let mut state = host(&caller);
                let val = state
                    .variables
                    .get(&name)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                state.push_val(val)
            },
        )?;

        // store_var(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "store_var",
            |mut caller: Caller<'_, StoreData>, ptr: i32, len: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let name = read_guest_string(&mut caller, ptr, len)?;
                let mut state = host(&caller);
                if let Some(val) = state.pop_val() {
                    state.set_var(name, val)?;
                }
                Ok(())
            },
        )?;

        // bin_op(op_code: i32)
        linker.func_wrap(
            "env",
            "bin_op",
            |mut caller: Caller<'_, StoreData>, op_code: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let mut state = host(&caller);
                let right = state.pop_val().unwrap_or(serde_json::Value::Null);
                let left = state.pop_val().unwrap_or(serde_json::Value::Null);

                // Simple evaluation for now
                let res = match op_code {
                    0 => {
                        // Add
                        match (left, right) {
                            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                                serde_json::json!(
                                    l.as_f64().unwrap_or(0.0) + r.as_f64().unwrap_or(0.0)
                                )
                            }
                            (serde_json::Value::String(l), serde_json::Value::String(r)) => {
                                // Size is checked before the string is built.
                                let total = l.len().saturating_add(r.len());
                                if total > MAX_VALUE_BYTES {
                                    return Err(state.violate(
                                        "MAX_VALUE_BYTES",
                                        format!("string of {total} bytes, limit {MAX_VALUE_BYTES}"),
                                    ));
                                }
                                let mut s = String::with_capacity(total);
                                s.push_str(&l);
                                s.push_str(&r);
                                serde_json::Value::String(s)
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    1 => {
                        // Sub
                        match (left, right) {
                            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                                serde_json::json!(
                                    l.as_f64().unwrap_or(0.0) - r.as_f64().unwrap_or(0.0)
                                )
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    2 => {
                        // Mul
                        match (left, right) {
                            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                                serde_json::json!(
                                    l.as_f64().unwrap_or(0.0) * r.as_f64().unwrap_or(0.0)
                                )
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    3 => {
                        // Div
                        match (left, right) {
                            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                                let r_val = r.as_f64().unwrap_or(0.0);
                                if r_val != 0.0 {
                                    serde_json::json!(l.as_f64().unwrap_or(0.0) / r_val)
                                } else {
                                    serde_json::Value::Null
                                }
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    5 => {
                        // Eq
                        serde_json::Value::Bool(left == right)
                    }
                    6 => {
                        // Ne
                        serde_json::Value::Bool(left != right)
                    }
                    11 => {
                        // And
                        match (left, right) {
                            (serde_json::Value::Bool(l), serde_json::Value::Bool(r)) => {
                                serde_json::Value::Bool(l && r)
                            }
                            _ => serde_json::Value::Bool(false),
                        }
                    }
                    12 => {
                        // Or
                        match (left, right) {
                            (serde_json::Value::Bool(l), serde_json::Value::Bool(r)) => {
                                serde_json::Value::Bool(l || r)
                            }
                            _ => serde_json::Value::Bool(false),
                        }
                    }
                    13 => {
                        // Concat. The rendered length is counted first so
                        // a doubling program is refused before it
                        // allocates.
                        match concat_len(&left, &right) {
                            Some(_) => serde_json::Value::String(format!("{}{}", left, right)),
                            None => {
                                return Err(state.violate(
                                    "MAX_VALUE_BYTES",
                                    format!("concatenation over {MAX_VALUE_BYTES} bytes"),
                                ));
                            }
                        }
                    }
                    _ => serde_json::Value::Null,
                };
                state.push_val(res)
            },
        )?;

        // define_element_from_stack(id: i32)
        linker.func_wrap(
            "env",
            "define_element_from_stack",
            |mut caller: Caller<'_, StoreData>, id: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let mut state = host(&caller);
                let tag = state.pop_str().unwrap_or_else(|| "div".to_string());
                state.add_element(WasmElement {
                    id: id as u32,
                    tag,
                    attributes: HashMap::new(),
                    parent_id: None,
                })
            },
        )?;

        // set_attribute_from_stack(id: i32, key_ptr: i32, key_len: i32)
        linker.func_wrap(
            "env",
            "set_attribute_from_stack",
            |mut caller: Caller<'_, StoreData>,
             id: i32,
             key_ptr: i32,
             key_len: i32|
             -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let key = read_guest_string(&mut caller, key_ptr, key_len)?;
                let scanned = host(&caller).elements.len() as u64;
                charge_fuel(&mut caller, scanned)?;
                let mut state = host(&caller);
                let val = state.pop_val().map(|v| v.to_string()).unwrap_or_default();
                state.set_element_attribute(id as u32, key, val)
            },
        )?;

        // define_text_from_stack()
        linker.func_wrap(
            "env",
            "define_text_from_stack",
            |mut caller: Caller<'_, StoreData>| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let mut state = host(&caller);
                let text = state
                    .pop_val()
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();

                // Text elements are special, for now we'll just create a "text" element
                let id = state.elements.len() as u32 + 1000; // Simple ID generation
                state.add_element(WasmElement {
                    id,
                    tag: "text".to_string(),
                    attributes: [("content".to_string(), text)].into_iter().collect(),
                    parent_id: None,
                })
            },
        )?;

        // declare_state_from_stack(name_ptr: i32, name_len: i32)
        linker.func_wrap(
            "env",
            "declare_state_from_stack",
            |mut caller: Caller<'_, StoreData>, name_ptr: i32, name_len: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let name = read_guest_string(&mut caller, name_ptr, name_len)?;
                let mut state = host(&caller);
                if let Some(val) = state.pop_val() {
                    state.set_var(name, val)?;
                }
                Ok(())
            },
        )?;

        // update_state_from_stack(name_ptr: i32, name_len: i32)
        linker.func_wrap(
            "env",
            "update_state_from_stack",
            |mut caller: Caller<'_, StoreData>, name_ptr: i32, name_len: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let name = read_guest_string(&mut caller, name_ptr, name_len)?;
                let mut state = host(&caller);
                if let Some(val) = state.pop_val() {
                    state.set_var(name, val)?;
                }
                Ok(())
            },
        )?;

        // navigate_from_stack()
        linker.func_wrap(
            "env",
            "navigate_from_stack",
            |mut caller: Caller<'_, StoreData>| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let mut state = host(&caller);
                if let Some(url) = state.pop_str() {
                    state.add_action(WasmAction::Navigate(url))?;
                }
                Ok(())
            },
        )?;

        // search_from_stack()
        linker.func_wrap(
            "env",
            "search_from_stack",
            |mut caller: Caller<'_, StoreData>| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let mut state = host(&caller);
                if let Some(query) = state.pop_str() {
                    state.add_action(WasmAction::Search(query))?;
                }
                Ok(())
            },
        )?;

        // store_knowledge_from_stack(tags_ptr: i32, tags_len: i32)
        linker.func_wrap(
            "env",
            "store_knowledge_from_stack",
            |mut caller: Caller<'_, StoreData>, tags_ptr: i32, tags_len: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let tags_str = read_guest_string(&mut caller, tags_ptr, tags_len)?;
                let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();

                let mut state = host(&caller);
                let value = state.pop_val().unwrap_or(serde_json::Value::Null);
                let key = state.pop_str().unwrap_or_default();
                state.add_action(WasmAction::StoreKnowledge { key, value, tags })
            },
        )?;

        // query_knowledge_from_stack(tags_ptr: i32, tags_len: i32, limit: i32)
        linker.func_wrap(
            "env",
            "query_knowledge_from_stack",
            |mut caller: Caller<'_, StoreData>,
             tags_ptr: i32,
             tags_len: i32,
             limit: i32|
             -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let tags_str = read_guest_string(&mut caller, tags_ptr, tags_len)?;
                let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();

                let mut state = host(&caller);
                let query = state.pop_str().unwrap_or_default();
                state.add_action(WasmAction::QueryKnowledge {
                    query,
                    tags,
                    limit: limit.max(0) as usize,
                })
            },
        )?;

        // call_func(name_ptr: i32, name_len: i32, num_args: i32)
        linker.func_wrap(
            "env",
            "call_func",
            |mut caller: Caller<'_, StoreData>,
             name_ptr: i32,
             name_len: i32,
             num_args: i32|
             -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let name = read_guest_string(&mut caller, name_ptr, name_len)?;

                // The count is checked before anything is popped or
                // allocated: it is a guest-supplied i32.
                if num_args < 0 || num_args as usize > MAX_CALL_ARGS {
                    return Err(host(&caller).violate(
                        "MAX_CALL_ARGS",
                        format!("num_args {num_args}, limit {MAX_CALL_ARGS}"),
                    ));
                }
                charge_fuel(&mut caller, num_args as u64)?;

                let mut state = host(&caller);
                let mut args = Vec::with_capacity(num_args as usize);
                for _ in 0..num_args {
                    args.push(state.pop_val().unwrap_or(serde_json::Value::Null));
                }
                args.reverse();

                // Built-in functions
                let res = match name.as_str() {
                    "len" => {
                        let arg = state.pop_val().unwrap_or(serde_json::Value::Null);
                        match arg {
                            serde_json::Value::Array(a) => serde_json::json!(a.len()),
                            serde_json::Value::String(s) => serde_json::json!(s.len()),
                            serde_json::Value::Object(m) => serde_json::json!(m.len()),
                            _ => serde_json::json!(0),
                        }
                    }
                    "str" => {
                        let arg = state.pop_val().unwrap_or(serde_json::Value::Null);
                        match arg {
                            serde_json::Value::String(s) => serde_json::Value::String(s),
                            _ => serde_json::Value::String(arg.to_string()),
                        }
                    }
                    "num" => {
                        let arg = state.pop_val().unwrap_or(serde_json::Value::Null);
                        let n = match arg {
                            serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
                            serde_json::Value::String(s) => s.parse().unwrap_or(0.0),
                            serde_json::Value::Bool(b) if b => 1.0,
                            _ => 0.0,
                        };
                        serde_json::json!(n)
                    }
                    "print" => {
                        // Guest-controlled text goes to the log at debug
                        // level, not to the daemon's stdout.
                        let arg = state.pop_val().unwrap_or(serde_json::Value::Null);
                        log::debug!("[HLS PRINT] {}", arg);
                        serde_json::Value::Null
                    }
                    "list" => {
                        let mut items = Vec::new();
                        for _ in 0..num_args {
                            if let Some(val) = state.pop_val() {
                                items.push(val);
                            }
                        }
                        items.reverse();
                        serde_json::Value::Array(items)
                    }
                    "object" => {
                        let mut map = serde_json::Map::new();
                        for _ in 0..(num_args / 2) {
                            let val = state.pop_val().unwrap_or(serde_json::Value::Null);
                            let key = state.pop_str().unwrap_or_default();
                            map.insert(key, val);
                        }
                        serde_json::Value::Object(map)
                    }
                    "emit" => {
                        let payload = if num_args > 1 {
                            state.pop_val().unwrap_or(serde_json::Value::Null)
                        } else {
                            serde_json::Value::Null
                        };
                        let event_name = state.pop_str().unwrap_or_default();
                        state.add_event(WasmEvent {
                            name: event_name,
                            payload,
                        })?;
                        serde_json::Value::Null
                    }
                    "reason" => {
                        let query = state.pop_str().unwrap_or_default();
                        state.add_action(WasmAction::Reason(query))?;
                        serde_json::Value::Null
                    }
                    "remember" => {
                        let value = state.pop_val().unwrap_or(serde_json::Value::Null);
                        let key = state.pop_str().unwrap_or_default();
                        state.add_action(WasmAction::StoreKnowledge {
                            key,
                            value,
                            tags: vec!["hls".to_string()],
                        })?;
                        serde_json::Value::Null
                    }
                    "query_memory" => {
                        let query = state.pop_str().unwrap_or_default();
                        state.add_action(WasmAction::QueryKnowledge {
                            query,
                            tags: vec![],
                            limit: 5,
                        })?;
                        serde_json::Value::Null
                    }
                    _ => serde_json::Value::Null,
                };
                state.push_val(res)
            },
        )?;

        // emit_event_from_stack(name_ptr: i32, name_len: i32)
        linker.func_wrap(
            "env",
            "emit_event_from_stack",
            |mut caller: Caller<'_, StoreData>, name_ptr: i32, name_len: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let name = read_guest_string(&mut caller, name_ptr, name_len)?;
                let mut state = host(&caller);
                let payload = state.pop_val().unwrap_or(serde_json::Value::Null);
                state.add_event(WasmEvent { name, payload })
            },
        )?;

        // unary_op(op_code: i32)
        linker.func_wrap(
            "env",
            "unary_op",
            |mut caller: Caller<'_, StoreData>, op_code: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let mut state = host(&caller);
                let val = state.pop_val().unwrap_or(serde_json::Value::Null);

                let res = match op_code {
                    0 => {
                        // Not
                        match val {
                            serde_json::Value::Bool(b) => serde_json::Value::Bool(!b),
                            _ => serde_json::Value::Bool(false),
                        }
                    }
                    1 => {
                        // Neg
                        match val {
                            serde_json::Value::Number(n) => {
                                serde_json::json!(-n.as_f64().unwrap_or(0.0))
                            }
                            _ => serde_json::Value::Null,
                        }
                    }
                    _ => serde_json::Value::Null,
                };
                state.push_val(res)
            },
        )?;

        // call_target(target: i32)
        linker.func_wrap(
            "env",
            "call_target",
            |mut caller: Caller<'_, StoreData>, _target: i32| -> Result<()> {
                // Internal calls not yet supported in WASM runtime
                charge_fuel(&mut caller, HOST_CALL_FUEL)
            },
        )?;

        // stream_latent(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "stream_latent",
            |mut caller: Caller<'_, StoreData>, _ptr: i32, len: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                // `len` is a dimension count the guest supplies: check it
                // before the vector is allocated.
                if len < 0 || len as usize > MAX_LATENT_DIMS {
                    return Err(host(&caller).violate(
                        "MAX_LATENT_DIMS",
                        format!("latent vector of {len} dimensions, limit {MAX_LATENT_DIMS}"),
                    ));
                }
                charge_fuel(&mut caller, len as u64)?;
                let mut state = host(&caller);
                if state.latent_streams.len() >= MAX_LATENT_STREAMS {
                    return Err(state.violate(
                        "MAX_LATENT_STREAMS",
                        format!("more than {MAX_LATENT_STREAMS} latent vectors"),
                    ));
                }
                state.charge(RECORD_OVERHEAD + len as usize * std::mem::size_of::<f32>())?;
                state.latent_streams.push(vec![0.0f32; len as usize]);
                state.instruction_count += 1;
                Ok(())
            },
        )?;

        // morph_protocol(seed: i64)
        linker.func_wrap(
            "env",
            "morph_protocol",
            |mut caller: Caller<'_, StoreData>, _seed: i64| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                // Morphing would be handled by the protocol layer
                host(&caller).instruction_count += 1;
                Ok(())
            },
        )?;

        // inject_decoy(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "inject_decoy",
            |mut caller: Caller<'_, StoreData>, _ptr: i32, _len: i32| -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                // Decoy injection would be handled by the protocol layer
                host(&caller).instruction_count += 1;
                Ok(())
            },
        )?;

        // declare_state(name_ptr: i32, name_len: i32, val_ptr: i32, val_len: i32)
        linker.func_wrap(
            "env",
            "declare_state",
            |mut caller: Caller<'_, StoreData>,
             name_ptr: i32,
             name_len: i32,
             val_ptr: i32,
             val_len: i32|
             -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let name = read_guest_string(&mut caller, name_ptr, name_len)?;
                let val_str = read_guest_string(&mut caller, val_ptr, val_len)?;
                let val: serde_json::Value =
                    serde_json::from_str(&val_str).unwrap_or(serde_json::Value::Null);

                let mut state = host(&caller);
                state.add_event(WasmEvent {
                    name: "state_declared".to_string(),
                    payload: serde_json::json!({ "name": name, "value": val }),
                })?;
                state.instruction_count += 1;
                Ok(())
            },
        )?;

        // update_state(name_ptr: i32, name_len: i32, val_ptr: i32, val_len: i32)
        linker.func_wrap(
            "env",
            "update_state",
            |mut caller: Caller<'_, StoreData>,
             name_ptr: i32,
             name_len: i32,
             val_ptr: i32,
             val_len: i32|
             -> Result<()> {
                charge_fuel(&mut caller, HOST_CALL_FUEL)?;
                let name = read_guest_string(&mut caller, name_ptr, name_len)?;
                let val_str = read_guest_string(&mut caller, val_ptr, val_len)?;
                let val: serde_json::Value =
                    serde_json::from_str(&val_str).unwrap_or(serde_json::Value::Null);

                let mut state = host(&caller);
                state.add_event(WasmEvent {
                    name: "state_updated".to_string(),
                    payload: serde_json::json!({ "name": name, "value": val }),
                })?;
                state.instruction_count += 1;
                Ok(())
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
    use std::collections::HashMap;

    fn empty_binary(instructions: Vec<Instruction>) -> SpineBinary {
        SpineBinary {
            instructions,
            data: Vec::new(),
            render_start: 0,
            exported_functions: std::collections::BTreeMap::new(),
            capabilities: Vec::new(),
        }
    }

    #[test]
    fn test_compile_simple_hlb_to_wat() {
        let binary = empty_binary(vec![
            Instruction::DefineElement {
                id: 1,
                tag: "div".to_string(),
            },
            Instruction::SetAttribute {
                id: 1,
                key: "class".to_string(),
                value: "container".to_string(),
            },
        ]);

        let wat = HlbToWatCompiler::compile(&binary).unwrap();

        assert!(wat.contains("(module"));
        assert!(wat.contains("define_element"));
        assert!(wat.contains("set_attribute"));
        assert!(wat.contains("(export \"execute\")"));
    }

    #[test]
    fn test_wasm_execution() {
        let runtime = WasmRuntime::new().unwrap();

        let binary = empty_binary(vec![
            Instruction::DefineElement {
                id: 1,
                tag: "div".to_string(),
            },
            Instruction::DefineElement {
                id: 2,
                tag: "span".to_string(),
            },
            Instruction::AddChild {
                parent_id: 1,
                child_id: 2,
            },
            Instruction::SetAttribute {
                id: 2,
                key: "text".to_string(),
                value: "Hello".to_string(),
            },
        ]);

        let result = runtime.execute(&binary).unwrap();

        assert_eq!(result.elements.len(), 2);
        assert_eq!(result.stats.instructions_executed, 4);
    }

    #[test]
    fn test_wasm_events() {
        let runtime = WasmRuntime::new().unwrap();

        let binary = empty_binary(vec![Instruction::EmitEvent {
            name: "click".to_string(),
            payload: serde_json::json!({"id": 42}),
        }]);

        let result = runtime.execute(&binary).unwrap();

        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].name, "click");
    }

    #[test]
    fn test_escape_wat_string_basic() {
        assert_eq!(escape_wat_string("hello"), "hello");
        assert_eq!(escape_wat_string("hello world"), "hello world");
    }

    #[test]
    fn test_escape_wat_string_special_chars() {
        assert_eq!(escape_wat_string("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(escape_wat_string("back\\slash"), "back\\\\slash");
        assert_eq!(escape_wat_string("new\nline"), "new\\nline");
        assert_eq!(escape_wat_string("tab\there"), "tab\\there");
        assert_eq!(escape_wat_string("cr\rhere"), "cr\\rhere");
    }

    #[test]
    fn test_compile_empty_binary() {
        let binary = empty_binary(vec![]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("(module"));
        assert!(wat.contains("(i32.const 0)"));
        assert!(wat.contains("(export \"execute\")"));
    }

    #[test]
    fn test_execute_empty_binary() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![]);
        let result = runtime.execute(&binary).unwrap();
        assert_eq!(result.elements.len(), 0);
        assert_eq!(result.events.len(), 0);
        assert_eq!(result.actions.len(), 0);
        assert_eq!(result.stats.instructions_executed, 0);
    }

    /// REGRESSION GUARD — wasmtime's `consume_fuel` metering must be
    /// active. A runtime constructed with zero fuel must trap on the
    /// first instruction of any non-trivial HLS program. If this test
    /// ever passes the assertion in reverse (program completes), the
    /// CPU-DoS gate on `/api/sessions/{id}/execute` is gone and a
    /// tight loop inside HLS can hang the gateway.
    #[test]
    fn test_wasm_fuel_metering_is_active() {
        let runtime = WasmRuntime::with_fuel(0).unwrap();
        let binary = empty_binary(vec![
            Instruction::DefineElement {
                id: 1,
                tag: "div".to_string(),
            },
            Instruction::DefineElement {
                id: 2,
                tag: "span".to_string(),
            },
        ]);
        let err = runtime
            .execute(&binary)
            .expect_err("zero-fuel runtime should trap before completing any work");
        let msg = format!("{err:#}");
        assert!(
            msg.to_lowercase().contains("fuel")
                || msg.to_lowercase().contains("trap")
                || msg.to_lowercase().contains("interrupt"),
            "expected a fuel/trap error, got: {msg}"
        );
    }

    /// REGRESSION GUARD — the default fuel budget must be large enough
    /// to run real HLS programs. If this ever fails, the production
    /// default in `DEFAULT_FUEL_BUDGET` is too tight and legitimate
    /// callers will see spurious traps.
    #[test]
    fn test_default_fuel_budget_completes_normal_programs() {
        let runtime = WasmRuntime::new().unwrap();
        // A modestly-sized program — multiple host calls, state ops,
        // events. Representative of a real HLS render.
        let mut instructions: Vec<Instruction> = (0..50)
            .map(|i| Instruction::DefineElement {
                id: i,
                tag: format!("tag{i}"),
            })
            .collect();
        instructions.extend((0..50).map(|i| Instruction::SetAttribute {
            id: i,
            key: "class".to_string(),
            value: format!("c{i}"),
        }));
        let binary = empty_binary(instructions);
        let _ = runtime
            .execute(&binary)
            .expect("default fuel budget should complete a 100-instruction program");
    }

    #[test]
    fn test_compile_declare_state() {
        let binary = empty_binary(vec![Instruction::DeclareState {
            name: "counter".to_string(),
            initial_json: serde_json::json!(0),
        }]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("declare_state"));
        assert!(wat.contains("counter"));
    }

    #[test]
    fn test_compile_update_state() {
        let binary = empty_binary(vec![
            Instruction::DeclareState {
                name: "x".to_string(),
                initial_json: serde_json::json!(1),
            },
            Instruction::UpdateState {
                name: "x".to_string(),
                value_json: serde_json::json!(2),
            },
        ]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("update_state"));
    }

    #[test]
    fn test_execute_state_declaration() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![Instruction::DeclareState {
            name: "counter".to_string(),
            initial_json: serde_json::json!(42),
        }]);
        let result = runtime.execute(&binary).unwrap();
        // DeclareState emits a state_declared event
        assert!(result.events.iter().any(|e| e.name == "state_declared"));
    }

    #[test]
    fn test_compile_morph_protocol() {
        let binary = empty_binary(vec![Instruction::MorphProtocol { seed: 12345 }]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("morph_protocol"));
        assert!(wat.contains("12345"));
    }

    #[test]
    fn test_compile_stream_latent() {
        let binary = empty_binary(vec![Instruction::StreamLatent {
            vector: vec![1.0, 2.0, 3.0],
        }]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("stream_latent"));
        assert!(wat.contains("3")); // dimension count
    }

    #[test]
    fn test_compile_decoy() {
        let binary = empty_binary(vec![Instruction::Decoy {
            noise: vec![0.1, 0.2],
        }]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("inject_decoy"));
    }

    #[test]
    fn test_compile_push_pop() {
        let binary = empty_binary(vec![
            Instruction::Push(serde_json::json!("hello")),
            Instruction::Pop,
        ]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("push_value"));
        assert!(wat.contains("pop_value"));
    }

    #[test]
    fn test_compile_load_store_var() {
        let binary = empty_binary(vec![
            Instruction::Push(serde_json::json!(99)),
            Instruction::Store("x".to_string()),
            Instruction::Load("x".to_string()),
        ]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("store_var"));
        assert!(wat.contains("load_var"));
    }

    #[test]
    fn test_compile_binop() {
        use spine_protocol::ProtocolBinOp;
        let binary = empty_binary(vec![Instruction::BinOp(ProtocolBinOp::Add)]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("bin_op"));
    }

    #[test]
    fn test_compile_call() {
        let binary = empty_binary(vec![Instruction::Call {
            name: "my_func".to_string(),
            num_args: 2,
        }]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("call_func"));
        assert!(wat.contains("my_func"));
    }

    #[test]
    fn test_compile_navigate_from_stack() {
        let binary = empty_binary(vec![Instruction::NavigateFromStack]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("navigate_from_stack"));
    }

    #[test]
    fn test_knowledge_instructions_compile_to_a_module_that_parses() {
        // Both instructions call host functions and carry their tag list as
        // string data: the module must import the functions and hold the data.
        let binary = empty_binary(vec![
            Instruction::Push(serde_json::json!("key")),
            Instruction::Push(serde_json::json!({"v": 1})),
            Instruction::StoreKnowledgeFromStack {
                tags: vec!["t".into()],
            },
            Instruction::Push(serde_json::json!("question")),
            Instruction::QueryKnowledgeFromStack {
                tags: vec!["u".into()],
                limit: 3,
            },
        ]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("$store_knowledge_from_stack (param i32 i32)"));
        assert!(wat.contains("$query_knowledge_from_stack (param i32 i32 i32)"));
        assert!(
            wat.matches("(data ").count() >= 5,
            "three pushed values and two tag lists"
        );
        wat::parse_str(&wat).expect("the module parses");
    }

    #[test]
    fn test_compile_search_from_stack() {
        let binary = empty_binary(vec![Instruction::SearchFromStack]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("search_from_stack"));
    }

    #[test]
    fn test_execute_multiple_elements() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![
            Instruction::DefineElement {
                id: 1,
                tag: "html".to_string(),
            },
            Instruction::DefineElement {
                id: 2,
                tag: "head".to_string(),
            },
            Instruction::DefineElement {
                id: 3,
                tag: "body".to_string(),
            },
            Instruction::AddChild {
                parent_id: 1,
                child_id: 2,
            },
            Instruction::AddChild {
                parent_id: 1,
                child_id: 3,
            },
        ]);
        let result = runtime.execute(&binary).unwrap();
        assert_eq!(result.elements.len(), 3);
        assert_eq!(result.elements[0].tag, "html");
        assert_eq!(result.elements[1].tag, "head");
        assert_eq!(result.elements[2].tag, "body");
        assert_eq!(result.elements[1].parent_id, Some(1));
        assert_eq!(result.elements[2].parent_id, Some(1));
    }

    #[test]
    fn test_execute_multiple_events() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![
            Instruction::EmitEvent {
                name: "load".to_string(),
                payload: serde_json::json!(null),
            },
            Instruction::EmitEvent {
                name: "ready".to_string(),
                payload: serde_json::json!({"status": "ok"}),
            },
        ]);
        let result = runtime.execute(&binary).unwrap();
        assert_eq!(result.events.len(), 2);
        assert_eq!(result.events[0].name, "load");
        assert_eq!(result.events[1].name, "ready");
    }

    #[test]
    fn test_execute_set_attribute_creates_attr() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![
            Instruction::DefineElement {
                id: 1,
                tag: "div".to_string(),
            },
            Instruction::SetAttribute {
                id: 1,
                key: "class".to_string(),
                value: "main".to_string(),
            },
            Instruction::SetAttribute {
                id: 1,
                key: "id".to_string(),
                value: "root".to_string(),
            },
        ]);
        let result = runtime.execute(&binary).unwrap();
        assert_eq!(result.elements[0].attributes.get("class").unwrap(), "main");
        assert_eq!(result.elements[0].attributes.get("id").unwrap(), "root");
    }

    #[test]
    fn test_wasm_stats_populated() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![Instruction::DefineElement {
            id: 1,
            tag: "p".to_string(),
        }]);
        let result = runtime.execute(&binary).unwrap();
        assert!(result.stats.wasm_size_bytes > 0);
        assert_eq!(result.stats.memory_used_bytes, 65536);
        assert_eq!(result.stats.instructions_executed, 1);
    }

    #[test]
    fn test_wasm_runtime_default() {
        let runtime = WasmRuntime::default();
        let binary = empty_binary(vec![]);
        let result = runtime.execute(&binary).unwrap();
        assert_eq!(result.stats.instructions_executed, 0);
    }

    #[test]
    fn test_wasm_action_variants() {
        // Just test that the enum variants serialize/deserialize correctly
        let actions = vec![
            WasmAction::Navigate("https://example.com".into()),
            WasmAction::Search("rust lang".into()),
            WasmAction::StoreKnowledge {
                key: "k".into(),
                value: serde_json::json!(1),
                tags: vec!["t".into()],
            },
            WasmAction::QueryKnowledge {
                query: "q".into(),
                tags: vec![],
                limit: 5,
            },
            WasmAction::Reason("why?".into()),
        ];
        for action in &actions {
            let json = serde_json::to_string(action).unwrap();
            let _: WasmAction = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_wasm_execution_result_serialization() {
        let result = WasmExecutionResult {
            elements: vec![WasmElement {
                id: 1,
                tag: "div".into(),
                attributes: HashMap::new(),
                parent_id: None,
            }],
            events: vec![WasmEvent {
                name: "test".into(),
                payload: serde_json::json!(null),
            }],
            latent_streams: vec![vec![1.0, 2.0]],
            actions: vec![],
            stats: WasmStats::default(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deser: WasmExecutionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.elements.len(), 1);
        assert_eq!(deser.events.len(), 1);
    }

    #[test]
    fn test_compile_add_child() {
        let binary = empty_binary(vec![
            Instruction::DefineElement {
                id: 10,
                tag: "ul".to_string(),
            },
            Instruction::DefineElement {
                id: 20,
                tag: "li".to_string(),
            },
            Instruction::AddChild {
                parent_id: 10,
                child_id: 20,
            },
        ]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        assert!(wat.contains("add_child"));
        assert!(wat.contains("i32.const 10"));
        assert!(wat.contains("i32.const 20"));
    }

    #[test]
    fn test_compile_instruction_count_return() {
        let binary = empty_binary(vec![
            Instruction::DefineElement {
                id: 1,
                tag: "a".to_string(),
            },
            Instruction::DefineElement {
                id: 2,
                tag: "b".to_string(),
            },
            Instruction::DefineElement {
                id: 3,
                tag: "c".to_string(),
            },
        ]);
        let wat = HlbToWatCompiler::compile(&binary).unwrap();
        // The function returns the instruction count as i32.const N
        assert!(wat.contains("(i32.const 3)"));
    }

    // --- Host budgets (see the MAX_* constants) -----------------------

    fn err_text(result: Result<WasmExecutionResult>) -> String {
        format!(
            "{:#}",
            result.expect_err("program should have been refused")
        )
    }

    /// `x = "a"`, then `rounds` times `x = x <op> x`.
    fn doubling_program(rounds: usize, op: spine_protocol::ProtocolBinOp) -> SpineBinary {
        let mut ins = vec![
            Instruction::Push(serde_json::json!("a")),
            Instruction::Store("x".to_string()),
        ];
        for _ in 0..rounds {
            ins.push(Instruction::Load("x".to_string()));
            ins.push(Instruction::Load("x".to_string()));
            ins.push(Instruction::BinOp(op));
            ins.push(Instruction::Store("x".to_string()));
        }
        empty_binary(ins)
    }

    #[test]
    fn test_doubling_concat_is_refused_quickly() {
        let runtime = WasmRuntime::new().unwrap();
        let start = std::time::Instant::now();
        // 40 rounds would ask for terabytes if unchecked.
        let msg =
            err_text(runtime.execute(&doubling_program(40, spine_protocol::ProtocolBinOp::Concat)));
        assert!(msg.contains("MAX_VALUE_BYTES"), "got: {msg}");
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }

    #[test]
    fn test_doubling_string_add_is_refused() {
        let runtime = WasmRuntime::new().unwrap();
        let msg =
            err_text(runtime.execute(&doubling_program(40, spine_protocol::ProtocolBinOp::Add)));
        assert!(msg.contains("MAX_VALUE_BYTES"), "got: {msg}");
    }

    #[test]
    fn test_huge_num_args_is_refused() {
        let runtime = WasmRuntime::new().unwrap();
        for n in [MAX_CALL_ARGS + 1, 1_000_000_000, 2_000_000_000] {
            let binary = empty_binary(vec![Instruction::Call {
                name: "list".to_string(),
                num_args: n,
            }]);
            let msg = err_text(runtime.execute(&binary));
            assert!(msg.contains("MAX_CALL_ARGS"), "n={n}, got: {msg}");
        }
        // The cap itself is allowed.
        let binary = empty_binary(vec![Instruction::Call {
            name: "list".to_string(),
            num_args: MAX_CALL_ARGS,
        }]);
        runtime.execute(&binary).expect("num_args at the cap runs");
    }

    #[test]
    fn test_stack_depth_cap() {
        let runtime = WasmRuntime::new().unwrap();
        let push = |n: usize| {
            empty_binary(
                (0..n)
                    .map(|_| Instruction::Push(serde_json::json!(1)))
                    .collect(),
            )
        };
        runtime
            .execute(&push(MAX_STACK_DEPTH))
            .expect("a stack at the cap runs");
        let msg = err_text(runtime.execute(&push(MAX_STACK_DEPTH + 1)));
        assert!(msg.contains("MAX_STACK_DEPTH"), "got: {msg}");
    }

    #[test]
    fn test_value_bytes_cap() {
        let runtime = WasmRuntime::new().unwrap();
        let big = "z".repeat(MAX_VALUE_BYTES / 2 + 1000);
        let binary = empty_binary(vec![
            Instruction::Push(serde_json::json!(big)),
            Instruction::Push(serde_json::json!(big)),
            Instruction::BinOp(spine_protocol::ProtocolBinOp::Add),
        ]);
        let msg = err_text(runtime.execute(&binary));
        assert!(msg.contains("MAX_VALUE_BYTES"), "got: {msg}");
    }

    #[test]
    fn test_live_bytes_cap() {
        let runtime = WasmRuntime::new().unwrap();
        // Each value is under the per-value cap; together they are not.
        let big = "z".repeat(50_000);
        let binary = empty_binary(
            (0..30)
                .map(|_| Instruction::Push(serde_json::json!(big)))
                .collect(),
        );
        let msg = err_text(runtime.execute(&binary));
        assert!(msg.contains("MAX_LIVE_BYTES"), "got: {msg}");
    }

    #[test]
    fn test_element_cap() {
        let runtime = WasmRuntime::new().unwrap();
        let make = |n: usize| {
            empty_binary(
                (0..n)
                    .map(|i| Instruction::DefineElement {
                        id: i as u32,
                        tag: "d".to_string(),
                    })
                    .collect(),
            )
        };
        let ok = runtime.execute(&make(MAX_ELEMENTS)).unwrap();
        assert_eq!(ok.elements.len(), MAX_ELEMENTS);
        let msg = err_text(runtime.execute(&make(MAX_ELEMENTS + 1)));
        assert!(msg.contains("MAX_ELEMENTS"), "got: {msg}");
    }

    #[test]
    fn test_event_cap() {
        let runtime = WasmRuntime::new().unwrap();
        let make = |n: usize| {
            empty_binary(
                (0..n)
                    .map(|_| Instruction::EmitEvent {
                        name: "e".to_string(),
                        payload: serde_json::json!(null),
                    })
                    .collect(),
            )
        };
        let ok = runtime.execute(&make(MAX_EVENTS)).unwrap();
        assert_eq!(ok.events.len(), MAX_EVENTS);
        let msg = err_text(runtime.execute(&make(MAX_EVENTS + 1)));
        assert!(msg.contains("MAX_EVENTS"), "got: {msg}");
    }

    #[test]
    fn test_action_cap() {
        let runtime = WasmRuntime::new().unwrap();
        let make = |n: usize| {
            let mut ins = Vec::new();
            for _ in 0..n {
                ins.push(Instruction::Push(serde_json::json!("https://example.com/")));
                ins.push(Instruction::NavigateFromStack);
            }
            empty_binary(ins)
        };
        let ok = runtime.execute(&make(MAX_ACTIONS)).unwrap();
        assert_eq!(ok.actions.len(), MAX_ACTIONS);
        let msg = err_text(runtime.execute(&make(MAX_ACTIONS + 1)));
        assert!(msg.contains("MAX_ACTIONS"), "got: {msg}");
    }

    #[test]
    fn test_latent_caps() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![Instruction::StreamLatent {
            vector: vec![0.0; MAX_LATENT_DIMS + 1],
        }]);
        let msg = err_text(runtime.execute(&binary));
        assert!(msg.contains("MAX_LATENT_DIMS"), "got: {msg}");

        let binary = empty_binary(
            (0..=MAX_LATENT_STREAMS)
                .map(|_| Instruction::StreamLatent { vector: vec![0.0] })
                .collect(),
        );
        let msg = err_text(runtime.execute(&binary));
        assert!(msg.contains("MAX_LATENT_STREAMS"), "got: {msg}");
    }

    #[test]
    fn test_instruction_count_cap() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![Instruction::Pop; MAX_INSTRUCTIONS + 1]);
        let msg = err_text(runtime.execute(&binary));
        assert!(msg.contains("MAX_INSTRUCTIONS"), "got: {msg}");
        // The compiler refuses it too, before building any WAT.
        assert!(HlbToWatCompiler::compile(&binary).is_err());
    }

    #[test]
    fn test_string_data_cap() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![Instruction::DefineElement {
            id: 1,
            tag: "a".repeat(MAX_STRING_DATA_BYTES + 1),
        }]);
        let msg = err_text(runtime.execute(&binary));
        assert!(msg.contains("MAX_STRING_DATA_BYTES"), "got: {msg}");
    }

    #[test]
    fn test_host_work_draws_on_fuel() {
        // Guest instructions alone are far under this budget; the host
        // calls are not.
        let runtime = WasmRuntime::with_fuel(5_000).unwrap();
        let binary = empty_binary(
            (0..200)
                .map(|i| Instruction::DefineElement {
                    id: i,
                    tag: "d".to_string(),
                })
                .collect(),
        );
        let msg = err_text(runtime.execute(&binary));
        assert!(msg.contains("host budget exceeded: fuel"), "got: {msg}");
    }

    #[test]
    fn test_normal_stack_program_still_runs() {
        let runtime = WasmRuntime::new().unwrap();
        let binary = empty_binary(vec![
            Instruction::Push(serde_json::json!("a")),
            Instruction::Push(serde_json::json!("b")),
            Instruction::BinOp(spine_protocol::ProtocolBinOp::Concat),
            Instruction::DefineTextFromStack,
            Instruction::Push(serde_json::json!("https://example.com/")),
            Instruction::NavigateFromStack,
        ]);
        let result = runtime.execute(&binary).unwrap();
        assert_eq!(result.elements.len(), 1);
        assert_eq!(result.actions.len(), 1);
    }

    #[test]
    fn test_guest_memory_limit() {
        // The limiter the runtime installs refuses growth past
        // DEFAULT_MAX_MEMORY_PAGES.
        let engine = Engine::default();
        let wasm = wat::parse_str(
            r#"(module
                 (memory (export "memory") 1)
                 (func (export "grow") (param i32) (result i32)
                   (memory.grow (local.get 0))))"#,
        )
        .unwrap();
        let module = Module::new(&engine, &wasm).unwrap();
        let mut store = Store::new(
            &engine,
            StoreData {
                host: Arc::new(Mutex::new(HostState::default())),
                limits: guest_limits(),
            },
        );
        store.limiter(|data| &mut data.limits);
        let instance = Linker::<StoreData>::new(&engine)
            .instantiate(&mut store, &module)
            .unwrap();
        let grow = instance
            .get_typed_func::<i32, i32>(&mut store, "grow")
            .unwrap();
        // One page now; growing by the whole cap would pass it.
        assert_eq!(
            grow.call(&mut store, DEFAULT_MAX_MEMORY_PAGES as i32)
                .unwrap(),
            -1
        );
        // Growing to exactly the cap is allowed (returns the old size).
        assert_eq!(
            grow.call(&mut store, DEFAULT_MAX_MEMORY_PAGES as i32 - 1)
                .unwrap(),
            1
        );
    }
}
