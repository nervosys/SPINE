//! Checks `spine-name` against `docs/spec/02-names.vectors.json`.
//!
//! The vectors are produced by `scripts/names_v1.py`, an independent Python
//! implementation written from the specification. Everything here uses the
//! public API only. Record layout is pinned two ways: `signing_bytes()` is
//! public and compared byte for byte, and because Ed25519 is deterministic the
//! signature produced by re-signing with the vector's seed must equal the
//! vector's signature.

use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey};
use serde_json::Value;
use spine_name::{
    base32, content_hash, detect_equivocation, record_authorised, resource_id, verify_resource,
    Authority, Current, NameError, NameKey, NameRecord, RecordAuthority, Rel, SpineUri, Succession,
    SuccessionLog,
};

fn load() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("spec")
        .join("02-names.vectors.json");
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

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len() & 1 == 0, "odd-length hex string");
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
fn keys_authorities_and_base32() {
    let doc = load();
    for v in section(&doc, "keys") {
        let w = text(v, "label");
        let seed = unhex32(text(v, "seed"));
        let public = unhex32(text(v, "public"));
        let b32 = text(v, "base32");

        assert_eq!(
            SigningKey::from_bytes(&seed).verifying_key().to_bytes(),
            public,
            "{w}: public key from seed"
        );
        assert_eq!(base32::encode(&public), b32, "{w}: base32 encode");
        assert_eq!(NameKey::from_bytes(public).to_base32(), b32, "{w}: NameKey");
        assert_eq!(base32::decode_key(b32), Some(public), "{w}: decode");
        assert_eq!(
            base32::decode_key(&b32.to_uppercase()),
            Some(public),
            "{w}: upper-case spelling"
        );
        assert_eq!(
            serde_json::to_string(&NameKey::from_bytes(public)).unwrap(),
            format!("\"{b32}\""),
            "{w}: NameKey JSON"
        );
        assert_eq!(
            serde_json::from_str::<NameKey>(&format!("\"{b32}\"")).unwrap(),
            NameKey::from_bytes(public),
            "{w}: NameKey from JSON"
        );

        let authority = text(v, "authority");
        assert_eq!(authority, format!("did:{b32}"), "{w}: authority text");
        let uri = SpineUri::parse(text(v, "name")).unwrap();
        assert_eq!(uri.public_key(), Some(&public), "{w}: key in name");
        assert_eq!(uri.authority().to_string(), authority, "{w}: authority");
        assert_eq!(uri.to_string(), text(v, "name"), "{w}: name form");
        assert_eq!(
            SpineUri::did(public).to_string(),
            text(v, "name"),
            "{w}: did()"
        );

        let other = text(v, "noncanonical_base32");
        assert_eq!(base32::decode_key(other), None, "{w}: non-canonical base32");
        assert!(
            serde_json::from_str::<NameKey>(&format!("\"{other}\"")).is_err(),
            "{w}: non-canonical NameKey JSON"
        );
        assert!(
            SpineUri::parse(&format!("spine://did:{other}/")).is_err(),
            "{w}: non-canonical authority"
        );
    }
}

#[test]
fn names_parse_display_and_routing_key() {
    let doc = load();
    for v in section(&doc, "names") {
        let w = text(v, "label");
        let canonical = text(v, "canonical");
        let uri = SpineUri::parse(text(v, "input"))
            .unwrap_or_else(|e| panic!("{w}: input does not parse: {e}"));

        assert_eq!(uri.to_string(), canonical, "{w}: canonical form");
        assert_eq!(
            SpineUri::parse(canonical).unwrap(),
            uri,
            "{w}: canonical form is a fixed point"
        );
        assert_eq!(
            canonical.parse::<SpineUri>().unwrap().to_string(),
            canonical,
            "{w}: FromStr"
        );
        assert_eq!(
            hex(uri.key().as_bytes()),
            text(v, "routing_key"),
            "{w}: routing key"
        );
        assert_eq!(uri.path(), text(v, "path"), "{w}: path");
        assert_eq!(uri.fragment(), v["fragment"].as_str(), "{w}: fragment");

        let query: Vec<Vec<String>> = uri
            .query()
            .iter()
            .map(|(k, val)| vec![k.clone(), val.clone()])
            .collect();
        let want_query: Vec<Vec<String>> = serde_json::from_value(v["query"].clone()).unwrap();
        assert_eq!(query, want_query, "{w}: query");

        let a = &v["authority"];
        match a["kind"].as_str().unwrap() {
            "did" => {
                assert_eq!(
                    uri.public_key(),
                    Some(&unhex32(text(a, "key"))),
                    "{w}: did key"
                );
            }
            "blob" => {
                assert_eq!(
                    uri.content_hash(),
                    Some(&unhex32(text(a, "key"))),
                    "{w}: blob hash"
                );
            }
            "cap" => {
                assert_eq!(
                    uri.capability_term(),
                    Some(text(a, "term")),
                    "{w}: cap term"
                );
            }
            "host" => match uri.authority() {
                Authority::Host { host, port } => {
                    assert_eq!(host, text(a, "host"), "{w}: host");
                    assert_eq!(port.map(u64::from), a["port"].as_u64(), "{w}: port");
                }
                other => panic!("{w}: expected a host authority, got {other:?}"),
            },
            kind => panic!("{w}: unknown authority kind `{kind}`"),
        }
    }
}

