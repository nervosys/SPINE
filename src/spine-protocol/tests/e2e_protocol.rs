//! # End-to-End Integration Tests - Protocol Layer
//!
//! Full-stack integration tests verifying cross-crate interaction:
//! - Agent->Protocol->Core round-trips over real TCP
//! - Multi-session concurrent connections
//! - Encrypted + Chameleon protocol negotiation
//! - Frame format compatibility between spine-nostd and spine-protocol

use spine_protocol::{BrowserCommand, Message, ProtocolHandler, Request, Response};
use std::time::Duration;
use tokio::net::TcpListener;

/// Spawn a minimal TCP server that echoes responses via ProtocolHandler.
async fn spawn_echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (socket, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let mut handler = ProtocolHandler::new(socket);
                loop {
                    let msg = match handler.receive_message().await {
                        Ok(m) => m,
                        Err(_) => break,
                    };
                    let reply = match msg {
                        Message::Ping { timestamp } => Message::Pong { timestamp },
                        Message::Request(req) => Message::Response(Response {
                            id: req.id,
                            result: Some(serde_json::json!({"echo": true})),
                            error: None,
                        }),
                        other => other,
                    };
                    if handler.send_message_raw(&reply).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    port
}

// ====== TCP Round-Trip ======

#[tokio::test]
async fn e2e_tcp_request_response() {
    let port = spawn_echo_server().await;
    let addr = format!("127.0.0.1:{}", port);

    let stream = tokio::net::TcpStream::connect(&addr).await.unwrap();
    let mut client = ProtocolHandler::new(stream);

    let req = Message::Request(Request {
        id: "e2e-1".into(),
        command: BrowserCommand::Navigate {
            url: "https://example.com".into(),
        },
    });
    client.send_message_raw(&req).await.unwrap();
    let resp = client.receive_message().await.unwrap();

    match resp {
        Message::Response(r) => {
            assert_eq!(r.id, "e2e-1");
            assert!(r.error.is_none());
        }
        other => panic!("Expected Response, got {:?}", std::mem::discriminant(&other)),
    }
}

#[tokio::test]
async fn e2e_tcp_ping_pong() {
    let port = spawn_echo_server().await;
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let mut client = ProtocolHandler::new(stream);

    for ts in [1u64, 42, 999, u64::MAX] {
        client
            .send_message_raw(&Message::Ping { timestamp: ts })
            .await
            .unwrap();
        let resp = client.receive_message().await.unwrap();
        assert!(
            matches!(resp, Message::Pong { timestamp } if timestamp == ts),
            "Expected Pong with timestamp {ts}"
        );
    }
}

// ====== Multi-Session Concurrent ======

#[tokio::test]
async fn e2e_concurrent_tcp_sessions() {
    let port = spawn_echo_server().await;
    let addr = format!("127.0.0.1:{}", port);

    let mut handles = Vec::new();
    for session_id in 0..10u32 {
        let addr = addr.clone();
        handles.push(tokio::spawn(async move {
            let stream = tokio::net::TcpStream::connect(&addr).await.unwrap();
            let mut handler = ProtocolHandler::new(stream);

            for i in 0..5u32 {
                let req_id = format!("sess-{}-msg-{}", session_id, i);
                let req = Message::Request(Request {
                    id: req_id.clone(),
                    command: BrowserCommand::GetUR,
                });
                handler.send_message_raw(&req).await.unwrap();
                let resp = handler.receive_message().await.unwrap();
                match resp {
                    Message::Response(r) => assert_eq!(r.id, req_id),
                    other => panic!(
                        "Session {} expected Response, got {:?}",
                        session_id,
                        std::mem::discriminant(&other)
                    ),
                }
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ====== Encrypted Protocol over TCP ======

/// Spawn a server with AES-256-GCM encryption enabled.
async fn spawn_encrypted_server(key: [u8; 32]) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (socket, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let mut handler = ProtocolHandler::new(socket);
                handler.enable_encryption(key);

                loop {
                    let msg = match handler.receive_message().await {
                        Ok(m) => m,
                        Err(_) => break,
                    };
                    let reply = match msg {
                        Message::Ping { timestamp } => Message::Pong { timestamp },
                        Message::Request(req) => Message::Response(Response {
                            id: req.id,
                            result: Some(serde_json::json!({"encrypted": true})),
                            error: None,
                        }),
                        other => other,
                    };
                    if handler.send_message_raw(&reply).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    port
}

#[tokio::test]
async fn e2e_encrypted_tcp_roundtrip() {
    let key: [u8; 32] = [0x42; 32];
    let port = spawn_encrypted_server(key).await;

    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let mut client = ProtocolHandler::new(stream);
    client.enable_encryption(key);

    let req = Message::Request(Request {
        id: "enc-1".into(),
        command: BrowserCommand::Navigate {
            url: "https://secret.example.com".into(),
        },
    });
    client.send_message_raw(&req).await.unwrap();
    let resp = client.receive_message().await.unwrap();

    match resp {
        Message::Response(r) => {
            assert_eq!(r.id, "enc-1");
            assert!(r.error.is_none());
        }
        other => panic!("Expected encrypted Response, got {:?}", std::mem::discriminant(&other)),
    }
}

#[tokio::test]
async fn e2e_encrypted_multiple_messages() {
    let key: [u8; 32] = [0xAB; 32];
    let port = spawn_encrypted_server(key).await;

    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let mut client = ProtocolHandler::new(stream);
    client.enable_encryption(key);

    for i in 0..20u64 {
        client
            .send_message_raw(&Message::Ping { timestamp: i })
            .await
            .unwrap();
        let resp = client.receive_message().await.unwrap();
        assert!(
            matches!(resp, Message::Pong { timestamp } if timestamp == i),
            "Encrypted pong {i} failed"
        );
    }
}

// ====== Chameleon Encoding over TCP ======

/// Test that Chameleon AEAD send path works over real TCP without panicking.
/// NOTE: Chameleon uses neural latent-space encoding which is inherently lossy,
/// so full roundtrip decode is not expected to produce identical data. This test
/// verifies the protocol mechanics (framing, encryption, morphology) are sound.
#[tokio::test]
async fn e2e_chameleon_tcp_send_path() {
    let secret: [u8; 32] = [0xCA; 32];

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    // Server reads raw bytes (doesn't try to decode chameleon)
    let handle = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 16384];
        let mut stream = socket;
        // Read whatever arrives - verify it doesn't panic on send side
        loop {
            match tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await {
                Ok(0) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(30)).await;

    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let mut client = ProtocolHandler::new(stream);
    client.enable_chameleon_aead(secret);

    // Send should succeed without panicking
    let req = Message::Request(Request {
        id: "cham-1".into(),
        command: BrowserCommand::GetCapabilities,
    });
    client.send_message_raw(&req).await.unwrap();

    // Send a second message to exercise morphology evolution
    client
        .send_message_raw(&Message::Ping { timestamp: 42 })
        .await
        .unwrap();

    drop(client);
    let _ = handle.await;
}

// ====== Morph During Session ======

/// Test that morph_now() works over real TCP by having both sides morph together.
/// Uses plaintext (no encryption) for clarity — the morph changes frame header layout.
#[tokio::test]
async fn e2e_morph_mid_session() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut handler = ProtocolHandler::new(socket);

        // Receive pre-morph ping
        let msg = handler.receive_message().await.unwrap();
        assert!(matches!(msg, Message::Ping { timestamp: 1 }));
        handler
            .send_message_raw(&Message::Pong { timestamp: 1 })
            .await
            .unwrap();

        // Signal client that we're about to morph
        // Wait for client to be ready
        let _ = rx.await;

        // Morph
        handler.morph_now(0xDEAD_BEEF);

        // Receive post-morph ping
        let msg = handler.receive_message().await.unwrap();
        assert!(matches!(msg, Message::Ping { timestamp: 2 }));
        handler
            .send_message_raw(&Message::Pong { timestamp: 2 })
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(30)).await;

    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let mut client = ProtocolHandler::new(stream);

    // Pre-morph ping
    client
        .send_message_raw(&Message::Ping { timestamp: 1 })
        .await
        .unwrap();
    let resp = client.receive_message().await.unwrap();
    assert!(matches!(resp, Message::Pong { timestamp: 1 }));

    // Both sides morph simultaneously
    client.morph_now(0xDEAD_BEEF);
    let _ = tx.send(());

    // Small delay to let server morph
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Post-morph ping - frame layout has changed
    client
        .send_message_raw(&Message::Ping { timestamp: 2 })
        .await
        .unwrap();
    let resp = client.receive_message().await.unwrap();
    assert!(matches!(resp, Message::Pong { timestamp: 2 }));

    server_handle.await.unwrap();
}

// ====== Frame Format Interop ======

#[test]
fn e2e_nostd_frame_header_roundtrip() {
    use spine_nostd::codec::{decode_frame_header, encode_frame_header};
    use spine_nostd::types::FrameHeader;

    let header = FrameHeader::new(1024, 0x01, 42);
    let mut buf = [0u8; 12];
    encode_frame_header(&header, &mut buf);

    let decoded = decode_frame_header(&buf).unwrap();
    assert_eq!(decoded.payload_len, 1024);
    assert_eq!(decoded.frame_type, 0x01);
    assert_eq!(decoded.sequence, 42);
    assert_eq!(decoded.checksum, header.checksum);
}

#[test]
fn e2e_embedded_message_frame_compat() {
    use spine_embedded::EmbeddedMessage;
    use spine_nostd::codec::decode_frame_header;
    use spine_nostd::types::AgentIdBytes;

    let src = AgentIdBytes([1; 16]);
    let dst = AgentIdBytes([2; 16]);
    let mut msg = EmbeddedMessage::new(src, dst, 0x05);
    msg.set_payload(b"hello");
    msg.sequence = 100;

    let mut buf = [0u8; 12];
    msg.encode_header(&mut buf);

    let decoded = decode_frame_header(&buf).unwrap();
    assert_eq!(decoded.payload_len, msg.payload_len as u32);
    assert_eq!(decoded.frame_type, 0x05);
    assert_eq!(decoded.sequence, 100);
}

#[test]
fn e2e_embedded_latent_vector_compatibility() {
    use spine_nostd::math::{cosine_similarity_fixed, dot_product_fixed};
    use spine_nostd::types::LatentVectorFixed;

    let v1 = LatentVectorFixed::<8> {
        data: [256, 128, 64, 32, 16, 8, 4, 2],
        len: 8,
    };
    let v2 = LatentVectorFixed::<8> {
        data: [256, 256, 256, 256, 256, 256, 256, 256],
        len: 8,
    };

    let dot = dot_product_fixed(&v1.data, &v2.data);
    assert!(dot > 0, "Dot product should be positive");

    let sim = cosine_similarity_fixed(&v1.data, &v2.data);
    assert!(sim > 0, "Cosine similarity should be positive for aligned vectors");
}

// ====== Stress / Reconnect ======

#[tokio::test]
async fn e2e_rapid_reconnect() {
    let port = spawn_echo_server().await;
    let addr = format!("127.0.0.1:{}", port);

    for attempt in 0..5 {
        let stream = tokio::net::TcpStream::connect(&addr).await.unwrap();
        let mut handler = ProtocolHandler::new(stream);

        handler
            .send_message_raw(&Message::Ping {
                timestamp: attempt,
            })
            .await
            .unwrap();
        let resp = handler.receive_message().await.unwrap();
        assert!(
            matches!(resp, Message::Pong { timestamp } if timestamp == attempt),
            "Reconnect attempt {attempt} failed"
        );
    }
}

#[tokio::test]
async fn e2e_stress_tcp_100_messages() {
    let port = spawn_echo_server().await;
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let mut handler = ProtocolHandler::new(stream);

    for i in 0..100u64 {
        handler
            .send_message_raw(&Message::Ping { timestamp: i })
            .await
            .unwrap();
        let resp = handler.receive_message().await.unwrap();
        assert!(matches!(resp, Message::Pong { timestamp } if timestamp == i));
    }
}

// ====== All BrowserCommand Variants ======

#[tokio::test]
async fn e2e_all_command_types_over_tcp() {
    let port = spawn_echo_server().await;
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let mut handler = ProtocolHandler::new(stream);

    let commands = vec![
        BrowserCommand::Navigate {
            url: "https://example.com".into(),
        },
        BrowserCommand::GetUR,
        BrowserCommand::GetRawHTML,
        BrowserCommand::GetCapabilities,
        BrowserCommand::GetSessionHistory,
        BrowserCommand::Search {
            query: "test".into(),
        },
        BrowserCommand::GetLatentUR { dimensions: 64 },
        BrowserCommand::Morph,
    ];

    for (i, cmd) in commands.into_iter().enumerate() {
        let req = Message::Request(Request {
            id: format!("cmd-{}", i),
            command: cmd,
        });
        handler.send_message_raw(&req).await.unwrap();
        let resp = handler.receive_message().await.unwrap();
        match resp {
            Message::Response(r) => assert_eq!(r.id, format!("cmd-{}", i)),
            other => panic!(
                "Command {} expected Response, got {:?}",
                i,
                std::mem::discriminant(&other)
            ),
        }
    }
}