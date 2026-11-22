//! spine-wire v1 connection: the handshake of C.6 and the post-handshake rules
//! of C.7 to C.10 (`docs/spec/00-wire.md`), over any ordered byte stream.
//!
//! [`Connection`] is sequential and caller-driven, not an actor: it owns the
//! stream and a small amount of state (the negotiated limits, the accepted
//! extensions and the table of requests in flight) and does work only inside
//! the calls the caller makes. [`Connection::recv`] and [`Connection::send`]
//! take `&mut self`, so one task drives a connection. A caller that must read
//! and write at the same time (the daemon: it keeps reading while a command
//! runs) calls [`Connection::split`], which turns the established connection
//! into a [`ConnectionReader`] and a [`ConnectionWriter`] that share the same
//! state and apply the same rules.
//!
//! What this layer checks, using only its own state:
//!
//! - everything [`crate::frame`] checks (length, version, type, flags,
//!   compressed bodies, the stateless request-id rules), answered with the
//!   `CLOSE` code [`FrameError::close_code`] names;
//! - every body is one deterministic CBOR item (`malformed_frame`);
//! - direction: a client may send `REQUEST`, `STREAM_CANCEL`, `PING`, `PONG`,
//!   `EXT` and `CLOSE`; a server may send `RESPONSE`, `STREAM_*`, `PING`,
//!   `PONG`, `EXT` and `CLOSE`; a second `HELLO` or `HELLO_ACK` is never valid;
//! - request ids: a client id must not be in flight already; a `RESPONSE` or
//!   `STREAM_*` frame must name an in-flight request and follow the C.9 order
//!   (`STREAM_START`, tokens, `STREAM_END`, or one `RESPONSE`); an id is
//!   released by `RESPONSE` or `STREAM_END`;
//! - `max_inflight`: a server that receives a `REQUEST` while it is at the cap
//!   answers it itself with an `overloaded` `RESPONSE` and does not deliver it;
//! - `EXT` frames name an accepted extension index;
//! - `PING` is answered with `PONG` inside [`Connection::recv`].
//!
//! Not checked here, and left to the caller: the schema of bodies (unknown
//! command, invalid request, `seq` order of stream tokens), idle timeouts,
//! rate and byte budgets, and what an `EXT` kind means.
//!
//! After an error, a received `CLOSE`, a clean end of stream or a call to
//! [`Connection::close`] the connection refuses further use with
//! [`ConnectionError::Unusable`].

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};

use crate::cbor::{self, CborError};
use crate::frame::{
    encode_frame, read_frame, write_frame, CloseCode, Frame, FrameError, FrameType,
    PRE_HELLO_MAX_FRAME, VERSION,
};
use crate::hello::{negotiate, Close, Hello, HelloAck, LocalConfig};

/// `RESPONSE.err.code` for a request refused because the server is at
/// `max_inflight` (C.7).
pub const ERR_OVERLOADED: &str = "overloaded";
/// `RESPONSE.err.code` for a request that names an unknown command (C.7).
pub const ERR_UNKNOWN_COMMAND: &str = "unknown_command";
/// `RESPONSE.err.code` for a known command with a bad field (C.7).
pub const ERR_INVALID_REQUEST: &str = "invalid_request";

/// How long a best-effort `CLOSE` may take to write before it is given up.
const CLOSE_WRITE_TIMEOUT: Duration = Duration::from_secs(1);

/// Which end of the connection this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The side that sent `HELLO` and issues requests.
    Client,
    /// The side that sent `HELLO_ACK` and answers requests.
    Server,
}

/// Why a connection call failed.
#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    /// The peer sent `CLOSE`; the connection is finished.
    #[error("peer closed the connection: {0:?}")]
    Closed(Close),
    /// The peer broke a rule. We sent `CLOSE` with this code (best effort)
    /// and the connection is finished.
    #[error("protocol violation ({0:?}): {1}")]
    Violation(CloseCode, String),
    /// The handshake did not finish within its deadline, or
    /// [`Connection::recv_timeout`] saw no frame within its idle deadline.
    #[error("timed out")]
    Timeout,
    /// A frame was rejected or could not be encoded. For a received frame a
    /// `CLOSE` was sent (best effort) when [`FrameError::close_code`] has one,
    /// and the connection is finished. A frame that was too large to *send*
    /// is reported here too, and leaves the connection usable.
    #[error("frame error: {0}")]
    Frame(FrameError),
    /// A body could not be encoded, or failed the deterministic-CBOR check
    /// on the way out. The connection stays usable.
    #[error("body error: {0}")]
    Cbor(#[from] CborError),
    /// The transport failed or ended in the middle of the handshake. The
    /// connection is finished.
    #[error("i/o error: {0}")]
    Io(io::Error),
    /// The caller tried to send something this side may not send (wrong
    /// direction, an id already in flight, a response for an unknown
    /// request). Nothing was written and the connection stays usable.
    #[error("invalid use: {0}")]
    Usage(String),
    /// The connection is finished and refuses further use.
    #[error("the connection is finished")]
    Unusable,
}

impl ConnectionError {
    /// The `CLOSE` code that went with this error, when there was one.
    pub fn close_code(&self) -> Option<CloseCode> {
        match self {
            ConnectionError::Violation(code, _) => Some(*code),
            ConnectionError::Frame(e) => e.close_code(),
            _ => None,
        }
    }
}

/// A request's progress, as far as the in-flight table cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exchange {
    /// Sent or received; no reply yet.
    Pending,
    /// `STREAM_START` has been seen; `STREAM_END` releases the id.
    Streaming,
}

/// What to do with a frame that passed the state checks.
enum Verdict {
    /// Hand it to the caller.
    Deliver,
    /// Drop it silently.
    Ignore,
    /// Answer with `overloaded`; do not deliver.
    Overloaded,
}

/// What a successfully written frame does to the in-flight table.
enum Transition {
    None,
    Track,
    Stream,
    Release,
    Finish,
}

/// `{ok: value}`, the body of a successful `RESPONSE` (C.3).
#[derive(Serialize)]
struct OkBody<'a, T> {
    ok: &'a T,
}

/// `{code, message}`, the `err` of a failed `RESPONSE` (C.7).
#[derive(Serialize)]
struct ErrDetail<'a> {
    code: &'a str,
    message: &'a str,
}

/// `{err: {code, message}}`, the body of a failed `RESPONSE` (C.3, C.7).
#[derive(Serialize)]
struct ErrBody<'a> {
    err: ErrDetail<'a>,
}

/// The CBOR body `{ok: value}` for a successful `RESPONSE`.
pub fn response_ok_body<T: Serialize>(value: &T) -> Result<Vec<u8>, CborError> {
    cbor::to_vec(&OkBody { ok: value })
}

/// The CBOR body `{err: {code, message}}` for a failed `RESPONSE`. The
/// optional `details` field is not produced.
pub fn response_err_body(code: &str, message: &str) -> Result<Vec<u8>, CborError> {
    cbor::to_vec(&ErrBody {
        err: ErrDetail { code, message },
    })
}

/// The extension index of an `EXT` body `(index, kind, payload)`, read from
/// the head of the item without decoding the payload. `None` when the body is
/// not a three-element array that starts with an unsigned integer. The body
/// has already passed the deterministic check, so heads are in shortest form.
fn ext_index(body: &[u8]) -> Option<u64> {
    if body.first() != Some(&0x83) {
        return None;
    }
    let head = *body.get(1)?;
    if head >> 5 != 0 {
        return None;
    }
    let info = head & 0x1f;
    let width: usize = match info {
        0..=23 => return Some(u64::from(info)),
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => return None,
    };
    let bytes = body.get(2..2 + width)?;
    let mut value = 0u64;
    for b in bytes {
        value = (value << 8) | u64::from(*b);
    }
    Some(value)
}

/// Maps a frame-layer error to the connection error, keeping I/O separate.
fn map_frame_error(e: FrameError) -> ConnectionError {
    match e {
        FrameError::Io(io_error) => ConnectionError::Io(io_error),
        other => ConnectionError::Frame(other),
    }
}

/// The violation error that goes with a `CLOSE` body we sent.
fn violation_of(close: &Close) -> ConnectionError {
    ConnectionError::Violation(
        close.close_code().unwrap_or(CloseCode::ProtocolViolation),
        close.message.clone(),
    )
}

/// Writes a `CLOSE` and shuts the write side down, giving up after
/// [`CLOSE_WRITE_TIMEOUT`] and ignoring every failure: the peer may be gone.
async fn best_effort_close<S: AsyncWrite + Unpin>(stream: &mut S, close: &Close) {
    let frame = match close.to_frame() {
        Ok(f) => f,
        Err(_) => return,
    };
    let work = async {
        let _ = write_frame(stream, &frame, PRE_HELLO_MAX_FRAME, false).await;
        let _ = stream.shutdown().await;
    };
    let _ = tokio::time::timeout(CLOSE_WRITE_TIMEOUT, work).await;
}

/// Sends `close` (best effort) and returns the matching violation error.
async fn reject<S: AsyncWrite + Unpin>(stream: &mut S, close: Close) -> ConnectionError {
    best_effort_close(stream, &close).await;
    violation_of(&close)
}

/// Answers a frame-layer rejection of a received frame with the right
/// `CLOSE` (when there is one) and returns the error.
async fn close_for_frame_error<S: AsyncWrite + Unpin>(
    stream: &mut S,
    e: FrameError,
) -> ConnectionError {
    if let Some(code) = e.close_code() {
        best_effort_close(stream, &Close::new(code, e.to_string())).await;
    }
    map_frame_error(e)
}

/// The error for a stream that ended before the first frame arrived.
fn eof_before_handshake(what: &str) -> ConnectionError {
    ConnectionError::Io(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!("peer closed the stream before {what}"),
    ))
}

/// What a received frame turned out to be, once the state rules have been
/// applied to it. Shared by [`Connection`] and [`ConnectionReader`].
enum Admit {
    /// Hand it to the caller.
    Deliver(Frame),
    /// A frame this side must send back (a `PONG` for a `PING`).
    Pong(Frame),
    /// A request that arrived at `max_inflight`: answer it with `overloaded`.
    Overloaded(u64),
    /// Drop it silently.
    Ignore,
    /// The peer's `CLOSE`.
    Closed(Close),
}

/// A received frame that broke a rule.
enum Fault {
    /// Send `CLOSE` with this code and message.
    Violation(CloseCode, String),
    /// Send this `CLOSE` body (it came from a body check).
    Reject(Close),
}

/// The protocol state of one connection: the negotiated limits, the accepted
/// extensions and the table of requests in flight, with the rules that read and
/// change them. It does no I/O, so [`Connection`] (one owner) and the halves
/// of [`Connection::split`] (a shared copy behind a mutex) apply the same
/// rules.
#[derive(Debug)]
struct Tracker {
    role: Role,
    max_inflight: u32,
    extensions: Vec<String>,
    in_flight: HashMap<u64, Exchange>,
    overloaded: u64,
}

impl Tracker {
    /// Checks an `EXT` body against the accepted extensions.
    fn check_ext(&self, body: &[u8]) -> Result<(), String> {
        match ext_index(body) {
            None => Err("EXT body is not an array of index, kind and payload".to_string()),
            Some(index) if index >= self.extensions.len() as u64 => Err(format!(
                "EXT index {} is out of range ({} extensions accepted)",
                index,
                self.extensions.len()
            )),
            Some(_) => Ok(()),
        }
    }

