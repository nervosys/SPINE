//! gRPC / protobuf bridge for SPINE's agentic surface.
//!
//! Exposes a SPINE agent as a tonic [`AgentService`] so the entire
//! protobuf/gRPC ecosystem can call it: every language's generated client
//! (tonic, grpc-go, grpc-java), `grpc-web` for browsers, and reflection-driven
//! tooling like `grpcurl`. This is SPINE's third ecosystem bridge, alongside
//! the MCP server (`spine_protocol::mcp`) and the OpenAI-compatible gateway —
//! each reaches a different installed base.
//!
//! The mapping mirrors the native types:
//!
//! | gRPC                          | SPINE                              |
//! |-------------------------------|------------------------------------|
//! | `ListCapabilities`            | [`CapabilityAdvertisement`]        |
//! | `Capability`                  | [`Capability`]                     |
//! | `CallTool`                    | [`ToolCall`] / [`ToolResult`]      |
//! | `StreamChat` (server stream)  | StreamStart / StreamToken / StreamEnd |
//!
//! JSON-shaped fields (tool args, schemas, results) travel as serialized JSON
//! strings — lossless for a bridge and far simpler than modelling arbitrary
//! JSON in protobuf.
//!
//! ```no_run
//! # // tonic mandates `Result<T, tonic::Status>` on every service method and
//! # // stream item; `Status` is large, so `result_large_err` is unavoidable here.
//! use spine_grpc::{SpineAgent, agent_service_server::AgentServiceServer};
//! use spine_protocol::{CapabilityAdvertisement, ToolResult, ToolOutcome};
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let ad = CapabilityAdvertisement { id: "g".into(), agent_id: "did:spine".into(), capabilities: vec![] };
//! let svc = SpineAgent::new(ad, |call| ToolResult {
//!     id: call.id, outcome: ToolOutcome::Ok { content: call.args }, trace: None,
//! });
//! tonic::transport::Server::builder()
//!     .add_service(AgentServiceServer::new(svc))
//!     .serve("0.0.0.0:50051".parse()?)
//!     .await?;
//! # Ok(()) }
//! ```
// tonic mandates `Result<T, tonic::Status>` on every service method and stream
// item. `Status` is a large type, so `result_large_err` fires across the whole
// service surface and there's nothing to box — the error type isn't ours.
#![allow(clippy::result_large_err)]

use std::pin::Pin;
use std::sync::Arc;

use tonic::{Request, Response, Status};

use spine_protocol::{
    Capability, CapabilityAdvertisement, ToolCall, ToolOutcome, ToolResult,
};

/// Generated protobuf types + tonic stubs for `spine.agentic.v1`.
pub mod pb {
    tonic::include_proto!("spine.agentic.v1");
}

pub use pb::agent_service_server::{self, AgentService, AgentServiceServer};
pub use pb::agent_service_client::AgentServiceClient;
pub use pb::{
    Capability as PbCapability, CallToolRequest, CallToolResponse, ListCapabilitiesRequest,
    ListCapabilitiesResponse, StreamChatRequest, StreamChunk,
};

// ---------------------------------------------------------------------------
// SPINE <-> protobuf type mapping (pure, testable)
// ---------------------------------------------------------------------------

/// Serialize a JSON value to a compact string (`null` → empty string).
fn json_to_string(v: &serde_json::Value) -> String {
    if v.is_null() {
        String::new()
    } else {
        v.to_string()
    }
}

/// Parse a JSON string, treating empty/invalid input as JSON `null`.
fn string_to_json(s: &str) -> serde_json::Value {
    if s.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
    }
}

/// Map a SPINE [`Capability`] to its protobuf form.
pub fn capability_to_proto(cap: &Capability) -> PbCapability {
    PbCapability {
        uri: cap.uri.clone(),
        description: cap.description.clone(),
        input_schema_json: json_to_string(&cap.input_schema),
        output_schema_json: json_to_string(&cap.output_schema),
    }
}

/// Map a SPINE [`CapabilityAdvertisement`] to a `ListCapabilities` response,
/// applying the request `selector` (`all` / `exact:<uri>` / `prefix:<uri>`).
pub fn advertisement_to_proto(
    ad: &CapabilityAdvertisement,
    selector: &str,
) -> ListCapabilitiesResponse {
    let caps = ad
        .capabilities
        .iter()
        .filter(|c| selector_matches(selector, &c.uri))
        .map(capability_to_proto)
        .collect();
    ListCapabilitiesResponse {
        agent_id: ad.agent_id.clone(),
        capabilities: caps,
    }
}