#[test]
fn invalid_names_are_refused() {
    let doc = load();
    for v in section(&doc, "invalid_names") {
        let w = text(v, "label");
        let input = text(v, "input");
        assert!(
            SpineUri::parse(input).is_err(),
            "{w}: `{input}` must not parse"
        );
    }
}

#[test]
fn resource_ids_and_verification() {
    let doc = load();
    let items = section(&doc, "resource_ids");
    for v in items {
        let w = text(v, "label");
        let ct = text(v, "content_type");
        let bytes = unhex(text(v, "bytes"));
        let id = unhex32(text(v, "id"));

        assert_eq!(resource_id(ct, &bytes), Ok(id), "{w}: resource_id");
        assert_eq!(verify_resource(&id, ct, &bytes), Ok(()), "{w}: verify");
        assert_eq!(
            hex(&content_hash(&bytes)),
            text(v, "content_hash"),
            "{w}: content_hash"
        );
        assert_ne!(
            hex(&id),
            text(v, "content_hash"),
            "{w}: id is not the plain hash"
        );

        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(
            verify_resource(&id, ct, &longer),
            Err(NameError::ResourceIdMismatch),
            "{w}: different bytes"
        );
        assert_eq!(
            verify_resource(&id, "x-other", &bytes),
            Err(NameError::ResourceIdMismatch),
            "{w}: different type"
        );

        if let Some(other) = v["distinct_from"].as_str() {
            let other = items
                .iter()
                .find(|o| o["label"] == other)
                .unwrap_or_else(|| panic!("{w}: no vector `{other}`"));
            assert_ne!(text(v, "id"), text(other, "id"), "{w}: boundary shift");
        }
    }

    for v in section(&doc, "invalid_content_types") {
        let w = text(v, "label");
        assert_eq!(
            resource_id(text(v, "content_type"), b"x"),
            Err(NameError::InvalidContentType),
            "{w}"
        );
    }
}

#[test]
fn relations_normalise_builtin_aliases() {
    let doc = load();
    for v in section(&doc, "rel_aliases") {
        let w = text(v, "label");
        let rel: Rel = serde_json::from_value(v["json"].clone())
            .unwrap_or_else(|e| panic!("{w}: does not deserialize: {e}"));
        assert_eq!(rel.as_str(), text(v, "as_str"), "{w}: as_str");
        assert_eq!(
            matches!(rel, Rel::Other(_)),
            v["is_other"].as_bool().unwrap(),
            "{w}: variant"
        );
    }
}

#[test]
fn records_layout_signature_and_json() {
    let doc = load();
    for v in section(&doc, "records") {
        let w = text(v, "label");
        let record: NameRecord = serde_json::from_value(v["json"].clone())
            .unwrap_or_else(|e| panic!("{w}: json does not deserialize: {e}"));

        assert_eq!(
            hex(&record.signing_bytes()),
            text(v, "signing_bytes"),
            "{w}: signing_bytes"
        );
        assert_eq!(
            hex(&record.signature),
            text(v, "signature"),
            "{w}: stored signature"
        );
        assert_eq!(record.verify(), Ok(()), "{w}: verify");
        assert_eq!(record.validate_lifetime(), Ok(()), "{w}: lifetime");
        assert_eq!(
            record.is_tombstone(),
            v["is_tombstone"].as_bool().unwrap(),
            "{w}: is_tombstone"
        );

        // Ed25519 is deterministic, so this also pins the signed bytes.
        let key = signing_key(&doc, text(v, "signer"));
        let mut resigned = record.clone();
        resigned.signature.clear();
        resigned
            .sign(&key)
            .unwrap_or_else(|e| panic!("{w}: sign: {e}"));
        assert_eq!(
            hex(&resigned.signature),
            text(v, "signature"),
            "{w}: re-signed signature"
        );

        // The JSON form in the vector is what serde emits.
        assert_eq!(
            serde_json::to_value(&record).unwrap(),
            v["json"],
            "{w}: serde output"
        );
    }
}

