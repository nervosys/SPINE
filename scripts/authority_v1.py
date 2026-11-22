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
    if as_of is None or max(now - as_of, 0) > 0:
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
# Building vectors
# ---------------------------------------------------------------------------

SEEDS = {"owner": 1, "alice": 2, "bob": 3, "carol": 4, "dave": 5, "verifier": 9, "mallory": 20}
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
    } for label, r in [("standard", rec), ("minimal", rec2)]]


CONVENTIONS = [
    "spine-authority v1 conformance vectors. Generated by scripts/authority_v1.py from docs/spec/03-authority.md; do not edit by hand.",
    "Regenerate: python scripts/authority_v1.py generate > docs/spec/03-authority.vectors.json. Check: python scripts/authority_v1.py check docs/spec/03-authority.vectors.json.",
    "All 'hex' strings are lowercase, no spaces or prefix. 'seed' and 'public' are Ed25519 key bytes (32 bytes).",
    "grants: 'body' is the structured description (every byte string as hex, 'parent' null in a root). 'signed_bytes' = domains.grant followed by the deterministic CBOR of the body; 'signature' is Ed25519 over them; 'encoded' is the deterministic CBOR of {body, sig}; 'id' = SHA-256(domains.grant_id || encoded).",
    "invocations: same shape; 'signed_bytes' = domains.invocation || CBOR of the body; 'encoded' is CBOR of {body, sig}.",
    "cases: run the invocation against the chain as encoded bytes (the entries of 'chain', root first), with: 'now' (Unix seconds), 'recipient' (the verifier's own key), 'session' (its session binding), 'trusted_roots', 'revoked' (grant ids), 'revocation_as_of' (the time the revocation data is complete as of, or null for none), 'seen_nonces' (invoker/nonce pairs already used) and 'preload' (totals already charged to grant ids). Policy: no clock skew, invocations at most 60 seconds old, revocation data must be as of now or later (bound 0 for every effect class), no other state.",
    "cases[].expect is {ok: true} or {ok: false, step, denial}: verification must stop at that step with that denial, reading the chain as bytes (a chain entry that does not decode is step 1 if over a bound, step 2 otherwise).",
    "bounds and domains are normative constants; an implementation must use them.",
]


def build_document():
    cases = make_cases()
    for c in cases:
        got = verify_case(c)
        want = None if c["expect"]["ok"] else c["expect"]["denial"]
        if got != want:
            raise AssertionError("case %s: the reference verifier says %r, the case expects %r"
                                 % (c["label"], got, want))
    return {
        "conventions": CONVENTIONS,
        "domains": {
            "grant": GRANT_DOMAIN.hex(),
            "grant_id": GRANT_ID_DOMAIN.hex(),
            "invocation": INVOCATION_DOMAIN.hex(),
        },
        "bounds": BOUNDS,
        "keys": make_keys(),
        "grants": make_grants(),
        "invocations": make_invocations(),
        "cases": cases,
    }


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


def check_doc(doc):
    n = rfc8032_self_check()
    counts = {}
    expect_(doc["domains"] == {"grant": GRANT_DOMAIN.hex(), "grant_id": GRANT_ID_DOMAIN.hex(),
                               "invocation": INVOCATION_DOMAIN.hex()}, "domains")
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
