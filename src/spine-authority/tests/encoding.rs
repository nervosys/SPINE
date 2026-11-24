//! The canonical encoding: layout, round trips, tampering with every field,
//! non-canonical and unknown-field input, and the size bounds.

mod common;

use common::*;
use sha2::{Digest, Sha256};
use spine_authority::{
    Action, Broadening, Caveat, Chain, DecodeError, Effect, Grant, GrantBody, GrantSpec,
    IssueError, Limits, Malformation, ResourcePattern, GRANT_DOMAIN, GRANT_ID_DOMAIN,
    INVOCATION_DOMAIN, MAX_CHAIN_DEPTH, MAX_GRANT_BYTES,
};
use spine_cbor::CborError;

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || haystack.len() < needle.len() {
        return out;
    }
    for start in 0..=(haystack.len() - needle.len()) {
        if &haystack[start..start + needle.len()] == needle {
            out.push(start);
        }
    }
    out
}

/// Replaces the one occurrence of `old` in `bytes` with `new`.
fn replace_once(bytes: &[u8], old: &[u8], new: &[u8]) -> Vec<u8> {
    let found = find_all(bytes, old);
    assert_eq!(found.len(), 1, "pattern must occur exactly once");
    let at = found[0];
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(new);
    out.extend_from_slice(&bytes[at + old.len()..]);
    out
}

/// The last grant of the standard chain: nonce 3, limits all set.
fn leaf() -> Grant {
    World::new().grants().pop().unwrap()
}

// ---------------------------------------------------------------------------
// layout
// ---------------------------------------------------------------------------

#[test]
fn a_grant_is_a_two_entry_map_of_sig_and_body() {
    let bytes = leaf().encode().unwrap();
    // a2: map of 2. "sig" sorts before "body" (its encoded key is shorter).
    assert_eq!(&bytes[..6], &[0xa2, 0x63, b's', b'i', b'g', 0x58]);
    assert_eq!(bytes[6], 0x40, "a byte string of 64");
    let body_key = [0x64u8, b'b', b'o', b'd', b'y'];
    assert_eq!(&bytes[7 + 64..7 + 64 + 5], &body_key);
}

#[test]
fn signed_bytes_are_the_domain_then_the_cbor_of_the_body() {
    let g = leaf();
    let signed = g.body.signing_bytes().unwrap();
    assert_eq!(GRANT_DOMAIN, b"spine-authority-grant-v1\0");
    assert!(signed.starts_with(GRANT_DOMAIN));
    assert_eq!(
        &signed[GRANT_DOMAIN.len()..],
        spine_cbor::to_vec(&g.body).unwrap().as_slice()
    );
    // The signature does not appear in them.
    assert!(find_all(&signed, &g.signature).is_empty());
}

#[test]
fn the_id_is_sha256_over_its_domain_and_the_whole_encoding() {
    let g = leaf();
    let encoded = g.encode().unwrap();
    assert_eq!(GRANT_ID_DOMAIN, b"spine-authority-grant-id-v1\0");
    let mut h = Sha256::new();
    h.update(GRANT_ID_DOMAIN);
    h.update(encoded.as_slice());
    let expected: [u8; 32] = h.finalize().into();
    assert_eq!(g.id().unwrap(), expected);
    // Domain separation: not the plain hash.
    let plain: [u8; 32] = Sha256::digest(encoded.as_slice()).into();
    assert_ne!(g.id().unwrap(), plain);
    // Three different domains.
    assert_ne!(GRANT_DOMAIN, GRANT_ID_DOMAIN);
    assert_ne!(GRANT_DOMAIN, INVOCATION_DOMAIN);
}

#[test]
fn keys_hashes_nonces_and_signatures_are_byte_strings() {
    let bytes = leaf().encode().unwrap();
    // Every 32-byte key and the 16-byte nonce use the byte string heads
    // 58 20 and 50; an array of integers would use 98 20 and 90.
    let w = World::new();
    let iss = public(&w.keys[2]);
    let mut as_bstr = vec![0x58, 0x20];
    as_bstr.extend_from_slice(&iss);
    assert_eq!(find_all(&bytes, &as_bstr).len(), 1);
    assert!(find_all(&bytes, &[0x98, 0x20]).is_empty());
    let mut nonce = vec![0x50];
    nonce.extend_from_slice(&[3u8; 16]);
    assert_eq!(find_all(&bytes, &nonce).len(), 1);
}

