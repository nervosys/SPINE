#!/usr/bin/env python3
"""spine-wire v1: an independent reference implementation and the vector generator.

Written from the specification text (docs/spec/00-wire.md), not from the Rust
code, and since the reconciliation of the specification with the implementation
the two agree. Section numbers in the code comments below ("C.5 rule 2") use the
labels of the earlier document; Appendix D of the specification maps them to the
current sections. It shares nothing with `src/spine-protocol` and uses
only the Python standard library (3.8 or later) and the pure-Python RFC 8032
Ed25519 of `scripts/names_v1.py` (signing, verification, and the strict
verification of `verify_strict`: S below the group order, a small-order key or
R refused), which checks itself against the RFC's test vectors. It is the "second
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
connection state, the close codes of C.7), HELLO, HELLO_ACK (with `binding`,
`authority` and `time`) and CLOSE bodies, the IDENTIFY body and its signed
message (section 6.6), a model of the server's IDENTIFY state rules,
and `negotiate` (C.6, C.10), builders for REQUEST and RESPONSE bodies
(C.5 rule 7, D), the `delete_resource` proof and its signed message
(section 12.2), the shapes of the two delete replies (section 12.4) and the
list of error codes (section 13), which is also compared with the table of the
specification text, the bodies of `AUTH_REQUEST` and `RECEIPT` and the digest a
receipt carries (sections 8.5 and 8.6).

What it does not cover:

* zstd. The standard library has no zstd. A compressed body is handled by
  structure: `u32 uncompressed_len || zstd frame`, the zstd magic
  `28 b5 2f fd`, and the declared length against `max_frame - 11`. The one
  zstd feature implemented is a frame made only of raw (stored) blocks, which
  is valid zstd and which this script can both write and read; a frame with
  compressed blocks is reported as "opaque" and its length is not checked.
* Connection state: id uniqueness among in-flight requests, direction,
  "HELLO first", a second HELLO, extension indexes, timeouts, max_inflight.
  Those need a stream of frames, not one frame, and have no vectors here,
  except the IDENTIFY window (`identify_sessions`).
* Semantics of commands (what `navigate` does). Only the wire shape.

A "note" on a vector explains the rule it exercises and cites the section of
the specification that states it.
"""

import hashlib
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import names_v1  # noqa: E402  (the pure-Python RFC 8032 Ed25519 of the name reference)

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
AUTHORITY_MODES = ["off", "audit", "required"]
IDENTIFY_DOMAIN = b"spine-wire-identify-v1\x00"
DELETE_RESOURCE_DOMAIN = b"spine-wire-delete-resource-v1\x00"
# The fixed test key of every delete-proof vector.
DELETE_SEED = bytes([0x21]) * 32
# The fixed test key of every IDENTIFY vector (Ed25519 signatures are deterministic).
IDENTIFY_SEED = bytes([0x11]) * 32
DEFAULT_BINDING = bytes([0x42]) * 32
DEFAULT_TIME = 1700000000

FRAME_TYPES = {
    "HELLO": 0x01,
    "HELLO_ACK": 0x02,
    "CLOSE": 0x03,
    "PING": 0x04,
    "PONG": 0x05,
    "IDENTIFY": 0x06,
    "REQUEST": 0x10,
    "RESPONSE": 0x11,
    "AUTH_REQUEST": 0x12,
    "RECEIPT": 0x13,
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

# D.1 and D.2: the 17 kept commands plus tool_call and capability_query, and
# the two delete commands of the 2026-11-24 revision (section 12).
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
    "delete_resource",
    "delete_knowledge",
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
    "delete_resource": (["hash", "owner", "proof"], []),
    "delete_knowledge": (["key"], []),
}

# Byte-string fields of a command and their exact lengths (section 12.2: a byte
# field of the wrong length is invalid_request), and the fields that must be text.
BYTE_FIELDS = {
    "resolve_name": {"if_none_match": 32},
    "fetch_name": {"if_none_match": 32},
    "read_resource": {"hash": 32},
    "mesh_call": {"peer_key": 32},
    "delete_resource": {"hash": 32, "owner": 32, "proof": 64},
}
TEXT_FIELDS = {"delete_knowledge": ["key"]}

