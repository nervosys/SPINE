//! Agentic-first protocol primitives — the surface that makes SPINE more
//! than another binary RPC.
//!
//! Four families are defined here, and each is wired into [`crate::Message`]
//! as a first-class variant rather than smuggled through an opaque
//! `Request`:
//!
//! 1. **Tool calling** ([`ToolCall`] / [`ToolResult`]) — MCP-shaped:
//!    a stable `id` ties request to response so multiple tool calls can be
//!    in flight on a single stream. The `name` is the tool identifier and
//!    `args` is the structured argument payload. `ToolResult` carries
//!    either a success value or a typed error.
//!
//! 2. **Token streaming** ([`StreamStart`] / [`StreamToken`] / [`StreamEnd`])
//!    — LLM tokens are the currency of modern agents. SPINE expresses an
//!    LLM completion as a stream identified by an `id` shared across all
//!    three frames; tokens carry a monotonically increasing `seq` so the
//!    receiver can detect gaps. `StreamEnd::reason` matches the OpenAI /
//!    Anthropic finish-reason taxonomy.
//!
//! 3. **Capability negotiation** ([`CapabilityQuery`] /
//!    [`CapabilityAdvertisement`]) — agents announce what they can do.
//!    Queries can be exact (`capability:foo`), prefix (`capability:foo/*`),
//!    or semantic (an embedding the responder is expected to match against
//!    its own capability embeddings). The advertisement carries one or
//!    more [`Capability`] descriptors with a stable schema so a planner
//!    can route by interface shape rather than by hard-coded URL.
//!
//! 4. **Distributed tracing** ([`TraceContext`]) — a W3C-compatible
//!    `traceparent` (with optional `tracestate`) attached to any agent
//!    message so a swarm's call graph can be reconstructed end-to-end.
//!    The format matches `traceparent: 00-<trace_id>-<span_id>-<flags>`
//!    so existing OpenTelemetry collectors can ingest SPINE traces with
//!    no translator.
//!
//! All types are `serde`-friendly and round-trip through `bincode`. They
//! are deliberately small and `Clone`-able so they survive being passed
//! through retry / replay layers.

use serde::{Deserialize, Serialize};

use crate::agentic_codec::{DecodeHints, EncodedFrame};

// =============================================================================
// Tool calling (MCP-shaped)
// =============================================================================

/// One agent asks another to invoke a tool. Multiple `ToolCall`s with
/// distinct `id`s may be in flight concurrently on the same stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    /// Caller-chosen correlation id. Echoed back in the matching
    /// [`ToolResult`]. UUIDs are recommended but any string is allowed.
    pub id: String,
    /// Tool identifier — typically a URI like `tool:fetch_url` or a
    /// capability path like `agent.web/fetch`. Lookup is performed
    /// against the responder's [`CapabilityAdvertisement`].
    pub name: String,
    /// Structured arguments. JSON-shaped so a tool's schema can be
    /// expressed in JSON Schema and validated independently.
    pub args: serde_json::Value,
    /// Optional W3C trace context for distributed tracing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceContext>,
}

/// Result of a [`ToolCall`]. `id` is copied verbatim from the request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    /// Correlation id from the originating [`ToolCall`].
    pub id: String,
    /// The actual outcome.
    pub outcome: ToolOutcome,
    /// Optional W3C trace context (typically copied from the call).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceContext>,
}

/// Either a structured success payload or a typed error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolOutcome {
    /// Successful invocation. `content` is the tool's return value in the
    /// shape declared by the tool's schema.
    Ok { content: serde_json::Value },
    /// Tool refused, errored, or timed out. `code` is a stable string
    /// identifier (e.g. `"not_authorized"`, `"timeout"`, `"invalid_args"`)
    /// so callers can branch without parsing the human-readable `message`.
    Err {
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
}

// =============================================================================
// LLM token streaming
// =============================================================================