    /// State checks for a frame about to be sent.
    fn check_outgoing(&self, frame: &Frame) -> Result<Transition, String> {
        let id = frame.request_id;
        let state = self.in_flight.get(&id).copied();
        match (self.role, frame.frame_type) {
            (_, FrameType::Ping) | (_, FrameType::Pong) => Ok(Transition::None),
            (_, FrameType::Close) => Ok(Transition::Finish),
            (_, FrameType::Ext) => self.check_ext(&frame.body).map(|_| Transition::None),
            (Role::Client, FrameType::Request) => {
                if state.is_some() {
                    Err(format!("request id {id} is already in flight"))
                } else {
                    Ok(Transition::Track)
                }
            }
            (Role::Client, FrameType::StreamCancel) => Ok(Transition::None),
            (Role::Server, FrameType::Response) => match state {
                Some(Exchange::Pending) => Ok(Transition::Release),
                Some(Exchange::Streaming) => {
                    Err(format!("request {id} is streaming and gets no RESPONSE"))
                }
                None => Err(format!("RESPONSE for request {id}, which is not in flight")),
            },
            (Role::Server, FrameType::StreamStart) => match state {
                Some(Exchange::Pending) => Ok(Transition::Stream),
                Some(Exchange::Streaming) => Err(format!("request {id} is already streaming")),
                None => Err(format!(
                    "STREAM_START for request {id}, which is not in flight"
                )),
            },
            (Role::Server, FrameType::StreamToken) => match state {
                Some(Exchange::Streaming) => Ok(Transition::None),
                _ => Err(format!(
                    "STREAM_TOKEN for request {id}, which is not streaming"
                )),
            },
            (Role::Server, FrameType::StreamEnd) => match state {
                Some(Exchange::Streaming) => Ok(Transition::Release),
                _ => Err(format!(
                    "STREAM_END for request {id}, which is not streaming"
                )),
            },
            (role, frame_type) => Err(format!("a {role:?} may not send {frame_type:?}")),
        }
    }

    /// Applies what a successfully checked outgoing frame does to the table.
    fn apply(&mut self, id: u64, transition: &Transition) {
        match transition {
            Transition::None | Transition::Finish => {}
            Transition::Track => {
                self.in_flight.insert(id, Exchange::Pending);
            }
            Transition::Stream => {
                self.in_flight.insert(id, Exchange::Streaming);
            }
            Transition::Release => {
                self.in_flight.remove(&id);
            }
        }
    }

    /// State checks for a received frame of a kind that is neither `CLOSE`,
    /// `PING`, `PONG`, `HELLO` nor `HELLO_ACK`. The `Err` text is the message
    /// of a `protocol_violation`.
    fn check_incoming(&mut self, frame: &Frame) -> Result<Verdict, String> {
        let id = frame.request_id;
        let state = self.in_flight.get(&id).copied();
        match (self.role, frame.frame_type) {
            (_, FrameType::Ext) => self.check_ext(&frame.body).map(|_| Verdict::Deliver),
            (Role::Server, FrameType::Request) => {
                if state.is_some() {
                    return Err(format!("request id {id} is already in flight"));
                }
                if self.in_flight.len() as u64 >= u64::from(self.max_inflight) {
                    return Ok(Verdict::Overloaded);
                }
                self.in_flight.insert(id, Exchange::Pending);
                Ok(Verdict::Deliver)
            }
            (Role::Server, FrameType::StreamCancel) => {
                if state.is_some() {
                    Ok(Verdict::Deliver)
                } else {
                    Ok(Verdict::Ignore)
                }
            }
            (Role::Client, FrameType::Response) => match state {
                Some(Exchange::Pending) => {
                    self.in_flight.remove(&id);
                    Ok(Verdict::Deliver)
                }
                Some(Exchange::Streaming) => {
                    Err(format!("RESPONSE for request {id}, which is streaming"))
                }
                None => Err(format!("RESPONSE for request {id}, which is not in flight")),
            },
            (Role::Client, FrameType::StreamStart) => match state {
                Some(Exchange::Pending) => {
                    self.in_flight.insert(id, Exchange::Streaming);
                    Ok(Verdict::Deliver)
                }
                Some(Exchange::Streaming) => Err(format!("request {id} is already streaming")),
                None => Err(format!(
                    "STREAM_START for request {id}, which is not in flight"
                )),
            },
            (Role::Client, FrameType::StreamToken) => match state {
                Some(Exchange::Streaming) => Ok(Verdict::Deliver),
                _ => Err(format!(
                    "STREAM_TOKEN for request {id}, which is not streaming"
                )),
            },
            (Role::Client, FrameType::StreamEnd) => match state {
                Some(Exchange::Streaming) => {
                    self.in_flight.remove(&id);
                    Ok(Verdict::Deliver)
                }
                _ => Err(format!(
                    "STREAM_END for request {id}, which is not streaming"
                )),
            },
            (role, frame_type) => Err(format!("a {role:?} may not receive {frame_type:?}")),
        }
    }

    /// Applies the post-handshake rules to one received frame, without any
    /// I/O: the caller does what the result says.
    fn admit(&mut self, frame: Frame) -> Result<Admit, Fault> {
        if let Err(e) = cbor::check_deterministic(&frame.body) {
            return Err(Fault::Violation(
                CloseCode::MalformedFrame,
                format!("body is not deterministic CBOR: {e}"),
            ));
        }
        match frame.frame_type {
            FrameType::Close => match Close::from_frame(&frame) {
                Ok(close) => Ok(Admit::Closed(close)),
                Err(close) => Err(Fault::Reject(close)),
            },
            FrameType::Ping | FrameType::Pong => {
                if cbor::from_slice::<u64>(&frame.body).is_err() {
                    return Err(Fault::Violation(
                        CloseCode::ProtocolViolation,
                        "PING and PONG carry an unsigned integer token".to_string(),
                    ));
                }
                if frame.frame_type == FrameType::Ping {
                    Ok(Admit::Pong(Frame {
                        frame_type: FrameType::Pong,
                        request_id: frame.request_id,
                        body: frame.body,
                    }))
                } else {
                    Ok(Admit::Deliver(frame))
                }
            }
            FrameType::Hello | FrameType::HelloAck => Err(Fault::Violation(
                CloseCode::ProtocolViolation,
                "HELLO and HELLO_ACK are only valid as first frames".to_string(),
            )),
            _ => match self.check_incoming(&frame) {
                Err(message) => Err(Fault::Violation(CloseCode::ProtocolViolation, message)),
                Ok(Verdict::Deliver) => Ok(Admit::Deliver(frame)),
                Ok(Verdict::Ignore) => Ok(Admit::Ignore),
                Ok(Verdict::Overloaded) => {
                    self.overloaded += 1;
                    Ok(Admit::Overloaded(frame.request_id))
                }
            },
        }
    }
}

/// The `overloaded` `RESPONSE` for request `id`.
fn overloaded_frame(id: u64) -> Result<Frame, CborError> {
    Ok(Frame {
        frame_type: FrameType::Response,
        request_id: id,
        body: response_err_body(ERR_OVERLOADED, "too many requests in flight")?,
    })
}

