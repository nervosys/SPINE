//! How the current [`Message`] set rides the spine-wire v1 frame
//! (`docs/spec/00-wire.md`, sections C and F step 10).
//!
//! This is the one place that maps a [`Message`] onto frames, so the daemon
//! and the SDK cannot disagree. Bodies are still the current serde types,
//! encoded with [`cbor::to_vec`] and decoded with [`cbor::from_slice`]; the
//! body redesign (ids moved into the header, error codes, one command enum)
//! is a later step.
//!
//! # Mapping
//!
//! | Direction | [`Message`] variant | Frame |
//! | --- | --- | --- |
//! | client to server | `Request`, `ToolCall`, `CapabilityQuery` | `REQUEST`, body is the CBOR of the whole `Message` |
//! | server to client | `Response`, `ToolResult`, `CapabilityAd` | `RESPONSE` with the request's id, body is the CBOR of the whole `Message` |
//! | either | `Ping`, `Pong` | not sent as messages: use the `PING` and `PONG` frames (the token is an unsigned integer; the SDK uses its millisecond timestamp) |
//! | server to client | `StreamStart`, `StreamToken`, `StreamEnd` | not wired: nothing in the workspace produces them |
//! | client to server | `StreamCancel` | not wired, for the same reason |
//!
//! Any other variant in a `REQUEST` or `RESPONSE` body is a refusal
//! ([`NativeError::Unexpected`]). The ids that sit inside the bodies
//! (`Request.id`, `ToolCall.id`, ...) stay as they were and are **not**
//! cross-checked against the frame-header request id in this step: the header
//! id correlates, the body id is carried and ignored.
//!
//! A server that cannot serve a well-formed `REQUEST` answers with a `RESPONSE`
//! whose body is `Message::Response` with `error` set (see [`error_reply`]).
//! [`Connection`] itself answers an over-limit request with the spec's
//! `{err: {code, message}}` body; [`decode_reply`] turns that into the same
//! `Message::Response` shape so a caller handles one kind of refusal.

use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::cbor::{self, CborError};
use crate::connection::{Connection, ConnectionError};
use crate::frame::{CloseCode, Frame, FrameType, VERSION};
use crate::hello::{Hello, LocalConfig};
use crate::{Message, Response};

/// How long the daemon waits for `HELLO` and the SDK for `HELLO_ACK`, unless
/// configured otherwise (C.6).
pub const DEFAULT_HELLO_DEADLINE: Duration = Duration::from_secs(10);

/// How long one frame may take to write before the connection is given up.
/// The pre-v1 transport had the same 30-second deadline.
pub const WRITE_DEADLINE: Duration = Duration::from_secs(30);

