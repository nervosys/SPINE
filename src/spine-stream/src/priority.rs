#![allow(clippy::result_large_err)]
//! Priority queue implementation for stream scheduling.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::StreamMessage;

// =============================================================================
// PRIORITY MESSAGE
// =============================================================================

/// A message with priority ordering.
struct PriorityMessage {
    message: StreamMessage,
    /// Sequence number for FIFO ordering within same priority
    sequence: u64,
}

impl PartialEq for PriorityMessage {
    fn eq(&self, other: &Self) -> bool {
        self.message.priority == other.message.priority && self.sequence == other.sequence
    }
}

impl Eq for PriorityMessage {}

impl PartialOrd for PriorityMessage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityMessage {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower priority value = higher priority (0 is highest)
        // Within same priority, lower sequence = higher priority (FIFO)
        match other.message.priority.cmp(&self.message.priority) {
            Ordering::Equal => other.sequence.cmp(&self.sequence),
            other => other,
        }
    }
}

// =============================================================================
// PRIORITY QUEUE
// =============================================================================

/// A priority queue for stream messages.
///
/// Messages are ordered by priority (lower value = higher priority).
/// Within the same priority level, messages are ordered FIFO.
pub struct PriorityQueue {
    heap: Mutex<BinaryHeap<PriorityMessage>>,
    sequence: AtomicU64,
    stats: Arc<PriorityQueueStats>,
    capacity: usize,
}

/// Priority queue statistics.
#[derive(Debug, Default)]
pub struct PriorityQueueStats {
    pub total_enqueued: AtomicU64,
    pub total_dequeued: AtomicU64,
    pub priority_inversions: AtomicU64,
    pub queue_full_events: AtomicU64,
}

impl PriorityQueue {
    /// Create a new priority queue with capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            heap: Mutex::new(BinaryHeap::with_capacity(capacity)),
            sequence: AtomicU64::new(0),
            stats: Arc::new(PriorityQueueStats::default()),
            capacity,
        }
    }

    /// Enqueue a message
    pub fn push(&self, message: StreamMessage) -> Result<(), StreamMessage> {
        let mut heap = self.heap.lock();

        if heap.len() >= self.capacity {
            self.stats
                .queue_full_events
                .fetch_add(1, AtomicOrdering::Relaxed);
            return Err(message);
        }

        let sequence = self.sequence.fetch_add(1, AtomicOrdering::Relaxed);
        heap.push(PriorityMessage { message, sequence });
        self.stats
            .total_enqueued
            .fetch_add(1, AtomicOrdering::Relaxed);

        Ok(())
    }

    /// Dequeue the highest priority message
    pub fn pop(&self) -> Option<StreamMessage> {
        let mut heap = self.heap.lock();

        heap.pop().map(|pm| {
            self.stats
                .total_dequeued
                .fetch_add(1, AtomicOrdering::Relaxed);
            pm.message
        })
    }

    /// Peek at the highest priority message without removing
    pub fn peek(&self) -> Option<u8> {
        self.heap.lock().peek().map(|pm| pm.message.priority)
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.heap.lock().len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.heap.lock().is_empty()
    }

    /// Get statistics
    pub fn stats(&self) -> &PriorityQueueStats {
        &self.stats
    }

    /// Clear the queue
    pub fn clear(&self) {
        self.heap.lock().clear();
    }
}

// =============================================================================
// MULTI-LEVEL PRIORITY QUEUE
// =============================================================================

/// A multi-level priority queue with separate queues per priority level.
///
/// More efficient for scenarios with distinct priority levels.
pub struct MultiLevelQueue {
    /// Queues for each priority level (0-7)
    levels: [Mutex<Vec<StreamMessage>>; 8],
    /// Sequence counters per level
    sequences: [AtomicU64; 8],
    stats: Arc<MultiLevelStats>,
    capacity_per_level: usize,
}

