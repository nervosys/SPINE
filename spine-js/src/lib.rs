//! # SPINE JavaScript/TypeScript WASM Bindings
//!
//! Provides client-side WASM bindings for HTML parsing and HLS compilation.
//! These are stateless operations that run entirely in the browser.
//!
//! ## Usage (TypeScript)
//!
//! ```typescript
//! import init, { parseHtml, compileHls } from 'spine-js';
//!
//! await init();
//!
//! const ur = parseHtml('<html><body><h1>Hello</h1></body></html>');
//! console.log(ur.title);        // ""
//! console.log(ur.elementCount); // > 0
//!
//! const binary = compileHls('let x = 42 * 2; x');
//! console.log(binary.instructionCount);
//! ```

use wasm_bindgen::prelude::*;

use spine_compiler::Compiler;
use spine_parser::parse_html;

// ---------------------------------------------------------------------------
// UnifiedRepresentation wrapper
// ---------------------------------------------------------------------------

/// A structured representation of a web page.
#[wasm_bindgen]
pub struct UnifiedRepresentation {
    title: String,
    element_count: usize,
    metadata_json: String,
    raw_json: String,
}

#[wasm_bindgen]
impl UnifiedRepresentation {
    /// Page title.
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// Number of elements in the UR.
    #[wasm_bindgen(getter, js_name = "elementCount")]
    pub fn element_count(&self) -> usize {
        self.element_count
    }

    /// Metadata as a JSON string.
    #[wasm_bindgen(getter, js_name = "metadataJson")]
    pub fn metadata_json(&self) -> String {
        self.metadata_json.clone()
    }

    /// Full UR as a JSON string.
    #[wasm_bindgen(js_name = "toJson")]
    pub fn to_json(&self) -> String {
        self.raw_json.clone()
    }
}

// ---------------------------------------------------------------------------
// SpineBinary wrapper
// ---------------------------------------------------------------------------

/// A compiled HLS binary.
#[wasm_bindgen]
pub struct SpineBinary {
    instruction_count: usize,
    data_bytes: usize,
    exports_json: String,
    capabilities_json: String,
}

#[wasm_bindgen]
impl SpineBinary {
    /// Number of compiled instructions.
    #[wasm_bindgen(getter, js_name = "instructionCount")]
    pub fn instruction_count(&self) -> usize {
        self.instruction_count
    }

    /// Size of static data section in bytes.
    #[wasm_bindgen(getter, js_name = "dataBytes")]
    pub fn data_bytes(&self) -> usize {
        self.data_bytes
    }

    /// Exported function names as JSON array.
    #[wasm_bindgen(getter, js_name = "exportsJson")]
    pub fn exports_json(&self) -> String {
        self.exports_json.clone()
    }

    /// Required capabilities as JSON array.
    #[wasm_bindgen(getter, js_name = "capabilitiesJson")]
    pub fn capabilities_json(&self) -> String {
        self.capabilities_json.clone()
    }
}

// ---------------------------------------------------------------------------
// Module-level functions
// ---------------------------------------------------------------------------

/// Parse HTML into a UnifiedRepresentation.
///
/// This runs entirely client-side — no server connection needed.
#[wasm_bindgen(js_name = "parseHtml")]
pub fn parse_html_wasm(html: &str) -> Result<UnifiedRepresentation, JsError> {
    let ur = parse_html(html).map_err(|e| JsError::new(&e.to_string()))?;
    let metadata_json = serde_json::to_string(&ur.metadata).unwrap_or_default();
    let raw_json = serde_json::to_string(&ur).unwrap_or_default();
    Ok(UnifiedRepresentation {
        title: ur.title.clone(),
        element_count: ur.elements.len(),
        metadata_json,
        raw_json,
    })
}

/// Compile HLS source code to a SpineBinary.
///
/// This runs entirely client-side — no server connection needed.
#[wasm_bindgen(js_name = "compileHls")]
pub fn compile_hls_wasm(source: &str) -> Result<SpineBinary, JsError> {
    let binary = Compiler::compile(source).map_err(|e| JsError::new(&e.to_string()))?;
    let exports: Vec<&String> = binary.exported_functions.keys().collect();
    Ok(SpineBinary {
        instruction_count: binary.instructions.len(),
        data_bytes: binary.data.len(),
        exports_json: serde_json::to_string(&exports).unwrap_or_default(),
        capabilities_json: serde_json::to_string(&binary.capabilities).unwrap_or_default(),
    })
}

/// Get the SPINE version string.
#[wasm_bindgen(js_name = "version")]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
