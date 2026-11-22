#!/usr/bin/env python3
"""spine-name v1: an independent reference implementation and the vector generator.

Written from the layout description in docs/spec/02-names.md, not from the Rust
code. It shares nothing with `src/spine-name` and uses only the Python
standard library (3.8 or later). The vectors in
`docs/spec/02-names.vectors.json` are produced by it, and
`src/spine-name/tests/vectors.rs` checks the Rust crate against them.

Usage:

    python scripts/names_v1.py generate > docs/spec/02-names.vectors.json
    python scripts/names_v1.py check docs/spec/02-names.vectors.json

What it implements:

* base32: RFC 4648 alphabet, lowercase, unpadded, canonical spelling only
  (decoding rejects non-zero trailing bits).
* Ed25519: the RFC 8032 section 6 reference algorithm, validated at start-up
  against the section 7.1 test vectors 1 to 3, plus the "strict" verification
  profile (reject S >= L, reject a small-order public key or R).
* Names: canonical `spine://` string form and the DHT routing key.
* Name records: the v2 signed byte layout, signing and strict verification.
* Resource ids.

`generate` is deterministic: fixed seeds, sorted keys, LF line ends, no times.
`check` re-derives every value in a vector file and exits non-zero on any
mismatch.
"""

import hashlib
import json
import sys

# ---------------------------------------------------------------------------
# base32 (RFC 4648 alphabet, lowercase, no padding, canonical only)
# ---------------------------------------------------------------------------

B32_ALPHABET = "abcdefghijklmnopqrstuvwxyz234567"


def b32_encode(data):
    bits = "".join(format(b, "08b") for b in data)
    bits += "0" * (-len(bits) % 5)
    return "".join(B32_ALPHABET[int(bits[i:i + 5], 2)] for i in range(0, len(bits), 5))


def b32_decode(text):
    """Return bytes, or None if `text` is not a canonical spelling.

    Upper case letters are accepted and mean the same as lower case. A length
    that leaves 5 or more spare bits (1, 3 or 6 symbols mod 8) cannot be the
    encoding of any byte string, and the spare bits of a valid length must be
    zero.
    """
    text = text.lower()
    if any(c not in B32_ALPHABET for c in text):
        return None
    bits = "".join(format(B32_ALPHABET.index(c), "05b") for c in text)
    spare = len(bits) % 8
    if spare >= 5:
        return None
    if spare and int(bits[len(bits) - spare:], 2) != 0:
        return None
    bits = bits[:len(bits) - spare]
    return bytes(int(bits[i:i + 8], 2) for i in range(0, len(bits), 8))


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
# Names
# ---------------------------------------------------------------------------

SCHEME = "spine://"
WHITESPACE = " \t\r\n\x0b\x0c"
BUILTIN_RELS = [
    "child", "parent", "peer", "provides", "requires", "alternate", "describes",
    "snapshot",
]


def ascii_lower(s):
    return "".join(chr(ord(c) + 32) if "A" <= c <= "Z" else c for c in s)


def normalize_path(raw):
    segments = []
    for seg in raw.split("/"):
        if seg in ("", "."):
            continue
        if seg == "..":
            if segments:
                segments.pop()
            continue
        segments.append(seg)
    trailing = raw.endswith("/") and bool(segments)
    out = "".join("/" + s for s in segments)
    if out == "" or trailing:
        out += "/"
    return out


def parse_authority(raw):
    """Return (kind, canonical_authority, detail) or raise ValueError(reason)."""
    if ":" not in raw:
        raise ValueError("invalid_authority")
    tag, rest = raw.split(":", 1)
    kind = ascii_lower(tag)
    if kind in ("did", "blob"):
        key = b32_decode(rest)
        if key is None or len(key) != 32:
            raise ValueError("invalid_authority")
        return kind, kind + ":" + b32_encode(key), {"key": key.hex()}
    if kind == "cap":
        term = ascii_lower(rest.strip(WHITESPACE))
        if term == "" or any(c in WHITESPACE or c == "/" for c in term):
            raise ValueError("invalid_authority")
        return kind, "cap:" + term, {"term": term}
    if kind == "host":
        rest = rest.strip(WHITESPACE)
        if rest == "":
            raise ValueError("invalid_authority")
        if ":" in rest:
            h, p = rest.rsplit(":", 1)
            if p != "" and all(c in "0123456789" for c in p):
                port = int(p)
                if port > 65535 or h == "":
                    raise ValueError("invalid_authority")
                host = ascii_lower(h)
                return kind, "host:%s:%d" % (host, port), {"host": host, "port": port}
        host = ascii_lower(rest)
        return kind, "host:" + host, {"host": host, "port": None}
    raise ValueError("unknown_authority_kind")