/// An established spine-wire v1 connection. See the module documentation.
#[derive(Debug)]
pub struct Connection<S> {
    stream: S,
    version: u64,
    max_frame: u32,
    compress: bool,
    tracker: Tracker,
    next_id: u64,
    finished: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Connection<S> {
    /// Builds the established connection from the acknowledgement.
    fn established(stream: S, role: Role, ack: &HelloAck) -> Connection<S> {
        Connection {
            stream,
            version: ack.version,
            max_frame: ack.max_frame,
            compress: false,
            tracker: Tracker {
                role,
                max_inflight: ack.max_inflight,
                extensions: ack.extensions.clone(),
                in_flight: HashMap::new(),
                overloaded: 0,
            },
            next_id: 1,
            finished: false,
        }
    }

    /// Client side of C.6: sends `hello`, reads `HELLO_ACK` (or `CLOSE`),
    /// checks it against `hello` and adopts its limits.
    ///
    /// `deadline` bounds the write of `HELLO` and, separately, the wait for
    /// the reply; on expiry a `CLOSE(timeout)` is sent (best effort) and the
    /// result is [`ConnectionError::Timeout`]. A `CLOSE` from the server is
    /// [`ConnectionError::Closed`]. A `hello` that fails its own validation
    /// is [`ConnectionError::Usage`] and nothing is sent. The reply is read
    /// under the pre-`HELLO` frame limit.
    pub async fn client_handshake(
        mut stream: S,
        hello: Hello,
        deadline: Duration,
    ) -> Result<Connection<S>, ConnectionError> {
        hello
            .validate()
            .map_err(|close| ConnectionError::Usage(close.message))?;
        let hello_frame = hello.to_frame()?;
        match tokio::time::timeout(
            deadline,
            write_frame(&mut stream, &hello_frame, PRE_HELLO_MAX_FRAME, false),
        )
        .await
        {
            Err(_) => return Err(ConnectionError::Timeout),
            Ok(Err(e)) => return Err(map_frame_error(e)),
            Ok(Ok(())) => {}
        }

        let reply = match tokio::time::timeout(
            deadline,
            read_frame(&mut stream, PRE_HELLO_MAX_FRAME),
        )
        .await
        {
            Err(_) => {
                best_effort_close(
                    &mut stream,
                    &Close::new(CloseCode::Timeout, "no HELLO_ACK within the deadline"),
                )
                .await;
                return Err(ConnectionError::Timeout);
            }
            Ok(Err(e)) => return Err(close_for_frame_error(&mut stream, e).await),
            Ok(Ok(None)) => return Err(eof_before_handshake("HELLO_ACK")),
            Ok(Ok(Some(f))) => f,
        };

        if reply.frame_type == FrameType::Close {
            return match Close::from_frame(&reply) {
                Ok(close) => Err(ConnectionError::Closed(close)),
                Err(close) => Err(reject(&mut stream, close).await),
            };
        }
        let ack = match HelloAck::from_frame(&reply) {
            Ok(a) => a,
            Err(close) => return Err(reject(&mut stream, close).await),
        };
        if let Err(close) = ack.validate_against(&hello) {
            return Err(reject(&mut stream, close).await);
        }
        if ack.version != u64::from(VERSION) {
            let close = Close::unsupported_version(
                "the selected version is not implemented",
                vec![u64::from(VERSION)],
            );
            return Err(reject(&mut stream, close).await);
        }
        Ok(Connection::established(stream, Role::Client, &ack))
    }

    /// Server side of C.6: reads the first frame under the pre-`HELLO` limit
    /// and `deadline`, negotiates against `local` and sends `HELLO_ACK`.
    ///
    /// A first frame that is not a valid `HELLO`, or a `HELLO` that cannot be
    /// negotiated, is answered with the `CLOSE` C.7 assigns (best effort) and
    /// [`ConnectionError::Violation`]. No frame within `deadline` is answered
    /// with `CLOSE(timeout)` and [`ConnectionError::Timeout`]. The deadline
    /// applies separately to the read and to the write of `HELLO_ACK`.
    pub async fn server_handshake(
        mut stream: S,
        local: &LocalConfig,
        deadline: Duration,
    ) -> Result<Connection<S>, ConnectionError> {
        let first = match tokio::time::timeout(
            deadline,
            read_frame(&mut stream, PRE_HELLO_MAX_FRAME),
        )
        .await
        {
            Err(_) => {
                best_effort_close(
                    &mut stream,
                    &Close::new(CloseCode::Timeout, "no HELLO within the deadline"),
                )
                .await;
                return Err(ConnectionError::Timeout);
            }
            Ok(Err(e)) => return Err(close_for_frame_error(&mut stream, e).await),
            Ok(Ok(None)) => return Err(eof_before_handshake("HELLO")),
            Ok(Ok(Some(f))) => f,
        };

        let hello = match Hello::from_frame(&first) {
            Ok(h) => h,
            Err(close) => return Err(reject(&mut stream, close).await),
        };
        let ack = match negotiate(&hello, local) {
            Ok(a) => a,
            Err(close) => return Err(reject(&mut stream, close).await),
        };
        if ack.version != u64::from(VERSION) {
            let close = Close::unsupported_version(
                "the selected version is not implemented",
                vec![u64::from(VERSION)],
            );
            return Err(reject(&mut stream, close).await);
        }
        let ack_frame = ack.to_frame()?;
        match tokio::time::timeout(
            deadline,
            write_frame(&mut stream, &ack_frame, PRE_HELLO_MAX_FRAME, false),
        )
        .await
        {
            Err(_) => return Err(ConnectionError::Timeout),
            Ok(Err(e)) => return Err(map_frame_error(e)),
            Ok(Ok(())) => {}
        }
        Ok(Connection::established(stream, Role::Server, &ack))
    }

    /// Which end this is.
    pub fn role(&self) -> Role {
        self.tracker.role
    }

    /// The negotiated frame version.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The negotiated `max_frame`, which applies in both directions.
    pub fn max_frame(&self) -> u32 {
        self.max_frame
    }

    /// The negotiated `max_inflight`.
    pub fn max_inflight(&self) -> u32 {
        self.tracker.max_inflight
    }

    /// The accepted extensions; the position is the extension index.
    pub fn extensions(&self) -> &[String] {
        &self.tracker.extensions
    }

    /// The number of requests currently in flight, as this side tracks them.
    pub fn in_flight_len(&self) -> usize {
        self.tracker.in_flight.len()
    }

    /// How many received requests this (server) side has answered with
    /// `overloaded` without delivering them.
    pub fn overloaded_count(&self) -> u64 {
        self.tracker.overloaded
    }

    /// True once the connection refuses further use.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Turns sender-side compression on or off (off by default). Bodies under
    /// the C.4 threshold are never compressed. The receiver needs no setting.
    pub fn set_compression(&mut self, enabled: bool) {
        self.compress = enabled;
    }

    /// Fails with [`ConnectionError::Unusable`] once the connection is finished.
    fn ensure_open(&self) -> Result<(), ConnectionError> {
        if self.finished {
            Err(ConnectionError::Unusable)
        } else {
            Ok(())
        }
    }

    /// Writes a frame with the negotiated limit. An I/O failure finishes the
    /// connection; an encode failure (a body over the limit) does not.
    async fn write(&mut self, frame: &Frame) -> Result<(), ConnectionError> {
        match write_frame(&mut self.stream, frame, self.max_frame, self.compress).await {
            Ok(()) => Ok(()),
            Err(FrameError::Io(e)) => {
                self.finished = true;
                Err(ConnectionError::Io(e))
            }
            Err(e) => Err(ConnectionError::Frame(e)),
        }
    }

    /// Finishes the connection after a peer violation: best-effort `CLOSE`
    /// with `code`, then the error.
    async fn violation(&mut self, code: CloseCode, message: String) -> ConnectionError {
        self.finished = true;
        best_effort_close(&mut self.stream, &Close::new(code, message.as_str())).await;
        ConnectionError::Violation(code, message)
    }

    /// Sends the given `CLOSE` body (best effort) and finishes the connection.
    async fn reject(&mut self, close: Close) -> ConnectionError {
        self.finished = true;
        reject(&mut self.stream, close).await
    }

    /// Sends a `CLOSE` with `code` and `message` (best effort), shuts the
    /// write side down and finishes the connection. Does nothing if it is
    /// finished already. Use it for `shutting_down` and `timeout`.
    pub async fn close(&mut self, code: CloseCode, message: &str) {
        if self.finished {
            return;
        }
        self.finished = true;
        best_effort_close(&mut self.stream, &Close::new(code, message)).await;
    }

    /// Finishes the connection without writing anything. For a write that was
    /// cut off (a deadline): the stream may hold a partial frame, so no `CLOSE`
    /// can be read by the peer, and the caller should drop the connection.
    pub fn abandon(&mut self) {
        self.finished = true;
    }

    /// Sends one frame after checking it against this side's state.
    ///
    /// The body must be deterministic CBOR ([`ConnectionError::Cbor`]), the
    /// frame type must be one this role may send, and the request id must
    /// fit the in-flight table (see the module documentation); a violation
    /// of the last two is [`ConnectionError::Usage`]. In all those cases
    /// nothing is written and the connection stays usable. The table is
    /// updated after the frame is written. Sending `CLOSE` finishes the
    /// connection. `HELLO` and `HELLO_ACK` are never valid here.
    pub async fn send(&mut self, frame: Frame) -> Result<(), ConnectionError> {
        self.ensure_open()?;
        cbor::check_deterministic(&frame.body)?;
        let transition = self
            .tracker
            .check_outgoing(&frame)
            .map_err(ConnectionError::Usage)?;
        self.write(&frame).await?;
        self.tracker.apply(frame.request_id, &transition);
        if matches!(transition, Transition::Finish) {
            self.finished = true;
            let _ = self.stream.shutdown().await;
        }
        Ok(())
    }

    /// Client only: sends a `REQUEST` with the next free id (a counter from
    /// 1 that skips ids still in flight) and returns the id. The number of
    /// requests in flight is not capped here: a server at `max_inflight`
    /// answers the extra request with `overloaded`.
    pub async fn request(&mut self, body: Vec<u8>) -> Result<u64, ConnectionError> {
        if self.tracker.role != Role::Client {
            return Err(ConnectionError::Usage(
                "only a client sends requests".to_string(),
            ));
        }
        let mut id = self.next_id;
        loop {
            if id == 0 {
                id = 1;
            }
            if !self.tracker.in_flight.contains_key(&id) {
                break;
            }
            id = id.wrapping_add(1);
        }
        self.send(Frame {
            frame_type: FrameType::Request,
            request_id: id,
            body,
        })
        .await?;
        self.next_id = id.wrapping_add(1);
        Ok(id)
    }

    /// Server only: sends `RESPONSE {ok: value}` for `request_id`.
    pub async fn respond_ok<T: Serialize>(
        &mut self,
        request_id: u64,
        value: &T,
    ) -> Result<(), ConnectionError> {
        let body = response_ok_body(value)?;
        self.send(Frame {
            frame_type: FrameType::Response,
            request_id,
            body,
        })
        .await
    }

    /// Server only: sends `RESPONSE {err: {code, message}}` for `request_id`.
    pub async fn respond_err(
        &mut self,
        request_id: u64,
        code: &str,
        message: &str,
    ) -> Result<(), ConnectionError> {
        let body = response_err_body(code, message)?;
        self.send(Frame {
            frame_type: FrameType::Response,
            request_id,
            body,
        })
        .await
    }

    /// Applies the post-handshake rules to one received frame. `Ok(None)`
    /// means the frame was handled here (a `PING`, an ignored cancel, an
    /// overloaded request) and the caller should read on.
    async fn accept(&mut self, frame: Frame) -> Result<Option<Frame>, ConnectionError> {
        match self.tracker.admit(frame) {
            Ok(Admit::Deliver(frame)) => Ok(Some(frame)),
            Ok(Admit::Ignore) => Ok(None),
            Ok(Admit::Pong(pong)) => {
                self.write(&pong).await?;
                Ok(None)
            }
            Ok(Admit::Overloaded(id)) => {
                let answer = overloaded_frame(id)?;
                self.write(&answer).await?;
                Ok(None)
            }
            Ok(Admit::Closed(close)) => {
                self.finished = true;
                Err(ConnectionError::Closed(close))
            }
            Err(Fault::Violation(code, message)) => Err(self.violation(code, message).await),
            Err(Fault::Reject(close)) => Err(self.reject(close).await),
        }
    }

    /// Reads the next frame the caller must see.
    ///
    /// `PING` is answered with `PONG` here and not returned. A request that
    /// arrives while a server is at `max_inflight` is answered with an
    /// `overloaded` `RESPONSE` here and not returned (it is counted by
    /// [`Connection::overloaded_count`]). A `STREAM_CANCEL` for an id that is
    /// not in flight is dropped. Everything else that passes the rules in the
    /// module documentation is returned: `PONG`, `EXT`, and per role
    /// `REQUEST` and `STREAM_CANCEL` (server) or `RESPONSE` and `STREAM_*`
    /// (client).
    ///
    /// `Ok(None)` is a clean end of stream before a frame; the connection is
    /// then finished. A `CLOSE` from the peer is [`ConnectionError::Closed`].
    /// A peer violation is answered with a best-effort `CLOSE` and returned as
    /// [`ConnectionError::Violation`] or [`ConnectionError::Frame`]; either
    /// finishes the connection.
    ///
    /// This call is not cancel safe: dropping the future (for example in a
    /// timeout) can lose part of a frame, so abandon the connection if you do.
    pub async fn recv(&mut self) -> Result<Option<Frame>, ConnectionError> {
        self.recv_within(None).await
    }

    /// Like [`Connection::recv`], with an idle deadline that restarts for every
    /// frame read, including the `PING`s this call answers itself. A peer that
    /// sends nothing for `idle` (or stalls inside a frame for that long) is
    /// sent `CLOSE(timeout)` (best effort) and the result is
    /// [`ConnectionError::Timeout`]; the connection is finished.
    ///
    /// Unlike wrapping [`Connection::recv`] in a timeout, this keeps a `PING`
    /// keep-alive counting as activity and leaves the connection in a defined
    /// state when the deadline fires.
    pub async fn recv_timeout(&mut self, idle: Duration) -> Result<Option<Frame>, ConnectionError> {
        self.recv_within(Some(idle)).await
    }

    /// The shared body of [`Connection::recv`] and [`Connection::recv_timeout`].
    async fn recv_within(
        &mut self,
        idle: Option<Duration>,
    ) -> Result<Option<Frame>, ConnectionError> {
        loop {
            self.ensure_open()?;
            let read = match idle {
                None => read_frame(&mut self.stream, self.max_frame).await,
                Some(limit) => {
                    match tokio::time::timeout(limit, read_frame(&mut self.stream, self.max_frame))
                        .await
                    {
                        Ok(result) => result,
                        Err(_) => {
                            self.finished = true;
                            best_effort_close(
                                &mut self.stream,
                                &Close::new(CloseCode::Timeout, "idle timeout"),
                            )
                            .await;
                            return Err(ConnectionError::Timeout);
                        }
                    }
                }
            };
            let frame = match read {
                Ok(Some(f)) => f,
                Ok(None) => {
                    self.finished = true;
                    return Ok(None);
                }
                Err(e) => {
                    self.finished = true;
                    return Err(close_for_frame_error(&mut self.stream, e).await);
                }
            };
            if let Some(delivered) = self.accept(frame).await? {
                return Ok(Some(delivered));
            }
        }
    }
}

impl ConnectionError {
    /// The `CLOSE` a caller of the split halves should send for this error.
    ///
    /// [`ConnectionReader`] cannot write, so unlike [`Connection::recv`] it
    /// does not answer a violation with `CLOSE` itself. This gives the body the
    /// caller hands to [`ConnectionWriter::close`]: the violation or frame
    /// error's code, or `timeout` for [`ConnectionError::Timeout`]. `None` for
    /// errors that need no `CLOSE` (a peer `CLOSE`, a failed transport).
    pub fn close_to_send(&self) -> Option<Close> {
        match self {
            ConnectionError::Violation(code, message) => Some(Close::new(*code, message.as_str())),
            ConnectionError::Frame(e) => e.close_code().map(|code| Close::new(code, e.to_string())),
            ConnectionError::Timeout => Some(Close::new(CloseCode::Timeout, "idle timeout")),
            _ => None,
        }
    }
}

/// What the two halves of a split connection share: the protocol state, behind
/// a mutex that is never held across an await, and the finished flag.
#[derive(Debug)]
struct Shared {
    tracker: Mutex<Tracker>,
    finished: AtomicBool,
}

impl Shared {
    /// The protocol state. A panic while it was held cannot leave it half
    /// updated (every update is one table operation), so poison is ignored.
    fn lock(&self) -> MutexGuard<'_, Tracker> {
        self.tracker.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn finish(&self) {
        self.finished.store(true, Ordering::SeqCst);
    }

    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    fn ensure_open(&self) -> Result<(), ConnectionError> {
        if self.is_finished() {
            Err(ConnectionError::Unusable)
        } else {
            Ok(())
        }
    }
}

/// What [`ConnectionReader::recv_timeout`] returns.
#[derive(Debug)]
pub enum Incoming {
    /// A frame for the caller, as [`Connection::recv`] would return it:
    /// `PONG`, `EXT`, and per role `REQUEST` and `STREAM_CANCEL` (server) or
    /// `RESPONSE` and `STREAM_*` (client).
    Frame(Frame),
    /// A frame the reader produced for the peer and the caller must write, in
    /// order, with [`ConnectionWriter::answer`]: the `PONG` for a `PING`, or
    /// the `overloaded` `RESPONSE` for a request that arrived at
    /// `max_inflight` (which is not delivered and not tracked).
    Reply(Frame),
}

/// The read half of a connection made by [`Connection::split`].
///
/// It owns the read side of the stream and applies the same rules to received
/// frames as [`Connection::recv`] (it calls the same checks). It never writes:
/// what [`Connection::recv`] would have written itself comes back as
/// [`Incoming::Reply`], and an error that wants a `CLOSE` is turned into one by
/// [`ConnectionError::close_to_send`].
#[derive(Debug)]
pub struct ConnectionReader<S> {
    read: ReadHalf<S>,
    shared: Arc<Shared>,
    max_frame: u32,
}

/// The write half of a connection made by [`Connection::split`]. It owns the
/// write side of the stream, so frames are written one at a time, in the order
/// of the calls. It shares the in-flight table with the reader.
#[derive(Debug)]
pub struct ConnectionWriter<S> {
    write: WriteHalf<S>,
    shared: Arc<Shared>,
    max_frame: u32,
    compress: bool,
    /// A `CLOSE` has been written, or the write side was shut down.
    closed: bool,
    /// A write failed or was cut off: nothing more can be written.
    dead: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Connection<S> {
    /// Splits the connection into a reader and a writer that can run in
    /// different tasks, so that a server keeps reading (answering `PING`,
    /// refusing requests over `max_inflight`, seeing a cancel) while it works
    /// on a request and writes the answer.
    ///
    /// Both halves keep the state the connection had: the in-flight table, the
    /// negotiated limits, the compression setting and the finished flag. The
    /// table is shared behind a mutex that is held only for one check or
    /// update, never across an await. The sequential API of [`Connection`]
    /// stays as it is; use `split` only when reading and writing must overlap.
    pub fn split(self) -> (ConnectionReader<S>, ConnectionWriter<S>) {
        let (read, write) = tokio::io::split(self.stream);
        let shared = Arc::new(Shared {
            tracker: Mutex::new(self.tracker),
            finished: AtomicBool::new(self.finished),
        });
        (
            ConnectionReader {
                read,
                shared: Arc::clone(&shared),
                max_frame: self.max_frame,
            },
            ConnectionWriter {
                write,
                shared,
                max_frame: self.max_frame,
                compress: self.compress,
                closed: false,
                dead: false,
            },
        )
    }
}

impl<S: AsyncRead + Unpin> ConnectionReader<S> {
    /// The negotiated `max_inflight`.
    pub fn max_inflight(&self) -> u32 {
        self.shared.lock().max_inflight
    }

