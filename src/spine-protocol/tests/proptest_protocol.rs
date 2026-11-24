//! Property-based tests for the spine-wire v1 bodies.
//!
//! For every body type that crosses the wire:
//!
//! * `from_slice(to_vec(x)) == x`;
//! * `to_vec(from_slice(b)) == b` for `b = to_vec(x)` (one encoding per value);
//! * for commands, `decode_command(to_vec(cmd)) == cmd`.
//!
//! Floats are generated as `n as f64 / 4.0` (or `f32`), so every one is exactly
//! representable and none is NaN or infinite: the properties are about the
//! codec, not about float formatting.

use proptest::collection::{btree_map, vec};
use proptest::prelude::*;
use proptest::strategy::Union;
use proptest::test_runner::TestCaseError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use spine_protocol::body::{Reply, WireError};
use spine_protocol::native;
use spine_protocol::resource::ResourceChunk;
use spine_protocol::{
    cbor, BrowserCommand, Capability, CapabilityAdvertisement, CapabilityQuery, CapabilitySelector,
    DecodeHints, Instruction, NameProvenance, NameResolution, SpineBinary, StreamCancel,
    StreamData, StreamEnd, StreamEndReason, StreamRole, StreamStart, StreamToken, StreamUsage,
    ToolCall, TraceContext,
};
use std::fmt::Debug;

// ========== STRATEGIES ==========

fn arb_text() -> impl Strategy<Value = String> {
    "\\PC{0,24}"
}

fn arb_bytes32() -> impl Strategy<Value = [u8; 32]> {
    prop::array::uniform32(any::<u8>())
}

fn arb_data() -> impl Strategy<Value = Vec<u8>> {
    vec(any::<u8>(), 0..64)
}

/// Exactly representable, never NaN or infinite.
fn arb_f64() -> impl Strategy<Value = f64> {
    (-100_000i32..100_000).prop_map(|n| n as f64 / 4.0)
}

fn arb_f32() -> impl Strategy<Value = f32> {
    (-100_000i32..100_000).prop_map(|n| n as f32 / 4.0)
}

fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| serde_json::json!(n)),
        any::<u64>().prop_map(|n| serde_json::json!(n)),
        arb_f64().prop_map(|f| serde_json::json!(f)),
        arb_text().prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            vec(inner.clone(), 0..4).prop_map(Value::Array),
            btree_map(arb_text(), inner, 0..4).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// A value that may stand for an optional field: a present `null` is
/// rejected, so it is not generated.
fn arb_json_present() -> impl Strategy<Value = Value> {
    arb_json().prop_filter("an optional field is never null", |v| !v.is_null())
}

fn arb_wire_error() -> impl Strategy<Value = WireError> {
    (
        "[a-z_]{1,24}",
        arb_text(),
        prop::option::of(arb_json_present()),
    )
        .prop_map(|(code, message, details)| WireError {
            code,
            message,
            details,
        })
}

fn arb_reply() -> impl Strategy<Value = Reply<Value>> {
    prop_oneof![
        arb_json().prop_map(Reply::Ok),
        arb_wire_error().prop_map(Reply::Err),
    ]
}

fn arb_trace() -> impl Strategy<Value = TraceContext> {
    (
        prop::array::uniform16(any::<u8>()),
        prop::array::uniform8(any::<u8>()),
        any::<u8>(),
        vec((arb_text(), arb_text()), 0..3),
    )
        .prop_map(|(trace_id, span_id, flags, state)| TraceContext {
            trace_id,
            span_id,
            flags,
            state,
        })
}

fn arb_tool_call() -> impl Strategy<Value = ToolCall> {
    (arb_text(), arb_json(), prop::option::of(arb_trace()))
        .prop_map(|(name, args, trace)| ToolCall { name, args, trace })
}

fn arb_instruction() -> impl Strategy<Value = Instruction> {
    prop_oneof![
        Just(Instruction::Pop),
        Just(Instruction::Return),
        arb_json().prop_map(Instruction::Push),
        any::<usize>().prop_map(|n| Instruction::Jump(n >> 1)),
        (any::<u32>(), arb_text()).prop_map(|(id, tag)| Instruction::DefineElement { id, tag }),
        vec(arb_f32(), 0..4).prop_map(|vector| Instruction::StreamLatent { vector }),
    ]
}

fn arb_binary() -> impl Strategy<Value = SpineBinary> {
    (
        vec(arb_instruction(), 0..4),
        arb_data(),
        0usize..1000,
        btree_map(arb_text(), 0usize..1000, 0..3),
        vec(arb_text(), 0..3),
    )
        .prop_map(
            |(instructions, data, render_start, exported_functions, capabilities)| SpineBinary {
                instructions,
                data,
                render_start,
                exported_functions,
                capabilities,
            },
        )
}