def parse_name(text):
    """Parse and normalise a spine:// name. Returns a dict of components, or
    raises ValueError(reason)."""
    text = text.strip(WHITESPACE)
    if text[:len(SCHEME)].lower() != SCHEME:
        raise ValueError("not_spine_scheme")
    rest = text[len(SCHEME):]
    if rest == "":
        raise ValueError("invalid_authority")
    fragment = None
    if "#" in rest:
        rest, fragment = rest.split("#", 1)
    query_raw = None
    if "?" in rest:
        rest, query_raw = rest.split("?", 1)
    slash = rest.find("/")
    if slash >= 0:
        authority_raw, path_raw = rest[:slash], rest[slash:]
    else:
        authority_raw, path_raw = rest, "/"
    kind, authority, detail = parse_authority(authority_raw)
    path = normalize_path(path_raw)
    query = []
    if query_raw is not None:
        for pair in query_raw.split("&"):
            if pair == "":
                continue
            if "=" in pair:
                k, v = pair.split("=", 1)
                query.append((k, v))
            else:
                query.append((pair, ""))
        query.sort()
    if fragment == "":
        fragment = None
    return {
        "kind": kind, "authority": authority, "detail": detail, "path": path,
        "query": query, "fragment": fragment,
    }


def format_name(parts):
    out = SCHEME + parts["authority"] + parts["path"]
    if parts["query"]:
        out += "?" + "&".join(k if v == "" else "%s=%s" % (k, v) for k, v in parts["query"])
    if parts["fragment"] is not None:
        out += "#" + parts["fragment"]
    return out


def canonical_name(text):
    return format_name(parse_name(text))


def routing_key(text):
    """SHA-256 over `spine://` + authority + path; query and fragment excluded."""
    parts = parse_name(text)
    return hashlib.sha256((SCHEME + parts["authority"] + parts["path"]).encode("utf-8")).digest()


# ---------------------------------------------------------------------------
# Resource ids
# ---------------------------------------------------------------------------

RESOURCE_ID_DOMAIN = b"spine/resource-id/v1\x00"


def valid_content_type(ct):
    raw = ct.encode("utf-8")
    return (
        1 <= len(raw) <= 256
        and all(0x20 <= b <= 0x7E and not (0x41 <= b <= 0x5A) for b in raw)
        and raw[0] != 0x20
        and raw[-1] != 0x20
    )


def resource_id(content_type, data):
    assert valid_content_type(content_type), content_type
    ct = content_type.encode("utf-8")
    return hashlib.sha256(
        RESOURCE_ID_DOMAIN
        + len(ct).to_bytes(4, "big") + ct
        + len(data).to_bytes(8, "big") + data
    ).digest()


# ---------------------------------------------------------------------------
# Name records
# ---------------------------------------------------------------------------

SIGNING_DOMAIN = b"spine-name-record-v2\x00"

# Record lifetime rules (spec section 3.5). A record is admitted only if it
# verifies, its ttl_secs is within MAX_TTL_SECS, it is a well-formed tombstone if
# it carries the reserved marker, and (when the verifier has a clock) its
# published_at is at most MAX_SKEW_SECS ahead of that clock.
MAX_TTL_SECS = 7 * 24 * 60 * 60
MAX_SKEW_SECS = 300
REVOKED_KEY = "spine.revoked"
U64_MAX = 2 ** 64 - 1


def _str(s):
    raw = s.encode("utf-8")
    return len(raw).to_bytes(4, "big") + raw


def _count(n):
    return n.to_bytes(4, "big")


def signing_bytes(rec):
    """The bytes a record signature covers, from the structured description."""
    out = bytearray(SIGNING_DOMAIN)
    out += _str(rec["name"])
    out += rec["seq"].to_bytes(8, "big")
    out += rec["ttl_secs"].to_bytes(4, "big")
    out += rec["published_at"].to_bytes(8, "big")
    out += _count(len(rec["endpoints"]))
    for e in rec["endpoints"]:
        out += _str(e["transport"]) + _str(e["address"]) + bytes([e["priority"]])
    out += _count(len(rec["capabilities"]))
    for c in rec["capabilities"]:
        out += _str(c)
    if rec["content_hash"] is None:
        out += b"\x00"
    else:
        h = bytes.fromhex(rec["content_hash"])
        assert len(h) == 32
        out += b"\x01" + h
    out += _count(len(rec["links"]))
    for link in rec["links"]:
        out += _str(link["rel"]) + _str(link["target"])
        if link["title"] is None:
            out += b"\x00"
        else:
            out += b"\x01" + _str(link["title"])
        out += _count(len(link["capabilities"]))
        for c in link["capabilities"]:
            out += _str(c)
    meta = sorted(rec["meta"], key=lambda kv: kv[0].encode("utf-8"))
    assert len({k for k, _ in meta}) == len(meta), "duplicate meta key"
    out += _count(len(meta))
    for k, v in meta:
        out += _str(k) + _str(v)
    return bytes(out)


def record_json(rec, signature):
    """The JSON object serde emits for a NameRecord with this signature."""
    links = []
    for link in rec["links"]:
        item = {
            "rel": link["rel"] if link["rel"] in BUILTIN_RELS else {"other": link["rel"]},
            "target": link["target"],
        }
        if link["title"] is not None:
            item["title"] = link["title"]
        if link["capabilities"]:
            item["capabilities"] = list(link["capabilities"])
        links.append(item)
    return {
        "name": rec["name"],
        "seq": rec["seq"],
        "ttl_secs": rec["ttl_secs"],
        "published_at": rec["published_at"],
        "endpoints": [
            {"transport": e["transport"], "address": e["address"], "priority": e["priority"]}
            for e in rec["endpoints"]
        ],
        "capabilities": list(rec["capabilities"]),
        "content_hash": (
            None if rec["content_hash"] is None else list(bytes.fromhex(rec["content_hash"]))
        ),
        "links": links,
        "meta": {k: v for k, v in rec["meta"]},
        "signature": b32_encode(signature),
    }