# Section 13: every error code a v1 daemon produces, in the order of the table
# there. `check` compares this list with that table.
ERROR_CODES = [
    "overloaded", "unknown_command", "invalid_request", "internal", "rate_limit",
    "authority_required", "authority_not_enabled",
    "authority_step_1", "authority_step_2", "authority_step_3", "authority_step_4",
    "authority_step_5", "authority_step_6", "authority_step_7", "authority_step_8",
    "authority_step_9", "authority_step_10", "authority_step_11",
    "session_capacity", "session_not_found", "session_expired", "no_page_loaded",
    "capability_denied", "execution_failed", "knowledge_entry_too_large", "knowledge_capacity",
    "action_unsupported", "namespace_unavailable", "invalid_peer_id", "mesh_call_failed",
    "batch_too_large", "lookup_failed", "lookup_unavailable", "record_expired",
    "malformed_record", "record_rejected", "publish_failed", "resource_not_found",
    "delete_refused", "invalid_seed", "invalid_args", "unsupported_tool", "parse_error",
    "outbound_disabled", "outbound_address_denied", "outbound_scheme_denied",
    "outbound_credentials_denied", "outbound_fragment_denied", "outbound_redirect_denied",
    "outbound_encoding_denied", "outbound_response_limit", "outbound_busy", "outbound_timeout",
    "outbound_invalid_url", "outbound_invalid_host", "outbound_invalid_port",
    "outbound_dns_failed", "outbound_client_failed", "outbound_request_failed",
    "outbound_body_failed", "outbound_invalid_utf8", "outbound_url_limit", "outbound_dns_limit",
    "outbound_http_status",
]
PROTOCOL_ERROR_CODES = ["overloaded", "unknown_command", "invalid_request"]


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
    connection state (as implemented): HELLO, HELLO_ACK, CLOSE and IDENTIFY
    carry 0; REQUEST, AUTH_REQUEST, RECEIPT and PING carry a nonzero id. Other
    types are not checked here."""
    ok = True
    if ftype in (0x01, 0x02, 0x03, 0x06):
        ok = request_id == 0
    elif ftype in (0x10, 0x12, 0x13, 0x04):
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


def hello_ack_body(version, max_frame, max_inflight, extensions, agent=None,
                   binding=DEFAULT_BINDING, authority="off", time=DEFAULT_TIME):
    body = {
        "version": version,
        "max_frame": max_frame,
        "max_inflight": max_inflight,
        "extensions": list(extensions),
        "binding": bytes(binding),
        "authority": authority,
        "time": time,
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
    binding = _need(value, "binding", lambda v: isinstance(v, bytes) and len(v) == 32, "32 bytes")
    authority = _need(value, "authority", lambda v: isinstance(v, str) and v in AUTHORITY_MODES,
                      "one of off, audit, required")
    time = _need(value, "time", _is_uint, "an unsigned integer")
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
        "binding": binding.hex(),
        "authority": authority,
        "time": time,
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
    versions, max_frame, max_inflight, extensions, agent (None for no agent),
    binding (64 hex characters), authority and time: the per-connection values
    of section 6.3, which a real server draws fresh and a vector fixes.

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
        "binding": local["binding"],
        "authority": local["authority"],
        "time": local["time"],
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
    for field, length in BYTE_FIELDS.get(name, {}).items():
        if field in fields and not (isinstance(fields[field], bytes) and len(fields[field]) == length):
            raise ValueError("invalid_request: %s.%s must be a byte string of %d bytes" % (name, field, length))
    for field in TEXT_FIELDS.get(name, []):
        if field in fields and not isinstance(fields[field], str):
            raise ValueError("invalid_request: %s.%s must be text" % (name, field))
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


MAX_CHAIN_ITEMS = 8
MAX_RECEIPT_BYTES = 512
RESULT_DOMAIN = b"spine-wire-result-v1\x00"


def validate_auth_request_value(value):
    """Shape of an AUTH_REQUEST body (decoded; section 8.5): returns the name of
    its command, or raises ValueError with the answer code in the message. The
    body is exactly {auth: {chain, invocation}, command}; chain is an array of
    byte strings of any length (the gate, not the wire layer, refuses an empty
    chain, more than eight grants and an item over its bound), invocation is a
    byte string, and command is judged as a REQUEST body is. A body of the wrong
    shape whose command member names no command is unknown_command, as for a
    REQUEST (7.4); every other body of the wrong shape is invalid_request."""
    problem = None
    auth = value.get("auth") if isinstance(value, dict) else None
    if not isinstance(value, dict) or set(value) != {"auth", "command"}:
        problem = "not the map {auth, command}"
    elif not (isinstance(auth, dict) and set(auth) == {"chain", "invocation"}):
        problem = "auth is not the map {chain, invocation}"
    elif not (isinstance(auth["chain"], list) and all(isinstance(g, bytes) for g in auth["chain"])):
        problem = "auth.chain is not an array of byte strings"
    elif not isinstance(auth["invocation"], bytes):
        problem = "auth.invocation is not a byte string"
    if problem is not None:
        if isinstance(value, dict) and "command" in value:
            try:
                validate_request_value(value["command"])
            except ValueError as e:
                if str(e).startswith("unknown_command"):
                    raise
        raise ValueError("invalid_request: " + problem)
    return validate_request_value(value["command"])


def validate_receipt_bytes(body):
    """Why `body` (bytes) is not a RECEIPT body (section 8.6), or None: at most
    512 bytes, deterministic CBOR, and the closed map {body: map, sig: 64 bytes}."""
    if len(body) > MAX_RECEIPT_BYTES:
        return "longer than %d bytes" % MAX_RECEIPT_BYTES
    try:
        validate(body)
    except Reject as e:
        return "not deterministic CBOR (%s)" % e.reason
    value = decode(body)
    if not isinstance(value, dict) or set(value) != {"body", "sig"}:
        return "not the map {body, sig}"
    if not isinstance(value["body"], dict):
        return "body is not a map"
    if not (isinstance(value["sig"], bytes) and len(value["sig"]) == 64):
        return "sig is not a byte string of 64 bytes"
    return None


def result_digest(response_body):
    """The `result` of a receipt: SHA-256 of "spine-wire-result-v1" 00 and the
    body of the RESPONSE the receipt belongs to (section 8.6)."""
    return hashlib.sha256(RESULT_DOMAIN + response_body).digest()


def command_op(command_item_bytes):
    """The `op` of an invocation: SHA-256 of the encoding of the command item
    (section 8.5), which is the body a plain REQUEST for it would have."""
    return hashlib.sha256(command_item_bytes).digest()


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
    "command, auth_request, receipt, reply, ping, pong, stream, hello, hello_ack, identify, close, ext, or none (decode and re-encode of the typed value is not offered, e.g. a result with byte strings).",
    "A frame with flags 1 has the body u32 uncompressed_len, then a zstd frame. These vectors store the body in a raw-block zstd frame (valid zstd, not smaller). "
    "A receiver must decode them to 'body_hex'; they are not byte-for-byte what a compressing sender would emit, so re-encoding is not part of the check.",
    "invalid_frames: whole frames (or the prefix needed to decide) that a receiver using 'max_frame' as the maximum in force must reject with 'error' (the Rust frame::FrameError variant) and 'close_code' (C.7 wire text). "
    "'build' re-derives 'hex'. incomplete_frames: valid prefixes; 'needed' is the least number of further bytes (FrameError::NeedMore).",
    "The order of the stateless frame checks is: length, version, type, flags, request id, then the body (specification section 7.2).",
    "handshakes: 'hello_frame' is a whole HELLO frame, 'local' the server configuration (with the per-connection 'binding' as hex, 'authority' and 'time', "
    "which a real server draws fresh and a vector fixes), 'expect' the outcome of validating the HELLO and negotiating: "
    "an 'ack' (with the whole 'ack_frame') or a 'close' with its code and 'supported'. CLOSE 'message' text is not specified and not part of a vector.",
    "hello_acks: HELLO_ACK bodies ('body_hex', described by 'body') that a client accepts ('expect' ok) or refuses with the CLOSE code in 'expect' (section 6.3).",
    "identify: an IDENTIFY body ('body_hex') judged against the connection whose HELLO and HELLO_ACK bodies are 'hello_body' and 'ack_body'. 'expect' is 'valid' "
    "or the CLOSE code. A valid vector also gives the test 'seed' (Ed25519 signatures are deterministic), the proven 'key', the signed 'message' M (section 6.6) and the 'signature'.",
    "identify_sessions: 'hello_frame' and 'local' make a handshake; the server then receives the 'frames' (hex) in order. 'expect' is "
    "{result: ok, identity: hex or null} when all are accepted and the stream then ends, or {result: close, code} at the first one refused.",
    "name_results: a RESPONSE 'ok' value for resolve_name (section 12.5), 'body_hex' described by 'body'. 'expect' is 'ok' or 'invalid' (the body is "
    "not a name resolution a decoder accepts). Entries of a succession log are checked for shape only: their signatures are not part of the wire layer.",
    "ed25519_strict: 'public', 'message' and 'signature' (hex) with the verdict of plain RFC 8032 verification ('plain') and of verify_strict ('strict'); "
    "a conforming server uses the strict one (section 6.6).",
    "delete_proofs: the proof of a delete_resource request (section 12.2). 'owner', 'hash', 'binding' (the connection's HELLO_ACK binding) and 'proof' are hex; "
    "'expect' is 'valid' when strict Ed25519 verification of the message \"spine-wire-delete-resource-v1\" 00 || hash || binding under 'owner' accepts, else 'invalid'. "
    "A valid vector also gives the test 'seed' (signatures are deterministic) and the signed 'message'. The daemon's order is: verify the proof first (a failure is delete_refused), then look the blob up under 'owner'.",
    "invalid_commands: REQUEST bodies ('body_hex', described by 'body') that a daemon answers with the error code 'code' (invalid_request or unknown_command, sections 7.4, 12.1, 12.2).",
    "delete_results: the ok value of a delete reply (section 12.4), 'body_hex' described by 'body', for the command named in 'command'. 'expect' is 'ok' or 'invalid' (a closed map {deleted}: a boolean for delete_resource, an unsigned integer for delete_knowledge).",
    "auth_requests: AUTH_REQUEST bodies (section 8.5), 'body_hex' described by 'body'. 'expect' is 'ok' or the answer code (invalid_request or unknown_command). "
    "An 'ok' vector also gives the 'command_hex' (the encoding of the command item) and 'op', its SHA-256, which is what an invocation's op field must equal.",
    "receipts: RECEIPT bodies (section 8.6) of 'length' bytes. 'expect' is 'ok' or 'invalid' (not the closed map {body: map, sig: 64 bytes}, or over 512 bytes). Signatures and contents are not judged here.",
    "result_digests: the 'digest' a receipt carries as its result for a RESPONSE body: SHA-256 of \"spine-wire-result-v1\" 00 and 'body_hex' (section 8.6).",
    "error_codes: the closed list of codes a v1 daemon produces (section 13) in the order of its table, with their 'count' and the three 'protocol' codes. check also compares it with the table in docs/spec/00-wire.md.",
    "A vector with a 'note' explains the rule it exercises and cites the section of docs/spec/00-wire.md that states it.",
    "Not covered: connection state other than the IDENTIFY window (id reuse, direction, HELLO first, extension index, timeouts, max_inflight) and real zstd compression.",
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
    for t in (0x00, 0x07, 0x0F, 0x14, 0x1F, 0x24, 0x7F, 0x81, 0xFF):
        add("type_0x%02x" % t, _frame_build(ftype=t))
    # Reserved flag bits (C.7 row 3).
    for bit in range(1, 8):
        add("flag_bit_%d" % bit, _frame_build(flags=1 << bit))
    add("flags_all_set", _frame_build(flags=0xFF))
    add("flags_compressed_and_reserved", _frame_build(flags=0x03, body_hex=_compressed(1)))
    # Request-id rules that need no connection state, as implemented.
    for t, name in ((0x01, "hello"), (0x02, "hello_ack"), (0x03, "close"), (0x06, "identify")):
        add("%s_with_nonzero_id" % name, _frame_build(ftype=t, request_id=1))
    add("identify_with_max_id", _frame_build(ftype=0x06, request_id=U64_MAX))
    add("hello_with_max_id", _frame_build(ftype=0x01, request_id=U64_MAX))
    add("request_with_id_zero", _frame_build(ftype=0x10, request_id=0))
    add("auth_request_with_id_zero", _frame_build(ftype=0x12, request_id=0),
        note="an AUTH_REQUEST carries a nonzero id, as a REQUEST does (section 8.5)")
    add("receipt_with_id_zero", _frame_build(ftype=0x13, request_id=0),
        note="a RECEIPT carries the nonzero id of the AUTH_REQUEST it answers (section 8.6)")
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
        hello_ack_body(1, 65536, 8, ["b/two/1", "a/one/1"], "spined/0", bytes(range(32)), "required", 1700000123),
        "hello_ack",
        note="the order of extensions fixes the extension index (sections 6.4 and 11)")
    add("hello_ack_authority_audit", "HELLO_ACK", 0,
        hello_ack_body(1, 16777216, 8, [], None, bytes([0xAA]) * 32, "audit", 0), "hello_ack")
    identify_hello = encode(hello_body([1], 16777216, []))
    identify_ack = encode(hello_ack_body(1, 16777216, 8, []))
    add("identify_empty_channel", "IDENTIFY", 0, make_identify_body(IDENTIFY_SEED, identify_hello, identify_ack),
        "identify", note="a 118-byte body and a 133-byte frame (section 6.6)")
    auth_chain = [b"\x10" * 40, b"\x11" * 56]
    auth_body = {"auth": {"chain": auth_chain, "invocation": b"\x12" * 90},
                 "command": command("navigate", {"url": "https://example.org/"})}
    add("auth_request_navigate", "AUTH_REQUEST", 3, auth_body, "auth_request",
        note="{auth: {chain, invocation}, command} (section 8.5); the grants and the invocation here are placeholders, which the wire layer does not read")
    add("auth_request_unit_command", "AUTH_REQUEST", 4,
        {"auth": {"chain": [b"\x10" * 8], "invocation": b"\x12" * 8}, "command": command("get_ur")}, "auth_request")
    add("receipt_realistic", "RECEIPT", 3, RECEIPT_EXAMPLE, "receipt",
        note="{body, sig}, at most 512 bytes (section 8.6); the signature here is a placeholder")
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
    add("request_delete_resource", "REQUEST", 17,
        command("delete_resource", {"hash": b"\x11" * 32, "owner": b"\x22" * 32, "proof": b"\x33" * 64}), "command",
        note="three byte strings of 32, 32 and 64 bytes (section 12.2); the proof here is filler")
    add("request_delete_resource_signed", "REQUEST", 18,
        command("delete_resource", {"hash": DELETE_HASH, "owner": names_v1.ed25519_public(DELETE_SEED),
                                    "proof": names_v1.ed25519_sign(DELETE_SEED, delete_message(DELETE_HASH, DEFAULT_BINDING))}),
        "command", note="the proof of the first valid vector of delete_proofs, on the connection whose binding is 42 repeated")
    add("request_delete_knowledge", "REQUEST", 19, command("delete_knowledge", {"key": "k"}), "command",
        note="one text field (section 12.2)")
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
    add("response_ok_deleted_true", "RESPONSE", 12, reply_ok({"deleted": True}), "reply",
        note="the ok value of delete_resource (section 12.4)")
    add("response_ok_deleted_count", "RESPONSE", 13, reply_ok({"deleted": 2}), "reply",
        note="the ok value of delete_knowledge (section 12.4)")
    add("response_err_delete_refused", "RESPONSE", 14, reply_err("delete_refused", "the delete proof does not verify"), "reply",
        note="the one refusal of delete_resource (section 12.2 and 13)")
    add("response_err_knowledge_capacity", "RESPONSE", 15, reply_err("knowledge_capacity", "m"), "reply")
    add("response_err_knowledge_entry_too_large", "RESPONSE", 16, reply_err("knowledge_entry_too_large", "m"), "reply")
    add("response_err_action_unsupported", "RESPONSE", 17, reply_err("action_unsupported", "m"), "reply")
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


def _local(versions, max_frame=MAX_FRAME, max_inflight=1, extensions=(), agent=None,
           binding=DEFAULT_BINDING, authority="off", time=DEFAULT_TIME):
    return {"versions": list(versions), "max_frame": max_frame, "max_inflight": max_inflight,
            "extensions": list(extensions), "agent": agent,
            "binding": bytes(binding).hex(), "authority": authority, "time": time}


ACK_FIELDS = ("version", "max_frame", "max_inflight", "extensions", "agent", "binding", "authority", "time")


def _ack_body_of(ack):
    """The HELLO_ACK body for a negotiate() result."""
    return hello_ack_body(ack["version"], ack["max_frame"], ack["max_inflight"], ack["extensions"], ack["agent"],
                          bytes.fromhex(ack["binding"]), ack["authority"], ack["time"])


def make_hello_acks():
    out = []
    base = hello_ack_body(1, 4096, 8, ["x/a/1"], "srv/1")

    def add(name, body_value, expect, note=None):
        body = encode(body_value)
        v = {"name": name, "body": to_desc(body_value), "body_hex": body.hex(), "expect": expect}
        if note:
            v["note"] = note
        out.append(v)

    def without(key):
        body = dict(base)
        del body[key]
        return body

    def with_(**changes):
        body = dict(base)
        body.update(changes)
        return body

    add("valid_off", base, "ok")
    add("valid_audit", with_(authority="audit"), "ok", note="all three texts are valid on the wire (section 6.3)")
    add("valid_required", with_(authority="required"), "ok")
    add("valid_time_zero", with_(time=0), "ok")
    add("valid_time_u64_max", with_(time=U64_MAX), "ok")
    add("valid_binding_all_zero", with_(binding=bytes(32)), "ok",
        note="a client cannot tell a random binding from a constant one")
    add("valid_without_agent", {k: v for k, v in base.items() if k != "agent"}, "ok")
    schema = "a HELLO_ACK that is deterministic CBOR but misses or mistypes a required key is protocol_violation (section 6.3)"
    for key in ("binding", "authority", "time"):
        add("invalid_missing_%s" % key, without(key), "protocol_violation", note=schema)
    add("invalid_binding_empty", with_(binding=b""), "protocol_violation", note=schema)
    add("invalid_binding_31_bytes", with_(binding=bytes(31)), "protocol_violation", note=schema)
    add("invalid_binding_33_bytes", with_(binding=bytes(33)), "protocol_violation", note=schema)
    add("invalid_binding_is_text", with_(binding="b" * 32), "protocol_violation", note=schema)
    add("invalid_binding_is_32_ints", with_(binding=[0] * 32), "protocol_violation",
        note="a byte string is never an array of integers (section 5 rule 7)")
    add("invalid_binding_is_null", with_(binding=None), "protocol_violation", note=schema)
    for name, mode in (("maybe", "maybe"), ("capitalised", "Off"), ("upper_case", "REQUIRED"),
                       ("empty", ""), ("padded", "audit ")):
        add("invalid_authority_%s" % name, with_(authority=mode), "protocol_violation",
            note="the three texts are lower case and exact (section 6.3)")
    add("invalid_authority_is_int", with_(authority=0), "protocol_violation", note=schema)
    add("invalid_authority_is_null", with_(authority=None), "protocol_violation", note=schema)
    add("invalid_time_is_text", with_(time="1700000000"), "protocol_violation", note=schema)
    add("invalid_time_negative", with_(time=-1), "protocol_violation", note=schema)
    add("invalid_time_is_float", with_(time=1.5), "protocol_violation", note=schema)
    add("invalid_time_is_null", with_(time=None), "protocol_violation", note=schema)
    return out


NAME_PROVENANCE = ("cache", "stale_cache", "local", "network", "address")
NAME_OUTCOMES = ("resolved", "unchanged", "not_found", "invalid", "unavailable", "retired", "compromised", "revoked")
MAX_LOG_ENTRIES = 64
ENTRY_KEYS = ("commitment", "effective_at", "inception_key", "index", "key", "kind", "prev", "signature")


def log_entry(index, kind="rotate"):
    """A succession entry in the JSON form of 02-names 10.1. Only its shape
    matters here."""
    return {
        "index": index,
        "kind": kind,
        "inception_key": "z" + "a" * 52,
        "key": "z" + "b" * 52,
        "prev": None if index == 0 else "c" * 52,
        "commitment": None if kind == "retire" else "d" * 52,
        "effective_at": 1700000000 + index,
        "signature": "s" + "e" * 102,
    }


def log_of(n, last_kind="rotate"):
    return [log_entry(i, last_kind if i == n - 1 else "rotate") for i in range(n)]


NAME_RECORD = {
    "name": "spine://did:z" + "a" * 52 + "/",
    "seq": 2,
    "ttl_secs": 3600,
    "published_at": 1700000000,
    "endpoints": [{"transport": "tcp", "address": "127.0.0.1:9440", "priority": 0}],
}


def _short_text(v):
    return isinstance(v, str) and 0 < len(v.encode("utf-8")) <= 128


def check_entry_shape(entry):
    """The structure rule of 00-wire 12.5 for one log entry, or a reason."""
    if not isinstance(entry, dict) or set(entry) != set(ENTRY_KEYS):
        return "not exactly the fields of a succession entry"
    for key in ("inception_key", "key", "signature"):
        if not _short_text(entry[key]):
            return "%s is not short text" % key
    for key in ("prev", "commitment"):
        if entry[key] is not None and not _short_text(entry[key]):
            return "%s is not short text or null" % key
    for key in ("index", "effective_at"):
        v = entry[key]
        if not _is_uint(v):
            return "%s is not an unsigned integer" % key
    if entry["kind"] not in ("rotate", "retire"):
        return "kind is not rotate or retire"
    return None


def check_log_shape(log, minimum):
    if not isinstance(log, list):
        return "a log is not an array"
    if len(log) < minimum:
        return "fewer than %d entries" % minimum
    if len(log) > MAX_LOG_ENTRIES:
        return "more than %d entries" % MAX_LOG_ENTRIES
    for entry in log:
        problem = check_entry_shape(entry)
        if problem:
            return problem
    return None


def name_result_problem(value):
    """Why `value` is not a name resolution (section 12.5), or None when it is:
    an externally tagged variant with exactly its fields and a log of the right
    size and shape."""
    if not isinstance(value, dict) or len(value) != 1:
        return "not a single-variant map"
    (outcome, fields), = value.items()
    if outcome not in NAME_OUTCOMES:
        return "unknown outcome"
    if not isinstance(fields, dict):
        return "the variant is not a map"
    expect = {
        "resolved": {"record", "provenance", "log"},
        "unchanged": {"ttl_secs"},
        "not_found": {"name"},
        "invalid": {"name", "reason"},
        "unavailable": {"name", "reason"},
        "retired": {"name", "log"},
        "compromised": {"name", "logs"},
        "revoked": {"record", "provenance", "log"},
    }[outcome]
    if set(fields) != expect:
        return "the fields are not exactly %s" % sorted(expect)
    if "provenance" in fields and fields["provenance"] not in NAME_PROVENANCE:
        return "provenance is not one of the five texts"
    for key in ("name", "reason"):
        if key in fields and not isinstance(fields[key], str):
            return "%s is not text" % key
    if "ttl_secs" in fields and not _is_uint(fields["ttl_secs"], 0xFFFFFFFF):
        return "ttl_secs is not a u32"
    if outcome in ("resolved", "revoked"):
        return check_log_shape(fields["log"], 0)
    if outcome == "retired":
        return check_log_shape(fields["log"], 1)
    if outcome == "compromised":
        logs = fields["logs"]
        if not isinstance(logs, list) or len(logs) != 2:
            return "not exactly two logs"
        for log in logs:
            problem = check_log_shape(log, 1)
            if problem:
                return problem
    return None


def make_name_results():
    out = []

    def add(name, value, note=None):
        body = encode(value)
        v = {"name": name, "body": to_desc(value), "body_hex": body.hex(),
             "expect": "invalid" if name.startswith("invalid_") else "ok"}
        if note:
            v["note"] = note
        problem = name_result_problem(value)
        assert (problem is not None) == (v["expect"] == "invalid"), (name, problem)
        out.append(v)

    def resolved(log, **changes):
        fields = {"record": NAME_RECORD, "provenance": "network", "log": log}
        fields.update(changes)
        return {"resolved": fields}

    def retired(log, **changes):
        fields = {"name": NAME_RECORD["name"], "log": log}
        fields.update(changes)
        return {"retired": fields}

    def compromised(logs, **changes):
        fields = {"name": NAME_RECORD["name"], "logs": logs}
        fields.update(changes)
        return {"compromised": fields}

    def revoked(log, **changes):
        fields = {"record": NAME_RECORD, "provenance": "local", "log": log}
        fields.update(changes)
        return {"revoked": fields}

    note_log = "the log is always present, 0 to 64 entries (section 12.5)"
    add("valid_resolved_no_log", resolved([]), note=note_log + ": empty is the whole truth only for a name that never rotated")
    add("valid_resolved_one_entry", resolved(log_of(1)), note=note_log)
    add("valid_resolved_three_entries", resolved(log_of(3)), note=note_log)
    add("valid_resolved_max_log", resolved(log_of(MAX_LOG_ENTRIES)), note=note_log)
    for provenance in NAME_PROVENANCE:
        add("valid_resolved_provenance_%s" % provenance, resolved([], provenance=provenance))
    add("valid_unchanged", {"unchanged": {"ttl_secs": 300}}, note="no body, so no log")
    add("valid_not_found", {"not_found": {"name": "spine://did:zx/"}})
    add("valid_invalid", {"invalid": {"name": "x", "reason": "no scheme"}})
    add("valid_unavailable", {"unavailable": {"name": "spine://did:zx/", "reason": "the log is not held"}},
        note="a node that holds a rotated record without its log answers truthfully with this, never resolved with an empty log (section 12.5)")
    add("valid_retired_one_entry", retired(log_of(1, "retire")), note="1 to 64 entries, the last of which retires the name")
    add("valid_retired_two_entries", retired(log_of(2, "retire")))
    add("valid_retired_max_log", retired(log_of(MAX_LOG_ENTRIES, "retire")))
    add("valid_compromised_two_logs", compromised([log_of(1), log_of(1)]),
        note="exactly two logs of 1 to 64 entries: the equivocation proof (section 12.5)")
    add("valid_compromised_unequal_logs", compromised([log_of(1), log_of(3)]))
    add("valid_compromised_max_logs", compromised([log_of(MAX_LOG_ENTRIES), log_of(MAX_LOG_ENTRIES)]))
    add("valid_revoked_no_log", revoked([]), note="the tombstone and the log it is judged under, 0 to 64 entries")
    add("valid_revoked_with_log", revoked(log_of(2)))

    shape = "a name resolution is one variant with exactly its fields (section 12.5)"
    add("invalid_resolved_without_log", {"resolved": {"record": NAME_RECORD, "provenance": "network"}},
        note="the log is mandatory in resolved: it is how a client verifies a rotated name")
    add("invalid_resolved_log_null", resolved(None), note=shape)
    add("invalid_resolved_log_is_a_map", resolved({}), note=shape)
    add("invalid_resolved_log_over_long", resolved(log_of(MAX_LOG_ENTRIES + 1)), note="more than 64 entries")
    add("invalid_resolved_extra_key", resolved([], extra=1), note=shape)
    add("invalid_resolved_provenance_unknown", resolved([], provenance="cached"), note=shape)
    add("invalid_resolved_provenance_is_int", resolved([], provenance=2), note=shape)
    add("invalid_resolved_log_entry_is_text", resolved(["entry"]), note="an entry is a map of the eight fields")
    entry = log_entry(0)
    for key in ENTRY_KEYS:
        missing = {k: v for k, v in entry.items() if k != key}
        add("invalid_resolved_entry_missing_%s" % key, resolved([missing]), note="an entry has exactly the eight fields")
    add("invalid_resolved_entry_extra_key", resolved([dict(entry, extra=1)]), note="an entry has exactly the eight fields")
    add("invalid_resolved_entry_kind_unknown", resolved([dict(entry, kind="revoke")]), note="kind is rotate or retire")
    add("invalid_resolved_entry_index_is_text", resolved([dict(entry, index="0")]), note="index is an unsigned integer")
    add("invalid_resolved_entry_index_negative", resolved([dict(entry, index=-1)]), note="index is an unsigned integer")
    add("invalid_resolved_entry_signature_is_bytes", resolved([dict(entry, signature=b"\x00" * 64)]),
        note="the log travels in the JSON form: keys and signatures are text")
    add("invalid_resolved_entry_signature_empty", resolved([dict(entry, signature="")]), note="the text fields are not empty")
    add("invalid_resolved_entry_key_over_long", resolved([dict(entry, key="k" * 129)]), note="the text fields are at most 128 bytes")
    add("invalid_resolved_entry_prev_is_int", resolved([dict(entry, prev=0)]), note="prev is text or null")
    add("invalid_unchanged_with_log", {"unchanged": {"ttl_secs": 1, "log": []}}, note=shape)
    add("invalid_unchanged_ttl_over_u32", {"unchanged": {"ttl_secs": 4294967296}}, note="ttl_secs is a u32")
    add("invalid_not_found_with_log", {"not_found": {"name": "n", "log": []}}, note=shape)
    add("invalid_unknown_outcome", {"deleted": {"name": "n"}}, note="there is no other outcome")
    add("invalid_two_outcomes", {"not_found": {"name": "n"}, "unchanged": {"ttl_secs": 1}}, note=shape)
    add("invalid_outcome_is_text", "unchanged", note=shape)
    add("invalid_retired_empty_log", retired([]), note="retired carries 1 to 64 entries")
    add("invalid_retired_without_log", {"retired": {"name": NAME_RECORD["name"]}}, note=shape)
    add("invalid_retired_over_long", retired(log_of(MAX_LOG_ENTRIES + 1, "retire")), note="more than 64 entries")
    add("invalid_retired_logs_instead_of_log", {"retired": {"name": "n", "logs": [log_of(1)]}}, note=shape)
    add("invalid_retired_with_record", retired(log_of(1, "retire"), record=NAME_RECORD), note=shape)
    add("invalid_compromised_no_logs", compromised([]), note="exactly two logs")
    add("invalid_compromised_one_log", compromised([log_of(1)]), note="exactly two logs")
    add("invalid_compromised_three_logs", compromised([log_of(1), log_of(1), log_of(1)]), note="exactly two logs")
    add("invalid_compromised_empty_log", compromised([log_of(1), []]), note="each log has 1 to 64 entries")
    add("invalid_compromised_over_long_log", compromised([log_of(1), log_of(MAX_LOG_ENTRIES + 1)]), note="more than 64 entries")
    add("invalid_compromised_log_instead_of_logs", {"compromised": {"name": "n", "log": log_of(1)}}, note=shape)
    add("invalid_compromised_logs_is_a_map", compromised({}), note=shape)
    add("invalid_revoked_without_log", {"revoked": {"record": NAME_RECORD, "provenance": "local"}}, note=shape)
    add("invalid_revoked_without_provenance", {"revoked": {"record": NAME_RECORD, "log": []}}, note=shape)
    add("invalid_revoked_over_long_log", revoked(log_of(MAX_LOG_ENTRIES + 1)), note="more than 64 entries")
    return out


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
            ack_frame = encode_frame("HELLO_ACK", 0, encode(_ack_body_of(ack)))
            v["expect"] = {"result": "ack", "ack": {k: ack[k] for k in ACK_FIELDS}, "ack_frame": ack_frame.hex()}
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
    # The per-connection values of section 6.3.
    add("authority_audit_reported", hello(), _local([1], authority="audit"),
        note="the server reports its authority mode in HELLO_ACK (section 6.3)")
    add("authority_required_reported", hello(), _local([1], authority="required"))
    add("binding_and_time_are_the_servers", hello(), _local([1], binding=bytes(range(32)), time=0),
        note="the client's HELLO has no say in the binding or the time (section 6.3)")
    add("time_u64_max", hello(), _local([1], time=U64_MAX))
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



# ---------------------------------------------------------------------------
# IDENTIFY (section 6.6)
# ---------------------------------------------------------------------------


def sha256(data):
    return hashlib.sha256(bytes(data)).digest()


def identify_message(hello_body_bytes, ack_body_bytes, channel, key):
    """The signed message M of section 6.6:
    "spine-wire-identify-v1" 00 || SHA-256(H) || SHA-256(A) || u8(len(channel)) || channel || key,
    H the body bytes of the HELLO frame and A those of the HELLO_ACK frame, exactly as sent."""
    if len(channel) > 255:
        raise ValueError("a channel is at most 255 bytes")
    return (IDENTIFY_DOMAIN + sha256(hello_body_bytes) + sha256(ack_body_bytes)
            + bytes([len(channel)]) + bytes(channel) + bytes(key))


def identify_body(key, sig, channel=b"", **extra):
    body = {"key": key, "sig": sig, "channel": channel}
    body.update(extra)
    return body


def make_identify_body(seed, hello_body_bytes, ack_body_bytes, channel=b""):
    """A valid IDENTIFY body signed by `seed` (Ed25519 signatures are deterministic)."""
    key = names_v1.ed25519_public(seed)
    sig = names_v1.ed25519_sign(seed, identify_message(hello_body_bytes, ack_body_bytes, channel, key))
    return identify_body(key, sig, channel)


def parse_identify_body(body):
    """The shape rules of 6.6: a closed map {key: bytes 32, sig: bytes 64, channel: bytes}.
    A CBOR failure is malformed_frame, a shape failure protocol_violation (section 7.3)."""
    try:
        value = decode(body)
    except Reject as e:
        raise CloseReject("malformed_frame", "body is not deterministic CBOR: %s" % e)
    if not isinstance(value, dict):
        raise CloseReject("protocol_violation", "IDENTIFY body is not a map with text keys")
    extra = set(value) - {"key", "sig", "channel"}
    if extra:
        raise CloseReject("protocol_violation", "unknown key %r in IDENTIFY" % sorted(extra)[0])
    key = _need(value, "key", lambda v: isinstance(v, bytes) and len(v) == 32, "32 bytes")
    sig = _need(value, "sig", lambda v: isinstance(v, bytes) and len(v) == 64, "64 bytes")
    channel = _need(value, "channel", lambda v: isinstance(v, bytes), "bytes")
    return {"key": key, "sig": sig, "channel": channel}


def verify_identify(hello_body_bytes, ack_body_bytes, body):
    """What a server does with an IDENTIFY body: the shape, an empty channel,
    strict Ed25519 verification of M. Returns the proven key."""
    fields = parse_identify_body(body)
    if fields["channel"] != b"":
        raise CloseReject("protocol_violation", "IDENTIFY channel must be empty")
    message = identify_message(hello_body_bytes, ack_body_bytes, fields["channel"], fields["key"])
    if not names_v1.ed25519_verify(fields["key"], message, fields["sig"], strict=True):
        raise CloseReject("protocol_violation", "IDENTIFY signature does not verify")
    return fields["key"]


def make_identify():
    """Vectors for the body, the signed message and the proof."""
    out = []
    hello_b = encode(hello_body([1], MAX_FRAME, []))
    ack_b = encode(hello_ack_body(1, MAX_FRAME, 8, []))
    other_ack_b = encode(hello_ack_body(1, MAX_FRAME, 8, [], binding=bytes([0x43]) * 32))
    other_hello_b = encode(hello_body([1], 65536, []))
    seed = IDENTIFY_SEED
    other_seed = bytes([0x12]) * 32
    key = names_v1.ed25519_public(seed)
    good = make_identify_body(seed, hello_b, ack_b)

    def add(name, body_value, expect, note=None, ack=None, body_bytes=None):
        h, a = hello_b, ack or ack_b
        body = body_bytes if body_bytes is not None else encode(body_value)
        v = {"name": name, "hello_body": h.hex(), "ack_body": a.hex(), "body_hex": body.hex(), "expect": expect}
        if body_bytes is None:
            v["body"] = to_desc(body_value)
        if expect == "valid":
            fields = parse_identify_body(body)
            v["seed"] = seed.hex()
            v["key"] = fields["key"].hex()
            v["message"] = identify_message(h, a, fields["channel"], fields["key"]).hex()
            v["signature"] = fields["sig"].hex()
        if note:
            v["note"] = note
        out.append(v)

    add("valid_empty_channel", good, "valid",
        note="the body is 118 bytes and the frame 133 (section 6.6); the signature is deterministic")
    add("valid_over_another_acknowledgement", make_identify_body(seed, hello_b, other_ack_b), "valid", ack=other_ack_b,
        note="the same key over an acknowledgement with another binding signs another message")
    # A proof made for another connection, or by another key.
    add("invalid_wrong_binding", make_identify_body(seed, hello_b, other_ack_b), "protocol_violation",
        note="signed over an acknowledgement with another binding: M differs (section 6.6)")
    add("invalid_wrong_hello", make_identify_body(seed, other_hello_b, ack_b), "protocol_violation",
        note="signed over another HELLO body")
    sig_flip = bytearray(good["sig"])
    sig_flip[0] ^= 1
    add("invalid_signature_bit_flip", identify_body(key, bytes(sig_flip)), "protocol_violation")
    add("invalid_other_key_with_this_signature", identify_body(names_v1.ed25519_public(other_seed), good["sig"]),
        "protocol_violation", note="the signature covers the key (section 6.6)")
    add("invalid_signature_by_another_key",
        identify_body(key, make_identify_body(other_seed, hello_b, ack_b)["sig"]), "protocol_violation")
    # The channel is empty in this revision.
    for n in (1, 32):
        channel = bytes(range(1, n + 1))
        add("invalid_channel_%d_bytes_correctly_signed" % n, make_identify_body(seed, hello_b, ack_b, channel),
            "protocol_violation",
            note="a server refuses a non-empty channel even when the signature covers it (section 6.6)")
    # Lengths and types.
    add("invalid_key_31_bytes", identify_body(key[:31], good["sig"]), "protocol_violation")
    add("invalid_key_33_bytes", identify_body(key + b"\x00", good["sig"]), "protocol_violation")
    add("invalid_sig_63_bytes", identify_body(key, good["sig"][:63]), "protocol_violation")
    add("invalid_sig_65_bytes", identify_body(key, good["sig"] + b"\x00"), "protocol_violation")
    add("invalid_key_is_text", identify_body("k" * 32, good["sig"]), "protocol_violation")
    add("invalid_channel_is_text", identify_body(key, good["sig"], ""), "protocol_violation")
    add("invalid_channel_is_32_ints", identify_body(key, good["sig"], [0] * 32), "protocol_violation",
        note="a byte string is never an array of integers (section 5 rule 7)")
    add("invalid_key_is_32_ints", identify_body(list(key), good["sig"]), "protocol_violation")
    for missing in ("key", "sig", "channel"):
        body = dict(good)
        del body[missing]
        add("invalid_missing_%s" % missing, body, "protocol_violation")
    add("invalid_unknown_key", identify_body(key, good["sig"], b"", extra=1), "protocol_violation",
        note="IDENTIFY is a closed struct (section 5 rule 8)")
    add("invalid_body_is_array", [key, good["sig"], b""], "protocol_violation")
    add("invalid_body_is_null", None, "protocol_violation")
    add("invalid_body_is_empty_map", {}, "protocol_violation")
    add("invalid_null_channel", identify_body(key, good["sig"], None), "protocol_violation")
    # Not deterministic CBOR: malformed_frame.
    unsorted = (b"\xa3" + encode("sig") + encode(good["sig"]) + encode("key") + encode(key)
                + encode("channel") + encode(b""))
    add("invalid_keys_not_sorted", None, "malformed_frame", body_bytes=unsorted,
        note="a body that is not deterministic CBOR is malformed_frame whatever its type (section 7.2)")
    add("invalid_trailing_byte", None, "malformed_frame", body_bytes=encode(good) + b"\x00")
    # Strict verification: a small-order key, a small-order R and a non-canonical S.
    identity_point = bytes([1]) + bytes(31)
    add("invalid_small_order_key", identify_body(identity_point, identity_point + bytes(32)), "protocol_violation",
        note="the identity point as key with R = identity and S = 0 verifies under plain RFC 8032 and is refused by verify_strict")
    a_scalar = names_v1._expand(seed)[0]
    message = identify_message(hello_b, ack_b, b"", key)
    h = names_v1._sha512_modq(identity_point + key + message)
    s = (h * a_scalar) % names_v1._Q
    add("invalid_small_order_r", identify_body(key, identity_point + int.to_bytes(s, 32, "little")),
        "protocol_violation",
        note="R is the identity point and S = h*a: plain verification accepts it, verify_strict refuses a small-order R")
    s_plus_l = int.from_bytes(good["sig"][32:], "little") + names_v1._Q
    add("invalid_non_canonical_s", identify_body(key, good["sig"][:32] + int.to_bytes(s_plus_l, 32, "little")),
        "protocol_violation", note="S + L: S is not below the group order")
    return out


DELETE_HASH = bytes(range(32))


def delete_message(hash_, binding):
    """The message a delete_resource proof signs (section 12.2):
    "spine-wire-delete-resource-v1" 00 || hash (32) || binding (32)."""
    if len(hash_) != 32 or len(binding) != 32:
        raise ValueError("the hash and the binding are 32 bytes each")
    return DELETE_RESOURCE_DOMAIN + bytes(hash_) + bytes(binding)


def delete_proof_valid(owner, hash_, binding, proof):
    """What the daemon does first with a delete_resource request: strict
    Ed25519 verification of the message against the `owner` the request names."""
    return names_v1.ed25519_verify(bytes(owner), delete_message(hash_, binding), bytes(proof), strict=True)


def make_delete_proofs():
    """Vectors for the proof of delete_resource and the message it signs."""
    out = []
    seed = DELETE_SEED
    owner = names_v1.ed25519_public(seed)
    other_seed = bytes([0x22]) * 32
    other_owner = names_v1.ed25519_public(other_seed)
    binding = DEFAULT_BINDING
    other_binding = bytes([0x43]) * 32
    hash_ = DELETE_HASH
    good = names_v1.ed25519_sign(seed, delete_message(hash_, binding))

    def add(name, owner_key, hash_value, binding_value, proof, expect, note, seed_value=None):
        v = {"name": name, "owner": owner_key.hex(), "hash": hash_value.hex(), "binding": binding_value.hex(),
             "proof": proof.hex(), "expect": expect, "note": note}
        if expect == "valid":
            v["seed"] = seed_value.hex()
            v["message"] = delete_message(hash_value, binding_value).hex()
        out.append(v)

    add("valid", owner, hash_, binding, good, "valid",
        "the signature by the owner over the domain, the hash and the connection's binding (section 12.2)", seed)
    add("valid_another_blob_and_connection", owner, bytes([0xaa]) * 32, other_binding,
        names_v1.ed25519_sign(seed, delete_message(bytes([0xaa]) * 32, other_binding)), "valid",
        "another hash and another binding sign another message", seed)
    add("valid_another_owner", other_owner, hash_, binding, names_v1.ed25519_sign(other_seed, delete_message(hash_, binding)),
        "valid", "the check is against the owner the request names; whether that key owns the blob is the next step", other_seed)
    add("invalid_proof_for_another_connection", owner, hash_, binding,
        names_v1.ed25519_sign(seed, delete_message(hash_, other_binding)), "invalid",
        "signed for a connection with another binding: a captured proof does not verify elsewhere (section 12.2)")
    add("invalid_proof_for_another_hash", owner, hash_, binding,
        names_v1.ed25519_sign(seed, delete_message(bytes([0xbb]) * 32, binding)), "invalid",
        "the signature covers the hash")
    add("invalid_signed_by_another_key", owner, hash_, binding,
        names_v1.ed25519_sign(other_seed, delete_message(hash_, binding)), "invalid",
        "a proof by another key for the owner the request names")
    add("invalid_owner_is_another_key", other_owner, hash_, binding, good, "invalid",
        "the proof verifies only under the key that made it")
    flipped = bytearray(good)
    flipped[0] ^= 1
    add("invalid_signature_bit_flip_in_r", owner, hash_, binding, bytes(flipped), "invalid", "one bit of R changed")
    flipped = bytearray(good)
    flipped[40] ^= 1
    add("invalid_signature_bit_flip_in_s", owner, hash_, binding, bytes(flipped), "invalid", "one bit of S changed")
    s_plus_l = int.from_bytes(good[32:], "little") + names_v1._Q
    add("invalid_non_canonical_s", owner, hash_, binding, good[:32] + int.to_bytes(s_plus_l, 32, "little"), "invalid",
        "S + L: S is not below the group order, refused by strict verification")
    identity_point = bytes([1]) + bytes(31)
    add("invalid_small_order_owner", identity_point, hash_, binding, identity_point + bytes(32), "invalid",
        "the identity point as owner with R = identity and S = 0 verifies under plain RFC 8032 and is refused by strict verification")
    add("invalid_all_zero_owner_and_proof", bytes(32), hash_, binding, bytes(64), "invalid",
        "the key of an ownerless blob is all zeros: no proof can be made for it, so such a blob cannot be deleted through the wire")
    add("invalid_signed_with_the_identify_domain", owner, hash_, binding,
        names_v1.ed25519_sign(seed, IDENTIFY_DOMAIN + hash_ + binding), "invalid",
        "the domain string is unique to this message: a signature over another domain does not verify")
    add("invalid_message_without_the_domain", owner, hash_, binding,
        names_v1.ed25519_sign(seed, hash_ + binding), "invalid", "the domain is part of the message")
    add("invalid_hash_and_binding_swapped", owner, hash_, binding,
        names_v1.ed25519_sign(seed, DELETE_RESOURCE_DOMAIN + binding + hash_), "invalid", "the layout is hash then binding")
    return out


def make_invalid_commands():
    """REQUEST bodies of the delete commands (and byte fields of others) that a
    daemon answers with invalid_request or unknown_command (sections 7.4, 12.1, 12.2)."""
    out = []
    key32, key64 = b"\x22" * 32, b"\x33" * 64

    def add(name, value, code, note=None):
        body = encode(value)
        v = {"name": name, "body": to_desc(value), "body_hex": body.hex(), "code": code}
        if note:
            v["note"] = note
        out.append(v)

    good = {"hash": b"\x11" * 32, "owner": key32, "proof": key64}

    def with_(**changes):
        fields = dict(good)
        fields.update(changes)
        return {"delete_resource": fields}

    wrong = "a byte field of the wrong length is invalid_request (section 12.2)"
    add("delete_resource_hash_31_bytes", with_(hash=b"\x11" * 31), "invalid_request", wrong)
    add("delete_resource_hash_33_bytes", with_(hash=b"\x11" * 33), "invalid_request", wrong)
    add("delete_resource_owner_31_bytes", with_(owner=b"\x22" * 31), "invalid_request", wrong)
    add("delete_resource_owner_64_bytes", with_(owner=b"\x22" * 64), "invalid_request", wrong)
    add("delete_resource_proof_63_bytes", with_(proof=b"\x33" * 63), "invalid_request", wrong)
    add("delete_resource_proof_65_bytes", with_(proof=b"\x33" * 65), "invalid_request", wrong)
    add("delete_resource_proof_32_bytes", with_(proof=key32), "invalid_request", "a 32-byte value where the proof is 64")
    add("delete_resource_proof_empty", with_(proof=b""), "invalid_request", wrong)
    add("delete_resource_hash_is_text", with_(hash="11" * 32), "invalid_request", "a byte string is not text")
    add("delete_resource_proof_is_64_ints", with_(proof=[3] * 64), "invalid_request",
        "a byte string is never an array of integers (section 5 rule 7)")
    add("delete_resource_proof_is_null", with_(proof=None), "invalid_request", "a required field is not optional")
    for missing in ("hash", "owner", "proof"):
        fields = dict(good)
        del fields[missing]
        add("delete_resource_without_%s" % missing, {"delete_resource": fields}, "invalid_request",
            "all three fields are required (section 12.2)")
    add("delete_resource_extra_field", with_(extra=1), "invalid_request", "a command's fields are a closed map (section 5 rule 8)")
    add("delete_resource_bare_name", "delete_resource", "invalid_request", "a command with fields is not a bare name (section 12.1)")
    add("delete_resource_fields_not_a_map", {"delete_resource": [b"\x11" * 32]}, "invalid_request")
    add("delete_knowledge_key_is_int", {"delete_knowledge": {"key": 1}}, "invalid_request", "the key is text")
    add("delete_knowledge_key_is_bytes", {"delete_knowledge": {"key": b"k"}}, "invalid_request", "the key is text")
    add("delete_knowledge_key_is_null", {"delete_knowledge": {"key": None}}, "invalid_request")
    add("delete_knowledge_without_key", {"delete_knowledge": {}}, "invalid_request")
    add("delete_knowledge_extra_field", {"delete_knowledge": {"key": "k", "tags": []}}, "invalid_request",
        "no tag filter: the key is matched exactly")
    add("delete_knowledge_bare_name", "delete_knowledge", "invalid_request")
    add("read_resource_hash_31_bytes", {"read_resource": {"hash": b"\x11" * 31, "offset": 0, "max_bytes": 16}},
        "invalid_request", wrong)
    add("unknown_command_delete_everything", {"delete_everything": {}}, "unknown_command",
        "a name outside the 21 commands (section 7.4)")
    add("unknown_command_delete_name", "delete_name", "unknown_command", "no such command: a name record is not deleted by these commands")
    return out


def delete_result_problem(command_name, value):
    """Why `value` is not the ok value of `command_name`'s reply (section 12.4), or None."""
    if not isinstance(value, dict) or set(value) != {"deleted"}:
        return "the ok value is the map {deleted}"
    deleted = value["deleted"]
    if command_name == "delete_resource":
        return None if isinstance(deleted, bool) else "deleted is a boolean"
    if command_name == "delete_knowledge":
        if isinstance(deleted, bool) or not isinstance(deleted, int) or deleted < 0 or deleted > U64_MAX:
            return "deleted is an unsigned integer"
        return None
    return "no such command"


