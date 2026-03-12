//! High-performance streaming channels with backpressure.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot, Semaphore};
use uuid::Uuid;

use crate::{StreamError, StreamMessage, StreamPayload, StreamResult};

// =============================================================================
// BOUNDED CHANNEL
// =============================================================================

/// A bounded channel with backpressure support.
///
/// Uses a semaphore to implement flow control, blocking senders
/// when the channel is full.
pub struct BoundedChannel<T> {
    inner: Arc<BoundedChannelInner<T>>,
}

struct BoundedChannelInner<T> {
    tx: mpsc::Sender<T>,
    semaphore: Arc<Semaphore>,
    capacity: usize,
    closed: AtomicBool,
    stats: ChannelStats,
}

/// Channel statistics
#[derive(Debug, Default)]
pub struct ChannelStats {
    pub messages_sent: AtomicU64,
    pub messages_received: AtomicU64,
    pub backpressure_events: AtomicU64,
    pub send_errors: AtomicU64,
}

/// Sender half of a bounded channel
pub struct BoundedSender<T> {
    inner: Arc<BoundedChannelInner<T>>,
}

impl<T> Clone for BoundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Receiver half of a bounded channel
pub struct BoundedReceiver<T> {
    rx: mpsc::Receiver<T>,
    inner: Arc<BoundedChannelInner<T>>,
}

impl<T: Send + 'static> BoundedChannel<T> {
    /// Create a new bounded channel with the given capacity
    #[allow(clippy::new_ret_no_self)]
    pub fn new(capacity: usize) -> (BoundedSender<T>, BoundedReceiver<T>) {
        let (tx, rx) = mpsc::channel(capacity);
        let semaphore = Arc::new(Semaphore::new(capacity));

        let inner = Arc::new(BoundedChannelInner {
            tx,
            semaphore,
            capacity,
            closed: AtomicBool::new(false),
            stats: ChannelStats::default(),
        });

        (
            BoundedSender {
                inner: Arc::clone(&inner),
            },
            BoundedReceiver { rx, inner },
        )
    }
}

impl<T: Send + 'static> BoundedSender<T> {
    /// Send a message, waiting for capacity if needed
    pub async fn send(&self, item: T) -> StreamResult<()> {
        if self.inner.closed.load(Ordering::Relaxed) {
            return Err(StreamError::Closed);
        }

        // Acquire semaphore permit (backpressure)
        let permit = self
            .inner
            .semaphore
            .acquire()
            .await
            .map_err(|_| StreamError::Closed)?;

        match self.inner.tx.send(item).await {
            Ok(()) => {
                self.inner
                    .stats
                    .messages_sent
                    .fetch_add(1, Ordering::Relaxed);
                // Don't forget the permit - receiver will release it
                std::mem::forget(permit);
                Ok(())
            }
            Err(_) => {
                self.inner.stats.send_errors.fetch_add(1, Ordering::Relaxed);
                Err(StreamError::ChannelSendError)
            }
        }
    }

    /// Try to send without waiting
    pub fn try_send(&self, item: T) -> StreamResult<()> {
        if self.inner.closed.load(Ordering::Relaxed) {
            return Err(StreamError::Closed);
        }

        match self.inner.semaphore.try_acquire() {
            Ok(permit) => match self.inner.tx.try_send(item) {
                Ok(()) => {
                    self.inner
                        .stats
                        .messages_sent
                        .fetch_add(1, Ordering::Relaxed);
                    std::mem::forget(permit);
                    Ok(())
                }
                Err(_) => {
                    self.inner.stats.send_errors.fetch_add(1, Ordering::Relaxed);
                    Err(StreamError::ChannelFull)
                }
            },
            Err(_) => {
                self.inner
                    .stats
                    .backpressure_events
                    .fetch_add(1, Ordering::Relaxed);
                Err(StreamError::ChannelFull)
            }
        }
    }

    /// Close the channel
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Relaxed);
    }

    /// Check if closed
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Relaxed)
    }

    /// Get channel statistics
    pub fn stats(&self) -> &ChannelStats {
        &self.inner.stats
    }
}