def signature_verifies(rec, signature):
    """Strict signature check against the did: key in the record's name."""
    parts = parse_name(rec["name"])
    if parts["kind"] != "did":
        return False
    public = bytes.fromhex(parts["detail"]["key"])
    return ed25519_verify(public, signing_bytes(rec), signature, strict=True)


def is_tombstone(rec):
    return any(k == REVOKED_KEY for k, _ in rec["meta"])


def lifetime_error(rec):
    """None if the record obeys the clock-free lifetime rules, else the reason."""
    if rec["ttl_secs"] > MAX_TTL_SECS:
        return "ttl"
    if is_tombstone(rec):
        marker_only = [list(kv) for kv in rec["meta"]] == [[REVOKED_KEY, "1"]]
        empty = (not rec["endpoints"] and not rec["capabilities"]
                 and rec["content_hash"] is None and not rec["links"])
        if not marker_only or not empty or rec["ttl_secs"] != MAX_TTL_SECS:
            return "tombstone"
    return None


def admission_error(rec, signature, now=None):
    """None if the record is admitted, else why not: signature, ttl, tombstone,
    postdated. The signature is checked first; `now` (Unix seconds) enables the
    bound on published_at."""
    if not signature_verifies(rec, signature):
        return "signature"
    reason = lifetime_error(rec)
    if reason is not None:
        return reason
    if now is not None and rec["published_at"] > min(now + MAX_SKEW_SECS, U64_MAX):
        return "postdated"
    return None


# ---------------------------------------------------------------------------
# Vector construction
# ---------------------------------------------------------------------------

SEEDS = [bytes([i]) * 32 for i in (1, 2, 3)]


def key_label(i):
    return "k%d" % i


def make_keys():
    keys = []
    for i, seed in enumerate(SEEDS):
        public = ed25519_public(seed)
        b32 = b32_encode(public)
        # A second spelling of the same key: the last symbol carries four
        # padding bits, set one of them. A canonical decoder must refuse it.
        last = B32_ALPHABET.index(b32[-1])
        noncanonical = b32[:-1] + B32_ALPHABET[last + 1]
        keys.append({
            "label": key_label(i),
            "seed": seed.hex(),
            "public": public.hex(),
            "base32": b32,
            "authority": "did:" + b32,
            "name": SCHEME + "did:" + b32 + "/",
            "noncanonical_base32": noncanonical,
        })
    return keys


def make_names(keys):
    b = [k["base32"] for k in keys]
    blob = b32_encode(hashlib.sha256(b"abc").digest())
    inputs = [
        ("root without a slash", "spine://did:%s" % b[0]),
        ("upper case scheme, tag and key", "SPINE://DID:%s/" % b[0].upper()),
        ("path, query and fragment", "spine://did:%s/tools/search?q=rust&a=1#results" % b[1]),
        ("dot segments and repeated slashes", "spine://did:%s/a//b/./c/x/../d" % b[0]),
        ("trailing slash is kept", "spine://did:%s/a/b/" % b[0]),
        ("cannot climb above the root", "spine://did:%s/../../etc/passwd" % b[0]),
        ("bare and empty-valued query keys", "spine://did:%s/p?b&a=&c=3&&" % b[0]),
        ("duplicate query keys are kept and sorted", "spine://did:%s/p?k=2&k=1" % b[0]),
        ("empty fragment is dropped", "spine://did:%s/x#" % b[0]),
        ("fragment may contain ? and #", "spine://did:%s/x#a?b#c" % b[0]),
        ("only the first = splits a pair", "spine://did:%s/p?a=b=c" % b[0]),
        ("query value with slashes", "spine://did:%s/p?next=/a/b" % b[0]),
        ("non-ASCII path, query and fragment",
         "spine://did:%s/ツール/検索?語=値#節" % b[2]),
        ("surrounding whitespace is trimmed", "  spine://did:%s/x  " % b[0]),
        ("blob authority", "spine://blob:%s" % blob.upper()),
        ("cap authority is lower-cased", "spine://cap:Web.Search"),
        ("host with port is lower-cased", "spine://host:Node.Example.ORG:9440/x"),
        ("host without port", "spine://host:Example.org"),
    ]
    names = []
    for label, text in inputs:
        parts = parse_name(text)
        entry = {
            "label": label,
            "input": text,
            "canonical": format_name(parts),
            "routing_key": routing_key(text).hex(),
            "authority": dict(parts["detail"], kind=parts["kind"]),
            "path": parts["path"],
            "query": [list(p) for p in parts["query"]],
            "fragment": parts["fragment"],
        }
        names.append(entry)
    return names


