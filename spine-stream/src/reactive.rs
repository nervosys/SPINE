//! Reactive stream abstractions with backpressure support.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use pin_project_lite::pin_project;
use tokio::sync::{mpsc, Semaphore};
use tokio::time::Instant;

use crate::{StreamError, StreamResult};

// =============================================================================
// BACKPRESSURE STREAM
// =============================================================================

/// A stream with built-in backpressure support.
///
/// Uses a semaphore to limit in-flight items, automatically applying
/// backpressure when the receiver can't keep up.
pub struct BackpressureStream<T> {
    /// Receiver for items
    rx: mpsc::Receiver<T>,
    /// Semaphore for backpressure control
    semaphore: Arc<Semaphore>,
    /// Permits per item (configurable)
    permits_per_item: u32,
    /// Statistics
    stats: Arc<BackpressureStats>,
}

/// Sender half of a backpressure stream
pub struct BackpressureSender<T> {
    tx: mpsc::Sender<T>,
    semaphore: Arc<Semaphore>,
    permits_per_item: u32,
    stats: Arc<BackpressureStats>,
}

/// Statistics for backpressure monitoring
#[derive(Debug, Default)]
pub struct BackpressureStats {
    /// Items sent
    pub items_sent: AtomicU64,
    /// Items received
    pub items_received: AtomicU64,
    /// Times backpressure was applied
    pub backpressure_events: AtomicU64,
    /// Total wait time due to backpressure (microseconds)
    pub backpressure_wait_us: AtomicU64,
    /// Current in-flight items
    pub in_flight: AtomicU64,
    /// Is currently under backpressure?
    pub under_pressure: AtomicBool,
}

impl<T> BackpressureStream<T> {
    /// Create a new backpressure stream
    pub fn new(max_in_flight: usize) -> (BackpressureSender<T>, Self) {
        Self::with_config(max_in_flight, 1)
    }

    /// Create with custom permits per item
    pub fn with_config(
        max_in_flight: usize,
        permits_per_item: u32,
    ) -> (BackpressureSender<T>, Self) {
        let (tx, rx) = mpsc::channel(max_in_flight);
        let semaphore = Arc::new(Semaphore::new(max_in_flight));
        let stats = Arc::new(BackpressureStats::default());

        let sender = BackpressureSender {
            tx,
            semaphore: Arc::clone(&semaphore),
            permits_per_item,
            stats: Arc::clone(&stats),
        };

        let stream = Self {
            rx,
            semaphore,
            permits_per_item,
            stats,
        };

        (sender, stream)
    }

    /// Get statistics
    pub fn stats(&self) -> &BackpressureStats {
        &self.stats
    }

    /// Receive next item
    pub async fn recv(&mut self) -> Option<T> {
        let item = self.rx.recv().await?;
        self.semaphore.add_permits(self.permits_per_item as usize);
        self.stats.items_received.fetch_add(1, Ordering::Relaxed);
        self.stats.in_flight.fetch_sub(1, Ordering::Relaxed);
        Some(item)
    }

    /// Try to receive without blocking
    pub fn try_recv(&mut self) -> Result<T, mpsc::error::TryRecvError> {
        let item = self.rx.try_recv()?;
        self.semaphore.add_permits(self.permits_per_item as usize);
        self.stats.items_received.fetch_add(1, Ordering::Relaxed);
        self.stats.in_flight.fetch_sub(1, Ordering::Relaxed);
        Ok(item)
    }
}

