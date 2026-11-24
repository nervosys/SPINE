//! Agentic-first protocol primitives — the surface that makes SPINE more
//! than another binary RPC.
//!
//! Four families are defined here. Two of them are commands in a `REQUEST`
//! (see [`crate::BrowserCommand`]); the third is specified for the `STREAM_*`
//! frames; the fourth rides inside the others.
//!
//! 1. **Tool calling** ([`ToolCall`] / [`ToolOutcome`]) — MCP-shaped. The
//!    `tool_call` command is `{name, args[, trace]}`; the `RESPONSE` carries
//!    the tool's content as `{ok: content}` or a typed error as
//!    `{err: {code, message[, details]}}`. Requests are correlated by the
//!    frame header's request id, not by an id in the body, so several tool
//!    calls can be in flight on one connection. [`ToolOutcome`] is the Rust
//!    side of that reply, not a body.
//!
//! 2. **Token streaming** ([`StreamStart`] / [`StreamToken`] / [`StreamEnd`] /
//!    [`StreamCancel`]) — LLM tokens are the currency of modern agents. The
//!    stream's id is the id of the `REQUEST` that opened it, carried in the
//!    frame header of every `STREAM_*` frame; tokens carry a monotonically
//!    increasing `seq` so the receiver can detect gaps. `StreamEnd::reason`
//!    matches the OpenAI / Anthropic finish-reason taxonomy. Nothing in the
//!    workspace produces these bodies yet; the types and the codec
//!    ([`crate::native::decode_stream_body`]) exist so the frames have a
//!    definition.
//!
//! 3. **Capability negotiation** ([`CapabilityQuery`] /
//!    [`CapabilityAdvertisement`]) — agents announce what they can do. The
//!    `capability_query` command is `{selector}`; the advertisement is the
//!    `ok` value of its `RESPONSE`. Queries can be exact (`capability:foo`),
//!    prefix (`capability:foo/*`), or semantic (an embedding the responder is
//!    expected to match against its own capability embeddings). The
//!    advertisement carries one or more [`Capability`] descriptors with a
//!    stable schema so a planner can route by interface shape rather than by
//!    hard-coded URL.
//!
//! 4. **Distributed tracing** ([`TraceContext`]) — a W3C-compatible
//!    `traceparent` (with optional `tracestate`) attached to a tool call or a
//!    stream so a swarm's call graph can be reconstructed end-to-end. The
//!    format matches `traceparent: 00-<trace_id>-<span_id>-<flags>` so
//!    existing OpenTelemetry collectors can ingest SPINE traces with no
//!    translator.
//!
//! Every struct here is closed (an unknown field is an error) and every
//! optional field is omitted when absent and rejects a present `null`
//! (`docs/spec/00-wire.md`, C.5). Byte fields are CBOR byte strings. The types
//! are deliberately small and `Clone`-able so they survive being passed
//! through retry / replay layers.

use serde::{Deserialize, Serialize};

use crate::body::Reply;
use crate::wirefmt::{reject_null, reject_null_value};

/// Sampling parameters for an autoregressive decoder. Travels alongside
/// the request that asks a model to produce output. Every field is
/// optional so that the decoder's own defaults survive when the caller
/// doesn't care.
///
/// Field names match the OpenAI / Anthropic surface, so an existing
/// client SDK round-trips through the gateway without translation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DecodeHints {
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub temperature: Option<f32>,
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub top_p: Option<f32>,
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub top_k: Option<u32>,
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_tokens: Option<u32>,
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub repetition_penalty: Option<f32>,
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub presence_penalty: Option<f32>,
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub frequency_penalty: Option<f32>,
    /// Omitted when empty; a present empty list is rejected, so a value has
    /// one encoding.
    #[serde(
        default,
        deserialize_with = "crate::wirefmt::non_empty_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub stop_sequences: Vec<String>,
    /// Deterministic seed. Same seed + same inputs + same model →
    /// same outputs.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub seed: Option<u64>,
}

// =============================================================================
// Tool calling (MCP-shaped)
// =============================================================================