fn arb_selector() -> impl Strategy<Value = CapabilitySelector> {
    prop_oneof![
        arb_text().prop_map(CapabilitySelector::Exact),
        arb_text().prop_map(CapabilitySelector::Prefix),
        (vec(arb_f32(), 0..6), any::<u32>())
            .prop_map(|(embedding, top_k)| { CapabilitySelector::Semantic { embedding, top_k } }),
        Just(CapabilitySelector::All),
    ]
}

fn arb_capability() -> impl Strategy<Value = Capability> {
    (
        arb_text(),
        arb_text(),
        arb_json(),
        arb_json(),
        prop::option::of(vec(arb_f32(), 0..6)),
    )
        .prop_map(
            |(uri, description, input_schema, output_schema, embedding)| Capability {
                uri,
                description,
                input_schema,
                output_schema,
                embedding,
            },
        )
}

fn arb_advertisement() -> impl Strategy<Value = CapabilityAdvertisement> {
    (arb_text(), vec(arb_capability(), 0..3)).prop_map(|(agent_id, capabilities)| {
        CapabilityAdvertisement {
            agent_id,
            capabilities,
        }
    })
}

fn arb_usage() -> impl Strategy<Value = StreamUsage> {
    (any::<u64>(), any::<u64>()).prop_map(|(input_tokens, output_tokens)| StreamUsage {
        input_tokens,
        output_tokens,
    })
}

fn arb_stream_data() -> impl Strategy<Value = StreamData> {
    prop_oneof![
        arb_text().prop_map(StreamData::Text),
        arb_data().prop_map(StreamData::Bytes),
        arb_tool_call().prop_map(StreamData::ToolCall),
    ]
}

fn arb_stream_token() -> impl Strategy<Value = StreamToken> {
    (
        any::<u64>(),
        arb_stream_data(),
        prop::option::of(arb_usage()),
    )
        .prop_map(|(seq, data, usage)| StreamToken { seq, data, usage })
}

fn arb_stream_start() -> impl Strategy<Value = StreamStart> {
    let hints = (
        prop::option::of(arb_f32()),
        prop::option::of(any::<u32>()),
        prop::option::of(any::<u64>()),
        vec(arb_text(), 0..3),
    )
        .prop_map(|(temperature, top_k, seed, stop_sequences)| DecodeHints {
            temperature,
            top_k,
            seed,
            stop_sequences,
            ..DecodeHints::default()
        });
    (
        prop_oneof![
            Just(StreamRole::Assistant),
            Just(StreamRole::Tool),
            Just(StreamRole::User),
            Just(StreamRole::System),
        ],
        arb_text(),
        prop::option::of(arb_trace()),
        prop::option::of(hints),
    )
        .prop_map(|(role, model, trace, decode_hints)| StreamStart {
            role,
            model,
            trace,
            decode_hints,
        })
}

fn arb_stream_end() -> impl Strategy<Value = StreamEnd> {
    let reason = prop_oneof![
        Just(StreamEndReason::Stop),
        Just(StreamEndReason::Length),
        Just(StreamEndReason::ToolUse),
        Just(StreamEndReason::ContentFilter),
        Just(StreamEndReason::Cancelled),
        (arb_text(), arb_text())
            .prop_map(|(code, message)| StreamEndReason::Error { code, message }),
    ];
    (reason, prop::option::of(arb_usage())).prop_map(|(reason, usage)| StreamEnd { reason, usage })
}

fn arb_chunk() -> impl Strategy<Value = ResourceChunk> {
    (
        arb_bytes32(),
        any::<u64>(),
        any::<u64>(),
        arb_text(),
        arb_data(),
        any::<bool>(),
    )
        .prop_map(
            |(hash, offset, total_bytes, content_type, data, complete)| ResourceChunk {
                hash,
                offset,
                total_bytes,
                content_type,
                data,
                complete,
            },
        )
}

/// One succession entry of the shape of `02-names.md` 10.1 (the protocol layer
/// checks shape, not signatures): arbitrary short text and numbers in the
/// fields.
fn arb_log_entry() -> impl Strategy<Value = Value> {
    (
        "[a-z2-7]{1,52}",
        any::<u64>(),
        prop::option::of("[a-z2-7]{1,52}"),
        any::<bool>(),
        "[a-z2-7]{1,52}",
        prop::option::of("[a-z2-7]{1,52}"),
        any::<u64>(),
        "[a-z2-7]{1,103}",
    )
        .prop_map(
            |(inception_key, index, prev, retire, key, commitment, effective_at, signature)| {
                let kind = if retire { "retire" } else { "rotate" };
                serde_json::json!({
                    "inception_key": inception_key,
                    "index": index,
                    "prev": prev,
                    "kind": kind,
                    "key": key,
                    "commitment": commitment,
                    "effective_at": effective_at,
                    "signature": signature,
                })
            },
        )
}