/// Opens a token stream. The receiver should expect zero or more
/// [`StreamToken`]s tagged with the same `id`, terminated by a
/// [`StreamEnd`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamStart {
    /// Stream identifier. Must be unique within a single transport
    /// connection lifetime; UUIDs are recommended.
    pub id: String,
    /// Role of the producer (assistant, tool, user-echo, system).
    pub role: StreamRole,
    /// Model name / version (e.g. `claude-opus-4-7`). Free-form.
    pub model: String,
    /// Optional W3C trace context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceContext>,
    /// Optional sampling parameters for the decoder. When present, the
    /// producer is being asked to respect these for the duration of
    /// this stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_hints: Option<DecodeHints>,
    /// Optional codec the producer will use for [`StreamData::Encoded`]
    /// chunks in this stream. Lets the receiver pre-resolve the
    /// decoder once instead of looking it up per token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_codec: Option<String>,
}

/// Producer role for a stream.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum StreamRole {
    Assistant,
    Tool,
    User,
    System,
}

/// One chunk of stream output. `seq` starts at 0 and increments by one
/// per chunk per `id`; gaps indicate loss.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamToken {
    pub id: String,
    pub seq: u64,
    pub data: StreamData,
}

/// What's in a token chunk. Most LLM streams are `Text`; multimodal
/// agents emit `Bytes` (audio frames, image patches, tool deltas);
/// latent-streaming agents emit `Encoded` so receivers can act on the
/// raw representation without paying for a token detour.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StreamData {
    /// UTF-8 text fragment.
    Text(String),
    /// Opaque bytes (e.g. audio frame, partial image).
    Bytes(Vec<u8>),
    /// Tool invocation embedded mid-stream (function calling).
    ToolCall(ToolCall),
    /// Self-describing latent chunk. The receiver can decode it locally
    /// via a registered codec or forward it verbatim to a downstream
    /// peer that speaks the same encoder/decoder pair.
    Encoded(EncodedFrame),
}

/// Closes a token stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamEnd {
    pub id: String,
    pub reason: StreamEndReason,
    /// Optional usage statistics. Matches the OpenAI `usage` object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<StreamUsage>,
}

/// Why a stream ended. Mirrors OpenAI / Anthropic finish reasons so
/// existing client code can switch on the same set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StreamEndReason {
    /// Model emitted its end-of-turn token.
    Stop,
    /// `max_tokens` reached.
    Length,
    /// Stopped to invoke a tool.
    ToolUse,
    /// Content filter or policy block.
    ContentFilter,
    /// Cancelled by the client.
    Cancelled,
    /// Producer errored mid-stream.
    Error { code: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

// =============================================================================
// Capability negotiation
// =============================================================================

/// One agent asks another what it can do.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilityQuery {
    pub id: String,
    pub selector: CapabilitySelector,
}

/// Three ways to specify what we're looking for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CapabilitySelector {
    /// Exact capability URI match (e.g. `spine:cap/web/scraping`).
    Exact(String),
    /// Prefix match (e.g. `spine:cap/web/*`).
    Prefix(String),
    /// Semantic match — responder is expected to compare against its
    /// own capability embeddings and return the top-K matches.
    Semantic { embedding: Vec<f32>, top_k: u32 },
    /// Return everything.
    All,
}

/// Response to a [`CapabilityQuery`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilityAdvertisement {
    /// Correlation id from the originating [`CapabilityQuery`].
    pub id: String,
    /// Cryptographic identity of the advertising agent (e.g. an Ed25519
    /// public key fingerprint or a DID).
    pub agent_id: String,
    pub capabilities: Vec<Capability>,
}

/// One capability the agent offers. The schema is intentionally small —
/// rich types (cost, latency, SLAs) belong in higher-layer marketplaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Capability {
    /// Stable URI for this capability.
    pub uri: String,
    /// Human-readable description.
    pub description: String,
    /// JSON-Schema-shaped argument descriptor — what `ToolCall::args`
    /// must look like to invoke this capability.
    pub input_schema: serde_json::Value,
    /// JSON-Schema-shaped result descriptor — what `ToolResult` content
    /// will look like on success.
    pub output_schema: serde_json::Value,
    /// Optional semantic embedding for use with
    /// [`CapabilitySelector::Semantic`]. Length is provider-chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
}

// =============================================================================
// Distributed tracing (W3C-compatible)
// =============================================================================