/// Why a native exchange failed.
#[derive(Debug, thiserror::Error)]
pub enum NativeError {
    /// The connection layer failed or the peer closed it.
    #[error(transparent)]
    Connection(#[from] ConnectionError),
    /// A body could not be encoded or decoded.
    #[error("body error: {0}")]
    Body(#[from] CborError),
    /// A frame did not finish writing within [`WRITE_DEADLINE`]. The
    /// connection has been closed.
    #[error("write deadline exceeded")]
    WriteTimeout,
    /// The body decoded, but the message is not one this direction carries.
    #[error("unexpected message: {0}")]
    Unexpected(String),
    /// The stream ended before the reply arrived.
    #[error("the connection ended before a reply arrived")]
    Eof,
}

/// The server's limits for the daemon: version 1, the 16 MiB maximum frame,
/// one request at a time, no extensions.
pub fn local_config() -> LocalConfig {
    LocalConfig {
        versions: vec![u64::from(VERSION)],
        max_frame: crate::frame::MAX_FRAME,
        max_inflight: 1,
        extensions: Vec::new(),
        agent: None,
    }
}

/// The `HELLO` an SDK client sends: version 1, the 16 MiB maximum frame, no
/// extensions.
pub fn client_hello() -> Hello {
    Hello {
        versions: vec![u64::from(VERSION)],
        max_frame: crate::frame::MAX_FRAME,
        extensions: Vec::new(),
        agent: Some(format!("spine-protocol/{}", env!("CARGO_PKG_VERSION"))),
    }
}

/// Client side of the handshake over `stream`, with the default deadline.
/// A `CLOSE` from the server in place of `HELLO_ACK` is reported with its
/// code and message.
pub async fn client_connect<S>(stream: S) -> anyhow::Result<Connection<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match Connection::client_handshake(stream, client_hello(), DEFAULT_HELLO_DEADLINE).await {
        Ok(conn) => Ok(conn),
        Err(ConnectionError::Closed(close)) => Err(anyhow::anyhow!(
            "server closed the connection during the handshake: {}: {}",
            close.code,
            close.message
        )),
        Err(ConnectionError::Timeout) => Err(anyhow::anyhow!(
            "handshake deadline exceeded (no HELLO_ACK)"
        )),
        Err(other) => Err(anyhow::anyhow!("handshake failed: {other}")),
    }
}

/// True for the variants a client sends in a `REQUEST` body.
pub fn is_request(message: &Message) -> bool {
    matches!(
        message,
        Message::Request(_) | Message::ToolCall(_) | Message::CapabilityQuery(_)
    )
}

/// True for the variants a server sends in a `RESPONSE` body.
pub fn is_reply(message: &Message) -> bool {
    matches!(
        message,
        Message::Response(_) | Message::ToolResult(_) | Message::CapabilityAd(_)
    )
}

/// The `REQUEST` body for `message`.
pub fn encode_request(message: &Message) -> Result<Vec<u8>, NativeError> {
    if !is_request(message) {
        return Err(NativeError::Unexpected(
            "only Request, ToolCall and CapabilityQuery travel in a REQUEST".to_string(),
        ));
    }
    Ok(cbor::to_vec(message)?)
}

/// The message in a `REQUEST` body. A body that is not a [`Message`] is
/// [`NativeError::Body`]; a message of another kind is
/// [`NativeError::Unexpected`].
pub fn decode_request(body: &[u8]) -> Result<Message, NativeError> {
    let message: Message = cbor::from_slice(body)?;
    if is_request(&message) {
        Ok(message)
    } else {
        Err(NativeError::Unexpected(
            "a REQUEST carries a Request, ToolCall or CapabilityQuery".to_string(),
        ))
    }
}

/// The `RESPONSE` body for `message`.
pub fn encode_reply(message: &Message) -> Result<Vec<u8>, NativeError> {
    if !is_reply(message) {
        return Err(NativeError::Unexpected(
            "only Response, ToolResult and CapabilityAd travel in a RESPONSE".to_string(),
        ));
    }
    Ok(cbor::to_vec(message)?)
}

/// The `{err: {code, message}}` envelope [`Connection`] writes when it
/// refuses a request itself (for example `overloaded`).
#[derive(Deserialize)]
struct Refusal {
    err: RefusalDetail,
}

#[derive(Deserialize)]
struct RefusalDetail {
    code: String,
    message: String,
}

/// A `Message::Response` that carries only an error text. The body id is
/// empty: the frame id is what correlates.
pub fn error_reply(text: impl Into<String>) -> Message {
    Message::Response(Response {
        id: String::new(),
        result: None,
        error: Some(text.into()),
    })
}

/// The message in a `RESPONSE` body: a reply [`Message`], or the spec's
/// `{err: {code, message}}` refusal turned into [`error_reply`] with the text
/// `code: message`.
pub fn decode_reply(body: &[u8]) -> Result<Message, NativeError> {
    match cbor::from_slice::<Message>(body) {
        Ok(message) if is_reply(&message) => Ok(message),
        Ok(_) => Err(NativeError::Unexpected(
            "a RESPONSE carries a Response, ToolResult or CapabilityAd".to_string(),
        )),
        Err(message_error) => match cbor::from_slice::<Refusal>(body) {
            Ok(refusal) => Ok(error_reply(format!(
                "{}: {}",
                refusal.err.code, refusal.err.message
            ))),
            Err(_) => Err(NativeError::Body(message_error)),
        },
    }
}

/// The body of a `PING` or `PONG`: the token as a CBOR unsigned integer.
pub fn ping_body(token: u64) -> Result<Vec<u8>, NativeError> {
    Ok(cbor::to_vec(&token)?)
}

/// The token in a `PING` or `PONG` body.
pub fn ping_token(body: &[u8]) -> Result<u64, NativeError> {
    Ok(cbor::from_slice(body)?)
}

/// Sends `frame`, giving up after [`WRITE_DEADLINE`]. On expiry the
/// connection is closed (`timeout`) and cannot be used again.
pub async fn send_bounded<S>(conn: &mut Connection<S>, frame: Frame) -> Result<(), NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match tokio::time::timeout(WRITE_DEADLINE, conn.send(frame)).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            conn.close(CloseCode::Timeout, "write deadline exceeded")
                .await;
            Err(NativeError::WriteTimeout)
        }
    }
}

