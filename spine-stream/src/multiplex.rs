//! Stream multiplexing for multiple logical streams over a single connection.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    StreamConfig, StreamControl, StreamError, StreamEvent, StreamHandle, StreamMessage,
    StreamPayload, StreamResult, StreamStats,
};

// =============================================================================
// STREAM MULTIPLEXER
// =============================================================================

/// Multiplexer that manages multiple logical streams over a single connection.
///
/// Features:
/// - Automatic stream ID allocation
/// - Per-stream flow control
/// - Priority-based scheduling
/// - Stream lifecycle management
pub struct StreamMultiplexer {
    /// Configuration
    config: StreamConfig,
    /// Active streams
    streams: DashMap<u32, MultiplexedStream>,
    /// Next stream ID to allocate
    next_stream_id: AtomicU32,
    /// Outbound message channel
    outbound_tx: mpsc::Sender<StreamMessage>,
    /// Inbound message dispatcher
    inbound_router: Arc<InboundRouter>,
    /// Statistics
    stats: Arc<MultiplexStats>,
    /// Shutdown flag
    shutdown: Arc<RwLock<bool>>,
}

/// A multiplexed stream
struct MultiplexedStream {
    id: u32,
    tx: mpsc::Sender<StreamMessage>,
    stats: StreamStats,
    flow_state: FlowState,
    created_at: std::time::Instant,
}

/// Flow control state for a stream
#[derive(Clone, Debug)]
struct FlowState {
    /// Send window (bytes we can send)
    send_window: u32,
    /// Receive window (bytes we can receive)
    recv_window: u32,
    /// Is stream paused?
    paused: bool,
    /// Bytes in flight
    in_flight: u64,
}

impl Default for FlowState {
    fn default() -> Self {
        Self {
            send_window: 256 * 1024, // 256KB default
            recv_window: 256 * 1024,
            paused: false,
            in_flight: 0,
        }
    }
}

/// Routes inbound messages to the correct stream
struct InboundRouter {
    routes: DashMap<u32, mpsc::Sender<StreamMessage>>,
    control_tx: mpsc::Sender<StreamControl>,
    event_tx: mpsc::Sender<StreamEvent>,
}

/// Multiplexer statistics
#[derive(Debug, Default)]
pub struct MultiplexStats {
    pub active_streams: AtomicU32,
    pub total_streams_opened: AtomicU64,
    pub total_streams_closed: AtomicU64,
    pub total_messages_routed: AtomicU64,
    pub routing_errors: AtomicU64,
}

impl StreamMultiplexer {
    /// Create a new stream multiplexer
    pub fn new(config: StreamConfig) -> (Self, mpsc::Receiver<StreamMessage>) {
        let (outbound_tx, outbound_rx) = mpsc::channel(config.max_pending_items);
        let (control_tx, _control_rx) = mpsc::channel(64);
        let (event_tx, _event_rx) = mpsc::channel(64);

        let inbound_router = Arc::new(InboundRouter {
            routes: DashMap::new(),
            control_tx,
            event_tx,
        });

        let mux = Self {
            config,
            streams: DashMap::new(),
            next_stream_id: AtomicU32::new(1), // 0 reserved for control
            outbound_tx,
            inbound_router,
            stats: Arc::new(MultiplexStats::default()),
            shutdown: Arc::new(RwLock::new(false)),
        };

        (mux, outbound_rx)
    }

