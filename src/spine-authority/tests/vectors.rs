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
    verify_encoded, Chain, ChargeError, Decision, Denial, Effect, FeedCheck, FeedError, FeedHead,
    Grant, Invocation, LedgerConfig, Limits, MemoryLedger, PublicKey, Receipt, ReceiptError,
    Revocation, RevocationBounds, RevocationFeed, RevocationSet, RevocationView, Spent,
    UsageLedger, VerifyContext, FEED_DIGEST_DOMAIN, FEED_HEAD_DOMAIN, GRANT_DOMAIN,
    GRANT_ID_DOMAIN, INVOCATION_DOMAIN, INVOCATION_ID_DOMAIN, MAX_CAVEATS, MAX_CAVEAT_ARG_LEN,
    MAX_CAVEAT_NAME_LEN, MAX_CHAIN_DEPTH, MAX_FEED_HEAD_BYTES, MAX_FEED_REVOCATIONS,
    MAX_GRANT_BYTES, MAX_INVOCATION_BYTES, MAX_INVOCATION_RESOURCES, MAX_PATH_SEGMENTS,
    MAX_RECEIPT_BYTES, MAX_RESOURCES, MAX_REVOCATION_BYTES, MAX_ROLE_LEN, MAX_SEGMENT_LEN,
    RECEIPT_DOMAIN, REVOCATION_DOMAIN, REVOCATION_ID_DOMAIN,
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
    let domains: [(&str, &[u8]); 6] = [
        ("invocation_id", INVOCATION_ID_DOMAIN),
        ("revocation", REVOCATION_DOMAIN),
        ("revocation_id", REVOCATION_ID_DOMAIN),
        ("feed_head", FEED_HEAD_DOMAIN),
        ("feed_digest", FEED_DIGEST_DOMAIN),
        ("receipt", RECEIPT_DOMAIN),
    ];
    for (name, value) in domains {
        assert_eq!(text(&doc["domains"], name), hex(value), "{name}");
    }
    let bounds = &doc["bounds"];
    let expected: [(&str, usize); 16] = [
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
        ("max_revocation_bytes", MAX_REVOCATION_BYTES),
        ("max_feed_head_bytes", MAX_FEED_HEAD_BYTES),
        ("max_feed_revocations", MAX_FEED_REVOCATIONS),
        ("max_receipt_bytes", MAX_RECEIPT_BYTES),
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
        assert_eq!(hex(&inv.id().unwrap()), text(v, "id"), "{w}: id");

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

/// Runs one vector case against `revocations`, with an empty ledger apart
/// from the case's nonces and totals.
fn run_case(
    v: &Value,
    revocations: &dyn RevocationView,
    bounds: RevocationBounds,
) -> Result<Decision, Denial> {
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
    let mut ctx = VerifyContext::new(
        now,
        unhex32(text(v, "recipient")),
        unhex32(text(v, "session")),
        &roots,
    );
    ctx.revocation_bounds = bounds;

    let mut ledger = MemoryLedger::new();
    for seen in v["seen_nonces"].as_array().unwrap() {
        let nonce: [u8; 16] = unhex(text(seen, "nonce")).try_into().expect("16 bytes");
        ledger
            .charge(&Decision {
                chain_ids: Vec::new(),
                chain_limits: Vec::new(),
                chain_expiries: Vec::new(),
                nonce_horizon: u64::MAX,
                invoker: unhex32(text(seen, "invoker")),
                nonce,
                now: 0,
                effect: Effect::Read,
                cost: 0,
                bytes: 0,
            })
            .unwrap();
    }
    for p in v["preload"].as_array().unwrap() {
        ledger.preload(
            unhex32(text(p, "id")),
            number(p, "calls"),
            number(p, "cost"),
            number(p, "bytes"),
        );
    }
    verify_encoded(&chain, &invocation, &ctx, revocations, &ledger)
}

/// Checks a result against a vector's `expect`. Returns `true` if accepted.
fn assert_expected(w: &str, expect: &Value, result: Result<Decision, Denial>) -> bool {
    if expect["ok"].as_bool().unwrap() {
        assert!(result.is_ok(), "{w}: refused with {result:?}");
        true
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
        false
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
        let mut revocations = RevocationSet {
            as_of: v["revocation_as_of"].as_u64(),
            ..RevocationSet::default()
        };
        for id in v["revoked"].as_array().unwrap() {
            revocations.revoke(unhex32(id.as_str().unwrap()));
        }
        let result = run_case(v, &revocations, RevocationBounds::default());
        if assert_expected(w, &v["expect"], result) {
            accepted += 1;
        } else {
            refused += 1;
        }
    }
    assert!(
        accepted >= 5 && refused >= 40,
        "{accepted} accepted, {refused} refused"
    );
}

fn flag(v: &Value, field: &str) -> bool {
    v[field]
        .as_bool()
        .unwrap_or_else(|| panic!("vector `{}`: field `{field}` is not a bool", v["label"]))
}

fn feed_error_name(error: &FeedError) -> &'static str {
    match error {
        FeedError::Cbor(_) => "Cbor",
        FeedError::UnknownVersion => "UnknownVersion",
        FeedError::BadRevocationSignature => "BadRevocationSignature",
        FeedError::BadHeadSignature => "BadHeadSignature",
        FeedError::WrongPublisher => "WrongPublisher",
        FeedError::SequenceRollback => "SequenceRollback",
        FeedError::FutureHead => "FutureHead",
        FeedError::TooMany => "TooMany",
        FeedError::DuplicateRevocation => "DuplicateRevocation",
        FeedError::RevokedAfterHead => "RevokedAfterHead",
        FeedError::CountMismatch => "CountMismatch",
        FeedError::DigestMismatch => "DigestMismatch",
    }
}

