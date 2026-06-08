//! Model Context Protocol (MCP) bridge.
//!
//! MCP is the de-facto tool-server contract for agent runtimes (Claude Desktop,
//! Claude Code, and a growing set of IDE hosts speak it). This module maps
//! SPINE's native agentic primitives onto the MCP JSON-RPC 2.0 surface so an
//! existing MCP host can drive a SPINE agent *without any SPINE-specific code*:
//!
//! | MCP                        | SPINE                                   |
//! |----------------------------|-----------------------------------------|
//! | `tools/list` result        | [`CapabilityAdvertisement`]             |
//! | one `Tool`                 | one [`Capability`]                      |
//! | `tools/call` params        | [`ToolCall`]                            |
//! | `tools/call` result        | [`ToolResult`] / [`ToolOutcome`]        |
//!
//! The bridge is pure data mapping plus a transport-agnostic JSON-RPC dispatcher
//! ([`McpServer`]); wiring it to stdio or HTTP/SSE is the host's choice. The
//! reverse direction ([`tool_call_to_request`], [`result_from_mcp`]) lets a
//! SPINE agent act as an MCP *client* against a foreign MCP server.
//!
//! Interop note: this is one of SPINE's bridges to the existing ecosystem,
//! alongside the gateway's OpenAI-compatible `/v1/*` routes. Speaking MCP means
//! every MCP-capable host is a SPINE client for free.

use crate::{
    Capability, CapabilityAdvertisement, ToolCall, ToolOutcome, ToolResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// JSON-RPC 2.0 protocol version literal.
pub const JSONRPC_VERSION: &str = "2.0";

/// MCP protocol revision this bridge targets.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

// ---------------------------------------------------------------------------
// MCP wire types (camelCase per the MCP schema)
// ---------------------------------------------------------------------------

/// An MCP tool descriptor (the `tools/list` element).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpTool {
    /// Tool name — maps to [`Capability::uri`].
    pub name: String,
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// JSON Schema for the call arguments — maps to [`Capability::input_schema`].
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    /// Optional JSON Schema for the result — maps to
    /// [`Capability::output_schema`]. MCP added `outputSchema` in 2025-06-18.
    #[serde(rename = "outputSchema", default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
}

/// Result of `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListToolsResult {
    /// Advertised tools.
    pub tools: Vec<McpTool>,
}

/// Params for a `tools/call` request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallToolParams {
    /// Tool name to invoke (a [`Capability::uri`]).
    pub name: String,
    /// Structured arguments object.
    #[serde(default)]
    pub arguments: Value,
}

/// One content block in a `tools/call` result. SPINE emits `text`; the variant
/// is open so hosts that send images/resources back round-trip cleanly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpContent {
    /// A UTF-8 text block.
    Text {
        /// The text payload.
        text: String,
    },
}

impl McpContent {
    /// Convenience constructor for a text block.
    pub fn text(s: impl Into<String>) -> Self {
        McpContent::Text { text: s.into() }
    }
}

/// Result of `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallToolResult {
    /// Result content blocks.
    pub content: Vec<McpContent>,
    /// Optional machine-readable result (MCP 2025-06-18 `structuredContent`).
    #[serde(
        rename = "structuredContent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub structured_content: Option<Value>,
    /// True when the tool itself failed (distinct from a JSON-RPC error, which
    /// is reserved for protocol-level failures).
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

// ---------------------------------------------------------------------------
// SPINE -> MCP
// ---------------------------------------------------------------------------

/// Map one SPINE [`Capability`] to an MCP [`McpTool`].
pub fn capability_to_tool(cap: &Capability) -> McpTool {
    // An empty/null output schema is omitted rather than advertised as `null`.
    let output_schema = match &cap.output_schema {
        Value::Null => None,
        other => Some(other.clone()),
    };
    McpTool {
        name: cap.uri.clone(),
        description: cap.description.clone(),
        input_schema: cap.input_schema.clone(),
        output_schema,
    }
}

