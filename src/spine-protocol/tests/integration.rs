//! Integration test harness for SPINE.
//!
//! Tests end-to-end communication between ProtocolHandler pairs
//! over in-process duplex streams, verifying:
//! - Plain text message exchange
//! - Multi-session concurrent communication
//! - Bidirectional interleaved messaging
//! - Stress testing

use spine_protocol::{BrowserCommand, Message, ProtocolHandler, Request, Response};
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
        BrowserCommand::GetRawHTML,
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

    // Now server initiates (a ping)
    server
        .send_message_raw(&Message::Ping { timestamp: 9 })
        .await
        .unwrap();

    let ping = client.receive_message().await.unwrap();
    assert!(matches!(ping, Message::Ping { timestamp: 9 }));
}