/// W3C Trace Context. The `traceparent` header form is:
/// `00-<trace_id:32hex>-<span_id:16hex>-<flags:2hex>`.
///
/// Carried inline on [`ToolCall`], [`ToolResult`], and [`StreamStart`].
/// Existing OpenTelemetry collectors can ingest this format with no
/// translator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceContext {
    /// 128-bit trace id.
    pub trace_id: [u8; 16],
    /// 64-bit span id of the producer.
    pub span_id: [u8; 8],
    /// W3C flags byte (bit 0 = sampled).
    pub flags: u8,
    /// Optional `tracestate` key-value list (small, opaque).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<(String, String)>,
}

impl TraceContext {
    /// Render as the W3C `traceparent` header value.
    pub fn to_traceparent(&self) -> String {
        let trace_hex: String = self.trace_id.iter().map(|b| format!("{b:02x}")).collect();
        let span_hex: String = self.span_id.iter().map(|b| format!("{b:02x}")).collect();
        format!("00-{trace_hex}-{span_hex}-{:02x}", self.flags)
    }

    /// Parse a W3C `traceparent` header value. Returns `None` if the
    /// version, lengths, or hex encoding are malformed.
    pub fn from_traceparent(s: &str) -> Option<Self> {
        let mut parts = s.split('-');
        let version = parts.next()?;
        let trace = parts.next()?;
        let span = parts.next()?;
        let flags = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        if version != "00" || trace.len() != 32 || span.len() != 16 || flags.len() != 2 {
            return None;
        }
        let mut trace_id = [0u8; 16];
        for (i, b) in trace_id.iter_mut().enumerate() {
            *b = u8::from_str_radix(&trace[i * 2..i * 2 + 2], 16).ok()?;
        }
        let mut span_id = [0u8; 8];
        for (i, b) in span_id.iter_mut().enumerate() {
            *b = u8::from_str_radix(&span[i * 2..i * 2 + 2], 16).ok()?;
        }
        let flags = u8::from_str_radix(flags, 16).ok()?;
        // All-zero trace_id or span_id is reserved by the spec.
        if trace_id == [0u8; 16] || span_id == [0u8; 8] {
            return None;
        }
        Some(Self {
            trace_id,
            span_id,
            flags,
            state: Vec::new(),
        })
    }

    /// Whether this trace is marked sampled (bit 0 of flags).
    pub fn is_sampled(&self) -> bool {
        self.flags & 0x01 != 0
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// All agentic types must survive the wire format SPINE actually uses
    /// (`serde_json::to_vec` — see ProtocolHandler::send_message). Bincode
    /// 1.x cannot round-trip `serde_json::Value` because it relies on
    /// `deserialize_any`, so this helper matches production rather than
    /// papering over the gap with bincode.
    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: &T) {
        let bytes = serde_json::to_vec(v).expect("encode");
        let back: T = serde_json::from_slice(&bytes).expect("decode");
        assert_eq!(v, &back);
    }

    #[test]
    fn tool_call_round_trip() {
        let call = ToolCall {
            id: "call_42".into(),
            name: "fetch_url".into(),
            args: json!({"url": "https://example.com"}),
            trace: None,
        };
        round_trip(&call);
    }

    #[test]
    fn tool_result_ok_round_trip() {
        let r = ToolResult {
            id: "call_42".into(),
            outcome: ToolOutcome::Ok {
                content: json!({"status": 200, "body": "<html/>"}),
            },
            trace: None,
        };
        round_trip(&r);
    }

    #[test]
    fn tool_result_err_round_trip() {
        let r = ToolResult {
            id: "call_42".into(),
            outcome: ToolOutcome::Err {
                code: "timeout".into(),
                message: "tool exceeded 30s budget".into(),
                details: Some(json!({"elapsed_ms": 30_100})),
            },
            trace: None,
        };
        round_trip(&r);
    }