fn receipt_error_name(error: &ReceiptError) -> &'static str {
    match error {
        ReceiptError::Cbor(_) => "Cbor",
        ReceiptError::Malformed => "Malformed",
        ReceiptError::BadSignature => "BadSignature",
        ReceiptError::WrongRecipient => "WrongRecipient",
        ReceiptError::NotRecipient => "NotRecipient",
        ReceiptError::DecisionMismatch => "DecisionMismatch",
        ReceiptError::InvocationMismatch => "InvocationMismatch",
        ReceiptError::LeafMismatch => "LeafMismatch",
        ReceiptError::OutcomeMismatch => "OutcomeMismatch",
    }
}

fn charge_outcome(result: Result<(), ChargeError>) -> &'static str {
    match result {
        Ok(()) => "ok",
        Err(ChargeError::NonceReplayed) => "NonceReplayed",
        Err(ChargeError::LimitExhausted) => "LimitExhausted",
        Err(ChargeError::LedgerFull) => "LedgerFull",
        Err(ChargeError::MalformedDecision) => "MalformedDecision",
    }
}

fn denial_named(name: &str) -> Denial {
    match name {
        "Revoked" => Denial::Revoked,
        "RevocationDataStale" => Denial::RevocationDataStale,
        "LimitExhausted" => Denial::LimitExhausted,
        "ChainLimits" => Denial::ChainLimits,
        other => panic!("receipt vectors use an unknown denial `{other}`"),
    }
}

fn revocation_outcome(bytes: &[u8]) -> &'static str {
    match Revocation::decode(bytes) {
        Err(_) => "DecodeError",
        Ok(record) => match record.verify() {
            Ok(()) => "ok",
            Err(error) => feed_error_name(&error),
        },
    }
}

fn head_outcome(bytes: &[u8], publisher: &PublicKey) -> &'static str {
    match FeedHead::decode(bytes) {
        Err(_) => "DecodeError",
        Ok(head) => match head.verify(publisher) {
            Ok(()) => "ok",
            Err(error) => feed_error_name(&error),
        },
    }
}

fn receipt_outcome(bytes: &[u8], recipient: &PublicKey) -> &'static str {
    match Receipt::decode(bytes) {
        Err(_) => "DecodeError",
        Ok(receipt) => match receipt.verify(recipient) {
            Ok(()) => "ok",
            Err(error) => receipt_error_name(&error),
        },
    }
}

fn chain_of(hexes: &Value) -> Chain {
    let grants: Vec<Grant> = hexes
        .as_array()
        .unwrap()
        .iter()
        .map(|h| Grant::decode(&unhex(h.as_str().unwrap())).unwrap())
        .collect();
    Chain::new(grants)
}

/// Loads the feed of a vector, or says why it does not load.
fn load_feed(feed: &Value, now: u64) -> Result<RevocationFeed, &'static str> {
    let head = FeedHead::decode(&unhex(text(feed, "head"))).map_err(|_| "DecodeError")?;
    let mut revocations: Vec<Revocation> = Vec::new();
    for h in feed["revocations"].as_array().unwrap() {
        revocations
            .push(Revocation::decode(&unhex(h.as_str().unwrap())).map_err(|_| "DecodeError")?);
    }
    let check = FeedCheck {
        publisher: unhex32(text(feed, "publisher")),
        last_seen_sequence: feed["last_seen_sequence"].as_u64(),
        now,
    };
    RevocationFeed::load(revocations, &head, &check).map_err(|e| feed_error_name(&e))
}