/// True when `uri` satisfies the gRPC selector string.
fn selector_matches(selector: &str, uri: &str) -> bool {
    match selector {
        "" | "all" => true,
        s if s.starts_with("exact:") => uri == &s["exact:".len()..],
        s if s.starts_with("prefix:") => uri.starts_with(&s["prefix:".len()..]),
        // Unknown selector: fail open to "all" rather than silently hide tools.
        _ => true,
    }
}

/// Build a SPINE [`ToolCall`] from a protobuf `CallTool` request.
pub fn request_to_tool_call(req: &CallToolRequest) -> ToolCall {
    ToolCall {
        id: req.id.clone(),
        name: req.name.clone(),
        args: string_to_json(&req.args_json),
        trace: None,
    }
}

/// Map a SPINE [`ToolResult`] to a protobuf `CallTool` response.
pub fn tool_result_to_proto(result: &ToolResult) -> CallToolResponse {
    match &result.outcome {
        ToolOutcome::Ok { content } => CallToolResponse {
            id: result.id.clone(),
            ok: true,
            content_json: json_to_string(content),
            error_code: String::new(),
            error_message: String::new(),
        },
        ToolOutcome::Err {
            code,
            message,
            details,
        } => CallToolResponse {
            id: result.id.clone(),
            ok: false,
            content_json: details.as_ref().map(json_to_string).unwrap_or_default(),
            error_code: code.clone(),
            error_message: message.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// tonic service
// ---------------------------------------------------------------------------

/// Tool executor: receives a bridged [`ToolCall`], returns a [`ToolResult`].
/// Shared across concurrent RPCs, so it must be `Send + Sync`.
pub type ToolExecutor = Arc<dyn Fn(ToolCall) -> ToolResult + Send + Sync>;

/// A tonic [`AgentService`] backed by a SPINE [`CapabilityAdvertisement`] and a
/// tool executor.
pub struct SpineAgent {
    advertisement: CapabilityAdvertisement,
    executor: ToolExecutor,
}

impl SpineAgent {
    /// Build a service from an advertisement and a tool executor closure.
    pub fn new<F>(advertisement: CapabilityAdvertisement, executor: F) -> Self
    where
        F: Fn(ToolCall) -> ToolResult + Send + Sync + 'static,
    {
        Self {
            advertisement,
            executor: Arc::new(executor),
        }
    }

    /// Build a service from an advertisement and a pre-boxed executor.
    pub fn with_executor(advertisement: CapabilityAdvertisement, executor: ToolExecutor) -> Self {
        Self {
            advertisement,
            executor,
        }
    }
}

type StreamChunkStream =
    Pin<Box<dyn futures_core::Stream<Item = Result<StreamChunk, Status>> + Send>>;

#[tonic::async_trait]
impl AgentService for SpineAgent {
    async fn list_capabilities(
        &self,
        request: Request<ListCapabilitiesRequest>,
    ) -> Result<Response<ListCapabilitiesResponse>, Status> {
        let selector = request.into_inner().selector;
        Ok(Response::new(advertisement_to_proto(
            &self.advertisement,
            &selector,
        )))
    }

    async fn call_tool(
        &self,
        request: Request<CallToolRequest>,
    ) -> Result<Response<CallToolResponse>, Status> {
        let req = request.into_inner();
        let call = request_to_tool_call(&req);
        let result = (self.executor)(call);
        Ok(Response::new(tool_result_to_proto(&result)))
    }

    type StreamChatStream = StreamChunkStream;

    async fn stream_chat(
        &self,
        request: Request<StreamChatRequest>,
    ) -> Result<Response<Self::StreamChatStream>, Status> {
        let req = request.into_inner();
        // Demo generator mirroring the gateway's agentic SSE path: stream the
        // prompt back word by word, then a terminal `done` chunk. A production
        // deployment swaps this for a model-backed StreamStart/Token/End source.
        let id = format!("grpc-{}", req.prompt.len());
        let mut chunks: Vec<Result<StreamChunk, Status>> = req
            .prompt
            .split_whitespace()
            .enumerate()
            .map(|(i, w)| {
                Ok(StreamChunk {
                    id: id.clone(),
                    seq: i as u64,
                    text: if i == 0 { w.to_string() } else { format!(" {w}") },
                    done: false,
                    finish_reason: String::new(),
                })
            })
            .collect();
        chunks.push(Ok(StreamChunk {
            id,
            seq: chunks.len() as u64,
            text: String::new(),
            done: true,
            finish_reason: "stop".into(),
        }));
        let stream = tokio_stream::iter(chunks);
        Ok(Response::new(Box::pin(stream)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio_stream::StreamExt;

    fn sample_ad() -> CapabilityAdvertisement {
        CapabilityAdvertisement {
            id: "g1".into(),
            agent_id: "did:spine:test".into(),
            capabilities: vec![
                Capability {
                    uri: "agent.web/fetch_url".into(),
                    description: "Fetch a URL.".into(),
                    input_schema: json!({"type": "object"}),
                    output_schema: json!({"type": "object"}),
                    embedding: None,
                },
                Capability {
                    uri: "agent.fs/read".into(),
                    description: "Read a file.".into(),
                    input_schema: json!({"type": "object"}),
                    output_schema: serde_json::Value::Null,
                    embedding: None,
                },
            ],
        }
    }

    fn echo_agent() -> SpineAgent {
        SpineAgent::new(sample_ad(), |call: ToolCall| ToolResult {
            id: call.id,
            outcome: ToolOutcome::Ok {
                content: json!({"echoed": call.args}),
            },
            trace: None,
        })
    }

    #[test]
    fn capability_maps_to_proto() {
        let pb = capability_to_proto(&sample_ad().capabilities[0]);
        assert_eq!(pb.uri, "agent.web/fetch_url");
        assert!(pb.input_schema_json.contains("object"));
        // Null output schema becomes an empty string, not "null".
        let pb2 = capability_to_proto(&sample_ad().capabilities[1]);
        assert_eq!(pb2.output_schema_json, "");
    }

    #[test]
    fn selector_filters() {
        let all = advertisement_to_proto(&sample_ad(), "all");
        assert_eq!(all.capabilities.len(), 2);
        assert_eq!(all.agent_id, "did:spine:test");

        let exact = advertisement_to_proto(&sample_ad(), "exact:agent.fs/read");
        assert_eq!(exact.capabilities.len(), 1);
        assert_eq!(exact.capabilities[0].uri, "agent.fs/read");

        let prefix = advertisement_to_proto(&sample_ad(), "prefix:agent.web/");
        assert_eq!(prefix.capabilities.len(), 1);
        assert_eq!(prefix.capabilities[0].uri, "agent.web/fetch_url");
    }

    #[test]
    fn tool_call_roundtrips_through_proto() {
        let req = CallToolRequest {
            id: "c1".into(),
            name: "agent.web/fetch_url".into(),
            args_json: json!({"url": "https://x"}).to_string(),
        };
        let call = request_to_tool_call(&req);
        assert_eq!(call.id, "c1");
        assert_eq!(call.args, json!({"url": "https://x"}));
    }

    #[test]
    fn ok_and_err_results_map_to_proto() {
        let ok = tool_result_to_proto(&ToolResult {
            id: "c1".into(),
            outcome: ToolOutcome::Ok {
                content: json!({"status": 200}),
            },
            trace: None,
        });
        assert!(ok.ok);
        assert_eq!(ok.content_json, "{\"status\":200}");

        let err = tool_result_to_proto(&ToolResult {
            id: "c2".into(),
            outcome: ToolOutcome::Err {
                code: "timeout".into(),
                message: "slow".into(),
                details: None,
            },
            trace: None,
        });
        assert!(!err.ok);
        assert_eq!(err.error_code, "timeout");
        assert_eq!(err.error_message, "slow");
    }

    #[tokio::test]
    async fn service_list_and_call() {
        let agent = echo_agent();

        let list = agent
            .list_capabilities(Request::new(ListCapabilitiesRequest {
                selector: "all".into(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(list.capabilities.len(), 2);

        let resp = agent
            .call_tool(Request::new(CallToolRequest {
                id: "c1".into(),
                name: "agent.web/fetch_url".into(),
                args_json: json!({"url": "u"}).to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.ok);
        assert_eq!(resp.content_json, "{\"echoed\":{\"url\":\"u\"}}");
    }

    #[tokio::test]
    async fn stream_chat_emits_chunks_then_done() {
        let agent = echo_agent();
        let resp = agent
            .stream_chat(Request::new(StreamChatRequest {
                model: "demo".into(),
                prompt: "alpha beta gamma".into(),
            }))
            .await
            .unwrap()
            .into_inner();
        let chunks: Vec<StreamChunk> = resp.map(|r| r.unwrap()).collect().await;
        // 3 words + 1 terminal chunk.
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].text, "alpha");
        assert_eq!(chunks[1].text, " beta");
        assert!(!chunks[0].done);
        assert!(chunks[3].done);
        assert_eq!(chunks[3].finish_reason, "stop");
    }
}
