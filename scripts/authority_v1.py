#!/usr/bin/env python3
"""spine-authority v1: an independent reference implementation and the vector generator.

Written from docs/spec/03-authority.md (the grant layout, attenuation and the
eleven verification steps, including its "Provisional decisions"), not from the
Rust code. It shares nothing with `src/spine-authority` and uses only the
Python standard library (3.8 or later). The vectors in
`docs/spec/03-authority.vectors.json` are produced by it, and
`src/spine-authority/tests/vectors.rs` checks the Rust crate against them.

Usage:

    python scripts/authority_v1.py generate > docs/spec/03-authority.vectors.json
    python scripts/authority_v1.py check docs/spec/03-authority.vectors.json

What it implements:

* Ed25519: the RFC 8032 section 6 reference algorithm (copied from
  scripts/names_v1.py, not imported), validated at start-up against the
  section 7.1 test vectors 1 to 3, plus the "strict" verification profile
  (reject S >= L, reject a small-order public key or R).
* Deterministic CBOR: a minimal encoder (copied in outline from
  scripts/wire_v1.py) and a decoder that rejects what the layout does not
  allow. A decoded value must re-encode to the identical bytes.
* Grants and invocations: the signed bytes, signatures, ids, and a decoder
  that enforces the field set, the types and the size bounds.
* Attenuation and the eleven verification steps, for cases that are decidable
  from bytes plus a stated clock, trusted roots, revoked ids, the age of the
  revocation data, nonces already used, and totals already charged.
* Signed revocation records (who may revoke), the signed feed head and the rules
  for loading a feed, signed receipts (verifying one and matching it to an
  invocation and a decision), and a reference model of the usage ledger.

`generate` is deterministic: fixed seeds, sorted keys, LF line ends, no times.
`check` re-derives every value in a vector file, runs every case through the
Python verifier, and exits non-zero on any mismatch.
"""

import copy
import functools
import hashlib
import json
import sys
import unicodedata

# ---------------------------------------------------------------------------
# Ed25519, RFC 8032 section 6 (reference algorithm, not constant time)
# ---------------------------------------------------------------------------

_P = 2 ** 255 - 19
_Q = 2 ** 252 + 27742317777372353535851937790883648493


def _inv(x):
    return pow(x, _P - 2, _P)