/// The body of the `tool_call` command. The caller correlates the reply with
/// the frame header's request id; there is no id here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    /// Tool identifier — typically a URI like `tool:fetch_url` or a
    /// capability path like `agent.web/fetch`. Lookup is performed
    /// against the responder's [`CapabilityAdvertisement`].
    pub name: String,
    /// Structured arguments. JSON-shaped so a tool's schema can be
    /// expressed in JSON Schema and validated independently. Encoded as CBOR
    /// through the same path as every body: integers stay integers and floats
    /// keep the shortest width that preserves them (C.5 rule 2).
    pub args: serde_json::Value,
    /// Optional W3C trace context for distributed tracing.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub trace: Option<TraceContext>,
}

/// Either a structured success payload or a typed error: the Rust form of a
/// `tool_call` reply. On the wire it is a [`Reply`]: `Ok { content }` is
/// `{ok: content}` and `Err { code, message, details }` is
/// `{err: {code, message[, details]}}`. The serde form of this type is not a
/// body; it is what a task record stores.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum ToolOutcome {
    /// Successful invocation. `content` is the tool's return value in the
    /// shape declared by the tool's schema.
    #[serde(rename = "ok")]
    Ok { content: serde_json::Value },
    /// Tool refused, errored, or timed out. `code` is a stable string
    /// identifier (e.g. `"not_authorized"`, `"timeout"`, `"invalid_args"`)
    /// so callers can branch without parsing the human-readable `message`.
    #[serde(rename = "err")]
    Err {
        code: String,
        message: String,
        #[serde(
            default,
            deserialize_with = "reject_null_value",
            skip_serializing_if = "Option::is_none"
        )]
        details: Option<serde_json::Value>,
    },
}

impl ToolOutcome {
    /// The `RESPONSE` body this outcome travels as.
    pub fn into_reply(self) -> Reply<serde_json::Value> {
        match self {
            ToolOutcome::Ok { content } => Reply::Ok(content),
            ToolOutcome::Err {
                code,
                message,
                details,
            } => Reply::Err(crate::body::WireError {
                code,
                message,
                details,
            }),
        }
    }

    /// The outcome a `RESPONSE` body stands for.
    pub fn from_reply(reply: Reply<serde_json::Value>) -> ToolOutcome {
        match reply {
            Reply::Ok(content) => ToolOutcome::Ok { content },
            Reply::Err(error) => ToolOutcome::Err {
                code: error.code,
                message: error.message,
                details: error.details,
            },
        }
    }

    /// True for `Ok`.
    pub fn is_ok(&self) -> bool {
        matches!(self, ToolOutcome::Ok { .. })
    }
}

// =============================================================================
// LLM token streaming
// =============================================================================

/// The body of `STREAM_START`. The receiver should expect zero or more
/// [`StreamToken`]s, terminated by a [`StreamEnd`]; all carry the request id
/// of the request that opened the stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StreamStart {
    /// Role of the producer (assistant, tool, user-echo, system).
    pub role: StreamRole,
    /// Model name / version (e.g. `claude-opus-4-7`). Free-form.
    pub model: String,
    /// Optional W3C trace context.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub trace: Option<TraceContext>,
    /// Optional sampling parameters for the decoder. When present, the
    /// producer is being asked to respect these for the duration of
    /// this stream.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub decode_hints: Option<DecodeHints>,
}

/// Producer role for a stream.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum StreamRole {
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "tool")]
    Tool,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "system")]
    System,
}

/// The body of `STREAM_TOKEN`: one chunk of stream output. `seq` starts at 0
/// and increments by one per chunk per stream; gaps indicate loss.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StreamToken {
    pub seq: u64,
    pub data: StreamData,
    /// Optional cumulative usage *as of this chunk*. Lets a consumer enforce a
    /// token budget mid-stream instead of waiting for [`StreamEnd::usage`] —
    /// the multiplexed analogue of OpenAI's `stream_options.include_usage`.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub usage: Option<StreamUsage>,
}