def make_invalid_names(keys):
    b0 = keys[0]["base32"]
    cases = [
        ("not the spine scheme", "http://example.org/"),
        ("empty after the scheme", "spine://"),
        ("unknown authority kind", "spine://zzz:abc/"),
        ("no authority kind", "spine://justahost/"),
        ("did key too short", "spine://did:%s/" % b32_encode(bytes(16))),
        ("did key too long", "spine://did:%s/" % b32_encode(bytes(33))),
        ("did key with a symbol outside the alphabet", "spine://did:%s1/" % b0[:-1]),
        ("did key with non-zero trailing bits", "spine://did:%s/" % keys[0]["noncanonical_base32"]),
        ("blob key with non-zero trailing bits", "spine://blob:%s/" % keys[1]["noncanonical_base32"]),
        ("empty cap term", "spine://cap:/"),
        ("cap term with whitespace", "spine://cap:a b/"),
        ("empty host", "spine://host:/"),
        ("host port beyond u16", "spine://host:example.org:65536/"),
        ("host with an empty name before the port", "spine://host::9440/"),
    ]
    out = []
    for label, text in cases:
        try:
            parse_name(text)
        except ValueError as e:
            out.append({"label": label, "input": text, "reason": str(e)})
        else:
            raise AssertionError("expected rejection: " + label)
    return out


def make_resource_ids():
    pattern = bytes(range(256))
    items = [
        ("json", "application/json", b'{"a":1}', None),
        ("empty bytes", "text/plain", b"", None),
        ("text", "text/plain", b"hello", None),
        ("type with parameters", "text/plain; charset=utf-8", b"hello", None),
        ("boundary a|bcdef", "a", b"bcdef", "boundary ab|cdef"),
        ("boundary ab|cdef", "ab", b"cdef", "boundary a|bcdef"),
        ("all byte values", "application/octet-stream", pattern, None),
        ("longest type", "a" * 256, b"x", None),
    ]
    out = []
    for label, ct, data, distinct in items:
        entry = {
            "label": label,
            "content_type": ct,
            "bytes": data.hex(),
            "id": resource_id(ct, data).hex(),
            "content_hash": hashlib.sha256(data).hexdigest(),
        }
        if distinct is not None:
            entry["distinct_from"] = distinct
        out.append(entry)
    return out


INVALID_CONTENT_TYPES = [
    ("empty", ""),
    ("upper case", "Text/Plain"),
    ("leading space", " text/plain"),
    ("trailing space", "text/plain "),
    ("control character", "text/pla\tin"),
    ("non-ASCII", "tëxt/plain"),
    ("longer than 256 bytes", "a" * 257),
]


def make_rel_aliases():
    out = []
    for name in BUILTIN_RELS + ["mirrors", "Mirrors", ""]:
        builtin = name in BUILTIN_RELS
        out.append({
            "label": "other(%r)" % name,
            "json": {"other": name},
            "as_str": name,
            "is_other": not builtin,
        })
    for name in BUILTIN_RELS:
        out.append({
            "label": "plain %r" % name,
            "json": name,
            "as_str": name,
            "is_other": False,
        })
    return out


def rec(name, seq, ttl, published, endpoints=(), capabilities=(), content_hash=None,
        links=(), meta=()):
    return {
        "name": canonical_name(name),
        "seq": seq,
        "ttl_secs": ttl,
        "published_at": published,
        "endpoints": [dict(e) for e in endpoints],
        "capabilities": list(capabilities),
        "content_hash": content_hash,
        "links": [dict(l) for l in links],
        "meta": [list(kv) for kv in meta],
    }


def ep(transport, address, priority=0):
    return {"transport": transport, "address": address, "priority": priority}


def link(rel, target, title=None, capabilities=()):
    return {
        "rel": rel,
        "target": canonical_name(target),
        "title": title,
        "capabilities": list(capabilities),
    }


