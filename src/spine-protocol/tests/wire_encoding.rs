//! Wire-codec round-trip + size proofs.
//!
//! These tests are the evidence behind SPINE's encoding-efficiency claim: every
//! representative v1 body must (a) survive a CBOR round-trip with equal values
//! and (b) be materially smaller on the wire than the equivalent UTF-8 JSON of
//! the same value. Bodies are deterministic CBOR (`cbor::to_vec`); the frame
//! layer compresses a body of at least 1 KiB when the sender turns compression
//! on. Tensor-heavy bodies shrink dramatically on plain CBOR alone;
//! text/structured bodies shrink modestly without compression and a lot with it.
//!
//! The bodies are the ones that exist in v1: a `REQUEST` body is a command, a
//! `RESPONSE` body is `{ok: T}` or `{err: ...}`, a `PING` body is an integer,
//! and the stream bodies carry no ids (the frame header does).

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;
use spine_protocol::body::{Reply, WireError};
use spine_protocol::frame::{
    decode_frame, encode_frame, Frame, FrameType, FLAG_COMPRESSED, HEADER_LEN, MAX_FRAME,
};
use spine_protocol::native;
use spine_protocol::{
    cbor, BrowserCommand, Capability, CapabilityAdvertisement, StreamCancel, StreamData,
    StreamToken, StreamUsage, ToolCall,
};
use std::fmt::Debug;

/// Decode-encode must preserve the value, and re-encoding the decoded value
/// must give the same bytes (one encoding per value).
fn assert_roundtrips<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let body = cbor::to_vec(value).expect("encode");
    let back: T = cbor::from_slice(&body).expect("decode");
    assert_eq!(value, &back, "round-trip altered the value");
    assert_eq!(
        cbor::to_vec(&back).expect("re-encode"),
        body,
        "re-encoding changed the bytes"
    );
    let strict: T = native::decode_strict(&body).expect("strict decode");
    assert_eq!(value, &strict);
}

/// CBOR body vs the JSON of the same value, as a ratio.
fn size_ratio<T: Serialize>(value: &T) -> (usize, usize, f64) {
    let json = serde_json::to_vec(value).unwrap();
    let wire = cbor::to_vec(value).unwrap();
    let ratio = wire.len() as f64 / json.len() as f64;
    (wire.len(), json.len(), ratio)
}

fn sample_tool_call() -> BrowserCommand {
    BrowserCommand::ToolCall(ToolCall {
        name: "agent.web/fetch_url".into(),
        args: json!({
            "url": "https://example.com/api/v2/resource?id=12345&fields=title,body,author",
            "method": "GET",
            "headers": { "accept": "application/json", "user-agent": "spine-agent/1.4" },
            "timeout_ms": 30000,
            "follow_redirects": true,
            "max_bytes": 1048576
        }),
        trace: None,
    })
}

fn sample_stream_token() -> StreamToken {
    StreamToken {
        seq: 128,
        data: StreamData::Text(
            " The quick brown fox jumps over the lazy dog, and then keeps going.".into(),
        ),
        usage: None,
    }
}

fn sample_tensor_token() -> StreamToken {
    // 1 KiB of tensor bytes (a 256-dim f32 embedding). JSON renders each byte as
    // a decimal-with-comma; CBOR ships them as a byte string.
    let data: Vec<u8> = (0..1024u32)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect();
    StreamToken {
        seq: 1,
        data: StreamData::Bytes(data),
        usage: None,
    }
}

fn sample_capability_ad() -> Reply<CapabilityAdvertisement> {
    let schema = json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "format": "uri" },
            "method": { "type": "string", "enum": ["GET", "POST", "PUT", "DELETE"] }
        },
        "required": ["url"]
    });
    let cap = |uri: &str, desc: &str| Capability {
        uri: uri.into(),
        description: desc.into(),
        input_schema: schema.clone(),
        output_schema: json!({ "type": "object", "properties": { "status": { "type": "integer" }, "body": { "type": "string" } } }),
        embedding: None,
    };
    Reply::Ok(CapabilityAdvertisement {
        agent_id: "did:spine:ed25519:7Hk9...4Qm".into(),
        capabilities: vec![
            cap(
                "agent.web/fetch_url",
                "Fetch a URL and return the response body.",
            ),
            cap(
                "agent.web/post_json",
                "POST a JSON document to an endpoint.",
            ),
        ],
    })
}

#[test]
fn representative_bodies_roundtrip() {
    assert_roundtrips(&sample_tool_call());
    assert_roundtrips(&sample_stream_token());
    assert_roundtrips(&sample_tensor_token());
    assert_roundtrips(&sample_capability_ad());
    assert_roundtrips(&1_700_000_000u64);
    assert_roundtrips(&StreamCancel {
        reason: Some("user aborted".into()),
    });
    assert_roundtrips(&StreamCancel { reason: None });
    // An error reply.
    assert_roundtrips(&Reply::<serde_json::Value>::Err(
        WireError::new("rate_limit", "slow down").with_details(json!({"retry_after_ms": 250})),
    ));
    // StreamToken carrying mid-stream cumulative usage.
    assert_roundtrips(&StreamToken {
        seq: 12,
        data: StreamData::Text("tok".into()),
        usage: Some(StreamUsage {
            input_tokens: 40,
            output_tokens: 12,
        }),
    });
}