def make_delete_results():
    out = []

    def add(name, command_name, value, expect, note=None):
        body = encode(value)
        v = {"name": name, "command": command_name, "body": to_desc(value), "body_hex": body.hex(), "expect": expect}
        if note:
            v["note"] = note
        out.append(v)

    r, k = "delete_resource", "delete_knowledge"
    add("resource_true", r, {"deleted": True}, "ok", "a blob was deleted")
    add("resource_false", r, {"deleted": False}, "ok",
        "no blob, or a blob that is not the proven key's: a success, so a probe learns nothing (section 12.2)")
    add("resource_one", r, {"deleted": 1}, "invalid", "a boolean, not an integer")
    add("resource_text", r, {"deleted": "true"}, "invalid")
    add("resource_null", r, {"deleted": None}, "invalid")
    add("resource_extra_key", r, {"deleted": True, "name": "n"}, "invalid", "the ok value is a closed map")
    add("resource_empty_map", r, {}, "invalid")
    add("resource_not_a_map", r, True, "invalid")
    add("resource_array", r, [True], "invalid")
    add("knowledge_zero", k, {"deleted": 0}, "ok", "nothing held the key: a success (section 12.2)")
    add("knowledge_two", k, {"deleted": 2}, "ok")
    add("knowledge_u64_max", k, {"deleted": U64_MAX}, "ok", "the count is an unsigned 64-bit integer")
    add("knowledge_true", k, {"deleted": True}, "invalid", "an integer, not a boolean")
    add("knowledge_negative", k, {"deleted": -1}, "invalid")
    add("knowledge_float", k, {"deleted": 1.5}, "invalid")
    add("knowledge_float_integral", k, {"deleted": 2.0}, "invalid", "a float is not an integer (section 5 rule 2)")
    add("knowledge_text", k, {"deleted": "2"}, "invalid")
    add("knowledge_extra_key", k, {"deleted": 2, "kept": 1}, "invalid")
    add("knowledge_empty_map", k, {}, "invalid")
    add("knowledge_not_a_map", k, 2, "invalid")
    return out


