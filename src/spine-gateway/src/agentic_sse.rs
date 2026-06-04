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
        // Latent chunk — SPINE-aware clients read `spine_encoded`,
        // legacy clients see an empty content delta and move on.
        StreamData::Encoded(frame) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&frame.data);
            json!({
                "spine_encoded": {
                    "codec": frame.codec,
                    "variant": frame.variant,
                    "modality": frame.metadata.modality,
                    "shape": frame.metadata.shape,
                    "dtype": frame.metadata.dtype,
                    "data_b64": b64,
                }
            })
        }
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
                StreamData::Encoded(_) => content,
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
///
/// **Privacy note**: `skip_all` keeps user message content out of
/// tracing spans. The handler intentionally emits no log lines that
/// include request bodies.
#[tracing::instrument(skip_all, fields(model = %req.model, stream = req.stream))]
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
        decode_hints: None,
        stream_codec: None,
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
#[tracing::instrument]
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
// Neural codec endpoints
// ---------------------------------------------------------------------------

use std::sync::{Arc, OnceLock};

use spine_protocol::{CodecRegistry, EmbeddingInput, EmbeddingRequest, TitansLatentCodec};

/// Process-wide codec registry. Lazily initialised on first access with
/// the default `TitansLatentCodec` (256-dim, f32). Production deployments
/// pre-populate via [`spine_protocol::CodecRegistry::register`] at start
/// up — once you hold an `Arc<CodecRegistry>`, registration is
/// concurrent-safe.
pub fn registry() -> &'static CodecRegistry {
    static REG: OnceLock<CodecRegistry> = OnceLock::new();
    REG.get_or_init(|| {
        let r = CodecRegistry::new();
        r.register(Arc::new(TitansLatentCodec::new(256)));
        r
    })
}

/// OpenAI-compatible request: `{ "input": <string|[string]>, "model": "..." }`.
#[derive(Debug, Deserialize)]
pub struct EmbeddingsHttpReq {
    pub input: serde_json::Value,
    #[serde(default = "default_embed_model")]
    pub model: String,
}

fn default_embed_model() -> String {
    "spine:codec/titans/v1@dim=256,dtype=f32".into()
}

/// `POST /v1/embeddings` — OpenAI-shaped wrapper around
/// [`EmbeddingRequest`] / [`EmbeddingResponse`]. Internally:
///
/// 1. Reshape the OpenAI payload into [`EmbeddingRequest`].
/// 2. Resolve the codec via the process registry.
/// 3. Encode every input element to an [`EncodedFrame`].
/// 4. Project the f32 latent back into the OpenAI response shape so the
///    existing client SDK reads `data[i].embedding` as a `[f32]`.
///
/// **Privacy note**: `skip_all` keeps embedding input out of tracing
/// spans. The handler logs no input content.
#[tracing::instrument(skip_all, fields(model = %req.model))]
pub async fn embeddings(
    Json(req): Json<EmbeddingsHttpReq>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    use axum::http::StatusCode;

    // Map OpenAI's input → SPINE EmbeddingInput.
    let input = match &req.input {
        serde_json::Value::String(s) => EmbeddingInput::Text(s.clone()),
        serde_json::Value::Array(arr) => {
            let texts: Result<Vec<String>, _> = arr
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .ok_or("array elements must be strings")
                })
                .collect();
            match texts {
                Ok(t) => EmbeddingInput::Texts(t),
                Err(e) => return Err((StatusCode::BAD_REQUEST, e.to_string())),
            }
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "input must be a string or array of strings".into(),
            ))
        }
    };

    let spine_req = EmbeddingRequest {
        id: uuid::Uuid::new_v4().to_string(),
        input,
        codec: Some(req.model.clone()),
        trace: None,
    };

    // Resolve codec — fall back to the registry's first codec when the
    // client asked for one we don't have (keeps existing OpenAI SDK
    // smoke tests working with `text-embedding-3-large`).
    let codec_id = if registry().get(&req.model).is_some() {
        req.model.clone()
    } else {
        registry()
            .ids()
            .first()
            .cloned()
            .ok_or((StatusCode::SERVICE_UNAVAILABLE, "no codecs registered".into()))?
    };

    let codec = registry().get(&codec_id).expect("just resolved");

    // Encode each input.
    let texts: Vec<String> = match &spine_req.input {
        EmbeddingInput::Text(s) => vec![s.clone()],
        EmbeddingInput::Texts(v) => v.clone(),
        EmbeddingInput::Encoded(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Encoded input not supported by this gateway endpoint; use /v1/embeddings/raw"
                    .into(),
            ))
        }
    };

    let mut data = Vec::with_capacity(texts.len());
    let mut total_input_bytes: u64 = 0;
    for (i, text) in texts.iter().enumerate() {
        let frame = match codec.encode(text.as_bytes()) {
            Ok(f) => f,
            Err(e) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("encode failed: {e}"),
                ))
            }
        };
        total_input_bytes += text.len() as u64;
        // Project f32 LE bytes back into a JSON array of numbers so the
        // OpenAI SDK reads `data[i].embedding` natively.
        let mut floats = Vec::with_capacity(frame.data.len() / 4);
        for chunk in frame.data.chunks_exact(4) {
            floats.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        data.push(json!({
            "object": "embedding",
            "embedding": floats,
            "index": i,
        }));
    }

    Ok(Json(json!({
        "object": "list",
        "data": data,
        "model": codec_id,
        "usage": {
            "prompt_tokens": total_input_bytes,
            "total_tokens": total_input_bytes,
        }
    })))
}

