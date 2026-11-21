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

use serde_json::Value;
use spine_protocol::connection::Connection;
use spine_protocol::frame::{Frame, FrameType};
use spine_protocol::native;
use spine_protocol::BrowserCommand;
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

/// Receive the next `REQUEST` on `server`, returning its frame id and command.
async fn next_request(server: &mut Conn) -> (u64, BrowserCommand) {
    let frame = within(server.recv()).await.unwrap().unwrap();
    assert_eq!(frame.frame_type, FrameType::Request);
    (
        frame.request_id,
        native::decode_command(&frame.body).unwrap(),
    )
}

// ========== REQUEST / RESPONSE ==========

#[tokio::test]
async fn protocol_request_response() {
    let (mut client, mut server) = connection_pair().await;

    let req = BrowserCommand::Navigate {
        url: "https://example.com".into(),
    };

    // Client sends, server receives
    let id = within(native::send_command(&mut client, &req))
        .await
        .unwrap();
    let (frame_id, received) = next_request(&mut server).await;
    // The frame request_id is the correlation id; bodies carry none.
    assert_eq!(frame_id, id);
    assert_eq!(received, req);

    // Server responds
    within(native::send_ok(
        &mut server,
        frame_id,
        &serde_json::json!({"status": "ok"}),
    ))
    .await
    .unwrap();
    let received: Value = within(native::await_reply(&mut client, id)).await.unwrap();
    assert_eq!(received, serde_json::json!({"status": "ok"}));
}

/// An `err` reply reaches the caller as `NativeError::Remote` with its code.
#[tokio::test]
async fn protocol_error_reply_is_remote_error() {
    use spine_protocol::body::{WireError, ERR_NO_PAGE};

    let (mut client, mut server) = connection_pair().await;
    let id = within(native::send_command(&mut client, &BrowserCommand::GetUR))
        .await
        .unwrap();
    let (frame_id, _) = next_request(&mut server).await;
    within(native::send_err(
        &mut server,
        frame_id,
        &WireError::new(ERR_NO_PAGE, "no page"),
    ))
    .await
    .unwrap();
    match within(native::await_reply::<_, Value>(&mut client, id)).await {
        Err(native::NativeError::Remote { code, message, .. }) => {
            assert_eq!(code, ERR_NO_PAGE);
            assert_eq!(message, "no page");
        }
        other => panic!("expected a remote error, got {other:?}"),
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

            let req = BrowserCommand::Navigate {
                url: format!("https://example.com/{}", session_id),
            };

            within(native::send_command(&mut client, &req))
                .await
                .unwrap();
            let (_, received) = next_request(&mut server).await;
            assert_eq!(received, req);
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
        within(native::send_command(&mut client, &cmd))
            .await
            .unwrap();
        let (frame_id, received) = next_request(&mut server).await;
        assert_eq!(received, cmd, "command {i}");
        // The sequential server must answer before the next request.
        within(native::send_ok(
            &mut server,
            frame_id,
            &serde_json::json!("done"),
        ))
        .await
        .unwrap();
        let done: Value = within(native::await_reply(&mut client, frame_id))
            .await
            .unwrap();
        assert_eq!(done, serde_json::json!("done"));
    }
}

// ========== STRESS TEST ==========

#[tokio::test]
async fn protocol_stress_100_messages() {
    let (mut client, mut server) = connection_pair().await;

    for i in 0u64..100 {
        let id = within(native::send_command(&mut client, &BrowserCommand::GetUR))
            .await
            .unwrap();
        let (frame_id, received) = next_request(&mut server).await;
        assert_eq!(frame_id, id);
        assert_eq!(received, BrowserCommand::GetUR);
        within(native::send_ok(
            &mut server,
            frame_id,
            &serde_json::json!({"i": i}),
        ))
        .await
        .unwrap();
        let reply: Value = within(native::await_reply(&mut client, id)).await.unwrap();
        assert_eq!(reply["i"], i);
    }
    assert_eq!(client.in_flight_len(), 0);
    assert_eq!(server.in_flight_len(), 0);
}

// ========== BIDIRECTIONAL ==========

#[tokio::test]
async fn protocol_bidirectional_interleaved() {
    let (mut client, mut server) = connection_pair().await;

    // Client sends request
    let id = within(native::send_command(&mut client, &BrowserCommand::GetUR))
        .await
        .unwrap();

    // Server reads and responds
    let (frame_id, _) = next_request(&mut server).await;
    within(native::send_ok(
        &mut server,
        frame_id,
        &serde_json::json!({"title": "Test"}),
    ))
    .await
    .unwrap();

    // Client reads response
    let resp: Value = within(native::await_reply(&mut client, id)).await.unwrap();
    assert_eq!(resp["title"], "Test");

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