impl<T> BackpressureSender<T> {
    /// Send an item, waiting for backpressure if necessary
    pub async fn send(&self, item: T) -> StreamResult<()> {
        let start = Instant::now();

        // Acquire permit (may block if under backpressure)
        let available = self.semaphore.available_permits();
        if available == 0 {
            self.stats
                .backpressure_events
                .fetch_add(1, Ordering::Relaxed);
            self.stats.under_pressure.store(true, Ordering::Relaxed);
        }

        let permit = self
            .semaphore
            .acquire_many(self.permits_per_item)
            .await
            .map_err(|_| StreamError::Closed)?;
        permit.forget(); // Will be restored when item is consumed

        let wait_time = start.elapsed();
        if wait_time > Duration::from_micros(100) {
            self.stats
                .backpressure_wait_us
                .fetch_add(wait_time.as_micros() as u64, Ordering::Relaxed);
        }
        self.stats.under_pressure.store(false, Ordering::Relaxed);

        // Send the item
        self.tx
            .send(item)
            .await
            .map_err(|_| StreamError::ChannelSendError)?;

        self.stats.items_sent.fetch_add(1, Ordering::Relaxed);
        self.stats.in_flight.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Try to send without blocking
    pub fn try_send(&self, item: T) -> StreamResult<()> {
        // Check if we have permits
        let permit = self
            .semaphore
            .try_acquire_many(self.permits_per_item)
            .map_err(|_| StreamError::BackpressureLimitReached)?;
        permit.forget();

        self.tx
            .try_send(item)
            .map_err(|_| StreamError::ChannelSendError)?;

        self.stats.items_sent.fetch_add(1, Ordering::Relaxed);
        self.stats.in_flight.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Check if under backpressure
    pub fn is_under_pressure(&self) -> bool {
        self.stats.under_pressure.load(Ordering::Relaxed)
    }

    /// Get current in-flight count
    pub fn in_flight(&self) -> u64 {
        self.stats.in_flight.load(Ordering::Relaxed)
    }
}

impl<T> Clone for BackpressureSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            semaphore: Arc::clone(&self.semaphore),
            permits_per_item: self.permits_per_item,
            stats: Arc::clone(&self.stats),
        }
    }
}

// =============================================================================
// BATCHING STREAM
// =============================================================================

pin_project! {
    /// A stream that batches items together for efficiency.
    pub struct BatchingStream<S, T> {
        #[pin]
        inner: S,
        batch: Vec<T>,
        max_batch_size: usize,
        max_wait: Duration,
        deadline: Option<Instant>,
    }
}

impl<S, T> BatchingStream<S, T>
where
    S: Stream<Item = T>,
{
    /// Create a new batching stream
    pub fn new(inner: S, max_batch_size: usize, max_wait: Duration) -> Self {
        Self {
            inner,
            batch: Vec::with_capacity(max_batch_size),
            max_batch_size,
            max_wait,
            deadline: None,
        }
    }
}

impl<S, T> Stream for BatchingStream<S, T>
where
    S: Stream<Item = T>,
{
    type Item = Vec<T>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            // Check if we should emit the batch due to timeout
            if let Some(deadline) = *this.deadline {
                if Instant::now() >= deadline && !this.batch.is_empty() {
                    *this.deadline = None;
                    return Poll::Ready(Some(std::mem::take(this.batch)));
                }
            }

            // Try to get more items
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(item)) => {
                    // Set deadline on first item
                    if this.batch.is_empty() {
                        *this.deadline = Some(Instant::now() + *this.max_wait);
                    }

                    this.batch.push(item);

                    // Emit if batch is full
                    if this.batch.len() >= *this.max_batch_size {
                        *this.deadline = None;
                        return Poll::Ready(Some(std::mem::take(this.batch)));
                    }
                }
                Poll::Ready(None) => {
                    // Stream ended, emit remaining batch
                    if this.batch.is_empty() {
                        return Poll::Ready(None);
                    } else {
                        return Poll::Ready(Some(std::mem::take(this.batch)));
                    }
                }
                Poll::Pending => {
                    // No items available
                    if !this.batch.is_empty() {
                        // We have items and are waiting for more
                        // TODO: Register deadline waker
                        return Poll::Pending;
                    }
                    return Poll::Pending;
                }
            }
        }
    }
}

// =============================================================================
// RATE LIMITED STREAM
// =============================================================================

/// A stream that limits throughput
pub struct RateLimitedStream<S> {
    inner: S,
    rate_limiter: Arc<tokio::sync::Mutex<RateLimiterState>>,
}

struct RateLimiterState {
    tokens: f64,
    last_update: Instant,
    tokens_per_sec: f64,
    max_tokens: f64,
}

impl<S> RateLimitedStream<S> {
    /// Create a new rate-limited stream
    pub fn new(inner: S, items_per_sec: f64, burst_size: usize) -> Self {
        Self {
            inner,
            rate_limiter: Arc::new(tokio::sync::Mutex::new(RateLimiterState {
                tokens: burst_size as f64,
                last_update: Instant::now(),
                tokens_per_sec: items_per_sec,
                max_tokens: burst_size as f64,
            })),
        }
    }
}

