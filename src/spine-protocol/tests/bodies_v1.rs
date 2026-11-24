//! The spine-wire v1 bodies, pinned down.
//!
//! * a table with every command (21) and a result for each, round-tripped in
//!   both directions;
//! * `COMMAND_NAMES` agrees with what the encoder writes;
//! * three bodies whose bytes were worked out by hand and cross-checked by an
//!   independent encoder (below);
//! * the rejection tests: every way a body can be wrong, and the code each one
//!   is answered with;
//! * the error codes.
//!
//! # The independent check
//!
//! The three hex literals in [`hand_checked_bodies`] were produced by a Python
//! script that shares no code with this crate. Its whole encoder, so that the
//! literals can be re-derived without it:
//!
//! ```text
//! def head(major, n):                    # major type, argument
//!     if n < 24:      return bytes([major << 5 | n])
//!     if n < 1 << 8:  return bytes([major << 5 | 24, n])
//!     if n < 1 << 16: return bytes([major << 5 | 25]) + n.to_bytes(2, 'big')
//!     if n < 1 << 32: return bytes([major << 5 | 26]) + n.to_bytes(4, 'big')
//!     return bytes([major << 5 | 27]) + n.to_bytes(8, 'big')
//!
//! def enc(v):
//!     if isinstance(v, int):   return head(0, v)              # unsigned
//!     if isinstance(v, bytes): return head(2, len(v)) + v     # byte string
//!     if isinstance(v, str):
//!         b = v.encode('utf-8'); return head(3, len(b)) + b   # text string
//!     if isinstance(v, dict):                                 # map, keys sorted
//!         items = sorted((enc(k), enc(x)) for k, x in v.items())   # by encoding
//!         return head(5, len(items)) + b''.join(k + x for k, x in items)
//!
//! enc('get_ur')
//! enc({'read_resource': {'hash': bytes([0x11]) * 32, 'offset': 0, 'max_bytes': 16}})
//! enc({'err': {'code': 'overloaded', 'message': 'm'}})
//! ```
//!
//! Reading the second one by hand: `a1` is a map of one entry; `6d` a text
//! string of 13 bytes (`read_resource`); `a3` a map of three; `64 hash` the
//! key `hash` (head `64`, 4 bytes) with `58 20` a byte string of 32 (one
//! length byte follows, because 32 does not fit the 5-bit argument); `66
//! offset` and `00`; `69 max_bytes` and `10` (16). The keys come out as hash,
//! offset, max_bytes because their encoded forms sort by the first byte
//! (`64` < `66` < `69`).

use ciborium::Value as Cbor;
use serde_json::{json, Value};
use spine_protocol::body::{
    Reply, WireError, ALL_ERROR_CODES, ERR_INVALID_REQUEST, ERR_OVERLOADED, ERR_UNKNOWN_COMMAND,
    PROTOCOL_ERROR_CODES,
};
use spine_protocol::connection::Connection;
use spine_protocol::frame::FrameType;
use spine_protocol::native::{self, NativeError};
use spine_protocol::resource::ResourceChunk;
use spine_protocol::{
    cbor, BrowserCommand, Capability, CapabilityAdvertisement, CapabilityQuery, CapabilitySelector,
    DecodeHints, Instruction, Message, NameProvenance, NameResolution, SpineBinary, StreamCancel,
    StreamData, StreamEnd, StreamEndReason, StreamRole, StreamStart, StreamToken, StreamUsage,
    ToolCall, ToolOutcome, TraceContext, COMMAND_NAMES,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;
use tokio::io::{duplex, DuplexStream};

const LIMIT: Duration = Duration::from_secs(30);

/// Bounds a future so that a hang fails the test.
async fn within<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(LIMIT, future)
        .await
        .expect("operation timed out")
}

// ========== BUILDING BODIES BY HAND ==========

fn t(s: &str) -> Cbor {
    Cbor::Text(s.into())
}

fn int(n: u64) -> Cbor {
    Cbor::Integer(n.into())
}

fn bytes_of(len: usize, byte: u8) -> Cbor {
    Cbor::Bytes(vec![byte; len])
}

fn map(entries: Vec<(&str, Cbor)>) -> Cbor {
    Cbor::Map(entries.into_iter().map(|(k, v)| (t(k), v)).collect())
}

fn enc(value: &Cbor) -> Vec<u8> {
    cbor::to_vec(value).unwrap()
}

/// `{name: fields}`: the body of a command that has fields.
fn command_body(name: &str, fields: Cbor) -> Vec<u8> {
    enc(&map(vec![(name, fields)]))
}