RECEIPT_EXAMPLE = {
    "body": {
        "v": 1, "recipient": b"\x09" * 32, "invocation": b"\x0a" * 32, "leaf": b"\x0b" * 32,
        "allowed": True, "effect": "irreversible", "step": None, "calls": 1, "cost": 0, "bytes": 0,
        "result": b"\x0c" * 32, "time": 1700000001,
    },
    "sig": b"\x0d" * 64,
}


def make_auth_requests():
    """AUTH_REQUEST bodies (section 8.5): 'expect' is "ok" or the answer code."""
    out = []
    good_auth = {"chain": [b"\x10" * 40, b"\x11" * 56], "invocation": b"\x12" * 90}
    nav = command("navigate", {"url": "https://example.org/"})

    def add(name, value, expect, note=None):
        body = encode(value)
        v = {"name": name, "body": to_desc(value), "body_hex": body.hex(), "expect": expect}
        if expect == "ok":
            item = encode(value["command"])
            v["command_hex"] = item.hex()
            v["op"] = command_op(item).hex()
        if note:
            v["note"] = note
        out.append(v)

    add("navigate", {"auth": good_auth, "command": nav}, "ok",
        "op is the SHA-256 of the command item, the bytes a plain REQUEST body would be (section 8.5)")
    add("unit_command", {"auth": good_auth, "command": command("get_ur")}, "ok")
    add("tool_call", {"auth": good_auth, "command": command("tool_call", {"name": "spine.content/hash", "args": {"text": "hi"}})}, "ok")
    add("empty_chain_is_valid_on_the_wire", {"auth": dict(good_auth, chain=[]), "command": nav}, "ok",
        "the wire layer reads the chain as byte strings and no more; the gate refuses an empty chain at step 1 (section 8.5)")
    add("nine_grants_is_valid_on_the_wire", {"auth": dict(good_auth, chain=[b"\x01"] * 9), "command": nav}, "ok",
        "more than eight grants is refused by the gate at step 1, not by the body decoder (section 8.5)")
    add("empty_invocation_is_valid_on_the_wire", {"auth": dict(good_auth, invocation=b""), "command": nav}, "ok",
        "an invocation that does not decode is refused by the gate at step 1")
    shape = "an AUTH_REQUEST body is exactly {auth: {chain, invocation}, command} (section 8.5)"
    add("a_bare_command", nav, "invalid_request", "a REQUEST body is not an AUTH_REQUEST body")
    add("a_bare_unit_command", "get_ur", "invalid_request", "a REQUEST body is not an AUTH_REQUEST body")
    add("an_array_of_auth_and_command", [good_auth, nav], "invalid_request", "a map, never an array (section 8.5)")
    add("without_command", {"auth": good_auth}, "invalid_request", shape)
    add("without_auth", {"command": nav}, "invalid_request", shape)
    add("extra_member", {"auth": good_auth, "command": nav, "trace": 1}, "invalid_request", "a closed map (section 5 rule 8)")
    add("auth_is_an_array", {"auth": [good_auth["chain"], good_auth["invocation"]], "command": nav}, "invalid_request",
        "auth is a map, never an array (section 8.5)")
    add("auth_without_chain", {"auth": {"invocation": b"\x12"}, "command": nav}, "invalid_request", shape)
    add("auth_without_invocation", {"auth": {"chain": [b"\x10"]}, "command": nav}, "invalid_request", shape)
    add("auth_extra_member", {"auth": dict(good_auth, mode="audit"), "command": nav}, "invalid_request", "a closed map (section 5 rule 8)")
    add("auth_is_null", {"auth": None, "command": nav}, "invalid_request")
    add("chain_is_a_byte_string", {"auth": dict(good_auth, chain=b"\x10" * 40), "command": nav}, "invalid_request",
        "an array of byte strings, not one byte string (section 8.5)")
    add("chain_item_is_text", {"auth": dict(good_auth, chain=["x"]), "command": nav}, "invalid_request")
    add("chain_item_is_an_array_of_integers", {"auth": dict(good_auth, chain=[[1, 2, 3]]), "command": nav}, "invalid_request",
        "a byte string is never an array of integers (section 5 rule 7)")
    add("chain_is_null", {"auth": dict(good_auth, chain=None), "command": nav}, "invalid_request")
    add("invocation_is_text", {"auth": dict(good_auth, invocation="x"), "command": nav}, "invalid_request")
    add("invocation_is_null", {"auth": dict(good_auth, invocation=None), "command": nav}, "invalid_request")
    add("command_with_a_missing_field", {"auth": good_auth, "command": {"navigate": {}}}, "invalid_request",
        "the command is judged as a REQUEST body is (section 8.5)")
    add("command_is_an_integer", {"auth": good_auth, "command": 7}, "invalid_request")
    add("command_delete_resource_short_hash",
        {"auth": good_auth, "command": {"delete_resource": {"hash": b"\x11" * 31, "owner": b"\x22" * 32, "proof": b"\x33" * 64}}},
        "invalid_request", "a byte field of the wrong length (section 12.2)")
    add("unknown_command", {"auth": good_auth, "command": "delete_everything"}, "unknown_command",
        "a command outside the 21 is unknown_command, as for a REQUEST (section 7.4)")
    add("an_auth_request_as_the_command", {"auth": good_auth, "command": {"auth": good_auth, "command": nav}}, "invalid_request",
        "a command is a text or a one-entry map, never an AUTH_REQUEST body")
    return out


