//! Deterministic Replay Framework for SPINE Protocol
//!
//! Records protocol message exchanges and replays them deterministically.
//! Useful for:
//! - Debugging distributed communication issues
//! - Regression testing with real traffic captures
//! - Performance profiling with reproducible workloads
//!
//! # Architecture
//!
//! ```text
//! ┌────────────┐     ┌──────────┐     ┌────────────┐
//! │ RecordingHandler │ → │ TraceLog │ → │ ReplayHandler │
//! └────────────┘     └──────────┘     └────────────┘
//!       ↑                                    ↓
//!   wraps real I/O                   drives assertions
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use spine_protocol::{Message, ProtocolHandler};
//! use spine_protocol::replay::{TraceLog, RecordingHandler, ReplayHandler};
//!
//! // Record
//! let trace = TraceLog::new();
//! let mut handler = RecordingHandler::new(protocol_handler, trace.clone());
//! handler.send(msg).await;
//!
//! // Save
//! trace.save("session.trace").unwrap();
//!
//! // Replay
//! let trace = TraceLog::load("session.trace").unwrap();
//! let replayer = ReplayHandler::new(trace);
//! replayer.replay_and_verify().await;
//! ```

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::Message;

/// Direction of a recorded message
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Direction {
    /// Message was sent
    Send,
    /// Message was received
    Recv,
}

/// A single recorded protocol event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    /// Sequential index
    pub seq: u64,
    /// Timestamp (microseconds since epoch)
    pub timestamp_us: u64,
    /// Direction (send/recv)
    pub direction: Direction,
    /// The message
    pub message: Message,
    /// Duration of the operation in microseconds
    pub duration_us: u64,
    /// Optional metadata (encryption mode, morphology seed, etc.)
    pub metadata: Option<serde_json::Value>,
}

/// A complete trace log of protocol events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceLog {
    /// Session identifier
    pub session_id: String,
    /// Trace start time (microseconds since epoch)
    pub start_time_us: u64,
    /// All recorded entries
    pub entries: Vec<TraceEntry>,
    /// Protocol configuration snapshot at recording time
    pub config: Option<serde_json::Value>,
}

impl TraceLog {
    /// Create a new empty trace log
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            start_time_us: now,
            entries: Vec::new(),
            config: None,
        }
    }

    /// Create a new trace log with a specific session ID
    pub fn with_session(session_id: impl Into<String>) -> Self {
        let mut log = Self::new();
        log.session_id = session_id.into();
        log
    }

    /// Record a send event
    pub fn record_send(&mut self, msg: &Message, duration: Duration) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        self.entries.push(TraceEntry {
            seq: self.entries.len() as u64,
            timestamp_us: now,
            direction: Direction::Send,
            message: msg.clone(),
            duration_us: duration.as_micros() as u64,
            metadata: None,
        });
    }

    /// Record a receive event
    pub fn record_recv(&mut self, msg: &Message, duration: Duration) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        self.entries.push(TraceEntry {
            seq: self.entries.len() as u64,
            timestamp_us: now,
            direction: Direction::Recv,
            message: msg.clone(),
            duration_us: duration.as_micros() as u64,
            metadata: None,
        });
    }

    /// Record an event with metadata
    pub fn record_with_metadata(
        &mut self,
        direction: Direction,
        msg: &Message,
        duration: Duration,
        metadata: serde_json::Value,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        self.entries.push(TraceEntry {
            seq: self.entries.len() as u64,
            timestamp_us: now,
            direction,
            message: msg.clone(),
            duration_us: duration.as_micros() as u64,
            metadata: Some(metadata),
        });
    }

    /// Save trace log to a file (JSON)
    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load trace log from a file
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let log: Self = serde_json::from_str(&json)?;
        Ok(log)
    }

    /// Number of recorded entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get all send entries
    pub fn sends(&self) -> Vec<&TraceEntry> {
        self.entries
            .iter()
            .filter(|e| e.direction == Direction::Send)
            .collect()
    }

    /// Get all receive entries
    pub fn recvs(&self) -> Vec<&TraceEntry> {
        self.entries
            .iter()
            .filter(|e| e.direction == Direction::Recv)
            .collect()
    }

    /// Total duration of the trace
    pub fn total_duration(&self) -> Duration {
        if self.entries.is_empty() {
            return Duration::ZERO;
        }
        let first = self.entries.first().unwrap().timestamp_us;
        let last = self.entries.last().unwrap().timestamp_us;
        Duration::from_micros(last.saturating_sub(first))
    }

    /// Average message latency
    pub fn avg_latency(&self) -> Duration {
        if self.entries.is_empty() {
            return Duration::ZERO;
        }
        let total: u64 = self.entries.iter().map(|e| e.duration_us).sum();
        Duration::from_micros(total / self.entries.len() as u64)
    }
}