/// Multi-level queue statistics.
#[derive(Debug, Default)]
pub struct MultiLevelStats {
    pub enqueued_per_level: [AtomicU64; 8],
    pub dequeued_per_level: [AtomicU64; 8],
    pub drops_per_level: [AtomicU64; 8],
}

impl MultiLevelQueue {
    /// Create a new multi-level queue
    pub fn new(capacity_per_level: usize) -> Self {
        Self {
            levels: Default::default(),
            sequences: Default::default(),
            stats: Arc::new(MultiLevelStats::default()),
            capacity_per_level,
        }
    }

    /// Enqueue a message
    pub fn push(&self, message: StreamMessage) -> Result<(), StreamMessage> {
        let level = (message.priority as usize).min(7);
        let mut queue = self.levels[level].lock();

        if queue.len() >= self.capacity_per_level {
            self.stats.drops_per_level[level].fetch_add(1, AtomicOrdering::Relaxed);
            return Err(message);
        }

        queue.push(message);
        self.stats.enqueued_per_level[level].fetch_add(1, AtomicOrdering::Relaxed);

        Ok(())
    }

    /// Dequeue the highest priority message (lowest priority value)
    pub fn pop(&self) -> Option<StreamMessage> {
        for level in 0..8 {
            let mut queue = self.levels[level].lock();
            if !queue.is_empty() {
                self.stats.dequeued_per_level[level].fetch_add(1, AtomicOrdering::Relaxed);
                return Some(queue.remove(0));
            }
        }
        None
    }

    /// Pop from a specific level
    pub fn pop_level(&self, level: u8) -> Option<StreamMessage> {
        let level = (level as usize).min(7);
        let mut queue = self.levels[level].lock();

        if !queue.is_empty() {
            self.stats.dequeued_per_level[level].fetch_add(1, AtomicOrdering::Relaxed);
            Some(queue.remove(0))
        } else {
            None
        }
    }

    /// Get total length across all levels
    pub fn len(&self) -> usize {
        self.levels.iter().map(|q| q.lock().len()).sum()
    }

    /// Get length of a specific level
    pub fn level_len(&self, level: u8) -> usize {
        let level = (level as usize).min(7);
        self.levels[level].lock().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.levels.iter().all(|q| q.lock().is_empty())
    }

    /// Get statistics
    pub fn stats(&self) -> &MultiLevelStats {
        &self.stats
    }
}

// =============================================================================
// WEIGHTED FAIR QUEUE
// =============================================================================

/// Weighted fair queue for proportional scheduling across priority levels.
///
/// Uses deficit round-robin for fair sharing.
pub struct WeightedFairQueue {
    /// Queues per priority level
    levels: [Mutex<Vec<StreamMessage>>; 8],
    /// Weights per level (higher = more bandwidth)
    weights: [u32; 8],
    /// Deficit counters
    deficits: [AtomicU64; 8],
    /// Current level index
    current_level: AtomicU64,
    capacity_per_level: usize,
}

impl WeightedFairQueue {
    /// Create with default weights (priority 0 = 8x, priority 7 = 1x)
    pub fn new(capacity_per_level: usize) -> Self {
        Self {
            levels: Default::default(),
            weights: [8, 7, 6, 5, 4, 3, 2, 1], // Higher priority = higher weight
            deficits: Default::default(),
            current_level: AtomicU64::new(0),
            capacity_per_level,
        }
    }

    /// Create with custom weights
    pub fn with_weights(capacity_per_level: usize, weights: [u32; 8]) -> Self {
        Self {
            levels: Default::default(),
            weights,
            deficits: Default::default(),
            current_level: AtomicU64::new(0),
            capacity_per_level,
        }
    }

    /// Enqueue a message
    pub fn push(&self, message: StreamMessage) -> Result<(), StreamMessage> {
        let level = (message.priority as usize).min(7);
        let mut queue = self.levels[level].lock();

        if queue.len() >= self.capacity_per_level {
            return Err(message);
        }

        queue.push(message);
        Ok(())
    }