def make_records(keys):
    b = [k["base32"] for k in keys]
    did = lambda i, rest="": "spine://did:%s%s" % (b[i], rest)  # noqa: E731
    page = resource_id("text/html", b"<html></html>").hex()
    specs = []

    specs.append(("minimal", 0, rec(did(0, "/"), 1, 3600, 1700000000)))

    specs.append(("every optional field", 1, rec(
        did(1, "/tools/search?q=rust&a=1#results"), 7, 600, 1700000123,
        endpoints=[ep("tcp", "10.0.0.4:9440"), ep("wss", "wss://agent.example.org/spine", 5)],
        capabilities=["web.search", "web.crawl"],
        content_hash=page,
        links=[
            link("child", did(0, "/docs")),
            link("provides", "spine://cap:web.search/", "Search provider", ["web.search"]),
            link("mirrors", did(2, "/mirror"), "Mirror", ["web.search", "web.crawl"]),
        ],
        # Deliberately not in key order; the signature sorts by key bytes.
        meta=[["zeta", "last"], ["alpha", "first"], ["mid", "middle"],
              ["～", "fullwidth tilde"], ["\U0001F600", "smile"]],
    )))

    specs.append(("every built-in relation", 2, rec(
        did(2, "/"), 2, 60, 1700000200,
        links=[link(r, did(0, "/")) for r in BUILTIN_RELS],
    )))

    specs.append(("multi-byte UTF-8", 0, rec(
        did(0, "/ツール"), 3, 120, 1700000300,
        endpoints=[ep("quic", "例え.example:443", 1)],
        capabilities=["検索"],
        links=[link("alternate", did(2, "/リンク"),
                    "日本語タイトル", ["能力"])],
        meta=[["名前", "値"], ["emoji", "\U0001F680"]],
    )))

    specs.append(("empty strings", 1, rec(
        did(1, "/"), 4, 0, 0,
        endpoints=[ep("", "", 0)],
        capabilities=[""],
        links=[link("", did(0, "/"), "", [""])],
        meta=[["", ""]],
    )))

    specs.append(("numeric extremes", 2, rec(
        did(2, "/"), 2 ** 64 - 1, MAX_TTL_SECS, 2 ** 64 - 1,
        endpoints=[ep("tcp", "h:1", 255)],
        content_hash="ff" * 32,
    )))

    specs.append(("maximum lifetime", 1, rec(
        did(1, "/"), 6, MAX_TTL_SECS, 1700000450,
        endpoints=[ep("tcp", "10.0.0.7:9440")],
    )))

    specs.append(("tombstone", 0, rec(
        did(0, "/"), 2, MAX_TTL_SECS, 1700000500,
        meta=[[REVOKED_KEY, "1"]],
    )))

    specs.append(("case is not normalised", 0, rec(
        did(0, "/Mixed/Case"), 5, 30, 1700000400,
        capabilities=["Web.Search"],
        links=[link("Mirrors", did(1, "/"), None, ["Link.CAP"])],
        meta=[["key", "value"], ["Key", "Value"]],
    )))

    records = []
    for label, signer, r in specs:
        signature = ed25519_sign(SEEDS[signer], signing_bytes(r))
        records.append({
            "label": label,
            "signer": key_label(signer),
            "is_tombstone": is_tombstone(r),
            "record": r,
            "signing_bytes": signing_bytes(r).hex(),
            "signature": signature.hex(),
            "json": record_json(r, signature),
        })
    return records


def clone(r):
    return json.loads(json.dumps(r))


