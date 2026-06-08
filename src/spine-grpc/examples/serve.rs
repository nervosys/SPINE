//! A runnable SPINE gRPC `AgentService` with server reflection.
//!
//! Boots the agent on `0.0.0.0:50051` (override with `SPINE_GRPC_ADDR`). With
//! reflection enabled, `grpcurl` introspects and calls it without the `.proto`:
//!
//! ```text
//! $ cargo run -p spine-grpc --example serve
//! $ grpcurl -plaintext localhost:50051 list
//! $ grpcurl -plaintext localhost:50051 spine.agentic.v1.AgentService/ListCapabilities
//! $ grpcurl -plaintext -d '{"id":"1","name":"agent.math/add","args_json":"{\"a\":2,\"b\":40}"}' \
//!     localhost:50051 spine.agentic.v1.AgentService/CallTool
//! ```
//!
//! `StreamChat` is backed by [`EchoModel`] by default; set
//! `SPINE_GRPC_MODEL_URL=http://localhost:9091` (any OpenAI-compatible
//! `/v1/chat/completions`, including SPINE's own gateway) to stream from a real
//! model, with `SPINE_GRPC_MODEL_KEY` for the bearer token.

use std::sync::Arc;

use serde_json::json;
use spine_grpc::{
    AgentServiceServer, ChatModel, EchoModel, OpenAiChatModel, SpineAgent, FILE_DESCRIPTOR_SET,
};
use spine_protocol::{Capability, CapabilityAdvertisement, ToolCall, ToolOutcome, ToolResult};
use tonic::transport::Server;

fn advertisement() -> CapabilityAdvertisement {
    CapabilityAdvertisement {
        id: "grpc".into(),
        agent_id: "did:spine:grpc-demo".into(),
        capabilities: vec![Capability {
            uri: "agent.math/add".into(),
            description: "Add two integers.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}},
                "required": ["a", "b"]
            }),
            output_schema: json!({"type": "object", "properties": {"sum": {"type": "integer"}}}),
            embedding: None,
        }],
    }
}

fn execute(call: ToolCall) -> ToolResult {
    let outcome = if call.name == "agent.math/add" {
        let a = call.args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
        let b = call.args.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
        ToolOutcome::Ok {
            content: json!({"sum": a + b}),
        }
    } else {
        ToolOutcome::Err {
            code: "unknown_tool".into(),
            message: format!("no such tool: {}", call.name),
            details: None,
        }
    };
    ToolResult {
        id: call.id,
        outcome,
        trace: None,
    }
}

/// EchoModel by default; a real OpenAI-compatible backend if `SPINE_GRPC_MODEL_URL` is set.
fn chat_model() -> Arc<dyn ChatModel> {
    match std::env::var("SPINE_GRPC_MODEL_URL") {
        Ok(url) if !url.is_empty() => {
            let mut m = OpenAiChatModel::new(url);
            if let Ok(key) = std::env::var("SPINE_GRPC_MODEL_KEY") {
                m = m.with_api_key(key);
            }
            println!("StreamChat backend: OpenAI-compatible model");
            Arc::new(m)
        }
        _ => {
            println!("StreamChat backend: EchoModel (set SPINE_GRPC_MODEL_URL for a real model)");
            Arc::new(EchoModel)
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::var("SPINE_GRPC_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50051".into())
        .parse()?;

    let agent = SpineAgent::new(advertisement(), execute).with_model(chat_model());

    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()?;

    println!("SPINE gRPC AgentService listening on {addr} (reflection enabled)");
    Server::builder()
        .add_service(AgentServiceServer::new(agent))
        .add_service(reflection)
        .serve(addr)
        .await?;
    Ok(())
}