_D = -121665 * _inv(121666) % _P
_SQRT_M1 = pow(2, (_P - 1) // 4, _P)


def _add(P, Q):
    A = (P[1] - P[0]) * (Q[1] - Q[0]) % _P
    B = (P[1] + P[0]) * (Q[1] + Q[0]) % _P
    C = 2 * P[3] * Q[3] * _D % _P
    D = 2 * P[2] * Q[2] % _P
    E, F, G, H = B - A, D - C, D + C, B + A
    return (E * F % _P, G * H % _P, F * G % _P, E * H % _P)


_IDENTITY = (0, 1, 1, 0)


def _mul(s, P):
    Q = _IDENTITY
    while s > 0:
        if s & 1:
            Q = _add(Q, P)
        P = _add(P, P)
        s >>= 1
    return Q


def _equal(P, Q):
    if (P[0] * Q[2] - Q[0] * P[2]) % _P != 0:
        return False
    return (P[1] * Q[2] - Q[1] * P[2]) % _P == 0


def _recover_x(y, sign):
    if y >= _P:
        return None
    x2 = (y * y - 1) * _inv(_D * y * y + 1) % _P
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (_P + 3) // 8, _P)
    if (x * x - x2) % _P != 0:
        x = x * _SQRT_M1 % _P
    if (x * x - x2) % _P != 0:
        return None
    if (x & 1) != sign:
        x = _P - x
    return x


_GY = 4 * _inv(5) % _P
_GX = _recover_x(_GY, 0)
_G = (_GX, _GY, 1, _GX * _GY % _P)


def _compress(P):
    zinv = _inv(P[2])
    x = P[0] * zinv % _P
    y = P[1] * zinv % _P
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _decompress(s):
    if len(s) != 32:
        return None
    y = int.from_bytes(s, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = _recover_x(y, sign)
    if x is None:
        return None
    return (x, y, 1, x * y % _P)


def _sha512_modq(s):
    return int.from_bytes(hashlib.sha512(s).digest(), "little") % _Q


def _expand(secret):
    h = hashlib.sha512(secret).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a, h[32:]


def ed25519_public(seed):
    a, _ = _expand(seed)
    return _compress(_mul(a, _G))


def ed25519_sign(seed, msg):
    a, prefix = _expand(seed)
    A = _compress(_mul(a, _G))
    r = _sha512_modq(prefix + msg)
    Rs = _compress(_mul(r, _G))
    h = _sha512_modq(Rs + A + msg)
    s = (r + h * a) % _Q
    return Rs + int.to_bytes(s, 32, "little")


def _small_order(P):
    return _equal(_mul(8, P), _IDENTITY)


def ed25519_verify(public, msg, sig, strict=False):
    """RFC 8032 section 5.1.7 verification. With `strict`, also reject a
    small-order public key or R, which is what the Rust side's
    `verify_strict` adds."""
    if len(public) != 32 or len(sig) != 64:
        return False
    A = _decompress(public)
    R = _decompress(sig[:32])
    if A is None or R is None:
        return False
    s = int.from_bytes(sig[32:], "little")
    if s >= _Q:
        return False
    if strict and (_small_order(A) or _small_order(R)):
        return False
    h = _sha512_modq(sig[:32] + public + msg)
    return _equal(_mul(s, _G), _add(R, _mul(h, A)))


# RFC 8032 section 7.1, TEST 1 to 3: (secret, public, message, signature).
RFC8032_VECTORS = [
    (
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "",
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    ),
    (
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        "72",
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    ),
    (
        "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        "af82",
        "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    ),
]


def rfc8032_self_check():
    for n, (secret, public, msg, sig) in enumerate(RFC8032_VECTORS, 1):
        secret, public = bytes.fromhex(secret), bytes.fromhex(public)
        msg, sig = bytes.fromhex(msg), bytes.fromhex(sig)
        assert ed25519_public(secret) == public, "RFC 8032 test %d: public key" % n
        assert ed25519_sign(secret, msg) == sig, "RFC 8032 test %d: signature" % n
        assert ed25519_verify(public, msg, sig), "RFC 8032 test %d: verify" % n
        bad = bytes([msg[0] ^ 1]) + msg[1:] if msg else b"\x00"
        assert not ed25519_verify(public, bad, sig), "RFC 8032 test %d: tamper" % n
    return len(RFC8032_VECTORS)


# ---------------------------------------------------------------------------
# Cached wrappers (the algorithm above is the unmodified reference code; the
# cache only avoids recomputing identical scalar multiplications)
# ---------------------------------------------------------------------------

@functools.lru_cache(maxsize=None)
def pub_of(seed):
    return ed25519_public(seed)


@functools.lru_cache(maxsize=None)
def sign_cached(seed, msg):
    return ed25519_sign(seed, msg)


@functools.lru_cache(maxsize=None)
def verify_cached(public, msg, sig):
    return ed25519_verify(public, msg, sig, strict=True)


# ---------------------------------------------------------------------------
# Constants (docs/spec/03-authority.md, "Encoding and bounds")
# ---------------------------------------------------------------------------

GRANT_DOMAIN = b"spine-authority-grant-v1\x00"
GRANT_ID_DOMAIN = b"spine-authority-grant-id-v1\x00"
INVOCATION_DOMAIN = b"spine-authority-invocation-v1\x00"
GRANT_VERSION = 1
INVOCATION_VERSION = 1
REVOCATION_VERSION = 1
FEED_HEAD_VERSION = 1
RECEIPT_VERSION = 1
REVOCATION_DOMAIN = b"spine-authority-revocation-v1\x00"
REVOCATION_ID_DOMAIN = b"spine-authority-revocation-id-v1\x00"
FEED_HEAD_DOMAIN = b"spine-authority-feed-head-v1\x00"
FEED_DIGEST_DOMAIN = b"spine-authority-feed-digest-v1\x00"
RECEIPT_DOMAIN = b"spine-authority-receipt-v1\x00"
INVOCATION_ID_DOMAIN = b"spine-authority-invocation-id-v1\x00"
MAX_REVOCATION_BYTES = 512
MAX_FEED_HEAD_BYTES = 512
MAX_RECEIPT_BYTES = 512
MAX_FEED_REVOCATIONS = 16384
RATE_WINDOW_SECS = 60
MAX_CHAIN_DEPTH = 8
MAX_GRANT_BYTES = 4096
MAX_INVOCATION_BYTES = 4096
MAX_RESOURCES = 16
MAX_ACTIONS = 4
MAX_CAVEATS = 8
MAX_CAVEAT_NAME_LEN = 64
MAX_CAVEAT_ARG_LEN = 256
MAX_ROLE_LEN = 16
MAX_PATH_SEGMENTS = 16
MAX_SEGMENT_LEN = 128
MAX_INVOCATION_RESOURCES = 16
MAX_INVOCATION_AGE_SECS = 60
KNOWN_ROLES = ["principal", "operator", "agent", "service"]
KNOWN_CAVEATS = []
ACTIONS = ["read", "invoke", "publish", "admin"]
EFFECTS = ["read", "reversible", "irreversible"]
LIMIT_KINDS = ["calls", "cost", "bytes", "rate"]
U32_MAX = 2 ** 32 - 1
U64_MAX = 2 ** 64 - 1
AUTO = bytes(32)

DOMAINS = {
    "grant": GRANT_DOMAIN.hex(),
    "grant_id": GRANT_ID_DOMAIN.hex(),
    "invocation": INVOCATION_DOMAIN.hex(),
    "invocation_id": INVOCATION_ID_DOMAIN.hex(),
    "revocation": REVOCATION_DOMAIN.hex(),
    "revocation_id": REVOCATION_ID_DOMAIN.hex(),
    "feed_head": FEED_HEAD_DOMAIN.hex(),
    "feed_digest": FEED_DIGEST_DOMAIN.hex(),
    "receipt": RECEIPT_DOMAIN.hex(),
}

BOUNDS = {
    "max_chain_depth": MAX_CHAIN_DEPTH,
    "max_grant_bytes": MAX_GRANT_BYTES,
    "max_invocation_bytes": MAX_INVOCATION_BYTES,
    "max_resources": MAX_RESOURCES,
    "max_actions": MAX_ACTIONS,
    "max_caveats": MAX_CAVEATS,
    "max_caveat_name_len": MAX_CAVEAT_NAME_LEN,
    "max_caveat_arg_len": MAX_CAVEAT_ARG_LEN,
    "max_role_len": MAX_ROLE_LEN,
    "max_path_segments": MAX_PATH_SEGMENTS,
    "max_segment_len": MAX_SEGMENT_LEN,
    "max_invocation_resources": MAX_INVOCATION_RESOURCES,
    "max_revocation_bytes": MAX_REVOCATION_BYTES,
    "max_feed_head_bytes": MAX_FEED_HEAD_BYTES,
    "max_feed_revocations": MAX_FEED_REVOCATIONS,
    "max_receipt_bytes": MAX_RECEIPT_BYTES,
}

# denial name -> step number (spec "Verification")
DENIALS = {
    "ChainLimits": 1,
    "InvocationLimits": 1,
    "Malformed": 2,
    "UnknownVersion": 2,
    "UnknownCaveat": 2,
    "RootNotAuthoritative": 3,
    "BrokenLink": 4,
    "BadGrantSignature": 5,
    "Broadened": 6,
    "NotYetValid": 7,
    "Expired": 7,
    "Revoked": 8,
    "RevocationDataStale": 8,
    "InvocationMalformed": 9,
    "ChainMismatch": 9,
    "InvokerNotAudience": 9,
    "WrongRecipient": 9,
    "WrongSession": 9,
    "InvocationStale": 9,
    "InvocationSignature": 9,
    "NonceReplayed": 9,
    "ResourceNotCovered": 10,
    "ActionNotGranted": 10,
    "EffectExceeded": 10,
    "LimitExhausted": 11,
    "UsageUnknown": 11,
}


# ---------------------------------------------------------------------------
# Deterministic CBOR: encoder
# ---------------------------------------------------------------------------

MAX_DEPTH = 32


def head(major, n):
    """A CBOR head with the shortest argument form."""
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


def encode(value, _depth=0):
    """Deterministic CBOR for None, bool, int, str, bytes, list and dict with
    str keys. Map entries are sorted by the bytewise order of the encoded key;
    a duplicate key is an error. No floats are used by this layer."""
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
    if isinstance(value, (bytes, bytearray)):
        return head(2, len(value)) + bytes(value)
    if isinstance(value, str):
        raw = value.encode("utf-8")
        return head(3, len(raw)) + raw
    if isinstance(value, list):
        if _depth + 1 > MAX_DEPTH:
            raise ValueError("nesting deeper than %d" % MAX_DEPTH)
        return head(4, len(value)) + b"".join(encode(v, _depth + 1) for v in value)
    if isinstance(value, dict):
        if _depth + 1 > MAX_DEPTH:
            raise ValueError("nesting deeper than %d" % MAX_DEPTH)
        entries = sorted((encode(k, _depth + 1), encode(v, _depth + 1)) for k, v in value.items())
        for a, b in zip(entries, entries[1:]):
            if a[0] == b[0]:
                raise ValueError("duplicate map key")
        return head(5, len(entries)) + b"".join(k + v for k, v in entries)
    raise TypeError("cannot encode %r" % (value,))


# ---------------------------------------------------------------------------
# Deterministic CBOR: decoder. It accepts only what this layer can contain
# (no tags, no floats, no simple values but false, true and null, definite
# lengths, text keys). Non-shortest integers and unsorted keys parse but are
# caught by the caller, which requires encode(decode(x)) == x.
# ---------------------------------------------------------------------------

class Malformed(Exception):
    pass


class DecodeFail(Exception):
    """kind is 'too_large', 'malformed' or 'bounds'."""

    def __init__(self, kind, why=""):
        Exception.__init__(self, "%s: %s" % (kind, why))
        self.kind = kind
        self.why = why


def cbor_decode(data):
    state = {"pos": 0}

    def take(n):
        end = state["pos"] + n
        if n < 0 or end > len(data):
            raise Malformed("truncated")
        chunk = data[state["pos"]:end]
        state["pos"] = end
        return chunk

    def argument(info):
        if info < 24:
            return info
        if info == 24:
            return take(1)[0]
        if info == 25:
            return int.from_bytes(take(2), "big")
        if info == 26:
            return int.from_bytes(take(4), "big")
        if info == 27:
            return int.from_bytes(take(8), "big")
        raise Malformed("reserved or indefinite")

    def item(depth):
        initial = take(1)[0]
        major, info = initial >> 5, initial & 0x1F
        if major == 7:
            if info == 20:
                return False
            if info == 21:
                return True
            if info == 22:
                return None
            raise Malformed("simple value or float")
        if major == 6:
            raise Malformed("tag")
        n = argument(info)
        if major == 0:
            return n
        if major == 1:
            return -1 - n
        if major == 2:
            return bytes(take(n))
        if major == 3:
            try:
                return take(n).decode("utf-8")
            except UnicodeDecodeError:
                raise Malformed("invalid utf-8")
        if depth + 1 > MAX_DEPTH:
            raise Malformed("too deep")
        if major == 4:
            if n > len(data) - state["pos"]:
                raise Malformed("truncated")
            return [item(depth + 1) for _ in range(n)]
        if n > (len(data) - state["pos"]) // 2:
            raise Malformed("truncated")
        out = {}
        for _ in range(n):
            key = item(depth + 1)
            if type(key) is not str:
                raise Malformed("non-text key")
            if key in out:
                raise Malformed("duplicate key")
            out[key] = item(depth + 1)
        return out

    value = item(0)
    if state["pos"] != len(data):
        raise Malformed("trailing bytes")
    return value


# ---------------------------------------------------------------------------
# Resource names and patterns
# ---------------------------------------------------------------------------

def segments_ok(path):
    if len(path) > MAX_PATH_SEGMENTS:
        return False
    for seg in path:
        if len(seg.encode("utf-8")) > MAX_SEGMENT_LEN:
            return False
        if seg == "" or seg in (".", "..") or "/" in seg:
            return False
        if any(unicodedata.category(ch) == "Cc" for ch in seg):
            return False
    return True


def pattern_contains(pattern, resource):
    if pattern["owner"] != resource["owner"]:
        return False
    if pattern["prefix"]:
        return resource["path"][:len(pattern["path"])] == pattern["path"]
    return pattern["path"] == resource["path"]


def pattern_covered_by(inner, outer):
    if inner["owner"] != outer["owner"]:
        return False
    if outer["prefix"]:
        return inner["path"][:len(outer["path"])] == outer["path"]
    return (not inner["prefix"]) and inner["path"] == outer["path"]


# ---------------------------------------------------------------------------
# Grants and invocations: shape checks (what the decoder accepts), well-formedness
# ---------------------------------------------------------------------------

def is_uint(x, limit):
    return type(x) is int and 0 <= x <= limit


def is_bytes(x, n):
    return type(x) is bytes and len(x) == n


def keys_are(d, names):
    return type(d) is dict and sorted(d.keys()) == sorted(names)


GRANT_BODY_KEYS = ["v", "iss", "aud", "parent", "role", "res", "act", "effect", "delegable",
                   "nbf", "exp", "limits", "caveats", "nonce"]
INVOCATION_BODY_KEYS = ["v", "invoker", "recipient", "session", "nonce", "time", "res", "act",
                        "effect", "op", "leaf", "cost", "bytes"]


def grant_shape_ok(g):
    if not keys_are(g, ["body", "sig"]) or not is_bytes(g["sig"], 64):
        return False
    b = g["body"]
    if not keys_are(b, GRANT_BODY_KEYS):
        return False
    if not (is_uint(b["v"], U64_MAX) and is_bytes(b["iss"], 32) and is_bytes(b["aud"], 32)):
        return False
    if not (b["parent"] is None or is_bytes(b["parent"], 32)):
        return False
    if type(b["role"]) is not str:
        return False
    if type(b["res"]) is not list:
        return False
    for p in b["res"]:
        if not keys_are(p, ["owner", "path", "prefix"]):
            return False
        if not (is_bytes(p["owner"], 32) and type(p["prefix"]) is bool):
            return False
        if type(p["path"]) is not list or any(type(s) is not str for s in p["path"]):
            return False
    if type(b["act"]) is not list or any(a not in ACTIONS for a in b["act"]):
        return False
    if b["effect"] not in EFFECTS:
        return False
    if not (is_uint(b["delegable"], U32_MAX) and is_uint(b["nbf"], U64_MAX) and is_uint(b["exp"], U64_MAX)):
        return False
    if not keys_are(b["limits"], LIMIT_KINDS):
        return False
    for k in LIMIT_KINDS:
        if not (b["limits"][k] is None or is_uint(b["limits"][k], U64_MAX)):
            return False
    if type(b["caveats"]) is not list:
        return False
    for c in b["caveats"]:
        if not keys_are(c, ["name", "arg"]) or type(c["name"]) is not str or type(c["arg"]) is not bytes:
            return False
    return is_bytes(b["nonce"], 16)


def grant_bounds_ok(b):
    if len(b["res"]) > MAX_RESOURCES or len(b["act"]) > MAX_ACTIONS or len(b["caveats"]) > MAX_CAVEATS:
        return False
    if len(b["role"].encode("utf-8")) > MAX_ROLE_LEN:
        return False
    for c in b["caveats"]:
        if len(c["name"].encode("utf-8")) > MAX_CAVEAT_NAME_LEN or len(c["arg"]) > MAX_CAVEAT_ARG_LEN:
            return False
    for p in b["res"]:
        if len(p["path"]) > MAX_PATH_SEGMENTS:
            return False
        if any(len(s.encode("utf-8")) > MAX_SEGMENT_LEN for s in p["path"]):
            return False
    return True


def decode_grant(data):
    if len(data) > MAX_GRANT_BYTES:
        raise DecodeFail("too_large")
    try:
        g = cbor_decode(data)
    except Malformed as e:
        raise DecodeFail("malformed", str(e))
    if not grant_shape_ok(g):
        raise DecodeFail("malformed", "shape")
    if encode(g) != data:
        raise DecodeFail("malformed", "not the canonical encoding")
    if not grant_bounds_ok(g["body"]):
        raise DecodeFail("bounds")
    return g


def invocation_shape_ok(i):
    if not keys_are(i, ["body", "sig"]) or not is_bytes(i["sig"], 64):
        return False
    b = i["body"]
    if not keys_are(b, INVOCATION_BODY_KEYS):
        return False
    if not (is_uint(b["v"], U64_MAX) and is_bytes(b["invoker"], 32) and is_bytes(b["recipient"], 32)
            and is_bytes(b["session"], 32) and is_bytes(b["nonce"], 16) and is_uint(b["time"], U64_MAX)
            and is_bytes(b["op"], 32) and is_bytes(b["leaf"], 32) and is_uint(b["cost"], U64_MAX)
            and is_uint(b["bytes"], U64_MAX)):
        return False
    if b["act"] not in ACTIONS or b["effect"] not in EFFECTS or type(b["res"]) is not list:
        return False
    for r in b["res"]:
        if not keys_are(r, ["owner", "path"]) or not is_bytes(r["owner"], 32):
            return False
        if type(r["path"]) is not list or any(type(s) is not str for s in r["path"]):
            return False
    return True


def decode_invocation(data):
    if len(data) > MAX_INVOCATION_BYTES:
        raise DecodeFail("too_large")
    try:
        i = cbor_decode(data)
    except Malformed as e:
        raise DecodeFail("malformed", str(e))
    if not invocation_shape_ok(i):
        raise DecodeFail("malformed", "shape")
    if encode(i) != data:
        raise DecodeFail("malformed", "not the canonical encoding")
    if len(i["body"]["res"]) > MAX_INVOCATION_RESOURCES:
        raise DecodeFail("bounds")
    return i


def grant_id(encoded):
    return hashlib.sha256(GRANT_ID_DOMAIN + encoded).digest()


# ---------------------------------------------------------------------------
# Attenuation (spec "Attenuation"; the order is the order of this function)
# ---------------------------------------------------------------------------

def attenuation_error(child, parent):
    """None if `child` (a body) is no broader than `parent`, else the name of
    the first thing it broadens."""
    if child["iss"] != parent["aud"]:
        return "Audience"
    for p in child["res"]:
        if not any(pattern_covered_by(p, outer) for outer in parent["res"]):
            return "Resource"
    for a in child["act"]:
        if a not in parent["act"]:
            return "Action"
    if EFFECTS.index(child["effect"]) > EFFECTS.index(parent["effect"]):
        return "Effect"
    if child["delegable"] >= parent["delegable"]:
        return "Delegable"
    if child["nbf"] < parent["nbf"]:
        return "NotBefore"
    if child["exp"] > parent["exp"]:
        return "Expiry"
    for k in LIMIT_KINDS:
        outer, inner = parent["limits"][k], child["limits"][k]
        if outer is not None and inner is None:
            return "LimitRemoved:" + k
        if outer is not None and inner is not None and inner > outer:
            return "LimitRaised:" + k
    for c in parent["caveats"]:
        if c not in child["caveats"]:
            return "CaveatDropped"
    return None


# ---------------------------------------------------------------------------
# The eleven steps, as a reference implementation over encoded input
# ---------------------------------------------------------------------------

class Denial(Exception):
    def __init__(self, name):
        Exception.__init__(self, name)
        self.name = name
        self.step = DENIALS[name]


def deny(name):
    raise Denial(name)


def check_well_formed(b):
    if b["v"] != GRANT_VERSION:
        deny("UnknownVersion")
    if b["role"] not in KNOWN_ROLES:
        deny("Malformed")
    if not b["res"]:
        deny("Malformed")
    for p in b["res"]:
        if not segments_ok(p["path"]):
            deny("Malformed")
    if not b["act"]:
        deny("Malformed")
    idx = [ACTIONS.index(a) for a in b["act"]]
    if any(x >= y for x, y in zip(idx, idx[1:])):
        deny("Malformed")
    if b["nbf"] >= b["exp"]:
        deny("Malformed")
    for c in b["caveats"]:
        if c["name"] not in KNOWN_CAVEATS:
            deny("UnknownCaveat")


def over(spent, add, limit):
    return limit is not None and spent + add > limit


def verify_case(c):
    """Runs one vector case. Returns None if the invocation is accepted,
    else the name of the denial. Policy: no clock skew, invocations at most
    60 seconds old, revocation data must be current, an empty ledger except
    for the stated nonces and totals."""
    try:
        verify_inner(c)
    except Denial as d:
        return d.name
    return None


def verify_inner(c):
    chain_bytes = [bytes.fromhex(h) for h in c["chain"]]
    inv_bytes = bytes.fromhex(c["invocation"])
    now = c["now"]
    if not chain_bytes or len(chain_bytes) > MAX_CHAIN_DEPTH:
        deny("ChainLimits")
    grants = []
    for raw in chain_bytes:
        try:
            grants.append(decode_grant(raw))
        except DecodeFail as e:
            deny("ChainLimits" if e.kind in ("too_large", "bounds") else "Malformed")
    try:
        inv = decode_invocation(inv_bytes)
    except DecodeFail as e:
        deny("InvocationLimits" if e.kind in ("too_large", "bounds") else "InvocationMalformed")
    ib = inv["body"]
    bodies = [g["body"] for g in grants]

    # Step 2
    for b in bodies:
        check_well_formed(b)
    ids = [grant_id(raw) for raw in chain_bytes]

    # Step 3
    root = bodies[0]
    trusted = [bytes.fromhex(h) for h in c["trusted_roots"]]
    if root["iss"] not in trusted:
        deny("RootNotAuthoritative")
    if any(r["owner"] != root["iss"] for r in ib["res"]):
        deny("RootNotAuthoritative")

    # Step 4
    if root["parent"] is not None:
        deny("BrokenLink")
    for i in range(1, len(bodies)):
        if bodies[i]["iss"] != bodies[i - 1]["aud"] or bodies[i]["parent"] != ids[i - 1]:
            deny("BrokenLink")

    # Step 5
    for g in grants:
        if not verify_cached(g["body"]["iss"], GRANT_DOMAIN + encode(g["body"]), g["sig"]):
            deny("BadGrantSignature")

    # Step 6
    for i in range(1, len(bodies)):
        if attenuation_error(bodies[i], bodies[i - 1]) is not None:
            deny("Broadened")

    # Step 7
    for b in bodies:
        if now < b["nbf"]:
            deny("NotYetValid")
        if now >= b["exp"]:
            deny("Expired")

    # Step 8
    revoked = set(bytes.fromhex(h) for h in c["revoked"])
    if any(i in revoked for i in ids):
        deny("Revoked")
    as_of = c["revocation_as_of"]
    bounds = c.get("revocation_bounds") or {"read": 0, "reversible": 0, "irreversible": 0}
    if as_of is None or max(now - as_of, 0) > bounds[ib["effect"]]:
        deny("RevocationDataStale")

    # Step 9
    if ib["v"] != INVOCATION_VERSION or not ib["res"] or any(not segments_ok(r["path"]) for r in ib["res"]):
        deny("InvocationMalformed")
    if ib["leaf"] != ids[-1]:
        deny("ChainMismatch")
    if ib["invoker"] != bodies[-1]["aud"]:
        deny("InvokerNotAudience")
    if ib["recipient"] != bytes.fromhex(c["recipient"]):
        deny("WrongRecipient")
    if ib["session"] != bytes.fromhex(c["session"]):
        deny("WrongSession")
    if ib["time"] > now or max(now - ib["time"], 0) > MAX_INVOCATION_AGE_SECS:
        deny("InvocationStale")
    if not verify_cached(ib["invoker"], INVOCATION_DOMAIN + encode(ib), inv["sig"]):
        deny("InvocationSignature")
    for s in c["seen_nonces"]:
        if bytes.fromhex(s["invoker"]) == ib["invoker"] and bytes.fromhex(s["nonce"]) == ib["nonce"]:
            deny("NonceReplayed")

    # Step 10
    leaf = bodies[-1]
    for r in ib["res"]:
        if not any(pattern_contains(p, r) for p in leaf["res"]):
            deny("ResourceNotCovered")
    if ib["act"] not in leaf["act"]:
        deny("ActionNotGranted")
    if EFFECTS.index(ib["effect"]) > EFFECTS.index(leaf["effect"]):
        deny("EffectExceeded")

    # Step 11
    totals = {}
    for p in c["preload"]:
        totals[bytes.fromhex(p["id"])] = (p["calls"], p["cost"], p["bytes"])
    for b, gid in zip(bodies, ids):
        lim = b["limits"]
        if all(lim[k] is None for k in LIMIT_KINDS):
            continue
        calls, cost, nbytes = totals.get(gid, (0, 0, 0))
        if (over(calls, 1, lim["calls"]) or over(cost, ib["cost"], lim["cost"])
                or over(nbytes, ib["bytes"], lim["bytes"]) or over(0, 1, lim["rate"])):
            deny("LimitExhausted")


# ---------------------------------------------------------------------------
# Revocations, feed heads and receipts (docs/spec/03-authority.md,
# "Revocation records", "Feed head", "Receipts"): shapes, decoding, checks
# ---------------------------------------------------------------------------

REVOCATION_BODY_KEYS = ["v", "revoker", "grant", "revoked_at", "nonce"]
HEAD_BODY_KEYS = ["v", "publisher", "seq", "as_of", "count", "digest"]
RECEIPT_BODY_KEYS = ["v", "recipient", "invocation", "leaf", "allowed", "effect", "step", "calls",
                     "cost", "bytes", "result", "time"]


def decode_signed(data, limit, shape_ok):
    """Decodes an object of the form {body, sig}: within the size limit, of the
    stated shape, and exactly the canonical encoding of what it decodes to."""
    if len(data) > limit:
        raise DecodeFail("too_large")
    try:
        value = cbor_decode(data)
    except Malformed as e:
        raise DecodeFail("malformed", str(e))
    if not shape_ok(value):
        raise DecodeFail("malformed", "shape")
    if encode(value) != data:
        raise DecodeFail("malformed", "not the canonical encoding")
    return value


def revocation_shape_ok(r):
    if not keys_are(r, ["body", "sig"]) or not is_bytes(r["sig"], 64):
        return False
    b = r["body"]
    return (keys_are(b, REVOCATION_BODY_KEYS) and is_uint(b["v"], U64_MAX) and is_bytes(b["revoker"], 32)
            and is_bytes(b["grant"], 32) and is_uint(b["revoked_at"], U64_MAX) and is_bytes(b["nonce"], 16))


def head_shape_ok(h):
    if not keys_are(h, ["body", "sig"]) or not is_bytes(h["sig"], 64):
        return False
    b = h["body"]
    return (keys_are(b, HEAD_BODY_KEYS) and is_uint(b["v"], U64_MAX) and is_bytes(b["publisher"], 32)
            and is_uint(b["seq"], U64_MAX) and is_uint(b["as_of"], U64_MAX)
            and is_uint(b["count"], U64_MAX) and is_bytes(b["digest"], 32))


def receipt_shape_ok(r):
    if not keys_are(r, ["body", "sig"]) or not is_bytes(r["sig"], 64):
        return False
    b = r["body"]
    if not keys_are(b, RECEIPT_BODY_KEYS):
        return False
    return (is_uint(b["v"], U64_MAX) and is_bytes(b["recipient"], 32) and is_bytes(b["invocation"], 32)
            and is_bytes(b["leaf"], 32) and type(b["allowed"]) is bool
            and (b["effect"] is None or b["effect"] in EFFECTS)
            and (b["step"] is None or is_uint(b["step"], U64_MAX))
            and is_uint(b["calls"], U64_MAX) and is_uint(b["cost"], U64_MAX)
            and is_uint(b["bytes"], U64_MAX) and is_bytes(b["result"], 32) and is_uint(b["time"], U64_MAX))


def decode_revocation(data):
    return decode_signed(data, MAX_REVOCATION_BYTES, revocation_shape_ok)


def decode_head(data):
    return decode_signed(data, MAX_FEED_HEAD_BYTES, head_shape_ok)


def decode_receipt(data):
    return decode_signed(data, MAX_RECEIPT_BYTES, receipt_shape_ok)


def revocation_id(encoded):
    return hashlib.sha256(REVOCATION_ID_DOMAIN + encoded).digest()


def invocation_id(encoded):
    return hashlib.sha256(INVOCATION_ID_DOMAIN + encoded).digest()


def feed_digest(ids):
    return hashlib.sha256(FEED_DIGEST_DOMAIN + b"".join(sorted(ids))).digest()


def check_revocation(data):
    """'ok', 'DecodeError', 'UnknownVersion' or 'BadRevocationSignature'."""
    try:
        r = decode_revocation(data)
    except DecodeFail:
        return "DecodeError"
    b = r["body"]
    if b["v"] != REVOCATION_VERSION:
        return "UnknownVersion"
    if not verify_cached(b["revoker"], REVOCATION_DOMAIN + encode(b), r["sig"]):
        return "BadRevocationSignature"
    return "ok"


def check_head(data, publisher):
    """'ok', 'DecodeError', 'UnknownVersion', 'WrongPublisher' or 'BadHeadSignature'."""
    try:
        h = decode_head(data)
    except DecodeFail:
        return "DecodeError"
    b = h["body"]
    if b["v"] != FEED_HEAD_VERSION:
        return "UnknownVersion"
    if b["publisher"] != publisher:
        return "WrongPublisher"
    if not verify_cached(b["publisher"], FEED_HEAD_DOMAIN + encode(b), h["sig"]):
        return "BadHeadSignature"
    return "ok"


def revocation_authorized(revocation, chain_bytes):
    """True when the revoked grant is in the chain and the revoker issued that
    grant or a grant above it."""
    ids = [grant_id(raw) for raw in chain_bytes]
    issuers = [decode_grant(raw)["body"]["iss"] for raw in chain_bytes]
    b = revocation["body"]
    if b["grant"] not in ids:
        return False
    index = ids.index(b["grant"])
    return b["revoker"] in issuers[:index + 1]


def feed_load(feed, now):
    """Loads a feed. `feed` has hex strings 'revocations', 'head', 'publisher'
    and 'last_seen_sequence' (an integer or None). Returns ('ok', head body,
    list of decoded revocations) or (error name, None, None)."""
    try:
        head = decode_head(bytes.fromhex(feed["head"]))
        revs = [decode_revocation(bytes.fromhex(h)) for h in feed["revocations"]]
    except DecodeFail:
        return "DecodeError", None, None
    hb = head["body"]
    if hb["v"] != FEED_HEAD_VERSION:
        return "UnknownVersion", None, None
    if hb["publisher"] != bytes.fromhex(feed["publisher"]):
        return "WrongPublisher", None, None
    if not verify_cached(hb["publisher"], FEED_HEAD_DOMAIN + encode(hb), head["sig"]):
        return "BadHeadSignature", None, None
    last = feed["last_seen_sequence"]
    if last is not None and hb["seq"] <= last:
        return "SequenceRollback", None, None
    if hb["as_of"] > now:
        return "FutureHead", None, None
    if len(revs) > MAX_FEED_REVOCATIONS:
        return "TooMany", None, None
    ids = [revocation_id(encode(r)) for r in revs]
    if len(set(ids)) != len(ids):
        return "DuplicateRevocation", None, None
    if hb["count"] != len(ids):
        return "CountMismatch", None, None
    if hb["digest"] != feed_digest(ids):
        return "DigestMismatch", None, None
    for r in revs:
        rb = r["body"]
        if rb["v"] != REVOCATION_VERSION:
            return "UnknownVersion", None, None
        if not verify_cached(rb["revoker"], REVOCATION_DOMAIN + encode(rb), r["sig"]):
            return "BadRevocationSignature", None, None
        if rb["revoked_at"] > hb["as_of"]:
            return "RevokedAfterHead", None, None
    return "ok", hb, revs


def feed_step8(case):
    """What step 8 learns from the case's feed: (revoked grant ids that an
    authorized revocation names, the feed's as_of or None, feed error or None).
    A feed that fails to load is no data."""
    status, head, revs = feed_load(case["feed"], case["now"])
    if status != "ok":
        return [], None, status
    chain_bytes = [bytes.fromhex(h) for h in case["chain"]]
    ids = [grant_id(raw) for raw in chain_bytes]
    issuers = [decode_grant(raw)["body"]["iss"] for raw in chain_bytes]
    revoked = []
    for index, gid in enumerate(ids):
        for r in revs:
            rb = r["body"]
            if rb["grant"] == gid and rb["revoker"] in issuers[:index + 1]:
                revoked.append(gid.hex())
                break
    return revoked, head["as_of"], None


def verify_feed_case(case):
    revoked, as_of, _ = feed_step8(case)
    c = dict(case)
    c["revoked"] = revoked
    c["revocation_as_of"] = as_of
    return verify_case(c)


def receipt_well_formed(b):
    if b["v"] != RECEIPT_VERSION:
        return False
    if b["allowed"]:
        return b["effect"] is not None and b["step"] is None and b["calls"] == 1
    return (b["effect"] is None and b["step"] is not None and 1 <= b["step"] <= 11
            and b["calls"] == 0 and b["cost"] == 0 and b["bytes"] == 0 and b["result"] == bytes(32))


def check_receipt(data, expected_recipient):
    """'ok', 'DecodeError', 'Malformed', 'WrongRecipient' or 'BadSignature'."""
    try:
        r = decode_receipt(data)
    except DecodeFail:
        return "DecodeError"
    b = r["body"]
    if not receipt_well_formed(b):
        return "Malformed"
    if b["recipient"] != expected_recipient:
        return "WrongRecipient"
    if not verify_cached(b["recipient"], RECEIPT_DOMAIN + encode(b), r["sig"]):
        return "BadSignature"
    return "ok"


def receipt_body_for(inv_bytes, verdict, result, time):
    """The receipt body a verdict on the invocation produces, or the string
    'DecisionMismatch'. `verdict` is {ok: true, leaf, invoker, nonce, effect,
    cost, bytes} or {ok: false, step}."""
    inv = decode_invocation(inv_bytes)
    ib = inv["body"]
    body = {"v": RECEIPT_VERSION, "recipient": ib["recipient"], "invocation": invocation_id(inv_bytes),
            "leaf": ib["leaf"], "allowed": False, "effect": None, "step": None, "calls": 0, "cost": 0,
            "bytes": 0, "result": bytes(32), "time": time}
    if verdict["ok"]:
        if not (bytes.fromhex(verdict["leaf"]) == ib["leaf"] and bytes.fromhex(verdict["invoker"]) == ib["invoker"]
                and bytes.fromhex(verdict["nonce"]) == ib["nonce"]):
            return "DecisionMismatch"
        body.update({"allowed": True, "effect": verdict["effect"], "calls": 1, "cost": verdict["cost"],
                     "bytes": verdict["bytes"], "result": result})
    else:
        body["step"] = verdict["step"]
    return body


def match_receipt(data, inv_bytes, verdict):
    """'ok', 'InvocationMismatch', 'LeafMismatch', 'WrongRecipient', 'DecisionMismatch'
    or 'OutcomeMismatch'."""
    b = decode_receipt(data)["body"]
    ib = decode_invocation(inv_bytes)["body"]
    if b["invocation"] != invocation_id(inv_bytes):
        return "InvocationMismatch"
    if b["leaf"] != ib["leaf"]:
        return "LeafMismatch"
    if b["recipient"] != ib["recipient"]:
        return "WrongRecipient"
    expected = receipt_body_for(inv_bytes, verdict, b["result"], b["time"])
    if expected == "DecisionMismatch":
        return expected
    return "ok" if b == expected else "OutcomeMismatch"


# ---------------------------------------------------------------------------
# The in-memory usage ledger (spec "Usage ledger"): a reference model
# ---------------------------------------------------------------------------

class RefLedger:
    """One charge is atomic: every check is made before anything changes."""

    def __init__(self, max_nonces, max_grants):
        self.max_nonces = max_nonces
        self.max_grants = max_grants
        self.nonces = {}  # (invoker, nonce) -> horizon
        self.usage = {}   # grant id -> {calls, cost, bytes, expires, window {second: count}}

    @staticmethod
    def _recent(u, now):
        return sum(n for second, n in u["window"].items() if second + RATE_WINDOW_SECS > now)

    def spent(self, gid, now):
        u = self.usage.get(gid)
        if u is None:
            return {"calls": 0, "cost": 0, "bytes": 0, "recent_calls": 0}
        return {"calls": u["calls"], "cost": u["cost"], "bytes": u["bytes"],
                "recent_calls": self._recent(u, now)}

    def charge(self, d):
        ids = [bytes.fromhex(h) for h in d["chain_ids"]]
        limits, expiries = d["chain_limits"], d["chain_expiries"]
        if len(limits) != len(ids) or len(expiries) != len(ids) or len(set(ids)) != len(ids):
            return "MalformedDecision"
        now = d["now"]
        for key in [k for k, horizon in self.nonces.items() if horizon < now]:
            del self.nonces[key]
        key = (bytes.fromhex(d["invoker"]), bytes.fromhex(d["nonce"]))
        if key in self.nonces:
            return "NonceReplayed"
        new_records = 0
        for gid, lim in zip(ids, limits):
            if all(lim[k] is None for k in LIMIT_KINDS):
                continue
            if gid in self.usage:
                spent = self.spent(gid, now)
            else:
                new_records += 1
                spent = {"calls": 0, "cost": 0, "bytes": 0, "recent_calls": 0}
            if (over(spent["calls"], 1, lim["calls"]) or over(spent["cost"], d["cost"], lim["cost"])
                    or over(spent["bytes"], d["bytes"], lim["bytes"])
                    or over(spent["recent_calls"], 1, lim["rate"])):
                return "LimitExhausted"
        if len(self.nonces) >= self.max_nonces:
            return "LedgerFull"
        if len(self.usage) + new_records > self.max_grants:
            self.usage = {g: u for g, u in self.usage.items() if u["expires"] > now}
            if len(self.usage) + new_records > self.max_grants:
                return "LedgerFull"
        self.nonces[key] = d["nonce_horizon"]
        for gid, lim, exp in zip(ids, limits, expiries):
            if all(lim[k] is None for k in LIMIT_KINDS):
                continue
            u = self.usage.setdefault(gid, {"calls": 0, "cost": 0, "bytes": 0, "expires": exp, "window": {}})
            u["expires"] = max(u["expires"], exp)
            u["calls"] += 1
            u["cost"] += d["cost"]
            u["bytes"] += d["bytes"]
            u["window"] = {s: n for s, n in u["window"].items() if s + RATE_WINDOW_SECS > now}
            u["window"][now] = u["window"].get(now, 0) + 1
        return "ok"


def run_ledger_case(case):
    """Runs the steps of a ledger case; returns (results, queries) in the
    shape stored in the case."""
    ledger = RefLedger(case["config"]["max_nonces"], case["config"]["max_grants"])
    results = [ledger.charge(step["decision"]) for step in case["steps"]]
    q = case["queries"]
    out = {
        "nonces": [{"invoker": n["invoker"], "nonce": n["nonce"],
                    "seen": (bytes.fromhex(n["invoker"]), bytes.fromhex(n["nonce"])) in ledger.nonces}
                   for n in q["nonces"]],
        "spent": [dict(s, **ledger.spent(bytes.fromhex(s["id"]), s["now"])) for s in q["spent"]],
        "nonce_count": len(ledger.nonces),
        "grant_count": len(ledger.usage),
    }
    return results, out


# ---------------------------------------------------------------------------
# Building vectors
# ---------------------------------------------------------------------------

SEEDS = {"owner": 1, "alice": 2, "bob": 3, "carol": 4, "dave": 5, "verifier": 9, "mallory": 20, "publisher": 30}
NOW = 5000
SESSION = bytes([0x55]) * 32


def seed_of(label):
    return bytes([SEEDS[label]]) * 32


def pub(label):
    return pub_of(seed_of(label))


def pat(owner, path, prefix):
    return {"owner": owner, "path": list(path), "prefix": prefix}


def rname(owner, path):
    return {"owner": owner, "path": list(path)}


def lim(calls=None, cost=None, nbytes=None, rate=None):
    return {"calls": calls, "cost": cost, "bytes": nbytes, "rate": rate}


def gbody(iss, aud, role, res, act, effect, delegable, window, limits, nonce_byte,
          parent=AUTO, caveats=None, v=GRANT_VERSION):
    return {
        "v": v, "iss": pub(iss), "aud": pub(aud), "parent": parent, "role": role,
        "res": res, "act": act, "effect": effect, "delegable": delegable,
        "nbf": window[0], "exp": window[1], "limits": limits,
        "caveats": caveats or [], "nonce": bytes([nonce_byte]) * 16,
    }


def sign_grant(signer, body):
    signed = GRANT_DOMAIN + encode(body)
    sig = sign_cached(seed_of(signer), signed)
    encoded = encode({"body": body, "sig": sig})
    return {"signer": signer, "body": body, "signed": signed, "sig": sig,
            "encoded": encoded, "id": grant_id(encoded)}


def build_chain(bodies, signers):
    out = []
    for i, (body, signer) in enumerate(zip(bodies, signers)):
        body = copy.deepcopy(body)
        if body["parent"] == AUTO:
            body["parent"] = None if i == 0 else out[i - 1]["id"]
        out.append(sign_grant(signer, body))
    return out


def ibody(invoker, recipient, nonce_byte, time, res, act, effect, leaf, cost, nbytes,
          session=SESSION, v=INVOCATION_VERSION):
    return {
        "v": v, "invoker": pub(invoker), "recipient": pub(recipient), "session": session,
        "nonce": bytes([nonce_byte]) * 16, "time": time, "res": res, "act": act,
        "effect": effect, "op": bytes([0xAB]) * 32, "leaf": leaf, "cost": cost, "bytes": nbytes,
    }


def sign_invocation(signer, body):
    signed = INVOCATION_DOMAIN + encode(body)
    sig = sign_cached(seed_of(signer), signed)
    return {"signer": signer, "body": body, "signed": signed, "sig": sig,
            "encoded": encode({"body": body, "sig": sig})}


def standard_bodies():
    owner, alice, bob, carol = "owner", "alice", "bob", "carol"
    return [
        gbody(owner, alice, "principal", [pat(pub(owner), ["data"], True)],
              ["read", "invoke", "publish"], "irreversible", 3, (1000, 10000),
              lim(100, 1000, 1000000, 10), 1),
        gbody(alice, bob, "operator", [pat(pub(owner), ["data", "reports"], True)],
              ["read", "invoke"], "reversible", 2, (1000, 9000),
              lim(50, 500, 500000, 5), 2),
        gbody(bob, carol, "agent", [pat(pub(owner), ["data", "reports", "q3"], False)],
              ["invoke"], "reversible", 0, (2000, 8000),
              lim(10, 100, 10000, 2), 3),
    ]


STANDARD_SIGNERS = ["owner", "alice", "bob"]
STANDARD_RES = [rname(pub("owner"), ["data", "reports", "q3"])]


def standard_invocation(leaf, **over_):
    args = {"invoker": "carol", "recipient": "verifier", "nonce_byte": 7, "time": NOW,
            "res": copy.deepcopy(STANDARD_RES), "act": "invoke", "effect": "reversible",
            "leaf": leaf, "cost": 1, "nbytes": 10}
    args.update(over_)
    return ibody(**args)


def ok():
    return {"ok": True}


def refused(name):
    return {"ok": False, "step": DENIALS[name], "denial": name}


def make_case(label, note, chain_bytes, inv_bytes, expect, now=NOW, recipient="verifier",
              session=SESSION, trusted=("owner",), revoked=(), as_of="now", seen=(), preload=()):
    return {
        "label": label,
        "note": note,
        "chain": [b.hex() for b in chain_bytes],
        "invocation": inv_bytes.hex(),
        "now": now,
        "recipient": pub(recipient).hex(),
        "session": session.hex(),
        "trusted_roots": [pub(t).hex() for t in trusted],
        "revoked": [r.hex() for r in revoked],
        "revocation_as_of": now if as_of == "now" else as_of,
        "seen_nonces": [{"invoker": pub(i).hex(), "nonce": (bytes([n]) * 16).hex()} for i, n in seen],
        "preload": [{"id": i.hex(), "calls": c, "cost": k, "bytes": b} for i, c, k, b in preload],
        "expect": expect,
    }


def chain_case(label, note, expect, bodies=None, signers=None, inv_signer="carol",
               inv=None, now=NOW, **ctx):
    """A case built from signed grants. `inv` overrides fields of the standard
    invocation (a callable gets the chain and returns an invocation body)."""
    chain = build_chain(bodies if bodies is not None else standard_bodies(),
                        signers if signers is not None else STANDARD_SIGNERS)
    leaf = chain[-1]["id"]
    if callable(inv):
        body = inv(chain)
    else:
        over_ = dict(inv or {})
        over_.setdefault("time", now)
        cited = over_.pop("leaf", leaf)
        body = standard_invocation(cited, **over_)
    record = sign_invocation(inv_signer, body)
    return make_case(label, note, [g["encoded"] for g in chain], record["encoded"], expect,
                     now=now, **ctx), chain, record


def std_with(index, **changes):
    bodies = standard_bodies()
    bodies[index].update(changes)
    return bodies


def std_limits(index, **changes):
    bodies = standard_bodies()
    bodies[index]["limits"].update(changes)
    return bodies


def mangled_case(label, note, expect, mangle, index=2):
    """The standard chain with the encoding of grant `index` replaced by
    `mangle(encoded_bytes)`. The invocation cites the unmangled last grant."""
    case, chain, _ = chain_case(label, note, expect)
    raw = [g["encoded"] for g in chain]
    raw[index] = mangle(raw[index])
    case["chain"] = [b.hex() for b in raw]
    return case


def replace_once(data, old, new):
    assert data.count(old) == 1, "pattern must occur exactly once"
    return data.replace(old, new)


def make_cases():
    cases = []
    add = cases.append

    # ---- accepted ----
    c, chain, inv = chain_case("three-link-chain", "owner -> alice -> bob -> carol; carol invokes on /data/reports/q3", ok())
    add(c)
    ids = [g["id"] for g in chain]

    single = [gbody("owner", "alice", "operator", [pat(pub("owner"), [], True)], ["read", "invoke"],
                    "reversible", 0, (1000, 9000), lim(), 1)]
    c, _, _ = chain_case(
        "single-link-chain", "the owner grants alice everything under its root, with no limits", ok(),
        bodies=single, signers=["owner"], inv_signer="alice",
        inv={"invoker": "alice", "res": [rname(pub("owner"), ["any", "thing"])], "act": "read", "effect": "read"})
    add(c)
    c, _, _ = chain_case("valid-at-the-start-of-the-last-window", "now equals the last grant's nbf", ok(), now=2000)
    add(c)
    c, _, _ = chain_case("valid-one-second-before-expiry", "now is one before the last grant's exp", ok(), now=7999)
    add(c)
    c, _, _ = chain_case("one-short-of-every-limit", "totals charged to the root are one short of each ceiling", ok(),
                         preload=[(ids[0], 99, 999, 999990)])
    add(c)

    # ---- step 1 ----
    long_bodies, long_signers = [], []
    holders = ["owner", "alice", "bob", "carol", "dave", "alice", "bob", "carol", "dave", "alice"]
    for i in range(9):
        long_bodies.append(gbody(holders[i], holders[i + 1], "agent", [pat(pub("owner"), ["data"], True)],
                                 ["invoke"], "reversible", 20 - i, (1000, 10000), lim(), i + 1))
        long_signers.append(holders[i])
    c, _, _ = chain_case("chain-of-nine", "nine grants where eight is the limit", refused("ChainLimits"),
                         bodies=long_bodies, signers=long_signers, inv_signer="alice",
                         inv={"invoker": "alice"})
    add(c)
    c, chain, inv = chain_case("empty-chain", "no grants at all", refused("ChainLimits"))
    c["chain"] = []
    add(c)
    many = [rname(pub("owner"), ["data", "reports", "r%d" % i]) for i in range(17)]
    c, _, _ = chain_case("invocation-names-seventeen-resources", "one more resource than the limit",
                         refused("InvocationLimits"), inv={"res": many})
    add(c)
    b = standard_bodies()
    b[2]["res"] = [pat(pub("owner"), ["data", "reports", "q%d" % i], False) for i in range(17)]
    c, _, _ = chain_case("grant-with-seventeen-patterns", "one more pattern than the limit", refused("ChainLimits"), bodies=b)
    add(c)
    b = standard_bodies()
    b[2]["res"] = [pat(pub("owner"), ["data", "s" * 129], False)]
    c, _, _ = chain_case("segment-of-129-bytes", "a path segment one byte over the limit", refused("ChainLimits"), bodies=b)
    add(c)
    b = standard_bodies()
    b[2]["res"] = [pat(pub("owner"), ["data"] + ["q" * 100] * 15, False) for _ in range(4)]
    c, _, _ = chain_case("grant-over-4096-bytes", "four deep patterns make a grant larger than the limit",
                         refused("ChainLimits"), bodies=b)
    add(c)
    b = standard_bodies()
    b[2]["caveats"] = [{"name": "c", "arg": bytes(257)}]
    c, _, _ = chain_case("caveat-argument-of-257-bytes", "one byte over the limit", refused("ChainLimits"), bodies=b)
    add(c)

    # ---- step 2 ----
    c, _, _ = chain_case("unknown-version", "grant version 2, validly signed", refused("UnknownVersion"),
                         bodies=std_with(1, v=2))
    add(c)
    b = standard_bodies()
    b[2]["caveats"] = [{"name": "needs-human", "arg": b"\x01"}]
    c, _, _ = chain_case("unrecognised-caveat", "the registry of caveats is empty", refused("UnknownCaveat"), bodies=b)
    add(c)
    for label, note, changes in [
        ("empty-action-set", "act is empty", {"act": []}),
        ("actions-not-ascending", "invoke before read", {"act": ["invoke", "read"]}),
        ("duplicate-actions", "invoke twice", {"act": ["invoke", "invoke"]}),
        ("empty-validity-window", "nbf equals exp", {"nbf": 8000}),
        ("unknown-role", "role is not a known label", {"role": "root"}),
        ("no-resources", "res is empty", {"res": []}),
        ("dotted-segment", "a path segment is ..", {"res": [pat(pub("owner"), ["..", "x"], False)]}),
        ("empty-segment", "a path segment is empty", {"res": [pat(pub("owner"), [""], False)]}),
        ("control-character-in-segment", "a segment holds a newline", {"res": [pat(pub("owner"), ["a\nb"], False)]}),
        ("slash-in-segment", "a segment holds a slash", {"res": [pat(pub("owner"), ["a/b"], False)]}),
    ]:
        c, _, _ = chain_case(label, note, refused("Malformed"), bodies=std_with(2, **changes))
        add(c)

    add(mangled_case("non-shortest-integer", "the version number written in two bytes", refused("Malformed"),
                     lambda e: replace_once(e, b"\x61v\x01", b"\x61v\x18\x01")))
    add(mangled_case("indefinite-length-map", "the outer map has an indefinite length", refused("Malformed"),
                     lambda e: b"\xbf" + e[1:] + b"\xff"))
    add(mangled_case("trailing-byte", "one byte after the grant", refused("Malformed"), lambda e: e + b"\x00"))

    def swap_entries(e):
        assert e[0] == 0xA2
        return b"\xa2" + e[71:] + e[1:71]
    add(mangled_case("top-level-keys-out-of-order", "body before sig", refused("Malformed"), swap_entries))

    def unknown_top(e):
        return b"\xa3" + e[1:71] + b"\x63zzz\x00" + e[71:]
    add(mangled_case("unknown-top-level-field", "an extra entry zzz", refused("Malformed"), unknown_top))

    case, chain, _ = chain_case("unknown-limits-field", "x", refused("Malformed"))
    body = copy.deepcopy(chain[2]["body"])
    body["limits"]["x"] = 0
    add(mangled_case("unknown-limits-field", "limits has an extra entry x", refused("Malformed"),
                     lambda e: encode({"body": body, "sig": chain[2]["sig"]})))

    body = copy.deepcopy(chain[2]["body"])
    body["limits"] = {}
    add(mangled_case("null-limits-omitted", "limits is an empty map: deterministic CBOR, not the canonical form",
                     refused("Malformed"), lambda e: encode({"body": body, "sig": chain[2]["sig"]})))

    body = copy.deepcopy(chain[2]["body"])
    body["nonce"] = [3] * 16
    add(mangled_case("nonce-as-array-of-integers", "sixteen integers where a byte string belongs",
                     refused("Malformed"), lambda e: encode({"body": body, "sig": chain[2]["sig"]})))

    body = copy.deepcopy(chain[2]["body"])
    body["nonce"] = bytes([3]) * 15
    add(mangled_case("nonce-of-15-bytes", "a byte string of the wrong length", refused("Malformed"),
                     lambda e: encode({"body": body, "sig": chain[2]["sig"]})))

    body = copy.deepcopy(chain[2]["body"])
    body["v"] = "1"
    add(mangled_case("version-as-text", "a text string where the number belongs", refused("Malformed"),
                     lambda e: encode({"body": body, "sig": chain[2]["sig"]})))

    body = copy.deepcopy(chain[2]["body"])
    body["act"] = ["invoke", "delete"]
    add(mangled_case("unknown-action", "an action the format does not have", refused("Malformed"),
                     lambda e: encode({"body": body, "sig": chain[2]["sig"]})))

    body = copy.deepcopy(chain[2]["body"])
    body["parent"] = chain[1]["id"].hex()
    add(mangled_case("parent-as-text", "the parent id as a hex string", refused("Malformed"),
                     lambda e: encode({"body": body, "sig": chain[2]["sig"]})))

    # ---- step 3 ----
    c, _, _ = chain_case("root-not-trusted", "the verifier trusts another key as root", refused("RootNotAuthoritative"),
                         trusted=("mallory",))
    add(c)
    c, _, _ = chain_case("no-trusted-roots", "the verifier trusts no root", refused("RootNotAuthoritative"), trusted=())
    add(c)
    c, _, _ = chain_case("resource-not-owned-by-the-root", "the operation touches alice's name",
                         refused("RootNotAuthoritative"),
                         inv={"res": [rname(pub("alice"), ["data", "reports", "q3"])]})
    add(c)
    c, _, _ = chain_case("one-resource-owned-one-not", "every resource must be owned",
                         refused("RootNotAuthoritative"),
                         inv={"res": [rname(pub("owner"), ["data", "reports", "q3"]),
                                      rname(pub("alice"), ["x"])]})
    add(c)

    # ---- step 4 ----
    c, _, _ = chain_case("parent-is-not-the-previous-grant", "grant 2 names an unrelated parent id",
                         refused("BrokenLink"), bodies=std_with(1, parent=bytes([9]) * 32))
    add(c)
    c, _, _ = chain_case("issuer-is-not-the-previous-audience", "mallory signs the third grant as issuer",
                         refused("BrokenLink"), bodies=std_with(2, iss=pub("mallory")),
                         signers=["owner", "alice", "mallory"])
    add(c)
    c, _, _ = chain_case("root-names-a-parent", "the first grant is not a root", refused("BrokenLink"),
                         bodies=std_with(0, parent=bytes([5]) * 32))
    add(c)
    c, chain, _ = chain_case("grants-out-of-order", "grants 2 and 3 swapped", refused("BrokenLink"))
    c["chain"] = [chain[0]["encoded"].hex(), chain[2]["encoded"].hex(), chain[1]["encoded"].hex()]
    add(c)

    # ---- step 5 ----
    def damaged(chain_in):
        out = copy.deepcopy(chain_in)
        sig = bytearray(out[1]["sig"])
        sig[10] ^= 1
        out[1]["sig"] = bytes(sig)
        out[1]["encoded"] = encode({"body": out[1]["body"], "sig": out[1]["sig"]})
        out[1]["id"] = grant_id(out[1]["encoded"])
        return out

    case, chain, _ = chain_case("damaged-signature", "one bit of grant 2's signature flipped", refused("BadGrantSignature"))
    d = damaged(chain)
    b3 = copy.deepcopy(chain[2]["body"])
    b3["parent"] = d[1]["id"]
    g3 = sign_grant("bob", b3)
    record = sign_invocation("carol", standard_invocation(g3["id"]))
    add(make_case("damaged-signature", "one bit of grant 2's signature flipped, grant 3 re-linked to it",
                  [chain[0]["encoded"], d[1]["encoded"], g3["encoded"]], record["encoded"],
                  refused("BadGrantSignature")))
    c, _, _ = chain_case("signature-by-another-key", "grant 2 is signed by mallory", refused("BadGrantSignature"),
                         signers=["owner", "mallory", "bob"])
    add(c)
    case, chain, _ = chain_case("body-changed-after-signing", "x", refused("BadGrantSignature"))
    body = copy.deepcopy(chain[2]["body"])
    body["nonce"] = bytes([0xEE]) * 16
    tampered = encode({"body": body, "sig": chain[2]["sig"]})
    record = sign_invocation("carol", standard_invocation(grant_id(tampered)))
    add(make_case("body-changed-after-signing", "the last grant's nonce changed, the signature kept",
                  [chain[0]["encoded"], chain[1]["encoded"], tampered], record["encoded"],
                  refused("BadGrantSignature")))

    # ---- step 6 ----
    for label, note, changes in [
        ("wider-resource", "grant 3 asks for all of /data", {"res": [pat(pub("owner"), ["data"], True)]}),
        ("resource-of-another-owner", "grant 3 names alice's resource",
         {"res": [pat(pub("alice"), ["data", "reports", "q3"], False)]}),
        ("sibling-by-raw-string-prefix", "reportsX is not under reports",
         {"res": [pat(pub("owner"), ["data", "reportsX", "q3"], False)]}),
        ("extra-action", "grant 3 adds publish", {"act": ["read", "invoke", "publish"]}),
        ("higher-effect-ceiling", "grant 3 raises the effect class", {"effect": "irreversible"}),
        ("delegable-not-lower", "delegable equals the parent's", {"delegable": 2}),
        ("starts-earlier", "nbf before the parent's", {"nbf": 500}),
        ("ends-later", "exp after the parent's", {"exp": 9500}),
    ]:
        c, _, _ = chain_case(label, note, refused("Broadened"), bodies=std_with(2, **changes))
        add(c)
    # An exact pattern in the parent is not widened by a prefix in the child.
    b = standard_bodies()
    b[1]["res"] = [pat(pub("owner"), ["data", "reports", "q3"], False)]
    b[2]["res"] = [pat(pub("owner"), ["data", "reports", "q3"], True)]
    c, _, _ = chain_case("exact-pattern-widened-to-prefix", "the parent holds an exact name, the child a prefix of it",
                         refused("Broadened"), bodies=b)
    add(c)
    for kind, value in [("calls", 60), ("cost", 501), ("bytes", 500001), ("rate", 6)]:
        c, _, _ = chain_case("limit-%s-raised" % kind, "grant 3's %s ceiling is above its parent's" % kind,
                             refused("Broadened"), bodies=std_limits(2, **{kind: value}))
        add(c)
    for kind in LIMIT_KINDS:
        c, _, _ = chain_case("limit-%s-removed" % kind, "the parent sets %s and the child does not" % kind,
                             refused("Broadened"), bodies=std_limits(2, **{kind: None}))
        add(c)

    # ---- step 7 ----
    c, _, _ = chain_case("not-yet-valid", "now is before the last grant's nbf", refused("NotYetValid"), now=1500)
    add(c)
    c, _, _ = chain_case("expired-at-the-boundary", "now equals the last grant's exp", refused("Expired"), now=8000)
    add(c)
    c, _, _ = chain_case("expired-long-ago", "now is after the middle grant's exp", refused("Expired"), now=9500)
    add(c)

    # ---- step 8 ----
    for i, name in enumerate(["root", "middle", "leaf"]):
        c, _, _ = chain_case("revoked-%s-grant" % name, "grant %d is on the revocation list" % (i + 1),
                             refused("Revoked"), revoked=[ids[i]])
        add(c)
    c, _, _ = chain_case("revocation-data-missing", "the verifier has no revocation data",
                         refused("RevocationDataStale"), as_of=None)
    add(c)
    c, _, _ = chain_case("revocation-data-one-second-old", "the bound for every class is 0",
                         refused("RevocationDataStale"), as_of=NOW - 1)
    add(c)

    # ---- step 9 ----
    c, _, _ = chain_case("invocation-for-another-recipient", "the verifier is mallory, the invocation names the verifier",
                         refused("WrongRecipient"), recipient="mallory")
    add(c)
    c, _, _ = chain_case("invocation-for-another-session", "the verifier's session differs",
                         refused("WrongSession"), session=bytes([0x66]) * 32)
    add(c)
    c, _, _ = chain_case("invoker-is-not-the-last-audience", "bob invokes with carol's grant as leaf",
                         refused("InvokerNotAudience"), inv_signer="bob", inv={"invoker": "bob"})
    add(c)
    c, _, _ = chain_case("invocation-cites-another-chain", "leaf is not the last grant's id",
                         refused("ChainMismatch"), inv={"leaf": bytes([3]) * 32})
    add(c)
    c, _, _ = chain_case("invocation-signed-by-another-key", "mallory signs carol's invocation",
                         refused("InvocationSignature"), inv_signer="mallory")
    add(c)
    c, _, _ = chain_case("invocation-sixty-one-seconds-old", "one more than the maximum age",
                         refused("InvocationStale"), inv={"time": NOW - 61})
    add(c)
    c, _, _ = chain_case("invocation-sixty-seconds-old", "exactly the maximum age", ok(), inv={"time": NOW - 60})
    add(c)
    c, _, _ = chain_case("invocation-from-the-future", "one second ahead with no skew allowed",
                         refused("InvocationStale"), inv={"time": NOW + 1})
    add(c)
    c, _, _ = chain_case("invocation-version-2", "an invocation of an unknown version",
                         refused("InvocationMalformed"), inv={"v": 2})
    add(c)
    c, _, _ = chain_case("invocation-names-no-resource", "res is empty", refused("InvocationMalformed"),
                         inv={"res": []})
    add(c)
    c, chain, inv = chain_case("invocation-with-trailing-byte", "one byte after the invocation",
                               refused("InvocationMalformed"))
    c["invocation"] = (inv["encoded"] + b"\x00").hex()
    add(c)
    c, _, _ = chain_case("nonce-already-used", "carol's nonce is in the ledger", refused("NonceReplayed"),
                         seen=[("carol", 7)])
    add(c)
    c, _, _ = chain_case("another-invokers-nonce-does-not-count", "the same nonce value from bob", ok(),
                         seen=[("bob", 7)])
    add(c)

    # ---- step 10 ----
    for label, note, res in [
        ("sibling-resource", "q4 is not q3", [rname(pub("owner"), ["data", "reports", "q4"])]),
        ("child-of-an-exact-pattern", "an exact pattern does not cover what is below it",
         [rname(pub("owner"), ["data", "reports", "q3", "detail"])]),
        ("parent-of-the-pattern", "the pattern does not cover what is above it",
         [rname(pub("owner"), ["data", "reports"])]),
        ("covered-and-uncovered", "every resource must be covered",
         [rname(pub("owner"), ["data", "reports", "q3"]), rname(pub("owner"), ["data", "reports", "q4"])]),
    ]:
        c, _, _ = chain_case(label, note, refused("ResourceNotCovered"), inv={"res": res})
        add(c)
    for action in ["read", "publish", "admin"]:
        c, _, _ = chain_case("action-%s-not-granted" % action, "the last grant allows only invoke",
                             refused("ActionNotGranted"), inv={"act": action})
        add(c)
    c, _, _ = chain_case("effect-above-the-last-ceiling", "irreversible against a reversible ceiling",
                         refused("EffectExceeded"), inv={"effect": "irreversible"})
    add(c)

    # ---- step 11 ----
    c, _, _ = chain_case("calls-exhausted-at-the-root", "100 of 100 calls already charged to grant 1",
                         refused("LimitExhausted"), preload=[(ids[0], 100, 0, 0)])
    add(c)
    c, _, _ = chain_case("cost-exhausted-at-the-leaf", "100 of 100 cost units charged to grant 3",
                         refused("LimitExhausted"), preload=[(ids[2], 0, 100, 0)])
    add(c)
    c, _, _ = chain_case("bytes-exhausted-in-the-middle", "500000 of 500000 bytes charged to grant 2",
                         refused("LimitExhausted"), preload=[(ids[1], 0, 0, 500000)])
    add(c)
    return cases


def make_keys():
    return [{"label": label, "seed": seed_of(label).hex(), "public": pub(label).hex()}
            for label in sorted(SEEDS)]


def body_to_json(body):
    out = dict(body)
    out["iss"] = body["iss"].hex()
    out["aud"] = body["aud"].hex()
    out["parent"] = None if body["parent"] is None else body["parent"].hex()
    out["nonce"] = body["nonce"].hex()
    out["res"] = [dict(p, owner=p["owner"].hex()) for p in body["res"]]
    out["caveats"] = [dict(c, arg=c["arg"].hex()) for c in body["caveats"]]
    return out


def body_from_json(j):
    out = dict(j)
    out["iss"] = bytes.fromhex(j["iss"])
    out["aud"] = bytes.fromhex(j["aud"])
    out["parent"] = None if j["parent"] is None else bytes.fromhex(j["parent"])
    out["nonce"] = bytes.fromhex(j["nonce"])
    out["res"] = [dict(p, owner=bytes.fromhex(p["owner"])) for p in j["res"]]
    out["caveats"] = [dict(c, arg=bytes.fromhex(c["arg"])) for c in j["caveats"]]
    return out


def inv_body_to_json(body):
    out = dict(body)
    for k in ("invoker", "recipient", "session", "nonce", "op", "leaf"):
        out[k] = body[k].hex()
    out["res"] = [dict(r, owner=r["owner"].hex()) for r in body["res"]]
    return out


def inv_body_from_json(j):
    out = dict(j)
    for k in ("invoker", "recipient", "session", "nonce", "op", "leaf"):
        out[k] = bytes.fromhex(j[k])
    out["res"] = [dict(r, owner=bytes.fromhex(r["owner"])) for r in j["res"]]
    return out


def make_grants():
    """The standard three-link chain, a root-only grant, and one with every
    optional field in use, each with its signed bytes, signature and id."""
    out = []
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    for label, g in zip(["standard-1-root", "standard-2-operator", "standard-3-agent"], chain):
        out.append((label, g))
    bare = build_chain([gbody("owner", "alice", "service", [pat(pub("owner"), [], False)], ["admin"],
                              "read", 0, (0, 1), lim(), 0)], ["owner"])[0]
    out.append(("minimal-root-no-limits", bare))
    many = build_chain([gbody("owner", "bob", "principal",
                              [pat(pub("owner"), ["a", "b"], True), pat(pub("owner"), ["c"], False)],
                              ["read", "invoke", "publish", "admin"], "irreversible", 4294967295,
                              (0, 18446744073709551615), lim(0, 18446744073709551615, 1, 255), 255)],
                       ["owner"])[0]
    out.append(("every-action-extreme-numbers", many))
    return [{
        "label": label,
        "signer": g["signer"],
        "body": body_to_json(g["body"]),
        "signed_bytes": g["signed"].hex(),
        "signature": g["sig"].hex(),
        "encoded": g["encoded"].hex(),
        "id": g["id"].hex(),
    } for label, g in out]


def make_invocations():
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    rec = sign_invocation("carol", standard_invocation(chain[-1]["id"]))
    rec2 = sign_invocation("alice", ibody("alice", "verifier", 1, 0, [rname(pub("owner"), [])], "admin", "read",
                                          bytes(32), 0, 0))
    return [{
        "label": label,
        "signer": r["signer"],
        "body": inv_body_to_json(r["body"]),
        "signed_bytes": r["signed"].hex(),
        "signature": r["sig"].hex(),
        "encoded": r["encoded"].hex(),
        "id": invocation_id(r["encoded"]).hex(),
    } for label, r in [("standard", rec), ("minimal", rec2)]]


# ---------------------------------------------------------------------------
# Building the revocation, feed, receipt and ledger vectors
# ---------------------------------------------------------------------------

def sign_object(domain, signer, body):
    signed = domain + encode(body)
    sig = sign_cached(seed_of(signer), signed)
    return {"signer": signer, "body": body, "signed": signed, "sig": sig,
            "encoded": encode({"body": body, "sig": sig})}


def make_revocation(signer, gid, revoked_at, nonce_byte, v=REVOCATION_VERSION, revoker=None):
    body = {"v": v, "revoker": pub(revoker or signer), "grant": gid, "revoked_at": revoked_at,
            "nonce": bytes([nonce_byte]) * 16}
    record = sign_object(REVOCATION_DOMAIN, signer, body)
    record["id"] = revocation_id(record["encoded"])
    return record


def make_head(signer, seq, as_of, revs, v=FEED_HEAD_VERSION, count=None, digest=None):
    ids = [r["id"] for r in revs]
    body = {"v": v, "publisher": pub(signer), "seq": seq, "as_of": as_of,
            "count": len(revs) if count is None else count,
            "digest": feed_digest(ids) if digest is None else digest}
    return sign_object(FEED_HEAD_DOMAIN, signer, body)


def make_receipt(signer, inv_record, verdict, v=RECEIPT_VERSION, result=bytes([0xCD]) * 32, time=NOW,
                 **changes):
    body = receipt_body_for(inv_record["encoded"], verdict, result, time)
    assert body != "DecisionMismatch"
    body["v"] = v
    body.update(changes)
    return sign_object(RECEIPT_DOMAIN, signer, body)


def body_header_index(encoded):
    key = b"\x64body"
    assert encoded.count(key) == 1
    return encoded.index(key) + len(key)


def with_extra_field(encoded):
    at = body_header_index(encoded)
    header = encoded[at]
    assert 0xA1 <= header <= 0xB6 and encoded[at + 1:at + 4] == b"\x61v\x01"
    return (encoded[:at] + bytes([header + 1]) + encoded[at + 1:at + 4] + b"\x62zz\x00"
            + encoded[at + 4:])


def with_long_form_version(encoded):
    at = body_header_index(encoded)
    assert encoded[at + 1:at + 4] == b"\x61v\x01"
    return encoded[:at + 3] + b"\x18\x01" + encoded[at + 4:]


def with_array_for_bytes(encoded, value):
    """The 32-byte string `value` written as an array of 32 integers instead."""
    marker = b"\x58\x20" + value
    assert encoded.count(marker) == 1
    at = encoded.index(marker)
    return encoded[:at] + b"\x98\x20" + b"".join(encode(x) for x in value) + encoded[at + 34:]


def mutations(encoded, array_value=None):
    out = [
        ("trailing-byte", encoded + b"\x00"),
        ("truncated", encoded[:-1]),
        ("unknown-field", with_extra_field(encoded)),
        ("long-form-integer", with_long_form_version(encoded)),
        ("empty", b""),
    ]
    if array_value is not None:
        out.append(("array-instead-of-bytes", with_array_for_bytes(encoded, array_value)))
    return out


def make_revocations_section():
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    ids = [g["id"] for g in chain]
    records = [
        ("alice-revokes-her-grant", make_revocation("alice", ids[1], 4000, 1)),
        ("owner-revokes-the-root-grant", make_revocation("owner", ids[0], 4500, 2)),
        ("bob-revokes-the-last-grant", make_revocation("bob", ids[2], 4999, 3)),
        ("revocation-at-time-zero", make_revocation("owner", bytes([0x42]) * 32, 0, 0)),
        ("revocation-extreme-time", make_revocation("mallory", bytes([0xFF]) * 32, U64_MAX, 255)),
    ]
    out = []
    for label, r in records:
        out.append({
            "label": label,
            "signer": r["signer"],
            "body": {"v": r["body"]["v"], "revoker": r["body"]["revoker"].hex(),
                     "grant": r["body"]["grant"].hex(), "revoked_at": r["body"]["revoked_at"],
                     "nonce": r["body"]["nonce"].hex()},
            "signed_bytes": r["signed"].hex(),
            "signature": r["sig"].hex(),
            "encoded": r["encoded"].hex(),
            "id": r["id"].hex(),
        })
    return out


def make_heads_section():
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    revs = [make_revocation("alice", chain[1]["id"], 4000, 1), make_revocation("owner", chain[0]["id"], 4500, 2)]
    heads = [
        ("empty-feed", make_head("publisher", 1, NOW, [])),
        ("two-revocations", make_head("publisher", 7, NOW, revs)),
        ("two-revocations-reversed-order", make_head("publisher", 8, NOW, list(reversed(revs)))),
        ("extreme-numbers", make_head("publisher", U64_MAX, U64_MAX, [])),
    ]
    out = []
    for label, h in heads:
        b = h["body"]
        out.append({
            "label": label,
            "signer": h["signer"],
            "body": {"v": b["v"], "publisher": b["publisher"].hex(), "seq": b["seq"], "as_of": b["as_of"],
                     "count": b["count"], "digest": b["digest"].hex()},
            "signed_bytes": h["signed"].hex(),
            "signature": h["sig"].hex(),
            "encoded": h["encoded"].hex(),
        })
    return out


def make_revocation_checks():
    """Valid and invalid encodings of revocations, each with its outcome."""
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    good = make_revocation("alice", chain[1]["id"], 4000, 1)
    cases = [("valid", good["encoded"], "ok")]
    for label, raw in mutations(good["encoded"], array_value=chain[1]["id"]):
        cases.append((label, raw, "DecodeError"))
    cases.append(("oversize", bytes(MAX_REVOCATION_BYTES + 1), "DecodeError"))
    bad_sig = bytearray(good["encoded"])
    bad_sig[bad_sig.index(b"\x63sig\x58\x40") + 6] ^= 1
    cases.append(("bad-signature", bytes(bad_sig), "BadRevocationSignature"))
    forged = make_revocation("mallory", chain[1]["id"], 4000, 1, revoker="alice")
    cases.append(("signed-by-someone-else", forged["encoded"], "BadRevocationSignature"))
    for field, change in [("grant", lambda b: b.update(grant=chain[2]["id"])),
                          ("revoked-at", lambda b: b.update(revoked_at=4001)),
                          ("nonce", lambda b: b.update(nonce=bytes([9]) * 16)),
                          ("revoker", lambda b: b.update(revoker=pub("owner")))]:
        body = dict(good["body"])
        change(body)
        cases.append(("tampered-" + field, encode({"body": body, "sig": good["sig"]}), "BadRevocationSignature"))
    cases.append(("unknown-version", make_revocation("alice", chain[1]["id"], 4000, 1, v=2)["encoded"],
                  "UnknownVersion"))
    return [{"label": label, "encoded": raw.hex(), "expect": expect} for label, raw, expect in cases]


def make_authority_cases():
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    ids = [g["id"] for g in chain]
    raw = [g["encoded"].hex() for g in chain]
    names = ["owner", "alice", "bob", "carol", "mallory"]
    out = []
    for revoker in names:
        for target, gid in enumerate(ids):
            r = make_revocation(revoker, gid, 4000, 1)
            out.append({"label": "%s-revokes-grant-%d" % (revoker, target + 1), "chain": raw,
                        "revocation": r["encoded"].hex(),
                        "expect_authorized": revocation_authorized(decode_revocation(r["encoded"]), chain_bytes_of(raw))})
    unknown = make_revocation("owner", bytes([0x77]) * 32, 4000, 1)
    out.append({"label": "owner-revokes-a-grant-outside-the-chain", "chain": raw,
                "revocation": unknown["encoded"].hex(), "expect_authorized": False})
    return out


def chain_bytes_of(raw_hex):
    return [bytes.fromhex(h) for h in raw_hex]


def feed_json(revs, head, publisher="publisher", last_seen=None):
    return {"publisher": pub(publisher).hex(), "last_seen_sequence": last_seen,
            "revocations": [r["encoded"].hex() for r in revs], "head": head["encoded"].hex()}


def make_feed_cases():
    """Whether a set and a head load, and why not."""
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    ids = [g["id"] for g in chain]
    a = make_revocation("alice", ids[1], 4000, 1)
    b = make_revocation("owner", ids[0], 4500, 2)
    c = make_revocation("bob", ids[2], 4600, 3)
    pubr = "publisher"
    cases = []

    def add(label, note, feed, expect, now=NOW):
        cases.append({"label": label, "note": note, "now": now, "feed": feed, "expect": expect})

    head = make_head(pubr, 3, NOW, [a, b])
    add("complete-set", "the set the head describes", feed_json([a, b], head), "ok")
    add("complete-set-other-order", "the same set in another order", feed_json([b, a], head), "ok")
    add("empty-feed", "no revocations, a head that says so", feed_json([], make_head(pubr, 1, NOW, [])), "ok")
    add("first-contact", "no last-seen sequence: nothing to roll back", feed_json([a, b], head, last_seen=None), "ok")
    add("sequence-advances", "last seen 2, head 3", feed_json([a, b], head, last_seen=2), "ok")
    add("sequence-equal", "last seen 3, head 3", feed_json([a, b], head, last_seen=3), "SequenceRollback")
    add("sequence-lower", "last seen 9, head 3", feed_json([a, b], head, last_seen=9), "SequenceRollback")
    add("truncated-set", "one record missing", feed_json([a], head), "CountMismatch")
    add("padded-set", "one record more than the head lists", feed_json([a, b, c], head), "CountMismatch")
    add("swapped-record", "same count, one record replaced", feed_json([a, c], head), "DigestMismatch")
    add("empty-set-for-a-head-of-two", "everything dropped", feed_json([], head), "CountMismatch")
    dup_head = make_head(pubr, 3, NOW, [a, a])
    add("duplicate-record", "the same record twice, head over both", feed_json([a, a], dup_head), "DuplicateRevocation")
    add("replayed-old-head", "an old head with a newer set", feed_json([a, b], make_head(pubr, 1, NOW - 500, [a])), "CountMismatch")
    add("old-head-after-newer", "an old head, genuine, after a newer one was seen",
        feed_json([a], make_head(pubr, 1, NOW - 500, [a]), last_seen=2), "SequenceRollback")
    add("wrong-publisher", "head signed by another key",
        feed_json([a, b], make_head("mallory", 3, NOW, [a, b]), publisher="publisher"), "WrongPublisher")
    forged = make_head(pubr, 3, NOW, [a, b])
    bad = bytearray(forged["encoded"])
    bad[bad.index(b"\x63sig\x58\x40") + 6] ^= 1
    add("head-bad-signature", "one bit of the head signature flipped",
        {"publisher": pub(pubr).hex(), "last_seen_sequence": None,
         "revocations": [a["encoded"].hex(), b["encoded"].hex()], "head": bytes(bad).hex()}, "BadHeadSignature")
    tampered = dict(forged["body"], as_of=NOW - 1)
    add("head-tampered-as-of", "as_of changed, signature kept",
        {"publisher": pub(pubr).hex(), "last_seen_sequence": None,
         "revocations": [a["encoded"].hex(), b["encoded"].hex()],
         "head": encode({"body": tampered, "sig": forged["sig"]}).hex()}, "BadHeadSignature")
    add("head-future", "as_of after the verifier's clock", feed_json([a, b], make_head(pubr, 3, NOW + 1, [a, b])),
        "FutureHead")
    add("head-unknown-version", "version 2", feed_json([a, b], make_head(pubr, 3, NOW, [a, b], v=2)), "UnknownVersion")
    add("head-wrong-count", "count says 3, digest of two (signed)",
        feed_json([a, b], make_head(pubr, 3, NOW, [a, b], count=3)), "CountMismatch")
    add("head-wrong-digest", "digest of nothing (signed)",
        feed_json([a, b], make_head(pubr, 3, NOW, [a, b], digest=feed_digest([]))), "DigestMismatch")
    badrev = make_revocation("alice", ids[1], 4000, 1, revoker="bob")
    add("record-bad-signature", "a record signed by someone else, head over the set",
        feed_json([badrev], make_head(pubr, 3, NOW, [badrev])), "BadRevocationSignature")
    oldrev = make_revocation("alice", ids[1], 4000, 1, v=2)
    add("record-unknown-version", "a record of version 2, head over the set",
        feed_json([oldrev], make_head(pubr, 3, NOW, [oldrev])), "UnknownVersion")
    later = make_revocation("alice", ids[1], NOW + 1, 1)
    add("record-after-the-head", "revoked_at after the head's as_of",
        feed_json([later], make_head(pubr, 3, NOW, [later])), "RevokedAfterHead")
    cases.append({"label": "undecodable-record", "note": "a record with a trailing byte", "now": NOW,
                  "feed": {"publisher": pub(pubr).hex(), "last_seen_sequence": None,
                           "revocations": [(a["encoded"] + b"\x00").hex()],
                           "head": make_head(pubr, 3, NOW, [a])["encoded"].hex()},
                  "expect": "DecodeError"})
    return cases


def feed_step_case(label, note, expect, revs, head=None, bounds=None, effect="reversible", feed_over=None,
                   now=NOW, last_seen=None):
    """A chain and invocation checked against a feed (step 8)."""
    case, chain, _ = chain_case(label, note, expect, inv={"effect": effect}, now=now)
    head = head or make_head("publisher", 1, now, revs)
    case["feed"] = feed_json(revs, head, last_seen=last_seen)
    if feed_over:
        case["feed"].update(feed_over)
    case["revocation_bounds"] = bounds or {"read": 0, "reversible": 0, "irreversible": 0}
    del case["revoked"]
    del case["revocation_as_of"]
    return case, chain


def make_feed_step_cases():
    base, chain = feed_step_case("x", "x", ok(), [])
    ids = [g["id"] for g in chain]
    cases = []

    def add(label, note, expect, revs, **kw):
        case, _ = feed_step_case(label, note, expect, revs, **kw)
        _, _, error = feed_step8(case)
        if error is not None:
            case["feed_error"] = error
        cases.append(case)

    add("feed-empty-and-current", "an empty feed as of now", ok(), [])
    add("alice-revokes-the-middle-grant", "the issuer of grant 2 revokes it; the leaf is below",
        refused("Revoked"), [make_revocation("alice", ids[1], 4000, 1)])
    add("owner-revokes-the-root-grant", "the root issuer revokes grant 1",
        refused("Revoked"), [make_revocation("owner", ids[0], 4000, 1)])
    add("owner-revokes-the-last-grant", "the root issuer revokes the last grant",
        refused("Revoked"), [make_revocation("owner", ids[2], 4000, 1)])
    add("bob-revokes-the-last-grant", "the issuer of the last grant revokes it",
        refused("Revoked"), [make_revocation("bob", ids[2], 4000, 1)])
    add("alice-revokes-the-last-grant", "an issuer above the last grant revokes it",
        refused("Revoked"), [make_revocation("alice", ids[2], 4000, 1)])
    add("bob-revokes-a-grant-above-hers", "bob issued only grant 3; grant 2 is above it: ignored",
        ok(), [make_revocation("bob", ids[1], 4000, 1)])
    add("alice-revokes-the-root-grant", "alice issued grant 2, below grant 1: ignored",
        ok(), [make_revocation("alice", ids[0], 4000, 1)])
    add("carol-revokes-the-last-grant", "carol only holds grant 3: ignored",
        ok(), [make_revocation("carol", ids[2], 4000, 1)])
    add("mallory-revokes-the-middle-grant", "a stranger: ignored",
        ok(), [make_revocation("mallory", ids[1], 4000, 1)])
    add("revocation-of-an-unrelated-grant", "a grant outside the chain: nothing to do",
        ok(), [make_revocation("owner", bytes([0x42]) * 32, 4000, 1)])

    stale = make_head("publisher", 1, NOW - 60, [])
    bounds = {"read": 100, "reversible": 50, "irreversible": 0}
    add("data-60s-old-reversible-bound-50", "age 60 over the bound of 50", refused("RevocationDataStale"), [],
        head=stale, bounds=bounds)
    add("data-60s-old-reversible-bound-60", "age equal to the bound", ok(), [],
        head=stale, bounds=dict(bounds, reversible=60))
    add("data-60s-old-read-bound-100", "a read may use older data", ok(), [],
        head=stale, bounds=bounds, effect="read")
    add("data-1s-old-irreversible-bound-0", "stale at step 8, before step 10 refuses the class",
        refused("RevocationDataStale"), [], head=make_head("publisher", 1, NOW - 1, []), bounds=bounds,
        effect="irreversible")
    add("data-current-irreversible", "fresh data reaches step 10, which refuses the class",
        refused("EffectExceeded"), [], bounds=bounds, effect="irreversible")
    add("default-bounds-refuse-any-age", "age 1 and the all-zero bounds",
        refused("RevocationDataStale"), [], head=make_head("publisher", 1, NOW - 1, []))
    add("revoked-before-stale-is-considered", "a revoked grant is refused as revoked",
        refused("Revoked"), [make_revocation("owner", ids[0], 4000, 1)],
        head=make_head("publisher", 1, NOW - 50, [make_revocation("owner", ids[0], 4000, 1)]), bounds=bounds)

    # A feed that does not load is no data: step 8 refuses as stale.
    rev = make_revocation("alice", ids[1], 4000, 1)
    add("feed-with-a-truncated-set", "the head lists a record the set lacks: no data",
        refused("RevocationDataStale"), [], head=make_head("publisher", 1, NOW, [rev]))
    add("feed-with-a-rolled-back-sequence", "the head does not advance: no data",
        refused("RevocationDataStale"), [rev], last_seen=1)
    add("feed-from-the-wrong-publisher", "no data",
        refused("RevocationDataStale"), [rev], feed_over={"publisher": pub("mallory").hex()})
    return cases


def decision_json(ids, limits, expiries, horizon, invoker, nonce_byte, now, cost=0, nbytes=0):
    return {"chain_ids": [i.hex() for i in ids], "chain_limits": limits, "chain_expiries": expiries,
            "nonce_horizon": horizon, "invoker": pub(invoker).hex(), "nonce": (bytes([nonce_byte]) * 16).hex(),
            "now": now, "cost": cost, "bytes": nbytes}


def ledger_case(label, note, steps, nonces=(), spent=(), max_nonces=65536, max_grants=16384):
    case = {"label": label, "note": note,
            "config": {"max_nonces": max_nonces, "max_grants": max_grants},
            "steps": [{"decision": d, "expect": None} for d in steps],
            "queries": {"nonces": [{"invoker": pub(i).hex(), "nonce": (bytes([n]) * 16).hex()} for i, n in nonces],
                        "spent": [{"id": g.hex(), "now": t} for g, t in spent]}}
    results, out = run_ledger_case(case)
    for step, result in zip(case["steps"], results):
        step["expect"] = result
    case["queries"] = {"nonces": out["nonces"], "spent": out["spent"], "nonce_count": out["nonce_count"],
                       "grant_count": out["grant_count"]}
    return case


def make_ledger_cases():
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    ids = [g["id"] for g in chain]
    sb = standard_bodies()
    limits = [b["limits"] for b in sb]
    exps = [b["exp"] for b in sb]
    one = lim(calls=1)
    rate2 = lim(rate=2)
    nolim = lim()
    cases = []
    add = cases.append

    def full(n, now, **kw):
        return decision_json(ids, limits, exps, now + 60, "carol", n, now, **kw)

    add(ledger_case(
        "charge-the-whole-chain", "one charge: every grant, and the nonce",
        [full(1, NOW, cost=1, nbytes=10)],
        nonces=[("carol", 1), ("carol", 2)], spent=[(g, NOW) for g in ids]))
    add(ledger_case(
        "replay-of-a-recorded-nonce", "the second charge with the same nonce changes nothing",
        [full(1, NOW, cost=1, nbytes=10), full(1, NOW + 1, cost=1, nbytes=10)],
        nonces=[("carol", 1)], spent=[(ids[0], NOW + 1)]))
    # Atomic: the last grant has one call; a charge of the chain after another charge of the last grant alone.
    add(ledger_case(
        "exhausted-last-grant-charges-nothing", "the third grant is used up: the first two stay untouched",
        [decision_json([ids[2]], [one], [exps[2]], NOW + 60, "carol", 1, NOW, 1, 1),
         decision_json(ids, [limits[0], limits[1], one], exps, NOW + 60, "carol", 2, NOW, 1, 1)],
        nonces=[("carol", 1), ("carol", 2)], spent=[(g, NOW) for g in ids]))
    add(ledger_case(
        "exhausted-middle-grant-charges-nothing", "the middle grant is used up: the others stay untouched",
        [decision_json([ids[1]], [lim(calls=1)], [exps[1]], NOW + 60, "carol", 1, NOW, 1, 1),
         decision_json(ids, [limits[0], one, limits[2]], exps, NOW + 60, "carol", 2, NOW, 1, 1)],
        nonces=[("carol", 2)], spent=[(g, NOW) for g in ids]))
    add(ledger_case(
        "cost-and-bytes-ceilings", "cost 100 and bytes 10000 on the last grant",
        [full(1, NOW, cost=100, nbytes=10000), full(2, NOW, cost=1, nbytes=0), full(3, NOW, cost=0, nbytes=1)],
        spent=[(ids[2], NOW)]))
    add(ledger_case(
        "rate-window-rolls", "a rate of 2: the window follows the time passed in",
        [decision_json([ids[2]], [rate2], [9000], t + 60, "carol", n, t)
         for n, t in [(1, 1000), (2, 1001), (3, 1002), (4, 1060), (5, 1060), (6, 1121)]],
        spent=[(ids[2], 1000), (ids[2], 1060), (ids[2], 1121), (ids[2], 1180)]))
    add(ledger_case(
        "grants-without-limits-are-not-tracked", "a nonce is recorded, no usage record is made",
        [decision_json([ids[0]], [nolim], [exps[0]], NOW + 60, "carol", 1, NOW)],
        nonces=[("carol", 1)], spent=[(ids[0], NOW)]))
    add(ledger_case(
        "malformed-decisions", "lists of different lengths, and a grant twice",
        [decision_json([ids[0]], [], [exps[0]], NOW + 60, "carol", 1, NOW),
         decision_json([ids[0]], [one], [], NOW + 60, "carol", 2, NOW),
         decision_json([ids[0], ids[0]], [one, one], [9000, 9000], NOW + 60, "carol", 3, NOW)],
        nonces=[("carol", 1), ("carol", 2), ("carol", 3)]))
    # Nonce retention: kept until the horizon, refused rather than forgotten when full.
    nonce_steps = ([decision_json([], [], [], 160, "carol", n, 100) for n in range(1, 5)]
                   + [decision_json([], [], [], 160, "carol", 9, 100),
                      decision_json([], [], [], 220, "carol", 9, 160),
                      decision_json([], [], [], 221, "carol", 9, 161),
                      decision_json([], [], [], 221, "carol", 1, 161)])
    add(ledger_case(
        "nonces-are-kept-until-the-horizon", "four nonces with horizon 160, room for four: refuse, do not forget; "
        "evict only after 160",
        nonce_steps, nonces=[("carol", n) for n in (1, 2, 3, 4, 9)], max_nonces=4))
    add(ledger_case(
        "usage-records-go-when-the-grant-expires", "room for two grants: a third needs an expired one gone",
        [decision_json([ids[0]], [one], [500], 160, "carol", 1, 100),
         decision_json([ids[1]], [one], [600], 160, "carol", 2, 100),
         decision_json([ids[2]], [one], [700], 460, "carol", 3, 400),
         decision_json([ids[2]], [one], [700], 560, "carol", 3, 500)],
        spent=[(ids[0], 500), (ids[1], 500), (ids[2], 500)], max_grants=2))
    return cases


def make_receipts_section():
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    inv = sign_invocation("carol", standard_invocation(chain[-1]["id"]))
    other = sign_invocation("carol", standard_invocation(chain[-1]["id"], nonce_byte=8))
    leaf = chain[-1]["id"]
    allowed = {"ok": True, "leaf": leaf.hex(), "invoker": pub("carol").hex(), "nonce": (bytes([7]) * 16).hex(),
               "effect": "reversible", "cost": 1, "bytes": 10}
    denied8 = {"ok": False, "denial": "Revoked", "step": 8}
    denied8b = {"ok": False, "denial": "RevocationDataStale", "step": 8}
    denied11 = {"ok": False, "denial": "LimitExhausted", "step": 11}
    cases = []

    def add(label, note, receipt, verdict, expect_verify, expect_matches, recipient="verifier", inv_record=None):
        record = inv_record or inv
        cases.append({
            "label": label, "note": note, "receipt": receipt["encoded"].hex(),
            "expected_recipient": pub(recipient).hex(), "invocation": record["encoded"].hex(),
            "verdict": verdict, "expect_verify": expect_verify, "expect_matches": expect_matches})

    good = make_receipt("verifier", inv, allowed)
    add("allowed", "an allowed invocation, charged and performed", good, allowed, "ok", "ok")
    add("denied-at-step-8", "a denial records the step only", make_receipt("verifier", inv, denied8), denied8, "ok", "ok")
    add("two-denials-of-one-step-are-one-receipt", "the receipt for RevocationDataStale is the receipt for Revoked",
        make_receipt("verifier", inv, denied8), denied8b, "ok", "ok")
    add("denied-at-step-11", "limit exhausted", make_receipt("verifier", inv, denied11), denied11, "ok", "ok")
    add("wrong-expected-recipient", "checked against another key", good, allowed, "WrongRecipient", "ok",
        recipient="alice")
    bad = bytearray(good["encoded"])
    bad[bad.index(b"\x63sig\x58\x40") + 6] ^= 1
    cases.append({"label": "bad-signature", "note": "one bit of the signature flipped",
                  "receipt": bytes(bad).hex(), "expected_recipient": pub("verifier").hex(),
                  "invocation": inv["encoded"].hex(), "verdict": allowed, "expect_verify": "BadSignature",
                  "expect_matches": "ok"})
    add("signed-by-someone-else", "mallory signs a receipt that names the verifier",
        sign_object(RECEIPT_DOMAIN, "mallory", dict(good["body"])), allowed, "BadSignature", "ok")
    tampered = dict(good["body"], cost=2)
    add("tampered-cost", "cost changed, signature kept",
        {"encoded": encode({"body": tampered, "sig": good["sig"]})}, allowed, "BadSignature", "OutcomeMismatch")
    add("recorded-cost-differs-from-the-decision", "a genuine receipt of another cost",
        sign_object(RECEIPT_DOMAIN, "verifier", dict(good["body"], cost=2)), allowed, "ok", "OutcomeMismatch")
    add("allowed-without-effect", "inconsistent: allowed but no effect class",
        sign_object(RECEIPT_DOMAIN, "verifier", dict(good["body"], effect=None)), allowed, "Malformed", "OutcomeMismatch")
    add("allowed-with-a-step", "inconsistent: allowed with a refusing step",
        sign_object(RECEIPT_DOMAIN, "verifier", dict(good["body"], step=3)), allowed, "Malformed", "OutcomeMismatch")
    add("allowed-with-two-calls", "calls must be 1 when allowed",
        sign_object(RECEIPT_DOMAIN, "verifier", dict(good["body"], calls=2)), allowed, "Malformed", "OutcomeMismatch")
    den = make_receipt("verifier", inv, denied8)
    for label, change in [("denial-with-a-cost", {"cost": 1}), ("denial-with-bytes", {"bytes": 1}),
                          ("denial-with-a-call", {"calls": 1}), ("denial-with-a-result", {"result": bytes([1]) * 32}),
                          ("denial-with-an-effect", {"effect": "read"}), ("denial-with-step-12", {"step": 12}),
                          ("denial-with-step-0", {"step": 0}), ("denial-without-a-step", {"step": None})]:
        add(label, "a denial that says more than the step, or an impossible step",
            sign_object(RECEIPT_DOMAIN, "verifier", dict(den["body"], **change)), denied8, "Malformed",
            "OutcomeMismatch")
    add("unknown-version", "version 2", make_receipt("verifier", inv, allowed, v=2), allowed, "Malformed",
        "OutcomeMismatch")
    add("receipt-for-another-invocation", "a genuine receipt, checked against a different invocation",
        good, allowed, "ok", "InvocationMismatch", inv_record=other)
    add("receipt-for-an-allowed-invocation-checked-against-a-denial", "recorded as allowed, the verdict was a denial",
        good, denied8, "ok", "OutcomeMismatch")
    add("denial-checked-against-another-step", "recorded at step 8, the verdict says 11",
        make_receipt("verifier", inv, denied8), denied11, "ok", "OutcomeMismatch")
    add("wrong-leaf", "a genuine receipt naming another last grant",
        sign_object(RECEIPT_DOMAIN, "verifier", dict(good["body"], leaf=bytes([1]) * 32)), allowed, "ok", "LeafMismatch")
    add("decision-for-another-nonce", "the decision's nonce is not the invocation's", good,
        dict(allowed, nonce=(bytes([8]) * 16).hex()), "ok", "DecisionMismatch")
    return cases


def make_receipt_records():
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    inv = sign_invocation("carol", standard_invocation(chain[-1]["id"]))
    allowed = {"ok": True, "leaf": chain[-1]["id"].hex(), "invoker": pub("carol").hex(),
               "nonce": (bytes([7]) * 16).hex(), "effect": "reversible", "cost": 1, "bytes": 10}
    records = [
        ("allowed", make_receipt("verifier", inv, allowed)),
        ("allowed-irreversible-extreme-amounts",
         make_receipt("verifier", inv, dict(allowed, effect="irreversible", cost=U64_MAX, bytes=U64_MAX),
                      time=U64_MAX)),
        ("denied-at-step-8", make_receipt("verifier", inv, {"ok": False, "denial": "Revoked", "step": 8})),
        ("denied-at-step-1", make_receipt("verifier", inv, {"ok": False, "denial": "ChainLimits", "step": 1})),
        ("denied-at-step-11", make_receipt("verifier", inv, {"ok": False, "denial": "LimitExhausted", "step": 11})),
    ]
    out = []
    for label, r in records:
        b = r["body"]
        out.append({
            "label": label,
            "signer": r["signer"],
            "body": {"v": b["v"], "recipient": b["recipient"].hex(), "invocation": b["invocation"].hex(),
                     "leaf": b["leaf"].hex(), "allowed": b["allowed"], "effect": b["effect"], "step": b["step"],
                     "calls": b["calls"], "cost": b["cost"], "bytes": b["bytes"], "result": b["result"].hex(),
                     "time": b["time"]},
            "signed_bytes": r["signed"].hex(),
            "signature": r["sig"].hex(),
            "encoded": r["encoded"].hex(),
        })
    return out


def make_receipt_checks():
    chain = build_chain(standard_bodies(), STANDARD_SIGNERS)
    inv = sign_invocation("carol", standard_invocation(chain[-1]["id"]))
    allowed = {"ok": True, "leaf": chain[-1]["id"].hex(), "invoker": pub("carol").hex(),
               "nonce": (bytes([7]) * 16).hex(), "effect": "reversible", "cost": 1, "bytes": 10}
    good = make_receipt("verifier", inv, allowed)
    cases = [("valid", good["encoded"], "ok")]
    for label, raw in mutations(good["encoded"], array_value=chain[-1]["id"]):
        cases.append((label, raw, "DecodeError"))
    cases.append(("oversize", bytes(MAX_RECEIPT_BYTES + 1), "DecodeError"))
    return [{"label": label, "encoded": raw.hex(), "expected_recipient": pub("verifier").hex(), "expect": expect}
            for label, raw, expect in cases]


def make_head_checks():
    revs = []
    good = make_head("publisher", 1, NOW, revs)
    cases = [("valid", good["encoded"], "ok")]
    for label, raw in mutations(good["encoded"], array_value=good["body"]["digest"]):
        cases.append((label, raw, "DecodeError"))
    cases.append(("oversize", bytes(MAX_FEED_HEAD_BYTES + 1), "DecodeError"))
    cases.append(("unknown-version", make_head("publisher", 1, NOW, revs, v=2)["encoded"], "UnknownVersion"))
    cases.append(("signed-by-another-publisher", make_head("mallory", 1, NOW, revs)["encoded"], "WrongPublisher"))
    return [{"label": label, "encoded": raw.hex(), "publisher": pub("publisher").hex(), "expect": expect}
            for label, raw, expect in cases]


CONVENTIONS = [
    "spine-authority v1 conformance vectors. Generated by scripts/authority_v1.py from docs/spec/03-authority.md; do not edit by hand.",
    "Regenerate: python scripts/authority_v1.py generate > docs/spec/03-authority.vectors.json. Check: python scripts/authority_v1.py check docs/spec/03-authority.vectors.json.",
    "All 'hex' strings are lowercase, no spaces or prefix. 'seed' and 'public' are Ed25519 key bytes (32 bytes).",
    "grants: 'body' is the structured description (every byte string as hex, 'parent' null in a root). 'signed_bytes' = domains.grant followed by the deterministic CBOR of the body; 'signature' is Ed25519 over them; 'encoded' is the deterministic CBOR of {body, sig}; 'id' = SHA-256(domains.grant_id || encoded).",
    "invocations: same shape; 'signed_bytes' = domains.invocation || CBOR of the body; 'encoded' is CBOR of {body, sig}.",
    "cases: run the invocation against the chain as encoded bytes (the entries of 'chain', root first), with: 'now' (Unix seconds), 'recipient' (the verifier's own key), 'session' (its session binding), 'trusted_roots', 'revoked' (grant ids), 'revocation_as_of' (the time the revocation data is complete as of, or null for none), 'seen_nonces' (invoker/nonce pairs already used) and 'preload' (totals already charged to grant ids). Policy: no clock skew, invocations at most 60 seconds old, revocation data must be as of now or later (bound 0 for every effect class), no other state.",
    "cases[].expect is {ok: true} or {ok: false, step, denial}: verification must stop at that step with that denial, reading the chain as bytes (a chain entry that does not decode is step 1 if over a bound, step 2 otherwise).",
    "invocations also carry 'id' = SHA-256(domains.invocation_id || encoded), the value receipts cite.",
    "revocations, feed_heads, receipts: same shape as grants. 'signed_bytes' = the domain || CBOR of the body; 'encoded' is CBOR of {body, sig}. A revocation's 'id' = SHA-256(domains.revocation_id || encoded). A feed head's digest = SHA-256(domains.feed_digest || the revocation ids sorted ascending, concatenated).",
    "revocation_checks, feed_head_checks, receipt_checks: an encoding and the outcome of decoding and verifying it: 'ok', 'DecodeError' (not the canonical encoding of a value of that shape, or over the size bound), or the error name (UnknownVersion, BadRevocationSignature, WrongPublisher, BadHeadSignature, Malformed, WrongRecipient, BadSignature). For receipts 'expected_recipient' is the key the verifier expects; the checks run in the order stated.",
    "authority_cases: whether the revoker of 'revocation' may revoke its grant for the given 'chain': the grant must be in the chain and the revoker must be the issuer of that grant or of a grant above it.",
    "feed_cases: whether a feed (hex 'revocations', hex 'head', the trusted 'publisher', 'last_seen_sequence' or null, and the verifier's clock 'now') loads, in this order: decode (DecodeError), head version (UnknownVersion), publisher (WrongPublisher), head signature (BadHeadSignature), sequence above last seen (SequenceRollback), as_of not after now (FutureHead), at most max_feed_revocations (TooMany), no duplicate record (DuplicateRevocation), count (CountMismatch), digest (DigestMismatch), then for each record version (UnknownVersion), signature (BadRevocationSignature) and revoked_at not after as_of (RevokedAfterHead). 'expect' is 'ok' or the first error.",
    "feed_step_cases: like cases, but step 8 reads a feed instead of 'revoked'/'revocation_as_of'. A feed that does not load is no data (step 8 refuses as stale; 'feed_error' says why). Otherwise the feed's as_of is the age basis and a grant counts as revoked when a record in the feed names it and its revoker issued that grant or a grant above it in the chain. 'revocation_bounds' gives the staleness bound in seconds per effect class.",
    "receipt_cases: 'verdict' is the outcome the receipt should record: {ok: true, leaf, invoker, nonce, effect, cost, bytes} for an allowed invocation or {ok: false, denial, step} for a refusal. 'expect_verify' is the outcome of checking the receipt against 'expected_recipient'; 'expect_matches' the outcome of matching it to 'invocation' and 'verdict' (invocation id, last grant, recipient, then the whole body the verdict implies, with the receipt's own result digest and time; DecisionMismatch when the verdict is not for that invocation).",
    "ledger_cases: a fresh in-memory ledger with 'config' bounds; each step charges one decision (chain_ids, chain_limits and chain_expiries in the same order, nonce_horizon, invoker, nonce, now, cost, bytes) and states the result: ok, NonceReplayed, LimitExhausted, LedgerFull or MalformedDecision. A refused charge changes nothing. 'queries' then state which nonces are remembered, what each grant has been charged (calls, cost, bytes, and calls within the 60 seconds before the query time) and how many nonces and grants have records.",
    "bounds and domains are normative constants; an implementation must use them.",
]


def build_new_sections():
    """The revocation, feed, receipt and ledger sections, each case checked
    against the reference implementation as it is built."""
    sections = {
        "revocations": make_revocations_section(),
        "feed_heads": make_heads_section(),
        "receipts": make_receipt_records(),
        "revocation_checks": make_revocation_checks(),
        "feed_head_checks": make_head_checks(),
        "receipt_checks": make_receipt_checks(),
        "authority_cases": make_authority_cases(),
        "feed_cases": make_feed_cases(),
        "feed_step_cases": make_feed_step_cases(),
        "receipt_cases": make_receipts_section(),
        "ledger_cases": make_ledger_cases(),
    }
    check_new_sections(sections, {k["label"]: k for k in make_keys()})
    return sections


def build_document():
    cases = make_cases()
    for c in cases:
        got = verify_case(c)
        want = None if c["expect"]["ok"] else c["expect"]["denial"]
        if got != want:
            raise AssertionError("case %s: the reference verifier says %r, the case expects %r"
                                 % (c["label"], got, want))
    sections = build_new_sections()
    doc = {
        "conventions": CONVENTIONS,
        "domains": DOMAINS,
        "bounds": BOUNDS,
        "keys": make_keys(),
        "grants": make_grants(),
        "invocations": make_invocations(),
        "cases": cases,
    }
    doc.update(sections)
    return doc


def generate():
    rfc8032_self_check()
    doc = build_document()
    return json.dumps(doc, indent=2, sort_keys=True, ensure_ascii=True) + "\n"


# ---------------------------------------------------------------------------
# check
# ---------------------------------------------------------------------------

class Failure(Exception):
    pass


def expect_(cond, message):
    if not cond:
        raise Failure(message)


def hex_body(j, fields):
    out = dict(j)
    for f in fields:
        out[f] = bytes.fromhex(j[f])
    return out


def check_signed_records(name, items, domain, byte_fields, signer_field, decode, keys):
    for r in items:
        w = name + "/" + r["label"]
        body = hex_body(r["body"], byte_fields)
        signed = domain + encode(body)
        expect_(signed.hex() == r["signed_bytes"], w + ": signed_bytes")
        sig = bytes.fromhex(r["signature"])
        expect_(ed25519_verify(body[signer_field], signed, sig, strict=True), w + ": signature does not verify")
        expect_(ed25519_sign(bytes.fromhex(keys[r["signer"]]["seed"]), signed) == sig, w + ": re-signing differs")
        encoded = encode({"body": body, "sig": sig})
        expect_(encoded.hex() == r["encoded"], w + ": encoded")
        expect_(decode(encoded) == {"body": body, "sig": sig}, w + ": decode")
        if "id" in r:
            expect_(revocation_id(encoded).hex() == r["id"], w + ": id")


def check_new_sections(doc, keys):
    """Re-derives and runs every revocation, feed, receipt and ledger vector.
    Used when building (against the data just made) and by `check`."""
    counts = {}
    check_signed_records("revocations", doc["revocations"], REVOCATION_DOMAIN,
                         ["revoker", "grant", "nonce"], "revoker", decode_revocation, keys)
    counts["revocations"] = len(doc["revocations"])
    check_signed_records("feed_heads", doc["feed_heads"], FEED_HEAD_DOMAIN, ["publisher", "digest"],
                         "publisher", decode_head, keys)
    counts["feed_heads"] = len(doc["feed_heads"])
    for h in doc["feed_heads"]:
        # The digest is over the revocations the head was made for; the empty
        # feed's digest is fixed.
        if h["body"]["count"] == 0:
            expect_(h["body"]["digest"] == feed_digest([]).hex(), "feed_heads/" + h["label"] + ": empty digest")
    check_signed_records("receipts", doc["receipts"], RECEIPT_DOMAIN, ["recipient", "invocation", "leaf", "result"],
                         "recipient", decode_receipt, keys)
    counts["receipts"] = len(doc["receipts"])

    for c in doc["revocation_checks"]:
        got = check_revocation(bytes.fromhex(c["encoded"]))
        expect_(got == c["expect"], "revocation_checks/%s: got %s, expected %s" % (c["label"], got, c["expect"]))
    counts["revocation_checks"] = len(doc["revocation_checks"])
    for c in doc["feed_head_checks"]:
        got = check_head(bytes.fromhex(c["encoded"]), bytes.fromhex(c["publisher"]))
        expect_(got == c["expect"], "feed_head_checks/%s: got %s, expected %s" % (c["label"], got, c["expect"]))
    counts["feed_head_checks"] = len(doc["feed_head_checks"])
    for c in doc["receipt_checks"]:
        got = check_receipt(bytes.fromhex(c["encoded"]), bytes.fromhex(c["expected_recipient"]))
        expect_(got == c["expect"], "receipt_checks/%s: got %s, expected %s" % (c["label"], got, c["expect"]))
    counts["receipt_checks"] = len(doc["receipt_checks"])

    for c in doc["authority_cases"]:
        got = revocation_authorized(decode_revocation(bytes.fromhex(c["revocation"])), chain_bytes_of(c["chain"]))
        expect_(got == c["expect_authorized"], "authority_cases/" + c["label"])
    counts["authority_cases"] = len(doc["authority_cases"])

    seen = set()
    for c in doc["feed_cases"]:
        got = feed_load(c["feed"], c["now"])[0]
        expect_(got == c["expect"], "feed_cases/%s: got %s, expected %s" % (c["label"], got, c["expect"]))
        seen.add(got)
    expect_(seen == {"ok", "DecodeError", "UnknownVersion", "WrongPublisher", "BadHeadSignature",
                     "SequenceRollback", "FutureHead", "DuplicateRevocation", "CountMismatch", "DigestMismatch",
                     "BadRevocationSignature", "RevokedAfterHead"}, "feed_cases: every error but TooMany has a case")
    counts["feed_cases"] = len(doc["feed_cases"])

    for c in doc["feed_step_cases"]:
        w = "feed_step_cases/" + c["label"]
        got = verify_feed_case(c)
        want = None if c["expect"]["ok"] else c["expect"]["denial"]
        expect_(got == want, "%s: the verifier says %r, the case says %r" % (w, got, want))
        if not c["expect"]["ok"]:
            expect_(DENIALS[c["expect"]["denial"]] == c["expect"]["step"], w + ": step number")
        expect_(c.get("feed_error") == feed_step8(c)[2], w + ": feed_error")
    counts["feed_step_cases"] = len(doc["feed_step_cases"])

    for c in doc["receipt_cases"]:
        w = "receipt_cases/" + c["label"]
        raw = bytes.fromhex(c["receipt"])
        got = check_receipt(raw, bytes.fromhex(c["expected_recipient"]))
        expect_(got == c["expect_verify"], "%s: verify gives %s, expected %s" % (w, got, c["expect_verify"]))
        got = match_receipt(raw, bytes.fromhex(c["invocation"]), c["verdict"])
        expect_(got == c["expect_matches"], "%s: matches gives %s, expected %s" % (w, got, c["expect_matches"]))
    counts["receipt_cases"] = len(doc["receipt_cases"])

    results_seen = set()
    for c in doc["ledger_cases"]:
        w = "ledger_cases/" + c["label"]
        results, out = run_ledger_case(c)
        expect_(results == [step["expect"] for step in c["steps"]], w + ": results")
        expect_(out == c["queries"], w + ": queries")
        results_seen.update(results)
    expect_(results_seen == {"ok", "NonceReplayed", "LimitExhausted", "LedgerFull", "MalformedDecision"},
            "ledger_cases: every charge result has a case")
    counts["ledger_cases"] = len(doc["ledger_cases"])
    return counts


def check_doc(doc):
    n = rfc8032_self_check()
    counts = {}
    expect_(doc["domains"] == DOMAINS, "domains")
    expect_(doc["bounds"] == BOUNDS, "bounds")

    keys = {k["label"]: k for k in doc["keys"]}
    for k in doc["keys"]:
        w = "keys/" + k["label"]
        expect_(k["public"] == ed25519_public(bytes.fromhex(k["seed"])).hex(), w + ": public key")
    counts["keys"] = len(doc["keys"])

    for g in doc["grants"]:
        w = "grants/" + g["label"]
        body = body_from_json(g["body"])
        signed = GRANT_DOMAIN + encode(body)
        expect_(signed.hex() == g["signed_bytes"], w + ": signed_bytes")
        sig = bytes.fromhex(g["signature"])
        expect_(ed25519_verify(body["iss"], signed, sig, strict=True), w + ": signature does not verify")
        expect_(ed25519_sign(bytes.fromhex(keys[g["signer"]]["seed"]), signed) == sig, w + ": re-signing differs")
        expect_(bytes.fromhex(keys[g["signer"]]["public"]) == body["iss"], w + ": signer is the issuer")
        encoded = encode({"body": body, "sig": sig})
        expect_(encoded.hex() == g["encoded"], w + ": encoded")
        expect_(grant_id(encoded).hex() == g["id"], w + ": id")
        expect_(decode_grant(encoded) == {"body": body, "sig": sig}, w + ": decode")
    counts["grants"] = len(doc["grants"])

    for i in doc["invocations"]:
        w = "invocations/" + i["label"]
        body = inv_body_from_json(i["body"])
        signed = INVOCATION_DOMAIN + encode(body)
        expect_(signed.hex() == i["signed_bytes"], w + ": signed_bytes")
        sig = bytes.fromhex(i["signature"])
        expect_(ed25519_verify(body["invoker"], signed, sig, strict=True), w + ": signature does not verify")
        expect_(ed25519_sign(bytes.fromhex(keys[i["signer"]]["seed"]), signed) == sig, w + ": re-signing differs")
        encoded = encode({"body": body, "sig": sig})
        expect_(encoded.hex() == i["encoded"], w + ": encoded")
        expect_(invocation_id(encoded).hex() == i["id"], w + ": id")
        expect_(decode_invocation(encoded) == {"body": body, "sig": sig}, w + ": decode")
    counts["invocations"] = len(doc["invocations"])

    seen_denials = set()
    labels = set()
    for c in doc["cases"]:
        w = "cases/" + c["label"]
        expect_(c["label"] not in labels, w + ": duplicate label")
        labels.add(c["label"])
        got = verify_case(c)
        e = c["expect"]
        if e["ok"]:
            expect_(got is None, w + ": the verifier refuses with %s but the case is accepted" % got)
        else:
            expect_(got == e["denial"], w + ": the verifier says %r, the case says %r" % (got, e["denial"]))
            expect_(DENIALS[e["denial"]] == e["step"], w + ": step number")
            seen_denials.add(e["denial"])
    # Every denial that a stateless byte-level case can reach has a case.
    missing = sorted(set(DENIALS) - seen_denials - {"UsageUnknown"})
    expect_(not missing, "no case for denial(s): %s" % ", ".join(missing))
    counts["cases"] = len(doc["cases"])
    counts["denial_kinds"] = len(seen_denials)
    counts.update(check_new_sections(doc, keys))
    return n, counts


def main(argv):
    if len(argv) == 2 and argv[1] == "generate":
        sys.stdout.buffer.write(generate().encode("utf-8"))
        return 0
    if len(argv) == 3 and argv[1] == "check":
        with open(argv[2], "rb") as f:
            doc = json.loads(f.read().decode("utf-8"))
        try:
            n, counts = check_doc(doc)
        except Failure as e:
            sys.stderr.write("FAIL: %s\n" % e)
            return 1
        except (KeyError, ValueError, TypeError, AssertionError, IndexError) as e:
            sys.stderr.write("FAIL: malformed or inconsistent vector file: %r\n" % (e,))
            return 1
        sys.stderr.write(
            "ok: RFC 8032 vectors %d/%d; %s\n"
            % (n, n, ", ".join("%s %d" % kv for kv in sorted(counts.items())))
        )
        return 0
    sys.stderr.write(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
