//! spine-wire v1 frame codec (`docs/spec/00-wire.md`, sections C.1 to C.4 and C.7).
//!
//! One frame, all integers big-endian:
//!
//! ```text
//! | length : u32 | version : u8 | type : u8 | flags : u8 | request id : u64 | body |
//! ```
//!
//! `length = 11 + len(body)`, that is, the count of bytes after the length
//! field. Flag bit 0 means the body is `u32_be uncompressed_len || zstd frame`.
//!
//! This layer is stateless apart from the limits the caller passes in. It
//! checks the length, version, type, flags, the compressed-body rules and the
//! request-id rules that hold for a frame type unconditionally (`HELLO`,
//! `HELLO_ACK` and `CLOSE` carry 0; `REQUEST` and `PING` carry a nonzero id).
//!
//! It does **not** check anything that depends on connection state: id
//! uniqueness among in-flight requests, direction (which side may send which
//! type), `HELLO` being first, extension indexes, or the maximum in force
//! (the caller passes [`PRE_HELLO_MAX_FRAME`] before `HELLO` has been processed
//! and the negotiated limit afterwards).
//!
//! Frame bodies are opaque bytes here. Whether a body is one well-formed,
//! deterministic CBOR item is left to the caller, which owns that rule and the
//! `malformed_frame` close that follows from it.
//!
//! Every function that can reject a frame returns a [`FrameError`], and
//! [`FrameError::close_code`] gives the code the caller should send in `CLOSE`.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The only wire version this codec speaks.
pub const VERSION: u8 = 1;
/// Bytes of the header that follow the length field: version, type, flags and
/// the request id.
pub const HEADER_AFTER_LENGTH: usize = 11;
/// Total header size: the 4-byte length plus [`HEADER_AFTER_LENGTH`].
pub const HEADER_LEN: usize = 15;
/// Default and absolute ceiling for `length`.
pub const MAX_FRAME: u32 = 16_777_216;
/// Ceiling for `length` before `HELLO` has been processed.
pub const PRE_HELLO_MAX_FRAME: u32 = 65_536;
/// Flag bit 0: the body is compressed.
pub const FLAG_COMPRESSED: u8 = 1;
/// Mask of the flag bits that MUST be zero.
pub const FLAGS_RESERVED_MASK: u8 = 0xfe;
/// zstd window limit, as a base-2 logarithm (2^23 bytes).
pub const ZSTD_WINDOW_LOG_MAX: u32 = 23;
/// zstd compression level used by the sender.
pub const ZSTD_LEVEL: i32 = 3;
/// Sender policy (C.4): do not compress a body smaller than this many bytes.
pub const COMPRESS_THRESHOLD: usize = 1024;
/// Size of the chunks in which [`read_frame`] reads a body.
pub const READ_CHUNK: usize = 64 * 1024;

/// A frame type (C.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameType {
    /// Client's first frame.
    Hello = 0x01,
    /// Server's first frame.
    HelloAck = 0x02,
    /// Either side; the sender closes after sending.
    Close = 0x03,
    /// Liveness probe.
    Ping = 0x04,
    /// Answer to a `PING`.
    Pong = 0x05,
    /// A command from the client.
    Request = 0x10,
    /// The server's answer to a request.
    Response = 0x11,
    /// Start of a stream.
    StreamStart = 0x20,
    /// One stream element.
    StreamToken = 0x21,
    /// End of a stream.
    StreamEnd = 0x22,
    /// The client cancels a stream.
    StreamCancel = 0x23,
    /// Extension frame.
    Ext = 0x80,
}

impl FrameType {
    /// The wire value of this type.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for FrameType {
    type Error = FrameError;

    fn try_from(value: u8) -> Result<Self, FrameError> {
        match value {
            0x01 => Ok(FrameType::Hello),
            0x02 => Ok(FrameType::HelloAck),
            0x03 => Ok(FrameType::Close),
            0x04 => Ok(FrameType::Ping),
            0x05 => Ok(FrameType::Pong),
            0x10 => Ok(FrameType::Request),
            0x11 => Ok(FrameType::Response),
            0x20 => Ok(FrameType::StreamStart),
            0x21 => Ok(FrameType::StreamToken),
            0x22 => Ok(FrameType::StreamEnd),
            0x23 => Ok(FrameType::StreamCancel),
            0x80 => Ok(FrameType::Ext),
            other => Err(FrameError::UnknownFrameType(other)),
        }
    }
}

/// A `CLOSE` code (C.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseCode {
    /// Bad `version`, or no supported version offered.
    UnsupportedVersion,
    /// Unallocated frame type, or `0x00`.
    UnknownFrameType,
    /// Reserved flag bits, a bad compressed body, or a body that is not valid CBOR.
    MalformedFrame,
    /// `length` out of range, or a declared uncompressed length over the limit.
    FrameTooLarge,
    /// A well-formed frame that breaks a protocol rule.
    ProtocolViolation,
    /// No `HELLO` in time, or idle too long.
    Timeout,
    /// The server is shutting down.
    ShuttingDown,
}

