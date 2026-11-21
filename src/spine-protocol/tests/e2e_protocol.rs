//! # End-to-End Integration Tests - Protocol Layer
//!
//! Full-stack integration tests verifying cross-crate interaction:
//! - Agent->Protocol->Core round-trips over real TCP
//! - Multi-session concurrent connections
//!
//! The server is a minimal spine-wire v1 echo: it completes the handshake,
//! answers every `REQUEST` with a `RESPONSE` and lets `Connection` answer
//! `PING`. Every network await has a timeout.

use serde_json::Value;
use spine_protocol::connection::Connection;
use spine_protocol::frame::{Frame, FrameType};
use spine_protocol::native;
use spine_protocol::BrowserCommand;
use std::future::Future;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

const LIMIT: Duration = Duration::from_secs(30);

/// Bounds a future so that a hang fails the test.
async fn within<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(LIMIT, future)
        .await
        .expect("operation timed out")
}

/// Spawn a minimal TCP server that echoes responses over a v1 `Connection`.
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
                let mut conn = match Connection::server_handshake(
                    socket,
                    &native::local_config(),
                    native::DEFAULT_HELLO_DEADLINE,
                )
                .await
                {
                    Ok(conn) => conn,
                    Err(_) => return,
                };
                loop {
                    let frame = match conn.recv().await {
                        Ok(Some(frame)) => frame,
                        Ok(None) | Err(_) => break,
                    };
                    if frame.frame_type != FrameType::Request {
                        continue;
                    }
                    let sent = match native::decode_command(&frame.body) {
                        Ok(_) => {
                            native::send_ok(
                                &mut conn,
                                frame.request_id,
                                &serde_json::json!({"echo": true}),
                            )
                            .await
                        }
                        Err(error) => {
                            native::send_err(&mut conn, frame.request_id, &error.to_wire()).await
                        }
                    };
                    if sent.is_err() {
                        break;
                    }
                }
            });
        }
    });

    port
}

/// Connect and complete the handshake.
async fn connect(port: u16) -> Connection<TcpStream> {
    let stream = within(TcpStream::connect(("127.0.0.1", port)))
        .await
        .unwrap();
    within(native::client_connect(stream)).await.unwrap()
}

/// Send a `PING` with `token` and wait for the matching `PONG`.
async fn ping(conn: &mut Connection<TcpStream>, id: u64, token: u64) -> u64 {
    let frame = Frame {
        frame_type: FrameType::Ping,
        request_id: id,
        body: native::ping_body(token).unwrap(),
    };
    within(conn.send(frame)).await.unwrap();
    loop {
        match within(conn.recv()).await.unwrap() {
            Some(frame) if frame.frame_type == FrameType::Pong && frame.request_id == id => {
                return native::ping_token(&frame.body).unwrap();
            }
            Some(_) => {}
            None => panic!("connection ended before the PONG"),
        }
    }
}

// ====== TCP Round-Trip ======

#[tokio::test]
async fn e2e_tcp_request_response() {
    let port = spawn_echo_server().await;
    let mut client = connect(port).await;

    let req = BrowserCommand::Navigate {
        url: "https://example.com".into(),
    };
    let id = within(native::send_command(&mut client, &req))
        .await
        .unwrap();
    // The frame request_id is the only correlation id.
    let resp: Value = within(native::await_reply(&mut client, id)).await.unwrap();
    assert_eq!(resp, serde_json::json!({"echo": true}));
}

/// A body that is not a command is answered with `err`, and the connection
/// survives it.
#[tokio::test]
async fn e2e_tcp_bad_commands_get_err_replies() {
    let port = spawn_echo_server().await;
    let mut client = connect(port).await;

    // CBOR text "bogus": a unit-variant shape naming no command.
    let body = vec![0x65, b'b', b'o', b'g', b'u', b's'];
    let id = within(client.request(body)).await.unwrap();
    match within(native::await_reply::<_, Value>(&mut client, id)).await {
        Err(native::NativeError::Remote { code, .. }) => assert_eq!(code, "unknown_command"),
        other => panic!("expected unknown_command, got {other:?}"),
    }

    // CBOR integer 7: not a command at all.
    let id = within(client.request(vec![0x07])).await.unwrap();
    match within(native::await_reply::<_, Value>(&mut client, id)).await {
        Err(native::NativeError::Remote { code, .. }) => assert_eq!(code, "invalid_request"),
        other => panic!("expected invalid_request, got {other:?}"),
    }

    // The connection still serves a good command.
    let resp: Value = within(native::call(&mut client, &BrowserCommand::GetUR))
        .await
        .unwrap();
    assert_eq!(resp, serde_json::json!({"echo": true}));
}