#[test]
fn invalid_records_fail_verification() {
    let doc = load();
    for v in section(&doc, "invalid_records") {
        let w = text(v, "label");
        assert_eq!(text(v, "expected"), "reject", "{w}: expected");
        let record: NameRecord = serde_json::from_value(v["json"].clone())
            .unwrap_or_else(|e| panic!("{w}: json does not deserialize: {e}"));
        let got = record.verify();
        match text(v, "reason") {
            "signature" => assert_eq!(got, Err(NameError::BadSignature), "{w}: must be rejected"),
            "ttl" => assert!(
                matches!(got, Err(NameError::TtlTooLong(_))),
                "{w}: must be refused for its ttl, got {got:?}"
            ),
            "tombstone" => assert!(
                matches!(got, Err(NameError::MalformedTombstone(_))),
                "{w}: must be refused as a malformed tombstone, got {got:?}"
            ),
            other => panic!("{w}: unknown reason {other}"),
        }
        // A correctly signed but unadmissible record cannot be produced by `sign`.
        if text(v, "reason") != "signature" {
            let key = signing_key(&doc, "k0");
            let mut resigned = record.clone();
            resigned.signature.clear();
            assert!(resigned.sign(&key).is_err(), "{w}: sign must refuse it");
        }
    }
}

#[test]
fn admission_applies_the_lifetime_rules_and_the_clock_bound() {
    let doc = load();
    for v in section(&doc, "admission") {
        let w = text(v, "label");
        let name = text(v, "record");
        let target = section(&doc, "records")
            .iter()
            .chain(section(&doc, "invalid_records").iter())
            .find(|r| text(r, "label") == name)
            .unwrap_or_else(|| panic!("{w}: no record {name}"));
        let record: NameRecord = serde_json::from_value(target["json"].clone())
            .unwrap_or_else(|e| panic!("{w}: json does not deserialize: {e}"));
        let now = v["now"].as_u64().unwrap();
        // verify() covers the signature and the clock-free lifetime rules;
        // validate_not_postdated() is the clock bound.
        let got = record
            .verify()
            .and_then(|()| record.validate_not_postdated(now));
        match text(v, "expected") {
            "admit" => assert_eq!(got, Ok(()), "{w}: must be admitted"),
            "reject" => match text(v, "reason") {
                "postdated" => assert!(
                    matches!(got, Err(NameError::PostDated { .. })),
                    "{w}: got {got:?}"
                ),
                "ttl" => assert!(
                    matches!(got, Err(NameError::TtlTooLong(_))),
                    "{w}: got {got:?}"
                ),
                "tombstone" => assert!(
                    matches!(got, Err(NameError::MalformedTombstone(_))),
                    "{w}: got {got:?}"
                ),
                other => panic!("{w}: unknown reason {other}"),
            },
            other => panic!("{w}: unknown expectation {other}"),
        }
    }
}

fn any_key<'a>(doc: &'a Value, label: &str) -> &'a Value {
    section(doc, "keys")
        .iter()
        .chain(section(doc, "succession_keys").iter())
        .find(|k| k["label"] == label)
        .unwrap_or_else(|| panic!("no key `{label}`"))
}

fn load_log(v: &Value) -> SuccessionLog {
    let w = text(v, "label");
    let mut entries = Vec::new();
    for x in v["entries"].as_array().expect("entries") {
        let e: Succession = serde_json::from_value(x["json"].clone())
            .unwrap_or_else(|e| panic!("{w}: entry does not deserialize: {e}"));
        assert_eq!(
            hex(&e.signing_bytes()),
            text(x, "signing_bytes"),
            "{w}: signing_bytes"
        );
        assert_eq!(hex(&e.hash()), text(x, "hash"), "{w}: hash");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            x["json"],
            "{w}: serde output"
        );
        entries.push(e);
    }
    SuccessionLog { entries }
}

fn inception_of(doc: &Value, v: &Value) -> [u8; 32] {
    unhex32(text(any_key(doc, text(v, "inception")), "public"))
}

#[test]
fn succession_logs_verify_to_the_stated_state() {
    let doc = load();
    for v in section(&doc, "succession_logs") {
        let w = text(v, "label");
        let log = load_log(v);
        let state = log
            .verify(&inception_of(&doc, v))
            .unwrap_or_else(|e| panic!("{w}: must verify: {e}"));
        let want = &v["state"];
        assert_eq!(state.index(), want["index"].as_u64().unwrap(), "{w}: index");
        assert_eq!(
            state.head().map(|h| hex(h)),
            want["head"].as_str().map(String::from),
            "{w}: head"
        );
        let current = match state.current() {
            Current::Key(k) => hex(&k),
            Current::Retired => "retired".to_string(),
        };
        assert_eq!(current, text(want, "current"), "{w}: current");
        assert_eq!(
            state.commitment().map(|c| hex(c)),
            want["commitment"].as_str().map(String::from),
            "{w}: commitment"
        );
        let superseded: Vec<String> = state.superseded_keys().iter().map(|k| hex(k)).collect();
        let want_superseded: Vec<String> =
            serde_json::from_value(want["superseded"].clone()).unwrap();
        assert_eq!(superseded, want_superseded, "{w}: superseded keys");
    }
}