    /// The number of requests currently in flight, as the shared table has them.
    pub fn in_flight_len(&self) -> usize {
        self.shared.lock().in_flight.len()
    }

    /// How many requests have been answered with `overloaded` instead of
    /// being delivered.
    pub fn overloaded_count(&self) -> u64 {
        self.shared.lock().overloaded
    }

    /// True once the connection refuses further use (either half finished it).
    pub fn is_finished(&self) -> bool {
        self.shared.is_finished()
    }

    /// Reads the next frame the caller must see.
    ///
    /// Like [`Connection::recv_timeout`], except that it never writes (see
    /// [`Incoming::Reply`]), and that the idle deadline is suspended while any
    /// request is in flight: a client that is waiting for a slow answer is not
    /// idle. When the deadline fires with nothing in flight the result is
    /// [`ConnectionError::Timeout`] and the connection is finished; the caller
    /// sends `CLOSE(timeout)` (see [`ConnectionError::close_to_send`]). With a
    /// request in flight the wait simply continues, so the effective idle time
    /// after the last answer is between one and two `idle` periods.
    ///
    /// A clean end of stream is `Ok(None)`, a peer `CLOSE` is
    /// [`ConnectionError::Closed`], and a violation is
    /// [`ConnectionError::Violation`] or [`ConnectionError::Frame`]; each
    /// finishes the connection.
    ///
    /// This call is not cancel safe: dropping the future can lose part of a
    /// frame, so stop using the connection if you do.
    pub async fn recv_timeout(
        &mut self,
        idle: Duration,
    ) -> Result<Option<Incoming>, ConnectionError> {
        loop {
            self.shared.ensure_open()?;
            let read = read_frame(&mut self.read, self.max_frame);
            tokio::pin!(read);
            let result = loop {
                match tokio::time::timeout(idle, &mut read).await {
                    Ok(result) => break result,
                    Err(_) => {
                        let busy = !self.shared.lock().in_flight.is_empty();
                        if !busy {
                            self.shared.finish();
                            return Err(ConnectionError::Timeout);
                        }
                    }
                }
            };
            let frame = match result {
                Ok(Some(frame)) => frame,
                Ok(None) => {
                    self.shared.finish();
                    return Ok(None);
                }
                Err(e) => {
                    self.shared.finish();
                    return Err(map_frame_error(e));
                }
            };
            let admitted = self.shared.lock().admit(frame);
            match admitted {
                Ok(Admit::Deliver(frame)) => return Ok(Some(Incoming::Frame(frame))),
                Ok(Admit::Pong(pong)) => return Ok(Some(Incoming::Reply(pong))),
                Ok(Admit::Overloaded(id)) => {
                    return Ok(Some(Incoming::Reply(overloaded_frame(id)?)));
                }
                Ok(Admit::Ignore) => {}
                Ok(Admit::Closed(close)) => {
                    self.shared.finish();
                    return Err(ConnectionError::Closed(close));
                }
                Err(Fault::Violation(code, message)) => {
                    self.shared.finish();
                    return Err(ConnectionError::Violation(code, message));
                }
                Err(Fault::Reject(close)) => {
                    self.shared.finish();
                    return Err(violation_of(&close));
                }
            }
        }
    }
}

impl<S: AsyncWrite + Unpin> ConnectionWriter<S> {
    /// The number of requests currently in flight, as the shared table has them.
    pub fn in_flight_len(&self) -> usize {
        self.shared.lock().in_flight.len()
    }

    /// True once the connection refuses further use (either half finished it).
    pub fn is_finished(&self) -> bool {
        self.shared.is_finished()
    }

    /// True after a write failed or was cut off ([`ConnectionWriter::abandon`]):
    /// nothing more can be written, not even a `CLOSE`. Unlike
    /// [`ConnectionWriter::is_finished`] it is not set by the reader.
    pub fn is_dead(&self) -> bool {
        self.dead
    }

    /// Finishes the connection without writing anything, for a write that was
    /// cut off by a deadline: the stream may hold a partial frame, so no
    /// `CLOSE` can be read by the peer and the caller drops the halves.
    pub fn abandon(&mut self) {
        self.dead = true;
        self.shared.finish();
    }

    async fn put(&mut self, bytes: &[u8]) -> Result<(), ConnectionError> {
        let written = async {
            self.write.write_all(bytes).await?;
            self.write.flush().await
        }
        .await;
        match written {
            Ok(()) => Ok(()),
            Err(e) => {
                self.dead = true;
                self.shared.finish();
                Err(ConnectionError::Io(e))
            }
        }
    }

    /// Sends one frame after checking it against the shared state, exactly as
    /// [`Connection::send`] does, with one difference in ordering: the in-flight
    /// table is updated before the bytes are written, not after. A client can
    /// read a `RESPONSE` the moment its last byte is out and reuse the id at
    /// once, and the reader must find the id released by then. The frame is
    /// encoded first, so a frame that cannot be encoded (over `max_frame`)
    /// leaves the table and the connection as they were; a failed write
    /// finishes the connection, so the early update cannot be observed.
    ///
    /// Not cancel safe once writing has begun: the caller bounds the call with
    /// a deadline and then calls [`ConnectionWriter::abandon`].
    pub async fn send(&mut self, frame: Frame) -> Result<(), ConnectionError> {
        self.shared.ensure_open()?;
        cbor::check_deterministic(&frame.body)?;
        let (bytes, finishes) = {
            let mut tracker = self.shared.lock();
            let transition = tracker
                .check_outgoing(&frame)
                .map_err(ConnectionError::Usage)?;
            let bytes = encode_frame(&frame, self.max_frame, self.compress)
                .map_err(ConnectionError::Frame)?;
            tracker.apply(frame.request_id, &transition);
            let finishes = matches!(transition, Transition::Finish);
            if finishes {
                self.shared.finish();
            }
            (bytes, finishes)
        };
        self.put(&bytes).await?;
        if finishes {
            self.closed = true;
            let _ = self.write.shutdown().await;
        }
        Ok(())
    }

    /// Writes a frame the reader returned as [`Incoming::Reply`] (a `PONG` or
    /// an `overloaded` `RESPONSE`). The in-flight table is not consulted or
    /// changed: the refused request was never tracked.
    pub async fn answer(&mut self, frame: Frame) -> Result<(), ConnectionError> {
        self.shared.ensure_open()?;
        let bytes =
            encode_frame(&frame, self.max_frame, self.compress).map_err(ConnectionError::Frame)?;
        self.put(&bytes).await
    }

