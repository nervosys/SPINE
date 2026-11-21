//! How the v1 bodies ride the spine-wire frames (`docs/spec/00-wire.md`,
//! sections C and D).
//!
//! This is the one place that maps bodies onto frames, so the daemon and the
//! SDK cannot disagree. Bodies are encoded with [`cbor::to_vec`] and decoded
//! with [`cbor::from_slice`].
//!
//! # Mapping
//!
//! | Frame | Direction | Body |
//! | --- | --- | --- |
//! | `REQUEST` | client to server | a [`BrowserCommand`]: a single-entry map `{name: fields}` or the text of a unit command |
//! | `RESPONSE` | server to client, with the request's id | `{ok: result}` or `{err: {code, message[, details]}}`, a [`Reply`] |
//! | `PING`, `PONG` | either | an unsigned-integer token (the SDK uses its millisecond timestamp) |
//! | `STREAM_START`, `STREAM_TOKEN`, `STREAM_END`, `STREAM_CANCEL` | server to client, `STREAM_CANCEL` the reverse | the stream structs of [`crate::agentic`]; decoded by [`decode_stream_body`], produced by nothing yet |
//!
//! The frame header's request id is the only correlation id: no body carries
//! one.
//!
//! # Errors
//!
//! A server that cannot serve a well-formed `REQUEST` answers with `err`.
//! [`decode_command`] returns the code for a body that does not decode:
//! `unknown_command` when it names a command outside the v1 set,
//! `invalid_request` for any other mismatch (C.7). The client side turns an
//! `err` into [`NativeError::Remote`], which keeps the code and message;
//! `anyhow` callers recover them with [`remote_error`].

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::body::{Reply, WireError, ERR_INVALID_REQUEST, ERR_UNKNOWN_COMMAND};
use crate::cbor::{self, CborError};
use crate::connection::{Connection, ConnectionError};
use crate::frame::{Frame, FrameType, VERSION};
use crate::hello::{Hello, LocalConfig};
use crate::{BrowserCommand, Message, COMMAND_NAMES};

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
    /// A frame did not finish writing within [`WRITE_DEADLINE`]. The stream
    /// may hold a partial frame, so nothing more is written: the connection
    /// is finished and the caller drops it.
    #[error("write deadline exceeded")]
    WriteTimeout,
    /// The body decoded, but it is not one this frame type carries.
    #[error("unexpected message: {0}")]
    Unexpected(String),
    /// The stream ended before the reply arrived.
    #[error("the connection ended before a reply arrived")]
    Eof,
    /// The peer answered with `{err: {code, message[, details]}}`. Branch on
    /// `code`; `message` is for people.
    #[error("{code}: {message}")]
    Remote {
        code: String,
        message: String,
        details: Option<serde_json::Value>,
    },
}

impl From<WireError> for NativeError {
    fn from(error: WireError) -> NativeError {
        NativeError::Remote {
            code: error.code,
            message: error.message,
            details: error.details,
        }
    }
}

/// The remote error inside an [`anyhow::Error`], when that is what it holds:
/// `(code, message)`. Context added with `.context(..)` does not hide it.
pub fn remote_error(error: &anyhow::Error) -> Option<(&str, &str)> {
    match error.downcast_ref::<NativeError>() {
        Some(NativeError::Remote { code, message, .. }) => Some((code.as_str(), message.as_str())),
        _ => None,
    }
}

/// The remote error code inside an [`anyhow::Error`], if any.
pub fn remote_code(error: &anyhow::Error) -> Option<&str> {
    remote_error(error).map(|(code, _)| code)
}

/// A `REQUEST` body that does not decode: the `err` to answer with.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct RequestError {
    /// [`ERR_UNKNOWN_COMMAND`] or [`ERR_INVALID_REQUEST`].
    pub code: &'static str,
    /// Human-readable detail.
    pub message: String,
}