/// The body of `STREAM_CANCEL`: cooperatively cancel the stream whose id is
/// in the frame header.
///
/// SSE/HTTP clients cancel a completion by closing the socket. SPINE
/// multiplexes many streams over one connection, so closing the transport is
/// too blunt — this frame cancels exactly one stream. The producer should stop
/// emitting and send a final [`StreamEnd`] with
/// [`StreamEndReason::Cancelled`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct StreamCancel {
    /// Optional human-readable reason (e.g. `"user aborted"`), for logs/traces.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<String>,
}

/// What's in a token chunk. Most LLM streams are `Text`; multimodal
/// agents emit `Bytes` (audio frames, image patches, tool deltas).
/// The latent `Encoded` chunk and `StreamStart::stream_codec` belong to the
/// neural-codec extension (see `research/spine-protocol-research`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StreamData {
    /// UTF-8 text fragment.
    #[serde(rename = "text")]
    Text(String),
    /// Opaque bytes (e.g. audio frame, partial image). Encoded as a CBOR
    /// byte string on the wire (1-byte overhead), not an integer array.
    #[serde(rename = "bytes")]
    Bytes(#[serde(with = "crate::wirefmt::bytes")] Vec<u8>),
    /// Tool invocation embedded mid-stream (function calling).
    #[serde(rename = "tool_call")]
    ToolCall(ToolCall),
}

/// The body of `STREAM_END`: closes a token stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StreamEnd {
    pub reason: StreamEndReason,
    /// Optional usage statistics. Matches the OpenAI `usage` object.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub usage: Option<StreamUsage>,
}

/// Why a stream ended. Mirrors OpenAI / Anthropic finish reasons so
/// existing client code can switch on the same set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub enum StreamEndReason {
    /// Model emitted its end-of-turn token.
    #[serde(rename = "stop")]
    Stop,
    /// `max_tokens` reached.
    #[serde(rename = "length")]
    Length,
    /// Stopped to invoke a tool.
    #[serde(rename = "tool_use")]
    ToolUse,
    /// Content filter or policy block.
    #[serde(rename = "content_filter")]
    ContentFilter,
    /// Cancelled by the client.
    #[serde(rename = "cancelled")]
    Cancelled,
    /// Producer errored mid-stream.
    #[serde(rename = "error")]
    Error { code: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StreamUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

// =============================================================================
// Capability negotiation
// =============================================================================

/// The body of the `capability_query` command: `{selector}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityQuery {
    pub selector: CapabilitySelector,
}

/// Three ways to specify what we're looking for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum CapabilitySelector {
    /// Exact capability URI match (e.g. `spine:cap/web/scraping`).
    #[serde(rename = "exact")]
    Exact(String),
    /// Prefix match (e.g. `spine:cap/web/*`).
    #[serde(rename = "prefix")]
    Prefix(String),
    /// Semantic match — responder is expected to compare against its
    /// own capability embeddings and return the top-K matches.
    #[serde(rename = "semantic")]
    Semantic { embedding: Vec<f32>, top_k: u32 },
    /// Return everything.
    #[serde(rename = "all")]
    All,
}

/// The `ok` value of a `capability_query` reply: `{agent_id, capabilities}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityAdvertisement {
    /// Cryptographic identity of the advertising agent (e.g. an Ed25519
    /// public key fingerprint or a DID).
    pub agent_id: String,
    pub capabilities: Vec<Capability>,
}

/// One capability the agent offers. The schema is intentionally small —
/// rich types (cost, latency, SLAs) belong in higher-layer marketplaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    /// Stable URI for this capability.
    pub uri: String,
    /// Human-readable description.
    pub description: String,
    /// JSON-Schema-shaped argument descriptor — what `ToolCall::args`
    /// must look like to invoke this capability.
    pub input_schema: serde_json::Value,
    /// JSON-Schema-shaped result descriptor — what the `ok` value of a tool
    /// reply will look like on success.
    pub output_schema: serde_json::Value,
    /// Optional semantic embedding for use with
    /// [`CapabilitySelector::Semantic`]. Length is provider-chosen.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub embedding: Option<Vec<f32>>,
}