// ---------------------------------------------------------------------------
// round trips
// ---------------------------------------------------------------------------

#[test]
fn grants_chains_and_invocations_round_trip() {
    let w = World::new();
    let grants = w.grants();
    for g in &grants {
        let bytes = g.encode().unwrap();
        let back = Grant::decode(&bytes).unwrap();
        assert_eq!(&back, g);
        assert_eq!(back.encode().unwrap(), bytes);
        assert_eq!(back.id().unwrap(), g.id().unwrap());
        assert!(back.verify_signature());
    }
    let chain = Chain::new(grants.clone());
    let bytes = chain.encode().unwrap();
    assert_eq!(Chain::decode(&bytes).unwrap(), chain);

    let inv = w.invocation(&grants);
    let bytes = inv.encode().unwrap();
    let back = spine_authority::Invocation::decode(&bytes).unwrap();
    assert_eq!(back, inv);
    assert!(back.verify_signature());
}

#[test]
fn the_root_grant_encodes_a_null_parent() {
    let g = World::new().grants().remove(0);
    assert!(g.body.parent.is_none());
    let bytes = g.encode().unwrap();
    let key = [0x66u8, b'p', b'a', b'r', b'e', b'n', b't', 0xf6];
    assert_eq!(find_all(&bytes, &key).len(), 1);
    assert_eq!(Grant::decode(&bytes).unwrap(), g);
}

// ---------------------------------------------------------------------------
// tampering
// ---------------------------------------------------------------------------

fn tamper(label: &str, g: &mut Grant) {
    let b = &mut g.body;
    match label {
        "version" => b.version += 1,
        "iss" => b.iss[0] ^= 1,
        "aud" => b.aud[0] ^= 1,
        "parent" => b.parent = Some([7u8; 32]),
        "role" => b.role = "service".to_string(),
        "res owner" => b.res[0].owner[0] ^= 1,
        "res path" => b.res[0].path.push("x".to_string()),
        "res prefix" => b.res[0].prefix = !b.res[0].prefix,
        "res extra" => b.res.push(b.res[0].clone()),
        "act" => b.act.push(Action::Admin),
        "effect" => b.effect = Effect::Read,
        "delegable" => b.delegable += 1,
        "nbf" => b.nbf += 1,
        "exp" => b.exp += 1,
        "limits calls" => b.limits.calls = Some(11),
        "limits cost" => b.limits.cost = None,
        "limits bytes" => b.limits.bytes = Some(1),
        "limits rate" => b.limits.rate = Some(99),
        "caveats" => b.caveats.push(caveat("c")),
        "nonce" => b.nonce[15] ^= 1,
        "signature first" => g.signature[0] ^= 1,
        "signature last" => g.signature[63] ^= 0x80,
        _ => panic!("unknown tamper {label}"),
    }
}

#[test]
fn changing_any_field_breaks_the_signature_and_changes_the_id() {
    let labels = [
        "version",
        "iss",
        "aud",
        "parent",
        "role",
        "res owner",
        "res path",
        "res prefix",
        "res extra",
        "act",
        "effect",
        "delegable",
        "nbf",
        "exp",
        "limits calls",
        "limits cost",
        "limits bytes",
        "limits rate",
        "caveats",
        "nonce",
        "signature first",
        "signature last",
    ];
    let original = leaf();
    assert!(original.verify_signature());
    for label in labels {
        let mut g = original.clone();
        tamper(label, &mut g);
        assert_ne!(g, original, "{label} changed nothing");
        assert!(!g.verify_signature(), "{label}: signature still verifies");
        assert_ne!(g.id().unwrap(), original.id().unwrap(), "{label}: same id");
    }
}