    /// Sends a `CLOSE` with `code` and `message` (best effort, bounded by one
    /// second), shuts the write side down and finishes the connection. Unlike
    /// [`Connection::close`] it still writes when the reader has finished the
    /// connection first (a violation, an idle timeout): the reader cannot send
    /// the `CLOSE` it asks for, so this half does. It does nothing after an
    /// earlier `close`, a sent `CLOSE`, a failed write or [`abandon`].
    ///
    /// [`abandon`]: ConnectionWriter::abandon
    pub async fn close(&mut self, code: CloseCode, message: &str) {
        if self.closed || self.dead {
            return;
        }
        self.closed = true;
        self.shared.finish();
        best_effort_close(&mut self.write, &Close::new(code, message)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{encode_frame, MAX_FRAME};
    use serde::Deserialize;
    use tokio::io::{duplex, DuplexStream};

    /// The CBOR `null`, a valid body for any frame.
    const NULL: [u8; 1] = [0xf6];
    const LONG: Duration = Duration::from_secs(5);
    const EXT_A: &str = "x/a/1";
    const EXT_B: &str = "x/b/1";

    fn fr(frame_type: FrameType, request_id: u64, body: &[u8]) -> Frame {
        Frame {
            frame_type,
            request_id,
            body: body.to_vec(),
        }
    }

    /// Encoded bytes of a frame.
    fn enc(frame_type: FrameType, request_id: u64, body: &[u8]) -> Vec<u8> {
        encode_frame(&fr(frame_type, request_id, body), MAX_FRAME, false).unwrap()
    }

    /// Raw frame bytes with arbitrary header fields.
    fn raw(version: u8, ty: u8, flags: u8, id: u64, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&((11 + body.len()) as u32).to_be_bytes());
        v.push(version);
        v.push(ty);
        v.push(flags);
        v.extend_from_slice(&id.to_be_bytes());
        v.extend_from_slice(body);
        v
    }

    fn hello_of(extensions: &[&str]) -> Hello {
        Hello {
            versions: vec![1],
            max_frame: MAX_FRAME,
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            agent: None,
        }
    }

    fn local_cfg(max_inflight: u32, extensions: &[&str]) -> LocalConfig {
        LocalConfig {
            versions: vec![1],
            max_frame: MAX_FRAME,
            max_inflight,
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            agent: None,
        }
    }

    fn ack_of(max_inflight: u32, extensions: &[&str]) -> HelloAck {
        HelloAck {
            version: 1,
            max_frame: MAX_FRAME,
            max_inflight,
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            agent: None,
        }
    }

    /// Both ends are real connections; the handshakes run side by side.
    async fn connected(
        local: LocalConfig,
        hello: Hello,
    ) -> (Connection<DuplexStream>, Connection<DuplexStream>) {
        let (a, b) = duplex(65_536);
        let (c, s) = tokio::join!(
            Connection::client_handshake(a, hello, LONG),
            Connection::server_handshake(b, &local, LONG)
        );
        (c.unwrap(), s.unwrap())
    }

    /// A real server and a scripted raw client. The `HELLO` is written first
    /// (it fits the buffer), so the handshake needs no second task.
    async fn server_pair(
        local: LocalConfig,
        hello: Hello,
    ) -> (Connection<DuplexStream>, DuplexStream) {
        let (mut peer, ours) = duplex(65_536);
        write_frame(
            &mut peer,
            &hello.to_frame().unwrap(),
            PRE_HELLO_MAX_FRAME,
            false,
        )
        .await
        .unwrap();
        let conn = Connection::server_handshake(ours, &local, LONG)
            .await
            .unwrap();
        let ack = read_frame(&mut peer, MAX_FRAME).await.unwrap().unwrap();
        assert_eq!(ack.frame_type, FrameType::HelloAck);
        (conn, peer)
    }

    /// A real client and a scripted raw server. The `HELLO_ACK` is written
    /// first; the `HELLO` the client sent is read and checked afterwards.
    async fn client_pair(hello: Hello, ack: HelloAck) -> (Connection<DuplexStream>, DuplexStream) {
        let (mut peer, ours) = duplex(65_536);
        write_frame(
            &mut peer,
            &ack.to_frame().unwrap(),
            PRE_HELLO_MAX_FRAME,
            false,
        )
        .await
        .unwrap();
        let conn = Connection::client_handshake(ours, hello, LONG)
            .await
            .unwrap();
        let sent = read_frame(&mut peer, MAX_FRAME).await.unwrap().unwrap();
        assert_eq!(sent.frame_type, FrameType::Hello);
        (conn, peer)
    }

    /// Reads the next frame from the peer, which must be a `CLOSE` with `code`.
    async fn expect_close(peer: &mut DuplexStream, code: CloseCode) -> Close {
        let f = read_frame(peer, MAX_FRAME).await.unwrap().unwrap();
        assert_eq!(f.frame_type, FrameType::Close);
        let close = Close::from_frame(&f).unwrap();
        assert_eq!(close.code, code.as_str());
        close
    }

    #[derive(Deserialize)]
    struct OkProbe {
        ok: u32,
    }

    #[derive(Deserialize)]
    struct ErrInner {
        code: String,
        message: String,
    }

    #[derive(Deserialize)]
    struct ErrProbe {
        err: ErrInner,
    }

    #[test]
    fn connection_is_send_and_static() {
        fn assert_send<T: Send + 'static>() {}
        assert_send::<Connection<DuplexStream>>();
        assert_send::<ConnectionReader<DuplexStream>>();
        assert_send::<ConnectionWriter<DuplexStream>>();
    }

    #[test]
    fn response_bodies_have_the_spec_keys() {
        let ok = response_ok_body(&42u32).unwrap();
        // {"ok": 42}
        assert_eq!(ok, vec![0xa1, 0x62, b'o', b'k', 0x18, 0x2a]);
        let err = response_err_body("overloaded", "m").unwrap();
        let probe: ErrProbe = cbor::from_slice(&err).unwrap();
        assert_eq!(probe.err.code, "overloaded");
        assert_eq!(probe.err.message, "m");
    }

    #[test]
    fn ext_index_reads_the_head() {
        assert_eq!(ext_index(&[0x83, 0x00, 0x00, 0xf6]), Some(0));
        assert_eq!(ext_index(&[0x83, 0x17, 0x00, 0xf6]), Some(23));
        assert_eq!(ext_index(&[0x83, 0x18, 0x40, 0x00, 0xf6]), Some(64));
        assert_eq!(ext_index(&[0x83, 0x19, 0x01, 0x00, 0x00, 0xf6]), Some(256));
        assert_eq!(ext_index(&[0x82, 0x00, 0x00]), None);
        assert_eq!(ext_index(&[0x83, 0x20, 0x00, 0xf6]), None);
        assert_eq!(ext_index(&[0xf6]), None);
        assert_eq!(ext_index(&[]), None);
    }

    #[tokio::test]
    async fn happy_path_request_response_ping_pong() {
        let (mut c, mut s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        assert_eq!(c.role(), Role::Client);
        assert_eq!(s.role(), Role::Server);
        assert_eq!(c.version(), 1);
        assert_eq!(c.max_frame(), MAX_FRAME);
        assert_eq!(c.max_inflight(), 1);
        assert!(c.extensions().is_empty());

        // PING first; the server answers it while it waits for the request.
        c.send(fr(FrameType::Ping, 7, &[0x07])).await.unwrap();
        let id = c.request(vec![0xa0]).await.unwrap();
        assert_eq!(id, 1);
        assert_eq!(c.in_flight_len(), 1);

        let got = s.recv().await.unwrap().unwrap();
        assert_eq!(got.frame_type, FrameType::Request);
        assert_eq!(got.request_id, 1);
        assert_eq!(got.body, vec![0xa0]);
        assert_eq!(s.in_flight_len(), 1);

        s.respond_ok(1, &42u32).await.unwrap();
        assert_eq!(s.in_flight_len(), 0);

        let pong = c.recv().await.unwrap().unwrap();
        assert_eq!(pong.frame_type, FrameType::Pong);
        assert_eq!(pong.request_id, 7);
        assert_eq!(pong.body, vec![0x07]);

        let resp = c.recv().await.unwrap().unwrap();
        assert_eq!(resp.frame_type, FrameType::Response);
        assert_eq!(resp.request_id, 1);
        let probe: OkProbe = cbor::from_slice(&resp.body).unwrap();
        assert_eq!(probe.ok, 42);
        assert_eq!(c.in_flight_len(), 0);
    }

    #[tokio::test]
    async fn extensions_are_negotiated_and_exposed() {
        let (c, s) = connected(
            local_cfg(1, &[EXT_B, EXT_A]),
            hello_of(&[EXT_A, "x/c/1", EXT_B]),
        )
        .await;
        // The client's order, filtered to what the server supports.
        assert_eq!(c.extensions(), &[EXT_A.to_string(), EXT_B.to_string()]);
        assert_eq!(s.extensions(), c.extensions());
    }

    #[tokio::test]
    async fn compression_round_trips() {
        let (mut c, mut s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        c.set_compression(true);
        // A byte string of 5000 zeros: 0x59 0x13 0x88 then the bytes.
        let mut body = vec![0x59, 0x13, 0x88];
        body.resize(3 + 5000, 0);
        let id = c.request(body.clone()).await.unwrap();
        let got = s.recv().await.unwrap().unwrap();
        assert_eq!(got.request_id, id);
        assert_eq!(got.body, body);
    }

    #[tokio::test]
    async fn two_requests_in_flight_answered_out_of_order() {
        let (mut c, mut s) = connected(local_cfg(2, &[]), hello_of(&[])).await;
        let a = c.request(NULL.to_vec()).await.unwrap();
        let b = c.request(NULL.to_vec()).await.unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(c.in_flight_len(), 2);

        let first = s.recv().await.unwrap().unwrap();
        let second = s.recv().await.unwrap().unwrap();
        assert_eq!((first.request_id, second.request_id), (1, 2));
        assert_eq!(s.in_flight_len(), 2);

        s.respond_ok(2, &2u32).await.unwrap();
        assert_eq!(s.in_flight_len(), 1);
        s.respond_ok(1, &1u32).await.unwrap();
        assert_eq!(s.in_flight_len(), 0);

        let r1 = c.recv().await.unwrap().unwrap();
        let r2 = c.recv().await.unwrap().unwrap();
        assert_eq!((r1.request_id, r2.request_id), (2, 1));
        assert_eq!(c.in_flight_len(), 0);
    }

    #[tokio::test]
    async fn max_inflight_plus_one_is_overloaded_and_the_connection_survives() {
        let (mut c, mut s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        let a = c.request(NULL.to_vec()).await.unwrap();
        let b = c.request(NULL.to_vec()).await.unwrap();
        // A cancel for request 1 gives the server a frame to return after it
        // has answered request 2.
        c.send(fr(FrameType::StreamCancel, a, &[0xa0]))
            .await
            .unwrap();

        let first = s.recv().await.unwrap().unwrap();
        assert_eq!(first.request_id, a);
        let cancel = s.recv().await.unwrap().unwrap();
        assert_eq!(cancel.frame_type, FrameType::StreamCancel);
        assert_eq!(cancel.request_id, a);
        assert_eq!(s.overloaded_count(), 1);
        assert_eq!(s.in_flight_len(), 1);

        // The overloaded answer for request 2 is already on the wire.
        let over = c.recv().await.unwrap().unwrap();
        assert_eq!(over.frame_type, FrameType::Response);
        assert_eq!(over.request_id, b);
        let probe: ErrProbe = cbor::from_slice(&over.body).unwrap();
        assert_eq!(probe.err.code, ERR_OVERLOADED);
        assert_eq!(c.in_flight_len(), 1);

        // Still usable: answer request 1, then run a third request.
        s.respond_ok(a, &1u32).await.unwrap();
        let r1 = c.recv().await.unwrap().unwrap();
        assert_eq!(r1.request_id, a);
        let third = c.request(NULL.to_vec()).await.unwrap();
        let got = s.recv().await.unwrap().unwrap();
        assert_eq!(got.request_id, third);
        s.respond_ok(third, &3u32).await.unwrap();
        let r3 = c.recv().await.unwrap().unwrap();
        assert_eq!(r3.request_id, third);
        assert_eq!(c.in_flight_len(), 0);
        assert_eq!(s.in_flight_len(), 0);
    }

    #[tokio::test]
    async fn hello_deadline_expires_on_the_server() {
        let (mut peer, ours) = duplex(65_536);
        let err =
            Connection::server_handshake(ours, &LocalConfig::default(), Duration::from_millis(100))
                .await
                .unwrap_err();
        assert!(matches!(err, ConnectionError::Timeout));
        expect_close(&mut peer, CloseCode::Timeout).await;
    }

    #[tokio::test]
    async fn hello_ack_deadline_expires_on_the_client() {
        let (mut peer, ours) = duplex(65_536);
        let err = Connection::client_handshake(ours, hello_of(&[]), Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectionError::Timeout));
        let sent = read_frame(&mut peer, MAX_FRAME).await.unwrap().unwrap();
        assert_eq!(sent.frame_type, FrameType::Hello);
        expect_close(&mut peer, CloseCode::Timeout).await;
    }

    #[tokio::test]
    async fn recv_timeout_closes_an_idle_peer_with_timeout() {
        let (mut c, mut s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        let err = s
            .recv_timeout(Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectionError::Timeout));
        assert!(s.is_finished());
        let close = loop {
            match c.recv().await {
                Err(ConnectionError::Closed(close)) => break close,
                Ok(Some(_)) => {}
                other => panic!("expected a CLOSE, got {other:?}"),
            }
        };
        assert_eq!(close.code, CloseCode::Timeout.as_str());
    }

    #[tokio::test]
    async fn recv_timeout_restarts_on_a_ping_and_still_delivers_requests() {
        let (mut c, mut s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        let ping = Frame {
            frame_type: FrameType::Ping,
            request_id: 5,
            body: cbor::to_vec(&7u64).unwrap(),
        };
        let sender = tokio::spawn(async move {
            // A ping at 150 ms and a request at 300 ms: more than the 200 ms
            // deadline in total, less than it between frames.
            tokio::time::sleep(Duration::from_millis(150)).await;
            c.send(ping).await.unwrap();
            tokio::time::sleep(Duration::from_millis(150)).await;
            let id = c.request(NULL.to_vec()).await.unwrap();
            (c, id)
        });
        let frame = s
            .recv_timeout(Duration::from_millis(200))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame.frame_type, FrameType::Request);
        let (_c, sent) = sender.await.unwrap();
        assert_eq!(frame.request_id, sent);
    }

    #[tokio::test]
    async fn first_frame_that_is_not_hello_is_a_violation() {
        let (mut peer, ours) = duplex(65_536);
        peer.write_all(&enc(FrameType::Ping, 1, &[0x01]))
            .await
            .unwrap();
        let err = Connection::server_handshake(ours, &LocalConfig::default(), LONG)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectionError::Violation(CloseCode::ProtocolViolation, _)
        ));
        expect_close(&mut peer, CloseCode::ProtocolViolation).await;
    }

    #[tokio::test]
    async fn first_frame_with_a_bad_length_gets_frame_too_large() {
        let (mut peer, ours) = duplex(65_536);
        // Above the pre-HELLO limit of 65,536.
        peer.write_all(&70_000u32.to_be_bytes()).await.unwrap();
        let err = Connection::server_handshake(ours, &LocalConfig::default(), LONG)
            .await
            .unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::FrameTooLarge));
        expect_close(&mut peer, CloseCode::FrameTooLarge).await;
    }

