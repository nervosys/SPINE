#!/usr/bin/env python3
"""spine-wire v1: an independent reference implementation and the vector generator.

Written from the specification text (docs/spec/00-wire.md), not from the Rust
code, and since the reconciliation of the specification with the implementation
the two agree. Section numbers in the code comments below ("C.5 rule 2") use the
labels of the earlier document; Appendix D of the specification maps them to the
current sections. It shares nothing with `src/spine-protocol` and uses
only the Python standard library (3.8 or later). It is the "second
implementation" of requirement R14: the vectors in
`docs/spec/00-wire.vectors.json` are produced by it, and
`src/spine-protocol/tests/conformance_vectors.rs` checks the Rust crate
against them.

Usage:

    python scripts/wire_v1.py generate > docs/spec/00-wire.vectors.json
    python scripts/wire_v1.py check docs/spec/00-wire.vectors.json
    python scripts/wire_v1.py decode-body <hex>
    python scripts/wire_v1.py decode-frame <hex> [max_frame]

What it covers: the deterministic-CBOR encoder and strict validator (C.5
rules 1 to 6, each rejection with a named reason), the frame encoder and
decoder (C.1, C.3, C.4, the request-id rules that hold whatever the
connection state, the close codes of C.7), HELLO, HELLO_ACK and CLOSE bodies
and `negotiate` (C.6, C.10), and builders for REQUEST and RESPONSE bodies
(C.5 rule 7, D).

What it does not cover:

* zstd. The standard library has no zstd. A compressed body is handled by
  structure: `u32 uncompressed_len || zstd frame`, the zstd magic
  `28 b5 2f fd`, and the declared length against `max_frame - 11`. The one
  zstd feature implemented is a frame made only of raw (stored) blocks, which
  is valid zstd and which this script can both write and read; a frame with
  compressed blocks is reported as "opaque" and its length is not checked.
* Connection state: id uniqueness among in-flight requests, direction,
  "HELLO first", a second HELLO, extension indexes, timeouts, max_inflight.
  Those need a stream of frames, not one frame, and have no vectors here.
* Semantics of commands (what `navigate` does). Only the wire shape.

A "note" on a vector explains the rule it exercises and cites the section of
the specification that states it.
"""

import json
import struct
import sys

# ---------------------------------------------------------------------------
# Constants (C.1, C.3, C.6, C.7, D)
# ---------------------------------------------------------------------------

VERSION = 1
MAX_FRAME = 16_777_216
PRE_HELLO_MAX_FRAME = 65_536
MIN_MAX_FRAME = 1024
HEADER_AFTER_LENGTH = 11
HEADER_LEN = 15
MAX_DEPTH = 32
MAX_AGENT_LEN = 128
MAX_EXTENSION_ID_LEN = 128
U64_MAX = 2**64 - 1
U32_MAX = 2**32 - 1
JSON_SAFE_INT = 2**53

FRAME_TYPES = {
    "HELLO": 0x01,
    "HELLO_ACK": 0x02,
    "CLOSE": 0x03,
    "PING": 0x04,
    "PONG": 0x05,
    "REQUEST": 0x10,
    "RESPONSE": 0x11,
    "STREAM_START": 0x20,
    "STREAM_TOKEN": 0x21,
    "STREAM_END": 0x22,
    "STREAM_CANCEL": 0x23,
    "EXT": 0x80,
}
TYPE_NAMES = {code: name for name, code in FRAME_TYPES.items()}

CLOSE_CODES = [
    "unsupported_version",
    "unknown_frame_type",
    "malformed_frame",
    "frame_too_large",
    "protocol_violation",
    "timeout",
    "shutting_down",
]

# D.1 and D.2: the 17 kept commands plus tool_call and capability_query.
COMMAND_NAMES = [
    "navigate",
    "get_ur",
    "get_raw_html",
    "execute_binary",
    "store_knowledge",
    "query_knowledge",
    "get_binary_capabilities",
    "resolve_name",
    "resolve_names",
    "find_providers",
    "publish_name",
    "fetch_name",
    "crawl_names",
    "publish_resource",
    "read_resource",
    "namespace_status",
    "mesh_call",
    "tool_call",
    "capability_query",
]

# Field names of each command: (required, optional); None for a unit command.
# `execute_binary` carries a SpineBinary whose shape is not part of the wire
# layer and is not modelled here.
COMMAND_FIELDS = {
    "navigate": (["url"], []),
    "get_ur": None,
    "get_raw_html": None,
    "store_knowledge": (["key", "value", "tags"], []),
    "query_knowledge": (["query", "tags", "limit"], []),
    "get_binary_capabilities": None,
    "resolve_name": (["name"], ["if_none_match"]),
    "resolve_names": (["names"], []),
    "find_providers": (["capability"], ["limit"]),
    "publish_name": (["record"], []),
    "fetch_name": (["name"], ["if_none_match"]),
    "crawl_names": (["seed"], ["max_depth", "max_visits"]),
    "publish_resource": (["record", "content_type", "data"], []),
    "read_resource": (["hash", "offset", "max_bytes"], []),
    "namespace_status": None,
    "mesh_call": (["peer_id", "peer_key", "payload"], []),
    "tool_call": (["name", "args"], ["trace"]),
    "capability_query": (["selector"], []),
}


# ---------------------------------------------------------------------------
# Errors
# ---------------------------------------------------------------------------


class Reject(Exception):
    """A body that breaks a rule of C.5. `reason` is the Rust `CborError`
    variant name, so a conformance test can assert the exact variant."""

    def __init__(self, reason, detail=""):
        super().__init__("%s: %s" % (reason, detail) if detail else reason)
        self.reason = reason
        self.detail = detail


class FrameReject(Exception):
    """A frame that breaks a rule of C.1, C.3, C.4 or C.7. `error` is the Rust
    `FrameError` variant name and `close_code` the C.7 wire text (None when
    there is nothing to tell the peer: the input is merely incomplete)."""

    def __init__(self, error, close_code, needed=None, detail=""):
        super().__init__("%s: %s" % (error, detail) if detail else error)
        self.error = error
        self.close_code = close_code
        self.needed = needed
        self.detail = detail


class CloseReject(Exception):
    """A HELLO, HELLO_ACK or CLOSE body that fails: the CLOSE to send."""

    def __init__(self, code, why, supported=None):
        super().__init__("%s: %s" % (code, why))
        self.code = code
        self.why = why
        self.supported = supported


class Map:
    """A CBOR map whose keys are not all text strings, as ordered pairs."""

    def __init__(self, pairs):
        self.pairs = [(k, v) for k, v in pairs]


# ---------------------------------------------------------------------------
# Deterministic CBOR: encoder (C.5)
# ---------------------------------------------------------------------------


def head(major, n):
    """A CBOR head with the shortest argument form (section 5 rule 2)."""
    if n < 0 or n > U64_MAX:
        raise ValueError("argument out of range: %r" % n)
    m = major << 5
    if n < 24:
        return bytes([m | n])
    if n < 1 << 8:
        return bytes([m | 24, n])
    if n < 1 << 16:
        return bytes([m | 25]) + n.to_bytes(2, "big")
    if n < 1 << 32:
        return bytes([m | 26]) + n.to_bytes(4, "big")
    return bytes([m | 27]) + n.to_bytes(8, "big")


def _bits64(x):
    return struct.pack(">d", x)


def encode_float(x):
    """The shortest of binary16, binary32, binary64 that keeps the value;
    NaN is f9 7e 00 (section 5 rule 2). No integer to float conversion."""
    if x != x:
        return b"\xf9\x7e\x00"
    for fmt, marker in ((">e", 0xF9), (">f", 0xFA)):
        try:
            packed = struct.pack(fmt, x)
        except OverflowError:
            continue
        if _bits64(struct.unpack(fmt, packed)[0]) == _bits64(x):
            return bytes([marker]) + packed
    return b"\xfb" + _bits64(x)


def encode(value, _depth=0):
    """Deterministic CBOR for a Python value: None, bool, int, float, str,
    bytes, list, dict with str keys, or `Map`. Arrays and maps nest at most
    MAX_DEPTH deep (C.5 rule 6); map entries are sorted by the bytewise order
    of the encoded key and a duplicate key is an error (rule 3)."""
    if value is None:
        return b"\xf6"
    if value is True:
        return b"\xf5"
    if value is False:
        return b"\xf4"
    if isinstance(value, int):
        if value >= 0:
            return head(0, value)
        return head(1, -1 - value)
    if isinstance(value, float):
        return encode_float(value)
    if isinstance(value, (bytes, bytearray)):
        return head(2, len(value)) + bytes(value)
    if isinstance(value, str):
        raw = value.encode("utf-8")
        return head(3, len(raw)) + raw
    if isinstance(value, list):
        if _depth + 1 > MAX_DEPTH:
            raise ValueError("nesting deeper than %d" % MAX_DEPTH)
        return head(4, len(value)) + b"".join(encode(v, _depth + 1) for v in value)
    if isinstance(value, (dict, Map)):
        if _depth + 1 > MAX_DEPTH:
            raise ValueError("nesting deeper than %d" % MAX_DEPTH)
        pairs = list(value.items()) if isinstance(value, dict) else value.pairs
        entries = sorted((encode(k, _depth + 1), encode(v, _depth + 1)) for k, v in pairs)
        for a, b in zip(entries, entries[1:]):
            if a[0] == b[0]:
                raise ValueError("duplicate map key")
        return head(5, len(entries)) + b"".join(k + v for k, v in entries)
    raise TypeError("cannot encode %r" % (value,))


# ---------------------------------------------------------------------------
# Deterministic CBOR: strict validator (C.5 rules 1 to 6)
# ---------------------------------------------------------------------------
#
# Each rejection carries the name of the Rust CborError variant:
#   IndefiniteLength      rule 1: a string, array or map with an indefinite length
#   NonShortestInteger    rule 2: an integer or length not in its shortest head
#   NonShortestFloat      rule 2: a float that fits a narrower width
#   NonCanonicalNan       rule 2: a NaN other than f9 7e 00
#   UnsortedOrDuplicateKey rule 3: keys not strictly ascending by encoded bytes
#   Tag                   rule 4: any tag
#   UnsupportedSimple     rule 4: a simple value other than false, true, null
#   InvalidUtf8           rule 5: a text string that is not UTF-8
#   UnexpectedBreak       a break byte (ff) outside an indefinite item
#   ReservedInfo          additional information 28 to 30, or 31 on an integer
#   TrailingBytes         rule 6: bytes after the item
#   Truncated             the item ends early, or a length exceeds the input
#   DepthExceeded         rule 6: nesting deeper than 32

REJECT_REASONS = [
    "IndefiniteLength",
    "NonShortestInteger",
    "NonShortestFloat",
    "NonCanonicalNan",
    "UnsortedOrDuplicateKey",
    "Tag",
    "UnsupportedSimple",
    "InvalidUtf8",
    "UnexpectedBreak",
    "ReservedInfo",
    "TrailingBytes",
    "Truncated",
    "DepthExceeded",
]


def _take(data, pos, n):
    end = pos + n
    if end > len(data):
        raise Reject("Truncated", "need %d bytes at offset %d" % (n, pos))
    return data[pos:end], end


def _argument(data, pos, info):
    """The argument of a head with additional information `info`; enforces the
    shortest form. Returns (value, new position)."""
    if info < 24:
        return info, pos
    if info > 27:
        raise Reject("ReservedInfo", "additional information %d" % info)
    size = 1 << (info - 24)
    raw, pos = _take(data, pos, size)
    value = int.from_bytes(raw, "big")
    smallest = {1: 24, 2: 1 << 8, 4: 1 << 16, 8: 1 << 32}[size]
    if value < smallest:
        raise Reject("NonShortestInteger", "%d in %d bytes" % (value, size))
    return value, pos