def check_auth_request(v):
    body = bytes.fromhex(v["body_hex"])
    if encode(from_desc(v["body"])) != body:
        return "the description gives %s, not body_hex" % encode(from_desc(v["body"])).hex()[:80]
    validate(body)
    value = decode(body)
    try:
        validate_auth_request_value(value)
        answer = "ok"
    except ValueError as e:
        answer = str(e).split(":")[0]
    if answer != v["expect"]:
        return "answered %s, expected %s" % (answer, v["expect"])
    if answer == "ok":
        item = encode(value["command"])
        if item.hex() != v.get("command_hex") or command_op(item).hex() != v.get("op"):
            return "command_hex or op differs from the command item"
    return None


def make_receipts():
    """RECEIPT bodies (section 8.6): 'expect' is "ok" or "invalid"."""
    out = []

    def add(name, value_or_bytes, expect, note=None):
        if isinstance(value_or_bytes, bytes):
            body, desc = value_or_bytes, None
        else:
            body, desc = encode(value_or_bytes), to_desc(value_or_bytes)
        v = {"name": name, "body_hex": body.hex(), "length": len(body), "expect": expect}
        if desc is not None:
            v["body"] = desc
        if note:
            v["note"] = note
        out.append(v)

    sig = b"\x0d" * 64

    def padded(n):
        return {"body": {"pad": b"\x00" * n}, "sig": sig}

    add("realistic", RECEIPT_EXAMPLE, "ok", "what a node sends: the signed receipt of spine-authority (section 8.6)")
    exact = next(n for n in range(0, 600) if len(encode(padded(n))) == MAX_RECEIPT_BYTES)
    add("exactly_512_bytes", padded(exact), "ok", "512 is the limit of a RECEIPT body (section 8.6)")
    add("513_bytes", padded(exact + 1), "invalid", "one byte over the limit")
    add("minimal", {"body": {}, "sig": sig}, "ok", "the wire layer reads the shape, not what the receipt says")
    add("sig_63_bytes", {"body": {}, "sig": b"\x0d" * 63}, "invalid", "sig is 64 bytes")
    add("sig_65_bytes", {"body": {}, "sig": b"\x0d" * 65}, "invalid")
    add("sig_is_text", {"body": {}, "sig": "x" * 64}, "invalid")
    add("body_is_an_array", {"body": [], "sig": sig}, "invalid", "body is a map")
    add("body_is_bytes", {"body": b"\x00", "sig": sig}, "invalid")
    add("without_sig", {"body": {}}, "invalid")
    add("without_body", {"sig": sig}, "invalid")
    add("extra_member", {"body": {}, "sig": sig, "x": 1}, "invalid", "a closed map")
    add("an_array", [{}, sig], "invalid", "a map, never an array")
    add("empty_map", {}, "invalid")
    add("not_cbor_text", b"\xff", "invalid", "not deterministic CBOR")
    return out


