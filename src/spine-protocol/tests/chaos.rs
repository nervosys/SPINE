//! Chaos Testing Framework for SPINE
//!
//! Injects controlled failures into spine-wire v1 communication to verify
//! resilience and graceful degradation:
//!
//! - **Connection drops**: Abrupt stream closure at random points
//! - **Byte corruption**: Garbage and truncated frames
//! - **Large payloads**: Frames at and over the negotiated limit
//! - **Bidirectional traffic**: Requests, replies and pings in both directions
//!
//! Every network await is bounded by a timeout, so a regression shows up as a
//! failed test and not as a hung one.

use proptest::prelude::*;
use spine_protocol::connection::{Connection, ConnectionError};
use spine_protocol::frame::{
    read_frame, write_frame, CloseCode, Frame, FrameType, MAX_FRAME, PRE_HELLO_MAX_FRAME,
};
use spine_protocol::hello::Close;
use spine_protocol::native;
use spine_protocol::{BrowserCommand, Message, Request};
use std::future::Future;
use std::time::Duration;
use tokio::io::{duplex, AsyncWriteExt, DuplexStream};

const LIMIT: Duration = Duration::from_secs(30);

/// Bounds a future so that a hang fails the test.
async fn within<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(LIMIT, future)
        .await
        .expect("operation timed out")
}

type Conn = Connection<DuplexStream>;

/// A client and a server `Connection` joined by an in-process stream.
async fn pair(buffer: usize) -> (Conn, Conn) {
    let (a, b) = duplex(buffer);
    let (client, server) = within(async {
        let config = native::local_config();
        tokio::join!(
            Connection::client_handshake(a, native::client_hello(), LIMIT),
            Connection::server_handshake(b, &config, LIMIT)
        )
    })
    .await;
    (client.unwrap(), server.unwrap())
}

/// A real server and a raw peer that has already completed the handshake by
/// hand, so the test can write arbitrary bytes after it.
async fn server_with_raw_peer() -> (Conn, DuplexStream) {
    let (mut peer, ours) = duplex(64 * 1024);
    let hello = native::client_hello().to_frame().unwrap();
    within(write_frame(&mut peer, &hello, PRE_HELLO_MAX_FRAME, false))
        .await
        .unwrap();
    let server = within(Connection::server_handshake(
        ours,
        &native::local_config(),
        LIMIT,
    ))
    .await
    .unwrap();
    let ack = within(read_frame(&mut peer, MAX_FRAME))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ack.frame_type, FrameType::HelloAck);
    (server, peer)
}

fn ping_request(timestamp: u64) -> Message {
    Message::Request(Request {
        id: timestamp.to_string(),
        command: BrowserCommand::GetUR,
    })
}

fn navigate(id: &str, url: String) -> Message {
    Message::Request(Request {
        id: id.into(),
        command: BrowserCommand::Navigate { url },
    })
}

/// Server side of one exchange: reads a request and answers it.
async fn serve_one(server: &mut Conn) -> Message {
    let frame = within(server.recv()).await.unwrap().unwrap();
    assert_eq!(frame.frame_type, FrameType::Request);
    let request = native::decode_request(&frame.body).unwrap();
    within(native::send_reply(
        server,
        frame.request_id,
        &native::error_reply("served"),
    ))
    .await
    .unwrap();
    request
}

// ========== CONNECTION DROP TESTS ==========

#[tokio::test]
async fn chaos_connection_drop_mid_session() {
    let (mut client, mut server) = pair(64 * 1024).await;

    // A few exchanges succeed.
    for i in 0..3u64 {
        let (reply, _) = within(async {
            let request = ping_request(i);
            tokio::join!(native::call(&mut client, &request), serve_one(&mut server))
        })
        .await;
        reply.unwrap();
    }

    // Drop the server side.
    drop(server);

    // The next send may succeed (buffered) or fail; it must not panic, and
    // the client must see the end of the stream and not hang.
    let _ = within(native::send_request(&mut client, &ping_request(99))).await;
    let next = within(client.recv()).await;
    assert!(
        matches!(next, Ok(None) | Err(_)),
        "a dropped server cannot deliver a frame"
    );
}

#[tokio::test]
async fn chaos_abrupt_client_drop() {
    let (mut client, mut server) = pair(64 * 1024).await;

    // Client sends one request then vanishes.
    let sent = within(native::send_request(&mut client, &ping_request(1)))
        .await
        .unwrap();
    let frame = within(server.recv()).await.unwrap().unwrap();
    assert_eq!(frame.request_id, sent);

    drop(client);

    // The server sees the end of the stream at a frame boundary, not a panic.
    let result = within(server.recv()).await;
    assert!(
        matches!(result, Ok(None)),
        "expected a clean end of stream, got {result:?}"
    );
    assert!(server.is_finished());
}

// ========== CORRUPTED DATA TESTS ==========