fn hex(s: &str) -> Vec<u8> {
    assert_eq!(s.len() % 2, 0);
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn as_cbor(body: &[u8]) -> Cbor {
    cbor::from_slice(body).unwrap()
}

/// The command name a body claims: the text itself, or the only key.
fn command_name(body: &[u8]) -> String {
    match as_cbor(body) {
        Cbor::Text(name) => name,
        Cbor::Map(entries) => {
            assert_eq!(entries.len(), 1, "a command is a single-entry map");
            match &entries[0].0 {
                Cbor::Text(name) => name.clone(),
                other => panic!("the key is not text: {other:?}"),
            }
        }
        other => panic!("not a command shape: {other:?}"),
    }
}

// ========== THE TABLE ==========

fn sample_binary() -> SpineBinary {
    SpineBinary {
        instructions: vec![
            Instruction::Push(json!(1)),
            Instruction::Pop,
            Instruction::DefineElement {
                id: 1,
                tag: "div".into(),
            },
        ],
        data: vec![0, 1, 2, 255],
        render_start: 0,
        exported_functions: BTreeMap::from([("main".to_string(), 0usize)]),
        capabilities: vec!["dom".into()],
    }
}

fn sample_trace() -> TraceContext {
    TraceContext {
        trace_id: [0xab; 16],
        span_id: [0xcd; 8],
        flags: 1,
        state: vec![("vendor".into(), "value".into())],
    }
}

fn sample_record() -> Value {
    json!({"name": "spine://example", "expires": 1_900_000_000u64, "endpoints": ["a", "b"]})
}

/// `n` succession entries of the shape of `02-names.md` 10.1. The values are
/// not real keys or signatures: the protocol layer checks shape only.
fn sample_log(n: u64) -> Vec<Value> {
    (1..=n)
        .map(|index| {
            let prev = if index == 1 {
                Value::Null
            } else {
                json!("b".repeat(52))
            };
            json!({
                "inception_key": "a".repeat(52),
                "index": index,
                "prev": prev,
                "kind": "rotate",
                "key": "c".repeat(52),
                "commitment": "d".repeat(52),
                "effective_at": 1_700_000_000u64,
                "signature": "e".repeat(103),
            })
        })
        .collect()
}

/// One command per entry of `COMMAND_NAMES`, in that order, with a typical
/// `ok` value for its reply. The names are written out here and checked
/// against both the encoder and `COMMAND_NAMES`.
fn table() -> Vec<(&'static str, BrowserCommand, Value)> {
    vec![
        (
            "navigate",
            BrowserCommand::Navigate {
                url: "https://example.com/".into(),
            },
            json!({"url": "https://example.com/", "status": 200}),
        ),
        ("get_ur", BrowserCommand::GetUR, json!({"nodes": [1, 2, 3]})),
        (
            "get_raw_html",
            BrowserCommand::GetRawHTML,
            json!("<html></html>"),
        ),
        (
            "execute_binary",
            BrowserCommand::ExecuteBinary(sample_binary()),
            json!({"rendered": true}),
        ),
        (
            "store_knowledge",
            BrowserCommand::StoreKnowledge {
                key: "k".into(),
                value: json!({"a": [1, 2.5, null, "x"]}),
                tags: vec!["t1".into(), "t2".into()],
            },
            json!({"stored": true}),
        ),
        (
            "query_knowledge",
            BrowserCommand::QueryKnowledge {
                query: "q".into(),
                tags: vec![],
                limit: 10,
            },
            json!([{"key": "k", "score": 0.5}]),
        ),
        (
            "get_binary_capabilities",
            BrowserCommand::GetBinaryCapabilities,
            json!(["dom", "net"]),
        ),
        (
            "resolve_name",
            BrowserCommand::ResolveName {
                name: "spine://example".into(),
                if_none_match: Some([7; 32]),
            },
            json!({"unchanged": {"ttl_secs": 60}}),
        ),
        (
            "resolve_names",
            BrowserCommand::ResolveNames {
                names: vec!["spine://a".into(), "spine://b".into()],
            },
            json!([{"not_found": {"name": "spine://a"}}]),
        ),
        (
            "find_providers",
            BrowserCommand::FindProviders {
                capability: "cap".into(),
                limit: Some(5),
            },
            json!([]),
        ),
        (
            "publish_name",
            BrowserCommand::PublishName {
                record: sample_record(),
            },
            json!({"published": true}),
        ),
        (
            "fetch_name",
            BrowserCommand::FetchName {
                name: "spine://example".into(),
                if_none_match: None,
            },
            json!({"content_type": "text/plain"}),
        ),
        (
            "crawl_names",
            BrowserCommand::CrawlNames {
                seed: "spine://seed".into(),
                max_depth: Some(3),
                max_visits: Some(100),
            },
            json!({"visited": 3}),
        ),
        (
            "publish_resource",
            BrowserCommand::PublishResource {
                record: sample_record(),
                content_type: "text/plain".into(),
                data: b"hello".to_vec(),
            },
            json!({"hash_hex": "00"}),
        ),
        (
            "read_resource",
            BrowserCommand::ReadResource {
                hash: [0x11; 32],
                offset: 0,
                max_bytes: 16,
            },
            serde_json::to_value(ResourceChunk {
                hash: [0x11; 32],
                offset: 0,
                total_bytes: 5,
                content_type: "text/plain".into(),
                data: b"hello".to_vec(),
                complete: true,
            })
            .unwrap(),
        ),
        (
            "namespace_status",
            BrowserCommand::NamespaceStatus,
            json!({"connected": false}),
        ),
        (
            "mesh_call",
            BrowserCommand::MeshCall {
                peer_id: "00000000-0000-0000-0000-000000000001".into(),
                peer_key: [9; 32],
                payload: json!({"op": "status"}),
            },
            json!({"result": "ok"}),
        ),
        (
            "tool_call",
            BrowserCommand::ToolCall(ToolCall {
                name: "spine.content/hash".into(),
                args: json!({"text": "abc"}),
                trace: Some(sample_trace()),
            }),
            json!({"hash": "ba7816bf"}),
        ),
        (
            "capability_query",
            BrowserCommand::CapabilityQuery(CapabilityQuery {
                selector: CapabilitySelector::Prefix("spine:cap/web/".into()),
            }),
            serde_json::to_value(CapabilityAdvertisement {
                agent_id: "agent".into(),
                capabilities: vec![],
            })
            .unwrap(),
        ),
        (
            "delete_resource",
            BrowserCommand::DeleteResource {
                hash: [0x11; 32],
                owner: [0x22; 32],
                proof: [0x33; 64],
            },
            json!({"deleted": true}),
        ),
        (
            "delete_knowledge",
            BrowserCommand::DeleteKnowledge { key: "k".into() },
            json!({"deleted": 2}),
        ),
    ]
}

/// More shapes of the same commands: optional fields present and absent,
/// every selector, edge values.
fn variants() -> Vec<BrowserCommand> {
    vec![
        BrowserCommand::Navigate { url: String::new() },
        BrowserCommand::Navigate {
            url: "héllo ✓ \u{1F980}".into(),
        },
        BrowserCommand::ExecuteBinary(SpineBinary {
            instructions: vec![],
            data: vec![],
            render_start: 0,
            exported_functions: BTreeMap::new(),
            capabilities: vec![],
        }),
        BrowserCommand::StoreKnowledge {
            key: String::new(),
            value: Value::Null,
            tags: vec![],
        },
        BrowserCommand::ResolveName {
            name: "n".into(),
            if_none_match: None,
        },
        BrowserCommand::FindProviders {
            capability: "c".into(),
            limit: None,
        },
        BrowserCommand::CrawlNames {
            seed: "s".into(),
            max_depth: None,
            max_visits: None,
        },
        BrowserCommand::CrawlNames {
            seed: "s".into(),
            max_depth: Some(0),
            max_visits: None,
        },
        BrowserCommand::CrawlNames {
            seed: "s".into(),
            max_depth: None,
            max_visits: Some(0),
        },
        BrowserCommand::PublishResource {
            record: json!({}),
            content_type: String::new(),
            data: vec![],
        },
        BrowserCommand::PublishResource {
            record: json!({}),
            content_type: "application/octet-stream".into(),
            data: (0..=255u8).collect(),
        },
        BrowserCommand::ReadResource {
            hash: [0; 32],
            offset: u64::MAX,
            max_bytes: u32::MAX,
        },
        BrowserCommand::ToolCall(ToolCall {
            name: "t".into(),
            args: json!(null),
            trace: None,
        }),
        BrowserCommand::CapabilityQuery(CapabilityQuery {
            selector: CapabilitySelector::Exact("spine:cap/web/scraping".into()),
        }),
        BrowserCommand::CapabilityQuery(CapabilityQuery {
            selector: CapabilitySelector::Semantic {
                embedding: vec![0.25, -1.5, 0.0, 1024.0],
                top_k: 4,
            },
        }),
        BrowserCommand::CapabilityQuery(CapabilityQuery {
            selector: CapabilitySelector::All,
        }),
    ]
}

#[test]
fn the_table_has_every_command_in_command_names_order() {
    let table = table();
    assert_eq!(table.len(), 21);
    assert_eq!(table.len(), COMMAND_NAMES.len());
    for (i, (name, _, _)) in table.iter().enumerate() {
        assert_eq!(*name, COMMAND_NAMES[i], "entry {i}");
    }
}

/// `COMMAND_NAMES` is what the encoder writes, in declaration order, with no
/// duplicates; and it is the daemon's whole vocabulary: the text of each name
/// is a command when it is a unit command, `invalid_request` otherwise, and
/// never `unknown_command`.
#[test]
fn command_names_agree_with_the_encoder() {
    let mut seen = std::collections::HashSet::new();
    for name in COMMAND_NAMES {
        assert!(seen.insert(name), "duplicate {name}");
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{name} is not snake_case"
        );
    }
    for (name, command, _) in table() {
        let body = native::encode_command(&command).unwrap();
        assert_eq!(command_name(&body), name);
        assert!(COMMAND_NAMES.contains(&name));
    }
    for command in variants() {
        let body = native::encode_command(&command).unwrap();
        assert!(COMMAND_NAMES.contains(&command_name(&body).as_str()));
    }

    let unit = [
        "get_ur",
        "get_raw_html",
        "get_binary_capabilities",
        "namespace_status",
    ];
    for name in COMMAND_NAMES {
        let body = enc(&t(name));
        match native::decode_command(&body) {
            Ok(command) => {
                assert!(unit.contains(&name), "{name} decoded from bare text");
                assert_eq!(native::encode_command(&command).unwrap(), body);
            }
            Err(error) => {
                assert!(!unit.contains(&name), "{name}: {error}");
                assert_eq!(error.code, ERR_INVALID_REQUEST, "{name}: {error}");
            }
        }
    }
}

