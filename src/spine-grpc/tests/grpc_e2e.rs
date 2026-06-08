//! End-to-end gRPC: spin up the tonic `AgentService` on a real localhost
//! socket, then drive it with the generated client over HTTP/2. Proves the
//! actual wire path — protobuf encode/decode + transport — not just the service
//! methods, so "SPINE speaks gRPC" is verified, not asserted.

use serde_json::json;
use spine_grpc::{
    AgentServiceClient, AgentServiceServer, CallToolRequest, ListCapabilitiesRequest, SpineAgent,
    StreamChatRequest,
};
use spine_protocol::{Capability, CapabilityAdvertisement, ToolCall, ToolOutcome, ToolResult};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_stream::StreamExt;
use tonic::transport::Server;

fn agent() -> SpineAgent {
    let ad = CapabilityAdvertisement {
        id: "e2e".into(),
        agent_id: "did:spine:e2e".into(),
        capabilities: vec![Capability {
            uri: "agent.math/add".into(),
            description: "Add two integers.".into(),
            input_schema: json!({"type": "object"}),
            output_schema: json!({"type": "object"}),
            embedding: None,
        }],
    };
    SpineAgent::new(ad, |call: ToolCall| {
        let a = call.args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
        let b = call.args.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
        ToolResult {
            id: call.id,
            outcome: ToolOutcome::Ok {
                content: json!({"sum": a + b}),
            },
            trace: None,
        }
    })
}

#[tokio::test]
async fn grpc_over_the_wire() {
    // Bind first so the port is listening before the client connects (no race),
    // then serve on the bound listener in the background.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(AgentServiceServer::new(agent()))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    let mut client = AgentServiceClient::connect(format!("http://{addr}"))
        .await
        .expect("client connects to the SPINE gRPC service");

    // ListCapabilities
    let list = client
        .list_capabilities(ListCapabilitiesRequest {
            selector: "all".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(list.agent_id, "did:spine:e2e");
    assert_eq!(list.capabilities.len(), 1);
    assert_eq!(list.capabilities[0].uri, "agent.math/add");

    // CallTool — real protobuf round-trip through the executor.
    let resp = client
        .call_tool(CallToolRequest {
            id: "c1".into(),
            name: "agent.math/add".into(),
            args_json: json!({"a": 2, "b": 40}).to_string(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(resp.ok);
    assert_eq!(resp.content_json, "{\"sum\":42}");

    // StreamChat — server-streaming over HTTP/2.
    let mut stream = client
        .stream_chat(StreamChatRequest {
            model: "demo".into(),
            prompt: "one two".into(),
        })
        .await
        .unwrap()
        .into_inner();
    let mut texts = Vec::new();
    let mut saw_done = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap();
        if chunk.done {
            saw_done = true;
            assert_eq!(chunk.finish_reason, "stop");
        } else {
            texts.push(chunk.text);
        }
    }
    assert_eq!(texts, vec!["one".to_string(), " two".to_string()]);
    assert!(saw_done);
}
