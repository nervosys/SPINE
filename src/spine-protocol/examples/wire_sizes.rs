//! Measure SPINE's binary wire format against UTF-8 JSON, frame by frame.
//!
//! Prints, for each representative agent message, the JSON body size, the SPINE
//! wire frame size (CBOR or CBOR+zstd, 8-byte header included), the chosen
//! codec, and the size ratio. This is the data behind SPINE's
//! encoding-efficiency claim — run it and read the numbers off the wire.
//!
//! Run: `cargo run -p spine-protocol --example wire_sizes`

use serde_json::json;
use spine_protocol::wire::{self, FORMAT_CBOR, FORMAT_CBOR_ZSTD, FORMAT_JSON};
use spine_protocol::{
    Capability, CapabilityAdvertisement, DType, EncodedFrame, EncodedMetadata, Message, Modality,
    StreamData, StreamToken, ToolCall,
};

fn codec_name(format: u8) -> &'static str {
    match format {
        FORMAT_JSON => "json",
        FORMAT_CBOR => "cbor",
        FORMAT_CBOR_ZSTD => "cbor+zstd",
        _ => "?",
    }
}

fn row(label: &str, msg: &Message) {
    let json = serde_json::to_vec(msg).unwrap();
    let frame = wire::encode(msg).unwrap();
    let ratio = frame.len() as f64 / json.len() as f64;
    let saved = 100.0 * (1.0 - ratio);
    println!(
        "{label:<18} {:>7} {:>9} {:>11} {:>7.2} {:>8.0}%",
        json.len(),
        frame.len(),
        codec_name(frame[3]),
        ratio,
        saved,
    );
}

fn main() {
    println!("SPINE wire format vs JSON — bytes on the wire (lower is better)\n");
    println!(
        "{:<18} {:>7} {:>9} {:>11} {:>7} {:>9}",
        "frame", "json", "spine", "codec", "ratio", "saved"
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

    let embedding: Vec<u8> = (0..1024u32).map(|i| (i.wrapping_mul(31) % 251) as u8).collect();
    row(
        "EncodedFrame/emb",
        &Message::Encoded(EncodedFrame {
            codec: "spine:codec/titans/v1@dim=256,dtype=f32".into(),
            variant: Some("layer=11".into()),
            data: embedding,
            metadata: EncodedMetadata {
                modality: Modality::Embedding,
                shape: vec![256],
                dtype: DType::F32,
                original_len: Some(4096),
                source_hash: Some([0xAB; 32]),
            },
            trace: None,
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
                cap("agent.web/fetch_url", "Fetch a URL and return the response body."),
                cap("agent.web/post_json", "POST a JSON document to an endpoint."),
            ],
        }),
    );

    row("Ping", &Message::Ping { timestamp: 1_700_000_000 });

    println!(
        "\nNote: 'spine' includes the 8-byte SpineWireHeader. CBOR drops JSON's\n\
         quotes, key punctuation, and decimal-string number blowup; payloads past\n\
         {} bytes are additionally zstd-compressed.",
        wire::ZSTD_THRESHOLD
    );
}