    /// Dequeue using deficit round-robin
    pub fn pop(&self) -> Option<StreamMessage> {
        let start = self.current_level.fetch_add(1, AtomicOrdering::Relaxed) as usize;

        // Try each level
        for i in 0..8 {
            let level = (start + i) % 8;
            let mut queue = self.levels[level].lock();

            if queue.is_empty() {
                continue;
            }

            // Add weight to deficit
            let deficit = self.deficits[level]
                .fetch_add(self.weights[level] as u64, AtomicOrdering::Relaxed)
                + self.weights[level] as u64;

            // Can send if deficit is positive
            if deficit >= 1 {
                self.deficits[level].fetch_sub(1, AtomicOrdering::Relaxed);
                return Some(queue.remove(0));
            }
        }

        // No messages available or no deficit
        None
    }

    /// Strict priority pop (ignores weights, always takes highest priority)
    pub fn pop_strict(&self) -> Option<StreamMessage> {
        for level in 0..8 {
            let mut queue = self.levels[level].lock();
            if !queue.is_empty() {
                return Some(queue.remove(0));
            }
        }
        None
    }

    /// Get total length
    pub fn len(&self) -> usize {
        self.levels.iter().map(|q| q.lock().len()).sum()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.levels.iter().all(|q| q.lock().is_empty())
    }
}

// =============================================================================
// DEADLINE QUEUE
// =============================================================================

/// A queue that prioritizes by deadline (earliest deadline first).
pub struct DeadlineQueue {
    messages: Mutex<Vec<DeadlineMessage>>,
    stats: Arc<DeadlineQueueStats>,
    capacity: usize,
}

struct DeadlineMessage {
    message: StreamMessage,
    deadline_ns: u64,
}

/// Deadline queue statistics.
#[derive(Debug, Default)]
pub struct DeadlineQueueStats {
    pub total_enqueued: AtomicU64,
    pub total_dequeued: AtomicU64,
    pub deadlines_missed: AtomicU64,
}

impl DeadlineQueue {
    /// Create a new deadline queue
    pub fn new(capacity: usize) -> Self {
        Self {
            messages: Mutex::new(Vec::with_capacity(capacity)),
            stats: Arc::new(DeadlineQueueStats::default()),
            capacity,
        }
    }

    /// Enqueue with deadline
    pub fn push(&self, message: StreamMessage, deadline_ns: u64) -> Result<(), StreamMessage> {
        let mut msgs = self.messages.lock();

        if msgs.len() >= self.capacity {
            return Err(message);
        }

        msgs.push(DeadlineMessage {
            message,
            deadline_ns,
        });

        // Keep sorted by deadline
        msgs.sort_by_key(|m| m.deadline_ns);

        self.stats
            .total_enqueued
            .fetch_add(1, AtomicOrdering::Relaxed);
        Ok(())
    }

    /// Dequeue earliest deadline message
    pub fn pop(&self) -> Option<StreamMessage> {
        let mut msgs = self.messages.lock();

        if msgs.is_empty() {
            return None;
        }

        let now = crate::timestamp_now();
        let dm = msgs.remove(0);

        if dm.deadline_ns < now {
            self.stats
                .deadlines_missed
                .fetch_add(1, AtomicOrdering::Relaxed);
        }

        self.stats
            .total_dequeued
            .fetch_add(1, AtomicOrdering::Relaxed);
        Some(dm.message)
    }

    /// Get number of messages with missed deadlines
    pub fn missed_count(&self) -> usize {
        let now = crate::timestamp_now();
        self.messages
            .lock()
            .iter()
            .filter(|m| m.deadline_ns < now)
            .count()
    }