impl CloseCode {
    /// The exact text carried in the `code` field of a `CLOSE` body.
    pub fn as_str(self) -> &'static str {
        match self {
            CloseCode::UnsupportedVersion => "unsupported_version",
            CloseCode::UnknownFrameType => "unknown_frame_type",
            CloseCode::MalformedFrame => "malformed_frame",
            CloseCode::FrameTooLarge => "frame_too_large",
            CloseCode::ProtocolViolation => "protocol_violation",
            CloseCode::Timeout => "timeout",
            CloseCode::ShuttingDown => "shutting_down",
        }
    }
}

/// Why a frame was rejected, or why it could not be read.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// `length` is below 11.
    #[error("frame length {length} is below the minimum of 11")]
    LengthTooSmall {
        /// The declared (or computed) length.
        length: u64,
    },
    /// `length` is above the maximum in force.
    #[error("frame length {length} exceeds the maximum of {max}")]
    LengthTooLarge {
        /// The declared (or computed) length.
        length: u64,
        /// The maximum in force.
        max: u32,
    },
    /// The version byte is not 1.
    #[error("unsupported frame version {0}")]
    UnsupportedVersion(u8),
    /// The type byte is 0x00 or unallocated.
    #[error("unknown frame type {0:#04x}")]
    UnknownFrameType(u8),
    /// Flag bits 1 to 7 are set.
    #[error("reserved flag bits set: {0:#04x}")]
    ReservedFlags(u8),
    /// A request id that is wrong for the frame type regardless of connection state.
    #[error("request id {request_id} is not allowed on a {frame_type:?} frame")]
    BadRequestId {
        /// The frame type.
        frame_type: FrameType,
        /// The offending id.
        request_id: u64,
    },
    /// A compressed body shorter than its 4-byte length prefix.
    #[error("compressed body is shorter than 4 bytes")]
    CompressedBodyTooShort,
    /// The declared uncompressed length exceeds `max_frame - 11`.
    #[error("declared uncompressed length {declared} exceeds the limit of {limit}")]
    UncompressedTooLarge {
        /// The declared uncompressed length.
        declared: u32,
        /// The limit (`max_frame - 11`).
        limit: u32,
    },
    /// The decompressed output length differs from the declared length.
    #[error("decompressed length {actual} differs from declared length {declared}")]
    UncompressedLengthMismatch {
        /// The declared uncompressed length.
        declared: u32,
        /// The output length found; at most `declared + 1` because output is bounded.
        actual: u64,
    },
    /// The zstd stream is invalid.
    #[error("invalid zstd body: {0}")]
    Decompress(String),
    /// The input ended inside a frame (`decode_frame` and `read_frame`).
    #[error("input ended in the middle of a frame")]
    Truncated,
    /// `decode_frame` needs more bytes before it can decide.
    #[error("incomplete frame: at least {needed} more bytes needed")]
    NeedMore {
        /// A lower bound on the number of further bytes required.
        needed: usize,
    },
    /// An I/O error other than a truncated frame.
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

impl FrameError {
    /// The `CLOSE` code the caller should send, or `None` when the error is not
    /// a protocol rejection (`Truncated`, `NeedMore`, `Io`): there is nothing
    /// to tell a peer that has gone away.
    pub fn close_code(&self) -> Option<CloseCode> {
        match self {
            FrameError::LengthTooSmall { .. }
            | FrameError::LengthTooLarge { .. }
            | FrameError::UncompressedTooLarge { .. } => Some(CloseCode::FrameTooLarge),
            FrameError::UnsupportedVersion(_) => Some(CloseCode::UnsupportedVersion),
            FrameError::UnknownFrameType(_) => Some(CloseCode::UnknownFrameType),
            FrameError::ReservedFlags(_)
            | FrameError::CompressedBodyTooShort
            | FrameError::UncompressedLengthMismatch { .. }
            | FrameError::Decompress(_) => Some(CloseCode::MalformedFrame),
            FrameError::BadRequestId { .. } => Some(CloseCode::ProtocolViolation),
            FrameError::Truncated | FrameError::NeedMore { .. } | FrameError::Io(_) => None,
        }
    }

    /// True when the peer's stream ended inside a frame.
    pub fn is_truncated(&self) -> bool {
        matches!(self, FrameError::Truncated)
    }
}

/// One decoded frame. The body is the uncompressed bytes, opaque to this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The frame type.
    pub frame_type: FrameType,
    /// The request id from the header.
    pub request_id: u64,
    /// The body after any decompression.
    pub body: Vec<u8>,
}

/// Request-id rules that hold for a type whatever the connection state.
fn check_request_id(frame_type: FrameType, request_id: u64) -> Result<(), FrameError> {
    let ok = match frame_type {
        FrameType::Hello | FrameType::HelloAck | FrameType::Close => request_id == 0,
        FrameType::Request | FrameType::Ping => request_id != 0,
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(FrameError::BadRequestId {
            frame_type,
            request_id,
        })
    }
}

/// The ceiling in force: never above the absolute maximum.
fn effective_max(max_frame: u32) -> u32 {
    max_frame.min(MAX_FRAME)
}

/// Check a declared `length` against the minimum and the maximum.
fn check_length(length: u32, max_frame: u32) -> Result<(), FrameError> {
    if (length as usize) < HEADER_AFTER_LENGTH {
        return Err(FrameError::LengthTooSmall {
            length: u64::from(length),
        });
    }
    let max = effective_max(max_frame);
    if length > max {
        return Err(FrameError::LengthTooLarge {
            length: u64::from(length),
            max,
        });
    }
    Ok(())
}

