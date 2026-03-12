// =============================================================================
// REPLAY: Agent-Level Deterministic Replay Debugger
// =============================================================================
//
// Records agent decisions, state transitions, and message exchanges for
// deterministic re-execution. Enables post-mortem debugging by replaying
// the exact sequence of actions an agent took.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

use crate::AgentId;

/// A recorded agent action for replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEntry {
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    pub agent_id: AgentId,
    pub kind: ReplayEntryKind,
    pub state_hash: [u8; 32],
}

/// What happened in this replay entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplayEntryKind {
    /// Agent received a message.
    MessageReceived {
        from: AgentId,
        payload: String,
    },
    /// Agent sent a message.
    MessageSent {
        to: AgentId,
        payload: String,
    },
    /// Agent made a decision.
    Decision {
        input: String,
        output: String,
        rationale: Option<String>,
    },
    /// Agent state transition.
    StateTransition {
        from_state: String,
        to_state: String,
    },
    /// Agent performed a task.
    TaskAction {
        task_id: Uuid,
        action: String,
        result: Option<String>,
    },
    /// External event observed.
    ExternalEvent {
        source: String,
        event: String,
    },
    /// Error encountered.
    Error {
        message: String,
        recoverable: bool,
    },
}

/// Complete recording of an agent's execution for replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayLog {
    pub agent_id: AgentId,
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub entries: Vec<ReplayEntry>,
    /// Snapshot of initial agent state for deterministic replay.
    pub initial_state: HashMap<String, String>,
    /// SHA-256 of the entire log for integrity.
    pub integrity_hash: Option<[u8; 32]>,
}

impl ReplayLog {
    pub fn new(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            session_id: Uuid::new_v4().to_string(),
            started_at: Utc::now(),
            ended_at: None,
            entries: Vec::new(),
            initial_state: HashMap::new(),
            integrity_hash: None,
        }
    }

    /// Set the initial agent state snapshot.
    pub fn set_initial_state(&mut self, state: HashMap<String, String>) {
        self.initial_state = state;
    }

    /// Record an entry.
    pub fn record(&mut self, kind: ReplayEntryKind, state_hash: [u8; 32]) {
        self.entries.push(ReplayEntry {
            seq: self.entries.len() as u64,
            timestamp: Utc::now(),
            agent_id: self.agent_id,
            kind,
            state_hash,
        });
    }

    /// Finalize the log and compute integrity hash.
    pub fn finalize(&mut self) {
        self.ended_at = Some(Utc::now());
        self.integrity_hash = Some(self.compute_hash());
    }

    /// Verify integrity of the log.
    pub fn verify_integrity(&self) -> bool {
        self.integrity_hash
            .map(|h| h == self.compute_hash())
            .unwrap_or(false)
    }

    /// Save to file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Load from file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Count entries by kind.
    pub fn entry_counts(&self) -> HashMap<&'static str, usize> {
        let mut counts = HashMap::new();
        for entry in &self.entries {
            let key = match &entry.kind {
                ReplayEntryKind::MessageReceived { .. } => "message_received",
                ReplayEntryKind::MessageSent { .. } => "message_sent",
                ReplayEntryKind::Decision { .. } => "decision",
                ReplayEntryKind::StateTransition { .. } => "state_transition",
                ReplayEntryKind::TaskAction { .. } => "task_action",
                ReplayEntryKind::ExternalEvent { .. } => "external_event",
                ReplayEntryKind::Error { .. } => "error",
            };
            *counts.entry(key).or_default() += 1;
        }
        counts
    }

    /// Total duration of the recording.
    pub fn duration_ms(&self) -> Option<i64> {
        self.ended_at
            .map(|end| (end - self.started_at).num_milliseconds())
    }

    fn compute_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.agent_id.0.as_bytes());
        hasher.update(self.session_id.as_bytes());
        for entry in &self.entries {
            hasher.update(entry.seq.to_le_bytes());
            hasher.update(entry.state_hash);
        }
        hasher.finalize().into()
    }
}

/// Callback type for state verification during replay.
type StateVerifier = Box<dyn Fn(u64, &[u8; 32]) -> bool + Send>;

/// Replays a recorded log and verifies state consistency.
pub struct ReplayDebugger {
    log: ReplayLog,
    current_seq: u64,
    breakpoints: Vec<u64>,
    state_verifier: Option<StateVerifier>,
}