/// Map a SPINE [`CapabilityAdvertisement`] to an MCP `tools/list` result.
pub fn advertisement_to_list_tools(ad: &CapabilityAdvertisement) -> ListToolsResult {
    ListToolsResult {
        tools: ad.capabilities.iter().map(capability_to_tool).collect(),
    }
}

/// Map a SPINE [`ToolResult`] to an MCP `tools/call` result.
///
/// A success carries the JSON value both as a `text` block (rendered JSON) and
/// as `structuredContent`; an error sets `isError` and reports `code: message`.
pub fn tool_result_to_call_result(result: &ToolResult) -> CallToolResult {
    match &result.outcome {
        ToolOutcome::Ok { content } => {
            let text = match content {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            CallToolResult {
                content: vec![McpContent::text(text)],
                structured_content: Some(content.clone()),
                is_error: false,
            }
        }
        ToolOutcome::Err {
            code,
            message,
            details,
        } => CallToolResult {
            content: vec![McpContent::text(format!("{code}: {message}"))],
            structured_content: details.clone(),
            is_error: true,
        },
    }
}

// ---------------------------------------------------------------------------
// MCP -> SPINE
// ---------------------------------------------------------------------------

/// Build a SPINE [`ToolCall`] from MCP `tools/call` params and a correlation id.
pub fn call_params_to_tool_call(params: &CallToolParams, id: impl Into<String>) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: params.name.clone(),
        args: params.arguments.clone(),
        trace: None,
    }
}

/// Build MCP `tools/call` params from a SPINE [`ToolCall`] (SPINE-as-MCP-client).
pub fn tool_call_to_request(call: &ToolCall) -> CallToolParams {
    CallToolParams {
        name: call.name.clone(),
        arguments: call.args.clone(),
    }
}