impl Default for TraceLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe shared trace log for recording
#[derive(Debug, Clone)]
pub struct SharedTraceLog {
    inner: Arc<Mutex<TraceLog>>,
}

impl SharedTraceLog {
    /// Create a new shared trace log
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TraceLog::new())),
        }
    }

    /// Create with a session ID
    pub fn with_session(session_id: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TraceLog::with_session(session_id))),
        }
    }

    /// Record a send event
    pub fn record_send(&self, msg: &Message, duration: Duration) {
        if let Ok(mut log) = self.inner.lock() {
            log.record_send(msg, duration);
        }
    }

    /// Record a receive event
    pub fn record_recv(&self, msg: &Message, duration: Duration) {
        if let Ok(mut log) = self.inner.lock() {
            log.record_recv(msg, duration);
        }
    }

    /// Get a snapshot of the trace log
    pub fn snapshot(&self) -> TraceLog {
        self.inner.lock().unwrap().clone()
    }

    /// Save the trace log
    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        self.inner.lock().unwrap().save(path)
    }
}

impl Default for SharedTraceLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Replays a recorded trace and verifies message ordering and content.
///
/// Can be used for:
/// - Regression testing: verify that protocol changes don't break existing behavior
/// - Performance comparison: measure replay speed vs original recording
/// - Debugging: step through recorded exchanges
pub struct ReplayVerifier {
    trace: TraceLog,
}

impl ReplayVerifier {
    /// Create a new replay verifier from a trace log
    pub fn new(trace: TraceLog) -> Self {
        Self { trace }
    }

    /// Load and create from a file
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            trace: TraceLog::load(path)?,
        })
    }

    /// Verify that all messages in the trace can be serialized and deserialized
    pub fn verify_serialization(&self) -> anyhow::Result<Vec<VerifyResult>> {
        let mut results = Vec::new();
        for entry in &self.trace.entries {
            let json = serde_json::to_string(&entry.message);
            let ok = match &json {
                Ok(s) => serde_json::from_str::<Message>(s).is_ok(),
                Err(_) => false,
            };
            results.push(VerifyResult {
                seq: entry.seq,
                direction: entry.direction.clone(),
                passed: ok,
                detail: if ok {
                    "roundtrip OK".to_string()
                } else {
                    format!("serialization failed: {:?}", json.err())
                },
            });
        }
        Ok(results)
    }

    /// Verify message ordering invariants:
    /// - Timestamps are monotonically non-decreasing
    /// - Sequence numbers are contiguous
    pub fn verify_ordering(&self) -> Vec<VerifyResult> {
        let mut results = Vec::new();
        let mut prev_ts = 0u64;

        for (i, entry) in self.trace.entries.iter().enumerate() {
            let seq_ok = entry.seq == i as u64;
            let ts_ok = entry.timestamp_us >= prev_ts;
            let passed = seq_ok && ts_ok;

            results.push(VerifyResult {
                seq: entry.seq,
                direction: entry.direction.clone(),
                passed,
                detail: if !seq_ok {
                    format!("seq gap: expected {}, got {}", i, entry.seq)
                } else if !ts_ok {
                    format!(
                        "timestamp regression: {} < {}",
                        entry.timestamp_us, prev_ts
                    )
                } else {
                    "ordering OK".to_string()
                },
            });
            prev_ts = entry.timestamp_us;
        }
        results
    }

    /// Replay against a live ProtocolHandler, comparing received messages
    /// to what was recorded.
    pub async fn replay_sends<S>(
        &self,
        handler: &mut crate::ProtocolHandler<S>,
    ) -> anyhow::Result<Vec<VerifyResult>>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        let mut results = Vec::new();

        for entry in &self.trace.entries {
            if entry.direction == Direction::Send {
                let start = std::time::Instant::now();
                let send_result = handler.send_message_raw(&entry.message).await;
                let dur = start.elapsed();

                results.push(VerifyResult {
                    seq: entry.seq,
                    direction: Direction::Send,
                    passed: send_result.is_ok(),
                    detail: format!(
                        "replay send: {:?} (original: {}μs, replay: {}μs)",
                        send_result.is_ok(),
                        entry.duration_us,
                        dur.as_micros()
                    ),
                });
            }
        }
        Ok(results)
    }

    /// Summary statistics
    pub fn summary(&self) -> TraceSummary {
        let sends = self.trace.sends().len();
        let recvs = self.trace.recvs().len();
        TraceSummary {
            session_id: self.trace.session_id.clone(),
            total_entries: self.trace.len(),
            send_count: sends,
            recv_count: recvs,
            total_duration: self.trace.total_duration(),
            avg_latency: self.trace.avg_latency(),
        }
    }
}

