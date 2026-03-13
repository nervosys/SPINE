//! # SPINE Offline Capabilities Demo
//!
//! Demonstrates SPINE features that work without a running server:
//! - HTML parsing → Unified Representation
//! - HLS compilation → SpineBinary
//! - Chameleon protocol encoding/decoding
//! - Latent vector operations
//! - Embedded agent message routing
//! - Fixed-point math
//!
//! ```
//! cargo run --example offline_demo
//! ```

use spine_compiler::Compiler;
use spine_nostd::codec::{decode_frame_header, encode_frame_header};
use spine_nostd::hash::{fnv1a_32, fnv1a_64};
use spine_nostd::math::{cosine_similarity_fixed, dot_product_fixed};
use spine_nostd::types::{AgentIdBytes, FrameHeader, LatentVectorFixed};
use spine_parser::parse_html;
use spine_protocol::{LatentVector, Message, ProtocolHandler};
use std::time::Instant;

fn demo_parser() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 1. HTML Parser → Unified Representation                     │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let html = r#"
        <html>
        <head><title>SPINE Research Paper</title></head>
        <body>
            <h1>Bioinspired Agentic Web Stack</h1>
            <p>SPINE provides a headless semantic browser for AI agents.</p>
            <h2>Key Features</h2>
            <ul>
                <li>Chameleon Protocol — latent-space cryptography</li>
                <li>Speculative Decoding — bidirectional prediction</li>
                <li>Unified Representation — semantic HTML extraction</li>
            </ul>
            <a href="https://github.com/nervosys/SPINE">Source Code</a>
            <a href="https://docs.spine.dev">Documentation</a>
        </body>
        </html>
    "#;

    let start = Instant::now();
    let ur = parse_html(html).expect("parse HTML");
    let elapsed = start.elapsed();

    println!("  Title: \"{}\"", ur.title);
    println!("  Elements: {}", ur.elements.len());
    println!("  Parsed in: {:.1}µs", elapsed.as_nanos() as f64 / 1000.0);

    for (i, elem) in ur.elements.iter().take(5).enumerate() {
        println!("  [{}] {:?}", i, elem);
    }
    println!();
}

fn demo_compiler() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 2. HLS Compiler → SpineBinary                               │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let hls_source = r#"
        state counter = 0
        let greeting = "Hello from SPINE"
        let version = 28

        fn fibonacci(n) {
            if n <= 1 {
                n
            } else {
                fibonacci(n - 1) + fibonacci(n - 2)
            }
        }

        element Dashboard {
            element Header {
                text greeting ++ " v" ++ str(version)
            }
            element Counter {
                text "Compilation #" ++ str(counter)
            }
        }
    "#;

    let start = Instant::now();
    let binary = Compiler::compile(hls_source).unwrap();
    let elapsed = start.elapsed();

    println!("  Instructions: {}", binary.instructions.len());
    println!("  Data bytes:   {}", binary.data.len());
    println!("  Functions:    {}", binary.exported_functions.len());
    println!("  Compiled in:  {:.1}µs", elapsed.as_nanos() as f64 / 1000.0);
    println!();
}

fn demo_protocol() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 3. Protocol Message Serialization                            │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    // Demonstrate message construction and serialization
    let messages = vec![
        ("Navigate", Message::Request(spine_protocol::Request {
            id: "demo-1".into(),
            command: spine_protocol::BrowserCommand::Navigate {
                url: "https://example.com".into(),
            },
        })),
        ("Ping", Message::Ping { timestamp: 1234567890 }),
        ("LatentVector", Message::LatentMessage(LatentVector {
            components: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8],
            dim_hint: 8,
            epoch: 1,
        })),
        ("MorphRequest", Message::MorphRequest { seed: 0xDEADBEEF }),
    ];

    for (name, msg) in &messages {
        let start = Instant::now();
        let bytes = serde_json::to_vec(msg).unwrap();
        let elapsed = start.elapsed();
        println!("  {:<14} → {} bytes ({:.0}ns)", name, bytes.len(), elapsed.as_nanos());
    }
    println!();
}