#[test]
fn every_command_roundtrips_both_ways() {
    let commands = table()
        .into_iter()
        .map(|(_, command, _)| command)
        .chain(variants());
    for command in commands {
        // value -> bytes -> value
        let body = native::encode_command(&command).unwrap();
        cbor::check_deterministic(&body).unwrap();
        assert_eq!(
            native::decode_command(&body),
            Ok(command.clone()),
            "{command:?}"
        );
        let direct: BrowserCommand = cbor::from_slice(&body).unwrap();
        assert_eq!(direct, command);
        // bytes -> value -> bytes
        let again = native::encode_command(&direct).unwrap();
        assert_eq!(again, body, "{command:?}");
        // The shape: a unit command is its name, anything else a one-entry map.
        match (&command, as_cbor(&body)) {
            (
                BrowserCommand::GetUR
                | BrowserCommand::GetRawHTML
                | BrowserCommand::GetBinaryCapabilities
                | BrowserCommand::NamespaceStatus,
                Cbor::Text(_),
            ) => {}
            (_, Cbor::Map(entries)) => assert_eq!(entries.len(), 1),
            (other, shape) => panic!("{other:?} encoded as {shape:?}"),
        }
        // JSON (trace files, the gateway) carries the same value.
        let json = serde_json::to_string(&command).unwrap();
        let from_json: BrowserCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(from_json, command, "{json}");
    }
}

#[test]
fn optional_fields_are_omitted_not_null() {
    // Compared as bytes: the encoder sorts map keys, so a tree built in
    // reading order would not equal the decoded one entry for entry.
    let none = BrowserCommand::FindProviders {
        capability: "c".into(),
        limit: None,
    };
    assert_eq!(
        native::encode_command(&none).unwrap(),
        command_body("find_providers", map(vec![("capability", t("c"))]))
    );
    let some = BrowserCommand::FindProviders {
        capability: "c".into(),
        limit: Some(5),
    };
    assert_eq!(
        native::encode_command(&some).unwrap(),
        command_body(
            "find_providers",
            map(vec![("capability", t("c")), ("limit", int(5))])
        )
    );
    // An absent if_none_match is no key; a present one is a 32-byte string.
    assert_eq!(
        native::encode_command(&BrowserCommand::ResolveName {
            name: "n".into(),
            if_none_match: None,
        })
        .unwrap(),
        command_body("resolve_name", map(vec![("name", t("n"))]))
    );
    assert_eq!(
        native::encode_command(&BrowserCommand::ResolveName {
            name: "n".into(),
            if_none_match: Some([3; 32]),
        })
        .unwrap(),
        command_body(
            "resolve_name",
            map(vec![("name", t("n")), ("if_none_match", bytes_of(32, 3))])
        )
    );
}

#[test]
fn every_ok_and_err_shape_roundtrips_both_ways() {
    let mut replies: Vec<Reply<Value>> = table()
        .into_iter()
        .map(|(_, _, ok)| Reply::Ok(ok))
        .collect();
    replies.extend([
        Reply::Ok(Value::Null),
        Reply::Ok(json!(0)),
        Reply::Ok(json!(-1)),
        Reply::Ok(json!(u64::MAX)),
        Reply::Ok(json!(1.5)),
        Reply::Ok(json!("")),
        Reply::Ok(json!([])),
        Reply::Ok(json!({})),
        Reply::Err(WireError::new("no_page_loaded", "no page")),
        Reply::Err(WireError::new("x", "")),
        Reply::Err(WireError::new("rate_limit", "slow").with_details(json!({"retry_ms": 250}))),
        Reply::Err(WireError::new("c", "m").with_details(json!([1, "two", [3]]))),
        Reply::Err(WireError::new("c", "m").with_details(json!(false))),
        Reply::Err(WireError::new("c", "héllo ✓").with_details(json!("text"))),
    ]);
    for reply in replies {
        let body = cbor::to_vec(&reply).unwrap();
        cbor::check_deterministic(&body).unwrap();
        // The only top-level key is `ok` or `err`.
        match (&reply, as_cbor(&body)) {
            (Reply::Ok(_), Cbor::Map(entries)) => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].0, t("ok"));
            }
            (Reply::Err(error), Cbor::Map(entries)) => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].0, t("err"));
                let Cbor::Map(fields) = &entries[0].1 else {
                    panic!("err is not a map")
                };
                assert_eq!(fields.len(), if error.details.is_some() { 3 } else { 2 });
            }
            (_, shape) => panic!("{reply:?} encoded as {shape:?}"),
        }
        let back = native::decode_reply::<Value>(&body).unwrap();
        assert_eq!(back, reply);
        assert_eq!(cbor::to_vec(&back).unwrap(), body);
        // error_body is the same bytes as encoding the Reply.
        if let Reply::Err(error) = &reply {
            assert_eq!(native::error_body(error).unwrap(), body);
        }
        // JSON, for trace files.
        let json = serde_json::to_string(&reply).unwrap();
        assert_eq!(serde_json::from_str::<Reply<Value>>(&json).unwrap(), reply);
    }
}