def make_invalid_records(keys, records):
    by_label = {r["label"]: r for r in records}
    full = by_label["every optional field"]
    minimal = by_label["minimal"]
    full_sig = bytes.fromhex(full["signature"])
    min_sig = bytes.fromhex(minimal["signature"])
    out = []

    def add(label, base, mutate, signature=None, reason="signature", signer=None):
        r = clone(base["record"])
        mutate(r)
        if signer is not None:
            # Re-signed after the change, so only the lifetime rules can refuse it.
            sig = ed25519_sign(SEEDS[signer], signing_bytes(r))
        else:
            sig = bytes.fromhex(base["signature"]) if signature is None else signature
        out.append({
            "label": label,
            "base": base["label"],
            "record": r,
            "signature": sig.hex(),
            "json": record_json(r, sig),
            "expected": "reject",
            "reason": reason,
        })

    def set_(key, value):
        def f(r):
            r[key] = value
        return f

    add("seq changed", full, set_("seq", full["record"]["seq"] + 1))
    add("ttl_secs changed", full, set_("ttl_secs", full["record"]["ttl_secs"] + 1))
    add("published_at changed", full, set_("published_at", full["record"]["published_at"] + 1))
    add("name path changed", full, set_("name", canonical_name(
        "spine://did:%s/tools/other?q=rust&a=1#results" % keys[1]["base32"])))
    add("name query changed", full, set_("name", canonical_name(
        "spine://did:%s/tools/search?q=go&a=1#results" % keys[1]["base32"])))
    add("name fragment removed", full, set_("name", canonical_name(
        "spine://did:%s/tools/search?q=rust&a=1" % keys[1]["base32"])))
    add("name key swapped", full, set_("name", canonical_name(
        "spine://did:%s/tools/search?q=rust&a=1#results" % keys[2]["base32"])))

    def endpoint_field(i, field, value):
        def f(r):
            r["endpoints"][i][field] = value
        return f

    add("endpoint transport changed", full, endpoint_field(0, "transport", "udp"))
    add("endpoint address changed", full, endpoint_field(0, "address", "10.0.0.5:9440"))
    add("endpoint priority changed", full, endpoint_field(0, "priority", 1))
    add("endpoint appended", full, lambda r: r["endpoints"].append(ep("tcp", "1.2.3.4:5")))
    add("endpoint removed", full, lambda r: r["endpoints"].pop())
    add("endpoints reordered", full, lambda r: r["endpoints"].reverse())

    add("capability changed", full, lambda r: r["capabilities"].__setitem__(0, "web.fetch"))
    add("capability appended", full, lambda r: r["capabilities"].append("extra"))
    add("capabilities reordered", full, lambda r: r["capabilities"].reverse())

    def flip_hash(r):
        h = bytearray(bytes.fromhex(r["content_hash"]))
        h[0] ^= 1
        r["content_hash"] = bytes(h).hex()

    add("content_hash changed", full, flip_hash)
    add("content_hash removed", full, set_("content_hash", None))
    add("content_hash added", minimal, set_("content_hash", "00" * 32), min_sig)

    def link_field(i, field, value):
        def f(r):
            r["links"][i][field] = value
        return f

    add("link rel changed", full, link_field(0, "rel", "parent"))
    add("link target changed", full, link_field(0, "target", canonical_name(
        "spine://did:%s/other" % keys[0]["base32"])))
    add("link title changed", full, link_field(1, "title", "Other title"))
    add("link title removed", full, link_field(1, "title", None))
    add("link title added", full, link_field(0, "title", "Now titled"))
    add("link capability changed", full, lambda r: r["links"][1]["capabilities"].__setitem__(0, "web.fetch"))
    add("link capability added", full, lambda r: r["links"][0]["capabilities"].append("extra"))
    add("link capability removed", full, lambda r: r["links"][2]["capabilities"].pop())
    add("link removed", full, lambda r: r["links"].pop())
    add("links reordered", full, lambda r: r["links"].reverse())

    add("meta value changed", full, lambda r: r["meta"].__setitem__(0, ["zeta", "changed"]))
    add("meta key renamed", full, lambda r: r["meta"].__setitem__(0, ["zeta2", "last"]))
    add("meta entry added", full, lambda r: r["meta"].append(["extra", "x"]))
    add("meta entry removed", full, lambda r: r["meta"].pop())

    # Signature-level failures; the record is untouched.
    def keep(r):
        pass

    flipped = bytearray(full_sig)
    flipped[0] ^= 1
    add("signature bit flipped in R", full, keep, bytes(flipped))
    flipped = bytearray(full_sig)
    flipped[40] ^= 1
    add("signature bit flipped in S", full, keep, bytes(flipped))
    add("signature truncated to 63 bytes", full, keep, full_sig[:63])
    add("signature extended to 65 bytes", full, keep, full_sig + b"\x00")
    add("signature empty", full, keep, b"")
    add("signature all zero", full, keep, bytes(64))
    s = int.from_bytes(full_sig[32:], "little") + _Q
    add("signature S replaced by S + L", full, keep,
        full_sig[:32] + s.to_bytes(32, "little"))
    other = ed25519_sign(SEEDS[2], bytes.fromhex(full["signing_bytes"]))
    add("valid signature by a different key", full, keep, other)
    add("signature of another record", full, keep, min_sig)

    # A small-order ("weak") public key makes the non-strict equation accept
    # R = identity, S = 0 for any message. verify_strict must refuse it.
    weak_key = b"\x01" + bytes(31)
    weak = clone(minimal["record"])
    weak["name"] = canonical_name("spine://did:%s/" % b32_encode(weak_key))
    weak_sig = weak_key + bytes(32)
    assert ed25519_verify(weak_key, signing_bytes(weak), weak_sig, strict=False)
    assert not ed25519_verify(weak_key, signing_bytes(weak), weak_sig, strict=True)
    out.append({
        "label": "identity public key with R = identity, S = 0 (accepted by non-strict verification)",
        "base": "minimal",
        "record": weak,
        "signature": weak_sig.hex(),
        "json": record_json(weak, weak_sig),
        "expected": "reject",
        "reason": "signature",
    })

    # Lifetime rules. Each record is correctly signed by the name's key, so the
    # signature is valid and only the lifetime rule refuses it.
    tomb = by_label["tombstone"]
    add("ttl_secs one over the maximum", minimal,
        set_("ttl_secs", MAX_TTL_SECS + 1), reason="ttl", signer=0)
    add("ttl_secs at the u32 maximum", minimal,
        set_("ttl_secs", 2 ** 32 - 1), reason="ttl", signer=0)
    add("tombstone with a shorter ttl_secs", tomb,
        set_("ttl_secs", MAX_TTL_SECS - 1), reason="tombstone", signer=0)
    add("tombstone with an endpoint", tomb,
        lambda r: r["endpoints"].append(ep("tcp", "1.2.3.4:5")), reason="tombstone", signer=0)
    add("tombstone with a capability", tomb,
        lambda r: r["capabilities"].append("web.search"), reason="tombstone", signer=0)
    add("tombstone with a content hash", tomb,
        set_("content_hash", "00" * 32), reason="tombstone", signer=0)
    add("tombstone with extra metadata", tomb,
        lambda r: r["meta"].append(["title", "x"]), reason="tombstone", signer=0)
    add("revoked marker with another value", tomb,
        lambda r: r["meta"].__setitem__(0, [REVOKED_KEY, "0"]), reason="tombstone", signer=0)
    add("revoked marker on an ordinary record", minimal,
        lambda r: r["meta"].append([REVOKED_KEY, "1"]), reason="tombstone", signer=0)
    return out


