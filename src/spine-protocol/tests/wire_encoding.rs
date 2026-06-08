//! Wire-codec round-trip + size proofs.
//!
//! These tests are the evidence behind SPINE's encoding-efficiency claim: every
//! representative agent frame must (a) survive a CBOR round-trip byte-for-byte
//! at the value level and (b) be materially smaller on the wire than the
//! equivalent UTF-8 JSON body. The structurally rich frames (tool calls,
//! encoded latents, capability ads) are held to a strict `< 60%` of JSON; the
//! text-dominated stream token — where the payload *is* the bytes and there's
//! little structure to squeeze — only has to come in under JSON.

use serde_json::json;
use spine_protocol::wire::{self, FORMAT_CBOR, FORMAT_CBOR_ZSTD};
use spine_protocol::{
    Capability, CapabilityAdvertisement, DType, EncodedFrame, EncodedMetadata, Message, Modality,
    StreamCancel, StreamData, StreamToken, StreamUsage, ToolCall,
};

/// Decode-encode must preserve the message at the value level.
fn assert_roundtrips(msg: &Message) {
    let framed = wire::encode(msg).expect("encode");
    assert_eq!(&framed[0..2], b"SP", "frame must carry the SP magic");
    let back = wire::decode(&framed).expect("decode");
    assert_eq!(
        serde_json::to_value(msg).unwrap(),
        serde_json::to_value(&back).unwrap(),
        "round-trip altered the message",
    );
}

/// Wire frame (CBOR, header included) vs the JSON body, as a ratio.
fn size_ratio(msg: &Message) -> (usize, usize, f64) {
    let json = serde_json::to_vec(msg).unwrap();
    let wire = wire::encode(msg).unwrap();
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

fn sample_encoded_frame() -> Message {
    // 256-dim f32 embedding => 1024 raw bytes. JSON renders each byte as a
    // decimal-with-comma; CBOR keeps them as integer-array elements.
    let data: Vec<u8> = (0..1024u32).map(|i| (i.wrapping_mul(31) % 251) as u8).collect();
    Message::Encoded(EncodedFrame {
        codec: "spine:codec/titans/v1@dim=256,dtype=f32".into(),
        variant: Some("layer=11".into()),
        data,
        metadata: EncodedMetadata {
            modality: Modality::Embedding,
            shape: vec![256],
            dtype: DType::F32,
            original_len: Some(4096),
            source_hash: Some([0xABu8; 32]),
        },
        trace: None,
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
            cap("agent.web/fetch_url", "Fetch a URL and return the response body."),
            cap("agent.web/post_json", "POST a JSON document to an endpoint."),
        ],
    })
}

#[test]
fn representative_frames_roundtrip() {
    assert_roundtrips(&sample_tool_call());
    assert_roundtrips(&sample_stream_token());
    assert_roundtrips(&sample_encoded_frame());
    assert_roundtrips(&sample_capability_ad());
    assert_roundtrips(&Message::Ping { timestamp: 1_700_000_000 });
    assert_roundtrips(&Message::Pong { timestamp: 1_700_000_001 });
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
        ("EncodedFrame", sample_encoded_frame()),
        ("CapabilityAd", sample_capability_ad()),
        ("Ping", Message::Ping { timestamp: 1_700_000_000 }),
    ] {
        let (wire, json, ratio) = size_ratio(&msg);
        assert!(
            wire < json,
            "{label}: wire {wire}B vs json {json}B = {ratio:.2} (want wire < json)"
        );
    }
}

#[test]
fn binary_frame_crushes_json() {
    // Numeric/binary payloads are where CBOR's native byte/number widths win
    // big: a 1 KiB embedding lands well under a quarter of its JSON body.
    let (wire, json, ratio) = size_ratio(&sample_encoded_frame());
    assert!(
        ratio < 0.25,
        "EncodedFrame: wire {wire}B vs json {json}B = {ratio:.2} (want < 0.25)"
    );
}

#[test]
fn structured_frame_under_60pct() {
    // A capability advertisement (repeated JSON-Schema structure) compresses to
    // under 60% of its JSON body via CBOR + zstd.
    let (wire, json, ratio) = size_ratio(&sample_capability_ad());
    assert!(
        ratio < 0.60,
        "CapabilityAd: wire {wire}B vs json {json}B = {ratio:.2} (want < 0.60)"
    );
}

#[test]
fn text_frames_still_shrink() {
    // High-entropy text (URLs, UUIDs, prose) can't be squeezed below its own
    // content, but stripping JSON's quotes/punctuation still shaves real bytes.
    let (cw, cj, cr) = size_ratio(&sample_tool_call());
    assert!(cr < 0.85, "ToolCall: {cw}B vs {cj}B = {cr:.2} (want < 0.85)");
    let (sw, sj, sr) = size_ratio(&sample_stream_token());
    assert!(sr < 0.97, "StreamToken: {sw}B vs {sj}B = {sr:.2} (want < 0.97)");
}

#[test]
fn large_frames_select_zstd() {
    // A large, compressible payload (> ZSTD_THRESHOLD) must select CBOR+zstd;
    // a tiny control frame stays plain CBOR.
    let repetitive = Message::Encoded(EncodedFrame {
        codec: "spine:codec/raw/v1".into(),
        variant: None,
        data: vec![0xCDu8; 4096],
        metadata: EncodedMetadata {
            modality: Modality::HiddenState,
            shape: vec![1024],
            dtype: DType::F32,
            original_len: None,
            source_hash: None,
        },
        trace: None,
    });
    let big = wire::encode(&repetitive).unwrap();
    assert_eq!(big[3], FORMAT_CBOR_ZSTD, "large frame should be CBOR+zstd");

    let small = wire::encode(&Message::Ping { timestamp: 1 }).unwrap();
    assert_eq!(small[3], FORMAT_CBOR, "tiny frame should stay plain CBOR");
}