/// The typed results decode from the same bytes as the JSON-shaped ones,
/// except where a result carries byte strings.
#[test]
fn typed_results_agree_with_the_json_shaped_ones() {
    let chunk = ResourceChunk {
        hash: [0x22; 32],
        offset: 4,
        total_bytes: 9,
        content_type: "text/plain".into(),
        data: b"abc".to_vec(),
        complete: false,
    };
    let advertisement = CapabilityAdvertisement {
        agent_id: "agent".into(),
        capabilities: vec![Capability {
            uri: "spine:cap/x".into(),
            description: "x".into(),
            input_schema: json!({"type": "object"}),
            output_schema: json!({"type": "object"}),
            embedding: Some(vec![0.5, -0.25]),
        }],
    };
    let resolutions = [
        NameResolution::Resolved {
            record: sample_record(),
            provenance: NameProvenance::Network,
            log: Vec::new(),
        },
        NameResolution::Resolved {
            record: sample_record(),
            provenance: NameProvenance::Local,
            log: sample_log(2),
        },
        NameResolution::Unchanged { ttl_secs: 30 },
        NameResolution::NotFound { name: "n".into() },
        NameResolution::Invalid {
            name: "n".into(),
            reason: "r".into(),
        },
        NameResolution::Unavailable {
            name: "n".into(),
            reason: "r".into(),
        },
        NameResolution::Retired {
            name: "n".into(),
            log: sample_log(1),
        },
        NameResolution::Compromised {
            name: "n".into(),
            logs: vec![sample_log(1), sample_log(2)],
        },
        NameResolution::Revoked {
            record: sample_record(),
            provenance: NameProvenance::Cache,
            log: sample_log(3),
        },
    ];

    fn check<T>(value: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let body = cbor::to_vec(&Reply::Ok(value)).unwrap();
        let typed = native::decode_reply::<T>(&body).unwrap();
        assert_eq!(typed.into_result().unwrap(), *value);
        // Read as JSON-shaped, then written again: the same bytes.
        let loose = native::decode_reply::<Value>(&body).unwrap();
        assert_eq!(cbor::to_vec(&loose).unwrap(), body);
    }
    // A result with byte-string fields has no JSON-shaped reading: CBOR byte
    // strings are not a JSON value, so such results are only read typed.
    let body = cbor::to_vec(&Reply::Ok(&chunk)).unwrap();
    let typed = native::decode_reply::<ResourceChunk>(&body).unwrap();
    assert_eq!(typed.into_result().unwrap(), chunk);
    assert!(native::decode_reply::<Value>(&body).is_err());
    check(&advertisement);
    for resolution in &resolutions {
        check(resolution);
    }
    check(&vec![resolutions[1].clone(), resolutions[3].clone()]);
}

/// The answer to a name resolution is read strictly: the log is always there,
/// at most 64 entries, each a closed map, `compromised` has exactly two logs
/// and `retired` at least one entry (`docs/spec/00-wire.md` 12.5).
#[test]
fn name_resolutions_are_read_strictly() {
    fn reads(value: Value) -> bool {
        // As the daemon sends it and a client reads it: a JSON-shaped value,
        // written as deterministic CBOR, read back as the typed result.
        let body = cbor::to_vec(&Reply::Ok(&value)).unwrap();
        native::decode_reply::<NameResolution>(&body).is_ok()
    }
    let record = sample_record();
    let resolved = |log: Value| json!({"resolved": {"record": record.clone(), "provenance": "local", "log": log}});
    assert!(reads(resolved(json!([]))));
    assert!(reads(resolved(Value::Array(sample_log(64)))));
    assert!(
        !reads(resolved(Value::Array(sample_log(65)))),
        "an over-long log"
    );
    assert!(
        !reads(json!({"resolved": {"record": record.clone(), "provenance": "local"}})),
        "a resolved answer without its log"
    );
    assert!(
        !reads(json!({"resolved": {"record": record.clone(), "provenance": "local", "log": null}})),
        "a null log is not an empty one"
    );
    assert!(
        !reads(
            json!({"resolved": {"record": record.clone(), "provenance": "local", "log": [], "extra": 1}})
        ),
        "an unknown key"
    );
    assert!(!reads(resolved(json!("log"))), "a log of the wrong type");
    assert!(!reads(resolved(json!([1, 2]))), "entries that are not maps");
    let mut extra_field = sample_log(1);
    extra_field[0]["extra"] = json!(1);
    assert!(!reads(resolved(Value::Array(extra_field))));

    let retired = |log: Value| json!({"retired": {"name": "n", "log": log}});
    assert!(reads(retired(Value::Array(sample_log(1)))));
    assert!(!reads(retired(json!([]))), "a retirement shows its entry");
    assert!(!reads(retired(Value::Array(sample_log(65)))));
    assert!(!reads(json!({"retired": {"name": "n"}})));
    assert!(!reads(json!({"retired": {"log": sample_log(1)}})));

    let compromised = |logs: Value| json!({"compromised": {"name": "n", "logs": logs}});
    let one = Value::Array(sample_log(1));
    let two = Value::Array(sample_log(2));
    assert!(reads(compromised(json!([one.clone(), two.clone()]))));
    assert!(!reads(compromised(json!([]))), "no logs");
    assert!(!reads(compromised(json!([one.clone()]))), "one log");
    assert!(
        !reads(compromised(json!([one.clone(), two.clone(), one.clone()]))),
        "three logs"
    );
    assert!(
        !reads(compromised(json!([one.clone(), []]))),
        "an empty second log"
    );
    assert!(!reads(compromised(json!([
        one.clone(),
        Value::Array(sample_log(65))
    ]))));
    assert!(
        !reads(json!({"compromised": {"name": "n", "log": [one.clone(), two.clone()]}})),
        "the key is logs"
    );

    let revoked = |log: Value| json!({"revoked": {"record": record.clone(), "provenance": "cache", "log": log}});
    assert!(reads(revoked(json!([]))));
    assert!(reads(revoked(Value::Array(sample_log(3)))));
    assert!(!reads(revoked(Value::Array(sample_log(65)))));
    assert!(!reads(
        json!({"revoked": {"record": record.clone(), "provenance": "cache"}})
    ));
    assert!(!reads(json!({"revoked": {"record": record, "log": []}})));

    // The sender's check is the decoder's.
    let too_long = NameResolution::Resolved {
        record: sample_record(),
        provenance: NameProvenance::Local,
        log: sample_log(65),
    };
    assert!(too_long.validate().is_err());
    let one_log = NameResolution::Compromised {
        name: "n".into(),
        logs: vec![sample_log(1)],
    };
    assert!(one_log.validate().is_err());
    let no_entry = NameResolution::Retired {
        name: "n".into(),
        log: Vec::new(),
    };
    assert!(no_entry.validate().is_err());
    assert!(NameResolution::Unchanged { ttl_secs: 1 }.validate().is_ok());
}

#[test]
fn tool_outcomes_map_to_replies_and_back() {
    let ok = ToolOutcome::Ok {
        content: json!({"a": 1}),
    };
    assert_eq!(ok.clone().into_reply(), Reply::Ok(json!({"a": 1})));
    assert_eq!(ToolOutcome::from_reply(ok.clone().into_reply()), ok);
    let err = ToolOutcome::Err {
        code: "invalid_args".into(),
        message: "bad".into(),
        details: Some(json!({"field": "text"})),
    };
    assert_eq!(
        err.clone().into_reply(),
        Reply::Err(WireError::new("invalid_args", "bad").with_details(json!({"field": "text"})))
    );
    assert_eq!(ToolOutcome::from_reply(err.clone().into_reply()), err);
}

