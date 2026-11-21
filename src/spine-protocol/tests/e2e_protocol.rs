//! # End-to-End Integration Tests - Protocol Layer
//!
//! Full-stack integration tests verifying cross-crate interaction:
//! - Agent->Protocol->Core round-trips over real TCP
//! - Multi-session concurrent connections
//!
//! The server is a minimal spine-wire v1 echo: it completes the handshake,
//! answers every `REQUEST` with a `RESPONSE` and lets `Connection` answer
//! `PING`. Every network await has a timeout.

use spine_protocol::connection::Connection;
use spine_protocol::frame::{Frame, FrameType};
use spine_protocol::native;
use spine_protocol::{BrowserCommand, Message, Request, Response};
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
                    let reply = match native::decode_request(&frame.body) {
                        Ok(Message::Request(req)) => Message::Response(Response {
                            id: req.id,
                            result: Some(serde_json::json!({"echo": true})),
                            error: None,
                        }),
                        Ok(_) => native::error_reply("unsupported"),
                        Err(error) => native::error_reply(error.to_string()),
                    };
                    if native::send_reply(&mut conn, frame.request_id, &reply)
                        .await
                        .is_err()
                    {
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

fn request(id: impl Into<String>, command: BrowserCommand) -> Message {
    Message::Request(Request {
        id: id.into(),
        command,
    })
}

// ====== TCP Round-Trip ======

#[tokio::test]
async fn e2e_tcp_request_response() {
    let port = spawn_echo_server().await;
    let mut client = connect(port).await;

    let req = request(
        "e2e-1",
        BrowserCommand::Navigate {
            url: "https://example.com".into(),
        },
    );
    let resp = within(native::call(&mut client, &req)).await.unwrap();

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
                let req_id = format!("sess-{}-msg-{}", session_id, i);
                let req = request(req_id.clone(), BrowserCommand::GetUR);
                let resp = within(native::call(&mut client, &req)).await.unwrap();
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
        let req = request(format!("r-{attempt}"), BrowserCommand::GetUR);
        let resp = within(native::call(&mut client, &req)).await.unwrap();
        assert!(matches!(resp, Message::Response(r) if r.id == format!("r-{attempt}")));
    }
}

#[tokio::test]
async fn e2e_stress_tcp_100_messages() {
    let port = spawn_echo_server().await;
    let mut client = connect(port).await;

    for i in 0..100u64 {
        assert_eq!(ping(&mut client, i + 1, i).await, i);
        let req = request(format!("s-{i}"), BrowserCommand::GetUR);
        let resp = within(native::call(&mut client, &req)).await.unwrap();
        assert!(matches!(resp, Message::Response(r) if r.id == format!("s-{i}")));
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
        let req = request(format!("cmd-{}", i), cmd);
        let resp = within(native::call(&mut client, &req)).await.unwrap();
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