#[tokio::test]
async fn chaos_garbage_before_the_handshake() {
    let (mut raw_writer, server_stream) = duplex(64 * 1024);
    raw_writer.write_all(&[0xFF; 64]).await.unwrap();
    raw_writer.flush().await.unwrap();

    let result = within(Connection::server_handshake(
        server_stream,
        &native::local_config(),
        LIMIT,
    ))
    .await;
    assert!(result.is_err(), "garbage is not a HELLO");
}

#[tokio::test]
async fn chaos_corrupted_frame_header() {
    let (mut server, mut peer) = server_with_raw_peer().await;

    // A length of 0xFFFFFFFF is far over the negotiated maximum.
    peer.write_all(&[0xFF; 64]).await.unwrap();
    peer.flush().await.unwrap();

    let error = within(server.recv()).await.unwrap_err();
    assert_eq!(error.close_code(), Some(CloseCode::FrameTooLarge));
    assert!(server.is_finished());

    // The server says why, best effort, before it closes.
    let close = within(read_frame(&mut peer, MAX_FRAME))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(close.frame_type, FrameType::Close);
    let close = Close::from_frame(&close).unwrap();
    assert_eq!(close.code, CloseCode::FrameTooLarge.as_str());

    // A finished connection refuses further use.
    assert!(matches!(
        within(server.recv()).await,
        Err(ConnectionError::Unusable)
    ));
}

#[tokio::test]
async fn chaos_truncated_message() {
    let (mut server, mut peer) = server_with_raw_peer().await;

    // A valid-looking length prefix but a truncated body.
    let fake_len: u32 = 1000;
    peer.write_all(&fake_len.to_be_bytes()).await.unwrap();
    peer.write_all(b"short").await.unwrap();
    drop(peer); // EOF before the full frame

    let result = within(server.recv()).await;
    assert!(result.is_err(), "expected an error, got {result:?}");
    assert!(server.is_finished());
}

#[tokio::test]
async fn chaos_zero_length_message() {
    let (mut server, mut peer) = server_with_raw_peer().await;

    // A length below the 11-byte minimum is refused without reading a body.
    peer.write_all(&0u32.to_be_bytes()).await.unwrap();
    peer.flush().await.unwrap();

    let result = within(server.recv()).await;
    assert!(result.is_err(), "expected an error, got {result:?}");
    assert!(server.is_finished());
}

// ========== STRESS AND PRESSURE TESTS ==========

#[tokio::test]
async fn chaos_rapid_reconnection() {
    for _ in 0..20 {
        let (mut client, mut server) = pair(4096).await;

        let (reply, request) = within(async {
            let request = ping_request(42);
            tokio::join!(native::call(&mut client, &request), serve_one(&mut server))
        })
        .await;
        reply.unwrap();
        assert!(matches!(request, Message::Request(r) if r.id == "42"));

        // Abrupt drop and reconnect.
        drop(client);
        drop(server);
    }
}

#[tokio::test]
async fn chaos_many_small_messages() {
    let (mut client, mut server) = pair(1024 * 1024).await;

    // One request at a time (the daemon's max_inflight is 1): 1000 tiny
    // exchanges, each checked.
    for i in 0..1000u64 {
        let (reply, request) = within(async {
            let request = ping_request(i);
            tokio::join!(native::call(&mut client, &request), serve_one(&mut server))
        })
        .await;
        reply.unwrap();
        assert!(
            matches!(request, Message::Request(r) if r.id == i.to_string()),
            "message {i} mismatch"
        );
    }
    assert_eq!(client.in_flight_len(), 0);
    assert_eq!(server.in_flight_len(), 0);
}

#[tokio::test]
async fn chaos_large_payload() {
    let (mut client, mut server) = pair(2 * 1024 * 1024).await;

    // 100 KB: well inside the limit.
    let msg = navigate("large-1", "x".repeat(100_000));
    let (reply, request) =
        within(async { tokio::join!(native::call(&mut client, &msg), serve_one(&mut server)) })
            .await;
    reply.unwrap();
    match request {
        Message::Request(r) => {
            assert_eq!(r.id, "large-1");
            match r.command {
                BrowserCommand::Navigate { url } => assert_eq!(url.len(), 100_000),
                _ => panic!("wrong command type"),
            }
        }
        _ => panic!("wrong message type"),
    }

    // Near the limit: the body (url plus a few dozen bytes of CBOR) and the
    // 11-byte header still fit the negotiated 16 MiB.
    let near = MAX_FRAME as usize - 11 - 256;
    let msg = navigate("near-limit", "y".repeat(near));
    let (reply, request) =
        within(async { tokio::join!(native::call(&mut client, &msg), serve_one(&mut server)) })
            .await;
    reply.unwrap();
    match request {
        Message::Request(r) => match r.command {
            BrowserCommand::Navigate { url } => assert_eq!(url.len(), near),
            _ => panic!("wrong command type"),
        },
        _ => panic!("wrong message type"),
    }

    // Over the limit: refused on the way out, nothing written, and the
    // connection still works.
    let msg = navigate("too-big", "z".repeat(MAX_FRAME as usize));
    let error = within(native::send_request(&mut client, &msg))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            native::NativeError::Connection(ConnectionError::Frame(_))
        ),
        "{error:?}"
    );
    let (reply, _) = within(async {
        let request = ping_request(5);
        tokio::join!(native::call(&mut client, &request), serve_one(&mut server))
    })
    .await;
    reply.unwrap();
}