#[test]
fn revocations_decode_and_re_encode_to_the_vector() {
    let doc = load();
    assert!(!section(&doc, "revocations").is_empty());
    for v in section(&doc, "revocations") {
        let w = text(v, "label");
        let encoded = unhex(text(v, "encoded"));
        let record = Revocation::decode(&encoded).unwrap_or_else(|e| panic!("{w}: decode: {e}"));
        assert_eq!(hex(&record.encode().unwrap()), text(v, "encoded"), "{w}");
        assert_eq!(
            hex(&record.body.signing_bytes().unwrap()),
            text(v, "signed_bytes"),
            "{w}: signed bytes"
        );
        assert_eq!(hex(&record.signature), text(v, "signature"), "{w}");
        assert_eq!(hex(&record.id().unwrap()), text(v, "id"), "{w}: id");
        assert_eq!(record.verify(), Ok(()), "{w}: verify");

        let signer = signing_key(&doc, text(v, "signer"));
        assert_eq!(
            signer.verifying_key().to_bytes(),
            record.body.revoker,
            "{w}"
        );
        let again = Revocation::sign(&signer, record.body.clone()).unwrap();
        assert_eq!(
            hex(&again.signature),
            text(v, "signature"),
            "{w}: re-signing"
        );

        let b = &v["body"];
        assert_eq!(number(b, "v"), record.body.version, "{w}: v");
        assert_eq!(
            text(b, "revoker"),
            hex(&record.body.revoker),
            "{w}: revoker"
        );
        assert_eq!(text(b, "grant"), hex(&record.body.grant), "{w}: grant");
        assert_eq!(number(b, "revoked_at"), record.body.revoked_at, "{w}");
        assert_eq!(text(b, "nonce"), hex(&record.body.nonce), "{w}: nonce");
    }
}

#[test]
fn feed_heads_decode_and_re_encode_to_the_vector() {
    let doc = load();
    assert!(!section(&doc, "feed_heads").is_empty());
    for v in section(&doc, "feed_heads") {
        let w = text(v, "label");
        let encoded = unhex(text(v, "encoded"));
        let head = FeedHead::decode(&encoded).unwrap_or_else(|e| panic!("{w}: decode: {e}"));
        assert_eq!(hex(&head.encode().unwrap()), text(v, "encoded"), "{w}");
        assert_eq!(
            hex(&head.body.signing_bytes().unwrap()),
            text(v, "signed_bytes"),
            "{w}: signed bytes"
        );
        assert_eq!(hex(&head.signature), text(v, "signature"), "{w}");
        assert_eq!(head.verify(&head.body.publisher), Ok(()), "{w}: verify");

        let signer = signing_key(&doc, text(v, "signer"));
        let again = FeedHead::sign(&signer, head.body.clone()).unwrap();
        assert_eq!(
            hex(&again.signature),
            text(v, "signature"),
            "{w}: re-signing"
        );

        let b = &v["body"];
        assert_eq!(number(b, "v"), head.body.version, "{w}: v");
        assert_eq!(text(b, "publisher"), hex(&head.body.publisher), "{w}");
        assert_eq!(number(b, "seq"), head.body.sequence, "{w}: seq");
        assert_eq!(number(b, "as_of"), head.body.as_of, "{w}: as_of");
        assert_eq!(number(b, "count"), head.body.count, "{w}: count");
        assert_eq!(text(b, "digest"), hex(&head.body.digest), "{w}: digest");
    }
}