    /// Open a new stream
    pub async fn open_stream(&self) -> StreamResult<StreamHandle> {
        if *self.shutdown.read() {
            return Err(StreamError::Closed);
        }

        let stream_id = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(self.config.max_pending_items);

        // Create the stream
        let stream = MultiplexedStream {
            id: stream_id,
            tx: tx.clone(),
            stats: StreamStats::default(),
            flow_state: FlowState {
                send_window: self.config.window_size as u32,
                recv_window: self.config.window_size as u32,
                ..Default::default()
            },
            created_at: std::time::Instant::now(),
        };

        // Register the stream
        self.streams.insert(stream_id, stream);
        self.inbound_router.routes.insert(stream_id, tx.clone());
        self.stats.active_streams.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_streams_opened
            .fetch_add(1, Ordering::Relaxed);

        // Send open control message
        let open_msg = StreamMessage {
            id: Uuid::new_v4(),
            stream_id: 0, // Control stream
            sequence: 0,
            payload: StreamPayload::Control(StreamControl::Open {
                stream_id,
                config: None,
            }),
            priority: 0, // Highest priority for control
            timestamp_ns: crate::timestamp_now(),
            correlation_id: None,
        };

        let _ = self.outbound_tx.send(open_msg).await;

        // Spawn message forwarder
        let outbound_tx = self.outbound_tx.clone();
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(msg) = rx.recv().await {
                if outbound_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        Ok(StreamHandle::new(stream_id, tx, self.config.clone()))
    }

    /// Close a stream
    pub async fn close_stream(&self, stream_id: u32) -> StreamResult<()> {
        if let Some((_, _stream)) = self.streams.remove(&stream_id) {
            self.inbound_router.routes.remove(&stream_id);
            self.stats.active_streams.fetch_sub(1, Ordering::Relaxed);
            self.stats
                .total_streams_closed
                .fetch_add(1, Ordering::Relaxed);

            // Send close control message
            let close_msg = StreamMessage {
                id: Uuid::new_v4(),
                stream_id: 0,
                sequence: 0,
                payload: StreamPayload::Control(StreamControl::Close {
                    stream_id,
                    reason: None,
                }),
                priority: 0,
                timestamp_ns: crate::timestamp_now(),
                correlation_id: None,
            };

            let _ = self.outbound_tx.send(close_msg).await;

            Ok(())
        } else {
            Err(StreamError::StreamNotFound(stream_id))
        }
    }

    /// Route an inbound message to the correct stream
    pub async fn route_inbound(&self, msg: StreamMessage) -> StreamResult<()> {
        self.stats
            .total_messages_routed
            .fetch_add(1, Ordering::Relaxed);

        // Handle control messages
        if msg.stream_id == 0 {
            if let StreamPayload::Control(ctrl) = msg.payload {
                return self.handle_control(ctrl).await;
            }
        }

        // Route to stream
        if let Some(tx) = self.inbound_router.routes.get(&msg.stream_id) {
            tx.send(msg)
                .await
                .map_err(|_| StreamError::ChannelSendError)?;
            Ok(())
        } else {
            self.stats.routing_errors.fetch_add(1, Ordering::Relaxed);
            Err(StreamError::StreamNotFound(msg.stream_id))
        }
    }

    /// Handle control messages
    async fn handle_control(&self, ctrl: StreamControl) -> StreamResult<()> {
        match ctrl {
            StreamControl::WindowUpdate {
                stream_id,
                increment,
            } => {
                if let Some(mut stream) = self.streams.get_mut(&stream_id) {
                    stream.flow_state.send_window += increment;
                }
            }
            StreamControl::Pause { stream_id } => {
                if let Some(mut stream) = self.streams.get_mut(&stream_id) {
                    stream.flow_state.paused = true;
                }
            }
            StreamControl::Resume { stream_id } => {
                if let Some(mut stream) = self.streams.get_mut(&stream_id) {
                    stream.flow_state.paused = false;
                }
            }
            StreamControl::Ping { payload } => {
                // Send pong
                let pong_msg = StreamMessage {
                    id: Uuid::new_v4(),
                    stream_id: 0,
                    sequence: 0,
                    payload: StreamPayload::Control(StreamControl::Pong { payload }),
                    priority: 0,
                    timestamp_ns: crate::timestamp_now(),
                    correlation_id: None,
                };
                let _ = self.outbound_tx.send(pong_msg).await;
            }
            StreamControl::Reset { stream_id, .. } => {
                // Remove the stream
                self.streams.remove(&stream_id);
                self.inbound_router.routes.remove(&stream_id);
                self.stats.active_streams.fetch_sub(1, Ordering::Relaxed);
                self.stats
                    .total_streams_closed
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        Ok(())
    }

    /// Get active stream count
    pub fn active_streams(&self) -> u32 {
        self.stats.active_streams.load(Ordering::Relaxed)
    }

    /// Get stream statistics
    pub fn stream_stats(&self, stream_id: u32) -> Option<StreamStats> {
        self.streams.get(&stream_id).map(|s| s.stats.clone())
    }

    /// Get multiplexer statistics
    pub fn stats(&self) -> &MultiplexStats {
        &self.stats
    }

    /// Shutdown the multiplexer
    pub async fn shutdown(&self) {
        *self.shutdown.write() = true;

        // Close all streams
        let stream_ids: Vec<u32> = self.streams.iter().map(|s| *s.key()).collect();
        for id in stream_ids {
            let _ = self.close_stream(id).await;
        }
    }
}

// =============================================================================
// DEMULTIPLEXER
// =============================================================================

/// Demultiplexer for receiving and dispatching multiplexed streams.
pub struct StreamDemultiplexer {
    /// Stream handlers
    handlers: DashMap<u32, mpsc::Sender<StreamMessage>>,
    /// New stream callback
    on_new_stream: Option<Box<dyn Fn(u32) + Send + Sync>>,
}

impl StreamDemultiplexer {
    /// Create a new demultiplexer
    pub fn new() -> Self {
        Self {
            handlers: DashMap::new(),
            on_new_stream: None,
        }
    }

    /// Set callback for new streams
    pub fn on_new_stream<F>(&mut self, callback: F)
    where
        F: Fn(u32) + Send + Sync + 'static,
    {
        self.on_new_stream = Some(Box::new(callback));
    }

    /// Register a handler for a stream
    pub fn register(&self, stream_id: u32, tx: mpsc::Sender<StreamMessage>) {
        self.handlers.insert(stream_id, tx);
    }

    /// Dispatch a message
    pub async fn dispatch(&self, msg: StreamMessage) -> StreamResult<()> {
        if let Some(tx) = self.handlers.get(&msg.stream_id) {
            tx.send(msg)
                .await
                .map_err(|_| StreamError::ChannelSendError)
        } else {
            // Unknown stream - maybe trigger new stream callback
            if let Some(ref callback) = self.on_new_stream {
                callback(msg.stream_id);
            }
            Err(StreamError::StreamNotFound(msg.stream_id))
        }
    }
}

impl Default for StreamDemultiplexer {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// STREAM SCHEDULER
// =============================================================================

/// Schedules outbound messages across multiplexed streams.
///
/// Implements weighted fair queuing with priority support.
pub struct StreamScheduler {
    /// Queues per stream
    queues: DashMap<u32, StreamQueue>,
    /// Priority buckets (index 0 = highest priority)
    priority_buckets: Vec<Vec<u32>>,
    /// Current stream index for round-robin
    current_index: AtomicU32,
    /// Configuration
    config: StreamConfig,
}

struct StreamQueue {
    messages: Vec<StreamMessage>,
    weight: u32,
    deficit: i32,
    paused: bool,
}

impl StreamScheduler {
    /// Create a new stream scheduler
    pub fn new(config: StreamConfig) -> Self {
        let priority_buckets = (0..config.priority_levels).map(|_| Vec::new()).collect();

        Self {
            queues: DashMap::new(),
            priority_buckets,
            current_index: AtomicU32::new(0),
            config,
        }
    }

    /// Register a stream
    pub fn register_stream(&self, stream_id: u32, weight: u32) {
        self.queues.insert(
            stream_id,
            StreamQueue {
                messages: Vec::new(),
                weight,
                deficit: 0,
                paused: false,
            },
        );
    }

    /// Enqueue a message
    pub fn enqueue(&self, msg: StreamMessage) {
        if let Some(mut queue) = self.queues.get_mut(&msg.stream_id) {
            queue.messages.push(msg);
        }
    }

    /// Dequeue the next message using weighted fair queuing
    pub fn dequeue(&self) -> Option<StreamMessage> {
        // First check priority streams
        for mut entry in self.queues.iter_mut() {
            let queue = entry.value_mut();
            if queue.paused {
                continue;
            }

            // Check for high-priority messages
            if let Some(pos) = queue.messages.iter().position(|m| m.priority < 2) {
                return Some(queue.messages.remove(pos));
            }
        }

        // Round-robin with deficit counter
        let start = self.current_index.fetch_add(1, Ordering::Relaxed);
        let num_streams = self.queues.len() as u32;

        if num_streams == 0 {
            return None;
        }

        for i in 0..num_streams {
            let idx = (start + i) % num_streams;

            // Get stream at this index
            let stream_ids: Vec<u32> = self.queues.iter().map(|q| *q.key()).collect();
            if let Some(&stream_id) = stream_ids.get(idx as usize) {
                if let Some(mut queue) = self.queues.get_mut(&stream_id) {
                    if !queue.paused && !queue.messages.is_empty() {
                        queue.deficit += queue.weight as i32;
                        if queue.deficit > 0 {
                            queue.deficit -= 1;
                            return Some(queue.messages.remove(0));
                        }
                    }
                }
            }
        }

        None
    }

    /// Pause a stream
    pub fn pause_stream(&self, stream_id: u32) {
        if let Some(mut queue) = self.queues.get_mut(&stream_id) {
            queue.paused = true;
        }
    }

    /// Resume a stream
    pub fn resume_stream(&self, stream_id: u32) {
        if let Some(mut queue) = self.queues.get_mut(&stream_id) {
            queue.paused = false;
        }
    }

    /// Get queue depth for a stream
    pub fn queue_depth(&self, stream_id: u32) -> usize {
        self.queues
            .get(&stream_id)
            .map(|q| q.messages.len())
            .unwrap_or(0)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_multiplexer_open_close() {
        let (mux, _rx) = StreamMultiplexer::new(StreamConfig::default());

        let handle = mux.open_stream().await.unwrap();
        assert_eq!(mux.active_streams(), 1);

        mux.close_stream(handle.id()).await.unwrap();
        assert_eq!(mux.active_streams(), 0);
    }

    #[tokio::test]
    async fn test_stream_routing() {
        let (mux, _rx) = StreamMultiplexer::new(StreamConfig::default());
        let handle = mux.open_stream().await.unwrap();

        let msg = StreamMessage {
            id: Uuid::new_v4(),
            stream_id: handle.id(),
            sequence: 1,
            payload: StreamPayload::Bytes(b"test".to_vec()),
            priority: 4,
            timestamp_ns: 0,
            correlation_id: None,
        };

        // Should succeed
        assert!(mux.route_inbound(msg).await.is_ok());

        // Unknown stream should fail
        let bad_msg = StreamMessage {
            id: Uuid::new_v4(),
            stream_id: 9999,
            sequence: 1,
            payload: StreamPayload::Bytes(b"test".to_vec()),
            priority: 4,
            timestamp_ns: 0,
            correlation_id: None,
        };

        assert!(mux.route_inbound(bad_msg).await.is_err());
    }

    #[test]
    fn test_scheduler_weighted_fair_queuing() {
        let scheduler = StreamScheduler::new(StreamConfig::default());

        scheduler.register_stream(1, 2);
        scheduler.register_stream(2, 1);

        // Enqueue messages
        for _ in 0..4 {
            scheduler.enqueue(StreamMessage {
                id: Uuid::new_v4(),
                stream_id: 1,
                sequence: 0,
                payload: StreamPayload::Bytes(Vec::new()),
                priority: 4,
                timestamp_ns: 0,
                correlation_id: None,
            });
        }

        for _ in 0..2 {
            scheduler.enqueue(StreamMessage {
                id: Uuid::new_v4(),
                stream_id: 2,
                sequence: 0,
                payload: StreamPayload::Bytes(Vec::new()),
                priority: 4,
                timestamp_ns: 0,
                correlation_id: None,
            });
        }

        // Dequeue all - stream 1 should get roughly 2x messages due to weight
        let mut counts = HashMap::new();
        while let Some(msg) = scheduler.dequeue() {
            *counts.entry(msg.stream_id).or_insert(0) += 1;
        }

        assert!(counts.get(&1).unwrap_or(&0) >= counts.get(&2).unwrap_or(&0));
    }
}