def make_admission(records, invalid_records):
    """Cases that depend on the verifier's clock or combine the rules above.
    'record' names an entry of `records` or `invalid_records`; 'now' is the
    verifier's clock in Unix seconds."""
    by_label = {r["label"]: r for r in records}
    minimal = by_label["minimal"]["record"]["published_at"]
    out = []

    def add(label, record, now, expected, reason=None):
        out.append({"label": label, "record": record, "now": now,
                    "expected": expected, "reason": reason})

    add("published exactly the skew ahead", "minimal", minimal - MAX_SKEW_SECS, "admit")
    add("published one second past the skew", "minimal",
        minimal - MAX_SKEW_SECS - 1, "reject", "postdated")
    add("published in the past", "minimal", minimal + 10 ** 9, "admit")
    add("published at the u64 maximum, clock at the u64 maximum", "numeric extremes",
        U64_MAX, "admit")
    add("published at the u64 maximum, clock now", "numeric extremes",
        minimal, "reject", "postdated")
    add("maximum lifetime", "maximum lifetime", 1700000450, "admit")
    add("tombstone", "tombstone", 1700000500, "admit")
    add("tombstone published past the skew", "tombstone",
        1700000500 - MAX_SKEW_SECS - 1, "reject", "postdated")
    add("ttl_secs one over the maximum", "ttl_secs one over the maximum",
        minimal, "reject", "ttl")
    add("tombstone with a shorter ttl_secs", "tombstone with a shorter ttl_secs",
        1700000500, "reject", "tombstone")
    return out


CONVENTIONS = [
    "spine-name v1 conformance vectors. Generated by scripts/names_v1.py from docs/spec/02-names.md; do not edit by hand.",
    "Regenerate: python scripts/names_v1.py generate > docs/spec/02-names.vectors.json. Check: python scripts/names_v1.py check docs/spec/02-names.vectors.json.",
    "All 'hex' strings are lowercase, no spaces or prefix. 'seed' and 'public' are Ed25519 key bytes (32 bytes).",
    "keys: base32 is the unpadded lowercase RFC 4648 spelling of the public key; 'noncanonical_base32' is a second spelling with a non-zero padding bit, which a decoder must refuse.",
    "names: parsing 'input' must give 'canonical' (and parsing 'canonical' must give itself). routing_key is SHA-256 of 'spine://' + authority + path; query and fragment are excluded.",
    "invalid_names: parsing must fail. 'reason' is informative.",
    "resource_ids: id = SHA-256('spine/resource-id/v1' 00 || u32be len(type) || type || u64be len(bytes) || bytes). content_hash is the plain SHA-256 of the bytes, shown for contrast.",
    "rel_aliases: a Rel read from 'json' must have the given as_str, and be an Other variant only if is_other. Other(x) with x the exact name of a built-in relation reads as that built-in.",
    "records: 'record' is the structured description (meta is a list of [key, value] pairs in insertion order; content_hash is hex or null; link rel is its wire string). 'signing_bytes' and 'signature' are hex. 'json' is the object serde_json emits for the NameRecord (signature as unpadded base32).",
    "invalid_records: each 'json' must deserialize (it is well formed) and then be refused. 'reason' says why: 'signature' (verification fails, under the strict profile: RFC 8032 plus rejecting a small-order public key or R), 'ttl' (ttl_secs above the maximum record lifetime, 604800), or 'tombstone' (carries the spine.revoked marker but is not a well-formed tombstone). The 'ttl' and 'tombstone' records are correctly signed.",
    "lifetime: a record is admitted only if its signature verifies, ttl_secs <= 604800, and, if it carries the 'spine.revoked' meta key, it is a well-formed tombstone: meta is exactly {spine.revoked: '1'}, no endpoints, capabilities, content hash or links, and ttl_secs == 604800. A signer MUST refuse to sign anything else. 'records[].is_tombstone' says whether the marker is present.",
    "admission: 'record' names an entry of records or invalid_records; 'now' is the verifier's clock (Unix seconds). 'admit' means the lifetime rules hold and published_at <= now + 300 (saturating at the u64 maximum); 'reject' gives the reason ('postdated', 'ttl', 'tombstone').",
]


def generate():
    rfc8032_self_check()
    keys = make_keys()
    records = make_records(keys)
    invalid_records = make_invalid_records(keys, records)
    doc = {
        "conventions": CONVENTIONS,
        "keys": keys,
        "names": make_names(keys),
        "invalid_names": make_invalid_names(keys),
        "resource_ids": make_resource_ids(),
        "invalid_content_types": [
            {"label": label, "content_type": ct} for label, ct in INVALID_CONTENT_TYPES
        ],
        "rel_aliases": make_rel_aliases(),
        "records": records,
        "invalid_records": invalid_records,
        "admission": make_admission(records, invalid_records),
    }
    return json.dumps(doc, indent=2, sort_keys=True, ensure_ascii=True) + "\n"


# ---------------------------------------------------------------------------
# check
# ---------------------------------------------------------------------------

class Failure(Exception):
    pass


def expect(cond, message):
    if not cond:
        raise Failure(message)


