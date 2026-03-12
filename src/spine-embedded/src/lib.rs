//! # SPINE Embedded Runtime
//!
//! Minimal agent runtime for embedded and IoT targets (ARM Cortex-M, ESP32,
//! RISC-V). Built entirely on `spine-nostd` primitives — zero heap allocation,
//! fixed-point math, stack-only message processing.
//!
//! ## Features
//!
//! - **EmbeddedAgent**: Lightweight agent loop with inbox/outbox ring buffers
//! - **MessageRouter**: Fixed routing table for multi-hop delivery
//! - **SensorBridge**: Ingest sensor data as latent vectors
//! - **FixedPointInference**: Neural similarity without FPU
//! - **WatchdogTimer**: Deadline monitoring for real-time constraints

#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

use spine_nostd::codec::{decode_frame_header, encode_frame_header};
use spine_nostd::hash::fnv1a_64;
use spine_nostd::math::{cosine_similarity_fixed, dot_product_fixed};
use spine_nostd::types::{AgentIdBytes, FrameHeader, LatentVectorFixed};

// ══════════════════════════════ Agent Config ══════════════════════════════

/// Configuration for an embedded agent (all stack-allocated).
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedConfig {
    /// Maximum messages in the inbox ring buffer
    pub inbox_capacity: usize,
    /// Maximum messages in the outbox ring buffer
    pub outbox_capacity: usize,
    /// Watchdog timeout in milliseconds (0 = disabled)
    pub watchdog_ms: u32,
    /// Maximum routing table entries
    pub max_routes: usize,
    /// Similarity threshold for message matching (Q8.8 fixed-point)
    pub similarity_threshold: i16,
}

impl Default for EmbeddedConfig {
    fn default() -> Self {
        Self {
            inbox_capacity: 16,
            outbox_capacity: 16,
            watchdog_ms: 5000,
            max_routes: 8,
            similarity_threshold: 200, // ~0.78 in Q8.8
        }
    }
}

// ══════════════════════════════ Messages ══════════════════════════════

/// A fixed-size message for embedded transport (no heap).
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedMessage {
    /// Source agent
    pub src: AgentIdBytes,
    /// Destination agent
    pub dst: AgentIdBytes,
    /// Message type tag
    pub msg_type: u8,
    /// TTL for multi-hop routing
    pub ttl: u8,
    /// Sequence number
    pub sequence: u16,
    /// Payload (fixed 64 bytes, length-prefixed)
    pub payload_len: u8,
    pub payload: [u8; 64],
    /// Latent embedding of this message (for similarity matching)
    pub embedding: LatentVectorFixed<16>,
}

impl EmbeddedMessage {
    /// Create a new message with empty payload.
    pub fn new(src: AgentIdBytes, dst: AgentIdBytes, msg_type: u8) -> Self {
        Self {
            src,
            dst,
            msg_type,
            ttl: 8,
            sequence: 0,
            payload_len: 0,
            payload: [0u8; 64],
            embedding: LatentVectorFixed::zero(),
        }
    }

    /// Set payload from a byte slice (truncated to 64 bytes).
    pub fn set_payload(&mut self, data: &[u8]) {
        let len = data.len().min(64);
        self.payload[..len].copy_from_slice(&data[..len]);
        self.payload_len = len as u8;
    }

    /// Get payload slice.
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len as usize]
    }

    /// Compute a hash fingerprint of this message.
    pub fn fingerprint(&self) -> u64 {
        let mut data = [0u8; 35];
        data[..16].copy_from_slice(self.src.as_bytes());
        data[16..32].copy_from_slice(self.dst.as_bytes());
        data[32] = self.msg_type;
        data[33] = (self.sequence >> 8) as u8;
        data[34] = self.sequence as u8;
        fnv1a_64(&data)
    }

    /// Encode this message into a frame header for wire transmission.
    pub fn to_frame_header(&self) -> FrameHeader {
        FrameHeader::new(
            self.payload_len as u32,
            self.msg_type,
            self.sequence,
        )
    }

    /// Encode header bytes into a buffer. Returns 12 (header size).
    pub fn encode_header(&self, buf: &mut [u8; 12]) {
        let header = self.to_frame_header();
        encode_frame_header(&header, buf);
    }

    /// Decode a frame header from wire bytes.
    pub fn decode_header(buf: &[u8]) -> Option<FrameHeader> {
        decode_frame_header(buf)
    }
}