// ========== CONCURRENT CHAOS ==========

/// The sequential model still carries traffic both ways: the client sends
/// requests and pings, the server sends its own pings between replies. Each
/// side answers the other's `PING` inside `recv`.
#[tokio::test]
async fn chaos_bidirectional_flood() {
    let (mut client, mut server) = pair(1024 * 1024).await;

    let server_task = tokio::spawn(async move {
        let mut served = 0u64;
        loop {
            match within(server.recv()).await.unwrap() {
                None => break,
                Some(frame) => match frame.frame_type {
                    FrameType::Request => {
                        let ping = Frame {
                            frame_type: FrameType::Ping,
                            request_id: frame.request_id,
                            body: native::ping_body(served).unwrap(),
                        };
                        within(server.send(ping)).await.unwrap();
                        within(native::send_reply(
                            &mut server,
                            frame.request_id,
                            &native::error_reply("ok"),
                        ))
                        .await
                        .unwrap();
                        served += 1;
                    }
                    // The client's answers to the server's pings.
                    FrameType::Pong => {}
                    other => panic!("unexpected {other:?}"),
                },
            }
        }
        served
    });

    for i in 0..100u64 {
        let ping = Frame {
            frame_type: FrameType::Ping,
            request_id: 1_000_000 + i,
            body: native::ping_body(i).unwrap(),
        };
        within(client.send(ping)).await.unwrap();
        let reply = within(native::call(&mut client, &ping_request(i)))
            .await
            .unwrap();
        assert!(matches!(reply, Message::Response(_)));
    }

    drop(client);
    let served = within(server_task).await.unwrap();
    assert_eq!(served, 100);
}

// ========== PROPTEST-DRIVEN CHAOS ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn chaos_arbitrary_message_survives_roundtrip(
        timestamp in 0u64..u64::MAX,
        id in "[a-z0-9]{1,20}",
        query in ".{0,1000}",
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let (mut client, mut server) = pair(64 * 1024).await;

            // The server answers every request with an error reply and hands
            // the decoded requests back when the client hangs up.
            let server_task = tokio::spawn(async move {
                let mut seen = Vec::new();
                loop {
                    match within(server.recv()).await.unwrap() {
                        None => break,
                        Some(frame) if frame.frame_type == FrameType::Request => {
                            seen.push(native::decode_request(&frame.body).unwrap());
                            within(native::send_reply(
                                &mut server,
                                frame.request_id,
                                &native::error_reply("echo"),
                            ))
                            .await
                            .unwrap();
                        }
                        Some(_) => {}
                    }
                }
                seen
            });

            // A ping token survives the round trip.
            let ping = Frame {
                frame_type: FrameType::Ping,
                request_id: 7,
                body: native::ping_body(timestamp).unwrap(),
            };
            within(client.send(ping)).await.unwrap();
            let pong = loop {
                match within(client.recv()).await.unwrap() {
                    Some(frame) if frame.frame_type == FrameType::Pong => break frame,
                    Some(_) => {}
                    None => panic!("connection ended before the PONG"),
                }
            };
            prop_assert_eq!(native::ping_token(&pong.body).unwrap(), timestamp);

            // A request with an arbitrary string arrives intact.
            let msg = navigate(&id, query.clone());
            let reply = within(native::call(&mut client, &msg)).await.unwrap();
            prop_assert!(matches!(reply, Message::Response(_)), "expected a Response");

            drop(client);
            let seen = within(server_task).await.unwrap();
            prop_assert_eq!(seen.len(), 1);
            match &seen[0] {
                Message::Request(r) => {
                    prop_assert_eq!(&r.id, &id);
                    match &r.command {
                        BrowserCommand::Navigate { url } => prop_assert_eq!(url, &query),
                        _ => prop_assert!(false, "wrong command"),
                    }
                }
                _ => prop_assert!(false, "wrong type"),
            }

            Ok(())
        })?;
    }

    #[test]
    fn chaos_random_buffer_size_works(
        buf_size in 256usize..65536,
        msg_count in 1usize..50,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let (mut client, mut server) = pair(buf_size).await;

            for i in 0..msg_count as u64 {
                let (reply, request) = within(async {
                    let request = ping_request(i);
                    tokio::join!(
                        native::call(&mut client, &request),
                        serve_one(&mut server)
                    )
                })
                .await;
                prop_assert!(reply.is_ok(), "exchange {} failed", i);
                prop_assert!(matches!(request, Message::Request(_)), "expected Request");
            }

            Ok(())
        })?;
    }
}
