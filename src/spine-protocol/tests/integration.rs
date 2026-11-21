//! Integration test harness for SPINE.
//!
//! Tests end-to-end communication between spine-wire v1 `Connection` pairs
//! over in-process duplex streams, verifying:
//! - Request and response exchange
//! - Ping and pong
//! - Multi-session concurrent communication
//! - Bidirectional interleaved messaging within the sequential model
//! - Stress testing
//!
//! Every await on a stream has a timeout.

use spine_protocol::connection::Connection;
use spine_protocol::frame::{Frame, FrameType};
use spine_protocol::native;
use spine_protocol::{BrowserCommand, Message, Request, Response};
use std::future::Future;
use std::time::Duration;
use tokio::io::{duplex, DuplexStream};

const LIMIT: Duration = Duration::from_secs(30);

/// Bounds a future so that a hang fails the test.
async fn within<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(LIMIT, future)
        .await
        .expect("operation timed out")
}

type Conn = Connection<DuplexStream>;

/// Create a client and a server `Connection` joined by an in-process duplex
/// stream, handshake done.
async fn connection_pair() -> (Conn, Conn) {
    let (client, server) = duplex(64 * 1024);
    let (client, server) = within(async {
        let config = native::local_config();
        tokio::join!(
            Connection::client_handshake(client, native::client_hello(), LIMIT),
            Connection::server_handshake(server, &config, LIMIT)
        )
    })
    .await;
    (client.unwrap(), server.unwrap())
}

/// Receive the next `REQUEST` on `server`, returning its frame id and message.
async fn next_request(server: &mut Conn) -> (u64, Message) {
    let frame = within(server.recv()).await.unwrap().unwrap();
    assert_eq!(frame.frame_type, FrameType::Request);
    (
        frame.request_id,
        native::decode_request(&frame.body).unwrap(),
    )
}

// ========== REQUEST / RESPONSE ==========

#[tokio::test]
async fn protocol_request_response() {
    let (mut client, mut server) = connection_pair().await;

    let req = Message::Request(Request {
        id: "test-1".into(),
        command: BrowserCommand::Navigate {
            url: "https://example.com".into(),
        },
    });

    // Client sends, server receives
    let id = within(native::send_request(&mut client, &req))
        .await
        .unwrap();
    let (frame_id, received) = next_request(&mut server).await;
    assert_eq!(frame_id, id);

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
    within(native::send_reply(&mut server, frame_id, &resp))
        .await
        .unwrap();
    let received = within(native::await_reply(&mut client, id)).await.unwrap();

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
async fn protocol_ping_pong() {
    let (mut client, mut server) = connection_pair().await;

    let ping = Frame {
        frame_type: FrameType::Ping,
        request_id: 3,
        body: native::ping_body(42).unwrap(),
    };
    within(client.send(ping)).await.unwrap();

    // The server's recv answers the PING itself and keeps waiting, so drive
    // it in the background until the client hangs up.
    let server_task = tokio::spawn(async move {
        let end = within(server.recv()).await;
        assert!(matches!(end, Ok(None)));
    });

    let pong = within(client.recv()).await.unwrap().unwrap();
    assert_eq!(pong.frame_type, FrameType::Pong);
    assert_eq!(pong.request_id, 3);
    assert_eq!(native::ping_token(&pong.body).unwrap(), 42);

    drop(client);
    within(server_task).await.unwrap();
}

// ========== CONCURRENT SESSIONS ==========

#[tokio::test]
async fn protocol_concurrent_sessions() {
    let mut handles = Vec::new();

    for session_id in 0..5 {
        handles.push(tokio::spawn(async move {
            let (mut client, mut server) = connection_pair().await;

            let req = Message::Request(Request {
                id: format!("session-{}", session_id),
                command: BrowserCommand::Navigate {
                    url: format!("https://example.com/{}", session_id),
                },
            });

            within(native::send_request(&mut client, &req))
                .await
                .unwrap();
            let (_, received) = next_request(&mut server).await;

            match received {
                Message::Request(r) => {
                    assert_eq!(r.id, format!("session-{}", session_id));
                }
                _ => panic!("wrong message type"),
            }
        }));
    }

    for handle in handles {
        within(handle).await.unwrap();
    }
}

// ========== BROWSER COMMAND COVERAGE ==========

#[tokio::test]
async fn protocol_all_command_types() {
    let (mut client, mut server) = connection_pair().await;

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
        within(native::send_request(&mut client, &req))
            .await
            .unwrap();
        let (frame_id, received) = next_request(&mut server).await;
        match received {
            Message::Request(r) => assert_eq!(r.id, format!("cmd-{}", i)),
            _ => panic!("expected Request for command {}", i),
        }
        // The sequential server must answer before the next request.
        within(native::send_reply(
            &mut server,
            frame_id,
            &native::error_reply("done"),
        ))
        .await
        .unwrap();
        within(native::await_reply(&mut client, frame_id))
            .await
            .unwrap();
    }
}

// ========== STRESS TEST ==========

#[tokio::test]
async fn protocol_stress_100_messages() {
    let (mut client, mut server) = connection_pair().await;

    for i in 0u64..100 {
        let req = Message::Request(Request {
            id: i.to_string(),
            command: BrowserCommand::GetUR,
        });
        let id = within(native::send_request(&mut client, &req))
            .await
            .unwrap();
        let (frame_id, received) = next_request(&mut server).await;
        assert!(matches!(received, Message::Request(r) if r.id == i.to_string()));
        within(native::send_reply(
            &mut server,
            frame_id,
            &native::error_reply("ok"),
        ))
        .await
        .unwrap();
        within(native::await_reply(&mut client, id)).await.unwrap();
    }
    assert_eq!(client.in_flight_len(), 0);
    assert_eq!(server.in_flight_len(), 0);
}

// ========== BIDIRECTIONAL ==========

#[tokio::test]
async fn protocol_bidirectional_interleaved() {
    let (mut client, mut server) = connection_pair().await;

    // Client sends request
    let id = within(native::send_request(
        &mut client,
        &Message::Request(Request {
            id: "req-1".into(),
            command: BrowserCommand::GetUR,
        }),
    ))
    .await
    .unwrap();

    // Server reads and responds
    let (frame_id, _) = next_request(&mut server).await;
    within(native::send_reply(
        &mut server,
        frame_id,
        &Message::Response(Response {
            id: "req-1".into(),
            result: Some(serde_json::json!({"title": "Test"})),
            error: None,
        }),
    ))
    .await
    .unwrap();

    // Client reads response
    let resp = within(native::await_reply(&mut client, id)).await.unwrap();
    match resp {
        Message::Response(r) => {
            assert_eq!(r.id, "req-1");
            assert!(r.result.is_some());
        }
        _ => panic!("expected Response"),
    }

    // Now the server initiates: a PING. The client answers it inside recv and
    // keeps waiting, so the server sees the PONG and the client sees nothing.
    let ping = Frame {
        frame_type: FrameType::Ping,
        request_id: 9,
        body: native::ping_body(9).unwrap(),
    };
    within(server.send(ping)).await.unwrap();

    let client_task = tokio::spawn(async move {
        let end = within(client.recv()).await;
        assert!(matches!(end, Ok(None)));
    });

    let pong = within(server.recv()).await.unwrap().unwrap();
    assert_eq!(pong.frame_type, FrameType::Pong);
    assert_eq!(native::ping_token(&pong.body).unwrap(), 9);

    // Hang up: the client's recv ends cleanly.
    drop(server);
    within(client_task).await.unwrap();
}
