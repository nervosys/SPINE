//! HTTP/SSE bridge for SPINE's agentic frame types.
//!
//! Modern LLM clients (OpenAI SDK, LangChain, Vercel AI SDK, the
//! Anthropic SDK in stream-passthrough mode) speak Server-Sent Events
//! over an `/v1/chat/completions`-style POST. This module translates
//! between that wire format and SPINE's native
//! [`spine_protocol::StreamStart`] / [`spine_protocol::StreamToken`] /
//! [`spine_protocol::StreamEnd`] frame triple, so every existing HTTP
//! client can consume a SPINE-emitted token stream without learning a
//! new SDK.
//!
//! The conversion goes both ways:
//!
//! * `StreamToken` → OpenAI `chat.completion.chunk` (one SSE event)
//! * `StreamEnd::reason` → OpenAI `finish_reason` string
//!
//! [`chat_completions_stream`] is a working demonstration: it accepts a
//! standard OpenAI request, tokenises the last user message, threads it
//! through SPINE's `StreamToken` pipeline, and emits OpenAI SSE. Real
//! deployments swap the in-memory echo source for the LLM provider of
//! their choice — the converter doesn't change.

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use spine_protocol::{
    StreamData, StreamEnd, StreamEndReason, StreamRole, StreamStart, StreamToken,
};
use std::convert::Infallible;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// OpenAI-compatible request / response shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    #[serde(default = "default_model")]
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_tokens: Option<u64>,
}

fn default_model() -> String {
    "spine-echo".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Conversion helpers — these are the load-bearing functions.
// ---------------------------------------------------------------------------

/// Translate a SPINE [`StreamToken`] into a single OpenAI-format
/// `chat.completion.chunk` payload (the value that goes after `data: `).
///
/// `stream_id` and `model` come from the corresponding [`StreamStart`].
pub fn stream_token_to_openai_chunk(
    stream_id: &str,
    model: &str,
    token: &StreamToken,
) -> serde_json::Value {
    let content: serde_json::Value = match &token.data {
        StreamData::Text(s) => json!(s),
        // OpenAI SSE has no native binary delta — fall back to base64.
        StreamData::Bytes(b) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(b);
            json!({"b64": b64})
        }
        // Tool calls embed as a function_call delta — modern OpenAI
        // clients accept the v2 `tool_calls` array shape.
        StreamData::ToolCall(tc) => json!({
            "tool_calls": [{
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.name,
                    "arguments": tc.args.to_string(),
                }
            }]
        }),
    };

    json!({
        "id": format!("chatcmpl-{stream_id}"),
        "object": "chat.completion.chunk",
        "created": unix_now_secs(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": match &token.data {
                StreamData::ToolCall(_) => content,
                _ => json!({"role": "assistant", "content": content}),
            },
            "finish_reason": serde_json::Value::Null,
        }]
    })
}

/// Translate a SPINE [`StreamEnd`] into the final OpenAI chunk (with
/// non-null `finish_reason`). After this chunk, callers should emit the
/// terminating `data: [DONE]\n\n` SSE line — see [`openai_sse_done`].
pub fn stream_end_to_openai_chunk(
    stream_id: &str,
    model: &str,
    end: &StreamEnd,
) -> serde_json::Value {
    let reason = match &end.reason {
        StreamEndReason::Stop => "stop",
        StreamEndReason::Length => "length",
        StreamEndReason::ToolUse => "tool_calls",
        StreamEndReason::ContentFilter => "content_filter",
        StreamEndReason::Cancelled => "cancelled",
        StreamEndReason::Error { .. } => "error",
    };

    let mut chunk = json!({
        "id": format!("chatcmpl-{stream_id}"),
        "object": "chat.completion.chunk",
        "created": unix_now_secs(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": reason,
        }]
    });

    if let Some(u) = &end.usage {
        chunk["usage"] = json!({
            "prompt_tokens": u.input_tokens,
            "completion_tokens": u.output_tokens,
            "total_tokens": u.input_tokens + u.output_tokens,
        });
    }
    chunk
}