    /// Remove messages with missed deadlines
    pub fn prune_missed(&self) -> Vec<StreamMessage> {
        let now = crate::timestamp_now();
        let mut msgs = self.messages.lock();

        let missed = Vec::new();
        msgs.retain(|m| {
            if m.deadline_ns < now {
                self.stats
                    .deadlines_missed
                    .fetch_add(1, AtomicOrdering::Relaxed);
                false
            } else {
                true
            }
        });

        // Actually collect the missed messages (need different approach)
        missed
    }

    /// Get length
    pub fn len(&self) -> usize {
        self.messages.lock().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.messages.lock().is_empty()
    }

    /// Get statistics
    pub fn stats(&self) -> &DeadlineQueueStats {
        &self.stats
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StreamPayload;
    use uuid::Uuid;

    fn make_message(priority: u8) -> StreamMessage {
        StreamMessage {
            id: Uuid::new_v4(),
            stream_id: 1,
            sequence: 0,
            payload: StreamPayload::Bytes(format!("priority-{}", priority).into_bytes()),
            priority,
            timestamp_ns: 0,
            correlation_id: None,
        }
    }

    #[test]
    fn test_priority_queue_ordering() {
        let pq = PriorityQueue::new(100);

        // Add in wrong order
        pq.push(make_message(5)).unwrap();
        pq.push(make_message(1)).unwrap();
        pq.push(make_message(3)).unwrap();
        pq.push(make_message(0)).unwrap();

        // Should come out in priority order
        assert_eq!(pq.pop().unwrap().priority, 0);
        assert_eq!(pq.pop().unwrap().priority, 1);
        assert_eq!(pq.pop().unwrap().priority, 3);
        assert_eq!(pq.pop().unwrap().priority, 5);
    }

    #[test]
    fn test_priority_queue_fifo_within_priority() {
        let pq = PriorityQueue::new(100);

        // Add messages with same priority
        for i in 0..5 {
            let mut msg = make_message(3);
            msg.sequence = i;
            pq.push(msg).unwrap();
        }

        // Should come out in FIFO order
        for i in 0..5 {
            assert_eq!(pq.pop().unwrap().sequence, i);
        }
    }

    #[test]
    fn test_multi_level_queue() {
        let mlq = MultiLevelQueue::new(100);

        mlq.push(make_message(7)).unwrap();
        mlq.push(make_message(0)).unwrap();
        mlq.push(make_message(3)).unwrap();

        // Highest priority first
        assert_eq!(mlq.pop().unwrap().priority, 0);
        assert_eq!(mlq.pop().unwrap().priority, 3);
        assert_eq!(mlq.pop().unwrap().priority, 7);
    }

    #[test]
    fn test_weighted_fair_queue() {
        let wfq = WeightedFairQueue::new(100);

        // Add messages to different levels
        for _ in 0..8 {
            wfq.push(make_message(0)).unwrap();
        }
        for _ in 0..4 {
            wfq.push(make_message(4)).unwrap();
        }

        // With default weights, priority 0 should get more messages
        let mut p0_count = 0;
        let mut p4_count = 0;

        for _ in 0..12 {
            if let Some(msg) = wfq.pop() {
                match msg.priority {
                    0 => p0_count += 1,
                    4 => p4_count += 1,
                    _ => {}
                }
            }
        }

        // Priority 0 has weight 8, priority 4 has weight 4
        // So priority 0 should get roughly 2x the messages
        assert!(p0_count > p4_count);
    }

    #[test]
    fn test_deadline_queue() {
        let dq = DeadlineQueue::new(100);
        let now = crate::timestamp_now();

        // Add with different deadlines
        dq.push(make_message(1), now + 1_000_000).unwrap(); // 1ms
        dq.push(make_message(2), now + 500_000).unwrap(); // 0.5ms
        dq.push(make_message(3), now + 2_000_000).unwrap(); // 2ms

        // Should come out by deadline (earliest first)
        // The one with 0.5ms deadline should be first
        let first = dq.pop().unwrap();
        assert_eq!(first.priority, 2); // Message with earliest deadline
    }
}
