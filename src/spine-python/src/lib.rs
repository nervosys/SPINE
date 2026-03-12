//! # SPINE Python Bindings
//!
//! Python bindings for the SPINE agentic web stack using PyO3.
//!
//! ## Usage
//!
//! ```python
//! import spine
//!
//! # Connect to a SPINE server
//! client = spine.connect("127.0.0.1:8080")
//!
//! # Navigate and extract content
//! client.navigate("https://example.com")
//! ur = client.get_ur()
//! print(ur.title)
//!
//! # Execute HLS scripts
//! result = client.execute_hls("let x = 42 * 2; x")
//!
//! # Parse HTML offline
//! ur = spine.parse_html("<html>...</html>")
//! ```

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use tokio::net::TcpStream;

use spine_agent::AgentClient;
use spine_compiler::Compiler;
use spine_parser::parse_html;

// Global tokio runtime for async operations
fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime")
    })
}

// ---------------------------------------------------------------------------
// UnifiedRepresentation wrapper
// ---------------------------------------------------------------------------

/// A structured representation of a web page.
#[pyclass(name = "UnifiedRepresentation")]
#[derive(Clone)]
struct PyUnifiedRepresentation {
    #[pyo3(get)]
    title: String,
    #[pyo3(get)]
    element_count: usize,
    #[pyo3(get)]
    metadata: std::collections::HashMap<String, String>,
    raw_json: String,
}

#[pymethods]
impl PyUnifiedRepresentation {
    fn __repr__(&self) -> String {
        format!(
            "UnifiedRepresentation(title='{}', elements={})",
            self.title, self.element_count
        )
    }

    /// Get the full UR as a JSON string.
    fn to_json(&self) -> &str {
        &self.raw_json
    }
}

// ---------------------------------------------------------------------------
// SpineBinary wrapper
// ---------------------------------------------------------------------------

/// A compiled HLS binary.
#[pyclass(name = "SpineBinary")]
struct PySpineBinary {
    #[pyo3(get)]
    instruction_count: usize,
    #[pyo3(get)]
    data_bytes: usize,
    #[pyo3(get)]
    exported_functions: Vec<String>,
    #[pyo3(get)]
    capabilities: Vec<String>,
}

#[pymethods]
impl PySpineBinary {
    fn __repr__(&self) -> String {
        format!(
            "SpineBinary(instructions={}, exports={:?})",
            self.instruction_count, self.exported_functions
        )
    }
}

// ---------------------------------------------------------------------------
// AgentClient wrapper
// ---------------------------------------------------------------------------

/// A connected SPINE agent client.
#[pyclass(name = "Client")]
struct PyClient {
    client: std::sync::Mutex<AgentClient<TcpStream>>,
}

#[pymethods]
impl PyClient {
    /// Navigate to a URL.
    fn navigate(&self, url: &str) -> PyResult<()> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        runtime()
            .block_on(client.navigate(url))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Get the Unified Representation of the current page.
    fn get_ur(&self) -> PyResult<PyUnifiedRepresentation> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let ur = runtime()
            .block_on(client.get_ur())
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let raw_json = serde_json::to_string(&ur).unwrap_or_default();
        Ok(PyUnifiedRepresentation {
            title: ur.title.clone(),
            element_count: ur.elements.len(),
            metadata: ur.metadata.clone(),
            raw_json,
        })
    }

    /// Get raw HTML of the current page.
    fn get_raw_html(&self) -> PyResult<String> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        runtime()
            .block_on(client.get_raw_html())
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Click an element by ID.
    fn click(&self, element_id: &str) -> PyResult<()> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        runtime()
            .block_on(client.click(element_id))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Type text into an element.
    fn type_text(&self, element_id: &str, text: &str) -> PyResult<()> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        runtime()
            .block_on(client.type_text(element_id, text))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Execute an HLS script and return the result as JSON string.
    fn execute_hls(&self, script: &str) -> PyResult<String> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let result = runtime()
            .block_on(client.execute_hls(script))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(serde_json::to_string(&result).unwrap_or_default())
    }

    /// Get latent vector representation of current page.
    fn get_latent_ur(&self, dimensions: usize) -> PyResult<Vec<f32>> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        runtime()
            .block_on(client.get_latent_ur(dimensions))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Search across the web.
    fn search(&self, query: &str) -> PyResult<String> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let result = runtime()
            .block_on(client.search(query))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(serde_json::to_string(&result).unwrap_or_default())
    }

    /// Ping the server and return round-trip time in milliseconds.
    fn ping(&self) -> PyResult<u64> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        runtime()
            .block_on(client.ping())
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Trigger protocol morphing.
    fn morph(&self) -> PyResult<()> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        runtime()
            .block_on(client.morph())
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Get server capabilities.
    fn get_capabilities(&self) -> PyResult<Vec<String>> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        runtime()
            .block_on(client.get_capabilities())
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Store a knowledge entry.
    fn store_knowledge(&self, key: &str, value: &str, tags: Vec<String>) -> PyResult<()> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val: serde_json::Value = serde_json::from_str(value)
            .map_err(|e| PyValueError::new_err(format!("invalid JSON: {e}")))?;
        runtime()
            .block_on(client.store_knowledge(key, val, tags))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Query knowledge entries.
    fn query_knowledge(&self, query: &str, tags: Vec<String>, limit: usize) -> PyResult<String> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let results = runtime()
            .block_on(client.query_knowledge(query, tags, limit))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(serde_json::to_string(&results).unwrap_or_default())
    }

    fn __repr__(&self) -> &str {
        "Client(connected)"
    }
}

// ---------------------------------------------------------------------------
// Module-level functions
// ---------------------------------------------------------------------------

/// Connect to a SPINE server over TCP.
#[pyfunction]
fn connect(addr: &str) -> PyResult<PyClient> {
    let client = runtime()
        .block_on(AgentClient::connect(addr))
        .map_err(|e| PyRuntimeError::new_err(format!("connect failed: {e}")))?;
    Ok(PyClient {
        client: std::sync::Mutex::new(client),
    })
}

/// Parse HTML into a UnifiedRepresentation (offline, no server needed).
#[pyfunction]
#[pyo3(name = "parse_html")]
fn py_parse_html(html: &str) -> PyResult<PyUnifiedRepresentation> {
    let ur = parse_html(html).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let raw_json = serde_json::to_string(&ur).unwrap_or_default();
    Ok(PyUnifiedRepresentation {
        title: ur.title.clone(),
        element_count: ur.elements.len(),
        metadata: ur.metadata.clone(),
        raw_json,
    })
}

/// Compile HLS source to a SpineBinary (offline, no server needed).
#[pyfunction]
fn compile_hls(source: &str) -> PyResult<PySpineBinary> {
    let binary = Compiler::compile(source)
        .map_err(|e| PyValueError::new_err(format!("compile error: {e}")))?;
    Ok(PySpineBinary {
        instruction_count: binary.instructions.len(),
        data_bytes: binary.data.len(),
        exported_functions: binary.exported_functions.keys().cloned().collect(),
        capabilities: binary.capabilities,
    })
}

// ---------------------------------------------------------------------------
// Module definition
// ---------------------------------------------------------------------------

/// SPINE: A headless semantic browser with adaptive encryption for AI agents.
#[pymodule]
fn spine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(connect, m)?)?;
    m.add_function(wrap_pyfunction!(py_parse_html, m)?)?;
    m.add_function(wrap_pyfunction!(compile_hls, m)?)?;
    m.add_class::<PyClient>()?;
    m.add_class::<PyUnifiedRepresentation>()?;
    m.add_class::<PySpineBinary>()?;
    Ok(())
}