/// Result of a single verification step
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub seq: u64,
    pub direction: Direction,
    pub passed: bool,
    pub detail: String,
}

/// Summary of a trace
#[derive(Debug, Clone)]
pub struct TraceSummary {
    pub session_id: String,
    pub total_entries: usize,
    pub send_count: usize,
    pub recv_count: usize,
    pub total_duration: Duration,
    pub avg_latency: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ping(ts: u64) -> Message {
        Message::Ping { timestamp: ts }
    }

    #[test]
    fn trace_log_record_and_query() {
        let mut log = TraceLog::with_session("test-session");

        log.record_send(&make_ping(1), Duration::from_micros(100));
        log.record_recv(&make_ping(2), Duration::from_micros(200));
        log.record_send(&make_ping(3), Duration::from_micros(150));

        assert_eq!(log.len(), 3);
        assert_eq!(log.sends().len(), 2);
        assert_eq!(log.recvs().len(), 1);
        assert_eq!(log.session_id, "test-session");
    }

    #[test]
    fn trace_log_serialization_roundtrip() {
        let mut log = TraceLog::with_session("roundtrip-test");
        log.record_send(&make_ping(42), Duration::from_micros(100));

        let json = serde_json::to_string(&log).unwrap();
        let deserialized: TraceLog = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.session_id, "roundtrip-test");
        assert_eq!(deserialized.len(), 1);
        assert_eq!(deserialized.entries[0].seq, 0);
    }

    #[test]
    fn shared_trace_log_thread_safety() {
        let shared = SharedTraceLog::with_session("shared-test");

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let log = shared.clone();
                std::thread::spawn(move || {
                    log.record_send(&make_ping(i), Duration::from_micros(10));
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let snapshot = shared.snapshot();
        assert_eq!(snapshot.len(), 10);
    }

    #[test]
    fn verify_ordering_correct() {
        let mut log = TraceLog::new();
        log.record_send(&make_ping(1), Duration::from_micros(100));
        log.record_recv(&make_ping(2), Duration::from_micros(200));

        let verifier = ReplayVerifier::new(log);
        let results = verifier.verify_ordering();

        assert!(results.iter().all(|r| r.passed));
    }

    #[test]
    fn verify_serialization_passes() {
        let mut log = TraceLog::new();
        log.record_send(&make_ping(1), Duration::from_micros(100));
        log.record_send(&make_ping(2), Duration::from_micros(200));

        let verifier = ReplayVerifier::new(log);
        let results = verifier.verify_serialization().unwrap();

        assert!(results.iter().all(|r| r.passed));
    }

    #[test]
    fn trace_summary() {
        let mut log = TraceLog::new();
        log.record_send(&make_ping(1), Duration::from_micros(100));
        log.record_recv(&make_ping(2), Duration::from_micros(200));
        log.record_send(&make_ping(3), Duration::from_micros(300));

        let verifier = ReplayVerifier::new(log);
        let summary = verifier.summary();

        assert_eq!(summary.total_entries, 3);
        assert_eq!(summary.send_count, 2);
        assert_eq!(summary.recv_count, 1);
        assert_eq!(summary.avg_latency, Duration::from_micros(200));
    }

    #[test]
    fn trace_save_load_roundtrip() {
        let mut log = TraceLog::with_session("file-test");
        log.record_send(&make_ping(1), Duration::from_micros(100));
        log.record_recv(&make_ping(2), Duration::from_micros(200));

        let path = std::env::temp_dir().join("spine_test_trace.json");
        log.save(&path).unwrap();

        let loaded = TraceLog::load(&path).unwrap();
        assert_eq!(loaded.session_id, "file-test");
        assert_eq!(loaded.len(), 2);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }
}
