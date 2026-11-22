//! Checks `spine-authority` against `docs/spec/03-authority.vectors.json`.
//!
//! The vectors are produced by `scripts/authority_v1.py`, an independent
//! Python implementation written from the specification. Everything here uses
//! the public API only. Encodings are pinned two ways: decoding the vector's
//! bytes must give a grant whose signed bytes, signature, id and re-encoding
//! match the vector, and because Ed25519 is deterministic, re-signing with the
//! vector's seed must give the vector's signature. Every case is run through
//! `verify_encoded` and must stop at the step, with the denial, that the
//! vector states.

use std::path::Path;

use ed25519_dalek::SigningKey;
use serde_json::Value;
use spine_authority::{
    verify_encoded, Decision, Effect, Grant, Invocation, MemoryLedger, PublicKey, RevocationSet,
    VerifyContext, GRANT_DOMAIN, GRANT_ID_DOMAIN, INVOCATION_DOMAIN, MAX_CAVEATS,
    MAX_CAVEAT_ARG_LEN, MAX_CAVEAT_NAME_LEN, MAX_CHAIN_DEPTH, MAX_GRANT_BYTES,
    MAX_INVOCATION_BYTES, MAX_INVOCATION_RESOURCES, MAX_PATH_SEGMENTS, MAX_RESOURCES, MAX_ROLE_LEN,
    MAX_SEGMENT_LEN,
};

fn load() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("spec")
        .join("03-authority.vectors.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn section<'a>(doc: &'a Value, name: &str) -> &'a Vec<Value> {
    doc[name]
        .as_array()
        .unwrap_or_else(|| panic!("section `{name}` is missing or not an array"))
}

fn text<'a>(v: &'a Value, field: &str) -> &'a str {
    v[field]
        .as_str()
        .unwrap_or_else(|| panic!("vector `{}`: field `{field}` is not a string", v["label"]))
}

fn number(v: &Value, field: &str) -> u64 {
    v[field]
        .as_u64()
        .unwrap_or_else(|| panic!("vector `{}`: field `{field}` is not a u64", v["label"]))
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2) && s.is_ascii(), "bad hex string");
    s.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).expect("hex is ASCII");
            u8::from_str_radix(digits, 16).unwrap_or_else(|_| panic!("bad hex `{digits}`"))
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex32(s: &str) -> [u8; 32] {
    unhex(s).try_into().expect("32 bytes")
}

fn signing_key(doc: &Value, label: &str) -> SigningKey {
    let key = section(doc, "keys")
        .iter()
        .find(|k| k["label"] == label)
        .unwrap_or_else(|| panic!("no key `{label}`"));
    SigningKey::from_bytes(&unhex32(text(key, "seed")))
}

#[test]
fn domains_and_bounds_are_the_ones_the_vectors_state() {
    let doc = load();
    assert_eq!(text(&doc["domains"], "grant"), hex(GRANT_DOMAIN));
    assert_eq!(text(&doc["domains"], "grant_id"), hex(GRANT_ID_DOMAIN));
    assert_eq!(text(&doc["domains"], "invocation"), hex(INVOCATION_DOMAIN));
    let bounds = &doc["bounds"];
    let expected: [(&str, usize); 12] = [
        ("max_chain_depth", MAX_CHAIN_DEPTH),
        ("max_grant_bytes", MAX_GRANT_BYTES),
        ("max_invocation_bytes", MAX_INVOCATION_BYTES),
        ("max_resources", MAX_RESOURCES),
        ("max_actions", 4),
        ("max_caveats", MAX_CAVEATS),
        ("max_caveat_name_len", MAX_CAVEAT_NAME_LEN),
        ("max_caveat_arg_len", MAX_CAVEAT_ARG_LEN),
        ("max_role_len", MAX_ROLE_LEN),
        ("max_path_segments", MAX_PATH_SEGMENTS),
        ("max_segment_len", MAX_SEGMENT_LEN),
        ("max_invocation_resources", MAX_INVOCATION_RESOURCES),
    ];
    for (name, value) in expected {
        assert_eq!(number(bounds, name), value as u64, "{name}");
    }
}

#[test]
fn keys_are_the_seeds_public_keys() {
    let doc = load();
    for v in section(&doc, "keys") {
        let seed = unhex32(text(v, "seed"));
        let public = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
        assert_eq!(text(v, "public"), hex(&public), "{}", text(v, "label"));
    }
}