def check_receipt(v):
    body = bytes.fromhex(v["body_hex"])
    if len(body) != v["length"]:
        return "length is %d, the vector says %d" % (len(body), v["length"])
    if "body" in v and encode(from_desc(v["body"])) != body:
        return "the description gives %s, not body_hex" % encode(from_desc(v["body"])).hex()[:80]
    problem = validate_receipt_bytes(body)
    verdict = "invalid" if problem else "ok"
    if verdict != v["expect"]:
        return "judged %s (%s), expected %s" % (verdict, problem, v["expect"])
    return None


def make_result_digests():
    """The `result` a receipt carries for a RESPONSE body (section 8.6)."""
    out = []
    for name, value in (
        ("ok_navigated", reply_ok({"navigated": True})),
        ("ok_empty_map", reply_ok({})),
        ("err_rate_limit", reply_err("rate_limit", "Rate limit exceeded")),
        ("err_internal_oversize_replacement", reply_err("internal", "response could not be sent")),
    ):
        body = encode(value)
        out.append({"name": name, "body": to_desc(value), "body_hex": body.hex(),
                    "digest": result_digest(body).hex(),
                    "note": "SHA-256 of \"spine-wire-result-v1\" 00 and the RESPONSE body as sent (section 8.6)"})
    return out


def check_result_digest(v):
    body = bytes.fromhex(v["body_hex"])
    if encode(from_desc(v["body"])) != body:
        return "the description gives %s, not body_hex" % encode(from_desc(v["body"])).hex()[:80]
    if result_digest(body).hex() != v["digest"]:
        return "digest is %s, the vector says %s" % (result_digest(body).hex(), v["digest"])
    return None