#[test]
fn stream_and_agentic_bodies_roundtrip_through_the_stream_codec() {
    let messages = [
        Message::StreamStart(StreamStart {
            role: StreamRole::Assistant,
            model: "m".into(),
            trace: Some(sample_trace()),
            decode_hints: Some(DecodeHints {
                temperature: Some(0.5),
                top_k: Some(40),
                stop_sequences: vec!["\n\n".into()],
                ..DecodeHints::default()
            }),
        }),
        Message::StreamStart(StreamStart {
            role: StreamRole::Tool,
            model: String::new(),
            trace: None,
            decode_hints: None,
        }),
        Message::StreamToken(StreamToken {
            seq: 0,
            data: StreamData::Text("hi".into()),
            usage: None,
        }),
        Message::StreamToken(StreamToken {
            seq: u64::MAX,
            data: StreamData::Bytes(vec![0, 1, 2, 255]),
            usage: Some(StreamUsage {
                input_tokens: 1,
                output_tokens: 2,
            }),
        }),
        Message::StreamToken(StreamToken {
            seq: 3,
            data: StreamData::ToolCall(ToolCall {
                name: "t".into(),
                args: json!({"a": 1}),
                trace: None,
            }),
            usage: None,
        }),
        Message::StreamEnd(StreamEnd {
            reason: StreamEndReason::Stop,
            usage: None,
        }),
        Message::StreamEnd(StreamEnd {
            reason: StreamEndReason::Error {
                code: "internal".into(),
                message: "boom".into(),
            },
            usage: Some(StreamUsage {
                input_tokens: 5,
                output_tokens: 6,
            }),
        }),
        Message::StreamCancel(StreamCancel { reason: None }),
        Message::StreamCancel(StreamCancel {
            reason: Some("user aborted".into()),
        }),
    ];
    for message in messages {
        let (frame_type, body) = native::encode_stream_body(&message).unwrap();
        assert!(matches!(
            frame_type,
            FrameType::StreamStart
                | FrameType::StreamToken
                | FrameType::StreamEnd
                | FrameType::StreamCancel
        ));
        assert_eq!(
            native::decode_stream_body(frame_type, &body).unwrap(),
            message
        );
    }
    // Only stream messages travel in stream frames, and only stream frames
    // decode as stream bodies.
    assert!(native::encode_stream_body(&Message::Ping { timestamp: 1 }).is_err());
    assert!(native::decode_stream_body(FrameType::Request, &[0x00]).is_err());
}

// ========== HAND-CHECKED BODIES ==========

#[test]
fn hand_checked_bodies() {
    // 1. A unit command is the text string of its name: `66` (text, 6 bytes)
    //    then "get_ur".
    const GET_UR: &str = "666765745f7572";
    assert_eq!(
        native::encode_command(&BrowserCommand::GetUR).unwrap(),
        hex(GET_UR)
    );
    assert_eq!(
        native::decode_command(&hex(GET_UR)),
        Ok(BrowserCommand::GetUR)
    );

    // 2. read_resource: a map of one entry whose value is a map of three, with
    //    a 32-byte byte string, a zero and a 16.
    const READ_RESOURCE: &str = concat!(
        "a1",                           // map(1)
        "6d726561645f7265736f75726365", // text(13) "read_resource"
        "a3",                           // map(3)
        "6468617368",                   // text(4) "hash"
        "5820",                         // bytes(32)
        "1111111111111111111111111111111111111111111111111111111111111111",
        "666f6666736574",       // text(6) "offset"
        "00",                   // 0
        "696d61785f6279746573", // text(9) "max_bytes"
        "10",                   // 16
    );
    let read_resource = BrowserCommand::ReadResource {
        hash: [0x11; 32],
        offset: 0,
        max_bytes: 16,
    };
    assert_eq!(
        native::encode_command(&read_resource).unwrap(),
        hex(READ_RESOURCE)
    );
    assert_eq!(
        native::decode_command(&hex(READ_RESOURCE)),
        Ok(read_resource)
    );

    // 3. An err reply: `{err: {code, message}}`, code before message.
    const ERR_OVERLOADED_BODY: &str = concat!(
        "a1",                     // map(1)
        "63657272",               // text(3) "err"
        "a2",                     // map(2)
        "64636f6465",             // text(4) "code"
        "6a6f7665726c6f61646564", // text(10) "overloaded"
        "676d657373616765",       // text(7) "message"
        "616d",                   // text(1) "m"
    );
    let error = WireError::new(ERR_OVERLOADED, "m");
    assert_eq!(
        native::error_body(&error).unwrap(),
        hex(ERR_OVERLOADED_BODY)
    );
    assert_eq!(
        native::decode_reply::<Value>(&hex(ERR_OVERLOADED_BODY)).unwrap(),
        Reply::Err(error)
    );
}

// ========== REJECTION ==========

/// A valid `read_resource` body as a tree, to damage one part at a time.
fn read_resource_fields() -> Vec<(&'static str, Cbor)> {
    vec![
        ("hash", bytes_of(32, 0x11)),
        ("offset", int(0)),
        ("max_bytes", int(16)),
    ]
}