#[test]
fn invalid_succession_logs_are_refused_for_the_stated_reason() {
    let doc = load();
    for v in section(&doc, "invalid_succession_logs") {
        let w = text(v, "label");
        let log = load_log(v);
        let err = log
            .verify(&inception_of(&doc, v))
            .expect_err("must be refused");
        assert_eq!(err.reason(), text(v, "reason"), "{w}: {err}");
    }
}

#[test]
fn equivocation_is_detected_as_specified() {
    let doc = load();
    let logs: Vec<(&str, SuccessionLog, [u8; 32])> = section(&doc, "succession_logs")
        .iter()
        .chain(section(&doc, "invalid_succession_logs").iter())
        .map(|v| (text(v, "label"), load_log(v), inception_of(&doc, v)))
        .collect();
    let find = |label: &str| {
        logs.iter()
            .find(|(l, _, _)| *l == label)
            .unwrap_or_else(|| panic!("no log `{label}`"))
    };
    for v in section(&doc, "succession_equivocation") {
        let w = text(v, "label");
        let (_, a, inception) = find(text(v, "a"));
        let (_, b, _) = find(text(v, "b"));
        let got = detect_equivocation(inception, a, b);
        match text(v, "expected") {
            "invalid" => assert_eq!(
                got.expect_err("one log is invalid").reason(),
                text(v, "reason"),
                "{w}"
            ),
            "none" => assert_eq!(got, Ok(None), "{w}"),
            "equivocation" => {
                let proof = got.unwrap().expect("a fork");
                assert_eq!(Some(proof.index()), v["index"].as_u64(), "{w}: index");
                assert_eq!(proof.verify(inception), Ok(()), "{w}: proof");
            }
            other => panic!("{w}: unknown expectation {other}"),
        }
    }
}

#[test]
fn records_are_authorised_by_the_log_position() {
    let doc = load();
    let mut states = std::collections::BTreeMap::new();
    for v in section(&doc, "succession_logs") {
        let state = load_log(v).verify(&inception_of(&doc, v)).unwrap();
        states.insert(text(v, "label").to_string(), state);
    }
    for v in section(&doc, "succession_records") {
        let w = text(v, "label");
        let record: NameRecord = serde_json::from_value(v["json"].clone())
            .unwrap_or_else(|e| panic!("{w}: json does not deserialize: {e}"));
        assert_eq!(
            hex(&record.signing_bytes()),
            text(v, "signing_bytes"),
            "{w}: signing_bytes"
        );
        assert_eq!(
            hex(&record.signature),
            text(v, "signature"),
            "{w}: signature"
        );
        let signer =
            SigningKey::from_bytes(&unhex32(text(any_key(&doc, text(v, "signer")), "seed")));
        let resigned: Signature = signer.sign(&record.signing_bytes());
        assert_eq!(
            hex(&resigned.to_bytes()),
            text(v, "signature"),
            "{w}: re-signed signature"
        );
        assert_eq!(
            serde_json::to_value(&record).unwrap(),
            v["json"],
            "{w}: serde output"
        );

        let state = &states[text(v, "log")];
        let got = record_authorised(&record, Some(state));
        match (text(v, "expected"), got) {
            ("authorised", RecordAuthority::Authorised) => {}
            ("need_log", RecordAuthority::NeedLog { claimed, held }) => {
                assert_eq!(claimed.index, v["claimed_index"].as_u64().unwrap(), "{w}");
                assert_eq!(held, v["held_index"].as_u64().unwrap(), "{w}");
            }
            ("refused", RecordAuthority::Refused(refusal)) => {
                assert_eq!(refusal.reason(), text(v, "reason"), "{w}");
            }
            (want, got) => panic!("{w}: expected {want}, got {got:?}"),
        }
    }
}

#[test]
fn a_name_without_a_log_is_authorised_exactly_when_verify_passes() {
    let doc = load();
    for v in section(&doc, "records")
        .iter()
        .chain(section(&doc, "invalid_records").iter())
    {
        let w = text(v, "label");
        let record: NameRecord = serde_json::from_value(v["json"].clone()).unwrap();
        assert_eq!(
            record_authorised(&record, None) == RecordAuthority::Authorised,
            record.verify().is_ok(),
            "{w}"
        );
    }
}