def make_error_codes():
    return {"count": len(ERROR_CODES), "codes": list(ERROR_CODES), "protocol": list(PROTOCOL_ERROR_CODES)}


def make_ed25519_strict():
    """Verification verdicts, strict and plain, over the same inputs."""
    out = []
    seed = IDENTIFY_SEED
    public = names_v1.ed25519_public(seed)
    msg = b"spine-wire strict verification"
    sig = names_v1.ed25519_sign(seed, msg)
    identity_point = bytes([1]) + bytes(31)
    a_scalar = names_v1._expand(seed)[0]

    def add(name, public_key, message, signature, note):
        out.append({
            "name": name,
            "public": public_key.hex(),
            "message": message.hex(),
            "signature": signature.hex(),
            "plain": names_v1.ed25519_verify(public_key, message, signature, strict=False),
            "strict": names_v1.ed25519_verify(public_key, message, signature, strict=True),
            "note": note,
        })

    add("valid", public, msg, sig, "an ordinary signature is accepted by both")
    add("wrong_message", public, msg + b"!", sig, "refused by both")
    bad = bytearray(sig)
    bad[40] ^= 1
    add("bit_flip_in_s", public, msg, bytes(bad), "refused by both")
    s_plus_l = int.from_bytes(sig[32:], "little") + names_v1._Q
    add("s_plus_group_order", public, msg, sig[:32] + int.to_bytes(s_plus_l, 32, "little"),
        "S is not below the group order: refused by both")
    add("small_order_key_identity", identity_point, msg, identity_point + bytes(32),
        "R = identity, S = 0 and the identity point as key: RFC 8032 verification accepts, verify_strict refuses a small-order key")
    h = names_v1._sha512_modq(identity_point + public + msg)
    s = (h * a_scalar) % names_v1._Q
    add("small_order_r", public, msg, identity_point + int.to_bytes(s, 32, "little"),
        "a real key with R = identity and S = h*a: RFC 8032 verification accepts, verify_strict refuses a small-order R")
    return out


def _session_hello(extensions=()):
    return {"type": "HELLO", "request_id": 0, "flags": 0,
            "body": to_desc(hello_body([1], MAX_FRAME, list(extensions)))}


def run_session(v):
    """The server side of a connection after its handshake: what the state
    rules of sections 6.6 and 8 do with the frames of `v`, in order. Returns
    {"result": "ok", "identity": hex or None} when all are accepted, or
    {"result": "close", "code": ...} at the first that is refused."""
    decoded, _ = decode_frame(bytes.fromhex(v["hello_frame"]), PRE_HELLO_MAX_FRAME)
    hello = parse_hello(decoded["type"], decoded["body"])
    ack = negotiate(hello, v["local"])
    ack_b = encode(_ack_body_of(ack))
    violation = {"result": "close", "code": "protocol_violation"}
    window, identity, in_flight = True, None, set()
    for frame_hex in v["frames"]:
        try:
            frame, _ = decode_frame(bytes.fromhex(frame_hex), ack["max_frame"])
        except FrameReject as e:
            return {"result": "close", "code": e.close_code}
        try:
            validate(frame["body"])
        except Reject:
            return {"result": "close", "code": "malformed_frame"}
        t = TYPE_NAMES[frame["type"]]
        if t in ("PING", "PONG"):
            if not _is_uint(decode(frame["body"])):
                return violation
            continue
        if t == "IDENTIFY":
            if not window:
                return violation
            window = False
            try:
                identity = verify_identify(decoded["body"], ack_b, frame["body"])
            except CloseReject as e:
                return {"result": "close", "code": e.code}
            continue
        if t in ("REQUEST", "STREAM_CANCEL", "EXT"):
            window = False
        if t == "REQUEST":
            if frame["request_id"] in in_flight:
                return violation
            if len(in_flight) < ack["max_inflight"]:
                in_flight.add(frame["request_id"])
        elif t == "EXT":
            value = decode(frame["body"])
            if not (isinstance(value, list) and len(value) == 3 and _is_uint(value[0])
                    and value[0] < len(ack["extensions"])):
                return violation
        elif t != "STREAM_CANCEL":
            return violation
    return {"result": "ok", "identity": identity.hex() if identity is not None else None}


def make_identify_sessions():
    out = []
    seed = IDENTIFY_SEED

    def add(name, frames_for, local=None, extensions=(), note=None):
        local = local or _local([1], max_inflight=4, extensions=extensions)
        hello_frame = frame_from_spec(_session_hello(extensions))
        decoded, _ = decode_frame(hello_frame, PRE_HELLO_MAX_FRAME)
        ack = negotiate(parse_hello(decoded["type"], decoded["body"]), local)
        frames = frames_for(decoded["body"], encode(_ack_body_of(ack)))
        v = {"name": name, "hello_frame": hello_frame.hex(), "local": local, "frames": [f.hex() for f in frames]}
        v["expect"] = run_session(v)
        if note:
            v["note"] = note
        out.append(v)

    def ident(h, a, channel=b""):
        return encode_frame("IDENTIFY", 0, encode(make_identify_body(seed, h, a, channel)))

    other_ack = encode(hello_ack_body(1, MAX_FRAME, 4, [], binding=bytes([0x43]) * 32))
    req = lambda i: encode_frame("REQUEST", i, encode(command("get_ur")))
    ping = encode_frame("PING", 5, encode(1))
    pong = encode_frame("PONG", 6, encode(2))
    cancel = encode_frame("STREAM_CANCEL", 99, encode({}))
    ext = encode_frame("EXT", 0, encode([0, 1, None]))

    add("no_identify_is_anonymous", lambda h, a: [req(1)], note="a client that sends no IDENTIFY is anonymous")
    add("identify_alone", lambda h, a: [ident(h, a)])
    add("identify_then_request", lambda h, a: [ident(h, a), req(1)])
    add("identify_pipelined_with_three_requests", lambda h, a: [ident(h, a), req(1), req(2), req(3)])
    add("ping_and_pong_do_not_close_the_window", lambda h, a: [ping, pong, ident(h, a), req(1)],
        note="a PING or PONG does not close the window (section 6.6)")
    add("identify_then_ping_then_request", lambda h, a: [ident(h, a), ping, req(1)])
    add("second_identify", lambda h, a: [ident(h, a), ident(h, a)], note="an IDENTIFY is accepted at most once")
    add("identify_after_request", lambda h, a: [req(1), ident(h, a)])
    add("identify_after_request_ping_and_pong", lambda h, a: [req(1), ping, pong, ident(h, a)])
    add("identify_after_stream_cancel", lambda h, a: [cancel, ident(h, a)],
        note="a STREAM_CANCEL for an id that is not in flight is ignored but closes the window")
    add("identify_after_ext", lambda h, a: [ext, ident(h, a)], extensions=["x/a/1"],
        note="an EXT frame closes the window")
    add("identify_after_a_request_refused_as_overloaded", lambda h, a: [req(1), req(2), ident(h, a)],
        local=_local([1], max_inflight=1),
        note="a request that arrives at max_inflight is answered overloaded and still closes the window")
    add("bad_signature", lambda h, a: [encode_frame("IDENTIFY", 0, encode(
        dict(make_identify_body(seed, h, a), sig=bytes(64))))])
    add("proof_for_another_binding", lambda h, a: [ident(h, other_ack)])
    add("non_empty_channel", lambda h, a: [ident(h, a, b"\x01")])
    add("identify_with_nonzero_request_id",
        lambda h, a: [raw_frame(1, 0x06, 0, 5, encode(make_identify_body(seed, h, a)))],
        note="IDENTIFY carries request id 0: a frame-level protocol_violation (section 3)")
    add("identify_body_not_deterministic", lambda h, a: [raw_frame(1, 0x06, 0, 0, b"\xa0\x00")],
        note="a body that is not deterministic CBOR is malformed_frame (section 7.2)")
    add("response_from_the_client_is_a_violation",
        lambda h, a: [raw_frame(1, 0x11, 0, 1, encode({"ok": 1}))], note="direction rule of section 8.2, as before")
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
        "hello_acks": make_hello_acks(),
        "identify": make_identify(),
        "identify_sessions": make_identify_sessions(),
        "ed25519_strict": make_ed25519_strict(),
        "name_results": make_name_results(),
        "delete_proofs": make_delete_proofs(),
        "invalid_commands": make_invalid_commands(),
        "delete_results": make_delete_results(),
        "auth_requests": make_auth_requests(),
        "receipts": make_receipts(),
        "result_digests": make_result_digests(),
        "error_codes": make_error_codes(),
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
    elif typed == "identify":
        parse_identify_body(body)
    elif typed == "close":
        parse_close(code, body)
    elif typed == "command":
        validate_request_value(decode(body))
    elif typed == "auth_request":
        validate_auth_request_value(decode(body))
    elif typed == "receipt":
        problem = validate_receipt_bytes(body)
        if problem:
            raise ValueError(problem)
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
    body = _ack_body_of(ack)
    if encode_frame("HELLO_ACK", 0, encode(body)).hex() != expect["ack_frame"]:
        return "ack_frame differs"
    parse_hello_ack(FRAME_TYPES["HELLO_ACK"], encode(body))
    return None