impl RequestError {
    /// The `err` body for this failure.
    pub fn to_wire(&self) -> WireError {
        WireError::new(self.code, self.message.clone())
    }
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

/// Decodes `body` into a `T` and requires that `T` encodes back to exactly
/// `body`.
///
/// [`cbor::from_slice`] checks that the bytes are deterministic CBOR, but
/// ciborium can still read a value from bytes that are not the canonical
/// encoding of that value: for example `{"get_ur": {"x": 1}}` decodes as the
/// unit command `get_ur` and the payload is skipped. This check closes that
/// gap, so every body that is accepted has one encoding per value (C.5, R4).
pub fn decode_strict<T: Serialize + DeserializeOwned>(body: &[u8]) -> Result<T, CborError> {
    let value: T = cbor::from_slice(body)?;
    if cbor::to_vec(&value)? != body {
        return Err(CborError::Decode(
            "the body is not the canonical encoding of its value".to_string(),
        ));
    }
    Ok(value)
}

/// The `REQUEST` body for `command`.
pub fn encode_command(command: &BrowserCommand) -> Result<Vec<u8>, NativeError> {
    Ok(cbor::to_vec(command)?)
}

/// The command in a `REQUEST` body.
///
/// On failure the error says which answer C.7 prescribes. The body is
/// inspected once more, as a generic CBOR item, to tell the two cases apart:
/// a text string, or a single-entry map whose key is text, that names a
/// command outside [`COMMAND_NAMES`] is `unknown_command`; everything else
/// that fails (a missing, unknown or mistyped field, a name that is known but
/// has the wrong shape, a body that is not a command at all) is
/// `invalid_request`.
pub fn decode_command(body: &[u8]) -> Result<BrowserCommand, RequestError> {
    match decode_strict::<BrowserCommand>(body) {
        Ok(command) => Ok(command),
        Err(error) => Err(classify_request_error(body, &error)),
    }
}

/// The command name a body claims, read without interpreting its fields.
fn claimed_command_name(body: &[u8]) -> Option<String> {
    match cbor::from_slice::<ciborium::Value>(body).ok()? {
        ciborium::Value::Text(name) => Some(name),
        ciborium::Value::Map(entries) if entries.len() == 1 => match entries.into_iter().next() {
            Some((ciborium::Value::Text(name), _)) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

fn classify_request_error(body: &[u8], error: &CborError) -> RequestError {
    match claimed_command_name(body) {
        Some(name) if !COMMAND_NAMES.contains(&name.as_str()) => {
            let shown: String = name.chars().take(64).collect();
            RequestError {
                code: ERR_UNKNOWN_COMMAND,
                message: format!("unknown command {shown:?}"),
            }
        }
        _ => RequestError {
            code: ERR_INVALID_REQUEST,
            message: format!("the request does not match its command: {error}"),
        },
    }
}

/// The `{err: {code, message[, details]}}` body for `error`.
pub fn error_body(error: &WireError) -> Result<Vec<u8>, NativeError> {
    #[derive(Serialize)]
    struct ErrBody<'a> {
        err: &'a WireError,
    }
    Ok(cbor::to_vec(&ErrBody { err: error })?)
}

/// The reply in a `RESPONSE` body whose `ok` value is a `T`.
pub fn decode_reply<T: Serialize + DeserializeOwned>(body: &[u8]) -> Result<Reply<T>, NativeError> {
    Ok(decode_strict(body)?)
}

/// The body of a `PING` or `PONG`: the token as a CBOR unsigned integer.
pub fn ping_body(token: u64) -> Result<Vec<u8>, NativeError> {
    Ok(cbor::to_vec(&token)?)
}

/// The token in a `PING` or `PONG` body.
pub fn ping_token(body: &[u8]) -> Result<u64, NativeError> {
    Ok(cbor::from_slice(body)?)
}

/// The frame type and body of a stream message. Only the four stream
/// messages travel in `STREAM_*` frames; any other [`Message`] is
/// [`NativeError::Unexpected`]. Nothing in the workspace calls this to
/// produce a stream.
pub fn encode_stream_body(message: &Message) -> Result<(FrameType, Vec<u8>), NativeError> {
    Ok(match message {
        Message::StreamStart(body) => (FrameType::StreamStart, cbor::to_vec(body)?),
        Message::StreamToken(body) => (FrameType::StreamToken, cbor::to_vec(body)?),
        Message::StreamEnd(body) => (FrameType::StreamEnd, cbor::to_vec(body)?),
        Message::StreamCancel(body) => (FrameType::StreamCancel, cbor::to_vec(body)?),
        _ => {
            return Err(NativeError::Unexpected(
                "only stream messages travel in STREAM_* frames".to_string(),
            ))
        }
    })
}

/// The stream message in the body of a `STREAM_*` frame.
pub fn decode_stream_body(frame_type: FrameType, body: &[u8]) -> Result<Message, NativeError> {
    Ok(match frame_type {
        FrameType::StreamStart => Message::StreamStart(decode_strict(body)?),
        FrameType::StreamToken => Message::StreamToken(decode_strict(body)?),
        FrameType::StreamEnd => Message::StreamEnd(decode_strict(body)?),
        FrameType::StreamCancel => Message::StreamCancel(decode_strict(body)?),
        other => {
            return Err(NativeError::Unexpected(format!(
                "{other:?} is not a stream frame"
            )))
        }
    })
}

/// The typed message in any frame that carries one, for traces and logs.
/// `REQUEST` is a command, `RESPONSE` a reply with a JSON-shaped result,
/// `PING` and `PONG` their tokens, the stream frames their bodies. Frames
/// without a message body (`HELLO`, `CLOSE`, `EXT`) are
/// [`NativeError::Unexpected`].
pub fn decode_message(frame: &Frame) -> Result<Message, NativeError> {
    Ok(match frame.frame_type {
        FrameType::Request => Message::Command(decode_strict(&frame.body)?),
        FrameType::Response => Message::Reply(decode_strict(&frame.body)?),
        FrameType::Ping => Message::Ping {
            timestamp: ping_token(&frame.body)?,
        },
        FrameType::Pong => Message::Pong {
            timestamp: ping_token(&frame.body)?,
        },
        other => return decode_stream_body(other, &frame.body),
    })
}

/// Sends `frame`, giving up after [`WRITE_DEADLINE`]. On expiry the
/// connection is finished without writing anything more (no `CLOSE`: the
/// dropped write may have left a partial frame) and the caller drops it.
pub async fn send_bounded<S>(conn: &mut Connection<S>, frame: Frame) -> Result<(), NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    send_within(conn, frame, WRITE_DEADLINE).await
}

async fn send_within<S>(
    conn: &mut Connection<S>,
    frame: Frame,
    deadline: Duration,
) -> Result<(), NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match tokio::time::timeout(deadline, conn.send(frame)).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            conn.abandon();
            Err(NativeError::WriteTimeout)
        }
    }
}