    #[test]
    fn stream_lifecycle_round_trip() {
        let start = StreamStart {
            id: "s1".into(),
            role: StreamRole::Assistant,
            model: "claude-opus-4-7".into(),
            trace: None,
            decode_hints: None,
            stream_codec: None,
        };
        round_trip(&start);

        let tok = StreamToken {
            id: "s1".into(),
            seq: 7,
            data: StreamData::Text("hello ".into()),
        };
        round_trip(&tok);

        let end = StreamEnd {
            id: "s1".into(),
            reason: StreamEndReason::Stop,
            usage: Some(StreamUsage {
                input_tokens: 42,
                output_tokens: 1337,
            }),
        };
        round_trip(&end);
    }

    /// Latent streaming: every token chunk carries a self-describing
    /// `EncodedFrame`. This is the path agents use when they want to
    /// hand each other hidden states or embeddings rather than text.
    #[test]
    fn stream_token_with_encoded_payload() {
        use crate::agentic_codec::{DType, EncodedFrame, EncodedMetadata, Modality};

        let frame = EncodedFrame {
            codec: "spine:codec/titans/v1@dim=4,dtype=f32".into(),
            variant: None,
            data: vec![0; 16],
            metadata: EncodedMetadata {
                modality: Modality::Embedding,
                shape: vec![4],
                dtype: DType::F32,
                original_len: Some(7),
                source_hash: None,
            },
            trace: None,
        };
        let tok = StreamToken {
            id: "s1".into(),
            seq: 3,
            data: StreamData::Encoded(frame.clone()),
        };
        round_trip(&tok);

        // And in StreamStart, decode_hints + stream_codec ride along.
        let start = StreamStart {
            id: "s1".into(),
            role: StreamRole::Assistant,
            model: "claude-opus-4-7".into(),
            trace: None,
            decode_hints: Some(crate::agentic_codec::DecodeHints {
                temperature: Some(0.3),
                top_p: Some(0.9),
                max_tokens: Some(512),
                ..Default::default()
            }),
            stream_codec: Some(frame.codec.clone()),
        };
        round_trip(&start);
    }

    #[test]
    fn stream_token_with_tool_call() {
        // Function-calling mid-stream.
        let tok = StreamToken {
            id: "s1".into(),
            seq: 12,
            data: StreamData::ToolCall(ToolCall {
                id: "call_99".into(),
                name: "search".into(),
                args: json!({"q": "rust async"}),
                trace: None,
            }),
        };
        round_trip(&tok);
    }

    #[test]
    fn capability_selector_round_trip() {
        round_trip(&CapabilitySelector::Exact("spine:cap/web/scraping".into()));
        round_trip(&CapabilitySelector::Prefix("spine:cap/web/*".into()));
        round_trip(&CapabilitySelector::All);
        round_trip(&CapabilitySelector::Semantic {
            embedding: vec![0.1, 0.2, 0.3, -0.4],
            top_k: 5,
        });
    }

    #[test]
    fn capability_advertisement_round_trip() {
        let ad = CapabilityAdvertisement {
            id: "q1".into(),
            agent_id: "did:spine:0xdeadbeef".into(),
            capabilities: vec![Capability {
                uri: "spine:cap/web/fetch".into(),
                description: "Fetch a URL and return body".into(),
                input_schema: json!({"type": "object",
                                    "properties": {"url": {"type": "string"}}}),
                output_schema: json!({"type": "object",
                                     "properties": {"body": {"type": "string"}}}),
                embedding: Some(vec![0.5; 8]),
            }],
        };
        round_trip(&ad);
    }

    #[test]
    fn traceparent_round_trip() {
        let tc = TraceContext {
            trace_id: [
                0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d,
                0x0e, 0x0e, 0x47, 0x36,
            ],
            span_id: [0x00, 0xf0, 0x67, 0xaa, 0x0b, 0xa9, 0x02, 0xb7],
            flags: 0x01,
            state: vec![("vendor".into(), "spine".into())],
        };
        let header = tc.to_traceparent();
        assert_eq!(
            header,
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        );
        let parsed = TraceContext::from_traceparent(&header).unwrap();
        assert_eq!(parsed.trace_id, tc.trace_id);
        assert_eq!(parsed.span_id, tc.span_id);
        assert_eq!(parsed.flags, tc.flags);
        // `state` is not encoded in the traceparent header; round-trip
        // is intentionally lossy for it.
        round_trip(&tc);
    }