/// Parse and check the 11 fixed header bytes that follow the length.
fn parse_header(h: &[u8]) -> Result<(FrameType, u8, u64), FrameError> {
    if h[0] != VERSION {
        return Err(FrameError::UnsupportedVersion(h[0]));
    }
    let frame_type = FrameType::try_from(h[1])?;
    let flags = h[2];
    if flags & FLAGS_RESERVED_MASK != 0 {
        return Err(FrameError::ReservedFlags(flags));
    }
    let mut id = [0u8; 8];
    id.copy_from_slice(&h[3..11]);
    let request_id = u64::from_be_bytes(id);
    check_request_id(frame_type, request_id)?;
    Ok((frame_type, flags, request_id))
}

/// Decompress a compressed body (`u32_be uncompressed_len || zstd frame`).
fn decompress_body(body: &[u8], max_frame: u32) -> Result<Vec<u8>, FrameError> {
    use std::io::Read;

    if body.len() < 4 {
        return Err(FrameError::CompressedBodyTooShort);
    }
    let declared = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    let limit = effective_max(max_frame).saturating_sub(HEADER_AFTER_LENGTH as u32);
    if declared > limit {
        return Err(FrameError::UncompressedTooLarge { declared, limit });
    }
    let mut decoder = zstd::stream::read::Decoder::new(&body[4..])
        .map_err(|e| FrameError::Decompress(e.to_string()))?;
    decoder
        .window_log_max(ZSTD_WINDOW_LOG_MAX)
        .map_err(|e| FrameError::Decompress(e.to_string()))?;
    let mut output = Vec::new();
    decoder
        .take(u64::from(declared) + 1)
        .read_to_end(&mut output)
        .map_err(|e| FrameError::Decompress(e.to_string()))?;
    if output.len() as u64 != u64::from(declared) {
        return Err(FrameError::UncompressedLengthMismatch {
            declared,
            actual: output.len() as u64,
        });
    }
    Ok(output)
}

/// Turn the raw body of a received frame into the final body.
fn finish_body(flags: u8, raw: Vec<u8>, max_frame: u32) -> Result<Vec<u8>, FrameError> {
    if flags & FLAG_COMPRESSED != 0 {
        decompress_body(&raw, max_frame)
    } else {
        Ok(raw)
    }
}