/// The OpenAI SSE terminator line. Conforming clients stop reading on
/// this marker.
pub fn openai_sse_done() -> &'static str {
    "[DONE]"
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Demonstration endpoint — `/v1/chat/completions`
// ---------------------------------------------------------------------------

/// Minimal OpenAI-shaped chat-completions handler. Tokenises the last
/// user message word-by-word, threads each word through SPINE's
/// [`StreamToken`] frame, and emits OpenAI SSE.
///
/// Real deployments replace the echo body with their LLM provider — the
/// SPINE→SSE converter (above) is what carries the load.
pub async fn chat_completions_stream(
    Json(req): Json<ChatCompletionRequest>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let user_msg = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .ok_or((
            StatusCode::BAD_REQUEST,
            "no user message in `messages`".to_string(),
        ))?;

    if !req.stream {
        // Non-streaming: return a single chat.completion envelope.
        let body = json!({
            "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion",
            "created": unix_now_secs(),
            "model": req.model,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": user_msg.clone()},
                "finish_reason": "stop",
            }],
            "usage": {
                "prompt_tokens": user_msg.len() as u64,
                "completion_tokens": user_msg.len() as u64,
                "total_tokens": user_msg.len() as u64 * 2,
            }
        });
        return Ok(Json(body).into_response());
    }

    let stream_id = uuid::Uuid::new_v4().to_string();
    let model = req.model.clone();
    let max = req.max_tokens.unwrap_or(u64::MAX);

    // Synthesise the StreamStart/Token/End triple from the echo body.
    let _start = StreamStart {
        id: stream_id.clone(),
        role: StreamRole::Assistant,
        model: model.clone(),
        trace: None,
    };
    let words: Vec<String> = user_msg
        .split_whitespace()
        .take(max as usize)
        .map(|w| format!("{w} "))
        .collect();
    let end = StreamEnd {
        id: stream_id.clone(),
        reason: StreamEndReason::Stop,
        usage: Some(spine_protocol::StreamUsage {
            input_tokens: user_msg.len() as u64,
            output_tokens: words.len() as u64,
        }),
    };

    // Compose the SSE stream: one chunk per token, then a final chunk
    // with finish_reason, then [DONE].
    let stream = sse_stream_from_tokens(stream_id, model, words, end);
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}

/// Build an `impl Stream<Item = Result<Event, Infallible>>` from a
/// pre-computed token list + end marker. Splitting this out lets the
/// converter be tested without spinning up axum.
fn sse_stream_from_tokens(
    stream_id: String,
    model: String,
    words: Vec<String>,
    end: StreamEnd,
) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    use futures::stream;

    let mut events = Vec::with_capacity(words.len() + 2);
    for (seq, word) in words.into_iter().enumerate() {
        let tok = StreamToken {
            id: stream_id.clone(),
            seq: seq as u64,
            data: StreamData::Text(word),
        };
        let chunk = stream_token_to_openai_chunk(&stream_id, &model, &tok);
        events.push(Ok(Event::default().data(chunk.to_string())));
    }
    let final_chunk = stream_end_to_openai_chunk(&stream_id, &model, &end);
    events.push(Ok(Event::default().data(final_chunk.to_string())));
    events.push(Ok(Event::default().data(openai_sse_done())));

    stream::iter(events)
}

// ---------------------------------------------------------------------------
// Capability advertisement endpoint
// ---------------------------------------------------------------------------