impl ReplayDebugger {
    pub fn new(log: ReplayLog) -> Self {
        Self {
            log,
            current_seq: 0,
            breakpoints: Vec::new(),
            state_verifier: None,
        }
    }

    /// Load from file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let log = ReplayLog::load(path)?;
        Ok(Self::new(log))
    }

    /// Set a breakpoint at a sequence number.
    pub fn add_breakpoint(&mut self, seq: u64) {
        if !self.breakpoints.contains(&seq) {
            self.breakpoints.push(seq);
            self.breakpoints.sort();
        }
    }

    /// Remove a breakpoint.
    pub fn remove_breakpoint(&mut self, seq: u64) {
        self.breakpoints.retain(|&s| s != seq);
    }

    /// Set a state verification callback.
    pub fn set_verifier(&mut self, f: impl Fn(u64, &[u8; 32]) -> bool + Send + 'static) {
        self.state_verifier = Some(Box::new(f));
    }

    /// Step forward one entry.
    pub fn step(&mut self) -> Option<ReplayStepResult> {
        let entry = self.log.entries.get(self.current_seq as usize)?;

        let state_valid = self
            .state_verifier
            .as_ref()
            .map(|v| v(entry.seq, &entry.state_hash))
            .unwrap_or(true);

        let hit_breakpoint = self.breakpoints.contains(&self.current_seq);

        self.current_seq += 1;

        Some(ReplayStepResult {
            entry: entry.clone(),
            state_valid,
            hit_breakpoint,
        })
    }

    /// Run to the next breakpoint or end.
    pub fn continue_to_breakpoint(&mut self) -> Vec<ReplayStepResult> {
        let mut results = Vec::new();
        while let Some(result) = self.step() {
            let bp = result.hit_breakpoint;
            results.push(result);
            if bp {
                break;
            }
        }
        results
    }

    /// Run to completion, collecting all results.
    pub fn run_to_end(&mut self) -> Vec<ReplayStepResult> {
        let mut results = Vec::new();
        while let Some(result) = self.step() {
            results.push(result);
        }
        results
    }

    /// Find divergence point: where state hashes stop matching a reference log.
    pub fn find_divergence(&self, reference: &ReplayLog) -> Option<u64> {
        for (i, (a, b)) in self
            .log
            .entries
            .iter()
            .zip(reference.entries.iter())
            .enumerate()
        {
            if a.state_hash != b.state_hash {
                return Some(i as u64);
            }
        }
        if self.log.entries.len() != reference.entries.len() {
            return Some(self.log.entries.len().min(reference.entries.len()) as u64);
        }
        None
    }

    /// Get the current position.
    pub fn position(&self) -> u64 {
        self.current_seq
    }

    /// Total entries in the log.
    pub fn total_entries(&self) -> usize {
        self.log.entries.len()
    }

    /// Reset to beginning.
    pub fn reset(&mut self) {
        self.current_seq = 0;
    }

    /// Get the underlying log.
    pub fn log(&self) -> &ReplayLog {
        &self.log
    }
}