/// Encode one frame.
///
/// The frame's uncompressed length (`11 + body.len()`) must not exceed
/// `max_frame` (itself capped at [`MAX_FRAME`]), because the receiver checks
/// the declared uncompressed length against that bound. With `compress` true
/// the C.4 sender policy applies: bodies of at least [`COMPRESS_THRESHOLD`]
/// bytes are compressed and the compressed form is kept only if it is smaller.
/// The stateless request-id rules are enforced here as well.
pub fn encode_frame(frame: &Frame, max_frame: u32, compress: bool) -> Result<Vec<u8>, FrameError> {
    check_request_id(frame.frame_type, frame.request_id)?;
    let max = effective_max(max_frame);
    let plain_length = (HEADER_AFTER_LENGTH as u64) + (frame.body.len() as u64);
    if plain_length > u64::from(max) {
        return Err(FrameError::LengthTooLarge {
            length: plain_length,
            max,
        });
    }

    let mut flags = 0u8;
    let mut compressed: Option<Vec<u8>> = None;
    if compress && frame.body.len() >= COMPRESS_THRESHOLD {
        let z = zstd::stream::encode_all(&frame.body[..], ZSTD_LEVEL)?;
        if 4 + z.len() < frame.body.len() {
            let mut b = Vec::with_capacity(4 + z.len());
            // plain_length <= MAX_FRAME, so the body length fits in u32.
            b.extend_from_slice(&(frame.body.len() as u32).to_be_bytes());
            b.extend_from_slice(&z);
            compressed = Some(b);
            flags |= FLAG_COMPRESSED;
        }
    }
    let body: &[u8] = match &compressed {
        Some(b) => b,
        None => &frame.body,
    };

    let length = (HEADER_AFTER_LENGTH + body.len()) as u32;
    let mut out = Vec::with_capacity(4 + HEADER_AFTER_LENGTH + body.len());
    out.extend_from_slice(&length.to_be_bytes());
    out.push(VERSION);
    out.push(frame.frame_type.as_u8());
    out.push(flags);
    out.extend_from_slice(&frame.request_id.to_be_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

/// Decode one frame from the start of `bytes`.
///
/// Returns the frame and the number of bytes consumed. If `bytes` does not yet
/// hold a whole frame the result is [`FrameError::NeedMore`], but a bad length,
/// version, type, flag or request id is reported as soon as those bytes are
/// present, without waiting for the body.
pub fn decode_frame(bytes: &[u8], max_frame: u32) -> Result<(Frame, usize), FrameError> {
    if bytes.len() < 4 {
        return Err(FrameError::NeedMore {
            needed: 4 - bytes.len(),
        });
    }
    let length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    check_length(length, max_frame)?;
    if bytes.len() < HEADER_LEN {
        return Err(FrameError::NeedMore {
            needed: HEADER_LEN - bytes.len(),
        });
    }
    let (frame_type, flags, request_id) = parse_header(&bytes[4..HEADER_LEN])?;
    let total = 4 + length as usize;
    if bytes.len() < total {
        return Err(FrameError::NeedMore {
            needed: total - bytes.len(),
        });
    }
    let body = finish_body(flags, bytes[HEADER_LEN..total].to_vec(), max_frame)?;
    Ok((
        Frame {
            frame_type,
            request_id,
            body,
        },
        total,
    ))
}

/// Read until `buf` is full or the stream ends; returns the bytes read.
async fn read_fill<R: AsyncRead + Unpin>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = r.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

/// Like `read_exact`, but a short stream is [`FrameError::Truncated`].
async fn read_exact_frame<R: AsyncRead + Unpin>(
    r: &mut R,
    buf: &mut [u8],
) -> Result<(), FrameError> {
    let n = read_fill(r, buf).await?;
    if n < buf.len() {
        return Err(FrameError::Truncated);
    }
    Ok(())
}

/// Read one frame from a stream.
///
/// Returns `Ok(None)` when the stream ends cleanly before the first byte of a
/// frame, and [`FrameError::Truncated`] when it ends inside one. The length is
/// validated after reading only the 4 length bytes, and the version, type,
/// flags and request id are validated before any body byte is read. The body
/// is read in [`READ_CHUNK`] pieces, so memory grows with the bytes that
/// actually arrive, not with the declared length.
pub async fn read_frame<R: AsyncRead + Unpin>(
    r: &mut R,
    max_frame: u32,
) -> Result<Option<Frame>, FrameError> {
    let mut len_buf = [0u8; 4];
    let n = read_fill(r, &mut len_buf).await?;
    if n == 0 {
        return Ok(None);
    }
    if n < 4 {
        return Err(FrameError::Truncated);
    }
    let length = u32::from_be_bytes(len_buf);
    check_length(length, max_frame)?;

    let mut header = [0u8; HEADER_AFTER_LENGTH];
    read_exact_frame(r, &mut header).await?;
    let (frame_type, flags, request_id) = parse_header(&header)?;

    let mut remaining = length as usize - HEADER_AFTER_LENGTH;
    let mut raw: Vec<u8> = Vec::new();
    while remaining > 0 {
        let chunk = remaining.min(READ_CHUNK);
        let start = raw.len();
        raw.resize(start + chunk, 0);
        read_exact_frame(r, &mut raw[start..]).await?;
        remaining -= chunk;
    }
    let body = finish_body(flags, raw, max_frame)?;
    Ok(Some(Frame {
        frame_type,
        request_id,
        body,
    }))
}

/// Encode a frame (see [`encode_frame`]), write it and flush.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    frame: &Frame,
    max_frame: u32,
    compress: bool,
) -> Result<(), FrameError> {
    let bytes = encode_frame(frame, max_frame, compress)?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::ReadBuf;

    const HELLO_HEX: &str = "00 00 00 32 01 01 00 00 00 00 00 00 00 00 00 a3 68 76 65 72 73 69 6f 6e 73 81 01 69 6d 61 78 5f 66 72 61 6d 65 1a 01 00 00 00 6a 65 78 74 65 6e 73 69 6f 6e 73 80";
    const REQUEST_HEX: &str = "00 00 00 3d 01 10 00 00 00 00 00 00 00 00 01 a1 69 74 6f 6f 6c 5f 63 61 6c 6c a2 64 61 72 67 73 a1 64 74 65 78 74 62 68 69 64 6e 61 6d 65 72 73 70 69 6e 65 2e 63 6f 6e 74 65 6e 74 2f 68 61 73 68";
    const PING_HEX: &str = "00 00 00 0d 01 04 00 00 00 00 00 00 00 00 07 18 2a";

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect()
    }

    /// A reader that yields at most `chunk` bytes per poll and, if
    /// `panic_at_end`, panics when polled after its data is used up.
    struct ScriptReader {
        data: Vec<u8>,
        pos: usize,
        chunk: usize,
        panic_at_end: bool,
    }

    impl ScriptReader {
        fn new(data: Vec<u8>, chunk: usize, panic_at_end: bool) -> Self {
            ScriptReader {
                data,
                pos: 0,
                chunk,
                panic_at_end,
            }
        }
    }

    impl AsyncRead for ScriptReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let this = self.get_mut();
            if this.pos >= this.data.len() {
                if this.panic_at_end {
                    panic!("reader polled after its data was used up");
                }
                return Poll::Ready(Ok(()));
            }
            let n = buf
                .remaining()
                .min(this.chunk)
                .min(this.data.len() - this.pos);
            buf.put_slice(&this.data[this.pos..this.pos + n]);
            this.pos += n;
            Poll::Ready(Ok(()))
        }
    }

    fn frame(t: FrameType, id: u64, body: &[u8]) -> Frame {
        Frame {
            frame_type: t,
            request_id: id,
            body: body.to_vec(),
        }
    }

    /// Build raw frame bytes with arbitrary header fields.
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

    fn compressible(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 7) as u8).collect()
    }

    fn zstd_of(data: &[u8]) -> Vec<u8> {
        zstd::stream::encode_all(data, ZSTD_LEVEL).unwrap()
    }

    fn compressed_body(declared: u32, data: &[u8]) -> Vec<u8> {
        let mut b = declared.to_be_bytes().to_vec();
        b.extend_from_slice(&zstd_of(data));
        b
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn spec_vector_hello_round_trips() {
        let bytes = hex(HELLO_HEX);
        assert_eq!(bytes.len(), 54);
        let (f, used) = decode_frame(&bytes, MAX_FRAME).unwrap();
        assert_eq!(used, 54);
        assert_eq!(f.frame_type, FrameType::Hello);
        assert_eq!(f.request_id, 0);
        assert_eq!(f.body, bytes[15..].to_vec());
        assert_eq!(f.body.len(), 39);
        assert_eq!(encode_frame(&f, MAX_FRAME, false).unwrap(), bytes);
    }

    #[test]
    fn spec_vector_request_round_trips() {
        let bytes = hex(REQUEST_HEX);
        assert_eq!(bytes.len(), 65);
        let (f, used) = decode_frame(&bytes, MAX_FRAME).unwrap();
        assert_eq!(used, 65);
        assert_eq!(f.frame_type, FrameType::Request);
        assert_eq!(f.request_id, 1);
        assert_eq!(f.body.len(), 50);
        assert_eq!(encode_frame(&f, MAX_FRAME, false).unwrap(), bytes);
    }

    #[test]
    fn ping_header_layout() {
        let expected: [u8; 17] = [
            0x00, 0x00, 0x00, 0x0d, // length = 13
            0x01, // version
            0x04, // PING
            0x00, // flags
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, // request id 7
            0x18, 0x2a, // CBOR 42
        ];
        assert_eq!(hex(PING_HEX), expected.to_vec());
        let f = frame(FrameType::Ping, 7, &[0x18, 0x2a]);
        assert_eq!(
            encode_frame(&f, MAX_FRAME, false).unwrap(),
            expected.to_vec()
        );
        let (g, used) = decode_frame(&expected, MAX_FRAME).unwrap();
        assert_eq!(used, 17);
        assert_eq!(g, f);
    }

    #[test]
    fn constants_match_spec() {
        assert_eq!(HEADER_LEN, 4 + HEADER_AFTER_LENGTH);
        assert_eq!(MAX_FRAME, 16 * 1024 * 1024);
        assert_eq!(PRE_HELLO_MAX_FRAME, 64 * 1024);
    }

    #[test]
    fn close_code_text() {
        assert_eq!(
            CloseCode::UnsupportedVersion.as_str(),
            "unsupported_version"
        );
        assert_eq!(CloseCode::UnknownFrameType.as_str(), "unknown_frame_type");
        assert_eq!(CloseCode::MalformedFrame.as_str(), "malformed_frame");
        assert_eq!(CloseCode::FrameTooLarge.as_str(), "frame_too_large");
        assert_eq!(CloseCode::ProtocolViolation.as_str(), "protocol_violation");
        assert_eq!(CloseCode::Timeout.as_str(), "timeout");
        assert_eq!(CloseCode::ShuttingDown.as_str(), "shutting_down");
    }

    #[test]
    fn frame_type_values() {
        let all = [
            (0x01u8, FrameType::Hello),
            (0x02, FrameType::HelloAck),
            (0x03, FrameType::Close),
            (0x04, FrameType::Ping),
            (0x05, FrameType::Pong),
            (0x10, FrameType::Request),
            (0x11, FrameType::Response),
            (0x20, FrameType::StreamStart),
            (0x21, FrameType::StreamToken),
            (0x22, FrameType::StreamEnd),
            (0x23, FrameType::StreamCancel),
            (0x80, FrameType::Ext),
        ];
        for (v, t) in all {
            assert_eq!(t.as_u8(), v);
            assert_eq!(FrameType::try_from(v).unwrap(), t);
        }
        let mut allocated = 0;
        for v in 0..=255u8 {
            if FrameType::try_from(v).is_ok() {
                allocated += 1;
            }
        }
        assert_eq!(allocated, all.len());
    }

    #[test]
    fn length_below_minimum_reads_no_more_than_four_bytes() {
        for len in [0u32, 1, 10] {
            let mut r = ScriptReader::new(len.to_be_bytes().to_vec(), 4, true);
            let err = block_on(read_frame(&mut r, MAX_FRAME)).unwrap_err();
            assert!(
                matches!(err, FrameError::LengthTooSmall { length } if length == u64::from(len))
            );
            assert_eq!(err.close_code(), Some(CloseCode::FrameTooLarge));
        }
    }

    #[test]
    fn length_above_maximum_reads_no_more_than_four_bytes() {
        let mut r = ScriptReader::new(1025u32.to_be_bytes().to_vec(), 4, true);
        let err = block_on(read_frame(&mut r, 1024)).unwrap_err();
        assert!(matches!(
            err,
            FrameError::LengthTooLarge {
                length: 1025,
                max: 1024
            }
        ));
        assert_eq!(err.close_code(), Some(CloseCode::FrameTooLarge));
    }

    #[test]
    fn length_u32_max_is_rejected_without_allocating() {
        let input = [0xffu8; 4];
        let mut r = &input[..];
        let err = block_on(read_frame(&mut r, MAX_FRAME)).unwrap_err();
        assert!(matches!(err, FrameError::LengthTooLarge { .. }));
        let err = decode_frame(&input, MAX_FRAME).unwrap_err();
        assert!(matches!(err, FrameError::LengthTooLarge { .. }));
    }

    #[test]
    fn pre_hello_limit_is_enforced() {
        let len = PRE_HELLO_MAX_FRAME + 1;
        let mut r = ScriptReader::new(len.to_be_bytes().to_vec(), 4, true);
        let err = block_on(read_frame(&mut r, PRE_HELLO_MAX_FRAME)).unwrap_err();
        assert!(matches!(err, FrameError::LengthTooLarge { .. }));
    }

    #[test]
    fn max_frame_is_capped_at_the_absolute_ceiling() {
        let len = MAX_FRAME + 1;
        let err = decode_frame(&len.to_be_bytes(), u32::MAX).unwrap_err();
        assert!(matches!(
            err,
            FrameError::LengthTooLarge { max: MAX_FRAME, .. }
        ));
    }

    #[test]
    fn bad_version_is_rejected_before_the_body() {
        for v in [0u8, 2, 255] {
            let mut data = raw(v, 0x04, 0, 7, &[]);
            data.truncate(HEADER_LEN);
            // length claims an 11-byte frame, so no body follows anyway; make it larger.
            data[3] = 20;
            let mut r = ScriptReader::new(data, 64, true);
            let err = block_on(read_frame(&mut r, MAX_FRAME)).unwrap_err();
            assert!(matches!(err, FrameError::UnsupportedVersion(x) if x == v));
            assert_eq!(err.close_code(), Some(CloseCode::UnsupportedVersion));
        }
    }

    #[test]
    fn unknown_and_zero_types_are_rejected() {
        for t in [0x00u8, 0x06, 0x0f, 0x12, 0x24, 0x7f, 0x81, 0xff] {
            let bytes = raw(1, t, 0, 1, &[0xf6]);
            let err = decode_frame(&bytes, MAX_FRAME).unwrap_err();
            assert!(matches!(err, FrameError::UnknownFrameType(x) if x == t));
            assert_eq!(err.close_code(), Some(CloseCode::UnknownFrameType));
        }
    }

    #[test]
    fn each_reserved_flag_bit_is_rejected() {
        for bit in 1..8u8 {
            let flags = 1u8 << bit;
            let bytes = raw(1, 0x10, flags, 1, &[0xf6]);
            let err = decode_frame(&bytes, MAX_FRAME).unwrap_err();
            assert!(matches!(err, FrameError::ReservedFlags(x) if x == flags));
            assert_eq!(err.close_code(), Some(CloseCode::MalformedFrame));
            let mut r = &bytes[..];
            let err = block_on(read_frame(&mut r, MAX_FRAME)).unwrap_err();
            assert!(matches!(err, FrameError::ReservedFlags(x) if x == flags));
        }
        // Reserved bit together with the compressed bit is still rejected.
        let bytes = raw(1, 0x10, 0x03, 1, &[0xf6]);
        assert!(matches!(
            decode_frame(&bytes, MAX_FRAME).unwrap_err(),
            FrameError::ReservedFlags(3)
        ));
    }

    #[test]
    fn stateless_request_id_rules() {
        let bad = [
            (FrameType::Hello, 1u64),
            (FrameType::HelloAck, 1),
            (FrameType::Close, 9),
            (FrameType::Request, 0),
            (FrameType::Ping, 0),
        ];
        for (t, id) in bad {
            let f = frame(t, id, &[0xf6]);
            let err = encode_frame(&f, MAX_FRAME, false).unwrap_err();
            assert!(matches!(err, FrameError::BadRequestId { .. }));
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
            let bytes = raw(1, t.as_u8(), 0, id, &[0xf6]);
            let err = decode_frame(&bytes, MAX_FRAME).unwrap_err();
            assert!(matches!(err, FrameError::BadRequestId { .. }));
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
        }
        let good = [
            (FrameType::Hello, 0u64),
            (FrameType::HelloAck, 0),
            (FrameType::Close, 0),
            (FrameType::Request, 1),
            (FrameType::Ping, u64::MAX),
            (FrameType::Pong, 7),
            (FrameType::Response, 0),
            (FrameType::StreamToken, 5),
            (FrameType::Ext, 0),
        ];
        for (t, id) in good {
            let f = frame(t, id, &[0xf6]);
            let bytes = encode_frame(&f, MAX_FRAME, false).unwrap();
            assert_eq!(decode_frame(&bytes, MAX_FRAME).unwrap().0, f);
        }
    }

    #[test]
    fn compressed_round_trip() {
        let body = compressible(100_000);
        let f = frame(FrameType::Request, 3, &body);
        let bytes = encode_frame(&f, MAX_FRAME, true).unwrap();
        assert_eq!(bytes[6], FLAG_COMPRESSED);
        assert!(bytes.len() < 10_000);
        let declared = u32::from_be_bytes([bytes[15], bytes[16], bytes[17], bytes[18]]);
        assert_eq!(declared as usize, body.len());
        let (g, used) = decode_frame(&bytes, MAX_FRAME).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(g, f);
        let mut r = &bytes[..];
        assert_eq!(block_on(read_frame(&mut r, MAX_FRAME)).unwrap().unwrap(), f);
    }

    #[test]
    fn small_or_incompressible_bodies_stay_plain() {
        let small = frame(FrameType::Request, 1, &compressible(COMPRESS_THRESHOLD - 1));
        assert_eq!(encode_frame(&small, MAX_FRAME, true).unwrap()[6], 0);
        // Pseudo-random bytes do not shrink, so the plain form is kept.
        let mut x: u32 = 12345;
        let noise: Vec<u8> = (0..4096)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 24) as u8
            })
            .collect();
        let f = frame(FrameType::Request, 1, &noise);
        let bytes = encode_frame(&f, MAX_FRAME, true).unwrap();
        assert_eq!(bytes[6], 0);
        assert_eq!(bytes.len(), 15 + 4096);
        // compress = false never compresses.
        let big = frame(FrameType::Request, 1, &compressible(50_000));
        assert_eq!(encode_frame(&big, MAX_FRAME, false).unwrap()[6], 0);
    }

    #[test]
    fn encode_refuses_a_frame_over_the_limit() {
        let f = frame(FrameType::Request, 1, &vec![0u8; 1014]);
        // 11 + 1014 = 1025 > 1024.
        let err = encode_frame(&f, 1024, false).unwrap_err();
        assert!(matches!(
            err,
            FrameError::LengthTooLarge {
                length: 1025,
                max: 1024
            }
        ));
        // Compression does not get around the bound on the uncompressed length.
        assert!(encode_frame(&f, 1024, true).is_err());
        let ok = frame(FrameType::Request, 1, &vec![0u8; 1013]);
        assert!(encode_frame(&ok, 1024, false).is_ok());
    }

    #[test]
    fn lying_uncompressed_length_is_rejected() {
        let real = compressible(5000);
        // Declared too small.
        let b = compressed_body(4999, &real);
        let bytes = raw(1, 0x10, FLAG_COMPRESSED, 1, &b);
        let err = decode_frame(&bytes, MAX_FRAME).unwrap_err();
        assert!(matches!(
            err,
            FrameError::UncompressedLengthMismatch {
                declared: 4999,
                actual: 5000
            }
        ));
        assert_eq!(err.close_code(), Some(CloseCode::MalformedFrame));
        // Declared too large (but within the cap).
        let b = compressed_body(5001, &real);
        let bytes = raw(1, 0x10, FLAG_COMPRESSED, 1, &b);
        let err = decode_frame(&bytes, MAX_FRAME).unwrap_err();
        assert!(matches!(
            err,
            FrameError::UncompressedLengthMismatch {
                declared: 5001,
                actual: 5000
            }
        ));
        // A small declared length and a much larger true output is bounded.
        let b = compressed_body(10, &vec![0u8; 1_000_000]);
        let bytes = raw(1, 0x10, FLAG_COMPRESSED, 1, &b);
        let err = decode_frame(&bytes, MAX_FRAME).unwrap_err();
        assert!(matches!(
            err,
            FrameError::UncompressedLengthMismatch {
                declared: 10,
                actual: 11
            }
        ));
    }

    #[test]
    fn declared_length_over_the_cap_is_rejected_before_decompression() {
        // The zstd payload is garbage: if the decompressor ran, the error would be Decompress.
        let mut b = 1014u32.to_be_bytes().to_vec(); // cap for max_frame 1024 is 1013
        b.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        let bytes = raw(1, 0x10, FLAG_COMPRESSED, 1, &b);
        let err = decode_frame(&bytes, 1024).unwrap_err();
        assert!(matches!(
            err,
            FrameError::UncompressedTooLarge {
                declared: 1014,
                limit: 1013
            }
        ));
        assert_eq!(err.close_code(), Some(CloseCode::FrameTooLarge));
        let mut r = &bytes[..];
        assert!(matches!(
            block_on(read_frame(&mut r, 1024)).unwrap_err(),
            FrameError::UncompressedTooLarge { .. }
        ));
        // u32::MAX declared.
        let mut b = u32::MAX.to_be_bytes().to_vec();
        b.extend_from_slice(&[1, 2, 3]);
        let bytes = raw(1, 0x10, FLAG_COMPRESSED, 1, &b);
        assert!(matches!(
            decode_frame(&bytes, MAX_FRAME).unwrap_err(),
            FrameError::UncompressedTooLarge { .. }
        ));
    }

    #[test]
    fn compressed_body_shorter_than_four_bytes_is_rejected() {
        for n in 0..4usize {
            let bytes = raw(1, 0x10, FLAG_COMPRESSED, 1, &vec![0u8; n]);
            let err = decode_frame(&bytes, MAX_FRAME).unwrap_err();
            assert!(matches!(err, FrameError::CompressedBodyTooShort));
            assert_eq!(err.close_code(), Some(CloseCode::MalformedFrame));
        }
    }

    #[test]
    fn invalid_zstd_is_malformed() {
        let mut b = 8u32.to_be_bytes().to_vec();
        b.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef, 1, 2, 3, 4]);
        let bytes = raw(1, 0x10, FLAG_COMPRESSED, 1, &b);
        let err = decode_frame(&bytes, MAX_FRAME).unwrap_err();
        assert!(matches!(err, FrameError::Decompress(_)));
        assert_eq!(err.close_code(), Some(CloseCode::MalformedFrame));
    }

    #[test]
    fn decode_frame_reports_need_more() {
        let bytes = hex(PING_HEX);
        for n in 0..bytes.len() {
            let err = decode_frame(&bytes[..n], MAX_FRAME).unwrap_err();
            assert!(matches!(err, FrameError::NeedMore { .. }), "prefix {}", n);
            assert_eq!(err.close_code(), None);
        }
        if let FrameError::NeedMore { needed } = decode_frame(&bytes[..10], MAX_FRAME).unwrap_err()
        {
            assert_eq!(needed, 5);
        } else {
            panic!("expected NeedMore");
        }
    }

    #[test]
    fn decode_frame_leaves_trailing_bytes() {
        let mut bytes = hex(PING_HEX);
        let n = bytes.len();
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        let (_, used) = decode_frame(&bytes, MAX_FRAME).unwrap();
        assert_eq!(used, n);
    }

    #[test]
    fn clean_eof_is_none_and_truncation_is_an_error() {
        let empty: &[u8] = &[];
        let mut r = empty;
        assert!(block_on(read_frame(&mut r, MAX_FRAME)).unwrap().is_none());

        let bytes = hex(PING_HEX);
        // EOF inside the length, inside the header, inside the body.
        for cut in [1usize, 3, 4, 9, 14, 15, 16] {
            let mut r = &bytes[..cut];
            let err = block_on(read_frame(&mut r, MAX_FRAME)).unwrap_err();
            assert!(err.is_truncated(), "cut {}", cut);
            assert!(matches!(err, FrameError::Truncated));
            assert_eq!(err.close_code(), None);
        }
        let mut r = &bytes[..];
        assert!(block_on(read_frame(&mut r, MAX_FRAME)).unwrap().is_some());
        assert!(block_on(read_frame(&mut r, MAX_FRAME)).unwrap().is_none());
    }

    #[test]
    fn truncated_multi_chunk_body() {
        let body = vec![7u8; READ_CHUNK * 2 + 10];
        let f = frame(FrameType::Response, 1, &body);
        let bytes = encode_frame(&f, MAX_FRAME, false).unwrap();
        let mut r = &bytes[..bytes.len() - 1];
        let err = block_on(read_frame(&mut r, MAX_FRAME)).unwrap_err();
        assert!(err.is_truncated());
        let mut r = &bytes[..];
        assert_eq!(block_on(read_frame(&mut r, MAX_FRAME)).unwrap().unwrap(), f);
    }

    #[test]
    fn two_frames_back_to_back_over_a_duplex_stream() {
        let a = frame(FrameType::Request, 1, &[0xa1, 0x01, 0x02]);
        let b = frame(FrameType::Ping, 2, &[0x18, 0x2a]);
        let big = frame(FrameType::Response, 1, &compressible(200_000));
        block_on(async {
            let (mut tx, mut rx) = tokio::io::duplex(1024);
            let (a2, b2, big2) = (a.clone(), b.clone(), big.clone());
            let writer = tokio::spawn(async move {
                write_frame(&mut tx, &a2, MAX_FRAME, false).await.unwrap();
                write_frame(&mut tx, &b2, MAX_FRAME, true).await.unwrap();
                write_frame(&mut tx, &big2, MAX_FRAME, true).await.unwrap();
                // tx dropped here: clean EOF for the reader.
            });
            assert_eq!(read_frame(&mut rx, MAX_FRAME).await.unwrap().unwrap(), a);
            assert_eq!(read_frame(&mut rx, MAX_FRAME).await.unwrap().unwrap(), b);
            assert_eq!(read_frame(&mut rx, MAX_FRAME).await.unwrap().unwrap(), big);
            assert!(read_frame(&mut rx, MAX_FRAME).await.unwrap().is_none());
            writer.await.unwrap();
        });
    }

    #[test]
    fn a_frame_delivered_one_byte_at_a_time() {
        let f = frame(FrameType::Request, 9, &compressible(3000));
        for compress in [false, true] {
            let bytes = encode_frame(&f, MAX_FRAME, compress).unwrap();
            let mut r = ScriptReader::new(bytes, 1, false);
            let got = block_on(read_frame(&mut r, MAX_FRAME)).unwrap().unwrap();
            assert_eq!(got, f);
            assert!(block_on(read_frame(&mut r, MAX_FRAME)).unwrap().is_none());
        }
    }

    #[test]
    fn round_trip_over_sizes() {
        for size in [0usize, 1, 1023, 1024, 65_535, 65_536, 1 << 20] {
            let f = frame(FrameType::Response, 4, &compressible(size));
            for compress in [false, true] {
                let bytes = encode_frame(&f, MAX_FRAME, compress).unwrap();
                let (g, used) = decode_frame(&bytes, MAX_FRAME).unwrap();
                assert_eq!(used, bytes.len(), "size {}", size);
                assert_eq!(g, f, "size {}", size);
            }
        }
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(64))]

            #[test]
            fn decode_of_encode_is_identity(
                ty in prop::sample::select(vec![
                    FrameType::Response,
                    FrameType::StreamToken,
                    FrameType::Pong,
                    FrameType::Ext,
                ]),
                id in any::<u64>(),
                body in prop::collection::vec(any::<u8>(), 0..4096),
                repeat in 1usize..8,
                compress in any::<bool>(),
            ) {
                // Repeating the body makes some cases compressible.
                let mut b = Vec::new();
                for _ in 0..repeat {
                    b.extend_from_slice(&body);
                }
                let f = Frame { frame_type: ty, request_id: id, body: b };
                let bytes = encode_frame(&f, MAX_FRAME, compress).unwrap();
                let (g, used) = decode_frame(&bytes, MAX_FRAME).unwrap();
                prop_assert_eq!(used, bytes.len());
                prop_assert_eq!(g, f);
            }
        }
    }
}