impl<T> BoundedReceiver<T> {
    /// Receive a message
    pub async fn recv(&mut self) -> Option<T> {
        let item = self.rx.recv().await;
        if item.is_some() {
            self.inner
                .stats
                .messages_received
                .fetch_add(1, Ordering::Relaxed);
            // Release semaphore permit
            self.inner.semaphore.add_permits(1);
        }
        item
    }

    /// Try to receive without waiting
    pub fn try_recv(&mut self) -> Option<T> {
        match self.rx.try_recv() {
            Ok(item) => {
                self.inner
                    .stats
                    .messages_received
                    .fetch_add(1, Ordering::Relaxed);
                self.inner.semaphore.add_permits(1);
                Some(item)
            }
            Err(_) => None,
        }
    }

    /// Close the channel
    pub fn close(&mut self) {
        self.inner.closed.store(true, Ordering::Relaxed);
        self.rx.close();
    }

    /// Get channel statistics
    pub fn stats(&self) -> &ChannelStats {
        &self.inner.stats
    }
}

// =============================================================================
// STREAM MESSAGE CHANNEL
// =============================================================================

/// A channel specifically for StreamMessage with built-in filtering and routing.
pub struct MessageChannel {
    tx: BoundedSender<StreamMessage>,
    stream_id: u32,
    sequence: AtomicU64,
}

impl MessageChannel {
    /// Create a new message channel
    pub fn new(capacity: usize, stream_id: u32) -> (Self, BoundedReceiver<StreamMessage>) {
        let (tx, rx) = BoundedChannel::new(capacity);
        (
            Self {
                tx,
                stream_id,
                sequence: AtomicU64::new(0),
            },
            rx,
        )
    }

    /// Send bytes
    pub async fn send_bytes(&self, data: Bytes) -> StreamResult<()> {
        let msg = self.create_message(StreamPayload::Bytes(data.to_vec()), 4);
        self.tx.send(msg).await
    }

    /// Send with priority
    pub async fn send_with_priority(&self, data: Bytes, priority: u8) -> StreamResult<()> {
        let msg = self.create_message(StreamPayload::Bytes(data.to_vec()), priority);
        self.tx.send(msg).await
    }

    /// Send a latent vector
    pub async fn send_latent(&self, dimensions: u32, vector: Vec<f32>) -> StreamResult<()> {
        let payload = StreamPayload::LatentVector { dimensions, vector };
        let msg = self.create_message(payload, 4);
        self.tx.send(msg).await
    }

    fn create_message(&self, payload: StreamPayload, priority: u8) -> StreamMessage {
        StreamMessage {
            id: Uuid::new_v4(),
            stream_id: self.stream_id,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            payload,
            priority,
            timestamp_ns: crate::timestamp_now(),
            correlation_id: None,
        }
    }

    /// Get the stream ID
    pub fn stream_id(&self) -> u32 {
        self.stream_id
    }

    /// Close the channel
    pub fn close(&self) {
        self.tx.close();
    }
}

// =============================================================================
// BROADCAST CHANNEL
// =============================================================================

/// A broadcast channel that sends to multiple receivers.
pub struct BroadcastChannel<T: Clone> {
    subscribers: Arc<Mutex<Vec<mpsc::Sender<T>>>>,
    capacity: usize,
    stats: Arc<BroadcastStats>,
}

/// Broadcast statistics
#[derive(Debug, Default)]
pub struct BroadcastStats {
    pub messages_broadcast: AtomicU64,
    pub total_deliveries: AtomicU64,
    pub failed_deliveries: AtomicU64,
    pub subscribers_added: AtomicU64,
    pub subscribers_removed: AtomicU64,
}