/// Client: sends `command` as a `REQUEST` and returns the frame id.
pub async fn send_command<S>(
    conn: &mut Connection<S>,
    command: &BrowserCommand,
) -> Result<u64, NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let body = encode_command(command)?;
    match tokio::time::timeout(WRITE_DEADLINE, conn.request(body)).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            conn.abandon();
            Err(NativeError::WriteTimeout)
        }
    }
}

/// Server: sends `{ok: value}` as the `RESPONSE` to request `request_id`.
pub async fn send_ok<S, T>(
    conn: &mut Connection<S>,
    request_id: u64,
    value: &T,
) -> Result<(), NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: Serialize,
{
    let body = crate::connection::response_ok_body(value)?;
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

/// Server: sends `{err: ...}` as the `RESPONSE` to request `request_id`.
pub async fn send_err<S>(
    conn: &mut Connection<S>,
    request_id: u64,
    error: &WireError,
) -> Result<(), NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let body = error_body(error)?;
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

/// Server: sends a [`Reply`] as the `RESPONSE` to request `request_id`.
pub async fn send_reply<S, T>(
    conn: &mut Connection<S>,
    request_id: u64,
    reply: &Reply<T>,
) -> Result<(), NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: Serialize,
{
    match reply {
        Reply::Ok(value) => send_ok(conn, request_id, value).await,
        Reply::Err(error) => send_err(conn, request_id, error).await,
    }
}

/// Client: reads frames until the `RESPONSE` for `request_id` arrives and
/// returns its `ok` value, or [`NativeError::Remote`] for an `err`. Frames
/// that do not match (a late reply to an earlier, abandoned request; a
/// `PONG`) are skipped. The caller bounds the wait with its own timeout and
/// drops the connection if that fires, because [`Connection::recv`] is not
/// cancel safe.
pub async fn await_reply<S, T>(conn: &mut Connection<S>, request_id: u64) -> Result<T, NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: Serialize + DeserializeOwned,
{
    loop {
        match conn.recv().await? {
            None => return Err(NativeError::Eof),
            Some(frame)
                if frame.frame_type == FrameType::Response && frame.request_id == request_id =>
            {
                return decode_reply::<T>(&frame.body)?
                    .into_result()
                    .map_err(NativeError::from);
            }
            Some(_) => {}
        }
    }
}

/// Client: one command, one reply.
pub async fn call<S, T>(
    conn: &mut Connection<S>,
    command: &BrowserCommand,
) -> Result<T, NativeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: Serialize + DeserializeOwned,
{
    let id = send_command(conn, command).await?;
    await_reply(conn, id).await
}

/// The `ok` value of a reply whose shape the caller does not name.
pub type JsonReply = Reply<serde_json::Value>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::CloseCode;
    use crate::{
        CapabilityQuery, CapabilitySelector, SpineBinary, StreamCancel, StreamEnd, StreamEndReason,
        ToolCall,
    };
    use serde_json::json;
    use tokio::io::duplex;

    const LONG: Duration = Duration::from_secs(10);

    fn commands() -> Vec<BrowserCommand> {
        let mut functions = std::collections::BTreeMap::new();
        functions.insert("main".to_string(), 3usize);
        functions.insert("a".to_string(), 0usize);
        vec![
            BrowserCommand::GetUR,
            BrowserCommand::Navigate {
                url: "https://example.com/".to_string(),
            },
            BrowserCommand::ExecuteBinary(SpineBinary {
                instructions: vec![crate::Instruction::Jump(4), crate::Instruction::Pop],
                data: vec![0, 1, 2, 255],
                render_start: 1,
                exported_functions: functions,
                capabilities: vec!["network".to_string()],
            }),
            BrowserCommand::ReadResource {
                hash: [3; 32],
                offset: 0,
                max_bytes: 10,
            },
            BrowserCommand::ToolCall(ToolCall {
                name: "spine.content/hash".to_string(),
                args: json!({"text": "x", "n": 1, "f": 0.5}),
                trace: None,
            }),
            BrowserCommand::CapabilityQuery(CapabilityQuery {
                selector: CapabilitySelector::All,
            }),
        ]
    }

    #[test]
    fn every_command_survives_the_body_codec() {
        for command in commands() {
            let body = encode_command(&command).unwrap();
            assert_eq!(decode_command(&body).unwrap(), command);
        }
    }

    fn body_of(value: &serde_json::Value) -> Vec<u8> {
        cbor::to_vec(value).unwrap()
    }

    #[test]
    fn an_unknown_command_name_is_unknown_command() {
        for body in [
            body_of(&json!("fly_to_the_moon")),
            body_of(&json!({"fly_to_the_moon": {"speed": 1}})),
            // Variant names of the pre-v1 envelope are not commands either.
            body_of(&json!({"Navigate": {"url": "u"}})),
            body_of(&json!({"request": {"id": "1"}})),
        ] {
            let error = decode_command(&body).unwrap_err();
            assert_eq!(error.code, ERR_UNKNOWN_COMMAND, "{error}");
        }
    }

    #[test]
    fn a_known_command_with_a_bad_shape_is_invalid_request() {
        for body in [
            // Unknown field.
            body_of(&json!({"navigate": {"url": "u", "extra": 1}})),
            // Missing field.
            body_of(&json!({"navigate": {}})),
            // Wrong type.
            body_of(&json!({"navigate": {"url": 5}})),
            // A struct command sent as a bare name.
            body_of(&json!("navigate")),
            // A unit command sent with fields.
            body_of(&json!({"get_ur": {"x": 1}})),
            // Not a command shape at all.
            body_of(&json!(7)),
            body_of(&json!([])),
            body_of(&json!({"navigate": {"url": "u"}, "get_ur": null})),
            body_of(&json!({})),
            // A present null for an optional.
            body_of(&json!({"find_providers": {"capability": "c", "limit": null}})),
            // A byte field sent as an array of integers.
            body_of(&json!({"read_resource": {
                "hash": vec![0u8; 32], "offset": 0, "max_bytes": 1}})),
        ] {
            let error = decode_command(&body).unwrap_err();
            assert_eq!(error.code, ERR_INVALID_REQUEST, "{error}");
        }
    }

    #[test]
    fn a_long_unknown_name_is_truncated_in_the_message() {
        let name = "x".repeat(10_000);
        let error = decode_command(&body_of(&json!(name))).unwrap_err();
        assert_eq!(error.code, ERR_UNKNOWN_COMMAND);
        assert!(error.message.len() < 200);
    }

    #[test]
    fn replies_round_trip_and_err_becomes_remote() {
        let ok = crate::connection::response_ok_body(&json!({"a": 1})).unwrap();
        let reply: JsonReply = decode_reply(&ok).unwrap();
        assert_eq!(reply, Reply::Ok(json!({"a": 1})));
        let err = error_body(&WireError::new("rate_limit", "slow down").with_details(json!([1])))
            .unwrap();
        let reply: JsonReply = decode_reply(&err).unwrap();
        match reply.into_result().map_err(NativeError::from) {
            Err(NativeError::Remote {
                code,
                message,
                details,
            }) => {
                assert_eq!(code, "rate_limit");
                assert_eq!(message, "slow down");
                assert_eq!(details, Some(json!([1])));
            }
            other => panic!("{other:?}"),
        }
        // The connection layer's own refusal decodes the same way.
        let refusal = crate::connection::response_err_body("overloaded", "too many").unwrap();
        let reply: JsonReply = decode_reply(&refusal).unwrap();
        assert!(matches!(reply, Reply::Err(e) if e.code == "overloaded"));
    }

    #[test]
    fn a_remote_error_is_recoverable_from_anyhow() {
        let error = anyhow::Error::from(NativeError::Remote {
            code: "rate_limit".into(),
            message: "slow down".into(),
            details: None,
        })
        .context("while doing something");
        assert_eq!(remote_code(&error), Some("rate_limit"));
        assert_eq!(remote_error(&error), Some(("rate_limit", "slow down")));
        assert!(remote_code(&anyhow::anyhow!("plain")).is_none());
    }

    #[test]
    fn stream_bodies_round_trip_without_the_envelope() {
        let messages = [
            Message::StreamEnd(StreamEnd {
                reason: StreamEndReason::Cancelled,
                usage: None,
            }),
            Message::StreamCancel(StreamCancel { reason: None }),
        ];
        for message in messages {
            let (frame_type, body) = encode_stream_body(&message).unwrap();
            assert_eq!(decode_stream_body(frame_type, &body).unwrap(), message);
        }
        assert!(encode_stream_body(&Message::Ping { timestamp: 1 }).is_err());
        assert!(decode_stream_body(FrameType::Request, &[0x00]).is_err());
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
    async fn a_command_gets_its_reply_over_a_connection() {
        let (c, s) = duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let mut conn = Connection::server_handshake(s, &local_config(), DEFAULT_HELLO_DEADLINE)
                .await
                .unwrap();
            let frame = conn.recv().await.unwrap().unwrap();
            assert_eq!(frame.frame_type, FrameType::Request);
            assert_eq!(
                decode_command(&frame.body).unwrap(),
                BrowserCommand::GetRawHTML
            );
            send_ok(&mut conn, frame.request_id, &json!({"html": "<p>"}))
                .await
                .unwrap();
            // A second request is refused with a code.
            let frame = conn.recv().await.unwrap().unwrap();
            send_err(
                &mut conn,
                frame.request_id,
                &WireError::new("rate_limit", "no"),
            )
            .await
            .unwrap();
            // Wait for the client to hang up.
            assert!(conn.recv().await.unwrap().is_none());
        });
        let mut conn = tokio::time::timeout(LONG, client_connect(c))
            .await
            .unwrap()
            .unwrap();
        let reply: serde_json::Value =
            tokio::time::timeout(LONG, call(&mut conn, &BrowserCommand::GetRawHTML))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(reply, json!({"html": "<p>"}));
        let refused = tokio::time::timeout(
            LONG,
            call::<_, serde_json::Value>(&mut conn, &BrowserCommand::GetRawHTML),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(matches!(refused, NativeError::Remote { ref code, .. } if code == "rate_limit"));
        drop(conn);
        tokio::time::timeout(LONG, server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_write_that_times_out_abandons_the_connection_without_a_close() {
        // A 64-byte pipe that nobody reads: a large frame cannot be written.
        let (c, s) = duplex(64);
        let (client, server) = tokio::time::timeout(LONG, async {
            tokio::join!(
                client_connect(c),
                Connection::server_handshake(s, &local_config(), DEFAULT_HELLO_DEADLINE)
            )
        })
        .await
        .unwrap();
        let mut client = client.unwrap();
        let _server = server.unwrap();
        let body = cbor::to_vec(&"a".repeat(10_000)).unwrap();
        let started = std::time::Instant::now();
        let error = send_within(
            &mut client,
            Frame {
                frame_type: FrameType::Request,
                request_id: 1,
                body,
            },
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, NativeError::WriteTimeout), "{error}");
        assert!(client.is_finished());
        // Writing a CLOSE into the full pipe would take its own one-second
        // timeout (`CLOSE_WRITE_TIMEOUT`); abandoning writes nothing.
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
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