def check_hello_ack(v):
    body = bytes.fromhex(v["body_hex"])
    if encode(from_desc(v["body"])) != body:
        return "the description gives %s, not body_hex" % encode(from_desc(v["body"])).hex()[:80]
    try:
        parse_hello_ack(FRAME_TYPES["HELLO_ACK"], body)
    except CloseReject as e:
        return None if e.code == v["expect"] else "closed with %s, expected %s" % (e.code, v["expect"])
    return None if v["expect"] == "ok" else "accepted, expected %s" % v["expect"]


def check_identify(v):
    hello_b, ack_b, body = (bytes.fromhex(v[k]) for k in ("hello_body", "ack_body", "body_hex"))
    parse_hello(FRAME_TYPES["HELLO"], hello_b)
    parse_hello_ack(FRAME_TYPES["HELLO_ACK"], ack_b)
    if "body" in v and encode(from_desc(v["body"])) != body:
        return "the description gives %s, not body_hex" % encode(from_desc(v["body"])).hex()[:80]
    expect = v["expect"]
    try:
        key = verify_identify(hello_b, ack_b, body)
    except CloseReject as e:
        return None if e.code == expect else "closed with %s, expected %s" % (e.code, expect)
    if expect != "valid":
        return "accepted, expected %s" % expect
    fields = parse_identify_body(body)
    seed = bytes.fromhex(v["seed"])
    message = identify_message(hello_b, ack_b, fields["channel"], key)
    if key.hex() != v["key"] or names_v1.ed25519_public(seed) != key:
        return "key differs from the key of the seed"
    if message.hex() != v["message"]:
        return "message M is %s, not the stored one" % message.hex()[:80]
    if names_v1.ed25519_sign(seed, message).hex() != v["signature"] or fields["sig"].hex() != v["signature"]:
        return "signature differs from the deterministic signature of M"
    if encode(make_identify_body(seed, hello_b, ack_b, fields["channel"])) != body:
        return "body differs from the body signed by the seed"
    return None


def check_name_result(v):
    body = bytes.fromhex(v["body_hex"])
    if encode(from_desc(v["body"])) != body:
        return "the description gives %s, not body_hex" % encode(from_desc(v["body"])).hex()[:80]
    validate(body)
    problem = name_result_problem(decode(body))
    verdict = "invalid" if problem else "ok"
    if verdict != v["expect"]:
        return "judged %s (%s), expected %s" % (verdict, problem, v["expect"])
    return None


def check_delete_proof(v):
    owner, hash_, binding, proof = (bytes.fromhex(v[k]) for k in ("owner", "hash", "binding", "proof"))
    verdict = "valid" if delete_proof_valid(owner, hash_, binding, proof) else "invalid"
    if verdict != v["expect"]:
        return "judged %s, expected %s" % (verdict, v["expect"])
    if verdict == "valid":
        seed = bytes.fromhex(v["seed"])
        message = delete_message(hash_, binding)
        if message.hex() != v["message"]:
            return "message is %s, not the stored one" % message.hex()[:80]
        if not message.startswith(DELETE_RESOURCE_DOMAIN) or len(message) != 30 + 32 + 32:
            return "the message is not domain || hash || binding"
        if names_v1.ed25519_public(seed) != owner:
            return "owner differs from the key of the seed"
        if names_v1.ed25519_sign(seed, message) != proof:
            return "proof differs from the deterministic signature of the message"
    return None


def check_invalid_command(v):
    body = bytes.fromhex(v["body_hex"])
    if encode(from_desc(v["body"])) != body:
        return "the description gives %s, not body_hex" % encode(from_desc(v["body"])).hex()[:80]
    validate(body)
    try:
        validate_request_value(decode(body))
    except ValueError as e:
        answer = str(e).split(":")[0]
        return None if answer == v["code"] else "answered %s, expected %s" % (answer, v["code"])
    return "accepted, expected %s" % v["code"]


def check_delete_result(v):
    body = bytes.fromhex(v["body_hex"])
    if encode(from_desc(v["body"])) != body:
        return "the description gives %s, not body_hex" % encode(from_desc(v["body"])).hex()[:80]
    validate(body)
    problem = delete_result_problem(v["command"], decode(body))
    verdict = "invalid" if problem else "ok"
    if verdict != v["expect"]:
        return "judged %s (%s), expected %s" % (verdict, problem, v["expect"])
    return None


SPEC_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "docs", "spec", "00-wire.md")


def spec_error_codes(text):
    """The codes of the table of section 13 of the specification, in order."""
    start = text.index("## 13. Error codes")
    end = text.index("## 14.", start)
    codes = []
    for line in text[start:end].splitlines():
        line = line.strip()
        if line.startswith("| `"):
            codes.append(line[3:].split("`", 1)[0])
    return codes


def check_error_codes(doc):
    """The code list of the vector file against this script's list and the
    specification's table. A missing specification file skips that part."""
    problems = []
    section = doc.get("error_codes")
    if not isinstance(section, dict):
        return ["error_codes: missing"]
    codes = section.get("codes")
    if codes != ERROR_CODES:
        problems.append("error_codes: the list differs from the reference list")
    if section.get("count") != len(ERROR_CODES) or not isinstance(codes, list) or len(codes) != section.get("count"):
        problems.append("error_codes: count is %r, the list has %d" % (section.get("count"), len(ERROR_CODES)))
    if section.get("protocol") != PROTOCOL_ERROR_CODES:
        problems.append("error_codes: the protocol codes differ")
    if len(set(ERROR_CODES)) != len(ERROR_CODES):
        problems.append("error_codes: a code appears twice")
    for code in ERROR_CODES:
        if not code or not all(c.islower() or c.isdigit() or c == "_" for c in code):
            problems.append("error_codes: %r is not snake_case" % code)
    if os.path.exists(SPEC_PATH):
        with open(SPEC_PATH, "rb") as f:
            text = f.read().decode("utf-8").replace("\r\n", "\n")
        table = spec_error_codes(text)
        if table != ERROR_CODES:
            only_spec = [c for c in table if c not in ERROR_CODES]
            only_list = [c for c in ERROR_CODES if c not in table]
            problems.append("error_codes: the table of section 13 differs (only in the text: %s; only in the list: %s; "
                            "or the order differs)" % (only_spec, only_list))
        expected = "A `REQUEST` body is one of %d commands" % len(COMMAND_NAMES)
        if expected not in text:
            problems.append("section 12.1 does not say %r" % expected)
        for name in COMMAND_NAMES:
            if "`%s`" % name not in text:
                problems.append("the specification never mentions the command `%s`" % name)
        count_text = "%d codes" % len(ERROR_CODES)
        if count_text not in text:
            problems.append("section 13 does not say %r" % count_text)
    return problems


def check_ed25519_strict(v):
    public, message, signature = (bytes.fromhex(v[k]) for k in ("public", "message", "signature"))
    plain = names_v1.ed25519_verify(public, message, signature, strict=False)
    strict = names_v1.ed25519_verify(public, message, signature, strict=True)
    if (plain, strict) != (v["plain"], v["strict"]):
        return "verdicts plain=%s strict=%s, expected plain=%s strict=%s" % (plain, strict, v["plain"], v["strict"])
    return None


def check_identify_session(v):
    result = run_session(v)
    return None if result == v["expect"] else "result %s, expected %s" % (result, v["expect"])


CHECKS = [
    ("valid_bodies", check_valid_body, 60),
    ("invalid_bodies", check_invalid_body, 60),
    ("valid_frames", check_valid_frame, 40),
    ("invalid_frames", check_invalid_frame, 40),
    ("incomplete_frames", check_incomplete_frame, 4),
    ("handshakes", check_handshake, 30),
    ("hello_acks", check_hello_ack, 25),
    ("identify", check_identify, 25),
    ("identify_sessions", check_identify_session, 15),
    ("ed25519_strict", check_ed25519_strict, 6),
    ("name_results", check_name_result, 60),
    ("delete_proofs", check_delete_proof, 12),
    ("invalid_commands", check_invalid_command, 25),
    ("delete_results", check_delete_result, 18),
    ("auth_requests", check_auth_request, 25),
    ("receipts", check_receipt, 12),
    ("result_digests", check_result_digest, 4),
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
    failures.extend(check_error_codes(doc))
    # The two delete commands have a valid REQUEST vector.
    seen = set()
    for v in doc.get("valid_frames", []):
        if isinstance(v, dict) and v.get("type") == "REQUEST":
            try:
                seen.add(validate_request_value(from_desc(v["body"])))
            except Exception:  # noqa: BLE001 - reported by the section check
                pass
    for name in ("delete_resource", "delete_knowledge"):
        if name not in seen:
            failures.append("valid_frames: no REQUEST vector for the command %s" % name)
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
    if len(argv) >= 2 and argv[1] in ("generate", "check"):
        names_v1.rfc8032_self_check()
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