#[tokio::test]
async fn e2e_tcp_ping_pong() {
    let port = spawn_echo_server().await;
    let mut client = connect(port).await;

    for (id, ts) in [1u64, 42, 999, u64::MAX].into_iter().enumerate() {
        assert_eq!(ping(&mut client, id as u64 + 1, ts).await, ts);
    }
}

// ====== Multi-Session Concurrent ======

#[tokio::test]
async fn e2e_concurrent_tcp_sessions() {
    let port = spawn_echo_server().await;

    let mut handles = Vec::new();
    for session_id in 0..10u32 {
        handles.push(tokio::spawn(async move {
            let mut client = connect(port).await;

            for i in 0..5u32 {
                let id = within(native::send_command(&mut client, &BrowserCommand::GetUR))
                    .await
                    .unwrap();
                let resp: Value = within(native::await_reply(&mut client, id))
                    .await
                    .unwrap_or_else(|e| panic!("session {session_id} message {i}: {e}"));
                assert_eq!(resp, serde_json::json!({"echo": true}));
            }
        }));
    }

    for h in handles {
        within(h).await.unwrap();
    }
}

// ====== Stress / Reconnect ======

#[tokio::test]
async fn e2e_rapid_reconnect() {
    let port = spawn_echo_server().await;

    for attempt in 0..5u64 {
        let mut client = connect(port).await;
        assert_eq!(ping(&mut client, 1, attempt).await, attempt);
        let resp: Value = within(native::call(&mut client, &BrowserCommand::GetUR))
            .await
            .unwrap();
        assert_eq!(resp, serde_json::json!({"echo": true}));
    }
}

#[tokio::test]
async fn e2e_stress_tcp_100_messages() {
    let port = spawn_echo_server().await;
    let mut client = connect(port).await;

    for i in 0..100u64 {
        assert_eq!(ping(&mut client, i + 1, i).await, i);
        let resp: Value = within(native::call(&mut client, &BrowserCommand::GetUR))
            .await
            .unwrap();
        assert_eq!(resp, serde_json::json!({"echo": true}));
    }
    assert_eq!(client.in_flight_len(), 0);
}

// ====== All BrowserCommand Variants ======

#[tokio::test]
async fn e2e_all_command_types_over_tcp() {
    let port = spawn_echo_server().await;
    let mut client = connect(port).await;

    let commands = vec![
        BrowserCommand::Navigate {
            url: "https://example.com".into(),
        },
        BrowserCommand::GetUR,
        BrowserCommand::GetRawHTML,
        BrowserCommand::GetBinaryCapabilities,
    ];

    for (i, cmd) in commands.into_iter().enumerate() {
        let resp: Value = within(native::call(&mut client, &cmd))
            .await
            .unwrap_or_else(|e| panic!("command {i} ({cmd:?}): {e}"));
        assert_eq!(resp, serde_json::json!({"echo": true}), "command {i}");
    }
}

// ====== Handshake edge ======

#[tokio::test]
async fn e2e_server_refuses_a_client_that_skips_hello() {
    use spine_protocol::frame::{decode_frame, MAX_FRAME};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let port = spawn_echo_server().await;
    let mut stream = within(TcpStream::connect(("127.0.0.1", port)))
        .await
        .unwrap();
    // A pre-v1 style frame instead of HELLO: a length below the minimum.
    within(stream.write_all(&[0, 0, 0, 3, 0, b'{', b'}']))
        .await
        .unwrap();
    let mut sink = Vec::new();
    // The server says why (best effort) and closes, so the read ends.
    let _ = within(stream.read_to_end(&mut sink)).await;
    if !sink.is_empty() {
        let (frame, _) = decode_frame(&sink, MAX_FRAME).unwrap();
        assert_eq!(frame.frame_type, FrameType::Close);
    }
}