/// A log of `min` to 64 entries.
fn arb_log(min: usize) -> impl Strategy<Value = Vec<Value>> {
    vec(arb_log_entry(), min..=4)
}

fn arb_provenance() -> impl Strategy<Value = NameProvenance> {
    prop_oneof![
        Just(NameProvenance::Cache),
        Just(NameProvenance::StaleCache),
        Just(NameProvenance::Local),
        Just(NameProvenance::Network),
        Just(NameProvenance::Address),
    ]
}

fn arb_resolution() -> impl Strategy<Value = NameResolution> {
    prop_oneof![
        (arb_json(), arb_provenance(), arb_log(0)).prop_map(|(record, provenance, log)| {
            NameResolution::Resolved {
                record,
                provenance,
                log,
            }
        }),
        any::<u32>().prop_map(|ttl_secs| NameResolution::Unchanged { ttl_secs }),
        arb_text().prop_map(|name| NameResolution::NotFound { name }),
        (arb_text(), arb_text())
            .prop_map(|(name, reason)| NameResolution::Invalid { name, reason }),
        (arb_text(), arb_text())
            .prop_map(|(name, reason)| NameResolution::Unavailable { name, reason }),
        (arb_text(), arb_log(1)).prop_map(|(name, log)| NameResolution::Retired { name, log }),
        (arb_text(), arb_log(1), arb_log(1)).prop_map(|(name, first, second)| {
            NameResolution::Compromised {
                name,
                logs: vec![first, second],
            }
        }),
        (arb_json(), arb_provenance(), arb_log(0)).prop_map(|(record, provenance, log)| {
            NameResolution::Revoked {
                record,
                provenance,
                log,
            }
        }),
    ]
}

/// All 19 commands.
fn arb_command() -> impl Strategy<Value = BrowserCommand> {
    let strategies: Vec<BoxedStrategy<BrowserCommand>> = vec![
        arb_text()
            .prop_map(|url| BrowserCommand::Navigate { url })
            .boxed(),
        Just(BrowserCommand::GetUR).boxed(),
        Just(BrowserCommand::GetRawHTML).boxed(),
        arb_binary().prop_map(BrowserCommand::ExecuteBinary).boxed(),
        (arb_text(), arb_json(), vec(arb_text(), 0..3))
            .prop_map(|(key, value, tags)| BrowserCommand::StoreKnowledge { key, value, tags })
            .boxed(),
        (arb_text(), vec(arb_text(), 0..3), 0usize..1000)
            .prop_map(|(query, tags, limit)| BrowserCommand::QueryKnowledge { query, tags, limit })
            .boxed(),
        Just(BrowserCommand::GetBinaryCapabilities).boxed(),
        (arb_text(), prop::option::of(arb_bytes32()))
            .prop_map(|(name, if_none_match)| BrowserCommand::ResolveName {
                name,
                if_none_match,
            })
            .boxed(),
        vec(arb_text(), 0..4)
            .prop_map(|names| BrowserCommand::ResolveNames { names })
            .boxed(),
        (arb_text(), prop::option::of(0usize..1000))
            .prop_map(|(capability, limit)| BrowserCommand::FindProviders { capability, limit })
            .boxed(),
        arb_json()
            .prop_map(|record| BrowserCommand::PublishName { record })
            .boxed(),
        (arb_text(), prop::option::of(arb_bytes32()))
            .prop_map(|(name, if_none_match)| BrowserCommand::FetchName {
                name,
                if_none_match,
            })
            .boxed(),
        (
            arb_text(),
            prop::option::of(any::<u32>()),
            prop::option::of(0usize..1000),
        )
            .prop_map(|(seed, max_depth, max_visits)| BrowserCommand::CrawlNames {
                seed,
                max_depth,
                max_visits,
            })
            .boxed(),
        (arb_json(), arb_text(), arb_data())
            .prop_map(
                |(record, content_type, data)| BrowserCommand::PublishResource {
                    record,
                    content_type,
                    data,
                },
            )
            .boxed(),
        (arb_bytes32(), any::<u64>(), any::<u32>())
            .prop_map(|(hash, offset, max_bytes)| BrowserCommand::ReadResource {
                hash,
                offset,
                max_bytes,
            })
            .boxed(),
        Just(BrowserCommand::NamespaceStatus).boxed(),
        (arb_text(), arb_bytes32(), arb_json())
            .prop_map(|(peer_id, peer_key, payload)| BrowserCommand::MeshCall {
                peer_id,
                peer_key,
                payload,
            })
            .boxed(),
        arb_tool_call().prop_map(BrowserCommand::ToolCall).boxed(),
        arb_selector()
            .prop_map(|selector| BrowserCommand::CapabilityQuery(CapabilityQuery { selector }))
            .boxed(),
    ];
    assert_eq!(strategies.len(), spine_protocol::COMMAND_NAMES.len());
    Union::new(strategies)
}