def _half_is_nan(bits):
    return bits & 0x7C00 == 0x7C00 and bits & 0x03FF != 0


def _fits(value, fmt):
    try:
        packed = struct.pack(fmt, value)
    except OverflowError:
        return False
    return _bits64(struct.unpack(fmt, packed)[0]) == _bits64(value)


def _scan(data, pos, depth):
    raw, pos = _take(data, pos, 1)
    initial = raw[0]
    major, info = initial >> 5, initial & 0x1F
    if major in (0, 1):
        _, pos = _argument(data, pos, info)
        return pos
    if major in (2, 3):
        if info == 31:
            raise Reject("IndefiniteLength", "string")
        n, pos = _argument(data, pos, info)
        body, pos = _take(data, pos, n)
        if major == 3:
            try:
                body.decode("utf-8")
            except UnicodeDecodeError:
                raise Reject("InvalidUtf8", "at offset %d" % (pos - n))
        return pos
    if major in (4, 5):
        if info == 31:
            raise Reject("IndefiniteLength", "array" if major == 4 else "map")
        n, pos = _argument(data, pos, info)
        if depth + 1 > MAX_DEPTH:
            raise Reject("DepthExceeded", "more than %d levels" % MAX_DEPTH)
        # Every array element takes one byte or more, every map entry two or more.
        room = len(data) - pos
        if n > (room if major == 4 else room // 2):
            raise Reject("Truncated", "%d items declared, %d bytes left" % (n, room))
        previous = None
        for _ in range(n):
            start = pos
            pos = _scan(data, pos, depth + 1)
            if major == 5:
                key = data[start:pos]
                if previous is not None and key <= previous:
                    raise Reject("UnsortedOrDuplicateKey", "key at offset %d" % start)
                previous = key
                pos = _scan(data, pos, depth + 1)
        return pos
    if major == 6:
        raise Reject("Tag", "tags are not allowed")
    # Major 7.
    if info in (20, 21, 22):
        return pos
    if info == 25:
        raw, pos = _take(data, pos, 2)
        bits = int.from_bytes(raw, "big")
        if _half_is_nan(bits) and bits != 0x7E00:
            raise Reject("NonCanonicalNan", "binary16 0x%04x" % bits)
        return pos
    if info == 26:
        raw, pos = _take(data, pos, 4)
        value = struct.unpack(">f", raw)[0]
        if value != value:
            raise Reject("NonCanonicalNan", "binary32 NaN")
        if _fits(value, ">e"):
            raise Reject("NonShortestFloat", "binary32 that fits binary16")
        return pos
    if info == 27:
        raw, pos = _take(data, pos, 8)
        value = struct.unpack(">d", raw)[0]
        if value != value:
            raise Reject("NonCanonicalNan", "binary64 NaN")
        if _fits(value, ">f"):
            raise Reject("NonShortestFloat", "binary64 that fits binary32")
        return pos
    if info == 24:
        _take(data, pos, 1)
        raise Reject("UnsupportedSimple", "one-byte simple value")
    if info in (28, 29, 30):
        raise Reject("ReservedInfo", "additional information %d" % info)
    if info == 31:
        raise Reject("UnexpectedBreak", "break outside an indefinite item")
    raise Reject("UnsupportedSimple", "simple value %d" % info)


def validate(data):
    """Raise `Reject` unless `data` is exactly one item satisfying C.5."""
    data = bytes(data)
    end = _scan(data, 0, 0)
    if end != len(data):
        raise Reject("TrailingBytes", "%d bytes after the item" % (len(data) - end))


# ---------------------------------------------------------------------------
# Decoder (for validated input)
# ---------------------------------------------------------------------------


def _decode(data, pos):
    initial = data[pos]
    major, info = initial >> 5, initial & 0x1F
    pos += 1
    if major == 7:
        if info == 20:
            return False, pos
        if info == 21:
            return True, pos
        if info == 22:
            return None, pos
        size = {25: 2, 26: 4, 27: 8}[info]
        raw = data[pos : pos + size]
        fmt = {2: ">e", 4: ">f", 8: ">d"}[size]
        if size == 2 and raw == b"\x7e\x00":
            return float("nan"), pos + size
        return struct.unpack(fmt, raw)[0], pos + size
    n, pos = _argument(data, pos, info)
    if major == 0:
        return n, pos
    if major == 1:
        return -1 - n, pos
    if major in (2, 3):
        body = data[pos : pos + n]
        return (bytes(body) if major == 2 else body.decode("utf-8")), pos + n
    if major == 4:
        items = []
        for _ in range(n):
            item, pos = _decode(data, pos)
            items.append(item)
        return items, pos
    pairs = []
    for _ in range(n):
        key, pos = _decode(data, pos)
        value, pos = _decode(data, pos)
        pairs.append((key, value))
    if all(isinstance(k, str) for k, _ in pairs):
        return dict(pairs), pos
    return Map(pairs), pos


def decode(data):
    """Validate, then decode one item to a Python value."""
    validate(data)
    return _decode(bytes(data), 0)[0]


# ---------------------------------------------------------------------------
# JSON descriptions of CBOR values
# ---------------------------------------------------------------------------
#
# Conventions (repeated in the vector file):
#   {"$bytes": "<hex>"}              a byte string
#   {"$int": "<decimal>"}            an integer beyond +-2^53 (JSON numbers are not exact there)
#   {"$float": "nan"|"inf"|"-inf"}   a float JSON cannot write
#   {"$map": [[key, value], ...]}    a map whose keys are not all text
#   a JSON number with "." or "e"    a CBOR float; any other number is an integer
#   a JSON object                    a map with text keys; array, string, true, false, null as in JSON


def to_desc(value):
    if value is None or isinstance(value, (bool, str)):
        return value
    if isinstance(value, int):
        return {"$int": str(value)} if abs(value) > JSON_SAFE_INT else value
    if isinstance(value, float):
        if value != value:
            return {"$float": "nan"}
        if value in (float("inf"), float("-inf")):
            return {"$float": "inf" if value > 0 else "-inf"}
        return value
    if isinstance(value, (bytes, bytearray)):
        return {"$bytes": bytes(value).hex()}
    if isinstance(value, list):
        return [to_desc(v) for v in value]
    if isinstance(value, dict):
        return {k: to_desc(v) for k, v in value.items()}
    if isinstance(value, Map):
        return {"$map": [[to_desc(k), to_desc(v)] for k, v in value.pairs]}
    raise TypeError("cannot describe %r" % (value,))


def from_desc(desc):
    if isinstance(desc, list):
        return [from_desc(v) for v in desc]
    if isinstance(desc, dict):
        if len(desc) == 1:
            (key, inner), = desc.items()
            if key == "$bytes":
                return bytes.fromhex(inner)
            if key == "$int":
                return int(inner)
            if key == "$float":
                return {"nan": float("nan"), "inf": float("inf"), "-inf": float("-inf")}[inner]
            if key == "$map":
                return Map((from_desc(k), from_desc(v)) for k, v in inner)
        return {k: from_desc(v) for k, v in desc.items()}
    return desc


# ---------------------------------------------------------------------------
# zstd, by structure only
# ---------------------------------------------------------------------------

ZSTD_MAGIC = bytes([0x28, 0xB5, 0x2F, 0xFD])


def zstd_raw_frame(data):
    """A valid zstd frame holding `data` in one raw (stored) block, no
    dictionary, no checksum. Not compression, but any zstd decoder accepts it,
    and it lets this script write compressed-flag vectors without zstd."""
    n = len(data)
    if n > 131072:
        raise ValueError("raw-block helper is limited to 128 KiB")
    if n <= 255:
        header = bytes([0x20, n])  # single segment, 1-byte content size
    elif n <= 255 + 65536:
        header = bytes([0x60]) + (n - 256).to_bytes(2, "little")  # 2-byte content size, offset 256
    else:
        header = bytes([0xA0]) + n.to_bytes(4, "little")  # 4-byte content size
    block = ((n << 3) | 1).to_bytes(3, "little")  # last block, type raw, size n
    return ZSTD_MAGIC + header + block + bytes(data)


def zstd_inspect(z):
    """What can be said about a zstd frame without zstd.

    Returns ("not_zstd", None) when the first four bytes are not the zstd
    magic or a skippable-frame magic, ("decoded", bytes) for a single frame of
    raw and RLE blocks without dictionary or checksum, and ("opaque", None)
    for anything else (compressed blocks, truncation, a checksum, trailing
    data), whose content this script cannot know."""
    if len(z) >= 4 and z[:4] != ZSTD_MAGIC:
        magic = int.from_bytes(z[:4], "little")
        if not (0x184D2A50 <= magic <= 0x184D2A5F):
            return "not_zstd", None
    if len(z) < 5 or z[:4] != ZSTD_MAGIC:
        return "opaque", None
    descriptor = z[4]
    pos = 5
    fcs_flag, single = descriptor >> 6, (descriptor >> 5) & 1
    if descriptor & 0x08 or descriptor & 0x04 or descriptor & 0x03:
        return "opaque", None  # reserved bit, checksum or dictionary
    if not single:
        pos += 1  # window descriptor
    fcs_size = {0: 1 if single else 0, 1: 2, 2: 4, 3: 8}[fcs_flag]
    declared = None
    if fcs_size:
        declared = int.from_bytes(z[pos : pos + fcs_size], "little")
        if fcs_flag == 1:
            declared += 256
        pos += fcs_size
    out = bytearray()
    while True:
        if pos + 3 > len(z):
            return "opaque", None
        h = int.from_bytes(z[pos : pos + 3], "little")
        pos += 3
        last, kind, size = h & 1, (h >> 1) & 3, h >> 3
        if kind == 0:
            if pos + size > len(z):
                return "opaque", None
            out += z[pos : pos + size]
            pos += size
        elif kind == 1:
            if pos + 1 > len(z):
                return "opaque", None
            out += z[pos : pos + 1] * size
            pos += 1
        else:
            return "opaque", None
        if last:
            break
    if pos != len(z) or (declared is not None and declared != len(out)):
        return "opaque", None
    return "decoded", bytes(out)


# ---------------------------------------------------------------------------
# Frames (C.1, C.3, C.4, C.7)
# ---------------------------------------------------------------------------


def raw_frame(version, ftype, flags, request_id, body, length=None):
    """Assemble a frame with no checks, to build invalid ones."""
    if length is None:
        length = HEADER_AFTER_LENGTH + len(body)
    return struct.pack(">IBBBQ", length, version, ftype, flags, request_id) + bytes(body)


def check_request_id(ftype, request_id):
    """The request-id rules that hold for a frame type whatever the
    connection state (as implemented): HELLO, HELLO_ACK and CLOSE carry 0;
    REQUEST and PING carry a nonzero id. Other types are not checked here."""
    ok = True
    if ftype in (0x01, 0x02, 0x03):
        ok = request_id == 0
    elif ftype in (0x10, 0x04):
        ok = request_id != 0
    if not ok:
        raise FrameReject(
            "BadRequestId",
            "protocol_violation",
            detail="%s with id %d" % (TYPE_NAMES[ftype], request_id),
        )


def _max_in_force(max_frame):
    return min(max_frame, MAX_FRAME)


def encode_frame(ftype, request_id, body, flags=0, max_frame=MAX_FRAME):
    """A frame. With flags bit 0 set, `body` is the uncompressed CBOR and is
    stored in a raw-block zstd frame behind its u32 length (see
    zstd_raw_frame); the receiver cannot tell it from a compressed one."""
    if isinstance(ftype, str):
        ftype = FRAME_TYPES[ftype]
    check_request_id(ftype, request_id)
    if flags & 0xFE:
        raise ValueError("reserved flag bits set")
    if HEADER_AFTER_LENGTH + len(body) > _max_in_force(max_frame):
        raise ValueError("frame longer than max_frame")
    wire_body = bytes(body)
    if flags & 1:
        wire_body = struct.pack(">I", len(body)) + zstd_raw_frame(body)
    return raw_frame(VERSION, ftype, flags, request_id, wire_body)


def _check_length(length, max_frame):
    if length < HEADER_AFTER_LENGTH:
        raise FrameReject(
            "LengthTooSmall", "frame_too_large", detail="length %d is below 11" % length
        )
    if length > _max_in_force(max_frame):
        raise FrameReject(
            "LengthTooLarge",
            "frame_too_large",
            detail="length %d exceeds %d" % (length, _max_in_force(max_frame)),
        )


def _decompress(wire_body, max_frame):
    """C.4. Returns the uncompressed body, or None when the zstd content is
    opaque to this script (and so its length cannot be checked)."""
    if len(wire_body) < 4:
        raise FrameReject("CompressedBodyTooShort", "malformed_frame")
    declared = struct.unpack(">I", wire_body[:4])[0]
    limit = _max_in_force(max_frame) - HEADER_AFTER_LENGTH
    if declared > limit:
        raise FrameReject(
            "UncompressedTooLarge",
            "frame_too_large",
            detail="declared %d, limit %d" % (declared, limit),
        )
    kind, out = zstd_inspect(wire_body[4:])
    if kind == "not_zstd":
        raise FrameReject("Decompress", "malformed_frame", detail="no zstd magic")
    if kind == "decoded" and len(out) != declared:
        raise FrameReject(
            "UncompressedLengthMismatch",
            "malformed_frame",
            detail="declared %d, found %d" % (declared, len(out)),
        )
    return out


def decode_frame(data, max_frame=MAX_FRAME):
    """Decode one frame from the start of `data`. Returns (frame, consumed)
    with frame a dict: version, type (code), flags, request_id, body (the
    uncompressed bytes, or None when zstd content is opaque) and wire_body.

    The checks run in this order, each as soon as its bytes are present:
    length, version, type, flags, request id, then the body. That order is the
    Rust implementation's; the specification does not fix it."""
    data = bytes(data)
    if len(data) < 4:
        raise FrameReject("NeedMore", None, needed=4 - len(data))
    length = struct.unpack(">I", data[:4])[0]
    _check_length(length, max_frame)
    if len(data) < HEADER_LEN:
        raise FrameReject("NeedMore", None, needed=HEADER_LEN - len(data))
    version, ftype, flags = data[4], data[5], data[6]
    request_id = struct.unpack(">Q", data[7:15])[0]
    if version != VERSION:
        raise FrameReject("UnsupportedVersion", "unsupported_version", detail="version %d" % version)
    if ftype not in TYPE_NAMES:
        raise FrameReject("UnknownFrameType", "unknown_frame_type", detail="type 0x%02x" % ftype)
    if flags & 0xFE:
        raise FrameReject("ReservedFlags", "malformed_frame", detail="flags 0x%02x" % flags)
    check_request_id(ftype, request_id)
    total = 4 + length
    if len(data) < total:
        raise FrameReject("NeedMore", None, needed=total - len(data))
    wire_body = data[HEADER_LEN:total]
    body = _decompress(wire_body, max_frame) if flags & 1 else wire_body
    frame = {
        "version": version,
        "type": ftype,
        "flags": flags,
        "request_id": request_id,
        "body": body,
        "wire_body": wire_body,
    }
    return frame, total


# ---------------------------------------------------------------------------
# HELLO, HELLO_ACK, CLOSE (C.6, C.7, C.10)
# ---------------------------------------------------------------------------


def _is_uint(v, limit=U64_MAX):
    return isinstance(v, int) and not isinstance(v, bool) and 0 <= v <= limit


def hello_body(versions, max_frame, extensions, agent=None, **unknown):
    body = {"versions": list(versions), "max_frame": max_frame, "extensions": list(extensions)}
    if agent is not None:
        body["agent"] = agent
    body.update(unknown)
    return body


def hello_ack_body(version, max_frame, max_inflight, extensions, agent=None):
    body = {
        "version": version,
        "max_frame": max_frame,
        "max_inflight": max_inflight,
        "extensions": list(extensions),
    }
    if agent is not None:
        body["agent"] = agent
    return body


def close_body(code, message, supported=None):
    if code not in CLOSE_CODES:
        raise ValueError("unknown close code %r" % code)
    body = {"code": code, "message": message}
    if supported is not None:
        body["supported"] = list(supported)
    return body


def _parse_body(frame_type_code, wire_type, body):
    """Check the frame type, the CBOR rules and that the item is a text-keyed
    map. A CBOR failure is malformed_frame; a shape failure protocol_violation
    (C.7)."""
    if frame_type_code != wire_type:
        raise CloseReject("protocol_violation", "wrong frame type 0x%02x" % frame_type_code)
    try:
        value = decode(body)
    except Reject as e:
        raise CloseReject("malformed_frame", "body is not deterministic CBOR: %s" % e)
    if not isinstance(value, dict):
        raise CloseReject("protocol_violation", "body is not a map with text keys")
    return value


def _need(body, key, check, what):
    if key not in body:
        raise CloseReject("protocol_violation", "missing key %r" % key)
    if not check(body[key]):
        raise CloseReject("protocol_violation", "%r is not %s" % (key, what))
    return body[key]


def _is_text_list(v):
    return isinstance(v, list) and all(isinstance(x, str) for x in v)


def _optional_text(body, key):
    if key not in body:
        return None
    if not isinstance(body[key], str):
        raise CloseReject("protocol_violation", "%r must be text when present (null is not allowed)" % key)
    return body[key]


def _check_max_frame(max_frame):
    if not MIN_MAX_FRAME <= max_frame <= MAX_FRAME:
        raise CloseReject("protocol_violation", "max_frame %d is outside 1024 to 16777216" % max_frame)


def _check_unique(extensions):
    if len(set(extensions)) != len(extensions):
        raise CloseReject("protocol_violation", "duplicate extension id")


def parse_hello(frame_type_code, body):
    """C.6 HELLO validation. Unknown keys are ignored (section 5 rule 9)."""
    value = _parse_body(frame_type_code, FRAME_TYPES["HELLO"], body)
    versions = _need(value, "versions", lambda v: isinstance(v, list) and all(_is_uint(x) for x in v), "an array of unsigned integers")
    max_frame = _need(value, "max_frame", lambda v: _is_uint(v, U32_MAX), "an unsigned 32-bit integer")
    extensions = _need(value, "extensions", _is_text_list, "an array of text")
    agent = _optional_text(value, "agent")
    if not versions:
        raise CloseReject("protocol_violation", "versions is empty")
    if any(a >= b for a, b in zip(versions, versions[1:])):
        raise CloseReject("protocol_violation", "versions are not strictly ascending")
    _check_max_frame(max_frame)
    for ext in extensions:
        if not ext.isascii() or len(ext.encode("utf-8")) > MAX_EXTENSION_ID_LEN:
            raise CloseReject("protocol_violation", "extension id is not ASCII or longer than 128 bytes")
    _check_unique(extensions)
    if agent is not None and len(agent.encode("utf-8")) > MAX_AGENT_LEN:
        raise CloseReject("protocol_violation", "agent is longer than 128 bytes")
    return {"versions": versions, "max_frame": max_frame, "extensions": extensions, "agent": agent}


def parse_hello_ack(frame_type_code, body):
    value = _parse_body(frame_type_code, FRAME_TYPES["HELLO_ACK"], body)
    version = _need(value, "version", _is_uint, "an unsigned integer")
    max_frame = _need(value, "max_frame", lambda v: _is_uint(v, U32_MAX), "an unsigned 32-bit integer")
    max_inflight = _need(value, "max_inflight", lambda v: _is_uint(v, U32_MAX), "an unsigned 32-bit integer")
    extensions = _need(value, "extensions", _is_text_list, "an array of text")
    agent = _optional_text(value, "agent")
    _check_max_frame(max_frame)
    if max_inflight < 1:
        raise CloseReject("protocol_violation", "max_inflight is 0")
    _check_unique(extensions)
    return {
        "version": version,
        "max_frame": max_frame,
        "max_inflight": max_inflight,
        "extensions": extensions,
        "agent": agent,
    }


def parse_close(frame_type_code, body):
    """CLOSE is a closed struct (C.5 rule 8): an unknown key is an error."""
    value = _parse_body(frame_type_code, FRAME_TYPES["CLOSE"], body)
    code = _need(value, "code", lambda v: isinstance(v, str), "text")
    message = _need(value, "message", lambda v: isinstance(v, str), "text")
    supported = None
    if "supported" in value:
        supported = _need(value, "supported", lambda v: isinstance(v, list) and all(_is_uint(x) for x in v), "an array of unsigned integers")
    extra = set(value) - {"code", "message", "supported"}
    if extra:
        raise CloseReject("protocol_violation", "unknown key %r in CLOSE" % sorted(extra)[0])
    return {"code": code, "message": message, "supported": supported}


def negotiate(hello, local):
    """C.6 server side. `hello` is parse_hello's result; `local` a dict with
    versions, max_frame, max_inflight, extensions, agent (None for no agent).

    Version: the highest one offered that the server supports; none means
    CLOSE unsupported_version with `supported` (sorted, without duplicates).
    max_frame: the smaller of the client's and the server's (the server's is
    clamped to 1024 .. 16777216). max_inflight: the server's, at least 1.
    Extensions: those offered that the server supports, in the CLIENT's order
    (section 6.4: the client's order fixes the index; this is the Rust
    implementation's choice)."""
    common = [v for v in hello["versions"] if v in local["versions"]]
    if not common:
        raise CloseReject(
            "unsupported_version",
            "no offered version is supported",
            supported=sorted(set(local["versions"])),
        )
    local_max = min(max(local["max_frame"], MIN_MAX_FRAME), MAX_FRAME)
    return {
        "version": max(common),
        "max_frame": min(hello["max_frame"], local_max),
        "max_inflight": max(local["max_inflight"], 1),
        "extensions": [e for e in hello["extensions"] if e in local["extensions"]],
        "agent": local["agent"],
    }


# ---------------------------------------------------------------------------
# REQUEST and RESPONSE bodies (C.5 rule 7, C.7, D)
# ---------------------------------------------------------------------------


def command(name, fields=None):
    """The REQUEST body for a command: the text of its name when it has no
    fields, else the single-entry map {name: fields}. `name` must be one of
    COMMAND_NAMES (the wire names of D)."""
    if name not in COMMAND_NAMES:
        raise ValueError("unknown command %r" % name)
    return name if fields is None else {name: fields}


def reply_ok(value):
    return {"ok": value}


def reply_err(code, message, details=None):
    err = {"code": code, "message": message}
    if details is not None:
        err["details"] = details
    return {"err": err}


def validate_request_value(value):
    """Shape of a REQUEST body (decoded): returns the command name, or raises
    ValueError with the C.7 answer code in the message."""
    if isinstance(value, str):
        name, fields = value, None
    elif isinstance(value, dict) and len(value) == 1:
        (name, fields), = value.items()
    else:
        raise ValueError("invalid_request: not a command")
    if name not in COMMAND_NAMES:
        raise ValueError("unknown_command: %s" % name)
    spec = COMMAND_FIELDS.get(name, ([], []))
    if name == "execute_binary":
        return name
    if spec is None:
        if fields is not None or not isinstance(value, str):
            raise ValueError("invalid_request: %s takes no fields" % name)
        return name
    required, optional = spec
    if not isinstance(fields, dict):
        raise ValueError("invalid_request: %s needs a map of fields" % name)
    missing = [k for k in required if k not in fields]
    unknown = [k for k in fields if k not in required and k not in optional]
    if missing or unknown:
        raise ValueError("invalid_request: %s missing=%s unknown=%s" % (name, missing, unknown))
    if any(fields[k] is None for k in optional if k in fields):
        raise ValueError("invalid_request: a present null for an optional field")
    return name


def validate_reply_value(value):
    if not isinstance(value, dict) or len(value) != 1:
        raise ValueError("RESPONSE body must be a single-entry map")
    (key, inner), = value.items()
    if key == "ok":
        return key
    if key == "err":
        if not isinstance(inner, dict) or not {"code", "message"} <= set(inner) <= {"code", "message", "details"}:
            raise ValueError("err must be {code, message[, details]}")
        if not isinstance(inner["code"], str) or not isinstance(inner["message"], str):
            raise ValueError("err.code and err.message must be text")
        if "details" in inner and inner["details"] is None:
            raise ValueError("details must be omitted, not null")
        return key
    raise ValueError("RESPONSE key must be ok or err")


STREAM_ROLES = ("assistant", "tool", "user", "system")
STREAM_END_REASONS = ("stop", "length", "tool_use", "content_filter", "cancelled")


def validate_stream_value(ftype_name, value):
    """Shapes of the four STREAM_* bodies (C.3, C.9 and the Rust types)."""
    if not isinstance(value, dict):
        raise ValueError("stream body must be a map")
    keys = set(value)
    if ftype_name == "STREAM_START":
        ok = {"role", "model"} <= keys <= {"role", "model", "trace", "decode_hints"} and value["role"] in STREAM_ROLES
    elif ftype_name == "STREAM_TOKEN":
        ok = {"seq", "data"} <= keys <= {"seq", "data", "usage"} and _is_uint(value["seq"])
        data = value.get("data")
        ok = ok and isinstance(data, dict) and len(data) == 1 and next(iter(data)) in ("text", "bytes", "tool_call")
    elif ftype_name == "STREAM_END":
        reason = value.get("reason")
        ok = "reason" in keys and keys <= {"reason", "usage"} and (
            reason in STREAM_END_REASONS
            or (isinstance(reason, dict) and list(reason) == ["error"])
        )
    else:  # STREAM_CANCEL
        ok = keys <= {"reason"}
    if not ok:
        raise ValueError("bad %s body" % ftype_name)


# ---------------------------------------------------------------------------
# Vectors
# ---------------------------------------------------------------------------

CONVENTIONS = [
    "spine-wire v1 conformance vectors. Generated by scripts/wire_v1.py from docs/spec/00-wire.md; do not edit by hand.",
    "Regenerate: python scripts/wire_v1.py generate > docs/spec/00-wire.vectors.json. Check: python scripts/wire_v1.py check docs/spec/00-wire.vectors.json.",
    "All 'hex' strings are lowercase, no spaces or prefix. A frame is u32 length, u8 version, u8 type, u8 flags, u64 request id (all big-endian), then the body.",
    "Value descriptions: a JSON object is a CBOR map with text keys; arrays, strings, true, false and null are as in JSON.",
    "A JSON number with a '.' or an exponent is a CBOR float; any other JSON number is a CBOR integer.",
    "{\"$bytes\": \"<hex>\"} is a CBOR byte string. {\"$int\": \"<decimal>\"} is an integer beyond +-2^53 (JSON numbers are inexact there).",
    "{\"$float\": \"nan\"|\"inf\"|\"-inf\"} is a float JSON cannot write. {\"$map\": [[key, value], ...]} is a map whose keys are not all text.",
    "valid_bodies: deterministic CBOR items a decoder must accept. 'hex' is the exact encoding; 'value' describes it.",
    "invalid_bodies: items a decoder must reject. 'reason' is the Rust cbor::CborError variant name, which is also the rule broken (C.5).",
    "valid_frames: whole frames. 'type', 'request_id', 'flags' and 'body' (or 'body_hex') describe 'hex'. 'typed' names the body type that decodes it: "
    "command, reply, ping, pong, stream, hello, hello_ack, close, ext, or none (decode and re-encode of the typed value is not offered, e.g. a result with byte strings).",
    "A frame with flags 1 has the body u32 uncompressed_len, then a zstd frame. These vectors store the body in a raw-block zstd frame (valid zstd, not smaller). "
    "A receiver must decode them to 'body_hex'; they are not byte-for-byte what a compressing sender would emit, so re-encoding is not part of the check.",
    "invalid_frames: whole frames (or the prefix needed to decide) that a receiver using 'max_frame' as the maximum in force must reject with 'error' (the Rust frame::FrameError variant) and 'close_code' (C.7 wire text). "
    "'build' re-derives 'hex'. incomplete_frames: valid prefixes; 'needed' is the least number of further bytes (FrameError::NeedMore).",
    "The order of the stateless frame checks is: length, version, type, flags, request id, then the body (specification section 7.2).",
    "handshakes: 'hello_frame' is a whole HELLO frame, 'local' the server configuration, 'expect' the outcome of validating the HELLO and negotiating: "
    "an 'ack' (with the whole 'ack_frame') or a 'close' with its code and 'supported'. CLOSE 'message' text is not specified and not part of a vector.",
    "A vector with a 'note' explains the rule it exercises and cites the section of docs/spec/00-wire.md that states it.",
    "Not covered: connection state (id reuse, direction, HELLO first, extension index, timeouts, max_inflight) and real zstd compression.",
]

# Cross-check sources. Each must be reproduced byte for byte.
C12_HELLO = (
    "00 00 00 32 01 01 00 00 00 00 00 00 00 00 00 a3 68 76 65 72 73 69 6f 6e 73 81 01 69 6d 61 78 5f 66 72 61 6d 65 "
    "1a 01 00 00 00 6a 65 78 74 65 6e 73 69 6f 6e 73 80"
)
C12_REQUEST = (
    "00 00 00 3d 01 10 00 00 00 00 00 00 00 00 01 a1 69 74 6f 6f 6c 5f 63 61 6c 6c a2 64 61 72 67 73 a1 64 74 65 78 "
    "74 62 68 69 64 6e 61 6d 65 72 73 70 69 6e 65 2e 63 6f 6e 74 65 6e 74 2f 68 61 73 68"
)
C12_PING = "00 00 00 0d 01 04 00 00 00 00 00 00 00 00 07 18 2a"
BODIES_V1_GET_UR = "666765745f7572"
BODIES_V1_READ_RESOURCE = (
    "a1" "6d726561645f7265736f75726365" "a3" "6468617368" "5820"
    "1111111111111111111111111111111111111111111111111111111111111111"
    "666f6666736574" "00" "696d61785f6279746573" "10"
)
BODIES_V1_ERR_OVERLOADED = "a1" "63657272" "a2" "64636f6465" "6a6f7665726c6f61646564" "676d657373616765" "616d"


def _hex(s):
    return bytes.fromhex(s.replace(" ", ""))


def _id_desc(n):
    return {"$int": str(n)} if n > JSON_SAFE_INT else n


def _id_from(d):
    return int(d["$int"]) if isinstance(d, dict) else d


def frame_from_spec(spec):
    """The whole frame described by a valid-frame spec: type (name),
    request_id, flags, and body (a value description) or body_hex."""
    body = encode(from_desc(spec["body"])) if "body" in spec else bytes.fromhex(spec["body_hex"])
    return encode_frame(spec["type"], _id_from(spec["request_id"]), body, spec.get("flags", 0))


def valid_frame(name, ftype, request_id, body_value, typed, flags=0, note=None):
    body = encode(body_value)
    frame = encode_frame(ftype, request_id, body, flags)
    v = {
        "name": name,
        "type": ftype,
        "type_code": FRAME_TYPES[ftype],
        "request_id": _id_desc(request_id),
        "flags": flags,
        "body": to_desc(body_value),
        "body_hex": body.hex(),
        "hex": frame.hex(),
        "typed": typed,
    }
    if flags & 1:
        v["compressed"] = True
    if note:
        v["note"] = note
    return v


def valid_body(name, value, note=None, source=None):
    v = {"name": name, "value": to_desc(value), "hex": encode(value).hex()}
    if note:
        v["note"] = note
    if source:
        v["source"] = source
    return v


def _nest(n, inner, wrap):
    value = inner
    for _ in range(n):
        value = wrap(value)
    return value


def _mixed(levels):
    value = []
    for i in range(levels - 1):
        value = [value] if i % 2 == 0 else {"a": value}
    return value


def make_valid_bodies():
    out = []

    def add(name, value, **kw):
        out.append(valid_body(name, value, **kw))

    # Integers: every head-size boundary, both signs.
    for n in (0, 1, 23, 24, 25, 255, 256, 65535, 65536, 4294967295, 4294967296, U64_MAX):
        add("uint_%d" % n, n)
    for n in (-1, -24, -25, -256, -257, -65536, -65537, -4294967296, -4294967297, -(2**64)):
        add("nint_%d" % -n, n)
    # Lengths: byte and text strings, arrays and maps, at the head-size boundaries.
    for n in (0, 23, 24, 255, 256, 65536):
        add("bytes_len_%d" % n, b"\x00" * n)
    for n in (0, 23, 24, 255, 256):
        add("text_len_%d" % n, "a" * n)
        add("array_len_%d" % n, [0] * n)
    for n in (0, 23, 24, 255, 256):
        add("map_len_%d" % n, Map((i, 0) for i in range(n)), note="integer keys 0..n-1" if n else None)
    # Floats: width selection, boundaries, specials.
    floats = [
        ("zero", 0.0), ("negative_zero", -0.0), ("one", 1.0), ("one_and_a_half", 1.5),
        ("half_max_65504", 65504.0), ("needs_single_65505", 65505.0), ("single_100000", 100000.0),
        ("single_max", 3.4028234663852886e38), ("needs_double_1_1", 1.1), ("double_1e300", 1e300),
        ("half_min_subnormal", 5.960464477539063e-08), ("half_min_normal", 6.103515625e-05),
        ("minus_four", -4.0), ("minus_four_point_one", -4.1), ("infinity", float("inf")),
        ("negative_infinity", float("-inf")), ("nan", float("nan")),
        ("single_min_subnormal", 1.401298464324817e-45), ("double_min_subnormal", 5e-324),
        ("double_max", 1.7976931348623157e308),
    ]
    for name, f in floats:
        add("float_" + name, f)
    add("nan_is_f97e00", float("nan"), note="NaN of any payload or sign is written as f9 7e 00 (section 5 rule 2)")
    # Simple values and text.
    add("false", False)
    add("true", True)
    add("null", None)
    for name, s in (("empty", ""), ("ascii", "spine"), ("latin1_e_acute", "é"), ("bmp_check_mark", "✓"),
                    ("astral_g_clef", "\U0001d11e"), ("embedded_nul", "a\x00b")):
        add("text_" + name, s)
    add("bytes_short", bytes([0, 1, 2, 255]))
    # Key ordering: shorter keys first, then bytewise (C.5 rule 3).
    add("keys_shorter_first", {"b": 1, "aa": 2, "a": 3},
        note="encoded keys sort as 61 61 ('a'), 61 62 ('b'), 62 61 61 ('aa')")
    add("keys_by_first_byte", {"max_frame": 1, "versions": 2, "extensions": 3},
        note="the HELLO order of section 15.3: versions (68), max_frame (69), extensions (6a)")
    add("keys_mixed_types", Map([("a", 1), (10, 2), (-1, 3), (b"\x00", 4), (False, 5), (None, 6)]),
        note="keys of different types sort by encoded bytes: 0a, 20, 41 00, 61 61, f4, f6")
    add("keys_equal_prefix_text", {"ab": 1, "abc": 2, "b": 3, "": 4})
    add("empty_array", [])
    add("empty_map", {})
    add("nested_mixed", {"k": [1, {"z": [None, True, 1.5, b"\x01"]}, "x"], "a": {}})
    # Nesting depth: 32 is the limit (C.5 rule 6); scalars do not count.
    add("depth_32_arrays_empty_innermost", _nest(31, [], lambda v: [v]),
        note="32 nested arrays; the innermost is empty. Accepted.")
    add("depth_32_arrays_with_scalar", _nest(32, 0, lambda v: [v]),
        note="32 nested arrays around a scalar. Accepted: scalars do not count.")
    add("depth_32_maps", _nest(31, {}, lambda v: {"a": v}), note="32 nested maps. Accepted.")
    add("depth_32_mixed", _mixed(32), note="alternating arrays and maps, 32 levels, the innermost an empty array. Accepted.")
    # Sources cross-checked against the Rust tests and the specification (C.12).
    out.append(valid_body("bodies_v1_get_ur", "get_ur", source="src/spine-protocol/tests/bodies_v1.rs, hand_checked_bodies, GET_UR"))
    out.append(valid_body("bodies_v1_read_resource",
                          {"read_resource": {"hash": b"\x11" * 32, "offset": 0, "max_bytes": 16}},
                          source="src/spine-protocol/tests/bodies_v1.rs, hand_checked_bodies, READ_RESOURCE"))
    out.append(valid_body("bodies_v1_err_overloaded",
                          {"err": {"code": "overloaded", "message": "m"}},
                          source="src/spine-protocol/tests/bodies_v1.rs, hand_checked_bodies, ERR_OVERLOADED_BODY"))
    return out


def invalid_body(name, hex_text, reason, note=None):
    v = {"name": name, "hex": hex_text.replace(" ", ""), "reason": reason}
    if note:
        v["note"] = note
    return v


def make_invalid_bodies():
    D = []

    def add(reason, name, hx, note=None):
        D.append(invalid_body("%s_%s" % (reason, name), hx, reason, note))

    add("IndefiniteLength", "byte_string", "5f ff")
    add("IndefiniteLength", "text_string", "7f 61 61 ff")
    add("IndefiniteLength", "array", "9f 01 ff")
    add("IndefiniteLength", "map", "bf 61 61 01 ff")
    add("IndefiniteLength", "nested_in_array", "81 9f ff")
    for n, hx in (("one_byte_zero", "18 00"), ("one_byte_23", "18 17"), ("two_bytes_zero", "19 00 00"),
                  ("two_bytes_255", "19 00 ff"), ("four_bytes_65535", "1a 00 00 ff ff"),
                  ("eight_bytes_u32_max", "1b 00 00 00 00 ff ff ff ff"), ("negative_one_byte_zero", "38 00"),
                  ("negative_two_bytes_255", "39 00 ff")):
        add("NonShortestInteger", n, hx)
    add("NonShortestInteger", "byte_string_length", "58 00")
    add("NonShortestInteger", "text_string_length", "78 17")
    add("NonShortestInteger", "array_length", "98 00")
    add("NonShortestInteger", "map_length", "b8 00")
    add("NonShortestInteger", "length_in_nested_item", "81 59 00 01 00")
    for n, hx in (("binary32_one", "fa 3f 80 00 00"), ("binary32_one_and_a_half", "fa 3f c0 00 00"),
                  ("binary32_zero", "fa 00 00 00 00"), ("binary32_negative_zero", "fa 80 00 00 00"),
                  ("binary32_infinity", "fa 7f 80 00 00"), ("binary64_one", "fb 3f f0 00 00 00 00 00 00"),
                  ("binary64_one_and_a_half", "fb 3f f8 00 00 00 00 00 00"),
                  ("binary64_infinity", "fb 7f f0 00 00 00 00 00 00"), ("binary64_zero", "fb 00 00 00 00 00 00 00 00"),
                  ("binary64_fits_binary32", "fb 40 59 00 00 00 00 00 00")):
        add("NonShortestFloat", n, hx)
    for n, hx in (("binary16_payload_1", "f9 7e 01"), ("binary16_negative", "f9 fe 00"), ("binary16_signalling", "f9 7d 00"),
                  ("binary32_quiet", "fa 7f c0 00 00"), ("binary32_payload", "fa 7f c0 00 01"),
                  ("binary64_quiet", "fb 7f f8 00 00 00 00 00 00"), ("binary64_negative", "fb ff f8 00 00 00 00 00 00")):
        add("NonCanonicalNan", n, hx)
    add("UnsortedOrDuplicateKey", "descending", "a2 61 62 01 61 61 02")
    add("UnsortedOrDuplicateKey", "duplicate", "a2 61 61 01 61 61 02")
    add("UnsortedOrDuplicateKey", "longer_key_first", "a2 62 61 61 01 61 62 02",
        note="'aa' (62 61 61) must come after 'b' (61 62): shorter keys first")
    add("UnsortedOrDuplicateKey", "integer_keys_descending", "a2 01 00 00 00")
    add("UnsortedOrDuplicateKey", "nested_in_array", "81 a2 61 62 00 61 61 00")
    add("UnsortedOrDuplicateKey", "equal_keys_different_values", "a2 00 01 00 02")
    add("Tag", "datetime_string", "c0 74 32 30 31 33 2d 30 33 2d 32 31 54 32 30 3a 30 34 3a 30 30 5a")
    add("Tag", "epoch_integer", "c1 1a 51 4b 67 b0")
    add("Tag", "bignum", "c2 41 01")
    add("Tag", "one_byte_tag_number", "d8 18 41 00")
    add("Tag", "self_described_cbor", "d9 d9 f7 00")
    add("UnsupportedSimple", "undefined", "f7")
    add("UnsupportedSimple", "simple_16", "f0")
    add("UnsupportedSimple", "simple_0", "e0")
    add("UnsupportedSimple", "simple_19", "f3")
    add("UnsupportedSimple", "one_byte_simple_32", "f8 20")
    add("UnsupportedSimple", "one_byte_simple_0", "f8 00")
    add("UnsupportedSimple", "one_byte_simple_255", "f8 ff")
    add("UnsupportedSimple", "undefined_in_array", "81 f7")
    add("InvalidUtf8", "lone_continuation", "61 ff")
    add("InvalidUtf8", "truncated_sequence_in_length", "62 c3 28")
    add("InvalidUtf8", "surrogate", "63 ed a0 80")
    add("InvalidUtf8", "overlong", "62 c0 80")
    add("InvalidUtf8", "above_10ffff", "64 f4 90 80 80")
    add("InvalidUtf8", "map_key", "a1 62 c3 28 00")
    add("UnexpectedBreak", "alone", "ff")
    add("UnexpectedBreak", "in_array", "81 ff")
    add("UnexpectedBreak", "as_map_key", "a1 ff 00")
    add("UnexpectedBreak", "after_element", "82 01 ff")
    for n, hx in (("unsigned_28", "1c"), ("unsigned_29", "1d"), ("unsigned_30", "1e"), ("unsigned_31", "1f"),
                  ("negative_28", "3c"), ("negative_31", "3f"), ("bytes_28", "5c"), ("text_29", "7d"),
                  ("array_30", "9e"), ("map_28", "bc"), ("simple_28", "fc"), ("simple_29", "fd"), ("simple_30", "fe")):
        add("ReservedInfo", n, hx)
    add("TrailingBytes", "two_integers", "00 00")
    add("TrailingBytes", "after_null", "f6 00")
    add("TrailingBytes", "after_empty_map", "a0 00")
    add("TrailingBytes", "after_text", "61 61 00")
    add("TrailingBytes", "after_array_a_break", "80 ff", note="the break byte is not scanned: bytes remain after the item")
    add("Truncated", "empty_input", "")
    add("Truncated", "one_byte_head_cut", "18")
    add("Truncated", "two_byte_head_cut", "19 ff")
    add("Truncated", "four_byte_head_cut", "1a 00 00 00")
    add("Truncated", "text_cut", "62 61")
    add("Truncated", "text_without_body", "61")
    add("Truncated", "bytes_declared_thirty_has_one", "58 1e 01")
    add("Truncated", "array_missing_element", "82 01")
    add("Truncated", "array_without_elements", "81")
    add("Truncated", "map_missing_value", "a1 61 61")
    add("Truncated", "map_missing_entry", "a1")
    add("Truncated", "map_one_byte_for_one_entry", "a1 00")
    add("Truncated", "half_float_cut", "f9 00")
    add("Truncated", "single_float_cut", "fa 00 00 00")
    add("Truncated", "double_float_cut", "fb 00")
    add("Truncated", "one_byte_simple_cut", "f8")
    add("Truncated", "huge_bytes_length", "5b ff ff ff ff ff ff ff ff")
    add("Truncated", "huge_text_length", "7b 00 00 00 01 00 00 00 00")
    add("Truncated", "huge_array_length", "9b ff ff ff ff ff ff ff ff")
    add("Truncated", "huge_map_length", "bb ff ff ff ff ff ff ff ff")
    add("DepthExceeded", "33_arrays_around_scalar", "81" * 33 + "00")
    add("DepthExceeded", "33_arrays_innermost_empty", "81" * 32 + "80")
    add("DepthExceeded", "33_maps", "a1 61 61 " * 33 + "00")
    add("DepthExceeded", "33_deep_in_second_element", "82 00" + "81" * 32 + "00",
        note="the top array is level 1, so 32 more arrays make level 33")
    return D


def _frame_build(version=1, ftype=0x04, flags=0, request_id=1, length=None, body_hex="00", cut=None):
    return {"version": version, "type": ftype, "flags": flags, "request_id": _id_desc(request_id),
            "length": length, "body_hex": body_hex, "cut": cut}


def frame_from_build(b):
    data = raw_frame(b["version"], b["type"], b["flags"], _id_from(b["request_id"]),
                     bytes.fromhex(b["body_hex"]), b["length"])
    return data if b["cut"] is None else data[: b["cut"]]


def _compressed(declared, content=b"x"):
    """The body of a compressed frame: declared length, then a raw-block zstd frame."""
    return (struct.pack(">I", declared) + zstd_raw_frame(content)).hex()


def make_invalid_frames():
    out = []

    def add(name, build, max_frame=MAX_FRAME, note=None):
        data = frame_from_build(build)
        try:
            decode_frame(data, max_frame)
        except FrameReject as e:
            v = {"name": name, "build": build, "hex": data.hex(), "max_frame": max_frame,
                 "error": e.error, "close_code": e.close_code}
            if note:
                v["note"] = note
            out.append(v)
            return
        raise AssertionError("invalid frame vector %s was accepted" % name)

    # Length (C.1, C.7 row 1).
    for n in (0, 1, 10):
        add("length_%d_below_minimum" % n, _frame_build(length=n, body_hex=""))
    add("length_below_minimum_pre_hello", _frame_build(length=10, body_hex=""), PRE_HELLO_MAX_FRAME)
    add("length_65537_over_pre_hello_maximum", _frame_build(length=65537, body_hex=""), PRE_HELLO_MAX_FRAME,
        note="before HELLO is processed the maximum is 65536 (section 2.3)")
    add("length_16777217_over_default_maximum", _frame_build(length=16777217, body_hex=""))
    add("length_ffffffff", _frame_build(length=0xFFFFFFFF, body_hex=""),
        note="the body is not read and nothing is allocated for it (R2)")
    add("length_1025_over_negotiated_1024", _frame_build(length=1025, body_hex=""), 1024)
    add("length_over_ceiling_even_if_caller_allows_more", _frame_build(length=16777217, body_hex=""), 0xFFFFFFFF,
        note="a caller maximum above 16777216 is capped at 16777216 (section 2.3)")
    # Version (C.7 row 2).
    for ver in (0, 2, 255):
        add("version_%d" % ver, _frame_build(version=ver))
    # Unknown frame type (C.7 row 4).
    for t in (0x00, 0x06, 0x0F, 0x12, 0x1F, 0x24, 0x7F, 0x81, 0xFF):
        add("type_0x%02x" % t, _frame_build(ftype=t))
    # Reserved flag bits (C.7 row 3).
    for bit in range(1, 8):
        add("flag_bit_%d" % bit, _frame_build(flags=1 << bit))
    add("flags_all_set", _frame_build(flags=0xFF))
    add("flags_compressed_and_reserved", _frame_build(flags=0x03, body_hex=_compressed(1)))
    # Request-id rules that need no connection state, as implemented.
    for t, name in ((0x01, "hello"), (0x02, "hello_ack"), (0x03, "close")):
        add("%s_with_nonzero_id" % name, _frame_build(ftype=t, request_id=1))
    add("hello_with_max_id", _frame_build(ftype=0x01, request_id=U64_MAX))
    add("request_with_id_zero", _frame_build(ftype=0x10, request_id=0))
    add("ping_with_id_zero", _frame_build(ftype=0x04, request_id=0))
    # Order of the checks.
    add("order_version_before_type", _frame_build(version=2, ftype=0xFF),
        note="the stateless checks run in the order length, version, type, flags, request id, then the body (section 7.2)")
    add("order_type_before_flags", _frame_build(ftype=0xFF, flags=0x80),
        note="the stateless checks run in the order length, version, type, flags, request id, then the body (section 7.2)")
    add("order_flags_before_request_id", _frame_build(ftype=0x01, flags=0x02, request_id=1),
        note="the stateless checks run in the order length, version, type, flags, request id, then the body (section 7.2)")
    add("order_length_before_version", _frame_build(version=2, length=10, body_hex=""),
        note="the length is checked as soon as its four bytes are present (section 7.2 step 2)")
    # Compressed bodies (C.4).
    add("compressed_body_empty", _frame_build(flags=1, body_hex=""))
    add("compressed_body_three_bytes", _frame_build(flags=1, body_hex="000000"))
    add("compressed_declared_over_limit", _frame_build(flags=1, body_hex=_compressed(16777206)),
        note="a declared uncompressed length above max_frame - 11 is frame_too_large and is checked before decompression "
             "(section 4 rule 1); limit = 16777205")
    add("compressed_declared_over_limit_1024", _frame_build(flags=1, body_hex=_compressed(1014)), 1024,
        note="frame_too_large (section 4 rule 1), as in compressed_declared_over_limit; limit 1013")
    add("compressed_declared_ffffffff", _frame_build(flags=1, body_hex=_compressed(0xFFFFFFFF)),
        note="frame_too_large (section 4 rule 1), as in compressed_declared_over_limit")
    add("compressed_declared_at_limit_then_mismatch", _frame_build(flags=1, body_hex=_compressed(1013)), 1024,
        note="1013 equals the limit, so the declared-length check passes; the real output is 1 byte")
    add("compressed_declared_one_too_many", _frame_build(flags=1, body_hex=_compressed(2, b"x")))
    add("compressed_declared_one_too_few", _frame_build(flags=1, body_hex=_compressed(1, b"xy")))
    add("compressed_declared_zero_but_not_empty", _frame_build(flags=1, body_hex=_compressed(0, b"x")))
    add("compressed_not_zstd", _frame_build(flags=1, body_hex="00000005" + "68656c6c6f"),
        note="no zstd magic: malformed_frame (section 4 rule 2)")
    return out


def make_incomplete_frames():
    out = []
    ping = raw_frame(1, 0x04, 0, 7, b"\x18\x2a")

    def add(name, data, max_frame=MAX_FRAME):
        try:
            decode_frame(data, max_frame)
        except FrameReject as e:
            if e.error != "NeedMore":
                raise AssertionError("%s is not incomplete" % name)
            out.append({"name": name, "hex": data.hex(), "max_frame": max_frame, "error": "NeedMore",
                        "close_code": None, "needed": e.needed})
            return
        raise AssertionError("incomplete frame vector %s decoded" % name)

    add("empty", b"")
    add("two_length_bytes", ping[:2])
    add("length_only", ping[:4])
    add("header_cut_after_version", ping[:5])
    add("header_complete_body_missing", ping[:15])
    add("body_one_byte_short", ping[:-1])
    return out


def make_valid_frames():
    out = []
    key32 = b"\x22" * 32
    trace = {"trace_id": b"\xab" * 16, "span_id": b"\xcd" * 8, "flags": 1}

    def add(name, ftype, rid, body, typed, **kw):
        out.append(valid_frame(name, ftype, rid, body, typed, **kw))

    # The worked examples of C.12 (and the PING of its last paragraph).
    add("c12_hello", "HELLO", 0, hello_body([1], 16777216, []), "hello", note="worked example 1 (section 15.3)")
    add("c12_request_tool_call", "REQUEST", 1, command("tool_call", {"name": "spine.content/hash", "args": {"text": "hi"}}),
        "command", note="worked example 2 (section 15.3)")
    add("c12_ping_token_42_id_7", "PING", 7, 42, "ping", note="worked example, last paragraph (section 15.3)")
    # HELLO, HELLO_ACK, CLOSE.
    add("hello_with_agent_and_extension", "HELLO", 0,
        hello_body([1, 2], 65536, ["nervosys.org/neural-codec/1"], "conformance/1"), "hello")
    add("hello_min_max_frame_no_extensions", "HELLO", 0, hello_body([1], 1024, []), "hello")
    add("hello_ack_minimal", "HELLO_ACK", 0, hello_ack_body(1, 16777216, 1, []), "hello_ack")
    add("hello_ack_extensions_and_agent", "HELLO_ACK", 0,
        hello_ack_body(1, 65536, 8, ["b/two/1", "a/one/1"], "spined/0"), "hello_ack",
        note="the order of extensions fixes the extension index (sections 6.4 and 11)")
    add("close_without_supported", "CLOSE", 0, close_body("malformed_frame", "body is not deterministic CBOR"), "close")
    add("close_with_supported", "CLOSE", 0, close_body("unsupported_version", "no offered version is supported", [1, 2]), "close")
    add("close_shutting_down_empty_message", "CLOSE", 0, close_body("shutting_down", ""), "close")
    # PING, PONG.
    add("ping_token_zero", "PING", 1, 0, "ping")
    add("ping_token_max_id_max", "PING", U64_MAX, U64_MAX, "ping")
    add("pong_echoes_ping", "PONG", 7, 42, "pong")
    add("pong_with_id_zero", "PONG", 0, 42, "pong",
        note="the frame layer does not constrain the id of a PONG; the connection layer applies further rules (sections 2.2 and 9)")
    # REQUEST: unit commands, struct commands, optional fields, byte strings, nested arguments.
    add("request_unit_get_ur", "REQUEST", 1, command("get_ur"), "command")
    add("request_unit_namespace_status", "REQUEST", 2, command("namespace_status"), "command")
    add("request_unit_get_binary_capabilities", "REQUEST", 3, command("get_binary_capabilities"), "command")
    add("request_struct_navigate", "REQUEST", 4, command("navigate", {"url": "https://example.org/a?b=c"}), "command")
    add("request_bytes_field_read_resource", "REQUEST", 5,
        command("read_resource", {"hash": b"\x11" * 32, "offset": 0, "max_bytes": 16}), "command",
        note="same body as bodies_v1_read_resource")
    add("request_optional_absent_resolve_name", "REQUEST", 6, command("resolve_name", {"name": "spine://example"}), "command",
        note="an absent optional field is omitted, not null (section 5 rule 7)")
    add("request_optional_present_resolve_name", "REQUEST", 7,
        command("resolve_name", {"name": "spine://example", "if_none_match": key32}), "command")
    add("request_optional_absent_find_providers", "REQUEST", 8, command("find_providers", {"capability": "spine:cap/web"}), "command")
    add("request_optional_present_find_providers", "REQUEST", 9,
        command("find_providers", {"capability": "spine:cap/web", "limit": 10}), "command")
    add("request_bytes_data_publish_resource", "REQUEST", 10,
        command("publish_resource", {"record": {"v": 1}, "content_type": "text/plain", "data": b"hello"}), "command")
    add("request_mesh_call", "REQUEST", 11,
        command("mesh_call", {"peer_id": "p", "peer_key": key32, "payload": {"n": [1, 2]}}), "command")
    add("request_capability_query_unit_selector", "REQUEST", 12, command("capability_query", {"selector": "all"}), "command")
    add("request_capability_query_exact", "REQUEST", 13, command("capability_query", {"selector": {"exact": "spine:cap/x"}}), "command")
    add("request_store_knowledge", "REQUEST", 14,
        command("store_knowledge", {"key": "k", "value": {"b": [1, 2.5], "a": None}, "tags": []}), "command")
    add("request_tool_call_with_trace", "REQUEST", 15,
        command("tool_call", {"name": "t", "args": {}, "trace": trace}), "command")
    args = {
        "ints": [0, 23, 24, 255, 256, 65535, 65536, 4294967295, 4294967296, U64_MAX,
                 -1, -24, -25, -256, -257, -65536, -65537, -(2**63)],
        "floats": [0.0, -0.0, 1.5, 65504.0, 100000.0, 1.1, 1e300, 5.960464477539063e-08, -4.1, 3.4028234663852886e38],
        "scalars": [True, False, None, "", "é✓\U0001d11e"],
        "keys": {"b": 1, "aa": 2, "a": 3, "": 4},
        "nested": [[[[]]], {"x": {"y": {"z": []}}}],
    }
    add("request_tool_call_nested_json_arguments", "REQUEST", 16, command("tool_call", {"name": "t", "args": args}), "command",
        note="integers keep their integer type, floats the shortest width (section 5 rule 2); no NaN or infinity: a JSON-shaped value cannot hold them")
    # RESPONSE.
    add("response_ok_null", "RESPONSE", 1, reply_ok(None), "reply")
    add("response_with_id_zero", "RESPONSE", 0, reply_ok(None), "reply",
        note="the frame layer accepts id 0 on a RESPONSE; the connection layer rejects it because no request is in flight (section 8.1)")
    add("response_ok_integers", "RESPONSE", 2, reply_ok([0, -1, U64_MAX, -(2**63)]), "reply")
    add("response_ok_nested", "RESPONSE", 3, reply_ok({"agent_id": "a", "capabilities": [{"uri": "u", "input_schema": {}, "output_schema": {"type": "object"}}]}), "reply")
    add("response_ok_floats", "RESPONSE", 4, reply_ok({"relevance": 0.92, "half": 0.5}), "reply")
    add("response_ok_byte_string", "RESPONSE", 5, reply_ok({"data": b"\x01\x02", "n": 2}), "none",
        note="a byte string cannot be read into a JSON-shaped value, so this body is checked for determinism only")
    add("response_err_without_details", "RESPONSE", 6, reply_err("no_page_loaded", "no page"), "reply")
    add("response_err_with_details", "RESPONSE", 7, reply_err("rate_limit", "slow down", {"retry_ms": 250, "scope": ["session"]}), "reply")
    add("response_err_details_array", "RESPONSE", 8, reply_err("c", "m", [1, "two", [3]]), "reply")
    add("response_err_details_false", "RESPONSE", 9, reply_err("c", "m", False), "reply")
    add("response_err_unicode_message", "RESPONSE", 10, reply_err("x", "héllo ✓"), "reply")
    add("response_err_overloaded", "RESPONSE", 11, reply_err("overloaded", "m"), "reply",
        note="same body as bodies_v1_err_overloaded")
    # Streams, with the bodies as the implementation defines them.
    add("stream_start_minimal", "STREAM_START", 5, {"role": "assistant", "model": "m"}, "stream")
    add("stream_start_full", "STREAM_START", 5,
        {"role": "tool", "model": "m", "trace": trace,
         "decode_hints": {"temperature": 0.5, "top_p": 0.75, "top_k": 40, "max_tokens": 256,
                          "stop_sequences": ["\n\n"], "seed": 7}}, "stream")
    add("stream_token_text", "STREAM_TOKEN", 5, {"seq": 0, "data": {"text": "hi"}}, "stream")
    add("stream_token_bytes_with_usage", "STREAM_TOKEN", 5,
        {"seq": 24, "data": {"bytes": b"\x00\xff"}, "usage": {"input_tokens": 1, "output_tokens": 2}}, "stream")
    add("stream_token_tool_call", "STREAM_TOKEN", 5, {"seq": 2, "data": {"tool_call": {"name": "t", "args": {"a": 1}}}}, "stream")
    add("stream_end_stop", "STREAM_END", 5, {"reason": "stop"}, "stream")
    add("stream_end_cancelled", "STREAM_END", 5, {"reason": "cancelled"}, "stream")
    add("stream_end_error_with_usage", "STREAM_END", 5,
        {"reason": {"error": {"code": "internal", "message": "boom"}}, "usage": {"input_tokens": 5, "output_tokens": 6}}, "stream")
    add("stream_cancel_empty", "STREAM_CANCEL", 5, {}, "stream",
        note="a STREAM_CANCEL without a reason is the empty map a0 (section 10)")
    add("stream_cancel_with_reason", "STREAM_CANCEL", 5, {"reason": "user aborted"}, "stream")
    # EXT: [index, kind, payload].
    add("ext_id_zero_null_payload", "EXT", 0, [0, 1, None], "ext")
    add("ext_nonzero_id_map_payload", "EXT", 9, [3, 70000, {"k": b"\x01"}], "ext")
    # Compressed bodies (raw-block zstd, see CONVENTIONS).
    short = command("tool_call", {"name": "t", "args": {"text": "a" * 150}})
    long_ = command("tool_call", {"name": "t", "args": {"text": "ab" * 400}})
    add("request_compressed_body_under_256", "REQUEST", 20, short, "command", flags=1,
        note="zstd frame with a 1-byte content size")
    add("request_compressed_body_over_255", "REQUEST", 21, long_, "command", flags=1,
        note="zstd frame with a 2-byte content size")
    return out


def _local(versions, max_frame=MAX_FRAME, max_inflight=1, extensions=(), agent=None):
    return {"versions": list(versions), "max_frame": max_frame, "max_inflight": max_inflight,
            "extensions": list(extensions), "agent": agent}


def make_handshakes():
    out = []

    def add(name, hello_spec, local=None, note=None):
        local = local or _local([1])
        frame = frame_from_spec(hello_spec)
        data = decode_frame(frame, PRE_HELLO_MAX_FRAME)[0]
        v = {"name": name, "hello_spec": hello_spec, "hello_frame": frame.hex(), "local": local}
        try:
            hello = parse_hello(data["type"], data["body"])
            ack = negotiate(hello, local)
            body = hello_ack_body(ack["version"], ack["max_frame"], ack["max_inflight"], ack["extensions"], ack["agent"])
            ack_frame = encode_frame("HELLO_ACK", 0, encode(body))
            v["expect"] = {"result": "ack", "ack": {k: ack[k] for k in ("version", "max_frame", "max_inflight", "extensions", "agent")},
                           "ack_frame": ack_frame.hex()}
        except CloseReject as e:
            v["expect"] = {"result": "close", "code": e.code, "supported": e.supported}
        if note:
            v["note"] = note
        out.append(v)

    def hello(versions=(1,), max_frame=MAX_FRAME, extensions=(), agent=None, **unknown):
        return {"type": "HELLO", "request_id": 0, "flags": 0,
                "body": to_desc(hello_body(versions, max_frame, extensions, agent, **unknown))}

    def raw(body_hex, ftype="HELLO", rid=0):
        return {"type": ftype, "request_id": rid, "flags": 0, "body_hex": body_hex}

    # Version selection.
    add("one_common_version", hello())
    add("highest_common_version", hello([1, 2, 3]), _local([1, 2]))
    add("highest_common_version_server_unsorted", hello([1, 2, 5]), _local([5, 1, 2]))
    add("lower_common_version_only", hello([2, 7, 9]), _local([1, 2, 3]))
    add("no_common_version", hello([3, 4]), _local([1, 2]))
    add("version_zero_only_offered", hello([0]), note="version 0 is not supported (section 6.4)")
    add("u64_max_version_offered_with_one", hello([1, 2**64 - 1]), note="the highest common version is 1 (section 6.4)")
    add("no_common_version_supported_sorted_deduplicated", hello([9]), _local([4, 2, 2]),
        note="the supported list is sorted ascending without duplicates (section 6.4)")
    # max_frame.
    add("max_frame_client_smaller", hello(max_frame=65536))
    add("max_frame_client_minimum", hello(max_frame=1024))
    add("max_frame_server_smaller", hello(max_frame=MAX_FRAME), _local([1], 65536))
    add("max_frame_server_below_minimum_is_clamped", hello(max_frame=4096), _local([1], 100),
        note="a server maximum below 1024 is raised to 1024 before the minimum is taken (section 6.4)")
    add("max_frame_server_above_ceiling_is_clamped", hello(max_frame=MAX_FRAME), _local([1], 0xFFFFFFFF))
    add("max_frame_equal", hello(max_frame=4096), _local([1], 4096))
    # max_inflight, agent.
    add("max_inflight_from_server", hello(), _local([1], max_inflight=8))
    add("max_inflight_zero_becomes_one", hello(), _local([1], max_inflight=0))
    add("agent_in_ack_from_server", hello(agent="client/1"), _local([1], agent="spined/0"))
    add("agent_in_hello_only", hello(agent="client/1"))
    add("agent_128_bytes", hello(agent="a" * 128))
    add("agent_128_bytes_multibyte", hello(agent="é" * 64), note="the agent limit counts UTF-8 bytes (section 6.2)")
    # Extensions: subset and order.
    ex = ["x/a/1", "x/b/1", "x/c/1"]
    add("extensions_none_offered", hello(), _local([1], extensions=ex))
    add("extensions_none_supported", hello(extensions=ex), _local([1]))
    add("extensions_all_accepted", hello(extensions=ex), _local([1], extensions=ex))
    add("extensions_subset_keeps_client_order", hello(extensions=["x/c/1", "x/z/1", "x/a/1", "x/b/1"]),
        _local([1], extensions=["x/a/1", "x/b/1", "x/c/1"]),
        note="the index is the position in HELLO_ACK.extensions, and the server builds that list in the client's HELLO order (section 6.4)")
    add("extensions_server_order_ignored", hello(extensions=["x/b/1", "x/a/1"]), _local([1], extensions=["x/a/1", "x/b/1"]),
        note="the server's own order is ignored; the client's HELLO order fixes the index (section 6.4)")
    add("extension_id_128_bytes", hello(extensions=["a" * 128]), _local([1], extensions=["a" * 128]))
    add("extension_ids_are_case_sensitive", hello(extensions=["X/A/1"]), _local([1], extensions=["x/a/1"]))
    # Open HELLO: unknown keys are ignored (section 5 rule 9).
    add("unknown_hello_keys_ignored", hello(zz_future=1, aa_future={"k": [1]}))
    # Invalid HELLOs. The specification gives a code only for a schema failure in HELLO_ACK;
    # the implementation uses protocol_violation for HELLO as well.
    schema = "a body that is deterministic CBOR but does not fit the HELLO type or breaks a bound of section 6.2 is protocol_violation (the same rule applies to HELLO_ACK)"
    add("invalid_versions_empty", hello(versions=[]), note="versions MUST NOT be empty: protocol_violation (section 6.2)")
    add("invalid_versions_descending", hello([2, 1]), note=schema)
    add("invalid_versions_duplicate", hello([1, 1]), note=schema)
    add("invalid_max_frame_1023", hello(max_frame=1023), note=schema)
    add("invalid_max_frame_over_ceiling", hello(max_frame=MAX_FRAME + 1), note=schema)
    add("invalid_max_frame_zero", hello(max_frame=0), note=schema)
    add("invalid_max_frame_over_u32", hello(max_frame=2**32), note=schema)
    add("invalid_max_frame_negative", hello(max_frame=-1), note=schema)
    add("invalid_max_frame_float", hello(max_frame=1024.5), note=schema)
    add("invalid_extension_duplicate", hello(extensions=["x/a/1", "x/a/1"]), note=schema)
    add("invalid_extension_not_ascii", hello(extensions=["x/é/1"]), note=schema)
    add("invalid_extension_129_bytes", hello(extensions=["a" * 129]), note=schema)
    add("invalid_agent_129_bytes", hello(agent="a" * 129), note=schema)
    add("invalid_agent_130_bytes_multibyte", hello(agent="é" * 65), note=schema)
    add("invalid_agent_null", {"type": "HELLO", "request_id": 0, "flags": 0,
                               "body": {"versions": [1], "max_frame": 1024, "extensions": [], "agent": None}},
        note="a present null for an optional field is rejected (section 5 rule 7)")
    add("invalid_missing_extensions", {"type": "HELLO", "request_id": 0, "flags": 0,
                                       "body": {"versions": [1], "max_frame": 1024}}, note=schema)
    add("invalid_missing_versions", {"type": "HELLO", "request_id": 0, "flags": 0,
                                     "body": {"max_frame": 1024, "extensions": []}}, note=schema)
    add("invalid_versions_has_text", {"type": "HELLO", "request_id": 0, "flags": 0,
                                      "body": {"versions": ["1"], "max_frame": 1024, "extensions": []}}, note=schema)
    add("invalid_extension_is_not_text", {"type": "HELLO", "request_id": 0, "flags": 0,
                                          "body": {"versions": [1], "max_frame": 1024, "extensions": [1]}}, note=schema)
    add("invalid_body_is_array", {"type": "HELLO", "request_id": 0, "flags": 0, "body": [1]}, note=schema)
    add("invalid_body_is_null", {"type": "HELLO", "request_id": 0, "flags": 0, "body": None}, note=schema)
    # Non-text keys (section 5 rule 9). Each is a HELLO with every required field, so the
    # key is the only defect; a byte-string key spelling a field name must not stand in for it.
    tail = "69 6d 61 78 5f 66 72 61 6d 65 19 04 00 6a 65 78 74 65 6e 73 69 6f 6e 73 80"
    add("invalid_body_integer_key", raw("a4 00 01 68 76 65 72 73 69 6f 6e 73 81 01 " + tail),
        note="a map key that is not text: protocol_violation (section 5 rule 9)")
    add("invalid_body_unknown_byte_string_key", raw("a4 41 78 01 68 76 65 72 73 69 6f 6e 73 81 01 " + tail),
        note="a byte-string key is not ignored as an unknown key: protocol_violation (section 5 rule 9)")
    add("invalid_body_byte_string_key_spells_a_field", raw("a3 48 76 65 72 73 69 6f 6e 73 81 01 " + tail),
        note="a byte-string key spelling 'versions' does not supply that field: protocol_violation (section 5 rule 9)")
    add("invalid_body_indefinite_map", raw("bf ff"), note="a CBOR rule failure in the body is malformed_frame (section 7.3)")
    add("invalid_body_trailing_bytes", raw("a0 00"), note="trailing bytes: malformed_frame (section 7.3)")
    add("invalid_body_unsorted_keys", raw("a2 61 62 01 61 61 02"), note="unsorted keys: malformed_frame (section 7.3)")
    add("invalid_body_empty", raw(""), note="an empty body is not a CBOR item: malformed_frame")
    add("invalid_frame_type_is_ping", {"type": "PING", "request_id": 1, "flags": 0, "body": 1},
        note="a frame other than HELLO where HELLO is required: protocol_violation (section 7.3)")
    return out


def build_document():
    doc = {
        "format": "spine-wire-vectors",
        "version": 1,
        "conventions": CONVENTIONS,
        "valid_bodies": make_valid_bodies(),
        "invalid_bodies": make_invalid_bodies(),
        "valid_frames": make_valid_frames(),
        "invalid_frames": make_invalid_frames(),
        "incomplete_frames": make_incomplete_frames(),
        "handshakes": make_handshakes(),
    }
    return doc


# ---------------------------------------------------------------------------
# Cross-checks (run by generate and check)
# ---------------------------------------------------------------------------


def cross_checks(doc):
    """The three hand-checked bodies of tests/bodies_v1.rs and the worked
    examples of spec C.12 must be reproduced byte for byte."""
    by = {v["name"]: v for v in doc["valid_bodies"]}
    frames = {v["name"]: v for v in doc["valid_frames"]}
    pairs = [
        (by["bodies_v1_get_ur"]["hex"], BODIES_V1_GET_UR, "bodies_v1 GET_UR"),
        (by["bodies_v1_read_resource"]["hex"], BODIES_V1_READ_RESOURCE, "bodies_v1 READ_RESOURCE"),
        (by["bodies_v1_err_overloaded"]["hex"], BODIES_V1_ERR_OVERLOADED, "bodies_v1 ERR_OVERLOADED_BODY"),
        (frames["c12_hello"]["hex"], _hex(C12_HELLO).hex(), "spec C.12 example 1 (HELLO)"),
        (frames["c12_request_tool_call"]["hex"], _hex(C12_REQUEST).hex(), "spec C.12 example 2 (REQUEST)"),
        (frames["c12_ping_token_42_id_7"]["hex"], _hex(C12_PING).hex(), "spec C.12 PING"),
    ]
    failures = ["%s: produced %s, source has %s" % (n, a, b) for a, b, n in pairs if a != b]
    return failures


# ---------------------------------------------------------------------------
# check
# ---------------------------------------------------------------------------


def _typed_check(v, frame):
    typed = v["typed"]
    body = frame["body"]
    code = frame["type"]
    name = TYPE_NAMES[code]
    if typed == "hello":
        parse_hello(code, body)
    elif typed == "hello_ack":
        parse_hello_ack(code, body)
    elif typed == "close":
        parse_close(code, body)
    elif typed == "command":
        validate_request_value(decode(body))
    elif typed in ("reply", "none"):
        validate_reply_value(decode(body))
    elif typed in ("ping", "pong"):
        token = decode(body)
        if not _is_uint(token):
            raise ValueError("token is not an unsigned integer")
    elif typed == "stream":
        validate_stream_value(name, decode(body))
    elif typed == "ext":
        value = decode(body)
        if not (isinstance(value, list) and len(value) == 3 and _is_uint(value[0]) and _is_uint(value[1])):
            raise ValueError("EXT body must be [index, kind, payload]")
    else:
        raise ValueError("unknown typed kind %r" % typed)


def check_valid_body(v):
    data = bytes.fromhex(v["hex"])
    if encode(from_desc(v["value"])) != data:
        return "re-encoding the description gives %s" % encode(from_desc(v["value"])).hex()[:80]
    validate(data)
    if encode(decode(data)) != data:
        return "decode then encode changes the bytes"
    return None


def check_invalid_body(v):
    try:
        validate(bytes.fromhex(v["hex"]))
    except Reject as e:
        return None if e.reason == v["reason"] else "rejected as %s, expected %s" % (e.reason, v["reason"])
    return "accepted, expected %s" % v["reason"]


def check_valid_frame(v):
    data = bytes.fromhex(v["hex"])
    if frame_from_spec(v) != data:
        return "re-encoding the description gives %s" % frame_from_spec(v).hex()[:80]
    frame, used = decode_frame(data)
    if used != len(data):
        return "decode consumed %d of %d bytes" % (used, len(data))
    if (frame["type"] != FRAME_TYPES[v["type"]] or frame["request_id"] != _id_from(v["request_id"])
            or frame["flags"] != v["flags"] or frame["type"] != v["type_code"]):
        return "decoded header differs from the description"
    if frame["body"] is None or frame["body"].hex() != v["body_hex"]:
        return "decoded body differs from body_hex"
    validate(frame["body"])
    if "body" in v and encode(from_desc(v["body"])).hex() != v["body_hex"]:
        return "body description does not give body_hex"
    _typed_check(v, frame)
    return None


def check_invalid_frame(v):
    data = frame_from_build(v["build"])
    if data.hex() != v["hex"]:
        return "build gives %s, not the stored hex" % data.hex()[:80]
    try:
        decode_frame(data, v["max_frame"])
    except FrameReject as e:
        if e.error != v["error"] or e.close_code != v["close_code"]:
            return "rejected as %s/%s, expected %s/%s" % (e.error, e.close_code, v["error"], v["close_code"])
        return None
    return "accepted, expected %s" % v["error"]


def check_incomplete_frame(v):
    try:
        decode_frame(bytes.fromhex(v["hex"]), v["max_frame"])
    except FrameReject as e:
        if (e.error, e.close_code, e.needed) != ("NeedMore", None, v["needed"]):
            return "got %s needed=%s, expected needed=%s" % (e.error, e.needed, v["needed"])
        return None
    return "decoded, expected NeedMore"


def check_handshake(v):
    frame = frame_from_spec(v["hello_spec"])
    if frame.hex() != v["hello_frame"]:
        return "hello_spec gives %s, not the stored frame" % frame.hex()[:80]
    decoded, _ = decode_frame(frame, PRE_HELLO_MAX_FRAME)
    expect = v["expect"]
    try:
        hello = parse_hello(decoded["type"], decoded["body"])
        ack = negotiate(hello, v["local"])
    except CloseReject as e:
        if expect["result"] != "close":
            return "closed with %s, expected an ack" % e.code
        if e.code != expect["code"] or e.supported != expect["supported"]:
            return "closed with %s %s, expected %s %s" % (e.code, e.supported, expect["code"], expect["supported"])
        return None
    if expect["result"] != "ack":
        return "negotiated, expected a close with %s" % expect["code"]
    if ack != expect["ack"]:
        return "ack %s, expected %s" % (ack, expect["ack"])
    body = hello_ack_body(ack["version"], ack["max_frame"], ack["max_inflight"], ack["extensions"], ack["agent"])
    if encode_frame("HELLO_ACK", 0, encode(body)).hex() != expect["ack_frame"]:
        return "ack_frame differs"
    parse_hello_ack(FRAME_TYPES["HELLO_ACK"], encode(body))
    return None


CHECKS = [
    ("valid_bodies", check_valid_body, 60),
    ("invalid_bodies", check_invalid_body, 60),
    ("valid_frames", check_valid_frame, 40),
    ("invalid_frames", check_invalid_frame, 40),
    ("incomplete_frames", check_incomplete_frame, 4),
    ("handshakes", check_handshake, 30),
]


def check_document(doc):
    """Returns a list of failure strings. Any exception raised while checking
    a vector is a failure of that vector (a corrupt file must not crash the
    checker)."""
    failures = []
    if doc.get("format") != "spine-wire-vectors" or doc.get("version") != 1:
        failures.append("document: not a spine-wire-vectors version 1 file")
    for section, fn, minimum in CHECKS:
        vectors = doc.get(section)
        if not isinstance(vectors, list) or len(vectors) < minimum:
            failures.append("%s: missing or fewer than %d vectors" % (section, minimum))
            continue
        names = set()
        for v in vectors:
            name = v.get("name", "<unnamed>") if isinstance(v, dict) else "<not an object>"
            if name in names:
                failures.append("%s/%s: duplicate name" % (section, name))
            names.add(name)
            try:
                problem = fn(v)
            except Exception as e:  # noqa: BLE001 - any failure is a failed vector
                problem = "%s: %s" % (type(e).__name__, e)
            if problem:
                failures.append("%s/%s: %s" % (section, name, problem))
    # Coverage: every frame type, every rejection reason.
    types = {v.get("type") for v in doc.get("valid_frames", []) if isinstance(v, dict)}
    for t in FRAME_TYPES:
        if t not in types:
            failures.append("valid_frames: no vector for %s" % t)
    reasons = {v.get("reason") for v in doc.get("invalid_bodies", []) if isinstance(v, dict)}
    for r in REJECT_REASONS:
        if r not in reasons:
            failures.append("invalid_bodies: no vector for %s" % r)
    failures.extend("cross-check: " + f for f in _safe_cross_checks(doc))
    return failures


def _safe_cross_checks(doc):
    try:
        return cross_checks(doc)
    except Exception as e:  # noqa: BLE001
        return ["%s: %s" % (type(e).__name__, e)]


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def render(doc):
    return json.dumps(doc, indent=2, sort_keys=True, ensure_ascii=True) + "\n"


def main(argv):
    if len(argv) >= 2 and argv[1] == "generate":
        doc = build_document()
        problems = cross_checks(doc)
        if problems:
            sys.stderr.write("cross-check failed:\n  " + "\n  ".join(problems) + "\n")
            return 2
        failures = check_document(json.loads(render(doc)))
        if failures:
            sys.stderr.write("the generated document does not pass its own check:\n  " + "\n  ".join(failures) + "\n")
            return 2
        sys.stdout.buffer.write(render(doc).encode("ascii"))
        return 0
    if len(argv) == 3 and argv[1] == "check":
        with open(argv[2], "rb") as f:
            raw = f.read()
        if b"\r" in raw:
            sys.stderr.write("%s contains CR bytes: it must use LF line endings\n" % argv[2])
            return 1
        doc = json.loads(raw.decode("utf-8"))
        failures = check_document(doc)
        counts = ", ".join("%s=%d" % (s, len(doc.get(s, []))) for s, _, _ in CHECKS)
        if failures:
            sys.stderr.write("%d failure(s):\n  %s\n" % (len(failures), "\n  ".join(failures)))
            return 1
        print("ok: all vectors pass (%s)" % counts)
        return 0
    if len(argv) == 3 and argv[1] == "decode-body":
        data = bytes.fromhex(argv[2])
        try:
            print(json.dumps(to_desc(decode(data)), sort_keys=True))
        except Reject as e:
            print("rejected: %s" % e)
            return 1
        return 0
    if len(argv) in (3, 4) and argv[1] == "decode-frame":
        data = bytes.fromhex(argv[2])
        try:
            frame, used = decode_frame(data, int(argv[3]) if len(argv) == 4 else MAX_FRAME)
        except FrameReject as e:
            print("rejected: %s (close code %s)" % (e, e.close_code))
            return 1
        shown = {"version": frame["version"], "type": TYPE_NAMES[frame["type"]], "flags": frame["flags"],
                 "request_id": frame["request_id"], "consumed": used}
        if frame["body"] is not None:
            shown["body_hex"] = frame["body"].hex()
            try:
                shown["body"] = to_desc(decode(frame["body"]))
            except Reject as e:
                shown["body_rejected"] = str(e)
        print(json.dumps(shown, sort_keys=True))
        return 0
    sys.stderr.write(__doc__.split("Usage:")[1].split("What it covers")[0].replace("\n    ", "\n"))
    return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv))