#[test]
fn flipping_any_bit_of_the_encoding_is_caught() {
    let original = leaf();
    let bytes = original.encode().unwrap();
    let mut accepted_but_invalid = 0usize;
    let mut rejected = 0usize;
    for position in 0..bytes.len() {
        let mut damaged = bytes.clone();
        damaged[position] ^= 0x01;
        match Grant::decode(&damaged) {
            Err(_) => rejected += 1,
            Ok(grant) => {
                // Whatever decodes is a different value that no longer verifies.
                assert_ne!(grant, original, "byte {position}");
                assert!(!grant.verify_signature(), "byte {position} verifies");
                accepted_but_invalid += 1;
            }
        }
    }
    assert!(rejected > 0 && accepted_but_invalid > 0);
}

// ---------------------------------------------------------------------------
// non-canonical and unknown input
// ---------------------------------------------------------------------------

#[test]
fn non_deterministic_cbor_is_refused() {
    let bytes = leaf().encode().unwrap();

    // `v` written in two bytes where one is the shortest form.
    let wide = replace_once(&bytes, &[0x61, b'v', 0x01], &[0x61, b'v', 0x18, 0x01]);
    assert_eq!(
        Grant::decode(&wide),
        Err(DecodeError::Cbor(CborError::NonShortestInteger))
    );

    // An indefinite-length map.
    let mut indefinite = vec![0xbf];
    indefinite.extend_from_slice(&bytes[1..]);
    indefinite.push(0xff);
    assert_eq!(
        Grant::decode(&indefinite),
        Err(DecodeError::Cbor(CborError::IndefiniteLength))
    );

    // Bytes after the item.
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(
        Grant::decode(&trailing),
        Err(DecodeError::Cbor(CborError::TrailingBytes))
    );

    // The two top-level entries in the wrong order.
    let mut swapped = vec![0xa2];
    swapped.extend_from_slice(&bytes[1 + 70..]);
    swapped.extend_from_slice(&bytes[1..1 + 70]);
    assert_eq!(
        Grant::decode(&swapped),
        Err(DecodeError::Cbor(CborError::UnsortedOrDuplicateKey))
    );
}

#[test]
fn deterministic_cbor_that_is_not_the_canonical_form_is_refused() {
    // A body whose `limits` map leaves out the four null entries. It is valid
    // deterministic CBOR and serde fills the missing options in, so only the
    // re-encoding comparison catches it.
    let mut w = World::new();
    w.bodies[2].limits = Limits::default();
    let g = w.grants().pop().unwrap();
    let bytes = g.encode().unwrap();
    let all_null: Vec<u8> = {
        let mut v = vec![0xa4];
        v.extend_from_slice(&[0x64, b'c', b'o', b's', b't', 0xf6]);
        v.extend_from_slice(&[0x64, b'r', b'a', b't', b'e', 0xf6]);
        v.extend_from_slice(&[0x65, b'b', b'y', b't', b'e', b's', 0xf6]);
        v.extend_from_slice(&[0x65, b'c', b'a', b'l', b'l', b's', 0xf6]);
        v
    };
    assert_eq!(Grant::decode(&bytes).unwrap(), g);
    let short = replace_once(&bytes, &all_null, &[0xa0]);
    spine_cbor::check_deterministic(&short).expect("still deterministic CBOR");
    assert_eq!(Grant::decode(&short), Err(DecodeError::NonCanonical));
}

#[test]
fn unknown_fields_are_refused_at_every_level() {
    let bytes = leaf().encode().unwrap();

    // Top level: "zzz" sorts between "sig" and "body".
    let mut top = vec![0xa3];
    top.extend_from_slice(&bytes[1..1 + 70]);
    top.extend_from_slice(&[0x63, b'z', b'z', b'z', 0x00]);
    top.extend_from_slice(&bytes[1 + 70..]);
    spine_cbor::check_deterministic(&top).expect("deterministic");
    assert!(matches!(
        Grant::decode(&top),
        Err(DecodeError::Cbor(CborError::Decode(_)))
    ));

    // Inside `limits`: key "x" sorts before the other four.
    let cost = [0x64u8, b'c', b'o', b's', b't'];
    let at = find_all(&bytes, &cost);
    assert_eq!(at.len(), 1);
    // The limits map head `a4` sits immediately before its first key.
    assert_eq!(bytes[at[0] - 1], 0xa4);
    let mut inner = bytes[..at[0] - 1].to_vec();
    inner.extend_from_slice(&[0xa5, 0x61, b'x', 0x00]);
    inner.extend_from_slice(&bytes[at[0]..]);
    spine_cbor::check_deterministic(&inner).expect("deterministic");
    assert!(matches!(
        Grant::decode(&inner),
        Err(DecodeError::Cbor(CborError::Decode(_)))
    ));
}