fn raw(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

#[test]
fn the_good_read_resource_body_decodes() {
    // The builder makes what the encoder makes, so the damaged bodies below
    // differ from a good one only where they say they do.
    let body = command_body("read_resource", map(read_resource_fields()));
    assert_eq!(
        native::decode_command(&body),
        Ok(BrowserCommand::ReadResource {
            hash: [0x11; 32],
            offset: 0,
            max_bytes: 16
        })
    );
}

/// A valid `delete_resource` body as a tree, to damage one part at a time.
fn delete_resource_fields() -> Vec<(&'static str, Cbor)> {
    vec![
        ("hash", bytes_of(32, 0x11)),
        ("owner", bytes_of(32, 0x22)),
        ("proof", bytes_of(64, 0x33)),
    ]
}

#[test]
fn delete_commands_are_read_strictly() {
    let good = command_body("delete_resource", map(delete_resource_fields()));
    assert_eq!(
        native::decode_command(&good),
        Ok(BrowserCommand::DeleteResource {
            hash: [0x11; 32],
            owner: [0x22; 32],
            proof: [0x33; 64],
        })
    );
    let with = |field: &str, value: Option<Cbor>| {
        let mut fields = delete_resource_fields();
        match value {
            Some(value) => {
                for entry in fields.iter_mut() {
                    if entry.0 == field {
                        entry.1 = value.clone();
                    }
                }
            }
            None => fields.retain(|entry| entry.0 != field),
        }
        command_body("delete_resource", map(fields))
    };
    for (what, body) in [
        (
            "a proof of 63 bytes",
            with("proof", Some(bytes_of(63, 0x33))),
        ),
        (
            "a proof of 65 bytes",
            with("proof", Some(bytes_of(65, 0x33))),
        ),
        (
            "an owner of 31 bytes",
            with("owner", Some(bytes_of(31, 0x22))),
        ),
        ("a hash of 33 bytes", with("hash", Some(bytes_of(33, 0x11)))),
        ("a hash as text", with("hash", Some(t("11")))),
        (
            "a proof as an array of integers",
            with(
                "proof",
                Some(Cbor::Array((0..64).map(|_| int(3)).collect())),
            ),
        ),
        ("no proof", with("proof", None)),
        ("no owner", with("owner", None)),
        ("no hash", with("hash", None)),
        (
            "an extra field",
            command_body("delete_resource", {
                let mut fields = delete_resource_fields();
                fields.push(("extra", int(1)));
                map(fields)
            }),
        ),
        ("a bare name", enc(&t("delete_resource"))),
        ("a bare knowledge name", enc(&t("delete_knowledge"))),
        (
            "a knowledge key that is not text",
            command_body("delete_knowledge", map(vec![("key", int(1))])),
        ),
        (
            "a knowledge delete with no key",
            command_body("delete_knowledge", map(vec![])),
        ),
        (
            "a knowledge delete with a tag list",
            command_body(
                "delete_knowledge",
                map(vec![("key", t("k")), ("tags", Cbor::Array(vec![]))]),
            ),
        ),
    ] {
        let error = native::decode_command(&body).expect_err(what);
        assert_eq!(error.code, ERR_INVALID_REQUEST, "{what}");
    }
}

#[test]
fn bad_requests_are_answered_with_the_prescribed_code() {
    let with = |field: &str, value: Cbor| {
        let mut fields = read_resource_fields();
        for entry in fields.iter_mut() {
            if entry.0 == field {
                entry.1 = value.clone();
            }
        }
        command_body("read_resource", map(fields))
    };
    let name = b"read_resource";
    let good_prefix = raw(&[&[0xa1, 0x6d], name]);

    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        // ---- a command that is not in the set: unknown_command
        (
            "unknown name as text",
            enc(&t("frobnicate")),
            ERR_UNKNOWN_COMMAND,
        ),
        (
            "unknown name as a map key, empty fields",
            command_body("frobnicate", map(vec![])),
            ERR_UNKNOWN_COMMAND,
        ),
        (
            "unknown name as a map key, odd payload",
            command_body("frobnicate", int(1)),
            ERR_UNKNOWN_COMMAND,
        ),
        (
            "a v0 name",
            command_body("Navigate", map(vec![("url", t("u"))])),
            ERR_UNKNOWN_COMMAND,
        ),
        // ---- a known name with the wrong shape: invalid_request
        (
            "unit command with a map payload",
            command_body("get_ur", map(vec![])),
            ERR_INVALID_REQUEST,
        ),
        (
            "unit command with a null payload",
            command_body("get_ur", Cbor::Null),
            ERR_INVALID_REQUEST,
        ),
        (
            "unit command with a payload that has fields",
            command_body("namespace_status", map(vec![("x", int(1))])),
            ERR_INVALID_REQUEST,
        ),
        (
            "a command with fields, as bare text",
            enc(&t("navigate")),
            ERR_INVALID_REQUEST,
        ),
        (
            "unknown field",
            command_body("navigate", map(vec![("url", t("u")), ("extra", int(1))])),
            ERR_INVALID_REQUEST,
        ),
        (
            "missing field",
            command_body("navigate", map(vec![])),
            ERR_INVALID_REQUEST,
        ),
        (
            "field of the wrong type",
            command_body("navigate", map(vec![("url", int(1))])),
            ERR_INVALID_REQUEST,
        ),
        (
            "fields as an array",
            command_body("navigate", Cbor::Array(vec![t("u")])),
            ERR_INVALID_REQUEST,
        ),
        (
            "null for an optional list-less field (limit)",
            command_body(
                "find_providers",
                map(vec![("capability", t("c")), ("limit", Cbor::Null)]),
            ),
            ERR_INVALID_REQUEST,
        ),
        (
            "null for an optional byte field (if_none_match)",
            command_body(
                "resolve_name",
                map(vec![("name", t("n")), ("if_none_match", Cbor::Null)]),
            ),
            ERR_INVALID_REQUEST,
        ),
        (
            "null for max_depth",
            command_body(
                "crawl_names",
                map(vec![("seed", t("s")), ("max_depth", Cbor::Null)]),
            ),
            ERR_INVALID_REQUEST,
        ),
        (
            "null trace on a tool call",
            command_body(
                "tool_call",
                map(vec![
                    ("name", t("t")),
                    ("args", Cbor::Null),
                    ("trace", Cbor::Null),
                ]),
            ),
            ERR_INVALID_REQUEST,
        ),
        (
            "empty tracestate list",
            command_body(
                "tool_call",
                map(vec![
                    ("name", t("t")),
                    ("args", Cbor::Null),
                    (
                        "trace",
                        map(vec![
                            ("trace_id", bytes_of(16, 1)),
                            ("span_id", bytes_of(8, 1)),
                            ("flags", int(0)),
                            ("state", Cbor::Array(vec![])),
                        ]),
                    ),
                ]),
            ),
            ERR_INVALID_REQUEST,
        ),
        (
            "trace id of 15 bytes",
            command_body(
                "tool_call",
                map(vec![
                    ("name", t("t")),
                    ("args", Cbor::Null),
                    (
                        "trace",
                        map(vec![
                            ("trace_id", bytes_of(15, 1)),
                            ("span_id", bytes_of(8, 1)),
                            ("flags", int(0)),
                        ]),
                    ),
                ]),
            ),
            ERR_INVALID_REQUEST,
        ),
        (
            "unknown selector",
            command_body(
                "capability_query",
                map(vec![("selector", map(vec![("fuzzy", t("x"))]))]),
            ),
            ERR_INVALID_REQUEST,
        ),
        (
            "a selector that needs fields, as text",
            command_body("capability_query", map(vec![("selector", t("semantic"))])),
            ERR_INVALID_REQUEST,
        ),
        // ---- byte fields are byte strings, of the right length
        (
            "hash of 31 bytes",
            with("hash", bytes_of(31, 0x11)),
            ERR_INVALID_REQUEST,
        ),
        (
            "hash of 33 bytes",
            with("hash", bytes_of(33, 0x11)),
            ERR_INVALID_REQUEST,
        ),
        (
            "hash as an array of integers",
            with("hash", Cbor::Array((0..32).map(|_| int(17)).collect())),
            ERR_INVALID_REQUEST,
        ),
        (
            "hash as text",
            with("hash", t("11111111")),
            ERR_INVALID_REQUEST,
        ),
        (
            "data as an array of integers",
            command_body(
                "publish_resource",
                map(vec![
                    ("record", map(vec![])),
                    ("content_type", t("c")),
                    ("data", Cbor::Array(vec![int(1), int(2)])),
                ]),
            ),
            ERR_INVALID_REQUEST,
        ),
        // ---- numbers
        (
            "negative offset",
            with("offset", Cbor::Integer((-1i64).into())),
            ERR_INVALID_REQUEST,
        ),
        (
            "max_bytes over u32",
            with("max_bytes", int(1 << 32)),
            ERR_INVALID_REQUEST,
        ),
        (
            "offset as a float",
            with("offset", Cbor::Float(1.0)),
            ERR_INVALID_REQUEST,
        ),
        (
            "offset as text",
            with("offset", t("0")),
            ERR_INVALID_REQUEST,
        ),
        // ---- not a command at all
        ("an integer", enc(&int(7)), ERR_INVALID_REQUEST),
        ("null", enc(&Cbor::Null), ERR_INVALID_REQUEST),
        (
            "an array",
            enc(&Cbor::Array(vec![t("get_ur")])),
            ERR_INVALID_REQUEST,
        ),
        ("an empty map", enc(&map(vec![])), ERR_INVALID_REQUEST),
        (
            "two entries",
            enc(&map(vec![
                ("get_ur", map(vec![])),
                ("navigate", map(vec![])),
            ])),
            ERR_INVALID_REQUEST,
        ),
        (
            "a non-text key",
            enc(&Cbor::Map(vec![(int(1), map(vec![]))])),
            ERR_INVALID_REQUEST,
        ),
        ("an empty body", vec![], ERR_INVALID_REQUEST),
        // ---- not deterministic CBOR
        (
            "trailing bytes after a command",
            raw(&[&enc(&t("get_ur")), &[0x00]]),
            ERR_INVALID_REQUEST,
        ),
        (
            "an integer in a longer form than needed (offset 0 as 18 00)",
            raw(&[
                &good_prefix,
                &[0xa3, 0x64],
                b"hash",
                &[0x58, 0x20],
                &[0x11; 32],
                &[0x66],
                b"offset",
                &[0x18, 0x00],
                &[0x69],
                b"max_bytes",
                &[0x10],
            ]),
            ERR_INVALID_REQUEST,
        ),
        (
            "map keys out of order (offset before hash)",
            raw(&[
                &good_prefix,
                &[0xa3, 0x66],
                b"offset",
                &[0x00, 0x64],
                b"hash",
                &[0x58, 0x20],
                &[0x11; 32],
                &[0x69],
                b"max_bytes",
                &[0x10],
            ]),
            ERR_INVALID_REQUEST,
        ),
        (
            "a duplicate key",
            raw(&[
                &good_prefix,
                &[0xa3, 0x64],
                b"hash",
                &[0x58, 0x20],
                &[0x11; 32],
                &[0x64],
                b"hash",
                &[0x58, 0x20],
                &[0x11; 32],
                &[0x69],
                b"max_bytes",
                &[0x10],
            ]),
            ERR_INVALID_REQUEST,
        ),
        (
            "an indefinite-length map",
            vec![0xbf, 0xff],
            ERR_INVALID_REQUEST,
        ),
        (
            "an indefinite-length text string",
            vec![0x7f, 0x61, b'a', 0xff],
            ERR_INVALID_REQUEST,
        ),
        ("a tagged value", vec![0xc1, 0x00], ERR_INVALID_REQUEST),
        (
            "nesting deeper than the limit",
            raw(&[&[0x81; 40], &[0x00]]),
            ERR_INVALID_REQUEST,
        ),
    ];

    for (label, body, code) in cases {
        match native::decode_command(&body) {
            Ok(command) => panic!("{label}: accepted as {command:?}"),
            Err(error) => assert_eq!(error.code, code, "{label}: {error}"),
        }
        // The answer is itself a body a peer can read.
        let answer =
            native::error_body(&native::decode_command(&body).unwrap_err().to_wire()).unwrap();
        match native::decode_reply::<Value>(&answer).unwrap() {
            Reply::Err(wire) => assert_eq!(wire.code, code, "{label}"),
            other => panic!("{label}: {other:?}"),
        }
    }
}