def check_doc(doc):
    n = rfc8032_self_check()
    counts = {}
    keys = {k["label"]: k for k in doc["keys"]}
    for k in doc["keys"]:
        w = "keys/" + k["label"]
        seed = bytes.fromhex(k["seed"])
        public = ed25519_public(seed)
        expect(k["public"] == public.hex(), w + ": public key")
        expect(k["base32"] == b32_encode(public), w + ": base32")
        expect(b32_decode(k["base32"]) == public, w + ": base32 round trip")
        expect(b32_decode(k["base32"].upper()) == public, w + ": upper-case spelling")
        expect(k["authority"] == "did:" + k["base32"], w + ": authority")
        expect(k["name"] == SCHEME + k["authority"] + "/", w + ": name")
        expect(b32_decode(k["noncanonical_base32"]) is None, w + ": non-canonical spelling must be refused")
        expect(k["noncanonical_base32"] != k["base32"], w + ": spellings differ")
    counts["keys"] = len(doc["keys"])

    for v in doc["names"]:
        w = "names/" + v["label"]
        parts = parse_name(v["input"])
        expect(format_name(parts) == v["canonical"], w + ": canonical form")
        expect(canonical_name(v["canonical"]) == v["canonical"], w + ": canonical is a fixed point")
        expect(routing_key(v["input"]).hex() == v["routing_key"], w + ": routing key")
        expect(dict(parts["detail"], kind=parts["kind"]) == v["authority"], w + ": authority")
        expect(parts["path"] == v["path"], w + ": path")
        expect([list(p) for p in parts["query"]] == v["query"], w + ": query")
        expect(parts["fragment"] == v["fragment"], w + ": fragment")
    counts["names"] = len(doc["names"])

    for v in doc["invalid_names"]:
        w = "invalid_names/" + v["label"]
        try:
            parse_name(v["input"])
        except ValueError as e:
            expect(str(e) == v["reason"], w + ": reason %s != %s" % (e, v["reason"]))
        else:
            raise Failure(w + ": parsed but must be refused")
    counts["invalid_names"] = len(doc["invalid_names"])

    by_label = {}
    for v in doc["resource_ids"]:
        w = "resource_ids/" + v["label"]
        data = bytes.fromhex(v["bytes"])
        expect(resource_id(v["content_type"], data).hex() == v["id"], w + ": id")
        expect(hashlib.sha256(data).hexdigest() == v["content_hash"], w + ": content_hash")
        expect(v["id"] != v["content_hash"], w + ": id differs from the plain hash")
        by_label[v["label"]] = v
    for v in doc["resource_ids"]:
        if "distinct_from" in v:
            expect(by_label[v["distinct_from"]]["id"] != v["id"], "resource_ids/" + v["label"] + ": distinct")
    counts["resource_ids"] = len(doc["resource_ids"])

    for v in doc["invalid_content_types"]:
        expect(not valid_content_type(v["content_type"]), "invalid_content_types/" + v["label"])
    counts["invalid_content_types"] = len(doc["invalid_content_types"])

    for v in doc["rel_aliases"]:
        w = "rel_aliases/" + v["label"]
        j = v["json"]
        if isinstance(j, dict):
            name = j["other"]
            expect(v["as_str"] == name, w + ": as_str")
            expect(v["is_other"] == (name not in BUILTIN_RELS), w + ": is_other")
        else:
            expect(v["as_str"] == j and j in BUILTIN_RELS and not v["is_other"], w)
    counts["rel_aliases"] = len(doc["rel_aliases"])

    labels = set()
    for v in doc["records"]:
        w = "records/" + v["label"]
        labels.add(v["label"])
        r = v["record"]
        sb = signing_bytes(r)
        expect(sb.hex() == v["signing_bytes"], w + ": signing_bytes")
        expect(canonical_name(r["name"]) == r["name"], w + ": name is canonical")
        for link_ in r["links"]:
            expect(canonical_name(link_["target"]) == link_["target"], w + ": link target is canonical")
        sig = bytes.fromhex(v["signature"])
        expect(admission_error(r, sig) is None, w + ": record is not admitted")
        expect(v["is_tombstone"] == is_tombstone(r), w + ": is_tombstone")
        seed = bytes.fromhex(keys[v["signer"]]["seed"])
        expect(ed25519_sign(seed, sb) == sig, w + ": re-signing differs")
        expect(parse_name(r["name"])["detail"]["key"] == keys[v["signer"]]["public"], w + ": signer owns the name")
        expect(record_json(r, sig) == v["json"], w + ": json form")
    counts["records"] = len(doc["records"])

    for v in doc["invalid_records"]:
        w = "invalid_records/" + v["label"]
        expect(v["expected"] == "reject", w + ": expected")
        expect(v["base"] in labels, w + ": base record exists")
        sig = bytes.fromhex(v["signature"])
        expect(v["reason"] in ("signature", "ttl", "tombstone"), w + ": reason")
        expect(admission_error(v["record"], sig) == v["reason"],
               w + ": must be refused for reason %s, got %s"
               % (v["reason"], admission_error(v["record"], sig)))
        expect(record_json(v["record"], sig) == v["json"], w + ": json form")
    counts["invalid_records"] = len(doc["invalid_records"])

    known = {v["label"]: v for v in doc["records"] + doc["invalid_records"]}
    for v in doc["admission"]:
        w = "admission/" + v["label"]
        expect(v["record"] in known, w + ": record exists")
        target = known[v["record"]]
        got = admission_error(target["record"], bytes.fromhex(target["signature"]), v["now"])
        if v["expected"] == "admit":
            expect(got is None and v["reason"] is None, w + ": must be admitted, got %s" % got)
        else:
            expect(v["expected"] == "reject" and got == v["reason"],
                   w + ": must be refused for %s, got %s" % (v["reason"], got))
    counts["admission"] = len(doc["admission"])
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
        except (KeyError, ValueError, TypeError, AssertionError) as e:
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