#[test]
fn every_body_beats_json() {
    // No representative body is larger on the wire than its JSON form: even
    // the tiny control bodies and the high-entropy text bodies clear this bar.
    let (wire, json, ratio) = size_ratio(&sample_tool_call());
    assert!(wire < json, "ToolCall: {wire}B vs {json}B = {ratio:.2}");
    let (wire, json, ratio) = size_ratio(&sample_stream_token());
    assert!(wire < json, "StreamToken: {wire}B vs {json}B = {ratio:.2}");
    let (wire, json, ratio) = size_ratio(&sample_tensor_token());
    assert!(wire < json, "TensorToken: {wire}B vs {json}B = {ratio:.2}");
    let (wire, json, ratio) = size_ratio(&sample_capability_ad());
    assert!(wire < json, "CapabilityAd: {wire}B vs {json}B = {ratio:.2}");
    let (wire, json, ratio) = size_ratio(&1_700_000_000u64);
    assert!(wire < json, "Ping: {wire}B vs {json}B = {ratio:.2}");
}

#[test]
fn tensor_bodies_shrink_dramatically_on_plain_cbor() {
    // The fast default (plain CBOR, no compression) already crushes a 1 KiB
    // embedding to roughly a third of its JSON body — native byte/number widths
    // plus a byte-string tensor payload, at serialization speed.
    let (wire, json, ratio) = size_ratio(&sample_tensor_token());
    assert!(
        ratio < 0.35,
        "TensorToken (plain CBOR): wire {wire}B vs json {json}B = {ratio:.2} (want < 0.35)"
    );
}

#[test]
fn compression_is_available_for_bandwidth_paths() {
    // With compression on, the frame layer compresses a body of at least 1 KiB
    // and keeps the result when it is smaller. It must push the tensor body
    // far below plain CBOR, and the receiver must read the compressed form
    // back to the same body. The structured capability ad is under 1 KiB, so
    // the frame layer leaves it alone; zstd on its CBOR shows what a sender
    // that wants to compress such bodies would gain.
    let token = sample_tensor_token();
    let json = serde_json::to_vec(&token).unwrap();
    let body = cbor::to_vec(&token).unwrap();
    let wire_frame = Frame {
        frame_type: FrameType::StreamToken,
        request_id: 1,
        body: body.clone(),
    };
    let bytes = encode_frame(&wire_frame, MAX_FRAME, true).unwrap();
    assert_eq!(
        bytes[6] & FLAG_COMPRESSED,
        FLAG_COMPRESSED,
        "a 1 KiB tensor body must be compressed"
    );
    let ratio = (bytes.len() - HEADER_LEN) as f64 / json.len() as f64;
    assert!(
        ratio < 0.15,
        "TensorToken (cbor+zstd): {} vs {} = {ratio:.2} (want < 0.15)",
        bytes.len() - HEADER_LEN,
        json.len()
    );
    let (decoded, used) = decode_frame(&bytes, MAX_FRAME).unwrap();
    assert_eq!(used, bytes.len());
    assert_eq!(decoded.body, body);
    assert_eq!(
        native::decode_stream_body(decoded.frame_type, &decoded.body).unwrap(),
        spine_protocol::Message::StreamToken(token)
    );

    let ad = sample_capability_ad();
    let ad_json = serde_json::to_vec(&ad).unwrap();
    let ad_cbor = cbor::to_vec(&ad).unwrap();
    let ad_zstd = zstd::stream::encode_all(&ad_cbor[..], 3).unwrap();
    let ad_ratio = ad_zstd.len() as f64 / ad_json.len() as f64;
    assert!(
        ad_ratio < 0.60,
        "CapabilityAd (cbor+zstd): {} vs {} = {ad_ratio:.2} (want < 0.60)",
        ad_zstd.len(),
        ad_json.len()
    );
}

#[test]
fn text_and_structured_bodies_shrink_modestly_on_plain_cbor() {
    // High-entropy text (URLs, UUIDs, prose) and schema-shaped structures can't
    // be squeezed below their own content without compression, but dropping
    // JSON's quotes/punctuation still shaves real bytes on the fast path.
    let (cw, cj, cr) = size_ratio(&sample_tool_call());
    assert!(
        cr < 0.90,
        "ToolCall: {cw}B vs {cj}B = {cr:.2} (want < 0.90)"
    );
    let (sw, sj, sr) = size_ratio(&sample_stream_token());
    assert!(
        sr < 0.97,
        "StreamToken: {sw}B vs {sj}B = {sr:.2} (want < 0.97)"
    );
    let (aw, aj, ar) = size_ratio(&sample_capability_ad());
    assert!(
        ar < 0.90,
        "CapabilityAd: {aw}B vs {aj}B = {ar:.2} (want < 0.90)"
    );
}