// ========== PROPERTIES ==========

/// `from_slice(to_vec(x)) == x`, `to_vec(from_slice(b)) == b`, and the body is
/// deterministic and strictly canonical.
fn check<T>(value: &T) -> Result<(), TestCaseError>
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let body = cbor::to_vec(value).map_err(|e| TestCaseError::fail(e.to_string()))?;
    prop_assert!(cbor::check_deterministic(&body).is_ok());
    let back: T = cbor::from_slice(&body).map_err(|e| TestCaseError::fail(e.to_string()))?;
    prop_assert_eq!(&back, value);
    let again = cbor::to_vec(&back).map_err(|e| TestCaseError::fail(e.to_string()))?;
    prop_assert_eq!(&again, &body);
    let strict: T = native::decode_strict(&body).map_err(|e| TestCaseError::fail(e.to_string()))?;
    prop_assert_eq!(&strict, value);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn command_roundtrip(command in arb_command()) {
        check(&command)?;
        // The request path: encode_command and decode_command.
        let body = native::encode_command(&command).unwrap();
        prop_assert_eq!(&body, &cbor::to_vec(&command).unwrap());
        let decoded = native::decode_command(&body);
        prop_assert_eq!(decoded, Ok(command));
    }

    #[test]
    fn reply_roundtrip(reply in arb_reply()) {
        check(&reply)?;
        let body = cbor::to_vec(&reply).unwrap();
        let decoded = native::decode_reply::<Value>(&body).unwrap();
        prop_assert_eq!(decoded, reply);
    }

    #[test]
    fn wire_error_roundtrip(error in arb_wire_error()) {
        check(&error)?;
        // And inside the err body that error_body builds.
        let body = native::error_body(&error).unwrap();
        let decoded = native::decode_reply::<Value>(&body).unwrap();
        prop_assert_eq!(decoded, Reply::Err(error));
    }

    #[test]
    fn tool_call_roundtrip(call in arb_tool_call()) {
        check(&call)?;
    }

    #[test]
    fn trace_context_roundtrip(trace in arb_trace()) {
        check(&trace)?;
    }

    #[test]
    fn capability_advertisement_roundtrip(ad in arb_advertisement()) {
        check(&ad)?;
    }

    #[test]
    fn capability_selector_roundtrip(selector in arb_selector()) {
        check(&selector)?;
    }

    #[test]
    fn stream_start_roundtrip(body in arb_stream_start()) {
        check(&body)?;
    }

    #[test]
    fn stream_token_roundtrip(body in arb_stream_token()) {
        check(&body)?;
    }

    #[test]
    fn stream_end_roundtrip(body in arb_stream_end()) {
        check(&body)?;
    }

    #[test]
    fn stream_cancel_roundtrip(reason in prop::option::of(arb_text())) {
        check(&StreamCancel { reason })?;
    }

    #[test]
    fn resource_chunk_roundtrip(chunk in arb_chunk()) {
        check(&chunk)?;
    }

    #[test]
    fn name_resolution_roundtrip(resolution in arb_resolution()) {
        check(&resolution)?;
    }

    #[test]
    fn ping_token_roundtrip(token in any::<u64>()) {
        let body = native::ping_body(token).unwrap();
        prop_assert_eq!(native::ping_token(&body).unwrap(), token);
    }

    /// Whatever bytes arrive, decoding never panics, and a body that decodes
    /// as a command is exactly that command's canonical encoding.
    #[test]
    fn arbitrary_bytes_never_panic_and_accepted_bodies_are_canonical(
        bytes in vec(any::<u8>(), 0..256),
    ) {
        if let Ok(command) = native::decode_command(&bytes) {
            prop_assert_eq!(cbor::to_vec(&command).unwrap(), bytes.clone());
        }
        if let Ok(reply) = native::decode_reply::<Value>(&bytes) {
            prop_assert_eq!(cbor::to_vec(&reply).unwrap(), bytes.clone());
        }
        let _ = cbor::from_slice::<BrowserCommand>(&bytes);
    }

    /// A single flipped bit in a valid command body either still decodes to
    /// a command whose encoding is the mutated body, or is refused; it never
    /// decodes to something with a different encoding.
    #[test]
    fn a_flipped_bit_is_refused_or_canonical(
        command in arb_command(),
        position in any::<prop::sample::Index>(),
        bit in 0u8..8,
    ) {
        let mut body = cbor::to_vec(&command).unwrap();
        let at = position.index(body.len());
        body[at] ^= 1 << bit;
        if let Ok(decoded) = native::decode_command(&body) {
            prop_assert_eq!(cbor::to_vec(&decoded).unwrap(), body);
        }
    }
}