/// Interpret an MCP `tools/call` result as a SPINE [`ToolOutcome`]
/// (SPINE-as-MCP-client). Prefers `structuredContent`, falling back to joining
/// the text blocks.
pub fn result_from_mcp(result: &CallToolResult) -> ToolOutcome {
    let value = result.structured_content.clone().unwrap_or_else(|| {
        let text = result
            .content
            .iter()
            .map(|c| match c {
                McpContent::Text { text } => text.as_str(),
            })
            .collect::<Vec<_>>()
            .join("");
        Value::String(text)
    });
    if result.is_error {
        ToolOutcome::Err {
            code: "tool_error".into(),
            message: match &value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            },
            details: result.structured_content.clone(),
        }
    } else {
        ToolOutcome::Ok { content: value }
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 envelope + dispatcher
// ---------------------------------------------------------------------------

/// A JSON-RPC 2.0 request as received from an MCP host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Request id (string or number); absent for notifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// Method name (e.g. `"tools/list"`, `"tools/call"`).
    pub method: String,
    /// Method params.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    /// Numeric error code (JSON-RPC reserved range or MCP-specific).
    pub code: i64,
    /// Short human-readable message.
    pub message: String,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Echoes the request id.
    pub id: Value,
    /// Present on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Present on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Build a success response.
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Build an error response.
    pub fn err(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// JSON-RPC reserved code: method not found.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC reserved code: invalid params.
pub const INVALID_PARAMS: i64 = -32602;

/// A transport-agnostic MCP server view over a SPINE agent.
///
/// Hold a [`CapabilityAdvertisement`] (what this agent can do) and a tool
/// executor; feed it decoded JSON-RPC requests and it answers `initialize`,
/// `tools/list`, and `tools/call` per the MCP contract. The executor receives
/// the bridged [`ToolCall`] and returns a [`ToolResult`].
pub struct McpServer<F>
where
    F: FnMut(ToolCall) -> ToolResult,
{
    advertisement: CapabilityAdvertisement,
    executor: F,
    /// Server name reported in `initialize`.
    pub server_name: String,
    /// Server version reported in `initialize`.
    pub server_version: String,
}

impl<F> McpServer<F>
where
    F: FnMut(ToolCall) -> ToolResult,
{
    /// Build a server from an advertisement and a tool executor.
    pub fn new(advertisement: CapabilityAdvertisement, executor: F) -> Self {
        Self {
            advertisement,
            executor,
            server_name: "spine-agent".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    /// The `initialize` result describing this server to a host.
    fn initialize_result(&self) -> Value {
        json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": {
                "name": self.server_name,
                "version": self.server_version,
            },
        })
    }

    /// Handle one decoded JSON-RPC request, returning the response. Returns
    /// `None` for notifications (requests without an `id`), which MUST NOT be
    /// answered per JSON-RPC.
    pub fn handle(&mut self, req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = req.id.clone()?;
        let resp = match req.method.as_str() {
            "initialize" => JsonRpcResponse::ok(id, self.initialize_result()),
            "tools/list" => {
                let result = advertisement_to_list_tools(&self.advertisement);
                match serde_json::to_value(result) {
                    Ok(v) => JsonRpcResponse::ok(id, v),
                    Err(e) => JsonRpcResponse::err(id, INVALID_PARAMS, e.to_string()),
                }
            }
            "tools/call" => match serde_json::from_value::<CallToolParams>(req.params.clone()) {
                Ok(params) => {
                    let call = call_params_to_tool_call(&params, gen_call_id(&params));
                    let result = (self.executor)(call);
                    let mcp = tool_result_to_call_result(&result);
                    match serde_json::to_value(mcp) {
                        Ok(v) => JsonRpcResponse::ok(id, v),
                        Err(e) => JsonRpcResponse::err(id, INVALID_PARAMS, e.to_string()),
                    }
                }
                Err(e) => JsonRpcResponse::err(id, INVALID_PARAMS, e.to_string()),
            },
            other => {
                JsonRpcResponse::err(id, METHOD_NOT_FOUND, format!("unknown method: {other}"))
            }
        };
        Some(resp)
    }

    /// Convenience: decode a raw JSON-RPC request, handle it, and encode the
    /// response. Returns `None` for notifications. A malformed request yields a
    /// JSON-RPC parse-error response with a null id.
    pub fn handle_json(&mut self, raw: &[u8]) -> Option<Vec<u8>> {
        let req: JsonRpcRequest = match serde_json::from_slice(raw) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::err(Value::Null, -32700, e.to_string());
                return serde_json::to_vec(&resp).ok();
            }
        };
        let resp = self.handle(&req)?;
        serde_json::to_vec(&resp).ok()
    }
}