#[test]
fn a_number_where_bytes_belong_and_a_wrong_length_are_refused() {
    let g = leaf();
    assert_eq!(g.body.nonce, [3u8; 16]);
    let bytes = g.encode().unwrap();
    let mut as_bytes = vec![0x50];
    as_bytes.extend_from_slice(&[3u8; 16]);

    // The nonce as an array of sixteen integers.
    let mut as_array = vec![0x90];
    as_array.extend_from_slice(&[3u8; 16]);
    let array_form = replace_once(&bytes, &as_bytes, &as_array);
    assert!(matches!(
        Grant::decode(&array_form),
        Err(DecodeError::Cbor(CborError::Decode(_)))
    ));

    // A 15-byte nonce.
    let mut short = vec![0x4f];
    short.extend_from_slice(&[3u8; 15]);
    let short_form = replace_once(&bytes, &as_bytes, &short);
    assert!(matches!(
        Grant::decode(&short_form),
        Err(DecodeError::Cbor(CborError::Decode(_)))
    ));

    // A text string where the version number belongs.
    let text_version = replace_once(&bytes, &[0x61, b'v', 0x01], &[0x61, b'v', 0x61, b'1']);
    assert!(matches!(
        Grant::decode(&text_version),
        Err(DecodeError::Cbor(CborError::Decode(_)))
    ));

    // An action the format does not have.
    let bogus = replace_once(
        &bytes,
        &[0x66, b'i', b'n', b'v', b'o', b'k', b'e'],
        &[0x66, b'i', b'n', b'v', b'o', b'k', b'x'],
    );
    assert!(matches!(
        Grant::decode(&bogus),
        Err(DecodeError::Cbor(CborError::Decode(_)))
    ));
}

// ---------------------------------------------------------------------------
// bounds
// ---------------------------------------------------------------------------

fn encoded_with(edit: impl FnOnce(&mut GrantBody)) -> Vec<u8> {
    let mut w = World::new();
    edit(&mut w.bodies[0]);
    w.grants().remove(0).encode().unwrap()
}

#[test]
fn decoding_enforces_the_size_bounds() {
    let w = World::new();
    let owner = &w.keys[0];

    // Longer than a whole grant may be.
    let huge = vec![0u8; MAX_GRANT_BYTES + 1];
    assert_eq!(
        Grant::decode(&huge),
        Err(DecodeError::TooLarge(MAX_GRANT_BYTES))
    );

    let too_many = encoded_with(|b| {
        b.res = (0..17).map(|i| exact(owner, &format!("/a{i}"))).collect();
    });
    assert_eq!(
        Grant::decode(&too_many),
        Err(DecodeError::Bounds("too many resource patterns"))
    );

    // Sixteen are fine.
    let sixteen = encoded_with(|b| {
        b.res = (0..16).map(|i| exact(owner, &format!("/a{i}"))).collect();
    });
    assert!(Grant::decode(&sixteen).is_ok());

    let long_segment = encoded_with(|b| {
        b.res = vec![ResourcePattern {
            owner: public(owner),
            path: vec!["s".repeat(129)],
            prefix: true,
        }];
    });
    assert_eq!(
        Grant::decode(&long_segment),
        Err(DecodeError::Bounds("path segment too long"))
    );

    let deep = encoded_with(|b| {
        b.res = vec![ResourcePattern {
            owner: public(owner),
            path: (0..17).map(|i| format!("d{i}")).collect(),
            prefix: true,
        }];
    });
    assert_eq!(
        Grant::decode(&deep),
        Err(DecodeError::Bounds("too many path segments"))
    );

    let many_caveats = encoded_with(|b| {
        b.caveats = (0..9).map(|i| caveat(&format!("c{i}"))).collect();
    });
    assert_eq!(
        Grant::decode(&many_caveats),
        Err(DecodeError::Bounds("too many caveats"))
    );

    let long_arg = encoded_with(|b| {
        b.caveats = vec![Caveat {
            name: "c".to_string(),
            arg: vec![0u8; 257],
        }];
    });
    assert_eq!(
        Grant::decode(&long_arg),
        Err(DecodeError::Bounds("caveat argument too long"))
    );

    let long_name = encoded_with(|b| {
        b.caveats = vec![Caveat {
            name: "n".repeat(65),
            arg: Vec::new(),
        }];
    });
    assert_eq!(
        Grant::decode(&long_name),
        Err(DecodeError::Bounds("caveat name too long"))
    );

    let long_role = encoded_with(|b| b.role = "r".repeat(17));
    assert_eq!(
        Grant::decode(&long_role),
        Err(DecodeError::Bounds("role label too long"))
    );

    let many_actions = encoded_with(|b| {
        b.act = vec![
            Action::Read,
            Action::Invoke,
            Action::Publish,
            Action::Admin,
            Action::Admin,
        ]
    });
    assert_eq!(
        Grant::decode(&many_actions),
        Err(DecodeError::Bounds("too many actions"))
    );
}