#[test]
fn bad_replies_are_refused() {
    let err_fields = |extra: Vec<(&str, Cbor)>| {
        let mut fields = vec![("code", t("c")), ("message", t("m"))];
        fields.extend(extra);
        map(fields)
    };
    let bodies: Vec<(&str, Vec<u8>)> = vec![
        (
            "both ok and err",
            enc(&map(vec![("ok", int(1)), ("err", err_fields(vec![]))])),
        ),
        ("neither", enc(&map(vec![("maybe", int(1))]))),
        ("empty map", enc(&map(vec![]))),
        (
            "err without message",
            enc(&map(vec![("err", map(vec![("code", t("c"))]))])),
        ),
        (
            "err without code",
            enc(&map(vec![("err", map(vec![("message", t("m"))]))])),
        ),
        (
            "err with null details",
            enc(&map(vec![(
                "err",
                err_fields(vec![("details", Cbor::Null)]),
            )])),
        ),
        (
            "err with an unknown field",
            enc(&map(vec![("err", err_fields(vec![("extra", int(1))]))])),
        ),
        ("err that is text", enc(&map(vec![("err", t("boom"))]))),
        (
            "err with a non-text code",
            enc(&map(vec![(
                "err",
                map(vec![("code", int(5)), ("message", t("m"))]),
            )])),
        ),
        ("bare integer", enc(&int(7))),
        ("empty body", vec![]),
        (
            "trailing bytes after a reply",
            raw(&[&enc(&map(vec![("ok", int(1))])), &[0x00]]),
        ),
    ];
    for (label, body) in bodies {
        assert!(
            native::decode_reply::<Value>(&body).is_err(),
            "{label}: accepted"
        );
    }
    // A typed reply refuses the wrong shape too.
    let wrong = enc(&map(vec![("ok", map(vec![("hash", t("x"))]))]));
    assert!(native::decode_reply::<ResourceChunk>(&wrong).is_err());
}

#[test]
fn bad_stream_bodies_are_refused() {
    let cases: Vec<(&str, FrameType, Vec<u8>)> = vec![
        (
            "cancel with a null reason",
            FrameType::StreamCancel,
            enc(&map(vec![("reason", Cbor::Null)])),
        ),
        (
            "cancel with an unknown field",
            FrameType::StreamCancel,
            enc(&map(vec![("why", t("x"))])),
        ),
        (
            "start with an unknown role",
            FrameType::StreamStart,
            enc(&map(vec![("role", t("robot")), ("model", t("m"))])),
        ),
        (
            "start with empty stop_sequences",
            FrameType::StreamStart,
            enc(&map(vec![
                ("role", t("user")),
                ("model", t("m")),
                (
                    "decode_hints",
                    map(vec![("stop_sequences", Cbor::Array(vec![]))]),
                ),
            ])),
        ),
        (
            "token with bytes as an array",
            FrameType::StreamToken,
            enc(&map(vec![
                ("seq", int(0)),
                ("data", map(vec![("bytes", Cbor::Array(vec![int(1)]))])),
            ])),
        ),
        (
            "token with an id in the body (ids live in the frame header)",
            FrameType::StreamToken,
            enc(&map(vec![
                ("id", t("s1")),
                ("seq", int(0)),
                ("data", map(vec![("text", t("x"))])),
            ])),
        ),
        (
            "end with an unknown reason",
            FrameType::StreamEnd,
            enc(&map(vec![("reason", t("because"))])),
        ),
    ];
    for (label, frame_type, body) in cases {
        assert!(
            native::decode_stream_body(frame_type, &body).is_err(),
            "{label}: accepted"
        );
    }
    // The empty cancel is a value (`reason` is omitted, not null).
    let empty = enc(&map(vec![]));
    assert_eq!(
        native::decode_stream_body(FrameType::StreamCancel, &empty).unwrap(),
        Message::StreamCancel(StreamCancel { reason: None })
    );
    assert_eq!(cbor::to_vec(&StreamCancel { reason: None }).unwrap(), empty);
}

