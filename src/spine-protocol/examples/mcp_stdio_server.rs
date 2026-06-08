//! A runnable MCP server exposing a SPINE agent's capabilities over the MCP
//! **stdio transport** (newline-delimited JSON-RPC on stdin/stdout).
//!
//! This is what an MCP host (Claude Desktop, Claude Code, an MCP-capable IDE)
//! spawns and talks to. Point a host config at it and the SPINE agent's tools
//! show up with no SPINE-specific client code:
//!
//! ```jsonc
//! // claude_desktop_config.json
//! {
//!   "mcpServers": {
//!     "spine": { "command": "cargo", "args": ["run", "-p", "spine-protocol",
//!                "--example", "mcp_stdio_server"] }
//!   }
//! }
//! ```
//!
//! Try it by hand:
//! ```text
//! $ cargo run -p spine-protocol --example mcp_stdio_server
//! {"jsonrpc":"2.0","id":1,"method":"tools/list"}
//! {"jsonrpc":"2.0","id":1,"result":{"tools":[...]}}
//! {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"agent.demo/echo","arguments":{"msg":"hi"}}}
//! {"jsonrpc":"2.0","id":2,"result":{"content":[...],"structuredContent":{"echo":{"msg":"hi"}},"isError":false}}
//! ```

use serde_json::json;
use spine_protocol::mcp::{serve_stdio, McpServer};
use spine_protocol::{
    Capability, CapabilityAdvertisement, ToolCall, ToolOutcome, ToolResult,
};
use std::io::{self, BufReader};

fn demo_advertisement() -> CapabilityAdvertisement {
    CapabilityAdvertisement {
        id: "stdio".into(),
        agent_id: "did:spine:demo".into(),
        capabilities: vec![
            Capability {
                uri: "agent.demo/echo".into(),
                description: "Echo the arguments back to the caller.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "msg": { "type": "string" } },
                    "required": ["msg"]
                }),
                output_schema: json!({ "type": "object" }),
                embedding: None,
            },
            Capability {
                uri: "agent.demo/add".into(),
                description: "Add two integers.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "a": { "type": "integer" }, "b": { "type": "integer" } },
                    "required": ["a", "b"]
                }),
                output_schema: json!({ "type": "object", "properties": { "sum": { "type": "integer" } } }),
                embedding: None,
            },
        ],
    }
}

/// Demo executor: routes a bridged [`ToolCall`] to a tiny in-process tool.
fn execute(call: ToolCall) -> ToolResult {
    let outcome = match call.name.as_str() {
        "agent.demo/echo" => ToolOutcome::Ok {
            content: json!({ "echo": call.args }),
        },
        "agent.demo/add" => {
            let a = call.args.get("a").and_then(|v| v.as_i64());
            let b = call.args.get("b").and_then(|v| v.as_i64());
            match (a, b) {
                (Some(a), Some(b)) => ToolOutcome::Ok {
                    content: json!({ "sum": a + b }),
                },
                _ => ToolOutcome::Err {
                    code: "invalid_args".into(),
                    message: "expected integer fields `a` and `b`".into(),
                    details: None,
                },
            }
        }
        other => ToolOutcome::Err {
            code: "unknown_tool".into(),
            message: format!("no such tool: {other}"),
            details: None,
        },
    };
    ToolResult {
        id: call.id,
        outcome,
        trace: None,
    }
}

fn main() -> io::Result<()> {
    let mut server = McpServer::new(demo_advertisement(), execute);
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    serve_stdio(&mut server, BufReader::new(stdin.lock()), &mut stdout)
}