#[test]
fn receipts_decode_and_re_encode_to_the_vector() {
    let doc = load();
    assert!(!section(&doc, "receipts").is_empty());
    for v in section(&doc, "receipts") {
        let w = text(v, "label");
        let encoded = unhex(text(v, "encoded"));
        let receipt = Receipt::decode(&encoded).unwrap_or_else(|e| panic!("{w}: decode: {e}"));
        assert_eq!(hex(&receipt.encode().unwrap()), text(v, "encoded"), "{w}");
        assert_eq!(
            hex(&receipt.body.signing_bytes().unwrap()),
            text(v, "signed_bytes"),
            "{w}: signed bytes"
        );
        assert_eq!(hex(&receipt.signature), text(v, "signature"), "{w}");
        assert_eq!(
            receipt.verify(&receipt.body.recipient),
            Ok(()),
            "{w}: verify"
        );

        let signer = signing_key(&doc, text(v, "signer"));
        let again = Receipt::sign_unchecked(&signer, receipt.body.clone()).unwrap();
        assert_eq!(
            hex(&again.signature),
            text(v, "signature"),
            "{w}: re-signing"
        );

        let b = &v["body"];
        let body = &receipt.body;
        assert_eq!(number(b, "v"), body.version, "{w}: v");
        assert_eq!(text(b, "recipient"), hex(&body.recipient), "{w}: recipient");
        assert_eq!(
            text(b, "invocation"),
            hex(&body.invocation),
            "{w}: invocation"
        );
        assert_eq!(text(b, "leaf"), hex(&body.leaf), "{w}: leaf");
        assert_eq!(flag(b, "allowed"), body.allowed, "{w}: allowed");
        assert_eq!(
            b["effect"].as_str(),
            body.effect.map(|e| e.as_str()),
            "{w}: effect"
        );
        assert_eq!(b["step"].as_u64(), body.step, "{w}: step");
        assert_eq!(number(b, "calls"), body.calls, "{w}: calls");
        assert_eq!(number(b, "cost"), body.cost, "{w}: cost");
        assert_eq!(number(b, "bytes"), body.bytes, "{w}: bytes");
        assert_eq!(text(b, "result"), hex(&body.result), "{w}: result");
        assert_eq!(number(b, "time"), body.time, "{w}: time");
    }
}

#[test]
fn encodings_decode_or_fail_as_the_vectors_state() {
    let doc = load();
    for v in section(&doc, "revocation_checks") {
        let w = text(v, "label");
        assert_eq!(
            revocation_outcome(&unhex(text(v, "encoded"))),
            text(v, "expect"),
            "revocation_checks/{w}"
        );
    }
    for v in section(&doc, "feed_head_checks") {
        let w = text(v, "label");
        let publisher = unhex32(text(v, "publisher"));
        assert_eq!(
            head_outcome(&unhex(text(v, "encoded")), &publisher),
            text(v, "expect"),
            "feed_head_checks/{w}"
        );
    }
    for v in section(&doc, "receipt_checks") {
        let w = text(v, "label");
        let recipient = unhex32(text(v, "expected_recipient"));
        assert_eq!(
            receipt_outcome(&unhex(text(v, "encoded")), &recipient),
            text(v, "expect"),
            "receipt_checks/{w}"
        );
    }
}

#[test]
fn who_may_revoke_is_what_the_vectors_state() {
    let doc = load();
    let cases = section(&doc, "authority_cases");
    assert!(cases.len() >= 10);
    for v in cases {
        let w = text(v, "label");
        let chain = chain_of(&v["chain"]);
        let record = Revocation::decode(&unhex(text(v, "revocation"))).unwrap();
        assert_eq!(
            record.is_authorized_by(&chain),
            flag(v, "expect_authorized"),
            "{w}"
        );
    }
}

#[test]
fn feeds_load_or_are_refused_as_the_vectors_state() {
    let doc = load();
    let cases = section(&doc, "feed_cases");
    assert!(cases.len() >= 20);
    for v in cases {
        let w = text(v, "label");
        let outcome = match load_feed(&v["feed"], number(v, "now")) {
            Ok(_) => "ok",
            Err(name) => name,
        };
        assert_eq!(outcome, text(v, "expect"), "feed_cases/{w}");
    }
}

#[test]
fn step_8_against_a_feed_stops_where_the_vectors_state() {
    let doc = load();
    let cases = section(&doc, "feed_step_cases");
    assert!(cases.len() >= 15);
    for v in cases {
        let w = text(v, "label");
        let bounds = &v["revocation_bounds"];
        let revocation_bounds = RevocationBounds {
            read: number(bounds, "read"),
            reversible: number(bounds, "reversible"),
            irreversible: number(bounds, "irreversible"),
        };
        // A feed that does not load is no data.
        let feed = match load_feed(&v["feed"], number(v, "now")) {
            Ok(feed) => {
                assert!(v["feed_error"].is_null(), "{w}: feed_error but it loads");
                feed
            }
            Err(name) => {
                assert_eq!(name, text(v, "feed_error"), "{w}: feed_error");
                RevocationFeed::default()
            }
        };
        let result = run_case(v, &feed, revocation_bounds);
        assert_expected(w, &v["expect"], result);
    }
}