#[test]
fn chains_are_bounded_in_depth_and_in_size() {
    let w = World::new();
    let grants = w.grants();

    assert_eq!(
        Chain::decode(&spine_cbor::to_vec(&Vec::<Grant>::new()).unwrap()),
        Err(DecodeError::Bounds("empty chain"))
    );

    let nine: Vec<Grant> = (0..=MAX_CHAIN_DEPTH)
        .map(|i| grants[i % 3].clone())
        .collect();
    let bytes = spine_cbor::to_vec(&nine).unwrap();
    assert_eq!(
        Chain::decode(&bytes),
        Err(DecodeError::Bounds("chain too long"))
    );

    let eight: Vec<Grant> = (0..MAX_CHAIN_DEPTH)
        .map(|i| grants[i % 3].clone())
        .collect();
    let bytes = spine_cbor::to_vec(&eight).unwrap();
    assert_eq!(Chain::decode(&bytes).unwrap().len(), MAX_CHAIN_DEPTH);

    let huge = vec![0u8; spine_authority::MAX_CHAIN_BYTES + 1];
    assert_eq!(
        Chain::decode(&huge),
        Err(DecodeError::TooLarge(spine_authority::MAX_CHAIN_BYTES))
    );

    let mut trailing = Chain::new(grants).encode().unwrap();
    trailing.push(0);
    assert!(matches!(
        Chain::decode(&trailing),
        Err(DecodeError::Cbor(CborError::TrailingBytes))
    ));
}

#[test]
fn invocations_are_bounded_and_canonical() {
    let mut w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let mut bytes = inv.encode().unwrap();
    bytes.push(0);
    assert!(matches!(
        spine_authority::Invocation::decode(&bytes),
        Err(DecodeError::Cbor(CborError::TrailingBytes))
    ));
    assert!(matches!(
        spine_authority::Invocation::decode(&vec![0u8; 5000]),
        Err(DecodeError::TooLarge(_))
    ));

    w.inv.res = (0..17)
        .map(|i| name(&w.keys[0], &format!("/data/r{i}")))
        .collect();
    let big = w.invocation(&grants).encode().unwrap();
    assert_eq!(
        spine_authority::Invocation::decode(&big),
        Err(DecodeError::Bounds("too many resources"))
    );
}

// ---------------------------------------------------------------------------
// issuing
// ---------------------------------------------------------------------------

fn spec_from(b: &GrantBody, aud: &ed25519_dalek::SigningKey) -> GrantSpec {
    GrantSpec {
        aud: public(aud),
        role: b.role.clone(),
        res: b.res.clone(),
        act: b.act.clone(),
        effect: b.effect,
        delegable: b.delegable,
        nbf: b.nbf,
        exp: b.exp,
        limits: b.limits,
        caveats: b.caveats.clone(),
        nonce: b.nonce,
    }
}