// =============================================================================
// Distributed tracing (W3C-compatible)
// =============================================================================

/// W3C Trace Context. The `traceparent` header form is:
/// `00-<trace_id:32hex>-<span_id:16hex>-<flags:2hex>`.
///
/// Carried inline on [`ToolCall`] and [`StreamStart`]. Existing OpenTelemetry
/// collectors can ingest this format with no translator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TraceContext {
    /// 128-bit trace id, a CBOR byte string of 16 bytes.
    #[serde(with = "crate::wirefmt::byte_array")]
    pub trace_id: [u8; 16],
    /// 64-bit span id of the producer, a CBOR byte string of 8 bytes.
    #[serde(with = "crate::wirefmt::byte_array")]
    pub span_id: [u8; 8],
    /// W3C flags byte (bit 0 = sampled).
    pub flags: u8,
    /// Optional `tracestate` key-value list (small, opaque). Omitted when
    /// empty; a present empty list is rejected.
    #[serde(
        default,
        deserialize_with = "crate::wirefmt::non_empty_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
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
    use crate::cbor;
    use serde_json::json;

    /// Every type must survive CBOR, the encoding of the native wire:
    /// `from_slice(to_vec(x)) == x` and `to_vec(from_slice(b)) == b`.
    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: &T) {
        let bytes = cbor::to_vec(v).expect("encode");
        let back: T = cbor::from_slice(&bytes).expect("decode");
        assert_eq!(v, &back);
        assert_eq!(cbor::to_vec(&back).expect("re-encode"), bytes);
        // JSON (trace files, the gateway) must work too.
        let text = serde_json::to_vec(v).expect("json encode");
        let back: T = serde_json::from_slice(&text).expect("json decode");
        assert_eq!(v, &back);
    }

    fn trace() -> TraceContext {
        TraceContext {
            trace_id: [7; 16],
            span_id: [9; 8],
            flags: 1,
            state: vec![("k".to_string(), "v".to_string())],
        }
    }

    #[test]
    fn tool_call_round_trip() {
        let call = ToolCall {
            name: "fetch_url".into(),
            args: json!({"url": "https://example.com"}),
            trace: None,
        };
        round_trip(&call);
        let traced = ToolCall {
            trace: Some(trace()),
            ..call
        };
        round_trip(&traced);
    }

    #[test]
    fn tool_outcome_round_trip_and_reply_conversion() {
        let ok = ToolOutcome::Ok {
            content: json!({"status": 200, "body": "<html/>"}),
        };
        round_trip(&ok);
        let err = ToolOutcome::Err {
            code: "timeout".into(),
            message: "tool exceeded 30s budget".into(),
            details: Some(json!({"elapsed_ms": 30_100})),
        };
        round_trip(&err);
        for outcome in [ok, err] {
            let reply = outcome.clone().into_reply();
            assert_eq!(ToolOutcome::from_reply(reply), outcome);
        }
    }

    #[test]
    fn stream_lifecycle_round_trip() {
        let start = StreamStart {
            role: StreamRole::Assistant,
            model: "claude-opus-4-7".into(),
            trace: None,
            decode_hints: None,
        };
        round_trip(&start);

        let tok = StreamToken {
            seq: 7,
            data: StreamData::Text("hello ".into()),
            usage: None,
        };
        round_trip(&tok);
        round_trip(&StreamToken {
            seq: 8,
            data: StreamData::Bytes(vec![0, 1, 2, 255]),
            usage: Some(StreamUsage {
                input_tokens: 1,
                output_tokens: 2,
            }),
        });

        let end = StreamEnd {
            reason: StreamEndReason::Stop,
            usage: Some(StreamUsage {
                input_tokens: 42,
                output_tokens: 1337,
            }),
        };
        round_trip(&end);
        round_trip(&StreamEnd {
            reason: StreamEndReason::Error {
                code: "x".into(),
                message: "y".into(),
            },
            usage: None,
        });
        round_trip(&StreamCancel { reason: None });
        round_trip(&StreamCancel {
            reason: Some("user aborted".into()),
        });
    }

    /// `decode_hints` rides along on `StreamStart`.
    #[test]
    fn stream_start_with_decode_hints() {
        let start = StreamStart {
            role: StreamRole::Assistant,
            model: "claude-opus-4-7".into(),
            trace: Some(trace()),
            decode_hints: Some(DecodeHints {
                temperature: Some(0.5),
                top_p: Some(0.875),
                max_tokens: Some(512),
                ..Default::default()
            }),
        };
        round_trip(&start);
    }

    #[test]
    fn decode_hints_partial_round_trip() {
        let h = DecodeHints {
            temperature: Some(0.5),
            top_p: Some(0.25),
            max_tokens: Some(2048),
            stop_sequences: vec!["</tool>".into(), "USER:".into()],
            seed: Some(42),
            ..Default::default()
        };
        round_trip(&h);
        // The empty case must be tiny on the wire: every Option is skipped.
        let bytes = serde_json::to_vec(&DecodeHints::default()).unwrap();
        assert!(bytes.len() <= 4, "empty hints encoded as {bytes:?}");
        // An empty list is not accepted, so a value has one encoding.
        let body = cbor::to_vec(&json!({"stop_sequences": []})).unwrap();
        assert!(cbor::from_slice::<DecodeHints>(&body).is_err());
    }

    #[test]
    fn stream_token_with_tool_call() {
        // Function-calling mid-stream.
        let tok = StreamToken {
            seq: 12,
            data: StreamData::ToolCall(ToolCall {
                name: "search".into(),
                args: json!({"q": "rust async"}),
                trace: None,
            }),
            usage: None,
        };
        round_trip(&tok);
    }

    #[test]
    fn capability_selector_round_trip() {
        round_trip(&CapabilitySelector::Exact("spine:cap/web/scraping".into()));
        round_trip(&CapabilitySelector::Prefix("spine:cap/web/*".into()));
        round_trip(&CapabilitySelector::All);
        round_trip(&CapabilitySelector::Semantic {
            embedding: vec![0.5, 0.25, 0.125, -0.5],
            top_k: 5,
        });
        round_trip(&CapabilityQuery {
            selector: CapabilitySelector::All,
        });
    }

    #[test]
    fn capability_advertisement_round_trip() {
        let ad = CapabilityAdvertisement {
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
                0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d, 0x0e, 0x0e,
                0x47, 0x36,
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
    fn trace_ids_are_byte_strings() {
        let body = cbor::to_vec(&trace()).unwrap();
        // flags: 1, then trace_id as a 16-byte string (0x50 ...).
        assert!(body.windows(2).any(|w| w == [0x50, 7]));
        assert!(body.windows(2).any(|w| w == [0x48, 9]));
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

    /// The stream bodies survive being held in the recording [`crate::Message`].
    #[test]
    fn message_holds_every_body_kind() {
        use crate::{BrowserCommand, Message, Reply, WireError};

        let cases: Vec<Message> = vec![
            Message::Command(BrowserCommand::ToolCall(ToolCall {
                name: "tool".into(),
                args: json!(null),
                trace: None,
            })),
            Message::Command(BrowserCommand::CapabilityQuery(CapabilityQuery {
                selector: CapabilitySelector::All,
            })),
            Message::Reply(Reply::Ok(json!(42))),
            Message::Reply(Reply::Err(WireError::new("x", "y"))),
            Message::StreamStart(StreamStart {
                role: StreamRole::Assistant,
                model: "claude-opus-4-7".into(),
                trace: None,
                decode_hints: None,
            }),
            Message::StreamToken(StreamToken {
                seq: 0,
                data: StreamData::Text("hi".into()),
                usage: None,
            }),
            Message::StreamEnd(StreamEnd {
                reason: StreamEndReason::Stop,
                usage: None,
            }),
            Message::StreamCancel(StreamCancel { reason: None }),
            Message::Ping { timestamp: 1 },
            Message::Pong { timestamp: 2 },
        ];

        for msg in &cases {
            round_trip(msg);
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