#[test]
fn receipts_verify_and_match_as_the_vectors_state() {
    let doc = load();
    let cases = section(&doc, "receipt_cases");
    assert!(cases.len() >= 20);
    for v in cases {
        let w = text(v, "label");
        let receipt = Receipt::decode(&unhex(text(v, "receipt"))).unwrap();
        let expected = unhex32(text(v, "expected_recipient"));
        let outcome = match receipt.verify(&expected) {
            Ok(()) => "ok",
            Err(error) => receipt_error_name(&error),
        };
        assert_eq!(outcome, text(v, "expect_verify"), "{w}: verify");

        let invocation = Invocation::decode(&unhex(text(v, "invocation"))).unwrap();
        let verdict_json = &v["verdict"];
        let verdict: Result<Decision, Denial> = if flag(verdict_json, "ok") {
            Ok(Decision {
                chain_ids: vec![unhex32(text(verdict_json, "leaf"))],
                chain_limits: vec![Limits::default()],
                chain_expiries: vec![u64::MAX],
                nonce_horizon: u64::MAX,
                invoker: unhex32(text(verdict_json, "invoker")),
                nonce: unhex(text(verdict_json, "nonce"))
                    .try_into()
                    .expect("16 bytes"),
                now: 0,
                effect: Effect::parse(text(verdict_json, "effect")).expect("an effect class"),
                cost: number(verdict_json, "cost"),
                bytes: number(verdict_json, "bytes"),
            })
        } else {
            let denial = denial_named(text(verdict_json, "denial"));
            assert_eq!(
                u64::from(denial.step()),
                number(verdict_json, "step"),
                "{w}"
            );
            Err(denial)
        };
        let outcome = match receipt.matches(&invocation, &verdict) {
            Ok(()) => "ok",
            Err(error) => receipt_error_name(&error),
        };
        assert_eq!(outcome, text(v, "expect_matches"), "{w}: matches");
    }
}

fn decision_of(v: &Value) -> Decision {
    let limits: Vec<Limits> = v["chain_limits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| Limits {
            calls: l["calls"].as_u64(),
            cost: l["cost"].as_u64(),
            bytes: l["bytes"].as_u64(),
            rate: l["rate"].as_u64(),
        })
        .collect();
    Decision {
        chain_ids: v["chain_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| unhex32(h.as_str().unwrap()))
            .collect(),
        chain_limits: limits,
        chain_expiries: v["chain_expiries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap())
            .collect(),
        nonce_horizon: number(v, "nonce_horizon"),
        invoker: unhex32(text(v, "invoker")),
        nonce: unhex(text(v, "nonce")).try_into().expect("16 bytes"),
        now: number(v, "now"),
        effect: Effect::Read,
        cost: number(v, "cost"),
        bytes: number(v, "bytes"),
    }
}

#[test]
fn the_ledger_behaves_as_the_vectors_state() {
    let doc = load();
    let cases = section(&doc, "ledger_cases");
    assert!(cases.len() >= 8);
    for v in cases {
        let w = text(v, "label");
        let config = LedgerConfig {
            max_nonces: number(&v["config"], "max_nonces") as usize,
            max_grants: number(&v["config"], "max_grants") as usize,
        };
        let mut ledger = MemoryLedger::with_config(config);
        for (index, step) in v["steps"].as_array().unwrap().iter().enumerate() {
            let result = ledger.charge(&decision_of(&step["decision"]));
            assert_eq!(
                charge_outcome(result),
                text(step, "expect"),
                "{w}: step {index}"
            );
        }
        let q = &v["queries"];
        for n in q["nonces"].as_array().unwrap() {
            let nonce: [u8; 16] = unhex(text(n, "nonce")).try_into().expect("16 bytes");
            assert_eq!(
                ledger.nonce_seen(&unhex32(text(n, "invoker")), &nonce),
                flag(n, "seen"),
                "{w}: nonce"
            );
        }
        for s in q["spent"].as_array().unwrap() {
            let spent = ledger
                .spent(&unhex32(text(s, "id")), number(s, "now"))
                .unwrap();
            let expected = Spent {
                calls: number(s, "calls"),
                cost: number(s, "cost"),
                bytes: number(s, "bytes"),
                recent_calls: number(s, "recent_calls"),
            };
            assert_eq!(spent, expected, "{w}: spent");
        }
        assert_eq!(
            ledger.nonce_count() as u64,
            number(q, "nonce_count"),
            "{w}: nonce count"
        );
        assert_eq!(
            ledger.grant_count() as u64,
            number(q, "grant_count"),
            "{w}: grant count"
        );
    }
}