impl<T: Clone + Send + 'static> BroadcastChannel<T> {
    /// Create a new broadcast channel
    pub fn new(capacity: usize) -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(Vec::new())),
            capacity,
            stats: Arc::new(BroadcastStats::default()),
        }
    }

    /// Subscribe to the broadcast
    pub fn subscribe(&self) -> mpsc::Receiver<T> {
        let (tx, rx) = mpsc::channel(self.capacity);
        self.subscribers.lock().push(tx);
        self.stats.subscribers_added.fetch_add(1, Ordering::Relaxed);
        rx
    }

    /// Broadcast a message to all subscribers
    pub async fn broadcast(&self, item: T) {
        self.stats
            .messages_broadcast
            .fetch_add(1, Ordering::Relaxed);

        let mut subscribers = self.subscribers.lock();
        let mut to_remove = Vec::new();

        for (idx, tx) in subscribers.iter().enumerate() {
            match tx.try_send(item.clone()) {
                Ok(()) => {
                    self.stats.total_deliveries.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // Skip - subscriber is slow
                    self.stats.failed_deliveries.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    // Mark for removal
                    to_remove.push(idx);
                }
            }
        }

        // Remove closed subscribers (in reverse order to maintain indices)
        for idx in to_remove.into_iter().rev() {
            subscribers.remove(idx);
            self.stats
                .subscribers_removed
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get subscriber count
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.lock().len()
    }

    /// Get statistics
    pub fn stats(&self) -> &BroadcastStats {
        &self.stats
    }
}

// =============================================================================
// REQUEST-RESPONSE CHANNEL
// =============================================================================

/// A request-response channel for RPC-style communication.
pub struct RequestChannel<Req, Resp> {
    tx: mpsc::Sender<(Req, oneshot::Sender<Resp>)>,
}

impl<Req, Resp> Clone for RequestChannel<Req, Resp> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

/// Handler for request-response channel
pub struct RequestHandler<Req, Resp> {
    rx: mpsc::Receiver<(Req, oneshot::Sender<Resp>)>,
}

impl<Req: Send + 'static, Resp: Send + 'static> RequestChannel<Req, Resp> {
    /// Create a new request channel
    pub fn new(capacity: usize) -> (Self, RequestHandler<Req, Resp>) {
        let (tx, rx) = mpsc::channel(capacity);
        (Self { tx }, RequestHandler { rx })
    }

    /// Send a request and wait for response
    pub async fn request(&self, req: Req) -> StreamResult<Resp> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.tx
            .send((req, resp_tx))
            .await
            .map_err(|_| StreamError::ChannelSendError)?;

        resp_rx.await.map_err(|_| StreamError::RequestTimeout)
    }

    /// Send a request with timeout
    pub async fn request_timeout(
        &self,
        req: Req,
        timeout: std::time::Duration,
    ) -> StreamResult<Resp> {
        tokio::time::timeout(timeout, self.request(req))
            .await
            .map_err(|_| StreamError::RequestTimeout)?
    }
}

impl<Req, Resp> RequestHandler<Req, Resp> {
    /// Receive the next request
    pub async fn recv(&mut self) -> Option<(Req, oneshot::Sender<Resp>)> {
        self.rx.recv().await
    }
}

// =============================================================================
// SPSC RING BUFFER
// =============================================================================

/// Single-producer, single-consumer ring buffer for zero-copy streaming.
///
/// Uses a lock-free design for maximum performance.
pub struct SpscRingBuffer<T> {
    buffer: Box<[std::mem::MaybeUninit<T>]>,
    capacity: usize,
    head: AtomicU64,
    tail: AtomicU64,
    stats: Arc<RingBufferStats>,
}

/// Ring buffer statistics
#[derive(Debug, Default)]
pub struct RingBufferStats {
    pub items_written: AtomicU64,
    pub items_read: AtomicU64,
    pub write_waits: AtomicU64,
    pub read_waits: AtomicU64,
}

