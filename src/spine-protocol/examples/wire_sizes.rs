//! Measure SPINE's binary wire format against UTF-8 JSON, frame by frame.
//!
//! Prints, for each representative agent message, the JSON body size, the
//! spine-wire v1 body size (deterministic CBOR, and CBOR through the frame
//! layer's zstd path), and the size ratios. This is the data behind SPINE's
//! encoding-efficiency claim: run it and read the numbers off the wire.
//!
//! Run: `cargo run -p spine-protocol --example wire_sizes`

use serde_json::json;
use spine_protocol::cbor;
use spine_protocol::frame::{encode_frame, Frame, FrameType, HEADER_LEN, MAX_FRAME};
use spine_protocol::{
    Capability, CapabilityAdvertisement, Message, StreamData, StreamToken, ToolCall,
};

fn row(label: &str, msg: &Message) {
    let json = serde_json::to_vec(msg).unwrap();
    // Default hot-path codec: deterministic CBOR, no compression.
    let cbor = cbor::to_vec(msg).unwrap();
    // Opt-in: the frame layer compresses a body of at least 1 KiB when
    // compression is on and keeps the result only if it is smaller. The
    // figure is the body as it travels (frame header excluded).
    let frame = Frame {
        frame_type: FrameType::Request,
        request_id: 1,
        body: cbor.clone(),
    };
    let framed = encode_frame(&frame, MAX_FRAME, true).unwrap();
    let zstd_len = framed.len() - HEADER_LEN;
    let cbor_saved = 100.0 * (1.0 - cbor.len() as f64 / json.len() as f64);
    let zstd_saved = 100.0 * (1.0 - zstd_len as f64 / json.len() as f64);
    println!(
        "{label:<18} {:>7} {:>7} {:>7.0}% {:>10} {:>7.0}%",
        json.len(),
        cbor.len(),
        cbor_saved,
        zstd_len,
        zstd_saved,
    );
}

fn main() {
    println!("SPINE wire format vs JSON — bytes on the wire (lower is better)\n");
    println!(
        "{:<18} {:>7} {:>7} {:>8} {:>10} {:>8}",
        "frame", "json", "cbor", "saved", "cbor+zstd", "saved"
    );

    row(
        "ToolCall",
        &Message::ToolCall(ToolCall {
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
        }),
    );

    row(
        "StreamToken/text",
        &Message::StreamToken(StreamToken {
            id: "stream-7f3a".into(),
            seq: 128,
            data: StreamData::Text(
                " The quick brown fox jumps over the lazy dog, and then keeps going.".into(),
            ),
            usage: None,
        }),
    );

    let embedding: Vec<u8> = (0..1024u32)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect();
    row(
        "StreamToken/emb",
        &Message::StreamToken(StreamToken {
            id: "stream-7f3a".into(),
            seq: 1,
            data: StreamData::Bytes(embedding),
            usage: None,
        }),
    );

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
    row(
        "CapabilityAd",
        &Message::CapabilityAd(CapabilityAdvertisement {
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
        }),
    );

    row(
        "Ping",
        &Message::Ping {
            timestamp: 1_700_000_000,
        },
    );

    println!();
    println!("Note: sizes are frame bodies; the v1 frame header adds 15 bytes.");
    println!("'cbor' is the hot-path default (cbor::to_vec): fast, and already dense.");
    println!("CBOR drops JSON's quotes, key punctuation and decimal-string number");
    println!("blowup, and ships tensor bytes as a CBOR byte string. 'cbor+zstd' is what");
    println!("the frame layer sends with compression on: bodies under 1 KiB are never");
    println!("compressed, so small frames show no gain. It trades a high fixed per-call");
    println!("CPU cost for bytes, for bandwidth-bound paths only.");
}