#[test]
fn decode_hints_omit_empty_stop_sequences() {
    let hints = DecodeHints::default();
    assert_eq!(cbor::to_vec(&hints).unwrap(), enc(&map(vec![])));
    let body = cbor::to_vec(&DecodeHints {
        stop_sequences: vec!["x".into()],
        ..DecodeHints::default()
    })
    .unwrap();
    assert_eq!(
        body,
        enc(&map(vec![("stop_sequences", Cbor::Array(vec![t("x")]))]))
    );
}

// ========== ERROR CODES ==========

#[test]
fn error_codes_are_stable_unique_snake_case() {
    let mut seen = std::collections::HashSet::new();
    for code in ALL_ERROR_CODES {
        assert!(seen.insert(*code), "duplicate {code}");
        assert!(
            !code.is_empty()
                && code
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{code} is not snake_case"
        );
    }
    // The three the connection layer and the decoder produce.
    assert_eq!(ERR_OVERLOADED, "overloaded");
    assert_eq!(ERR_UNKNOWN_COMMAND, "unknown_command");
    assert_eq!(ERR_INVALID_REQUEST, "invalid_request");
    assert_eq!(
        PROTOCOL_ERROR_CODES,
        &["overloaded", "unknown_command", "invalid_request"]
    );
    for code in PROTOCOL_ERROR_CODES {
        assert!(ALL_ERROR_CODES.contains(code));
    }
    // Spot checks on names a client branches on.
    for code in [
        "internal",
        "rate_limit",
        "session_capacity",
        "session_expired",
        "no_page_loaded",
        "capability_denied",
        "record_expired",
        "unsupported_tool",
        "outbound_address_denied",
        "knowledge_entry_too_large",
        "knowledge_capacity",
        "action_unsupported",
        "delete_refused",
    ] {
        assert!(ALL_ERROR_CODES.contains(&code), "{code} missing");
    }
    assert_eq!(ALL_ERROR_CODES.len(), 51);
}

type Conn = Connection<DuplexStream>;

async fn pair() -> (Conn, Conn) {
    let (a, b) = duplex(64 * 1024);
    let (client, server) = within(async {
        let config = native::local_config();
        tokio::join!(
            Connection::client_handshake(a, native::client_hello(), LIMIT),
            Connection::server_handshake(b, &config, LIMIT)
        )
    })
    .await;
    (client.unwrap(), server.unwrap())
}

/// Every code reaches the caller as `NativeError::Remote` with that code, the
/// message and the details intact, over a real connection.
#[tokio::test]
async fn every_error_code_reaches_the_caller_as_a_remote_error() {
    let (mut client, mut server) = pair().await;
    for (i, code) in ALL_ERROR_CODES.iter().enumerate() {
        let id = within(native::send_command(&mut client, &BrowserCommand::GetUR))
            .await
            .unwrap();
        let frame = within(server.recv()).await.unwrap().unwrap();
        assert_eq!(frame.request_id, id);
        let details = (i % 2 == 0).then(|| json!({"n": i}));
        let mut error = WireError::new(*code, format!("message {i}"));
        error.details = details.clone();
        within(native::send_err(&mut server, frame.request_id, &error))
            .await
            .unwrap();
        match within(native::await_reply::<_, Value>(&mut client, id)).await {
            Err(NativeError::Remote {
                code: got,
                message,
                details: got_details,
            }) => {
                assert_eq!(got, *code);
                assert_eq!(message, format!("message {i}"));
                assert_eq!(got_details, details);
            }
            other => panic!("{code}: {other:?}"),
        }
    }
}

/// Every command in the table crosses a real connection and comes back with
/// its result, under the frame's request id.
#[tokio::test]
async fn every_command_and_result_crosses_a_connection() {
    let (mut client, mut server) = pair().await;
    for (name, command, ok) in table() {
        let id = within(native::send_command(&mut client, &command))
            .await
            .unwrap();
        let frame = within(server.recv()).await.unwrap().unwrap();
        assert_eq!(frame.frame_type, FrameType::Request, "{name}");
        assert_eq!(frame.request_id, id, "{name}");
        assert_eq!(native::decode_command(&frame.body), Ok(command), "{name}");
        within(native::send_ok(&mut server, frame.request_id, &ok))
            .await
            .unwrap();
        let got: Value = within(native::await_reply(&mut client, id)).await.unwrap();
        assert_eq!(got, ok, "{name}");
    }
    assert_eq!(client.in_flight_len(), 0);
}

/// A body the daemon cannot decode is answered with the prescribed code, and
/// the connection carries on.
#[tokio::test]
async fn a_bad_request_gets_its_code_and_the_connection_survives() {
    let (mut client, mut server) = pair().await;
    let bad: [(Vec<u8>, &str); 2] = [
        (enc(&t("frobnicate")), ERR_UNKNOWN_COMMAND),
        (command_body("get_ur", map(vec![])), ERR_INVALID_REQUEST),
    ];
    for (body, code) in bad {
        let id = within(client.request(body)).await.unwrap();
        let frame = within(server.recv()).await.unwrap().unwrap();
        let error = native::decode_command(&frame.body).unwrap_err();
        within(native::send_err(
            &mut server,
            frame.request_id,
            &error.to_wire(),
        ))
        .await
        .unwrap();
        match within(native::await_reply::<_, Value>(&mut client, id)).await {
            Err(NativeError::Remote { code: got, .. }) => assert_eq!(got, code),
            other => panic!("{other:?}"),
        }
    }
    let id = within(native::send_command(&mut client, &BrowserCommand::GetUR))
        .await
        .unwrap();
    let frame = within(server.recv()).await.unwrap().unwrap();
    assert_eq!(
        native::decode_command(&frame.body),
        Ok(BrowserCommand::GetUR)
    );
    within(native::send_ok(&mut server, frame.request_id, &json!(1)))
        .await
        .unwrap();
    let got: Value = within(native::await_reply(&mut client, id)).await.unwrap();
    assert_eq!(got, json!(1));
}