    #[tokio::test]
    async fn hello_with_no_common_version_gets_unsupported_version() {
        let (mut peer, ours) = duplex(65_536);
        let hello = Hello {
            versions: vec![2],
            max_frame: MAX_FRAME,
            extensions: Vec::new(),
            agent: None,
        };
        write_frame(&mut peer, &hello.to_frame().unwrap(), MAX_FRAME, false)
            .await
            .unwrap();
        let err = Connection::server_handshake(ours, &LocalConfig::default(), LONG)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectionError::Violation(CloseCode::UnsupportedVersion, _)
        ));
        let close = expect_close(&mut peer, CloseCode::UnsupportedVersion).await;
        assert_eq!(close.supported, Some(vec![1]));
    }

    #[tokio::test]
    async fn client_surfaces_a_close_in_place_of_hello_ack() {
        let (mut peer, ours) = duplex(65_536);
        let close = Close::unsupported_version("no", vec![3]);
        write_frame(&mut peer, &close.to_frame().unwrap(), MAX_FRAME, false)
            .await
            .unwrap();
        let err = Connection::client_handshake(ours, hello_of(&[]), LONG)
            .await
            .unwrap_err();
        match err {
            ConnectionError::Closed(c) => {
                assert_eq!(c.code, "unsupported_version");
                assert_eq!(c.supported, Some(vec![3]));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn client_rejects_an_ack_that_was_not_offered() {
        let (mut peer, ours) = duplex(65_536);
        let ack = ack_of(1, &["x/never/1"]);
        write_frame(&mut peer, &ack.to_frame().unwrap(), MAX_FRAME, false)
            .await
            .unwrap();
        let err = Connection::client_handshake(ours, hello_of(&[]), LONG)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectionError::Violation(CloseCode::ProtocolViolation, _)
        ));
        let sent = read_frame(&mut peer, MAX_FRAME).await.unwrap().unwrap();
        assert_eq!(sent.frame_type, FrameType::Hello);
        expect_close(&mut peer, CloseCode::ProtocolViolation).await;
    }

    /// Writes `bytes` to a server that has finished its handshake, expects
    /// the error with `code`, the matching `CLOSE`, and a finished connection.
    async fn assert_server_rejects(name: &str, bytes: Vec<u8>, code: CloseCode) {
        let (mut conn, mut peer) = server_pair(local_cfg(2, &[EXT_A]), hello_of(&[EXT_A])).await;
        peer.write_all(&bytes).await.unwrap();
        let result = tokio::time::timeout(LONG, conn.recv())
            .await
            .unwrap_or_else(|_| panic!("{name}: recv hung"));
        let err = result.unwrap_err();
        assert_eq!(err.close_code(), Some(code), "{name}: {err:?}");
        expect_close(&mut peer, code).await;
        assert!(conn.is_finished(), "{name}");
        assert!(matches!(conn.recv().await, Err(ConnectionError::Unusable)));
        assert!(matches!(
            conn.send(fr(FrameType::Pong, 1, &[0x01])).await,
            Err(ConnectionError::Unusable)
        ));
    }

    #[tokio::test]
    async fn server_closes_on_every_c7_violation_row() {
        let bad_cbor = [0x18, 0x01];
        let cases: Vec<(&str, Vec<u8>, CloseCode)> = vec![
            (
                "length above the maximum",
                0xffff_ffffu32.to_be_bytes().to_vec(),
                CloseCode::FrameTooLarge,
            ),
            (
                "length below 11",
                5u32.to_be_bytes().to_vec(),
                CloseCode::FrameTooLarge,
            ),
            (
                "bad version",
                raw(2, 0x10, 0, 1, &NULL),
                CloseCode::UnsupportedVersion,
            ),
            (
                "reserved flag bit",
                raw(1, 0x10, 0x02, 1, &NULL),
                CloseCode::MalformedFrame,
            ),
            (
                "compressed body too short",
                raw(1, 0x10, 0x01, 1, &[0, 0, 0]),
                CloseCode::MalformedFrame,
            ),
            (
                "unallocated type",
                raw(1, 0x30, 0, 1, &NULL),
                CloseCode::UnknownFrameType,
            ),
            (
                "type zero",
                raw(1, 0x00, 0, 1, &NULL),
                CloseCode::UnknownFrameType,
            ),
            (
                "request id zero",
                raw(1, 0x10, 0, 0, &NULL),
                CloseCode::ProtocolViolation,
            ),
            (
                "response sent to a server",
                enc(FrameType::Response, 1, &NULL),
                CloseCode::ProtocolViolation,
            ),
            (
                "stream start sent to a server",
                enc(FrameType::StreamStart, 1, &NULL),
                CloseCode::ProtocolViolation,
            ),
            (
                "second hello",
                hello_of(&[])
                    .to_frame()
                    .map(|f| encode_frame(&f, MAX_FRAME, false).unwrap())
                    .unwrap(),
                CloseCode::ProtocolViolation,
            ),
            (
                "hello ack sent to a server",
                enc(FrameType::HelloAck, 0, &NULL),
                CloseCode::ProtocolViolation,
            ),
            (
                "ext index out of range",
                enc(FrameType::Ext, 0, &[0x83, 0x01, 0x00, 0xf6]),
                CloseCode::ProtocolViolation,
            ),
            (
                "ext body is not an array",
                enc(FrameType::Ext, 0, &NULL),
                CloseCode::ProtocolViolation,
            ),
            (
                "body not in shortest form",
                enc(FrameType::Request, 1, &bad_cbor),
                CloseCode::MalformedFrame,
            ),
            (
                "ping token is not an integer",
                enc(FrameType::Ping, 1, &[0x61, 0x61]),
                CloseCode::ProtocolViolation,
            ),
        ];
        for (name, bytes, code) in cases {
            assert_server_rejects(name, bytes, code).await;
        }
    }

    #[tokio::test]
    async fn server_closes_on_a_reused_in_flight_id() {
        let (mut conn, mut peer) = server_pair(local_cfg(4, &[]), hello_of(&[])).await;
        peer.write_all(&enc(FrameType::Request, 5, &NULL))
            .await
            .unwrap();
        peer.write_all(&enc(FrameType::Request, 5, &NULL))
            .await
            .unwrap();
        let first = conn.recv().await.unwrap().unwrap();
        assert_eq!(first.request_id, 5);
        let err = conn.recv().await.unwrap_err();
        assert!(matches!(
            err,
            ConnectionError::Violation(CloseCode::ProtocolViolation, _)
        ));
        expect_close(&mut peer, CloseCode::ProtocolViolation).await;
        assert!(matches!(conn.recv().await, Err(ConnectionError::Unusable)));
    }

    #[tokio::test]
    async fn server_accepts_an_id_again_after_the_response() {
        let (mut conn, mut peer) = server_pair(local_cfg(1, &[]), hello_of(&[])).await;
        peer.write_all(&enc(FrameType::Request, 5, &NULL))
            .await
            .unwrap();
        conn.recv().await.unwrap().unwrap();
        conn.respond_ok(5, &1u32).await.unwrap();
        peer.write_all(&enc(FrameType::Request, 5, &NULL))
            .await
            .unwrap();
        let again = conn.recv().await.unwrap().unwrap();
        assert_eq!(again.request_id, 5);
    }

    #[tokio::test]
    async fn server_accepts_a_valid_ext_frame() {
        let (mut conn, mut peer) = server_pair(local_cfg(1, &[EXT_A]), hello_of(&[EXT_A])).await;
        peer.write_all(&enc(FrameType::Ext, 0, &[0x83, 0x00, 0x00, 0xf6]))
            .await
            .unwrap();
        let got = conn.recv().await.unwrap().unwrap();
        assert_eq!(got.frame_type, FrameType::Ext);
    }

    #[tokio::test]
    async fn stream_cancel_for_an_unknown_id_is_ignored() {
        let (mut conn, mut peer) = server_pair(local_cfg(1, &[]), hello_of(&[])).await;
        peer.write_all(&enc(FrameType::StreamCancel, 99, &[0xa0]))
            .await
            .unwrap();
        peer.write_all(&enc(FrameType::Request, 1, &NULL))
            .await
            .unwrap();
        let got = conn.recv().await.unwrap().unwrap();
        assert_eq!(got.frame_type, FrameType::Request);
        assert_eq!(got.request_id, 1);
    }

    /// Runs a scripted server against a client that has one request (id 1)
    /// in flight: writes `frames`, reads until the client errors, and checks
    /// the error, the `CLOSE` and that the connection is finished.
    async fn assert_client_rejects(name: &str, frames: Vec<Vec<u8>>, code: CloseCode) {
        let (mut conn, mut peer) = client_pair(hello_of(&[]), ack_of(1, &[])).await;
        let id = conn.request(NULL.to_vec()).await.unwrap();
        assert_eq!(id, 1);
        let sent = read_frame(&mut peer, MAX_FRAME).await.unwrap().unwrap();
        assert_eq!(sent.frame_type, FrameType::Request);
        for f in &frames {
            peer.write_all(f).await.unwrap();
        }
        let err = loop {
            let result = tokio::time::timeout(LONG, conn.recv())
                .await
                .unwrap_or_else(|_| panic!("{name}: recv hung"));
            match result {
                Ok(Some(_)) => continue,
                Ok(None) => panic!("{name}: stream ended without an error"),
                Err(e) => break e,
            }
        };
        assert_eq!(err.close_code(), Some(code), "{name}: {err:?}");
        expect_close(&mut peer, code).await;
        assert!(conn.is_finished(), "{name}");
        assert!(matches!(conn.recv().await, Err(ConnectionError::Unusable)));
    }

    #[tokio::test]
    async fn client_closes_on_every_c7_violation_row() {
        let cases: Vec<(&str, Vec<Vec<u8>>, CloseCode)> = vec![
            (
                "response for an unknown id",
                vec![enc(FrameType::Response, 7, &NULL)],
                CloseCode::ProtocolViolation,
            ),
            (
                "request sent to a client",
                vec![enc(FrameType::Request, 3, &NULL)],
                CloseCode::ProtocolViolation,
            ),
            (
                "stream cancel sent to a client",
                vec![enc(FrameType::StreamCancel, 1, &[0xa0])],
                CloseCode::ProtocolViolation,
            ),
            (
                "token before start",
                vec![enc(FrameType::StreamToken, 1, &NULL)],
                CloseCode::ProtocolViolation,
            ),
            (
                "end before start",
                vec![enc(FrameType::StreamEnd, 1, &NULL)],
                CloseCode::ProtocolViolation,
            ),
            (
                "response after stream start",
                vec![
                    enc(FrameType::StreamStart, 1, &NULL),
                    enc(FrameType::Response, 1, &NULL),
                ],
                CloseCode::ProtocolViolation,
            ),
            (
                "second stream start",
                vec![
                    enc(FrameType::StreamStart, 1, &NULL),
                    enc(FrameType::StreamStart, 1, &NULL),
                ],
                CloseCode::ProtocolViolation,
            ),
            (
                "token after the stream ended",
                vec![
                    enc(FrameType::StreamStart, 1, &NULL),
                    enc(FrameType::StreamEnd, 1, &NULL),
                    enc(FrameType::StreamToken, 1, &NULL),
                ],
                CloseCode::ProtocolViolation,
            ),
            (
                "second hello ack",
                vec![enc(FrameType::HelloAck, 0, &NULL)],
                CloseCode::ProtocolViolation,
            ),
            (
                "ext with no accepted extension",
                vec![enc(FrameType::Ext, 0, &[0x83, 0x00, 0x00, 0xf6])],
                CloseCode::ProtocolViolation,
            ),
            (
                "body not in shortest form",
                vec![enc(FrameType::Response, 1, &[0x18, 0x01])],
                CloseCode::MalformedFrame,
            ),
            (
                "unallocated type",
                vec![raw(1, 0x30, 0, 1, &NULL)],
                CloseCode::UnknownFrameType,
            ),
            (
                "bad version",
                vec![raw(2, 0x11, 0, 1, &NULL)],
                CloseCode::UnsupportedVersion,
            ),
        ];
        for (name, frames, code) in cases {
            assert_client_rejects(name, frames, code).await;
        }
    }

    #[tokio::test]
    async fn stream_start_tokens_cancel_and_end() {
        let (mut conn, mut peer) = client_pair(hello_of(&[]), ack_of(1, &[])).await;
        let id = conn.request(NULL.to_vec()).await.unwrap();
        let sent = read_frame(&mut peer, MAX_FRAME).await.unwrap().unwrap();
        assert_eq!(sent.request_id, id);

        peer.write_all(&enc(FrameType::StreamStart, id, &[0xa0]))
            .await
            .unwrap();
        peer.write_all(&enc(FrameType::StreamToken, id, &[0x00]))
            .await
            .unwrap();
        let start = conn.recv().await.unwrap().unwrap();
        assert_eq!(start.frame_type, FrameType::StreamStart);
        assert_eq!(conn.in_flight_len(), 1);
        let t0 = conn.recv().await.unwrap().unwrap();
        assert_eq!(t0.frame_type, FrameType::StreamToken);

        // Cancel mid-stream. The id stays in flight until STREAM_END.
        conn.send(fr(FrameType::StreamCancel, id, &[0xa0]))
            .await
            .unwrap();
        assert_eq!(conn.in_flight_len(), 1);
        let cancel = read_frame(&mut peer, MAX_FRAME).await.unwrap().unwrap();
        assert_eq!(cancel.frame_type, FrameType::StreamCancel);
        assert_eq!(cancel.request_id, id);

        // A token already in flight still arrives, then the end.
        peer.write_all(&enc(FrameType::StreamToken, id, &[0x01]))
            .await
            .unwrap();
        peer.write_all(&enc(FrameType::StreamEnd, id, &[0xa0]))
            .await
            .unwrap();
        let t1 = conn.recv().await.unwrap().unwrap();
        assert_eq!(t1.frame_type, FrameType::StreamToken);
        assert_eq!(conn.in_flight_len(), 1);
        let end = conn.recv().await.unwrap().unwrap();
        assert_eq!(end.frame_type, FrameType::StreamEnd);
        assert_eq!(conn.in_flight_len(), 0);
    }

    #[tokio::test]
    async fn server_side_stream_order_is_enforced_when_sending() {
        let (mut c, mut s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        let id = c.request(NULL.to_vec()).await.unwrap();
        s.recv().await.unwrap().unwrap();

        // A token before the start is refused locally, nothing is written.
        let err = s
            .send(fr(FrameType::StreamToken, id, &NULL))
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectionError::Usage(_)));
        assert!(!s.is_finished());

        s.send(fr(FrameType::StreamStart, id, &NULL)).await.unwrap();
        s.send(fr(FrameType::StreamToken, id, &NULL)).await.unwrap();
        // No RESPONSE once streaming.
        assert!(matches!(
            s.respond_ok(id, &1u32).await,
            Err(ConnectionError::Usage(_))
        ));
        s.send(fr(FrameType::StreamEnd, id, &NULL)).await.unwrap();
        assert_eq!(s.in_flight_len(), 0);

        for expected in [
            FrameType::StreamStart,
            FrameType::StreamToken,
            FrameType::StreamEnd,
        ] {
            let f = c.recv().await.unwrap().unwrap();
            assert_eq!(f.frame_type, expected);
        }
        assert_eq!(c.in_flight_len(), 0);
    }

    #[tokio::test]
    async fn misuse_is_refused_without_closing() {
        let (mut c, mut s) = connected(local_cfg(2, &[]), hello_of(&[])).await;
        // Wrong direction and first-frame types.
        for t in [
            FrameType::Hello,
            FrameType::HelloAck,
            FrameType::Response,
            FrameType::StreamStart,
        ] {
            let err = c.send(fr(t, 1, &NULL)).await.unwrap_err();
            assert!(matches!(err, ConnectionError::Usage(_)), "{t:?}");
        }
        assert!(matches!(
            s.send(fr(FrameType::Request, 1, &NULL)).await,
            Err(ConnectionError::Usage(_))
        ));
        assert!(matches!(
            s.request(NULL.to_vec()).await,
            Err(ConnectionError::Usage(_))
        ));
        // A response for a request that is not in flight.
        assert!(matches!(
            s.respond_ok(9, &1u32).await,
            Err(ConnectionError::Usage(_))
        ));
        // A body that is not deterministic CBOR.
        assert!(matches!(
            c.send(fr(FrameType::Request, 1, &[0x18, 0x01])).await,
            Err(ConnectionError::Cbor(_))
        ));
        // A reused id, and id zero.
        c.send(fr(FrameType::Request, 1, &NULL)).await.unwrap();
        assert!(matches!(
            c.send(fr(FrameType::Request, 1, &NULL)).await,
            Err(ConnectionError::Usage(_))
        ));
        assert!(matches!(
            c.send(fr(FrameType::Request, 0, &NULL)).await,
            Err(ConnectionError::Frame(_))
        ));
        // An EXT that names no accepted extension.
        assert!(matches!(
            c.send(fr(FrameType::Ext, 0, &[0x83, 0x00, 0x00, 0xf6]))
                .await,
            Err(ConnectionError::Usage(_))
        ));
        // Both ends are still usable.
        assert!(!c.is_finished());
        assert!(!s.is_finished());
        let got = s.recv().await.unwrap().unwrap();
        assert_eq!(got.request_id, 1);
        assert_eq!(c.in_flight_len(), 1);
    }

    #[tokio::test]
    async fn received_close_surfaces_and_refuses_further_use() {
        let (mut conn, mut peer) = client_pair(hello_of(&[]), ack_of(1, &[])).await;
        let close = Close::new(CloseCode::ShuttingDown, "bye");
        write_frame(&mut peer, &close.to_frame().unwrap(), MAX_FRAME, false)
            .await
            .unwrap();
        match conn.recv().await {
            Err(ConnectionError::Closed(c)) => {
                assert_eq!(c.code, "shutting_down");
                assert_eq!(c.message, "bye");
            }
            other => panic!("unexpected result: {other:?}"),
        }
        assert!(conn.is_finished());
        assert!(matches!(conn.recv().await, Err(ConnectionError::Unusable)));
        assert!(matches!(
            conn.request(NULL.to_vec()).await,
            Err(ConnectionError::Unusable)
        ));
    }

    #[tokio::test]
    async fn close_sends_the_code_and_finishes() {
        let (mut c, mut s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        s.close(CloseCode::ShuttingDown, "going away").await;
        assert!(s.is_finished());
        match c.recv().await {
            Err(ConnectionError::Closed(close)) => assert_eq!(close.code, "shutting_down"),
            other => panic!("unexpected result: {other:?}"),
        }
        assert!(matches!(
            s.send(fr(FrameType::Ping, 1, &[0x01])).await,
            Err(ConnectionError::Unusable)
        ));
    }

    #[tokio::test]
    async fn clean_end_of_stream_is_none_then_unusable() {
        let (mut conn, peer) = client_pair(hello_of(&[]), ack_of(1, &[])).await;
        drop(peer);
        assert!(conn.recv().await.unwrap().is_none());
        assert!(matches!(conn.recv().await, Err(ConnectionError::Unusable)));
    }

    #[tokio::test]
    async fn id_table_does_not_leak_over_100_exchanges() {
        let (mut c, mut s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        for i in 0..100u64 {
            let id = c.request(NULL.to_vec()).await.unwrap();
            assert_eq!(id, i + 1);
            let got = s.recv().await.unwrap().unwrap();
            assert_eq!(got.request_id, id);
            if i % 2 == 0 {
                s.respond_ok(id, &(i as u32)).await.unwrap();
                let r = c.recv().await.unwrap().unwrap();
                assert_eq!(r.frame_type, FrameType::Response);
            } else {
                s.send(fr(FrameType::StreamStart, id, &NULL)).await.unwrap();
                s.send(fr(FrameType::StreamEnd, id, &NULL)).await.unwrap();
                let a = c.recv().await.unwrap().unwrap();
                let b = c.recv().await.unwrap().unwrap();
                assert_eq!(a.frame_type, FrameType::StreamStart);
                assert_eq!(b.frame_type, FrameType::StreamEnd);
            }
            assert_eq!(c.in_flight_len(), 0, "client, iteration {i}");
            assert_eq!(s.in_flight_len(), 0, "server, iteration {i}");
        }
        assert_eq!(s.overloaded_count(), 0);
    }
    // ---- the split halves ----

    /// The next thing the reader hands over, under a deadline.
    async fn read_one(reader: &mut ConnectionReader<DuplexStream>) -> Incoming {
        tokio::time::timeout(LONG, reader.recv_timeout(LONG))
            .await
            .expect("reader deadline")
            .expect("reader error")
            .expect("end of stream")
    }

    /// The next frame a sequential connection receives, under a deadline.
    async fn recv_one(conn: &mut Connection<DuplexStream>) -> Frame {
        tokio::time::timeout(LONG, conn.recv())
            .await
            .expect("recv deadline")
            .expect("recv error")
            .expect("end of stream")
    }

    fn delivered(incoming: Incoming) -> Frame {
        match incoming {
            Incoming::Frame(frame) => frame,
            Incoming::Reply(frame) => panic!("expected a delivered frame, got reply {frame:?}"),
        }
    }

    fn reply_of(incoming: Incoming) -> Frame {
        match incoming {
            Incoming::Reply(frame) => frame,
            Incoming::Frame(frame) => panic!("expected a reply, got delivered {frame:?}"),
        }
    }

    fn ok_response(id: u64, value: u32) -> Frame {
        Frame {
            frame_type: FrameType::Response,
            request_id: id,
            body: response_ok_body(&value).unwrap(),
        }
    }

    #[tokio::test]
    async fn split_keeps_the_state_the_connection_had() {
        let (mut c, mut s) = connected(local_cfg(3, &[]), hello_of(&[])).await;
        let id = c.request(NULL.to_vec()).await.unwrap();
        let got = recv_one(&mut s).await;
        assert_eq!(got.request_id, id);
        let (reader, mut writer) = s.split();
        assert_eq!(reader.max_inflight(), 3);
        assert_eq!(reader.in_flight_len(), 1);
        assert_eq!(writer.in_flight_len(), 1);
        writer.send(ok_response(id, 7)).await.unwrap();
        assert_eq!(reader.in_flight_len(), 0);
        let answer = recv_one(&mut c).await;
        assert_eq!(answer.frame_type, FrameType::Response);
        assert_eq!(answer.request_id, id);
    }

    #[tokio::test]
    async fn a_ping_is_answered_through_the_writer_while_a_request_is_pending() {
        let (mut c, s) = connected(local_cfg(2, &[]), hello_of(&[])).await;
        let (mut reader, mut writer) = s.split();
        let id = c.request(NULL.to_vec()).await.unwrap();
        let request = delivered(read_one(&mut reader).await);
        assert_eq!(request.frame_type, FrameType::Request);
        assert_eq!(request.request_id, id);

        // The request has no answer yet, as if its command were still running.
        c.send(fr(FrameType::Ping, 77, &[0x05])).await.unwrap();
        let pong = reply_of(read_one(&mut reader).await);
        assert_eq!(pong.frame_type, FrameType::Pong);
        assert_eq!(pong.request_id, 77);
        assert_eq!(pong.body, vec![0x05]);
        writer.answer(pong).await.unwrap();
        let first = recv_one(&mut c).await;
        assert_eq!(first.frame_type, FrameType::Pong);
        assert_eq!(first.request_id, 77);

        writer.send(ok_response(id, 1)).await.unwrap();
        let second = recv_one(&mut c).await;
        assert_eq!(second.frame_type, FrameType::Response);
        assert_eq!(second.request_id, id);
    }

    #[tokio::test]
    async fn a_request_over_max_inflight_is_refused_while_another_is_pending() {
        let (mut c, s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        let (mut reader, mut writer) = s.split();
        let a = c.request(NULL.to_vec()).await.unwrap();
        let b = c.request(NULL.to_vec()).await.unwrap();
        assert_eq!(delivered(read_one(&mut reader).await).request_id, a);
        let refusal = reply_of(read_one(&mut reader).await);
        assert_eq!(refusal.frame_type, FrameType::Response);
        assert_eq!(refusal.request_id, b);
        assert_eq!(reader.overloaded_count(), 1);
        assert_eq!(reader.in_flight_len(), 1);
        writer.answer(refusal).await.unwrap();

        let over = recv_one(&mut c).await;
        assert_eq!(over.request_id, b);
        let probe: ErrProbe = cbor::from_slice(&over.body).unwrap();
        assert_eq!(probe.err.code, ERR_OVERLOADED);

        // The refused id was never tracked, so the writer will not send for it.
        assert!(matches!(
            writer.send(ok_response(b, 0)).await,
            Err(ConnectionError::Usage(_))
        ));
        writer.send(ok_response(a, 1)).await.unwrap();
        assert_eq!(recv_one(&mut c).await.request_id, a);
        assert_eq!(reader.in_flight_len(), 0);
    }

    #[tokio::test]
    async fn an_id_is_free_again_as_soon_as_its_response_is_readable() {
        let (mut c, s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        let (mut reader, mut writer) = s.split();
        for round in 0..50u32 {
            c.send(fr(FrameType::Request, 1, &NULL)).await.unwrap();
            assert_eq!(delivered(read_one(&mut reader).await).request_id, 1);
            writer.send(ok_response(1, round)).await.unwrap();
            let answer = recv_one(&mut c).await;
            assert_eq!(answer.request_id, 1);
            // The client reuses id 1 at once in the next round; the reader
            // must not see it as still in flight.
        }
        assert_eq!(reader.in_flight_len(), 0);
    }

    #[tokio::test]
    async fn the_idle_deadline_waits_while_a_request_is_in_flight() {
        let (mut c, s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        let (mut reader, mut writer) = s.split();
        let id = c.request(NULL.to_vec()).await.unwrap();
        assert_eq!(delivered(read_one(&mut reader).await).request_id, id);

        // With a request in flight the short idle period never ends the
        // connection: the outer deadline is what ends this wait.
        let waited = tokio::time::timeout(
            Duration::from_millis(400),
            reader.recv_timeout(Duration::from_millis(50)),
        )
        .await;
        assert!(waited.is_err(), "the reader must keep waiting while busy");
        assert!(!reader.is_finished());

        // Once the answer is out nothing is in flight and the deadline applies.
        writer.send(ok_response(id, 1)).await.unwrap();
        let idle = tokio::time::timeout(LONG, reader.recv_timeout(Duration::from_millis(50)))
            .await
            .expect("the idle deadline must fire when nothing is in flight");
        let error = idle.unwrap_err();
        assert!(matches!(error, ConnectionError::Timeout));
        assert_eq!(error.close_to_send().unwrap().code, "timeout");
        assert!(reader.is_finished());
        assert!(writer.is_finished());
    }

    #[tokio::test]
    async fn a_violation_is_reported_for_the_writer_to_answer() {
        let (server, mut peer) = server_pair(local_cfg(4, &[]), hello_of(&[])).await;
        let (mut reader, mut writer) = server.split();
        peer.write_all(&enc(FrameType::Request, 1, &NULL))
            .await
            .unwrap();
        peer.write_all(&enc(FrameType::Request, 1, &NULL))
            .await
            .unwrap();
        assert_eq!(delivered(read_one(&mut reader).await).request_id, 1);
        let error = tokio::time::timeout(LONG, reader.recv_timeout(LONG))
            .await
            .expect("reader deadline")
            .unwrap_err();
        assert_eq!(error.close_code(), Some(CloseCode::ProtocolViolation));
        assert!(reader.is_finished());
        assert!(matches!(
            reader.recv_timeout(LONG).await,
            Err(ConnectionError::Unusable)
        ));
        // The reader cannot write, so the CLOSE it asks for goes through the
        // writer even though the connection is already finished.
        let close = error.close_to_send().unwrap();
        writer
            .close(close.close_code().unwrap(), close.message.as_str())
            .await;
        let frame = tokio::time::timeout(LONG, read_frame(&mut peer, MAX_FRAME))
            .await
            .expect("peer deadline")
            .unwrap()
            .unwrap();
        assert_eq!(frame.frame_type, FrameType::Close);
        assert_eq!(
            Close::from_frame(&frame).unwrap().code,
            "protocol_violation"
        );
    }

    #[tokio::test]
    async fn a_close_sent_through_the_writer_ends_the_peer() {
        let (mut c, s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        let (_reader, mut writer) = s.split();
        writer
            .close(CloseCode::ShuttingDown, "the server is shutting down")
            .await;
        match tokio::time::timeout(LONG, c.recv())
            .await
            .expect("deadline")
        {
            Err(ConnectionError::Closed(close)) => assert_eq!(close.code, "shutting_down"),
            other => panic!("expected CLOSE(shutting_down), got {other:?}"),
        }
        assert!(matches!(
            writer.send(ok_response(1, 0)).await,
            Err(ConnectionError::Unusable)
        ));
    }

    #[tokio::test]
    async fn a_response_over_max_frame_leaves_the_request_in_flight() {
        let small = Hello {
            versions: vec![1],
            max_frame: 1024,
            extensions: Vec::new(),
            agent: None,
        };
        let (mut c, s) = connected(local_cfg(1, &[]), small).await;
        let (mut reader, mut writer) = s.split();
        let id = c.request(NULL.to_vec()).await.unwrap();
        assert_eq!(delivered(read_one(&mut reader).await).request_id, id);
        let big = "x".repeat(4096);
        let too_large = Frame {
            frame_type: FrameType::Response,
            request_id: id,
            body: response_ok_body(&big).unwrap(),
        };
        assert!(matches!(
            writer.send(too_large).await,
            Err(ConnectionError::Frame(_))
        ));
        assert!(!writer.is_finished());
        assert_eq!(writer.in_flight_len(), 1);
        writer.send(ok_response(id, 1)).await.unwrap();
        assert_eq!(recv_one(&mut c).await.request_id, id);
    }

    #[tokio::test]
    async fn end_of_stream_finishes_both_halves() {
        let (c, s) = connected(local_cfg(1, &[]), hello_of(&[])).await;
        let (mut reader, writer) = s.split();
        drop(c);
        let end = tokio::time::timeout(LONG, reader.recv_timeout(LONG))
            .await
            .expect("reader deadline")
            .unwrap();
        assert!(end.is_none());
        assert!(reader.is_finished());
        assert!(writer.is_finished());
    }
}