#[test]
fn issue_root_refuses_what_would_not_verify() {
    let w = World::new();
    let k = &w.keys;
    let good = spec_from(&w.bodies[0], &k[1]);
    assert!(Grant::issue_root(&k[0], good.clone()).is_ok());

    // Not the owner of the resource.
    assert_eq!(
        Grant::issue_root(&k[1], good.clone()).unwrap_err(),
        IssueError::NotOwner
    );

    let mut spec = good.clone();
    spec.caveats = vec![caveat("anything")];
    assert_eq!(
        Grant::issue_root(&k[0], spec).unwrap_err(),
        IssueError::Malformed(Malformation::UnknownCaveat)
    );

    let mut spec = good.clone();
    spec.exp = spec.nbf;
    assert_eq!(
        Grant::issue_root(&k[0], spec).unwrap_err(),
        IssueError::Malformed(Malformation::BadWindow)
    );

    let mut spec = good.clone();
    spec.act.clear();
    assert_eq!(
        Grant::issue_root(&k[0], spec).unwrap_err(),
        IssueError::Malformed(Malformation::EmptyActions)
    );

    let mut spec = good.clone();
    spec.role = "boss".to_string();
    assert_eq!(
        Grant::issue_root(&k[0], spec).unwrap_err(),
        IssueError::Malformed(Malformation::UnknownRole)
    );

    let mut spec = good.clone();
    spec.res = (0..17).map(|i| exact(&k[0], &format!("/x{i}"))).collect();
    assert!(matches!(
        Grant::issue_root(&k[0], spec).unwrap_err(),
        IssueError::Bounds(_)
    ));

    // Actions are sorted and de-duplicated for the caller.
    let mut spec = good;
    spec.act = vec![
        Action::Publish,
        Action::Read,
        Action::Publish,
        Action::Invoke,
    ];
    let g = Grant::issue_root(&k[0], spec).unwrap();
    assert_eq!(
        g.body.act,
        vec![Action::Read, Action::Invoke, Action::Publish]
    );
}

#[test]
fn delegate_refuses_to_sign_a_broader_grant() {
    let w = World::new();
    let k = &w.keys;
    let root = Grant::issue_root(&k[0], spec_from(&w.bodies[0], &k[1])).unwrap();
    let good = spec_from(&w.bodies[1], &k[2]);
    assert!(Grant::delegate(&k[1], &root, good.clone()).is_ok());

    // Not the parent's audience.
    assert_eq!(
        Grant::delegate(&k[2], &root, good.clone()).unwrap_err(),
        IssueError::Broadens(Broadening::Audience)
    );

    let mut cases: Vec<(&str, Broadening, GrantSpec)> = Vec::new();
    let mut s = good.clone();
    s.res = vec![prefix(&k[0], "/")];
    cases.push(("resource", Broadening::Resource, s));
    let mut s = good.clone();
    s.act = vec![Action::Admin];
    cases.push(("action", Broadening::Action, s));
    let mut s = good.clone();
    s.delegable = 3;
    cases.push(("delegable", Broadening::Delegable, s));
    let mut s = good.clone();
    s.nbf = 900;
    cases.push(("nbf", Broadening::NotBefore, s));
    let mut s = good.clone();
    s.exp = 10_001;
    cases.push(("exp", Broadening::Expiry, s));
    let mut s = good.clone();
    s.limits.calls = None;
    cases.push((
        "limit removed",
        Broadening::LimitRemoved(spine_authority::LimitKind::Calls),
        s,
    ));
    let mut s = good;
    s.limits.calls = Some(101);
    cases.push((
        "limit raised",
        Broadening::LimitRaised(spine_authority::LimitKind::Calls),
        s,
    ));
    for (label, expected, spec) in cases {
        assert_eq!(
            Grant::delegate(&k[1], &root, spec).unwrap_err(),
            IssueError::Broadens(expected),
            "{label}"
        );
    }
}