    #[test]
    fn traceparent_rejects_malformed() {
        // Wrong version.
        assert!(TraceContext::from_traceparent(
            "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        )
        .is_none());
        // All-zero trace id is reserved.
        assert!(TraceContext::from_traceparent(
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01"
        )
        .is_none());
        // All-zero span id is reserved.
        assert!(TraceContext::from_traceparent(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01"
        )
        .is_none());
        // Truncated.
        assert!(TraceContext::from_traceparent("00-4bf9-00f0-01").is_none());
        // Extra segments.
        assert!(TraceContext::from_traceparent(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra"
        )
        .is_none());
    }

    /// Confirm the new variants survive being wrapped in the top-level
    /// [`crate::Message`] enum (the actual on-the-wire envelope).
    #[test]
    fn message_envelope_dispatch() {
        use crate::Message;

        let cases: Vec<Message> = vec![
            Message::ToolCall(ToolCall {
                id: "c1".into(),
                name: "tool".into(),
                args: json!(null),
                trace: None,
            }),
            Message::ToolResult(ToolResult {
                id: "c1".into(),
                outcome: ToolOutcome::Ok { content: json!(42) },
                trace: None,
            }),
            Message::StreamStart(StreamStart {
                id: "s1".into(),
                role: StreamRole::Assistant,
                model: "claude-opus-4-7".into(),
                trace: None,
                decode_hints: None,
                stream_codec: None,
            }),
            Message::StreamToken(StreamToken {
                id: "s1".into(),
                seq: 0,
                data: StreamData::Text("hi".into()),
            }),
            Message::StreamEnd(StreamEnd {
                id: "s1".into(),
                reason: StreamEndReason::Stop,
                usage: None,
            }),
            Message::CapabilityQuery(CapabilityQuery {
                id: "q1".into(),
                selector: CapabilitySelector::All,
            }),
            Message::CapabilityAd(CapabilityAdvertisement {
                id: "q1".into(),
                agent_id: "agent_a".into(),
                capabilities: vec![],
            }),
            // Neural codec frames slot into the same envelope.
            Message::Encoded(crate::agentic_codec::EncodedFrame {
                codec: "spine:codec/echo/v1".into(),
                variant: None,
                data: b"abc".to_vec(),
                metadata: crate::agentic_codec::EncodedMetadata {
                    modality: crate::agentic_codec::Modality::Text,
                    shape: vec![3],
                    dtype: crate::agentic_codec::DType::U8,
                    original_len: Some(3),
                    source_hash: None,
                },
                trace: None,
            }),
            Message::CodecAd(crate::agentic_codec::CodecAdvertisement {
                id: "ad1".into(),
                agent_id: "agent_b".into(),
                codecs: vec![],
            }),
            Message::CodecNegotiation(crate::agentic_codec::CodecNegotiation {
                id: "neg1".into(),
                offered: vec!["spine:codec/echo/v1".into()],
                accepted: None,
                reason: None,
            }),
            Message::EmbeddingRequest(crate::agentic_codec::EmbeddingRequest {
                id: "e1".into(),
                input: crate::agentic_codec::EmbeddingInput::Text("hi".into()),
                codec: None,
                trace: None,
            }),
            Message::EmbeddingResponse(crate::agentic_codec::EmbeddingResponse {
                id: "e1".into(),
                codec: "spine:codec/echo/v1".into(),
                embeddings: vec![],
                trace: None,
            }),
        ];

        for msg in &cases {
            let bytes = serde_json::to_vec(msg).expect("encode");
            let back: Message = serde_json::from_slice(&bytes).expect("decode");
            let _ = back; // structural decode is enough — Message is not PartialEq.
        }
    }

    #[test]
    fn traceparent_sampled_flag() {
        let mut tc = TraceContext {
            trace_id: [1u8; 16],
            span_id: [2u8; 8],
            flags: 0x00,
            state: Vec::new(),
        };
        assert!(!tc.is_sampled());
        tc.flags = 0x01;
        assert!(tc.is_sampled());
        tc.flags = 0xFF;
        assert!(tc.is_sampled());
    }
}
