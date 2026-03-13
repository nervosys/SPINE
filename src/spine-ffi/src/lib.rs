//! # SPINE C FFI
//!
//! C-compatible foreign function interface for the SPINE agentic web stack.
//! Used by Go (cgo), Java (JNI), and other language bindings.
//!
//! ## Safety
//!
//! All functions follow C ABI conventions. Strings are passed as null-terminated
//! `*const c_char` and returned as owned allocations that must be freed with
//! [`spine_free_string`]. Opaque handles are returned as `*mut c_void`.

#![allow(clippy::missing_safety_doc)]

use libc::{c_char, c_int, c_void};
use std::ffi::{CStr, CString};
use std::sync::OnceLock;
use tokio::net::TcpStream;

use spine_agent::AgentClient;

// ---------------------------------------------------------------------------
// Global tokio runtime (shared across all FFI calls)
// ---------------------------------------------------------------------------

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("spine-ffi: failed to create tokio runtime")
    })
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

thread_local! {
    static LAST_ERROR: std::cell::RefCell<Option<CString>> = const { std::cell::RefCell::new(None) };
}

fn set_error(msg: &str) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = CString::new(msg).ok();
    });
}

/// Get the last error message. Returns NULL if no error.
/// The returned string is valid until the next FFI call on this thread.
#[no_mangle]
pub extern "C" fn spine_last_error() -> *const c_char {
    LAST_ERROR.with(|e| {
        e.borrow()
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

fn to_str(ptr: *const c_char) -> Option<&'static str> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}

fn to_c_string(s: &str) -> *mut c_char {
    CString::new(s)
        .unwrap_or_else(|_| CString::new("").unwrap())
        .into_raw()
}

/// Free a string returned by SPINE FFI functions.
///
/// # Safety
/// `ptr` must be a pointer returned by a SPINE FFI function, or NULL.
#[no_mangle]
pub unsafe extern "C" fn spine_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

// ---------------------------------------------------------------------------
// Client lifecycle
// ---------------------------------------------------------------------------

/// Connect to a SPINE server over TCP. Returns an opaque client handle, or NULL on error.
///
/// # Safety
/// `addr` must be a valid null-terminated C string (e.g. "127.0.0.1:8080").
#[no_mangle]
pub unsafe extern "C" fn spine_connect(addr: *const c_char) -> *mut c_void {
    let addr = match to_str(addr) {
        Some(a) => a,
        None => {
            set_error("spine_connect: addr is null or invalid UTF-8");
            return std::ptr::null_mut();
        }
    };

    match runtime().block_on(AgentClient::connect(addr)) {
        Ok(client) => {
            let boxed: Box<AgentClient<TcpStream>> = Box::new(client);
            Box::into_raw(boxed) as *mut c_void
        }
        Err(e) => {
            set_error(&format!("spine_connect: {e}"));
            std::ptr::null_mut()
        }
    }
}

/// Disconnect and free a client handle.
///
/// # Safety
/// `handle` must be a valid pointer returned by `spine_connect`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn spine_disconnect(handle: *mut c_void) {
    if !handle.is_null() {
        drop(Box::from_raw(handle as *mut AgentClient<TcpStream>));
    }
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

/// Navigate to a URL. Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid client handle. `url` must be a null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn spine_navigate(handle: *mut c_void, url: *const c_char) -> c_int {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_navigate: null handle");
            return -1;
        }
    };
    let url = match to_str(url) {
        Some(u) => u,
        None => {
            set_error("spine_navigate: url is null or invalid UTF-8");
            return -1;
        }
    };
    match runtime().block_on(client.navigate(url)) {
        Ok(()) => 0,
        Err(e) => {
            set_error(&format!("spine_navigate: {e}"));
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Content retrieval
// ---------------------------------------------------------------------------

/// Get the Unified Representation of the current page as a JSON string.
/// Returns a newly allocated C string (must be freed with `spine_free_string`), or NULL on error.
///
/// # Safety
/// `handle` must be a valid client handle.
#[no_mangle]
pub unsafe extern "C" fn spine_get_ur(handle: *mut c_void) -> *mut c_char {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_get_ur: null handle");
            return std::ptr::null_mut();
        }
    };
    match runtime().block_on(client.get_ur()) {
        Ok(ur) => {
            let json = serde_json::to_string(&ur).unwrap_or_default();
            to_c_string(&json)
        }
        Err(e) => {
            set_error(&format!("spine_get_ur: {e}"));
            std::ptr::null_mut()
        }
    }
}

/// Get raw HTML of the current page.
/// Returns a newly allocated C string, or NULL on error.
///
/// # Safety
/// `handle` must be a valid client handle.
#[no_mangle]
pub unsafe extern "C" fn spine_get_raw_html(handle: *mut c_void) -> *mut c_char {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_get_raw_html: null handle");
            return std::ptr::null_mut();
        }
    };
    match runtime().block_on(client.get_raw_html()) {
        Ok(html) => to_c_string(&html),
        Err(e) => {
            set_error(&format!("spine_get_raw_html: {e}"));
            std::ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Search across the web. Returns JSON results string, or NULL on error.
///
/// # Safety
/// `handle` must be a valid client handle. `query` must be a null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn spine_search(handle: *mut c_void, query: *const c_char) -> *mut c_char {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_search: null handle");
            return std::ptr::null_mut();
        }
    };
    let query = match to_str(query) {
        Some(q) => q,
        None => {
            set_error("spine_search: query is null or invalid UTF-8");
            return std::ptr::null_mut();
        }
    };
    match runtime().block_on(client.search(query)) {
        Ok(result) => to_c_string(&result.to_string()),
        Err(e) => {
            set_error(&format!("spine_search: {e}"));
            std::ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// HLS execution
// ---------------------------------------------------------------------------

/// Execute an HLS script. Returns JSON result string, or NULL on error.
///
/// # Safety
/// `handle` must be a valid client handle. `script` must be a null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn spine_execute_hls(
    handle: *mut c_void,
    script: *const c_char,
) -> *mut c_char {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_execute_hls: null handle");
            return std::ptr::null_mut();
        }
    };
    let script = match to_str(script) {
        Some(s) => s,
        None => {
            set_error("spine_execute_hls: script is null or invalid UTF-8");
            return std::ptr::null_mut();
        }
    };
    match runtime().block_on(client.execute_hls(script)) {
        Ok(result) => {
            let json = serde_json::to_string(&result).unwrap_or_default();
            to_c_string(&json)
        }
        Err(e) => {
            set_error(&format!("spine_execute_hls: {e}"));
            std::ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// Ping
// ---------------------------------------------------------------------------

/// Ping the server. Returns round-trip time in milliseconds, or -1 on error.
///
/// # Safety
/// `handle` must be a valid client handle.
#[no_mangle]
pub unsafe extern "C" fn spine_ping(handle: *mut c_void) -> i64 {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_ping: null handle");
            return -1;
        }
    };
    match runtime().block_on(client.ping()) {
        Ok(rtt) => rtt as i64,
        Err(e) => {
            set_error(&format!("spine_ping: {e}"));
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Protocol morphing
// ---------------------------------------------------------------------------

/// Trigger protocol morphing. Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid client handle.
#[no_mangle]
pub unsafe extern "C" fn spine_morph(handle: *mut c_void) -> c_int {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_morph: null handle");
            return -1;
        }
    };
    match runtime().block_on(client.morph()) {
        Ok(()) => 0,
        Err(e) => {
            set_error(&format!("spine_morph: {e}"));
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// Get server capabilities. Returns JSON array string, or NULL on error.
///
/// # Safety
/// `handle` must be a valid client handle.
#[no_mangle]
pub unsafe extern "C" fn spine_get_capabilities(handle: *mut c_void) -> *mut c_char {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_get_capabilities: null handle");
            return std::ptr::null_mut();
        }
    };
    match runtime().block_on(client.get_capabilities()) {
        Ok(caps) => {
            let json = serde_json::to_string(&caps).unwrap_or_default();
            to_c_string(&json)
        }
        Err(e) => {
            set_error(&format!("spine_get_capabilities: {e}"));
            std::ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// Knowledge
// ---------------------------------------------------------------------------

/// Store a knowledge entry. Returns 0 on success, -1 on error.
///
/// # Safety
/// All pointers must be valid null-terminated C strings. `tags_json` is a JSON array string.
#[no_mangle]
pub unsafe extern "C" fn spine_store_knowledge(
    handle: *mut c_void,
    key: *const c_char,
    value_json: *const c_char,
    tags_json: *const c_char,
) -> c_int {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_store_knowledge: null handle");
            return -1;
        }
    };
    let key = match to_str(key) {
        Some(k) => k,
        None => {
            set_error("spine_store_knowledge: key is null or invalid");
            return -1;
        }
    };
    let value_str = match to_str(value_json) {
        Some(v) => v,
        None => {
            set_error("spine_store_knowledge: value_json is null or invalid");
            return -1;
        }
    };
    let tags_str = match to_str(tags_json) {
        Some(t) => t,
        None => {
            set_error("spine_store_knowledge: tags_json is null or invalid");
            return -1;
        }
    };

    let value: serde_json::Value = match serde_json::from_str(value_str) {
        Ok(v) => v,
        Err(e) => {
            set_error(&format!("spine_store_knowledge: invalid value JSON: {e}"));
            return -1;
        }
    };
    let tags: Vec<String> = match serde_json::from_str(tags_str) {
        Ok(t) => t,
        Err(e) => {
            set_error(&format!("spine_store_knowledge: invalid tags JSON: {e}"));
            return -1;
        }
    };

    match runtime().block_on(client.store_knowledge(key, value, tags)) {
        Ok(()) => 0,
        Err(e) => {
            set_error(&format!("spine_store_knowledge: {e}"));
            -1
        }
    }
}

/// Query knowledge entries. Returns JSON string, or NULL on error.
///
/// # Safety
/// All pointers must be valid. `tags_json` is a JSON array string.
#[no_mangle]
pub unsafe extern "C" fn spine_query_knowledge(
    handle: *mut c_void,
    query: *const c_char,
    tags_json: *const c_char,
    limit: usize,
) -> *mut c_char {
    let client = match (handle as *mut AgentClient<TcpStream>).as_mut() {
        Some(c) => c,
        None => {
            set_error("spine_query_knowledge: null handle");
            return std::ptr::null_mut();
        }
    };
    let query = match to_str(query) {
        Some(q) => q,
        None => {
            set_error("spine_query_knowledge: query is null or invalid");
            return std::ptr::null_mut();
        }
    };
    let tags_str = match to_str(tags_json) {
        Some(t) => t,
        None => {
            set_error("spine_query_knowledge: tags_json is null or invalid");
            return std::ptr::null_mut();
        }
    };
    let tags: Vec<String> = match serde_json::from_str(tags_str) {
        Ok(t) => t,
        Err(e) => {
            set_error(&format!("spine_query_knowledge: invalid tags JSON: {e}"));
            return std::ptr::null_mut();
        }
    };

    match runtime().block_on(client.query_knowledge(query, tags, limit)) {
        Ok(results) => {
            let json = serde_json::to_string(&results).unwrap_or_default();
            to_c_string(&json)
        }
        Err(e) => {
            set_error(&format!("spine_query_knowledge: {e}"));
            std::ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// Offline utilities (no server connection needed)
// ---------------------------------------------------------------------------

/// Parse HTML into a Unified Representation (offline). Returns JSON string, or NULL on error.
///
/// # Safety
/// `html` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn spine_parse_html(html: *const c_char) -> *mut c_char {
    let html = match to_str(html) {
        Some(h) => h,
        None => {
            set_error("spine_parse_html: html is null or invalid UTF-8");
            return std::ptr::null_mut();
        }
    };
    match spine_parser::parse_html(html) {
        Ok(ur) => {
            let json = serde_json::to_string(&ur).unwrap_or_default();
            to_c_string(&json)
        }
        Err(e) => {
            set_error(&format!("spine_parse_html: {e}"));
            std::ptr::null_mut()
        }
    }
}

/// Compile HLS source code to a SpineBinary (offline). Returns JSON string, or NULL on error.
///
/// # Safety
/// `source` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn spine_compile_hls(source: *const c_char) -> *mut c_char {
    let source = match to_str(source) {
        Some(s) => s,
        None => {
            set_error("spine_compile_hls: source is null or invalid UTF-8");
            return std::ptr::null_mut();
        }
    };
    match spine_compiler::Compiler::compile(source) {
        Ok(binary) => {
            let json = serde_json::to_string(&binary).unwrap_or_default();
            to_c_string(&json)
        }
        Err(e) => {
            set_error(&format!("spine_compile_hls: {e}"));
            std::ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

/// Get the SPINE FFI library version.
#[no_mangle]
pub extern "C" fn spine_version() -> *const c_char {
    static VERSION: OnceLock<CString> = OnceLock::new();
    VERSION
        .get_or_init(|| CString::new(env!("CARGO_PKG_VERSION")).unwrap())
        .as_ptr()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let ptr = spine_version();
        assert!(!ptr.is_null());
        let version = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(version, "1.0.0");
    }

    #[test]
    fn test_free_string_null_safe() {
        unsafe { spine_free_string(std::ptr::null_mut()) };
    }

    #[test]
    fn test_last_error_initially_null() {
        let ptr = spine_last_error();
        assert!(ptr.is_null());
    }

    #[test]
    fn test_connect_null_addr() {
        let handle = unsafe { spine_connect(std::ptr::null()) };
        assert!(handle.is_null());
        let err = spine_last_error();
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("null"));
    }

    #[test]
    fn test_navigate_null_handle() {
        let url = CString::new("https://example.com").unwrap();
        let rc = unsafe { spine_navigate(std::ptr::null_mut(), url.as_ptr()) };
        assert_eq!(rc, -1);
    }

    #[test]
    fn test_get_ur_null_handle() {
        let ptr = unsafe { spine_get_ur(std::ptr::null_mut()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_search_null_handle() {
        let query = CString::new("test").unwrap();
        let ptr = unsafe { spine_search(std::ptr::null_mut(), query.as_ptr()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_ping_null_handle() {
        let rtt = unsafe { spine_ping(std::ptr::null_mut()) };
        assert_eq!(rtt, -1);
    }

    #[test]
    fn test_morph_null_handle() {
        let rc = unsafe { spine_morph(std::ptr::null_mut()) };
        assert_eq!(rc, -1);
    }

    #[test]
    fn test_parse_html_valid() {
        let html = CString::new("<html><head><title>Test</title></head><body>Hello</body></html>")
            .unwrap();
        let ptr = unsafe { spine_parse_html(html.as_ptr()) };
        assert!(!ptr.is_null());
        let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert!(json.contains("Test"));
        unsafe { spine_free_string(ptr) };
    }

    #[test]
    fn test_parse_html_null() {
        let ptr = unsafe { spine_parse_html(std::ptr::null()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_compile_hls_valid() {
        let source = CString::new("let x = 42").unwrap();
        let ptr = unsafe { spine_compile_hls(source.as_ptr()) };
        assert!(!ptr.is_null());
        let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert!(json.contains("instructions"));
        unsafe { spine_free_string(ptr) };
    }

    #[test]
    fn test_compile_hls_null() {
        let ptr = unsafe { spine_compile_hls(std::ptr::null()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_disconnect_null_safe() {
        unsafe { spine_disconnect(std::ptr::null_mut()) };
    }

    #[test]
    fn test_execute_hls_null_handle() {
        let script = CString::new("let x = 1").unwrap();
        let ptr = unsafe { spine_execute_hls(std::ptr::null_mut(), script.as_ptr()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_get_capabilities_null_handle() {
        let ptr = unsafe { spine_get_capabilities(std::ptr::null_mut()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_store_knowledge_null_handle() {
        let key = CString::new("k").unwrap();
        let val = CString::new("{}").unwrap();
        let tags = CString::new("[]").unwrap();
        let rc = unsafe {
            spine_store_knowledge(
                std::ptr::null_mut(),
                key.as_ptr(),
                val.as_ptr(),
                tags.as_ptr(),
            )
        };
        assert_eq!(rc, -1);
    }

    #[test]
    fn test_query_knowledge_null_handle() {
        let query = CString::new("q").unwrap();
        let tags = CString::new("[]").unwrap();
        let ptr = unsafe {
            spine_query_knowledge(std::ptr::null_mut(), query.as_ptr(), tags.as_ptr(), 5)
        };
        assert!(ptr.is_null());
    }
}


#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::ffi::{CStr, CString};
    use std::thread;

    // ---- Roundtrip Tests ----

    #[test]
    fn test_parse_html_roundtrip_complex() {
        let html = CString::new(r#"<html>
            <head><title>Complex Page</title></head>
            <body>
                <h1>Header</h1>
                <div class="content">
                    <p>Paragraph 1</p>
                    <p>Paragraph 2</p>
                    <a href="https://example.com">Link</a>
                </div>
            </body></html>"#).unwrap();
        let ptr = unsafe { spine_parse_html(html.as_ptr()) };
        assert!(!ptr.is_null(), "parse_html returned null for valid HTML");
        let json_str = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        let ur: serde_json::Value = serde_json::from_str(json_str).unwrap();
        assert_eq!(ur["title"], "Complex Page");
        unsafe { spine_free_string(ptr) };
    }

    #[test]
    fn test_compile_hls_roundtrip_complex() {
        let source = CString::new(r#"
            let x = 10
            let y = 20
            state counter = 0
            fn add(a, b) { a + b }
        "#).unwrap();
        let ptr = unsafe { spine_compile_hls(source.as_ptr()) };
        assert!(!ptr.is_null(), "compile_hls returned null for valid HLS");
        let json_str = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        let binary: serde_json::Value = serde_json::from_str(json_str).unwrap();
        assert!(binary["instructions"].is_array());
        assert!(binary["instructions"].as_array().unwrap().len() > 0);
        unsafe { spine_free_string(ptr) };
    }

    // ---- Error Propagation Tests ----

    #[test]
    fn test_error_propagation_chain() {
        // Connect to invalid address should set error
        let addr = CString::new("invalid:99999").unwrap();
        let handle = unsafe { spine_connect(addr.as_ptr()) };
        assert!(handle.is_null());
        let err = spine_last_error();
        assert!(!err.is_null(), "expected error after failed connect");
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_compile_hls_syntax_error_message() {
        let source = CString::new("!!!invalid syntax{{{").unwrap();
        let ptr = unsafe { spine_compile_hls(source.as_ptr()) };
        // May return null or an error JSON depending on how the compiler handles it
        if ptr.is_null() {
            let err = spine_last_error();
            assert!(!err.is_null());
        } else {
            unsafe { spine_free_string(ptr) };
        }
    }

    // ---- Memory Safety Tests ----

    #[test]
    fn test_double_free_protection() {
        let html = CString::new("<html><body>test</body></html>").unwrap();
        let ptr = unsafe { spine_parse_html(html.as_ptr()) };
        assert!(!ptr.is_null());
        unsafe { spine_free_string(ptr) };
        // Second free of a different allocation should be safe
        // (we can't test true double-free safely, but null-free is safe)
        unsafe { spine_free_string(std::ptr::null_mut()) };
    }

    #[test]
    fn test_free_string_multiple_null() {
        for _ in 0..100 {
            unsafe { spine_free_string(std::ptr::null_mut()) };
        }
    }

    // ---- Concurrent Access Tests ----

    #[test]
    fn test_concurrent_parse_html() {
        let handles: Vec<_> = (0..8)
            .map(|i| {
                thread::spawn(move || {
                    let html = CString::new(format!(
                        "<html><head><title>Thread {i}</title></head><body>Content {i}</body></html>"
                    )).unwrap();
                    let ptr = unsafe { spine_parse_html(html.as_ptr()) };
                    assert!(!ptr.is_null(), "thread {i} got null");
                    let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
                    unsafe { spine_free_string(ptr) };
                    json
                })
            })
            .collect();

        for (i, h) in handles.into_iter().enumerate() {
            let json = h.join().unwrap();
            assert!(json.contains(&format!("Thread {i}")));
        }
    }

    #[test]
    fn test_concurrent_compile_hls() {
        let handles: Vec<_> = (0..8)
            .map(|i| {
                thread::spawn(move || {
                    let src = CString::new(format!("let x{i} = {i}")).unwrap();
                    let ptr = unsafe { spine_compile_hls(src.as_ptr()) };
                    assert!(!ptr.is_null(), "thread {i} got null");
                    let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
                    unsafe { spine_free_string(ptr) };
                    assert!(json.contains("instructions"));
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_concurrent_version() {
        let handles: Vec<_> = (0..16)
            .map(|_| {
                thread::spawn(|| {
                    let ptr = spine_version();
                    assert!(!ptr.is_null());
                    let v = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
                    assert_eq!(v, "1.0.0");
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }

    // ---- UTF-8 / Encoding Tests ----

    #[test]
    fn test_parse_html_unicode() {
        let html = CString::new("<html><head><title>日本語テスト</title></head><body>Ünîcödé</body></html>").unwrap();
        let ptr = unsafe { spine_parse_html(html.as_ptr()) };
        assert!(!ptr.is_null());
        let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert!(json.contains("日本語テスト"));
        unsafe { spine_free_string(ptr) };
    }

    #[test]
    fn test_parse_html_empty() {
        let html = CString::new("").unwrap();
        let ptr = unsafe { spine_parse_html(html.as_ptr()) };
        // Empty string may return a valid minimal UR or null
        if !ptr.is_null() {
            unsafe { spine_free_string(ptr) };
        }
    }

    #[test]
    fn test_parse_html_large() {
        let mut html = String::from("<html><body>");
        for i in 0..1000 {
            html.push_str(&format!("<p>Paragraph {i}</p>"));
        }
        html.push_str("</body></html>");
        let chtml = CString::new(html).unwrap();
        let ptr = unsafe { spine_parse_html(chtml.as_ptr()) };
        assert!(!ptr.is_null());
        let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert!(json.contains("Paragraph 999"));
        unsafe { spine_free_string(ptr) };
    }

    // ---- Version Tests ----

    #[test]
    fn test_version_is_static() {
        let ptr1 = spine_version();
        let ptr2 = spine_version();
        // Same static pointer
        assert_eq!(ptr1, ptr2);
    }

    // ---- Null Input Edge Cases ----

    #[test]
    fn test_navigate_null_url() {
        let rc = unsafe { spine_navigate(std::ptr::null_mut(), std::ptr::null()) };
        assert_eq!(rc, -1);
    }

    #[test]
    fn test_search_null_query() {
        let ptr = unsafe { spine_search(std::ptr::null_mut(), std::ptr::null()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_get_raw_html_null_handle() {
        let ptr = unsafe { spine_get_raw_html(std::ptr::null_mut()) };
        assert!(ptr.is_null());
    }
}