/// Client: sends `message` as a `REQUEST` and returns the frame id.
pub async fn send_request<S>(
    conn: &mut Connection<S>,
    message: &Message,
) -> Result<u64, NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let body = encode_request(message)?;
    match tokio::time::timeout(WRITE_DEADLINE, conn.request(body)).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            conn.close(CloseCode::Timeout, "write deadline exceeded")
                .await;
            Err(NativeError::WriteTimeout)
        }
    }
}

/// Server: sends `message` as the `RESPONSE` to request `request_id`.
pub async fn send_reply<S>(
    conn: &mut Connection<S>,
    request_id: u64,
    message: &Message,
) -> Result<(), NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let body = encode_reply(message)?;
    send_bounded(
        conn,
        Frame {
            frame_type: FrameType::Response,
            request_id,
            body,
        },
    )
    .await
}

/// Client: reads frames until the `RESPONSE` for `request_id` arrives and
/// returns its message. Frames that do not match (a late reply to an earlier,
/// abandoned request; a `PONG`) are skipped. The caller bounds the wait with
/// its own timeout and drops the connection if that fires, because
/// [`Connection::recv`] is not cancel safe.
pub async fn await_reply<S>(
    conn: &mut Connection<S>,
    request_id: u64,
) -> Result<Message, NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        match conn.recv().await? {
            None => return Err(NativeError::Eof),
            Some(frame)
                if frame.frame_type == FrameType::Response && frame.request_id == request_id =>
            {
                return decode_reply(&frame.body);
            }
            Some(_) => {}
        }
    }
}