/// Result of stepping through one replay entry.
#[derive(Debug, Clone)]
pub struct ReplayStepResult {
    pub entry: ReplayEntry,
    pub state_valid: bool,
    pub hit_breakpoint: bool,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn state_hash(val: u8) -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0] = val;
        h
    }

    #[test]
    fn test_replay_log_record() {
        let agent = AgentId::new();
        let mut log = ReplayLog::new(agent);

        log.record(
            ReplayEntryKind::Decision {
                input: "query".into(),
                output: "result".into(),
                rationale: Some("fast path".into()),
            },
            state_hash(1),
        );
        log.record(
            ReplayEntryKind::MessageSent {
                to: AgentId::new(),
                payload: "hello".into(),
            },
            state_hash(2),
        );

        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.entries[0].seq, 0);
        assert_eq!(log.entries[1].seq, 1);
    }

    #[test]
    fn test_replay_log_integrity() {
        let mut log = ReplayLog::new(AgentId::new());
        log.record(
            ReplayEntryKind::StateTransition {
                from_state: "idle".into(),
                to_state: "active".into(),
            },
            state_hash(1),
        );
        log.finalize();

        assert!(log.verify_integrity());

        // Tamper with the log
        log.entries[0].state_hash[0] = 99;
        assert!(!log.verify_integrity());
    }

    #[test]
    fn test_replay_log_save_load() {
        let mut log = ReplayLog::new(AgentId::new());
        log.record(
            ReplayEntryKind::ExternalEvent {
                source: "web".into(),
                event: "page_loaded".into(),
            },
            state_hash(1),
        );
        log.finalize();

        let path = std::env::temp_dir().join("spine_replay_test.json");
        log.save(&path).unwrap();

        let loaded = ReplayLog::load(&path).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert!(loaded.verify_integrity());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_entry_counts() {
        let mut log = ReplayLog::new(AgentId::new());
        log.record(ReplayEntryKind::Decision { input: "a".into(), output: "b".into(), rationale: None }, state_hash(1));
        log.record(ReplayEntryKind::Decision { input: "c".into(), output: "d".into(), rationale: None }, state_hash(2));
        log.record(ReplayEntryKind::Error { message: "fail".into(), recoverable: true }, state_hash(3));

        let counts = log.entry_counts();
        assert_eq!(counts.get("decision"), Some(&2));
        assert_eq!(counts.get("error"), Some(&1));
    }

    #[test]
    fn test_debugger_step() {
        let mut log = ReplayLog::new(AgentId::new());
        log.record(ReplayEntryKind::StateTransition { from_state: "a".into(), to_state: "b".into() }, state_hash(1));
        log.record(ReplayEntryKind::StateTransition { from_state: "b".into(), to_state: "c".into() }, state_hash(2));
        log.finalize();

        let mut dbg = ReplayDebugger::new(log);
        let r1 = dbg.step().unwrap();
        assert_eq!(r1.entry.seq, 0);
        assert!(r1.state_valid);

        let r2 = dbg.step().unwrap();
        assert_eq!(r2.entry.seq, 1);

        assert!(dbg.step().is_none());
    }

    #[test]
    fn test_debugger_breakpoints() {
        let mut log = ReplayLog::new(AgentId::new());
        for i in 0..5 {
            log.record(ReplayEntryKind::Decision { input: format!("q{i}"), output: format!("r{i}"), rationale: None }, state_hash(i));
        }
        log.finalize();

        let mut dbg = ReplayDebugger::new(log);
        dbg.add_breakpoint(2);

        let results = dbg.continue_to_breakpoint();
        assert_eq!(results.len(), 3); // entries 0, 1, 2
        assert!(results.last().unwrap().hit_breakpoint);
        assert_eq!(dbg.position(), 3);
    }

    #[test]
    fn test_debugger_find_divergence() {
        let agent = AgentId::new();
        let mut log1 = ReplayLog::new(agent);
        let mut log2 = ReplayLog::new(agent);

        for i in 0..5 {
            log1.record(ReplayEntryKind::Decision { input: "q".into(), output: "r".into(), rationale: None }, state_hash(i));
            if i < 3 {
                log2.record(ReplayEntryKind::Decision { input: "q".into(), output: "r".into(), rationale: None }, state_hash(i));
            } else {
                log2.record(ReplayEntryKind::Decision { input: "q".into(), output: "r".into(), rationale: None }, state_hash(i + 10));
            }
        }

        let dbg = ReplayDebugger::new(log1);
        assert_eq!(dbg.find_divergence(&log2), Some(3));
    }

    #[test]
    fn test_debugger_reset() {
        let mut log = ReplayLog::new(AgentId::new());
        log.record(ReplayEntryKind::Decision { input: "a".into(), output: "b".into(), rationale: None }, state_hash(1));
        log.finalize();

        let mut dbg = ReplayDebugger::new(log);
        dbg.step();
        assert_eq!(dbg.position(), 1);
        dbg.reset();
        assert_eq!(dbg.position(), 0);
    }

    #[test]
    fn test_state_verifier_callback() {
        let mut log = ReplayLog::new(AgentId::new());
        log.record(ReplayEntryKind::Decision { input: "a".into(), output: "b".into(), rationale: None }, state_hash(1));
        log.record(ReplayEntryKind::Decision { input: "c".into(), output: "d".into(), rationale: None }, state_hash(99));
        log.finalize();

        let mut dbg = ReplayDebugger::new(log);
        dbg.set_verifier(|_seq, hash| hash[0] != 99); // reject entry with hash[0]=99

        let results = dbg.run_to_end();
        assert!(results[0].state_valid);
        assert!(!results[1].state_valid);
    }
}