/// Returns the gateway's [`CapabilityAdvertisement`] — what the SPINE
/// gateway can be asked to do. HTTP clients use this for discovery
/// without needing to learn SPINE binary frames.
pub async fn capabilities() -> Json<serde_json::Value> {
    use spine_protocol::{Capability, CapabilityAdvertisement};

    let ad = CapabilityAdvertisement {
        id: "static-gateway".into(),
        agent_id: "spine-gateway".into(),
        capabilities: vec![
            Capability {
                uri: "spine:cap/web/navigate".into(),
                description: "Open a URL in a SPINE session and return the parsed UR".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {"url": {"type": "string"}},
                    "required": ["url"]
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "element_count": {"type": "integer"},
                    }
                }),
                embedding: None,
            },
            Capability {
                uri: "spine:cap/compute/hls".into(),
                description: "Compile and execute an HLS script in the SPINE WASM runtime".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {"script": {"type": "string"}},
                    "required": ["script"]
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {"result": {}}
                }),
                embedding: None,
            },
            Capability {
                uri: "spine:cap/llm/chat".into(),
                description: "OpenAI-compatible chat-completions endpoint with SSE streaming"
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "messages": {"type": "array"},
                        "stream": {"type": "boolean"},
                    },
                    "required": ["messages"]
                }),
                output_schema: json!({
                    "type": "object",
                    "properties": {
                        "choices": {"type": "array"}
                    }
                }),
                embedding: None,
            },
        ],
    };
    Json(serde_json::to_value(&ad).expect("CapabilityAdvertisement is serializable"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use spine_protocol::{StreamData, StreamEndReason, StreamToken};

    #[test]
    fn token_text_to_openai_chunk_shape() {
        let tok = StreamToken {
            id: "s1".into(),
            seq: 3,
            data: StreamData::Text("hello".into()),
        };
        let chunk = stream_token_to_openai_chunk("s1", "claude-opus-4-7", &tok);
        assert_eq!(chunk["id"], "chatcmpl-s1");
        assert_eq!(chunk["object"], "chat.completion.chunk");
        assert_eq!(chunk["model"], "claude-opus-4-7");
        assert_eq!(chunk["choices"][0]["index"], 0);
        assert_eq!(chunk["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(chunk["choices"][0]["delta"]["content"], "hello");
        assert!(chunk["choices"][0]["finish_reason"].is_null());
    }

    #[test]
    fn end_chunk_carries_finish_reason_and_usage() {
        let end = StreamEnd {
            id: "s1".into(),
            reason: StreamEndReason::Length,
            usage: Some(spine_protocol::StreamUsage {
                input_tokens: 10,
                output_tokens: 100,
            }),
        };
        let chunk = stream_end_to_openai_chunk("s1", "test-model", &end);
        assert_eq!(chunk["choices"][0]["finish_reason"], "length");
        assert_eq!(chunk["usage"]["prompt_tokens"], 10);
        assert_eq!(chunk["usage"]["completion_tokens"], 100);
        assert_eq!(chunk["usage"]["total_tokens"], 110);
    }

    #[test]
    fn end_reason_maps_match_openai_taxonomy() {
        let cases = vec![
            (StreamEndReason::Stop, "stop"),
            (StreamEndReason::Length, "length"),
            (StreamEndReason::ToolUse, "tool_calls"),
            (StreamEndReason::ContentFilter, "content_filter"),
            (StreamEndReason::Cancelled, "cancelled"),
            (
                StreamEndReason::Error {
                    code: "x".into(),
                    message: "y".into(),
                },
                "error",
            ),
        ];
        for (reason, expected) in cases {
            let end = StreamEnd {
                id: "s".into(),
                reason,
                usage: None,
            };
            let chunk = stream_end_to_openai_chunk("s", "m", &end);
            assert_eq!(chunk["choices"][0]["finish_reason"], expected);
        }
    }

    #[test]
    fn tool_call_token_uses_tool_calls_delta() {
        let tc = spine_protocol::ToolCall {
            id: "call_1".into(),
            name: "search".into(),
            args: json!({"q": "rust"}),
            trace: None,
        };
        let tok = StreamToken {
            id: "s1".into(),
            seq: 0,
            data: StreamData::ToolCall(tc),
        };
        let chunk = stream_token_to_openai_chunk("s1", "m", &tok);
        let tool_calls = &chunk["choices"][0]["delta"]["tool_calls"];
        assert!(tool_calls.is_array());
        assert_eq!(tool_calls[0]["id"], "call_1");
        assert_eq!(tool_calls[0]["function"]["name"], "search");
    }
}