/// `GET /v1/agentic/codecs` — emits the gateway's
/// [`CodecAdvertisement`] (every registered [`NeuralCodec`]). Lets HTTP
/// clients discover what encoder/decoder pairs are available without
/// learning SPINE binary frames.
#[tracing::instrument]
pub async fn codecs() -> Json<serde_json::Value> {
    let ad = registry().advertise("spine-gateway", "static-gateway");
    Json(serde_json::to_value(&ad).expect("CodecAdvertisement is serializable"))
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

    #[tokio::test]
    async fn embeddings_endpoint_round_trip_via_titans() {
        // Single string input.
        let req = Json(EmbeddingsHttpReq {
            input: json!("the quick brown fox"),
            model: "spine:codec/titans/v1@dim=256,dtype=f32".into(),
        });
        let resp = embeddings(req).await.expect("ok").0;
        assert_eq!(resp["object"], "list");
        assert_eq!(resp["model"], "spine:codec/titans/v1@dim=256,dtype=f32");
        assert_eq!(resp["data"].as_array().unwrap().len(), 1);
        let embedding = resp["data"][0]["embedding"].as_array().unwrap();
        assert_eq!(embedding.len(), 256);
        // Every element is a finite f32.
        for v in embedding {
            let f = v.as_f64().expect("finite number");
            assert!(f.is_finite(), "embedding had NaN/Inf");
        }
        // Usage is reported by source byte length, not token count
        // (we don't have a tokeniser at this layer).
        assert_eq!(
            resp["usage"]["prompt_tokens"].as_u64().unwrap(),
            "the quick brown fox".len() as u64
        );
    }

    #[tokio::test]
    async fn embeddings_endpoint_handles_array_input() {
        let req = Json(EmbeddingsHttpReq {
            input: json!(["a", "b", "c"]),
            model: "spine:codec/titans/v1@dim=256,dtype=f32".into(),
        });
        let resp = embeddings(req).await.expect("ok").0;
        let data = resp["data"].as_array().unwrap();
        assert_eq!(data.len(), 3);
        // Indices are sequential.
        for (i, item) in data.iter().enumerate() {
            assert_eq!(item["index"].as_u64().unwrap(), i as u64);
        }
    }

    #[tokio::test]
    async fn embeddings_endpoint_falls_back_to_registry_default() {
        // An OpenAI-style model name the gateway doesn't know — should
        // fall back to whatever the registry has (Titans by default).
        let req = Json(EmbeddingsHttpReq {
            input: json!("hi"),
            model: "text-embedding-3-large".into(),
        });
        let resp = embeddings(req).await.expect("ok").0;
        assert!(resp["model"]
            .as_str()
            .unwrap()
            .starts_with("spine:codec/"));
    }

    #[tokio::test]
    async fn embeddings_endpoint_rejects_bad_input_shape() {
        let req = Json(EmbeddingsHttpReq {
            input: json!(42),
            model: default_embed_model(),
        });
        let err = embeddings(req).await.unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn codecs_endpoint_returns_advertisement() {
        let ad = codecs().await.0;
        assert_eq!(ad["agent_id"], "spine-gateway");
        let list = ad["codecs"].as_array().unwrap();
        assert!(!list.is_empty(), "registry should have at least Titans");
        let ids: Vec<&str> = list.iter().filter_map(|c| c["id"].as_str()).collect();
        assert!(
            ids.iter().any(|id| id.contains("titans")),
            "default Titans codec missing: {ids:?}"
        );
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
