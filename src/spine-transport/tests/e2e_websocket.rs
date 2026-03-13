//! WebSocket end-to-end integration tests
//!
//! These tests spin up a real WebSocket server, accept connections,
//! and verify frame round-trips through WebSocketBridge / WebSocketServerBridge.

use bytes::Bytes;
use spine_transport::websocket::{WebSocketBridge, WebSocketServerBridge};
use spine_transport::{Frame, FrameFlags, TransportBackend};
use std::time::Duration;
use tokio::net::TcpListener;

/// Spawn a WS echo server that accepts one connection and echoes frames back.
async fn spawn_ws_echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut server = WebSocketServerBridge::accept(stream).await.unwrap();
        while let Ok(frame) = server.recv_frame().await {
            if server.send_frame(frame).await.is_err() {
                break;
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    port
}

/// Spawn a WS server that accepts multiple connections (one per tokio task).
async fn spawn_ws_multi_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let mut server = WebSocketServerBridge::accept(stream).await.unwrap();
                while let Ok(frame) = server.recv_frame().await {
                    if server.send_frame(frame).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    port
}

// ============================================================
// Tests
// ============================================================

#[tokio::test]
async fn ws_single_frame_roundtrip() {
    let port = spawn_ws_echo_server().await;
    let mut client = WebSocketBridge::connect(&format!("ws://127.0.0.1:{}", port))
        .await
        .unwrap();

    let payload = Bytes::from_static(b"hello websocket");
    let frame = Frame::new(payload.clone());
    client.send_frame(frame).await.unwrap();

    let echoed = client.recv_frame().await.unwrap();
    assert_eq!(echoed.payload, payload);
    assert_eq!(echoed.header.length, payload.len() as u32);

    client.close().await.unwrap();
}

#[tokio::test]
async fn ws_multiple_frames_sequential() {
    let port = spawn_ws_echo_server().await;
    let mut client = WebSocketBridge::connect(&format!("ws://127.0.0.1:{}", port))
        .await
        .unwrap();

    for i in 0u32..10 {
        let data = format!("frame-{}", i);
        let frame = Frame::new(Bytes::from(data.clone()));
        client.send_frame(frame).await.unwrap();

        let echoed = client.recv_frame().await.unwrap();
        assert_eq!(echoed.payload, Bytes::from(data));
    }

    client.close().await.unwrap();
}

#[tokio::test]
async fn ws_empty_payload() {
    let port = spawn_ws_echo_server().await;
    let mut client = WebSocketBridge::connect(&format!("ws://127.0.0.1:{}", port))
        .await
        .unwrap();

    let frame = Frame::new(Bytes::new());
    client.send_frame(frame).await.unwrap();

    let echoed = client.recv_frame().await.unwrap();
    assert!(echoed.payload.is_empty());
    assert_eq!(echoed.header.length, 0);

    client.close().await.unwrap();
}

#[tokio::test]
async fn ws_large_payload() {
    let port = spawn_ws_echo_server().await;
    let mut client = WebSocketBridge::connect(&format!("ws://127.0.0.1:{}", port))
        .await
        .unwrap();

    let data = vec![0xABu8; 64 * 1024]; // 64 KiB
    let frame = Frame::new(Bytes::from(data.clone()));
    client.send_frame(frame).await.unwrap();

    let echoed = client.recv_frame().await.unwrap();
    assert_eq!(echoed.payload.len(), 64 * 1024);
    assert_eq!(&echoed.payload[..], &data[..]);

    client.close().await.unwrap();
}

#[tokio::test]
async fn ws_frame_flags_preserved() {
    let port = spawn_ws_echo_server().await;
    let mut client = WebSocketBridge::connect(&format!("ws://127.0.0.1:{}", port))
        .await
        .unwrap();

    let mut frame = Frame::new(Bytes::from_static(b"flagged"));
    frame.header.flags = FrameFlags::PRIORITY | FrameFlags::ACK_REQUIRED;
    frame.header.sequence = 42;
    frame.header.stream_id = 7;

    client.send_frame(frame).await.unwrap();
    let echoed = client.recv_frame().await.unwrap();

    assert!(echoed.header.flags.contains(FrameFlags::PRIORITY));
    assert!(echoed.header.flags.contains(FrameFlags::ACK_REQUIRED));
    assert_eq!(echoed.header.sequence, 42);
    assert_eq!(echoed.header.stream_id, 7);

    client.close().await.unwrap();
}

#[tokio::test]
async fn ws_concurrent_clients() {
    let port = spawn_ws_multi_server().await;

    let mut handles = Vec::new();
    for client_id in 0u32..5 {
        let handle = tokio::spawn(async move {
            let mut client =
                WebSocketBridge::connect(&format!("ws://127.0.0.1:{}", port))
                    .await
                    .unwrap();

            for i in 0u32..5 {
                let msg = format!("client-{}-msg-{}", client_id, i);
                client
                    .send_frame(Frame::new(Bytes::from(msg.clone())))
                    .await
                    .unwrap();
                let echoed = client.recv_frame().await.unwrap();
                assert_eq!(echoed.payload, Bytes::from(msg));
            }
            client.close().await.unwrap();
        });
        handles.push(handle);
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn ws_rapid_reconnect() {
    let port = spawn_ws_multi_server().await;

    for cycle in 0..5 {
        let mut client = WebSocketBridge::connect(&format!("ws://127.0.0.1:{}", port))
            .await
            .unwrap();

        let msg = format!("reconnect-{}", cycle);
        client
            .send_frame(Frame::new(Bytes::from(msg.clone())))
            .await
            .unwrap();
        let echoed = client.recv_frame().await.unwrap();
        assert_eq!(echoed.payload, Bytes::from(msg));

        client.close().await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn ws_health_tracking() {
    let port = spawn_ws_echo_server().await;
    let mut client = WebSocketBridge::connect(&format!("ws://127.0.0.1:{}", port))
        .await
        .unwrap();

    assert!(client.is_healthy());
    assert!(client.age() < Duration::from_secs(5));

    client
        .send_frame(Frame::new(Bytes::from_static(b"health")))
        .await
        .unwrap();
    let _ = client.recv_frame().await.unwrap();

    assert!(client.bytes_sent() > 0);
    assert!(client.bytes_received() > 0);
    assert!(client.idle_time() < Duration::from_secs(1));

    client.close().await.unwrap();
}