/// Derive a stable-ish call id from params without a clock/RNG (those are
/// unavailable here); the tool name plus arg fingerprint is enough to correlate.
fn gen_call_id(params: &CallToolParams) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    params.name.hash(&mut h);
    params.arguments.to_string().hash(&mut h);
    format!("mcp-{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ad() -> CapabilityAdvertisement {
        CapabilityAdvertisement {
            id: "q1".into(),
            agent_id: "did:spine:test".into(),
            capabilities: vec![Capability {
                uri: "agent.web/fetch_url".into(),
                description: "Fetch a URL.".into(),
                input_schema: json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}),
                output_schema: json!({"type": "object"}),
                embedding: None,
            }],
        }
    }

    #[test]
    fn advertisement_maps_to_tools_list() {
        let list = advertisement_to_list_tools(&sample_ad());
        assert_eq!(list.tools.len(), 1);
        let t = &list.tools[0];
        assert_eq!(t.name, "agent.web/fetch_url");
        assert_eq!(t.description, "Fetch a URL.");
        assert!(t.input_schema.get("properties").is_some());
        // camelCase keys land on the wire.
        let v = serde_json::to_value(t).unwrap();
        assert!(v.get("inputSchema").is_some());
    }

    #[test]
    fn call_params_become_tool_call() {
        let params = CallToolParams {
            name: "agent.web/fetch_url".into(),
            arguments: json!({"url": "https://example.com"}),
        };
        let call = call_params_to_tool_call(&params, "abc");
        assert_eq!(call.id, "abc");
        assert_eq!(call.name, "agent.web/fetch_url");
        assert_eq!(call.args, json!({"url": "https://example.com"}));
    }

    #[test]
    fn ok_result_maps_to_content_and_structured() {
        let result = ToolResult {
            id: "abc".into(),
            outcome: ToolOutcome::Ok {
                content: json!({"status": 200}),
            },
            trace: None,
        };
        let mcp = tool_result_to_call_result(&result);
        assert!(!mcp.is_error);
        assert_eq!(mcp.structured_content, Some(json!({"status": 200})));
        assert_eq!(mcp.content, vec![McpContent::text("{\"status\":200}")]);
    }

    #[test]
    fn err_result_sets_is_error() {
        let result = ToolResult {
            id: "abc".into(),
            outcome: ToolOutcome::Err {
                code: "timeout".into(),
                message: "took too long".into(),
                details: None,
            },
            trace: None,
        };
        let mcp = tool_result_to_call_result(&result);
        assert!(mcp.is_error);
        assert_eq!(mcp.content, vec![McpContent::text("timeout: took too long")]);
    }

    #[test]
    fn client_roundtrip_ok() {
        // SPINE-as-client: build a request, get an MCP result back, interpret it.
        let call = ToolCall {
            id: "x".into(),
            name: "t".into(),
            args: json!({"a": 1}),
            trace: None,
        };
        let req = tool_call_to_request(&call);
        assert_eq!(req.name, "t");
        let mcp_result = CallToolResult {
            content: vec![McpContent::text("ignored")],
            structured_content: Some(json!({"ok": true})),
            is_error: false,
        };
        match result_from_mcp(&mcp_result) {
            ToolOutcome::Ok { content } => assert_eq!(content, json!({"ok": true})),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn server_dispatches_tools_list_and_call() {
        let mut server = McpServer::new(sample_ad(), |call: ToolCall| ToolResult {
            id: call.id,
            outcome: ToolOutcome::Ok {
                content: json!({"echoed": call.args}),
            },
            trace: None,
        });

        // tools/list
        let req = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.into(),
            id: Some(json!(1)),
            method: "tools/list".into(),
            params: Value::Null,
        };
        let resp = server.handle(&req).unwrap();
        assert!(resp.error.is_none());
        let tools = resp.result.unwrap();
        assert_eq!(tools["tools"][0]["name"], "agent.web/fetch_url");

        // tools/call
        let req = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.into(),
            id: Some(json!(2)),
            method: "tools/call".into(),
            params: json!({"name": "agent.web/fetch_url", "arguments": {"url": "u"}}),
        };
        let resp = server.handle(&req).unwrap();
        assert!(resp.error.is_none(), "{:?}", resp.error);
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], json!(false));
        assert_eq!(result["structuredContent"], json!({"echoed": {"url": "u"}}));
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let mut server = McpServer::new(sample_ad(), |c: ToolCall| ToolResult {
            id: c.id,
            outcome: ToolOutcome::Ok { content: Value::Null },
            trace: None,
        });
        let req = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.into(),
            id: Some(json!(9)),
            method: "resources/list".into(),
            params: Value::Null,
        };
        let resp = server.handle(&req).unwrap();
        assert_eq!(resp.error.unwrap().code, METHOD_NOT_FOUND);
    }

    #[test]
    fn notification_without_id_is_not_answered() {
        let mut server = McpServer::new(sample_ad(), |c: ToolCall| ToolResult {
            id: c.id,
            outcome: ToolOutcome::Ok { content: Value::Null },
            trace: None,
        });
        let note = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.into(),
            id: None,
            method: "notifications/initialized".into(),
            params: Value::Null,
        };
        assert!(server.handle(&note).is_none());
    }
}
