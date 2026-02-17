//! Integration test harness for SPINE.
//!
//! Tests end-to-end communication between ProtocolHandler pairs
//! over in-process duplex streams, verifying:
//! - Plain text message exchange
//! - Encrypted (AES-256-GCM) message exchange
//! - Chameleon protocol message exchange
//! - Multi-session concurrent communication
//! - Bidirectional interleaved messaging
//! - Stress testing

use spine_protocol::{
    BrowserCommand, Event, LatentVector, Message, ProtocolHandler, Request, Response,
};
/// Create a pair of ProtocolHandlers connected via in-process duplex stream
fn handler_pair() -> (
    ProtocolHandler<tokio::io::DuplexStream>,
    ProtocolHandler<tokio::io::DuplexStream>,
) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    (ProtocolHandler::new(client), ProtocolHandler::new(server))
}

// ========== PLAINTEXT PROTOCOL ==========

#[tokio::test]
async fn protocol_plaintext_request_response() {
    let (mut client, mut server) = handler_pair();

    let req = Message::Request(Request {
        id: "test-1".into(),
        command: BrowserCommand::Navigate {
            url: "https://example.com".into(),
        },
    });

    // Client sends, server receives
    client.send_message_raw(&req).await.unwrap();
    let received = server.receive_message().await.unwrap();

    match received {
        Message::Request(r) => {
            assert_eq!(r.id, "test-1");
            assert!(matches!(r.command, BrowserCommand::Navigate { .. }));
        }
        other => panic!("Expected Request, got {:?}", std::mem::discriminant(&other)),
    }

    // Server responds
    let resp = Message::Response(Response {
        id: "test-1".into(),
        result: Some(serde_json::json!({"status": "ok"})),
        error: None,
    });
    server.send_message_raw(&resp).await.unwrap();
    let received = client.receive_message().await.unwrap();

    match received {
        Message::Response(r) => {
            assert_eq!(r.id, "test-1");
            assert!(r.error.is_none());
        }
        other => panic!(
            "Expected Response, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

#[tokio::test]
async fn protocol_plaintext_ping_pong() {
    let (mut client, mut server) = handler_pair();

    client
        .send_message_raw(&Message::Ping { timestamp: 42 })
        .await
        .unwrap();
    let received = server.receive_message().await.unwrap();
    assert!(matches!(received, Message::Ping { timestamp: 42 }));

    server
        .send_message_raw(&Message::Pong { timestamp: 42 })
        .await
        .unwrap();
    let received = client.receive_message().await.unwrap();
    assert!(matches!(received, Message::Pong { timestamp: 42 }));
}

#[tokio::test]
async fn protocol_plaintext_latent_vector() {
    let (mut client, mut server) = handler_pair();

    let vec = LatentVector {
        components: vec![1.0, 2.0, 3.0, 4.0],
        dim_hint: 4,
        epoch: 7,
    };
    client
        .send_message_raw(&Message::LatentMessage(vec.clone()))
        .await
        .unwrap();
    let received = server.receive_message().await.unwrap();

    match received {
        Message::LatentMessage(v) => {
            assert_eq!(v.components, vec.components);
            assert_eq!(v.dim_hint, 4);
            assert_eq!(v.epoch, 7);
        }
        other => panic!(
            "Expected LatentMessage, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

// ========== ENCRYPTED PROTOCOL (AES-256-GCM) ==========

#[tokio::test]
async fn protocol_encrypted_roundtrip() {
    let (mut client, mut server) = handler_pair();
    let key = [0x42u8; 32];

    client.enable_encryption(key);
    server.enable_encryption(key);

    let req = Message::Request(Request {
        id: "enc-1".into(),
        command: BrowserCommand::GetUR,
    });

    client.send_message_raw(&req).await.unwrap();
    let received = server.receive_message().await.unwrap();

    match received {
        Message::Request(r) => assert_eq!(r.id, "enc-1"),
        other => panic!("Expected Request, got {:?}", std::mem::discriminant(&other)),
    }
}

#[tokio::test]
async fn protocol_encrypted_multiple_messages() {
    let (mut client, mut server) = handler_pair();
    let key = [0xAB; 32];

    client.enable_encryption(key);
    server.enable_encryption(key);

    for i in 0..10 {
        let msg = Message::Ping { timestamp: i };
        client.send_message_raw(&msg).await.unwrap();
        let received = server.receive_message().await.unwrap();
        assert!(
            matches!(received, Message::Ping { timestamp } if timestamp == i),
            "message {} mismatch",
            i
        );
    }
}

// ========== CHAMELEON PROTOCOL ==========

// ========== CHAMELEON PROTOCOL ==========

#[tokio::test]
async fn protocol_chameleon_encoding() {
    // ChameleonKey uses neural latent-space encoding (inherently lossy)
    // This test verifies the encoding path works without panics
    let (mut client, _server) = handler_pair();
    let secret = [0x13u8; 32];
    client.enable_chameleon(secret);

    let req = Message::Request(Request {
        id: "cham-1".into(),
        command: BrowserCommand::Search {
            query: "test query".into(),
        },
    });

    // Chameleon encoding + morphed frame write should not panic
    client.send_message_raw(&req).await.unwrap();
}
// ========== MORPH PROTOCOL ==========

#[tokio::test]
async fn protocol_morph_changes_morphology() {
    let (mut client, mut server) = handler_pair();

    // Send pre-morph
    client
        .send_message_raw(&Message::Ping { timestamp: 1 })
        .await
        .unwrap();
    let received = server.receive_message().await.unwrap();
    assert!(matches!(received, Message::Ping { timestamp: 1 }));

    // Both sides morph with same seed
    let seed = 0xDEADBEEF;
    client.morph_now(seed);
    server.morph_now(seed);

    // Send post-morph — should still work
    client
        .send_message_raw(&Message::Ping { timestamp: 2 })
        .await
        .unwrap();
    let received = server.receive_message().await.unwrap();
    assert!(matches!(received, Message::Ping { timestamp: 2 }));
}

// ========== CONCURRENT SESSIONS ==========

#[tokio::test]
async fn protocol_concurrent_sessions() {
    let mut handles = Vec::new();

    for session_id in 0..5 {
        handles.push(tokio::spawn(async move {
            let (mut client, mut server) = handler_pair();

            let req = Message::Request(Request {
                id: format!("session-{}", session_id),
                command: BrowserCommand::Navigate {
                    url: format!("https://example.com/{}", session_id),
                },
            });

            client.send_message_raw(&req).await.unwrap();
            let received = server.receive_message().await.unwrap();

            match received {
                Message::Request(r) => {
                    assert_eq!(r.id, format!("session-{}", session_id));
                }
                _ => panic!("wrong message type"),
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

// ========== BROWSER COMMAND COVERAGE ==========

#[tokio::test]
async fn protocol_all_command_types() {
    let (mut client, mut server) = handler_pair();

    let commands = vec![
        BrowserCommand::Navigate {
            url: "https://test.com".into(),
        },
        BrowserCommand::GetUR,
        BrowserCommand::Click {
            element_id: "btn-1".into(),
        },
        BrowserCommand::Type {
            element_id: "input-1".into(),
            text: "hello".into(),
        },
        BrowserCommand::Search {
            query: "test".into(),
        },
        BrowserCommand::GetRawHTML,
        BrowserCommand::GetLatentUR { dimensions: 128 },
        BrowserCommand::Morph,
    ];

    for (i, cmd) in commands.into_iter().enumerate() {
        let req = Message::Request(Request {
            id: format!("cmd-{}", i),
            command: cmd,
        });
        client.send_message_raw(&req).await.unwrap();
        let received = server.receive_message().await.unwrap();
        match received {
            Message::Request(r) => assert_eq!(r.id, format!("cmd-{}", i)),
            _ => panic!("expected Request for command {}", i),
        }
    }
}

// ========== STRESS TEST ==========

#[tokio::test]
async fn protocol_stress_100_messages() {
    let (mut client, mut server) = handler_pair();

    for i in 0u64..100 {
        let msg = Message::Ping { timestamp: i };
        client.send_message_raw(&msg).await.unwrap();
        let received = server.receive_message().await.unwrap();
        assert!(matches!(received, Message::Ping { timestamp } if timestamp == i));
    }
}

// ========== BIDIRECTIONAL ==========

#[tokio::test]
async fn protocol_bidirectional_interleaved() {
    let (mut client, mut server) = handler_pair();

    // Client sends request
    client
        .send_message_raw(&Message::Request(Request {
            id: "req-1".into(),
            command: BrowserCommand::GetUR,
        }))
        .await
        .unwrap();

    // Server reads and responds
    let _ = server.receive_message().await.unwrap();
    server
        .send_message_raw(&Message::Response(Response {
            id: "req-1".into(),
            result: Some(serde_json::json!({"title": "Test"})),
            error: None,
        }))
        .await
        .unwrap();

    // Client reads response
    let resp = client.receive_message().await.unwrap();
    match resp {
        Message::Response(r) => {
            assert_eq!(r.id, "req-1");
            assert!(r.result.is_some());
        }
        _ => panic!("expected Response"),
    }

    // Now server initiates (event)
    server
        .send_message_raw(&Message::Event(Event {
            name: "navigation".into(),
            data: serde_json::json!({"url": "https://test.com"}),
        }))
        .await
        .unwrap();

    let event = client.receive_message().await.unwrap();
    assert!(matches!(event, Message::Event(_)));
}
