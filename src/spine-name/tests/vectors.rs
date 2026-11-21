//! Checks `spine-name` against `docs/spec/02-names.vectors.json`.
//!
//! The vectors are produced by `scripts/names_v1.py`, an independent Python
//! implementation written from the specification. Everything here uses the
//! public API only. Record layout is pinned two ways: `signing_bytes()` is
//! public and compared byte for byte, and because Ed25519 is deterministic the
//! signature produced by re-signing with the vector's seed must equal the
//! vector's signature.

use std::path::Path;

use ed25519_dalek::SigningKey;
use serde_json::Value;
use spine_name::{
    base32, content_hash, resource_id, verify_resource, Authority, NameError, NameKey, NameRecord,
    Rel, SpineUri,
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
        assert_eq!(
            record.verify(),
            Err(NameError::BadSignature),
            "{w}: must be rejected"
        );
    }
}
