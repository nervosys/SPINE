//! Wire-codec round-trip + size proofs.
//!
//! These tests are the evidence behind SPINE's encoding-efficiency claim: every
//! representative agent frame must (a) survive a CBOR round-trip byte-for-byte
//! at the value level and (b) be materially smaller on the wire than the
//! equivalent UTF-8 JSON body. Bodies are deterministic CBOR (`cbor::to_vec`);
//! the frame layer compresses a body of at least 1 KiB when the sender turns
//! compression on. Tensor-heavy frames shrink dramatically on plain CBOR alone;
//! text/structured frames shrink modestly without compression and a lot with
//! it.

use serde_json::json;
use spine_protocol::cbor;
use spine_protocol::frame::{
    decode_frame, encode_frame, Frame, FrameType, FLAG_COMPRESSED, HEADER_LEN, MAX_FRAME,
};
use spine_protocol::{
    Capability, CapabilityAdvertisement, Message, StreamCancel, StreamData, StreamToken,
    StreamUsage, ToolCall,
};

/// Decode-encode must preserve the message at the value level.
fn assert_roundtrips(msg: &Message) {
    let body = cbor::to_vec(msg).expect("encode");
    let back: Message = cbor::from_slice(&body).expect("decode");
    assert_eq!(
        serde_json::to_value(msg).unwrap(),
        serde_json::to_value(&back).unwrap(),
        "round-trip altered the message",
    );
}

/// CBOR body vs the JSON body, as a ratio.
fn size_ratio(msg: &Message) -> (usize, usize, f64) {
    let json = serde_json::to_vec(msg).unwrap();
    let wire = cbor::to_vec(msg).unwrap();
    let ratio = wire.len() as f64 / json.len() as f64;
    (wire.len(), json.len(), ratio)
}

fn sample_tool_call() -> Message {
    Message::ToolCall(ToolCall {
        id: "b3c1f2a4-0d8e-4c9a-9f1b-2e7d6c5a4b30".into(),
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

fn sample_stream_token() -> Message {
    Message::StreamToken(StreamToken {
        id: "stream-7f3a".into(),
        seq: 128,
        data: StreamData::Text(
            " The quick brown fox jumps over the lazy dog, and then keeps going.".into(),
        ),
        usage: None,
    })
}

fn sample_tensor_token() -> Message {
    // 1 KiB of tensor bytes (a 256-dim f32 embedding). JSON renders each byte as
    // a decimal-with-comma; CBOR ships them as a byte string.
    let data: Vec<u8> = (0..1024u32)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect();
    Message::StreamToken(StreamToken {
        id: "stream-7f3a".into(),
        seq: 1,
        data: StreamData::Bytes(data),
        usage: None,
    })
}

fn sample_capability_ad() -> Message {
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
    Message::CapabilityAd(CapabilityAdvertisement {
        id: "q-9921".into(),
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
fn representative_frames_roundtrip() {
    assert_roundtrips(&sample_tool_call());
    assert_roundtrips(&sample_stream_token());
    assert_roundtrips(&sample_tensor_token());
    assert_roundtrips(&sample_capability_ad());
    assert_roundtrips(&Message::Ping {
        timestamp: 1_700_000_000,
    });
    assert_roundtrips(&Message::Pong {
        timestamp: 1_700_000_001,
    });
    assert_roundtrips(&Message::StreamCancel(StreamCancel {
        id: "stream-7f3a".into(),
        reason: Some("user aborted".into()),
    }));
    // StreamToken carrying mid-stream cumulative usage.
    assert_roundtrips(&Message::StreamToken(StreamToken {
        id: "stream-7f3a".into(),
        seq: 12,
        data: StreamData::Text("tok".into()),
        usage: Some(StreamUsage {
            input_tokens: 40,
            output_tokens: 12,
        }),
    }));
}

#[test]
fn every_frame_beats_json() {
    // No representative frame is larger on the wire than its JSON body — even
    // the tiny control frames and the high-entropy text frames clear this bar.
    for (label, msg) in [
        ("ToolCall", sample_tool_call()),
        ("StreamToken", sample_stream_token()),
        ("TensorToken", sample_tensor_token()),
        ("CapabilityAd", sample_capability_ad()),
        (
            "Ping",
            Message::Ping {
                timestamp: 1_700_000_000,
            },
        ),
    ] {
        let (wire, json, ratio) = size_ratio(&msg);
        assert!(
            wire < json,
            "{label}: wire {wire}B vs json {json}B = {ratio:.2} (want wire < json)"
        );
    }
}

#[test]
fn tensor_frames_shrink_dramatically_on_plain_cbor() {
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
    // and keeps the result when it is smaller. It must push the tensor frame
    // far below plain CBOR, and the receiver must read the compressed form
    // back to the same body. The structured capability ad is under 1 KiB, so
    // the frame layer leaves it alone; zstd on its CBOR shows what a sender
    // that wants to compress such bodies would gain.
    let frame = sample_tensor_token();
    let json = serde_json::to_vec(&frame).unwrap();
    let body = cbor::to_vec(&frame).unwrap();
    let wire_frame = Frame {
        frame_type: FrameType::Request,
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
fn text_and_structured_frames_shrink_modestly_on_plain_cbor() {
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
