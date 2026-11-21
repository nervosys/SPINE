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
use serde_json::Value;
use spine_protocol::body::{WireError, ERR_RATE_LIMIT};
use spine_protocol::connection::{Connection, ConnectionError};
use spine_protocol::frame::{
    read_frame, write_frame, CloseCode, Frame, FrameType, MAX_FRAME, PRE_HELLO_MAX_FRAME,
};
use spine_protocol::hello::Close;
use spine_protocol::native;
use spine_protocol::BrowserCommand;
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

/// A command that differs per `n`, so a test can tell exchanges apart.
fn ping_request(n: u64) -> BrowserCommand {
    BrowserCommand::Navigate {
        url: format!("https://example.test/{n}"),
    }
}

fn navigate(url: String) -> BrowserCommand {
    BrowserCommand::Navigate { url }
}

/// The code `serve_one` answers with.
const SERVED: &str = ERR_RATE_LIMIT;

/// What a client sees for an exchange `serve_one` answered.
fn assert_served(reply: Result<Value, native::NativeError>) {
    match reply {
        Err(native::NativeError::Remote { code, .. }) => assert_eq!(code, SERVED),
        other => panic!("expected the served error, got {other:?}"),
    }
}

/// Server side of one exchange: reads a request and answers it with an `err`.
/// Returns the frame request_id and the decoded command.
async fn serve_one(server: &mut Conn) -> (u64, BrowserCommand) {
    let frame = within(server.recv()).await.unwrap().unwrap();
    assert_eq!(frame.frame_type, FrameType::Request);
    let command = native::decode_command(&frame.body).unwrap();
    within(native::send_err(
        server,
        frame.request_id,
        &WireError::new(SERVED, "served"),
    ))
    .await
    .unwrap();
    (frame.request_id, command)
}

// ========== CONNECTION DROP TESTS ==========

#[tokio::test]
async fn chaos_connection_drop_mid_session() {
    let (mut client, mut server) = pair(64 * 1024).await;

    // A few exchanges succeed.
    for i in 0..3u64 {
        let (reply, _) = within(async {
            let request = ping_request(i);
            tokio::join!(
                native::call::<_, Value>(&mut client, &request),
                serve_one(&mut server)
            )
        })
        .await;
        assert_served(reply);
    }

    // Drop the server side.
    drop(server);

    // The next send may succeed (buffered) or fail; it must not panic, and
    // the client must see the end of the stream and not hang.
    let _ = within(native::send_command(&mut client, &ping_request(99))).await;
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
    let sent = within(native::send_command(&mut client, &ping_request(1)))
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

        let (reply, (_, command)) = within(async {
            let request = ping_request(42);
            tokio::join!(
                native::call::<_, Value>(&mut client, &request),
                serve_one(&mut server)
            )
        })
        .await;
        assert_served(reply);
        assert_eq!(command, ping_request(42));

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
        let (reply, (_, command)) = within(async {
            let request = ping_request(i);
            tokio::join!(
                native::call::<_, Value>(&mut client, &request),
                serve_one(&mut server)
            )
        })
        .await;
        assert_served(reply);
        assert_eq!(command, ping_request(i), "message {i} mismatch");
    }
    assert_eq!(client.in_flight_len(), 0);
    assert_eq!(server.in_flight_len(), 0);
}

#[tokio::test]
async fn chaos_large_payload() {
    let (mut client, mut server) = pair(2 * 1024 * 1024).await;

    // 100 KB: well inside the limit.
    let msg = navigate("x".repeat(100_000));
    let (reply, (_, command)) = within(async {
        tokio::join!(
            native::call::<_, Value>(&mut client, &msg),
            serve_one(&mut server)
        )
    })
    .await;
    assert_served(reply);
    assert_eq!(command, msg);

    // Near the limit: the body (url plus a few dozen bytes of CBOR) and the
    // 11-byte header still fit the negotiated 16 MiB.
    let near = MAX_FRAME as usize - 11 - 256;
    let msg = navigate("y".repeat(near));
    let (reply, (_, command)) = within(async {
        tokio::join!(
            native::call::<_, Value>(&mut client, &msg),
            serve_one(&mut server)
        )
    })
    .await;
    assert_served(reply);
    assert_eq!(command, msg);

    // Over the limit: refused on the way out, nothing written, and the
    // connection still works.
    let msg = navigate("z".repeat(MAX_FRAME as usize));
    let error = within(native::send_command(&mut client, &msg))
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
        tokio::join!(
            native::call::<_, Value>(&mut client, &request),
            serve_one(&mut server)
        )
    })
    .await;
    assert_served(reply);
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
                        within(native::send_ok(
                            &mut server,
                            frame.request_id,
                            &serde_json::json!({"served": served}),
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
        let request = ping_request(i);
        let reply: Value = within(native::call(&mut client, &request)).await.unwrap();
        assert_eq!(reply["served"], i);
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
                            let command = native::decode_command(&frame.body).unwrap();
                            within(native::send_ok(
                                &mut server,
                                frame.request_id,
                                &serde_json::json!("echo"),
                            ))
                            .await
                            .unwrap();
                            seen.push((frame.request_id, command));
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

            // A request with an arbitrary string arrives intact, under the
            // frame id the client was given.
            let msg = navigate(query.clone());
            let id = within(native::send_command(&mut client, &msg)).await.unwrap();
            let reply: Value = within(native::await_reply(&mut client, id)).await.unwrap();
            prop_assert_eq!(reply, serde_json::json!("echo"));

            drop(client);
            let seen = within(server_task).await.unwrap();
            prop_assert_eq!(seen.len(), 1);
            prop_assert_eq!(seen[0].0, id);
            prop_assert_eq!(&seen[0].1, &msg);

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
                let (reply, (_, command)) = within(async {
                    let request = ping_request(i);
                    tokio::join!(
                        native::call::<_, Value>(&mut client, &request),
                        serve_one(&mut server)
                    )
                })
                .await;
                prop_assert!(
                    matches!(&reply, Err(native::NativeError::Remote { code, .. }) if code == SERVED),
                    "exchange {} failed: {:?}", i, reply
                );
                prop_assert_eq!(command, ping_request(i));
            }

            Ok(())
        })?;
    }
}