#[test]
fn grants_decode_and_re_encode_to_the_vector() {
    let doc = load();
    assert!(!section(&doc, "grants").is_empty());
    for v in section(&doc, "grants") {
        let w = text(v, "label");
        let encoded = unhex(text(v, "encoded"));
        let grant = Grant::decode(&encoded).unwrap_or_else(|e| panic!("{w}: decode: {e}"));

        assert_eq!(
            hex(&grant.encode().unwrap()),
            text(v, "encoded"),
            "{w}: encode"
        );
        assert_eq!(
            hex(&grant.body.signing_bytes().unwrap()),
            text(v, "signed_bytes"),
            "{w}: signed bytes"
        );
        assert_eq!(
            hex(&grant.signature),
            text(v, "signature"),
            "{w}: signature"
        );
        assert_eq!(hex(&grant.id().unwrap()), text(v, "id"), "{w}: id");
        assert!(grant.verify_signature(), "{w}: verify");
        assert!(grant.body.check_well_formed().is_ok(), "{w}: well formed");

        // Ed25519 is deterministic: signing again gives the same signature.
        let signer = signing_key(&doc, text(v, "signer"));
        assert_eq!(
            signer.verifying_key().to_bytes(),
            grant.body.iss,
            "{w}: issuer"
        );
        let again = Grant::sign_unchecked(&signer, grant.body.clone()).unwrap();
        assert_eq!(
            hex(&again.signature),
            text(v, "signature"),
            "{w}: re-signing"
        );

        // The structured description agrees with the decoded value.
        let b = &v["body"];
        let body = &grant.body;
        assert_eq!(text(b, "iss"), hex(&body.iss), "{w}: iss");
        assert_eq!(text(b, "aud"), hex(&body.aud), "{w}: aud");
        match (&b["parent"], &body.parent) {
            (Value::Null, None) => {}
            (Value::String(s), Some(p)) => assert_eq!(s, &hex(p), "{w}: parent"),
            _ => panic!("{w}: parent"),
        }
        assert_eq!(number(b, "v"), body.version, "{w}: v");
        assert_eq!(text(b, "role"), body.role, "{w}: role");
        assert_eq!(text(b, "effect"), body.effect.as_str(), "{w}: effect");
        assert_eq!(
            number(b, "delegable"),
            u64::from(body.delegable),
            "{w}: delegable"
        );
        assert_eq!(number(b, "nbf"), body.nbf, "{w}: nbf");
        assert_eq!(number(b, "exp"), body.exp, "{w}: exp");
        assert_eq!(text(b, "nonce"), hex(&body.nonce), "{w}: nonce");
        let acts: Vec<&str> = b["act"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        let have: Vec<&str> = body.act.iter().map(|a| a.as_str()).collect();
        assert_eq!(acts, have, "{w}: act");
        let patterns = b["res"].as_array().unwrap();
        assert_eq!(patterns.len(), body.res.len(), "{w}: res count");
        for (j, p) in patterns.iter().zip(body.res.iter()) {
            assert_eq!(text(j, "owner"), hex(&p.owner), "{w}: res owner");
            assert_eq!(j["prefix"].as_bool().unwrap(), p.prefix, "{w}: res prefix");
            let path: Vec<&str> = j["path"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect();
            let have: Vec<&str> = p.path.iter().map(|s| s.as_str()).collect();
            assert_eq!(path, have, "{w}: res path");
        }
        let limits = &b["limits"];
        assert_eq!(limits["calls"].as_u64(), body.limits.calls, "{w}: calls");
        assert_eq!(limits["cost"].as_u64(), body.limits.cost, "{w}: cost");
        assert_eq!(limits["bytes"].as_u64(), body.limits.bytes, "{w}: bytes");
        assert_eq!(limits["rate"].as_u64(), body.limits.rate, "{w}: rate");
        assert_eq!(
            b["caveats"].as_array().unwrap().len(),
            body.caveats.len(),
            "{w}: caveats"
        );
    }
}

#[test]
fn invocations_decode_and_re_encode_to_the_vector() {
    let doc = load();
    assert!(!section(&doc, "invocations").is_empty());
    for v in section(&doc, "invocations") {
        let w = text(v, "label");
        let encoded = unhex(text(v, "encoded"));
        let inv = Invocation::decode(&encoded).unwrap_or_else(|e| panic!("{w}: decode: {e}"));
        assert_eq!(
            hex(&inv.encode().unwrap()),
            text(v, "encoded"),
            "{w}: encode"
        );
        assert_eq!(
            hex(&inv.body.signing_bytes().unwrap()),
            text(v, "signed_bytes"),
            "{w}: signed bytes"
        );
        assert_eq!(hex(&inv.signature), text(v, "signature"), "{w}: signature");
        assert!(inv.verify_signature(), "{w}: verify");

        let signer = signing_key(&doc, text(v, "signer"));
        assert_eq!(
            signer.verifying_key().to_bytes(),
            inv.body.invoker,
            "{w}: invoker"
        );
        let again = Invocation::sign(&signer, inv.body.clone()).unwrap();
        assert_eq!(
            hex(&again.signature),
            text(v, "signature"),
            "{w}: re-signing"
        );

        let b = &v["body"];
        assert_eq!(text(b, "invoker"), hex(&inv.body.invoker), "{w}: invoker");
        assert_eq!(
            text(b, "recipient"),
            hex(&inv.body.recipient),
            "{w}: recipient"
        );
        assert_eq!(text(b, "session"), hex(&inv.body.session), "{w}: session");
        assert_eq!(text(b, "nonce"), hex(&inv.body.nonce), "{w}: nonce");
        assert_eq!(text(b, "op"), hex(&inv.body.op), "{w}: op");
        assert_eq!(text(b, "leaf"), hex(&inv.body.leaf), "{w}: leaf");
        assert_eq!(text(b, "act"), inv.body.act.as_str(), "{w}: act");
        assert_eq!(text(b, "effect"), inv.body.effect.as_str(), "{w}: effect");
        assert_eq!(number(b, "time"), inv.body.time, "{w}: time");
        assert_eq!(number(b, "cost"), inv.body.cost, "{w}: cost");
        assert_eq!(number(b, "bytes"), inv.body.bytes, "{w}: bytes");
        assert_eq!(
            b["res"].as_array().unwrap().len(),
            inv.body.res.len(),
            "{w}: res"
        );
    }
}

#[test]
fn every_case_stops_at_the_step_the_vector_states() {
    let doc = load();
    let cases = section(&doc, "cases");
    assert!(cases.len() >= 50, "only {} cases", cases.len());
    let mut accepted = 0usize;
    let mut refused = 0usize;
    for v in cases {
        let w = text(v, "label");
        let chain: Vec<Vec<u8>> = v["chain"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| unhex(h.as_str().unwrap()))
            .collect();
        let invocation = unhex(text(v, "invocation"));
        let roots: Vec<PublicKey> = v["trusted_roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| unhex32(h.as_str().unwrap()))
            .collect();
        let now = number(v, "now");
        let ctx = VerifyContext::new(
            now,
            unhex32(text(v, "recipient")),
            unhex32(text(v, "session")),
            &roots,
        );

        let mut revocations = RevocationSet {
            as_of: v["revocation_as_of"].as_u64(),
            ..RevocationSet::default()
        };
        for id in v["revoked"].as_array().unwrap() {
            revocations.revoke(unhex32(id.as_str().unwrap()));
        }

        let mut ledger = MemoryLedger::new();
        for seen in v["seen_nonces"].as_array().unwrap() {
            let nonce: [u8; 16] = unhex(text(seen, "nonce")).try_into().expect("16 bytes");
            ledger.charge(&Decision {
                chain_ids: Vec::new(),
                invoker: unhex32(text(seen, "invoker")),
                nonce,
                now: 0,
                effect: Effect::Read,
                cost: 0,
                bytes: 0,
            });
        }
        for p in v["preload"].as_array().unwrap() {
            ledger.preload(
                unhex32(text(p, "id")),
                number(p, "calls"),
                number(p, "cost"),
                number(p, "bytes"),
            );
        }

        let result = verify_encoded(&chain, &invocation, &ctx, &revocations, &ledger);
        let expect = &v["expect"];
        if expect["ok"].as_bool().unwrap() {
            assert!(result.is_ok(), "{w}: refused with {result:?}");
            accepted += 1;
        } else {
            let denial = match result {
                Err(denial) => denial,
                Ok(_) => panic!("{w}: accepted but must be refused"),
            };
            assert_eq!(denial.name(), text(expect, "denial"), "{w}: denial");
            assert_eq!(
                u64::from(denial.step()),
                number(expect, "step"),
                "{w}: step"
            );
            refused += 1;
        }
    }
    assert!(
        accepted >= 5 && refused >= 40,
        "{accepted} accepted, {refused} refused"
    );
}