/// Client: one request, one reply.
pub async fn call<S>(conn: &mut Connection<S>, message: &Message) -> Result<Message, NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let id = send_request(conn, message).await?;
    await_reply(conn, id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BrowserCommand, CapabilityAdvertisement, CapabilityQuery, CapabilitySelector, Request,
        SpineBinary, StreamCancel, ToolCall, ToolOutcome, ToolResult, TraceContext,
    };
    use serde_json::json;
    use tokio::io::duplex;

    const LONG: Duration = Duration::from_secs(10);

    fn trace() -> TraceContext {
        TraceContext {
            trace_id: [7; 16],
            span_id: [9; 8],
            flags: 1,
            state: vec![("k".to_string(), "v".to_string())],
        }
    }

    fn json_values() -> serde_json::Value {
        json!({
            "text": "hello",
            "int": -5,
            "big": u64::MAX,
            "float": 0.1,
            "whole_float": 1.0,
            "null": null,
            "list": [1, 2.5, "x", true, null, {"a": {"b": []}}],
            "b": 1,
            "aa": 2
        })
    }

    /// Every request-direction variant, with the awkward field kinds: JSON
    /// values with floats and 64-bit integers, a `HashMap`, byte arrays.
    fn requests() -> Vec<Message> {
        let mut functions = std::collections::HashMap::new();
        functions.insert("main".to_string(), 3usize);
        functions.insert("a".to_string(), 0usize);
        functions.insert("zz".to_string(), 9usize);
        vec![
            Message::Request(Request {
                id: "1".to_string(),
                command: BrowserCommand::GetUR,
            }),
            Message::Request(Request {
                id: "2".to_string(),
                command: BrowserCommand::Navigate {
                    url: "https://example.com/".to_string(),
                },
            }),
            Message::Request(Request {
                id: "3".to_string(),
                command: BrowserCommand::ExecuteBinary(SpineBinary {
                    instructions: vec![
                        crate::Instruction::Push(json_values()),
                        crate::Instruction::Jump(4),
                        crate::Instruction::Pop,
                    ],
                    data: vec![0, 1, 2, 255],
                    render_start: 1,
                    exported_functions: functions,
                    capabilities: vec!["navigate".to_string()],
                }),
            }),
            Message::Request(Request {
                id: "4".to_string(),
                command: BrowserCommand::StoreKnowledge {
                    key: "k".to_string(),
                    value: json_values(),
                    tags: vec!["t".to_string()],
                },
            }),
            Message::Request(Request {
                id: "5".to_string(),
                command: BrowserCommand::ReadResource {
                    hash: [3; 32],
                    offset: 0,
                    max_bytes: 10,
                },
            }),
            Message::ToolCall(ToolCall {
                id: "t1".to_string(),
                name: "spine.content/hash".to_string(),
                args: json_values(),
                trace: Some(trace()),
            }),
            Message::CapabilityQuery(CapabilityQuery {
                id: "c1".to_string(),
                selector: CapabilitySelector::All,
            }),
            Message::CapabilityQuery(CapabilityQuery {
                id: "c2".to_string(),
                selector: CapabilitySelector::Semantic {
                    embedding: vec![0.25, -1.5, 0.1],
                    top_k: 3,
                },
            }),
        ]
    }

    fn replies() -> Vec<Message> {
        vec![
            Message::Response(Response {
                id: "1".to_string(),
                result: Some(json_values()),
                error: None,
            }),
            Message::Response(Response {
                id: "2".to_string(),
                result: None,
                error: Some("nope".to_string()),
            }),
            Message::ToolResult(ToolResult {
                id: "t1".to_string(),
                trace: Some(trace()),
                outcome: ToolOutcome::Err {
                    code: "x".to_string(),
                    message: "y".to_string(),
                    details: Some(json_values()),
                },
            }),
            Message::CapabilityAd(CapabilityAdvertisement {
                id: "c1".to_string(),
                agent_id: "a".to_string(),
                capabilities: vec![crate::Capability {
                    uri: "u".to_string(),
                    description: "d".to_string(),
                    input_schema: json_values(),
                    output_schema: json!({}),
                    embedding: Some(vec![0.5, 0.1]),
                }],
            }),
        ]
    }

    /// The decoded message re-encodes to the same bytes.
    fn same(a: &Message, b: &Message) -> bool {
        cbor::to_vec(a).unwrap() == cbor::to_vec(b).unwrap()
    }

    #[test]
    fn every_request_variant_survives_the_body_codec() {
        for message in requests() {
            let body = encode_request(&message).unwrap();
            let back = decode_request(&body).unwrap();
            assert!(same(&message, &back), "{message:?}");
        }
    }

    #[test]
    fn every_reply_variant_survives_the_body_codec() {
        for message in replies() {
            let body = encode_reply(&message).unwrap();
            let back = decode_reply(&body).unwrap();
            assert!(same(&message, &back), "{message:?}");
        }
    }

    #[test]
    fn json_values_keep_their_shape_through_cbor() {
        let call = ToolCall {
            id: "x".to_string(),
            name: "n".to_string(),
            args: json_values(),
            trace: None,
        };
        let body = encode_request(&Message::ToolCall(call.clone())).unwrap();
        match decode_request(&body).unwrap() {
            Message::ToolCall(back) => assert_eq!(back, call),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn variants_that_do_not_belong_are_refused_both_ways() {
        let ping = Message::Ping { timestamp: 1 };
        assert!(matches!(
            encode_request(&ping),
            Err(NativeError::Unexpected(_))
        ));
        assert!(matches!(
            encode_reply(&ping),
            Err(NativeError::Unexpected(_))
        ));
        assert!(matches!(
            encode_request(&replies()[0]),
            Err(NativeError::Unexpected(_))
        ));
        assert!(matches!(
            encode_reply(&requests()[0]),
            Err(NativeError::Unexpected(_))
        ));
        let cancel = Message::StreamCancel(StreamCancel {
            id: "s".to_string(),
            reason: None,
        });
        assert!(matches!(
            encode_request(&cancel),
            Err(NativeError::Unexpected(_))
        ));
        // A well-formed message of the wrong kind inside a body.
        let body = cbor::to_vec(&ping).unwrap();
        assert!(matches!(
            decode_request(&body),
            Err(NativeError::Unexpected(_))
        ));
        assert!(matches!(
            decode_reply(&body),
            Err(NativeError::Unexpected(_))
        ));
    }

    #[test]
    fn a_body_that_is_not_a_message_is_a_body_error() {
        let body = cbor::to_vec(&json!({"bogus": 1})).unwrap();
        assert!(matches!(decode_request(&body), Err(NativeError::Body(_))));
        assert!(matches!(decode_reply(&body), Err(NativeError::Body(_))));
        assert!(matches!(
            decode_request(&[0x9f, 0xff]),
            Err(NativeError::Body(_))
        ));
    }

    #[test]
    fn the_connection_refusal_envelope_becomes_an_error_reply() {
        let body = crate::connection::response_err_body("overloaded", "too many").unwrap();
        match decode_reply(&body).unwrap() {
            Message::Response(r) => {
                assert!(r.result.is_none());
                assert_eq!(r.error.as_deref(), Some("overloaded: too many"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ping_tokens_round_trip() {
        for token in [0u64, 23, 24, 1_700_000_000_000, u64::MAX] {
            let body = ping_body(token).unwrap();
            assert_eq!(ping_token(&body).unwrap(), token);
        }
        assert!(ping_token(&cbor::to_vec(&"x").unwrap()).is_err());
    }

    #[test]
    fn defaults_are_version_one_sequential_and_without_extensions() {
        let local = local_config();
        assert_eq!(local.versions, vec![1]);
        assert_eq!(local.max_frame, 16 * 1024 * 1024);
        assert_eq!(local.max_inflight, 1);
        assert!(local.extensions.is_empty());
        let hello = client_hello();
        assert!(hello.validate().is_ok());
        assert_eq!(hello.versions, vec![1]);
        assert!(hello.extensions.is_empty());
    }

    #[tokio::test]
    async fn a_request_gets_its_reply_over_a_connection() {
        let (c, s) = duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let mut conn = Connection::server_handshake(s, &local_config(), DEFAULT_HELLO_DEADLINE)
                .await
                .unwrap();
            let frame = conn.recv().await.unwrap().unwrap();
            assert_eq!(frame.frame_type, FrameType::Request);
            let message = decode_request(&frame.body).unwrap();
            let reply = match message {
                Message::Request(r) => Message::Response(Response {
                    id: r.id,
                    result: Some(json!({"ok": true})),
                    error: None,
                }),
                other => panic!("{other:?}"),
            };
            send_reply(&mut conn, frame.request_id, &reply)
                .await
                .unwrap();
            // Wait for the client to hang up.
            assert!(conn.recv().await.unwrap().is_none());
        });
        let mut conn = tokio::time::timeout(LONG, client_connect(c))
            .await
            .unwrap()
            .unwrap();
        let request = Message::Request(Request {
            id: "9".to_string(),
            command: BrowserCommand::GetRawHTML,
        });
        let reply = tokio::time::timeout(LONG, call(&mut conn, &request))
            .await
            .unwrap()
            .unwrap();
        match reply {
            Message::Response(r) => {
                assert_eq!(r.id, "9");
                assert_eq!(r.result, Some(json!({"ok": true})));
            }
            other => panic!("{other:?}"),
        }
        drop(conn);
        tokio::time::timeout(LONG, server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn the_client_reports_a_close_in_place_of_the_ack() {
        let (c, mut s) = duplex(64 * 1024);
        let server = tokio::spawn(async move {
            // Read the HELLO, then refuse with a CLOSE.
            let hello = crate::frame::read_frame(&mut s, crate::frame::PRE_HELLO_MAX_FRAME)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(hello.frame_type, FrameType::Hello);
            let close = crate::hello::Close::new(CloseCode::ShuttingDown, "going away");
            let frame = close.to_frame().unwrap();
            crate::frame::write_frame(&mut s, &frame, crate::frame::PRE_HELLO_MAX_FRAME, false)
                .await
                .unwrap();
        });
        let error = tokio::time::timeout(LONG, client_connect(c))
            .await
            .unwrap()
            .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("shutting_down"), "{text}");
        assert!(text.contains("going away"), "{text}");
        tokio::time::timeout(LONG, server).await.unwrap().unwrap();
    }
}