impl<T> SpscRingBuffer<T> {
    /// Create a new ring buffer with the given capacity (rounded up to power of 2)
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.next_power_of_two();
        let buffer = (0..capacity)
            .map(|_| std::mem::MaybeUninit::uninit())
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            buffer,
            capacity,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            stats: Arc::new(RingBufferStats::default()),
        }
    }

    /// Try to push an item
    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        if head - tail >= self.capacity as u64 {
            self.stats.write_waits.fetch_add(1, Ordering::Relaxed);
            return Err(item);
        }

        let idx = (head as usize) & (self.capacity - 1);
        self.buffer[idx].write(item);
        self.head.store(head + 1, Ordering::Release);
        self.stats.items_written.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Try to pop an item
    pub fn try_pop(&mut self) -> Option<T> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        if tail >= head {
            self.stats.read_waits.fetch_add(1, Ordering::Relaxed);
            return None;
        }

        let idx = (tail as usize) & (self.capacity - 1);
        let item = unsafe { self.buffer[idx].assume_init_read() };
        self.tail.store(tail + 1, Ordering::Release);
        self.stats.items_read.fetch_add(1, Ordering::Relaxed);

        Some(item)
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        tail >= head
    }

    /// Check if full
    pub fn is_full(&self) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        head - tail >= self.capacity as u64
    }

    /// Get current length
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        (head - tail) as usize
    }

    /// Get capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get statistics
    pub fn stats(&self) -> &RingBufferStats {
        &self.stats
    }
}

impl<T> Drop for SpscRingBuffer<T> {
    fn drop(&mut self) {
        // Drop remaining items
        while self.try_pop().is_some() {}
    }
}

// Safety: SpscRingBuffer is safe to send between threads
unsafe impl<T: Send> Send for SpscRingBuffer<T> {}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bounded_channel() {
        let (tx, mut rx) = BoundedChannel::new(2);

        tx.send(1u32).await.unwrap();
        tx.send(2u32).await.unwrap();

        assert_eq!(rx.recv().await, Some(1));
        assert_eq!(rx.recv().await, Some(2));
    }

    #[tokio::test]
    async fn test_bounded_channel_backpressure() {
        let (tx, mut rx) = BoundedChannel::<u32>::new(1);

        tx.send(1).await.unwrap();

        // Should fail - channel full
        assert!(tx.try_send(2).is_err());

        // Receive to make space
        rx.recv().await;

        // Now should succeed
        assert!(tx.try_send(2).is_ok());
    }

    #[tokio::test]
    async fn test_message_channel() {
        let (chan, mut rx) = MessageChannel::new(10, 42);

        chan.send_bytes(Bytes::from("hello")).await.unwrap();

        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.stream_id, 42);
        assert_eq!(msg.sequence, 0);
    }

    #[tokio::test]
    async fn test_broadcast_channel() {
        let broadcast = BroadcastChannel::new(10);

        let mut rx1 = broadcast.subscribe();
        let mut rx2 = broadcast.subscribe();

        broadcast.broadcast(42u32).await;

        assert_eq!(rx1.recv().await, Some(42));
        assert_eq!(rx2.recv().await, Some(42));
    }

    #[tokio::test]
    async fn test_request_response() {
        let (client, mut handler) = RequestChannel::<u32, u32>::new(10);

        // Spawn handler
        tokio::spawn(async move {
            while let Some((req, resp_tx)) = handler.recv().await {
                let _ = resp_tx.send(req * 2);
            }
        });

        let response = client.request(21).await.unwrap();
        assert_eq!(response, 42);
    }

    #[test]
    fn test_spsc_ring_buffer() {
        let mut buffer = SpscRingBuffer::<u32>::new(4);

        assert!(buffer.try_push(1).is_ok());
        assert!(buffer.try_push(2).is_ok());
        assert!(buffer.try_push(3).is_ok());
        assert!(buffer.try_push(4).is_ok());

        // Full
        assert!(buffer.try_push(5).is_err());

        assert_eq!(buffer.try_pop(), Some(1));
        assert_eq!(buffer.try_pop(), Some(2));

        // Can push again
        assert!(buffer.try_push(5).is_ok());
    }
}