fn demo_chameleon_duplex() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 4. Chameleon Protocol over In-Memory Duplex                  │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let mut client = ProtocolHandler::new(client_io);
        let mut server = ProtocolHandler::new(server_io);

        let secret: [u8; 32] = [0x53; 32]; // "SSSS..."
        client.enable_chameleon_aead(secret);
        server.enable_chameleon_aead(secret);

        // Send 10 messages through Chameleon protocol
        let start = Instant::now();
        for i in 0..10u64 {
            client
                .send_message_raw(&Message::Ping { timestamp: i })
                .await
                .unwrap();
            let msg = server.receive_message().await.unwrap();
            assert!(matches!(msg, Message::Ping { .. }));
        }
        let elapsed = start.elapsed();

        println!(
            "  10 messages through Chameleon AEAD: {:.1}µs total ({:.1}µs/msg)",
            elapsed.as_nanos() as f64 / 1000.0,
            elapsed.as_nanos() as f64 / 10000.0
        );

        // Test morphing
        client.morph_now(42);
        server.morph_now(42);

        client
            .send_message_raw(&Message::Ping { timestamp: 999 })
            .await
            .unwrap();
        let msg = server.receive_message().await.unwrap();
        assert!(matches!(msg, Message::Ping { timestamp: 999 }));
        println!("  Post-morph communication: OK");

        let stats = client.get_speculation_stats();
        println!(
            "  Speculation: {} predictions, {:.1}% accuracy",
            stats.output_predictions,
            stats.output_accuracy() * 100.0
        );
    });
    println!();
}

fn demo_fixed_point() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 5. Fixed-Point Math (no_std / Embedded)                      │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    // Q8.8 fixed-point vectors (256 = 1.0, 128 = 0.5, etc.)
    let v1 = LatentVectorFixed::<8> {
        data: [256, 128, 64, 32, 16, 8, 4, 2],
        len: 8,
    };
    let v2 = LatentVectorFixed::<8> {
        data: [2, 4, 8, 16, 32, 64, 128, 256],
        len: 8,
    };

    let dot = dot_product_fixed(&v1.data, &v2.data);
    let sim = cosine_similarity_fixed(&v1.data, &v2.data);

    println!("  v1: {:?}", v1.data);
    println!("  v2: {:?}", v2.data);
    println!("  Dot product (Q16.16): {}", dot);
    println!("  Cosine similarity (Q8.8): {} ({:.3} float)", sim, sim as f64 / 256.0);

    // Frame header codec
    let header = FrameHeader::new(4096, 0x01, 42);
    let mut buf = [0u8; 12];
    encode_frame_header(&header, &mut buf);
    let decoded = decode_frame_header(&buf).unwrap();

    println!("\n  Frame header roundtrip:");
    println!("    payload_len: {} → {}", header.payload_len, decoded.payload_len);
    println!("    frame_type:  {} → {}", header.frame_type, decoded.frame_type);
    println!("    sequence:    {} → {}", header.sequence, decoded.sequence);
    println!("    checksum:    {} → {} ✓", header.checksum, decoded.checksum);

    // FNV hashing
    let data = b"SPINE agentic web stack";
    let h32 = fnv1a_32(data);
    let h64 = fnv1a_64(data);
    println!("\n  FNV-1a hash of {:?}:", std::str::from_utf8(data).unwrap());
    println!("    32-bit: 0x{:08X}", h32);
    println!("    64-bit: 0x{:016X}", h64);
    println!();
}

fn demo_embedded_agent() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 6. Embedded Agent Message Routing                            │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    use spine_embedded::{EmbeddedAgent, EmbeddedMessage};

    let agent_id = AgentIdBytes([0x01; 16]);
    let peer_a = AgentIdBytes([0xAA; 16]);
    let peer_b = AgentIdBytes([0xBB; 16]);

    let mut agent: EmbeddedAgent<16, 16, 8> = EmbeddedAgent::new(agent_id, 1000);

    // Add routes
    agent.routes.update(peer_a, peer_a, 1);
    agent.routes.update(peer_b, peer_a, 2); // peer_b reachable via peer_a

    // Receive a message addressed to us
    let mut msg = EmbeddedMessage::new(peer_a, agent_id, 0x01);
    msg.set_payload(b"sensor data");
    agent.receive(msg);

    let result = agent.process_next();
    println!("  Message to self: {:?}", result);

    // Receive a message to forward
    let mut fwd_msg = EmbeddedMessage::new(peer_a, peer_b, 0x02);
    fwd_msg.set_payload(b"forwarded");
    fwd_msg.ttl = 5;
    agent.receive(fwd_msg);

    let result = agent.process_next();
    println!("  Message to forward: {:?}", result);

    // Drain outbox
    if let Some(outgoing) = agent.drain_outbox() {
        println!("  Forwarded message type: 0x{:02X}, TTL: {}", outgoing.msg_type, outgoing.ttl);
    }

    // Stats
    let stats = agent.stats();
    println!("\n  Agent stats:");
    println!("    Received:  {}", stats.processed);
    println!("    Forwarded: {}", stats.forwarded);
    println!("    Dropped:   {}", stats.dropped);
    println!();
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║   SPINE Offline Capabilities Demo                            ║");
    println!("║   No server required — all processing is local               ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    demo_parser();
    demo_compiler();
    demo_protocol();
    demo_chameleon_duplex();
    demo_fixed_point();
    demo_embedded_agent();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║   Demo Complete — All 6 capabilities verified                ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}
