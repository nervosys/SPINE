//! # End-to-End Integration Tests - Protocol Layer
//!
//! Full-stack integration tests verifying cross-crate interaction:
//! - Agent->Protocol->Core round-trips over real TCP
//! - Multi-session concurrent connections

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
        other => panic!(
            "Expected Response, got {:?}",
            std::mem::discriminant(&other)
        ),
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

// ====== Stress / Reconnect ======

#[tokio::test]
async fn e2e_rapid_reconnect() {
    let port = spawn_echo_server().await;
    let addr = format!("127.0.0.1:{}", port);

    for attempt in 0..5 {
        let stream = tokio::net::TcpStream::connect(&addr).await.unwrap();
        let mut handler = ProtocolHandler::new(stream);

        handler
            .send_message_raw(&Message::Ping { timestamp: attempt })
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
        BrowserCommand::GetBinaryCapabilities,
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