// ══════════════════════════════ Ring Buffer ══════════════════════════════

/// Fixed-capacity ring buffer for messages (no heap allocation).
/// Uses a const generic for compile-time sizing.
#[derive(Debug)]
pub struct MessageRing<const N: usize> {
    buf: [Option<EmbeddedMessage>; N],
    head: usize,
    tail: usize,
    count: usize,
}

impl<const N: usize> Default for MessageRing<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> MessageRing<N> {
    /// # Safety
    /// N must be > 0 (enforced by type system since arrays of size 0 are valid but useless).
    pub const fn new() -> Self {
        // Initialize with None using const-compatible pattern
        Self {
            buf: [const { None }; N],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    /// Push a message. Returns false if full.
    pub fn push(&mut self, msg: EmbeddedMessage) -> bool {
        if self.count >= N {
            return false;
        }
        self.buf[self.tail] = Some(msg);
        self.tail = (self.tail + 1) % N;
        self.count += 1;
        true
    }

    /// Pop the oldest message.
    pub fn pop(&mut self) -> Option<EmbeddedMessage> {
        if self.count == 0 {
            return None;
        }
        let msg = self.buf[self.head].take();
        self.head = (self.head + 1) % N;
        self.count -= 1;
        msg
    }

    /// Peek at the oldest message without removing it.
    pub fn peek(&self) -> Option<&EmbeddedMessage> {
        if self.count == 0 {
            None
        } else {
            self.buf[self.head].as_ref()
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn is_full(&self) -> bool {
        self.count >= N
    }

    pub fn capacity(&self) -> usize {
        N
    }

    /// Clear all messages.
    pub fn clear(&mut self) {
        while self.pop().is_some() {}
    }
}

// ══════════════════════════════ Routing Table ══════════════════════════════

/// A fixed-size routing table entry.
#[derive(Debug, Clone, Copy)]
pub struct RouteEntry {
    /// Destination agent
    pub destination: AgentIdBytes,
    /// Next hop agent
    pub next_hop: AgentIdBytes,
    /// Hop count to destination
    pub hop_count: u8,
    /// Is this entry active?
    pub active: bool,
}

/// Fixed-size routing table for embedded environments.
#[derive(Debug)]
pub struct RoutingTable<const N: usize> {
    entries: [RouteEntry; N],
    count: usize,
}

impl<const N: usize> Default for RoutingTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> RoutingTable<N> {
    pub const fn new() -> Self {
        let empty = RouteEntry {
            destination: AgentIdBytes::zero(),
            next_hop: AgentIdBytes::zero(),
            hop_count: 0,
            active: false,
        };
        Self {
            entries: [empty; N],
            count: 0,
        }
    }

    /// Add or update a route. Returns false if table is full.
    pub fn update(&mut self, dest: AgentIdBytes, next_hop: AgentIdBytes, hops: u8) -> bool {
        // Update existing route if shorter
        for entry in self.entries.iter_mut() {
            if entry.active && entry.destination.as_bytes() == dest.as_bytes() {
                if hops < entry.hop_count {
                    entry.next_hop = next_hop;
                    entry.hop_count = hops;
                }
                return true;
            }
        }
        // Insert new route
        if self.count >= N {
            return false;
        }
        for entry in self.entries.iter_mut() {
            if !entry.active {
                *entry = RouteEntry {
                    destination: dest,
                    next_hop,
                    hop_count: hops,
                    active: true,
                };
                self.count += 1;
                return true;
            }
        }
        false
    }

    /// Look up the next hop for a destination.
    pub fn next_hop(&self, dest: &AgentIdBytes) -> Option<&AgentIdBytes> {
        self.entries
            .iter()
            .find(|e| e.active && e.destination.as_bytes() == dest.as_bytes())
            .map(|e| &e.next_hop)
    }

    /// Remove a route.
    pub fn remove(&mut self, dest: &AgentIdBytes) -> bool {
        for entry in self.entries.iter_mut() {
            if entry.active && entry.destination.as_bytes() == dest.as_bytes() {
                entry.active = false;
                self.count -= 1;
                return true;
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

// ══════════════════════════════ Sensor Bridge ══════════════════════════════

/// Converts raw sensor readings into fixed-point latent vectors.
pub struct SensorBridge<const D: usize> {
    /// Running min/max for normalization
    min_vals: [i16; D],
    max_vals: [i16; D],
    /// Number of samples ingested
    sample_count: u32,
}

impl<const D: usize> Default for SensorBridge<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const D: usize> SensorBridge<D> {
    pub const fn new() -> Self {
        Self {
            min_vals: [i16::MAX; D],
            max_vals: [i16::MIN; D],
            sample_count: 0,
        }
    }

    /// Ingest raw sensor readings and produce a normalized latent vector.
    /// Values are normalized to Q8.8 range [-256, 256] based on observed min/max.
    pub fn ingest(&mut self, readings: &[i16]) -> LatentVectorFixed<D> {
        let len = readings.len().min(D);
        // Update min/max
        for (i, &reading) in readings.iter().enumerate().take(len) {
            if reading < self.min_vals[i] {
                self.min_vals[i] = reading;
            }
            if reading > self.max_vals[i] {
                self.max_vals[i] = reading;
            }
        }
        self.sample_count += 1;

        // Normalize to Q8.8
        let mut data = [0i16; D];
        for i in 0..len {
            let range = (self.max_vals[i] as i32) - (self.min_vals[i] as i32);
            if range > 0 {
                let normalized =
                    ((readings[i] as i32 - self.min_vals[i] as i32) * 512) / range - 256;
                data[i] = normalized.clamp(-32768, 32767) as i16;
            }
        }
        LatentVectorFixed::from_slice(&data[..len])
    }

    /// Compare two sensor snapshots using cosine similarity.
    pub fn similarity(
        a: &LatentVectorFixed<D>,
        b: &LatentVectorFixed<D>,
    ) -> i16 {
        cosine_similarity_fixed(a.as_slice(), b.as_slice())
    }

    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }
}

// ══════════════════════════════ Watchdog Timer ══════════════════════════════

/// Simple deadline tracker for real-time constraints.
/// Uses a monotonic tick counter (caller must call `tick()` periodically).
#[derive(Debug)]
pub struct WatchdogTimer {
    /// Deadline in ticks
    deadline: u32,
    /// Current tick count
    current: u32,
    /// Whether the watchdog has fired
    fired: bool,
    /// Whether the watchdog is active
    active: bool,
}

impl WatchdogTimer {
    pub const fn new(deadline_ticks: u32) -> Self {
        Self {
            deadline: deadline_ticks,
            current: 0,
            fired: false,
            active: false,
        }
    }

    /// Start the watchdog.
    pub fn start(&mut self) {
        self.current = 0;
        self.fired = false;
        self.active = true;
    }

    /// Feed (reset) the watchdog — call this to indicate progress.
    pub fn feed(&mut self) {
        self.current = 0;
        self.fired = false;
    }

    /// Advance the tick counter. Returns true if the watchdog fires.
    pub fn tick(&mut self) -> bool {
        if !self.active || self.fired {
            return false;
        }
        self.current += 1;
        if self.current >= self.deadline {
            self.fired = true;
            return true;
        }
        false
    }

    pub fn has_fired(&self) -> bool {
        self.fired
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Stop the watchdog.
    pub fn stop(&mut self) {
        self.active = false;
        self.fired = false;
        self.current = 0;
    }

    /// Remaining ticks before deadline.
    pub fn remaining(&self) -> u32 {
        self.deadline.saturating_sub(self.current)
    }
}

// ══════════════════════════════ Embedded Agent ══════════════════════════════

/// Embedded agent processing result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessResult {
    /// No messages to process
    Idle,
    /// Message processed and consumed
    Consumed,
    /// Message forwarded to next hop
    Forwarded,
    /// Message queued for output
    Responded,
    /// Watchdog fired — agent is over deadline
    WatchdogFired,
    /// Inbox is full, message dropped
    Dropped,
}

/// Minimal agent runtime for embedded targets.
///
/// Uses fixed-size ring buffers for inbox/outbox and a fixed routing table.
/// All state is stack-allocated — no heap required.
pub struct EmbeddedAgent<const INBOX: usize, const OUTBOX: usize, const ROUTES: usize> {
    /// This agent's identity
    pub id: AgentIdBytes,
    /// Incoming message buffer
    pub inbox: MessageRing<INBOX>,
    /// Outgoing message buffer
    pub outbox: MessageRing<OUTBOX>,
    /// Routing table for multi-hop forwarding
    pub routes: RoutingTable<ROUTES>,
    /// Watchdog timer
    pub watchdog: WatchdogTimer,
    /// Messages processed count
    pub processed: u32,
    /// Messages forwarded count
    pub forwarded: u32,
    /// Messages dropped count
    pub dropped: u32,
}

impl<const INBOX: usize, const OUTBOX: usize, const ROUTES: usize>
    EmbeddedAgent<INBOX, OUTBOX, ROUTES>
{
    /// Create a new embedded agent.
    pub const fn new(id: AgentIdBytes, watchdog_ticks: u32) -> Self {
        Self {
            id,
            inbox: MessageRing::new(),
            outbox: MessageRing::new(),
            routes: RoutingTable::new(),
            watchdog: WatchdogTimer::new(watchdog_ticks),
            processed: 0,
            forwarded: 0,
            dropped: 0,
        }
    }

    /// Receive an incoming message from the transport layer.
    pub fn receive(&mut self, msg: EmbeddedMessage) -> ProcessResult {
        if self.inbox.is_full() {
            self.dropped += 1;
            return ProcessResult::Dropped;
        }
        self.inbox.push(msg);
        ProcessResult::Consumed
    }

    /// Process the next message in the inbox.
    ///
    /// If addressed to this agent, returns `Consumed`.
    /// If addressed to another agent and a route exists, forwards it.
    /// Otherwise drops it.
    pub fn process_next(&mut self) -> ProcessResult {
        // Check watchdog
        if self.watchdog.tick() {
            return ProcessResult::WatchdogFired;
        }

        let msg = match self.inbox.pop() {
            Some(m) => m,
            None => return ProcessResult::Idle,
        };

        // Is this for us?
        if msg.dst.as_bytes() == self.id.as_bytes() {
            self.processed += 1;
            self.watchdog.feed();
            return ProcessResult::Consumed;
        }

        // Forward if TTL allows
        if msg.ttl == 0 {
            self.dropped += 1;
            return ProcessResult::Dropped;
        }

        // Look up route
        if self.routes.next_hop(&msg.dst).is_some() {
            let mut fwd = msg;
            fwd.ttl -= 1;
            if self.outbox.push(fwd) {
                self.forwarded += 1;
                self.watchdog.feed();
                ProcessResult::Forwarded
            } else {
                self.dropped += 1;
                ProcessResult::Dropped
            }
        } else {
            self.dropped += 1;
            ProcessResult::Dropped
        }
    }

    /// Send a message (enqueue in outbox).
    pub fn send(&mut self, mut msg: EmbeddedMessage) -> bool {
        msg.src = self.id;
        self.outbox.push(msg)
    }

    /// Drain outbox: returns the next outgoing message.
    pub fn drain_outbox(&mut self) -> Option<EmbeddedMessage> {
        self.outbox.pop()
    }

    /// Compare two latent vectors using fixed-point cosine similarity.
    pub fn latent_similarity(
        a: &LatentVectorFixed<16>,
        b: &LatentVectorFixed<16>,
    ) -> i16 {
        cosine_similarity_fixed(a.as_slice(), b.as_slice())
    }

    /// Compute dot product of two latent vectors.
    pub fn latent_dot(
        a: &LatentVectorFixed<16>,
        b: &LatentVectorFixed<16>,
    ) -> i32 {
        dot_product_fixed(a.as_slice(), b.as_slice())
    }

    /// Get stats.
    pub fn stats(&self) -> AgentStats {
        AgentStats {
            processed: self.processed,
            forwarded: self.forwarded,
            dropped: self.dropped,
            inbox_len: self.inbox.len() as u16,
            outbox_len: self.outbox.len() as u16,
            routes: self.routes.len() as u16,
        }
    }
}

/// Agent statistics (copyable, no heap).
#[derive(Debug, Clone, Copy)]
pub struct AgentStats {
    pub processed: u32,
    pub forwarded: u32,
    pub dropped: u32,
    pub inbox_len: u16,
    pub outbox_len: u16,
    pub routes: u16,
}

// ══════════════════════════════ Re-exports ══════════════════════════════

pub use spine_nostd::codec;
pub use spine_nostd::hash;
pub use spine_nostd::math;
pub use spine_nostd::types;

// ══════════════════════════════ Tests ══════════════════════════════

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    fn agent_id(n: u8) -> AgentIdBytes {
        AgentIdBytes::from_bytes([n; 16])
    }

    // ── MessageRing ──

    #[test]
    fn test_ring_push_pop() {
        let mut ring: MessageRing<4> = MessageRing::new();
        assert!(ring.is_empty());

        let msg = EmbeddedMessage::new(agent_id(1), agent_id(2), 0);
        assert!(ring.push(msg));
        assert_eq!(ring.len(), 1);

        let popped = ring.pop().unwrap();
        assert_eq!(popped.msg_type, 0);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_ring_full() {
        let mut ring: MessageRing<2> = MessageRing::new();
        let msg = EmbeddedMessage::new(agent_id(1), agent_id(2), 0);
        assert!(ring.push(msg));
        let msg2 = EmbeddedMessage::new(agent_id(1), agent_id(2), 1);
        assert!(ring.push(msg2));
        assert!(ring.is_full());

        let msg3 = EmbeddedMessage::new(agent_id(1), agent_id(2), 2);
        assert!(!ring.push(msg3)); // full
    }

    #[test]
    fn test_ring_fifo_order() {
        let mut ring: MessageRing<4> = MessageRing::new();
        for i in 0..3 {
            let msg = EmbeddedMessage::new(agent_id(1), agent_id(2), i);
            ring.push(msg);
        }
        for i in 0..3 {
            let msg = ring.pop().unwrap();
            assert_eq!(msg.msg_type, i);
        }
    }

    #[test]
    fn test_ring_wrap_around() {
        let mut ring: MessageRing<2> = MessageRing::new();
        let msg1 = EmbeddedMessage::new(agent_id(1), agent_id(2), 10);
        ring.push(msg1);
        ring.pop(); // head advances

        let msg2 = EmbeddedMessage::new(agent_id(1), agent_id(2), 20);
        ring.push(msg2);
        let msg3 = EmbeddedMessage::new(agent_id(1), agent_id(2), 30);
        ring.push(msg3);

        assert_eq!(ring.pop().unwrap().msg_type, 20);
        assert_eq!(ring.pop().unwrap().msg_type, 30);
    }

    // ── RoutingTable ──

    #[test]
    fn test_routing_table_insert_lookup() {
        let mut rt: RoutingTable<4> = RoutingTable::new();
        assert!(rt.is_empty());

        let dest = agent_id(5);
        let hop = agent_id(3);
        assert!(rt.update(dest, hop, 2));
        assert_eq!(rt.len(), 1);

        let found = rt.next_hop(&dest).unwrap();
        assert_eq!(found.as_bytes(), hop.as_bytes());
    }

    #[test]
    fn test_routing_table_update_shorter() {
        let mut rt: RoutingTable<4> = RoutingTable::new();
        let dest = agent_id(5);
        rt.update(dest, agent_id(3), 4);
        rt.update(dest, agent_id(2), 2); // shorter
        let found = rt.next_hop(&dest).unwrap();
        assert_eq!(found.as_bytes(), agent_id(2).as_bytes());
    }

    #[test]
    fn test_routing_table_remove() {
        let mut rt: RoutingTable<4> = RoutingTable::new();
        let dest = agent_id(5);
        rt.update(dest, agent_id(3), 2);
        assert!(rt.remove(&dest));
        assert!(rt.is_empty());
        assert!(rt.next_hop(&dest).is_none());
    }

    #[test]
    fn test_routing_table_full() {
        let mut rt: RoutingTable<2> = RoutingTable::new();
        rt.update(agent_id(1), agent_id(10), 1);
        rt.update(agent_id(2), agent_id(10), 1);
        assert!(!rt.update(agent_id(3), agent_id(10), 1)); // full
    }

    // ── SensorBridge ──

    #[test]
    fn test_sensor_bridge_normalization() {
        let mut bridge: SensorBridge<4> = SensorBridge::new();
        let readings = [0i16, 100, 200, 300];
        let v1 = bridge.ingest(&readings);
        assert_eq!(bridge.sample_count(), 1);
        assert_eq!(v1.as_slice().len(), 4);
    }

    #[test]
    fn test_sensor_bridge_similarity() {
        let mut bridge: SensorBridge<4> = SensorBridge::new();
        // Establish a range with different readings
        let low = [0i16, 0, 0, 0];
        let high = [1000i16, 1000, 1000, 1000];
        let _ = bridge.ingest(&low);
        let _ = bridge.ingest(&high);
        // Now ingest two similar readings
        let r1 = [500i16, 600, 700, 800];
        let r2 = [510i16, 590, 710, 790];
        let v1 = bridge.ingest(&r1);
        let v2 = bridge.ingest(&r2);
        let sim = SensorBridge::<4>::similarity(&v1, &v2);
        assert!(sim > 200, "similarity was {sim}"); // high similarity
    }

    // ── WatchdogTimer ──

    #[test]
    fn test_watchdog_fires() {
        let mut wd = WatchdogTimer::new(5);
        wd.start();
        for _ in 0..4 {
            assert!(!wd.tick());
        }
        assert!(wd.tick()); // fires on 5th tick
        assert!(wd.has_fired());
    }

    #[test]
    fn test_watchdog_feed_resets() {
        let mut wd = WatchdogTimer::new(5);
        wd.start();
        wd.tick();
        wd.tick();
        wd.feed();
        assert_eq!(wd.remaining(), 5);
        assert!(!wd.has_fired());
    }

    #[test]
    fn test_watchdog_stop() {
        let mut wd = WatchdogTimer::new(5);
        wd.start();
        wd.stop();
        assert!(!wd.tick()); // should not fire when stopped
        assert!(!wd.is_active());
    }

    // ── EmbeddedMessage ──

    #[test]
    fn test_message_payload() {
        let mut msg = EmbeddedMessage::new(agent_id(1), agent_id(2), 42);
        msg.set_payload(b"hello");
        assert_eq!(msg.payload(), b"hello");
    }

    #[test]
    fn test_message_fingerprint_differs() {
        let msg1 = EmbeddedMessage::new(agent_id(1), agent_id(2), 0);
        let mut msg2 = EmbeddedMessage::new(agent_id(1), agent_id(2), 0);
        msg2.sequence = 1;
        assert_ne!(msg1.fingerprint(), msg2.fingerprint());
    }

    #[test]
    fn test_message_frame_header_roundtrip() {
        let msg = EmbeddedMessage::new(agent_id(1), agent_id(2), 5);
        let mut buf = [0u8; 12];
        msg.encode_header(&mut buf);
        let decoded = EmbeddedMessage::decode_header(&buf).unwrap();
        assert_eq!(decoded.frame_type, 5);
    }

    // ── EmbeddedAgent ──

    #[test]
    fn test_agent_receive_and_process() {
        let me = agent_id(1);
        let mut agent: EmbeddedAgent<8, 8, 4> = EmbeddedAgent::new(me, 100);
        agent.watchdog.start();

        let msg = EmbeddedMessage::new(agent_id(2), me, 1);
        assert_eq!(agent.receive(msg), ProcessResult::Consumed);
        assert_eq!(agent.process_next(), ProcessResult::Consumed);
        assert_eq!(agent.stats().processed, 1);
    }

    #[test]
    fn test_agent_forward() {
        let me = agent_id(1);
        let dest = agent_id(3);
        let mut agent: EmbeddedAgent<8, 8, 4> = EmbeddedAgent::new(me, 100);
        agent.watchdog.start();
        agent.routes.update(dest, agent_id(2), 1);

        let mut msg = EmbeddedMessage::new(agent_id(0), dest, 1);
        msg.ttl = 5;
        agent.receive(msg);
        assert_eq!(agent.process_next(), ProcessResult::Forwarded);
        assert_eq!(agent.stats().forwarded, 1);

        let fwd = agent.drain_outbox().unwrap();
        assert_eq!(fwd.ttl, 4); // decremented
    }

    #[test]
    fn test_agent_drop_no_route() {
        let me = agent_id(1);
        let mut agent: EmbeddedAgent<8, 8, 4> = EmbeddedAgent::new(me, 100);
        agent.watchdog.start();

        let msg = EmbeddedMessage::new(agent_id(0), agent_id(99), 1);
        agent.receive(msg);
        assert_eq!(agent.process_next(), ProcessResult::Dropped);
        assert_eq!(agent.stats().dropped, 1);
    }

    #[test]
    fn test_agent_drop_ttl_zero() {
        let me = agent_id(1);
        let dest = agent_id(3);
        let mut agent: EmbeddedAgent<8, 8, 4> = EmbeddedAgent::new(me, 100);
        agent.watchdog.start();
        agent.routes.update(dest, agent_id(2), 1);

        let mut msg = EmbeddedMessage::new(agent_id(0), dest, 1);
        msg.ttl = 0;
        agent.receive(msg);
        assert_eq!(agent.process_next(), ProcessResult::Dropped);
    }

    #[test]
    fn test_agent_send() {
        let me = agent_id(1);
        let mut agent: EmbeddedAgent<8, 8, 4> = EmbeddedAgent::new(me, 100);
        let msg = EmbeddedMessage::new(agent_id(0), agent_id(2), 1);
        assert!(agent.send(msg));
        let out = agent.drain_outbox().unwrap();
        assert_eq!(out.src.as_bytes(), me.as_bytes()); // src overwritten
    }

    #[test]
    fn test_agent_idle() {
        let me = agent_id(1);
        let mut agent: EmbeddedAgent<8, 8, 4> = EmbeddedAgent::new(me, 100);
        agent.watchdog.start();
        assert_eq!(agent.process_next(), ProcessResult::Idle);
    }

    #[test]
    fn test_agent_inbox_full() {
        let me = agent_id(1);
        let mut agent: EmbeddedAgent<2, 2, 4> = EmbeddedAgent::new(me, 100);
        let msg = EmbeddedMessage::new(agent_id(0), me, 1);
        agent.receive(msg);
        let msg2 = EmbeddedMessage::new(agent_id(0), me, 2);
        agent.receive(msg2);
        // inbox is full
        let msg3 = EmbeddedMessage::new(agent_id(0), me, 3);
        assert_eq!(agent.receive(msg3), ProcessResult::Dropped);
    }

    #[test]
    fn test_agent_watchdog_fires() {
        let me = agent_id(1);
        let mut agent: EmbeddedAgent<8, 8, 4> = EmbeddedAgent::new(me, 3);
        agent.watchdog.start();
        // No messages, watchdog ticks 3 times
        agent.process_next(); // tick 1
        agent.process_next(); // tick 2
        assert_eq!(agent.process_next(), ProcessResult::WatchdogFired); // tick 3
    }
}