impl<S, T> RateLimitedStream<S>
where
    S: Stream<Item = T> + Unpin,
{
    /// Get the next item, respecting rate limits
    pub async fn next(&mut self) -> Option<T> {
        use futures::StreamExt;

        // Wait for a token
        loop {
            let mut state = self.rate_limiter.lock().await;

            // Refill tokens
            let now = Instant::now();
            let elapsed = now.duration_since(state.last_update).as_secs_f64();
            state.tokens = (state.tokens + elapsed * state.tokens_per_sec).min(state.max_tokens);
            state.last_update = now;

            if state.tokens >= 1.0 {
                state.tokens -= 1.0;
                drop(state);

                // Get the item
                return self.inner.next().await;
            }

            // Calculate wait time
            let wait_secs = (1.0 - state.tokens) / state.tokens_per_sec;
            drop(state);

            tokio::time::sleep(Duration::from_secs_f64(wait_secs)).await;
        }
    }
}

// =============================================================================
// WINDOWED STREAM
// =============================================================================

/// A sliding window over a stream
pub struct WindowedStream<T> {
    buffer: Vec<T>,
    window_size: usize,
    stride: usize,
    position: usize,
}

impl<T: Clone> WindowedStream<T> {
    /// Create a new windowed stream
    pub fn new(window_size: usize, stride: usize) -> Self {
        assert!(stride > 0);
        assert!(window_size >= stride);

        Self {
            buffer: Vec::with_capacity(window_size),
            window_size,
            stride,
            position: 0,
        }
    }

    /// Push an item, returning a window if ready
    pub fn push(&mut self, item: T) -> Option<Vec<T>> {
        self.buffer.push(item);
        self.position += 1;

        // Check if we have a full window
        if self.buffer.len() >= self.window_size {
            let window = self.buffer.clone();

            // Remove stride items from front
            self.buffer.drain(0..self.stride);

            Some(window)
        } else {
            None
        }
    }

    /// Flush remaining items as a partial window
    pub fn flush(self) -> Option<Vec<T>> {
        if self.buffer.is_empty() {
            None
        } else {
            Some(self.buffer)
        }
    }
}

// =============================================================================
// TRANSFORM STREAM
// =============================================================================

/// Stream transformer that applies a function to each item
pub struct TransformStream<S, F, T, U>
where
    S: Stream<Item = T>,
    F: FnMut(T) -> U,
{
    inner: S,
    transform: F,
}

impl<S, F, T, U> TransformStream<S, F, T, U>
where
    S: Stream<Item = T>,
    F: FnMut(T) -> U,
{
    pub fn new(inner: S, transform: F) -> Self {
        Self { inner, transform }
    }
}

// =============================================================================
// REACTIVE HELPERS
// =============================================================================

/// Extension trait for reactive stream operations
pub trait ReactiveStreamExt: Stream + Sized {
    /// Batch items together
    fn batch(self, max_size: usize, max_wait: Duration) -> BatchingStream<Self, Self::Item> {
        BatchingStream::new(self, max_size, max_wait)
    }

    /// Apply rate limiting
    fn rate_limit(self, items_per_sec: f64, burst: usize) -> RateLimitedStream<Self> {
        RateLimitedStream::new(self, items_per_sec, burst)
    }
}

impl<S: Stream> ReactiveStreamExt for S {}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_backpressure_stream() {
        let (tx, mut rx) = BackpressureStream::<i32>::new(4);

        // Send items
        for i in 0..4 {
            tx.send(i).await.unwrap();
        }

        // Receive items
        for i in 0..4 {
            let item = rx.recv().await.unwrap();
            assert_eq!(item, i);
        }
    }

    #[test]
    fn test_windowed_stream() {
        let mut window = WindowedStream::new(3, 1);

        assert!(window.push(1).is_none());
        assert!(window.push(2).is_none());

        let w = window.push(3).unwrap();
        assert_eq!(w, vec![1, 2, 3]);

        let w = window.push(4).unwrap();
        assert_eq!(w, vec![2, 3, 4]);
    }

    #[tokio::test]
    async fn test_backpressure_stats() {
        let (tx, mut rx) = BackpressureStream::<i32>::new(2);

        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();

        assert_eq!(tx.in_flight(), 2);

        rx.recv().await;
        assert_eq!(rx.stats().items_received.load(Ordering::Relaxed), 1);
    }
}
